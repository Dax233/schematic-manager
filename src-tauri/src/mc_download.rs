//! 从官方 API 或 BMCL 镜像下载 Minecraft 客户端 jar 与语言文件。
//!
//! 官方接口分两步：先读版本清单拿到该版本 version.json 的地址，
//! 再从 version.json 里取客户端 jar 的下载地址。BMCL 镜像的客户端
//! 地址可以直接拼出来，不用发请求。
//! 清单/版本 JSON 的解析与「从哪个地址下载」是纯函数，便于离线测试；
//! 真正发请求的只有 [`fetch_manifest`]、[`fetch_text`]、[`download_to`]
//! 与 [`download_object`]。
//!
//! ## 为什么还要下语言文件
//!
//! 客户端 jar 里只有 `en_us.json`，简体中文随**资源索引**单独分发，正常由
//! 启动器下到 `.minecraft/assets/objects` 里。用户的机器上未必有（只装了
//! 别的版本、清理过资源、或者用的是第三方启动器），那时方块名整列没有中文。
//! 因此建库时缺中文就自己去下：资源对象按内容的 sha1 寻址，地址可以直接
//! 拼出来，不需要先读索引；下到的文件同时缓存进应用数据目录，之后重建
//! 别的版本可以直接复用，不再联网。

use crate::error::{AppError, AppResult};
use std::io::Read;
use std::path::Path;
use std::time::Duration;

/// 官方版本清单（manifest v2）地址
pub const OFFICIAL_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// BMCL 镜像的客户端 jar 地址前缀（`{前缀}/{版本 id}/client`）
pub const BMCL_CLIENT_BASE: &str = "https://bmclapi2.bangbang93.com/version";

/// 官方资源对象地址前缀（`{前缀}/{hash 前两位}/{hash}`）
pub const OFFICIAL_OBJECT_BASE: &str = "https://resources.download.minecraft.net";

/// BMCL 镜像的资源对象地址前缀（按版本下载时用 `{前缀}/{版本}/objects/...`，
/// 这里用不区分版本的通用入口，避免为中文名再区分一次来源）
pub const BMCL_OBJECT_BASE: &str = "https://bmclapi2.bangbang93.com/assets";

/// 请求超时要给足：客户端 jar 有 20~30 MB，镜像站在高峰时段并不快。
/// 没有这个上限时，一个挂起的连接会让「版本库」界面永远停在转圈。
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(300))
        .build()
}

/// 把网络错误翻译成带上下文的 AppError。
///
/// ureq 的错误文本本身不含 URL，直接向上抛会让用户分不清
/// 是清单没拉到还是 jar 没下到。
fn http_err(url: &str, e: ureq::Error) -> AppError {
    AppError::Parse(format!("下载失败 {url}：{e}"))
}

/// 拉取官方版本清单全文（几 MB 的 JSON，直接按文本处理）
pub fn fetch_manifest() -> AppResult<String> {
    let resp = agent()
        .get(OFFICIAL_MANIFEST_URL)
        .call()
        .map_err(|e| http_err(OFFICIAL_MANIFEST_URL, e))?;
    resp.into_string()
        .map_err(|e| AppError::Parse(format!("读取版本清单失败：{e}")))
}

/// 从官方版本清单里找某个版本的 version.json 地址。
///
/// 清单里没有该版本时报错而不是返回空：那意味着这个 id 根本不存在
/// （或不是正式版），静默跳过会让用户以为下载成功了。
pub fn version_json_url(manifest: &str, id: &str) -> AppResult<String> {
    let doc: serde_json::Value =
        serde_json::from_str(manifest).map_err(|e| AppError::Parse(format!("版本清单解析失败：{e}")))?;
    let versions = doc
        .get("versions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AppError::Parse("版本清单缺少 versions 数组".into()))?;
    for v in versions {
        if v.get("id").and_then(|x| x.as_str()) == Some(id) {
            return v
                .get("url")
                .and_then(|x| x.as_str())
                .map(str::to_owned)
                .ok_or_else(|| AppError::Parse(format!("版本 {id} 的条目缺少 url")));
        }
    }
    Err(AppError::Parse(format!(
        "官方版本清单里没有 {id}（可能不是正式版）"
    )))
}

/// 从 version.json 里取客户端 jar 的下载地址（downloads.client.url）
pub fn client_url_from_version_json(json: &str) -> AppResult<String> {
    let doc: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| AppError::Parse(format!("version.json 解析失败：{e}")))?;
    doc.get("downloads")
        .and_then(|d| d.get("client"))
        .and_then(|c| c.get("url"))
        .and_then(|u| u.as_str())
        .map(str::to_owned)
        .ok_or_else(|| AppError::Parse("version.json 里没有 downloads.client.url".into()))
}

/// 官方接口里某个版本客户端 jar 的下载地址：清单 → version.json → client.url
pub fn official_client_url(manifest: &str, id: &str) -> AppResult<String> {
    let json_url = version_json_url(manifest, id)?;
    let resp = agent()
        .get(&json_url)
        .call()
        .map_err(|e| http_err(&json_url, e))?;
    let body = resp
        .into_string()
        .map_err(|e| AppError::Parse(format!("读取 version.json 失败：{e}")))?;
    client_url_from_version_json(&body)
}

/// BMCL 镜像的客户端 jar 地址，直接按 id 拼接，不需要请求
pub fn bmcl_client_url(id: &str) -> String {
    format!("{BMCL_CLIENT_BASE}/{id}/client")
}

/// 把 URL 的内容下载到本地文件，过程中通过回调汇报进度。
///
/// `on_progress(已收字节, 总字节)`：总字节为 0 表示服务器没给长度。
/// 落盘细节见 [`write_body`]。
pub fn download_to(
    url: &str,
    dest: &Path,
    on_progress: &mut dyn FnMut(u64, u64),
) -> AppResult<()> {
    let resp = agent()
        .get(url)
        .call()
        .map_err(|e| http_err(url, e))?;
    let total: u64 = resp
        .header("Content-Length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut reader = resp.into_reader();
    write_body(&mut reader, dest, total, on_progress)
}

/// 拉取一份文本资源（版本 JSON、资源索引等，都是几十 KB 到几 MB 的 JSON）
pub fn fetch_text(url: &str) -> AppResult<String> {
    let resp = agent()
        .get(url)
        .call()
        .map_err(|e| http_err(url, e))?;
    resp.into_string()
        .map_err(|e| AppError::Parse(format!("读取 {url} 失败：{e}")))
}

/// 从版本 JSON 里取资源索引（assetIndex）的地址。
///
/// 索引里记着每个语言文件的 sha1，中文名要靠它才能定位。官方版本 JSON 直接
/// 给出索引的下载地址，照用即可，不必自己拼索引文件名。
pub fn asset_index_url_from_version_json(json: &str) -> AppResult<String> {
    let doc: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| AppError::Parse(format!("版本 JSON 解析失败：{e}")))?;
    doc.get("assetIndex")
        .and_then(|a| a.get("url"))
        .and_then(|u| u.as_str())
        .map(str::to_owned)
        .ok_or_else(|| AppError::Parse("版本 JSON 里没有 assetIndex.url".into()))
}

/// 从资源索引里取某个语言文件的 sha1。
///
/// 索引的键形如 `minecraft/lang/zh_cn.json`。取值可能与索引里的路径写法
/// 不完全一致（少数版本带别的前缀），因此按结尾匹配。
pub fn lang_hash_from_index(index_json: &str, lang: &str) -> Option<String> {
    let doc: serde_json::Value = serde_json::from_str(index_json).ok()?;
    let objects = doc.get("objects")?.as_object()?;
    let suffix = format!("lang/{lang}.json");
    objects
        .iter()
        .find(|(path, _)| path.ends_with(&suffix))
        .and_then(|(_, v)| v.get("hash"))
        .and_then(|h| h.as_str())
        .map(str::to_owned)
}

/// 资源对象的候选下载地址：官方在前，镜像在后。
///
/// 资源对象按内容的 sha1 寻址（`<前两位>/<后 38 位>`），两个源的文件完全相同，
/// 因此可以先试官方、失败了再试镜像，不必为镜像单独查一次索引。
///
/// 哈希不是 40 位 sha1 时返回空表：长度不足还去拼地址只会得到 404，
/// 让调用方直接报「哈希不合法」比让它去试一个必然失败的地址更清楚。
pub fn object_url_candidates(hash: &str) -> Vec<String> {
    if hash.len() != 40 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Vec::new();
    }
    let (a, b) = hash.split_at(2);
    vec![
        format!("{OFFICIAL_OBJECT_BASE}/{a}/{b}"),
        format!("{BMCL_OBJECT_BASE}/{a}/{b}"),
    ]
}

/// 把 URL 的内容整体读进内存。只用于几 MB 以内的资源（语言文件才几十 KB）
pub fn fetch_bytes(url: &str) -> AppResult<Vec<u8>> {
    let resp = agent()
        .get(url)
        .call()
        .map_err(|e| http_err(url, e))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| AppError::Parse(format!("读取 {url} 失败：{e}")))?;
    Ok(buf)
}

/// 把响应正文写进目标文件：先写 `.tmp`，读完才改名。
///
/// 读到一半连接断掉时 ureq 会以「正文提前结束」报错，此时必须清掉
/// 半截文件：留在目标路径上会被后续流程当成「已下载」直接复用，
/// 于是每次构建都失败，而用户看不出是下载坏了。
///
/// 正文来源做成 `Read` 参数而不是在这里发请求：落盘行为（改名、失败
/// 清理）是这段代码的实质，可以脱离网络直接验证。
fn write_body(
    reader: &mut dyn Read,
    dest: &Path,
    total: u64,
    on_progress: &mut dyn FnMut(u64, u64),
) -> AppResult<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = dest.to_path_buf();
    tmp.as_mut_os_string().push(".tmp");

    let mut read_all = |tmp: &Path| -> AppResult<()> {
        let mut out = std::fs::File::create(tmp)?;
        let mut buf = [0u8; 64 * 1024];
        let mut received: u64 = 0;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            std::io::Write::write_all(&mut out, &buf[..n])?;
            received += n as u64;
            on_progress(received, total);
        }
        Ok(())
    };

    if let Err(e) = read_all(&tmp) {
        // 清掉半截文件；删不掉也不掩盖原始错误，原样把它报上去
        if let Err(rm) = std::fs::remove_file(&tmp) {
            eprintln!("清理未完成的下载 {tmp:?} 失败：{rm}");
        }
        return Err(e);
    }
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn version_json_url_finds_the_requested_version() {
        let manifest = r#"{
            "latest": {"release": "1.20.4", "snapshot": "24w01a"},
            "versions": [
                {"id": "1.20.4", "type": "release", "url": "https://x/v1204.json"},
                {"id": "1.20.3", "type": "release", "url": "https://x/v1203.json"}
            ]
        }"#;
        assert_eq!(
            version_json_url(manifest, "1.20.3").expect("应找到"),
            "https://x/v1203.json"
        );
    }

    #[test]
    fn version_json_url_rejects_unknown_id() {
        let manifest = r#"{"versions": [{"id": "1.20.3", "url": "https://x/v.json"}]}"#;
        let e = version_json_url(manifest, "9.9.9").unwrap_err().to_string();
        assert!(e.contains("9.9.9"), "错误应指出是哪个 id 没找到：{e}");
    }

    #[test]
    fn client_url_comes_from_downloads_section() {
        let json = r#"{
            "id": "1.20.4",
            "downloads": {
                "client": {"url": "https://x/client.jar", "size": 100},
                "server": {"url": "https://x/server.jar"}
            }
        }"#;
        assert_eq!(
            client_url_from_version_json(json).expect("应取到"),
            "https://x/client.jar"
        );

        // 服务端地址与客户端地址同层，取错键会下到 server jar——
        // 那玩意儿不是资源包能解析的东西，所以缺键必须报错而不是将就
        let no_client = r#"{"downloads": {"server": {"url": "https://x/server.jar"}}}"#;
        assert!(client_url_from_version_json(no_client).is_err());
    }

    /** 测试目录落在 target 下：部分受限环境不允许写系统临时目录。 */
    fn test_dir(tag: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(format!(
                "mc_download_{tag}_{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir_all(&dir).expect("应能建立测试目录");
        dir
    }

    /// 读一半就断的正文：模拟连接被提前掐断（ureq 会在此处报错）
    struct CutOff {
        left: usize,
    }

    impl std::io::Read for CutOff {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.left == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "正文提前结束",
                ));
            }
            let n = self.left.min(buf.len());
            buf[..n].fill(b'x');
            self.left -= n;
            Ok(n)
        }
    }

    #[test]
    fn cut_off_body_leaves_nothing_behind() {
        let dir = test_dir("cutoff");
        let dest = dir.join("client.jar");
        let mut src = CutOff { left: 10 };

        let e = write_body(&mut src, &dest, 100, &mut |_, _| {}).unwrap_err();
        assert!(
            e.to_string().contains("提前结束"),
            "断流应如实报错：{e}"
        );
        assert!(
            !dest.exists(),
            "不完整的下载不能留在目标路径上——留下来就会被当成已下载并复用"
        );
        assert!(
            !dir.join("client.jar.tmp").exists(),
            "半截的临时文件也应清掉，避免堆在磁盘上"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn complete_body_is_renamed_into_place() {
        let dir = test_dir("complete");
        let dest = dir.join("client.jar");
        let mut src = std::io::Cursor::new(b"abcdef".to_vec());

        let mut seen: Vec<(u64, u64)> = Vec::new();
        write_body(&mut src, &dest, 6, &mut |r, t| seen.push((r, t))).expect("完整写入应成功");

        assert_eq!(std::fs::read(&dest).unwrap(), b"abcdef");
        assert!(
            seen.last().is_some_and(|&(r, t)| r == 6 && t == 6),
            "进度回调应报出收满：{seen:?}"
        );
        assert!(
            !dir.join("client.jar.tmp").exists(),
            "改名之后不该留下临时文件"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 资源索引地址要从版本 JSON 的 assetIndex.url 取。
    ///
    /// 这条是补中文的入口：拿不到索引就查不出语言文件的 sha1。
    #[test]
    fn asset_index_url_comes_from_version_json() {
        let json = r#"{
            "id": "1.20.1",
            "assetIndex": {"id": "5", "sha1": "abc", "url": "https://x/5.json"},
            "downloads": {"client": {"url": "https://x/client.jar"}}
        }"#;
        assert_eq!(
            asset_index_url_from_version_json(json).expect("应取到"),
            "https://x/5.json"
        );
        // 缺 assetIndex 必须报错：静默空串会让后面「查不到 sha1」看起来像是
        // 这个版本没有中文，而实际是压根没读到索引
        assert!(asset_index_url_from_version_json(r#"{"id":"1.20.1"}"#).is_err());
    }

    /// 语言文件的 sha1 要按 `...lang/<语言>.json` 结尾匹配。
    ///
    /// 索引里的键名各版本写法一致，但用结尾匹配可以不必关心前缀差异。
    #[test]
    fn lang_hash_is_found_by_suffix() {
        let index = r#"{
            "objects": {
                "minecraft/lang/en_us.json": {"hash": "aaaa", "size": 1},
                "minecraft/lang/zh_cn.json": {"hash": "bbbb", "size": 2},
                "minecraft/sounds.json": {"hash": "cccc", "size": 3}
            }
        }"#;
        assert_eq!(
            lang_hash_from_index(index, "zh_cn").as_deref(),
            Some("bbbb")
        );
        assert_eq!(
            lang_hash_from_index(index, "en_us").as_deref(),
            Some("aaaa")
        );
        // 索引里没有这门语言时必须给 None，而不是随便挑一个——
        // 挑错了就会把英文名当成中文名装进库里
        assert_eq!(lang_hash_from_index(index, "ja_jp"), None);
        assert_eq!(lang_hash_from_index("不是 JSON", "zh_cn"), None);
    }

    /// 资源对象地址按 sha1 拆成「前两位/后 38 位」，官方在前、镜像兜底。
    #[test]
    fn object_urls_split_the_hash_like_the_cdn() {
        let hash = "0f6d8cf9e501b428b98d57d45a5526309ff31407";
        let urls = object_url_candidates(hash);
        assert_eq!(urls.len(), 2, "官方与镜像各一个");
        assert_eq!(
            urls[0],
            "https://resources.download.minecraft.net/0f/6d8cf9e501b428b98d57d45a5526309ff31407"
        );
        assert!(urls[1].starts_with("https://bmclapi2.bangbang93.com/"));
        // 哈希不合法时给空表：拼出来的地址必然 404，不如让调用方直接报错
        assert!(object_url_candidates("ab").is_empty());
        assert!(object_url_candidates("").is_empty());
    }
}
