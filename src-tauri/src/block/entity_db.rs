//! 实体贴图库：各版本 `assets/minecraft/textures/entity/**` 的离线副本。
//!
//! 与方块库（blockdb）分开存放，因为两者的用法不同：方块贴图要被拼进图集、
//! 由模型按 uv 采样；实体贴图是按名字整张取用（贴到实体几何上）。
//!
//! 数据由 pack/entity_write.rs 从客户端 jar 构建（启动补版本界面触发）。
//! 容器格式由读写两端共同遵守；这里是读取端，所有读取都做边界检查，
//! 损坏数据返回错误而不是越界或 panic。

use crate::error::{AppError, AppResult};
use std::collections::HashMap;
use std::path::Path;

/// 魔数。与方块库的 `SMDB` 区分开，避免把两个包弄混后读出一堆无意义的数字。
const MAGIC: &[u8] = b"SMET";
const FORMAT_VERSION: u32 = 1;

/// 上限。这些数字只用于挡住损坏文件导致的超大分配，
/// 取的是「正常数据量级的若干倍」，不是业务约束。
const MAX_VERSIONS: u32 = 10_000;
const MAX_TEXTURES: u32 = 200_000;
const MAX_STRINGS: u32 = 200_000;
const MAX_ENTRIES: u32 = 200_000;
const MAX_NAME: usize = 4096;

/// 一张实体贴图在像素池中的位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityTexture {
    pub w: u16,
    pub h: u16,
    pub offset: u32,
    pub len: u32,
    pub has_alpha: bool,
}

/// 某个版本可用的实体贴图：路径 -> 贴图池下标
#[derive(Debug, Clone, Default)]
pub struct EntityVersion {
    pub data_version: i32,
    pub name: String,
    pub by_path: HashMap<String, u32>,
}

/// 实体贴图库
///
/// Debug 是派生实现，会打印整个像素池，因此不要在日志里直接打印本类型。
#[derive(Debug)]
pub struct EntityDb {
    pub paths: Vec<String>,
    pub textures: Vec<EntityTexture>,
    pub versions: Vec<EntityVersion>,
    /// zlib 解压后的 RGBA8888 像素池
    pub pixels: Vec<u8>,
}

impl EntityDb {
    /// 从已解压的字节解析
    pub fn load_from_bytes(data: &[u8]) -> AppResult<Self> {
        let mut r = Reader::new(data);
        if r.take(MAGIC.len())? != MAGIC {
            return Err(AppError::Parse("实体贴图库魔数不匹配".into()));
        }
        let fv = r.u32()?;
        if fv != FORMAT_VERSION {
            return Err(AppError::UnsupportedVersion(format!(
                "实体贴图库格式版本 {fv}，本程序支持 {FORMAT_VERSION}"
            )));
        }

        let str_count = r.count(MAX_STRINGS, "字符串")?;
        let mut paths = Vec::with_capacity(str_count as usize);
        for _ in 0..str_count {
            paths.push(r.short_str()?);
        }

        let tex_count = r.count(MAX_TEXTURES, "贴图")?;
        let mut textures = Vec::with_capacity(tex_count as usize);
        for _ in 0..tex_count {
            textures.push(EntityTexture {
                w: r.u16()?,
                h: r.u16()?,
                offset: r.u32()?,
                len: r.u32()?,
                has_alpha: r.u8()? != 0,
            });
        }

        let raw_size = r.u32()? as usize;
        let comp_size = r.u32()? as usize;
        let comp = r.take(comp_size)?;
        let pixels = decompress(comp, raw_size)?;

        // 贴图区间的合法性在解析阶段一次校验完。
        //
        // 放到取用时再判会让「下标越界」变成运行时的 0 像素，界面上表现为
        // 一张全黑贴图，看不出是数据坏了；这里直接拒绝整个文件。
        for (i, t) in textures.iter().enumerate() {
            let end = (t.offset as usize)
                .checked_add(t.len as usize)
                .ok_or_else(|| AppError::Corrupt(format!("贴图 {i} 的区间溢出")))?;
            if end > pixels.len() {
                return Err(AppError::Corrupt(format!(
                    "贴图 {i} 区间 {}..{} 超出像素池长度 {}",
                    t.offset,
                    end,
                    pixels.len()
                )));
            }
            let expect = t.w as usize * t.h as usize * 4;
            if t.len as usize != expect {
                return Err(AppError::Corrupt(format!(
                    "贴图 {i} 长度 {} 与尺寸 {}x{} 不符（应为 {expect}）",
                    t.len, t.w, t.h
                )));
            }
        }

        let ver_count = r.count(MAX_VERSIONS, "版本")?;
        let mut versions = Vec::with_capacity(ver_count as usize);
        for _ in 0..ver_count {
            let data_version = r.u32()? as i32;
            let name_len = r.u16()? as usize;
            if name_len > MAX_NAME {
                return Err(AppError::Corrupt(format!("版本名过长：{name_len}")));
            }
            let name = String::from_utf8_lossy(r.take(name_len)?).into_owned();
            let n = r.count(MAX_ENTRIES, "贴图条目")?;
            let mut by_path = HashMap::with_capacity(n.min(8192) as usize);
            for _ in 0..n {
                let path_idx = r.u32()? as usize;
                let pool_idx = r.u32()?;
                let path = paths.get(path_idx).ok_or_else(|| {
                    AppError::Corrupt(format!("贴图路径下标越界：{path_idx}"))
                })?;
                if pool_idx as usize >= textures.len() {
                    return Err(AppError::Corrupt(format!(
                        "贴图池下标越界：{pool_idx}（共 {} 张）",
                        textures.len()
                    )));
                }
                by_path.insert(path.clone(), pool_idx);
            }
            versions.push(EntityVersion { data_version, name, by_path });
        }

        Ok(Self { paths, textures, versions, pixels })
    }

    /// 从文件解析（自动识别 zlib / gzip / 未压缩）
    pub fn load(path: &Path) -> AppResult<Self> {
        let raw = std::fs::read(path).map_err(|e| {
            AppError::Parse(format!("读取实体贴图库 {} 失败：{e}", path.display()))
        })?;
        // 文件头 4 字节是魔数时说明未压缩，否则按容器解压
        let data = if raw.len() >= 4 && &raw[..4] == MAGIC {
            raw
        } else {
            decompress(&raw, 0)?
        };
        Self::load_from_bytes(&data)
    }

    /// 按 data_version 找版本；同一个 data_version 有多个版本名时取第一个。
    ///
    /// 用 data_version 而不是版本名：文件里只记 dataVersion，而版本名会随
    /// 快照/补丁变化（同一个 dataVersion 可能对应多个名字）。
    pub fn version_for(&self, data_version: i32) -> Option<&EntityVersion> {
        self.versions.iter().find(|v| v.data_version == data_version)
    }

    /// 按版本与路径取像素。路径形如 `entity/chest/normal`。
    pub fn pixels_of(&self, ver: &EntityVersion, path: &str) -> Option<(&EntityTexture, &[u8])> {
        let idx = *ver.by_path.get(path)? as usize;
        let t = self.textures.get(idx)?;
        let s = t.offset as usize;
        let e = s.checked_add(t.len as usize)?;
        Some((t, self.pixels.get(s..e)?))
    }
}

/// 与方块库同样的解压策略：zlib 优先，其次 gzip，都不行则原样返回。
///
/// `expect` 为 0 表示长度未知，此时不做长度校验；非 0 时长度必须一致。
fn decompress(bytes: &[u8], expect: usize) -> AppResult<Vec<u8>> {
    use std::io::Read;
    let try_read = |mut r: Box<dyn Read>| -> Option<Vec<u8>> {
        let mut out = Vec::new();
        r.read_to_end(&mut out).ok().map(|_| out)
    };
    let mut got = try_read(Box::new(flate2::read::ZlibDecoder::new(bytes)));
    if got.is_none() {
        got = try_read(Box::new(flate2::read::GzDecoder::new(bytes)));
    }
    let out = match got {
        Some(v) => v,
        None => bytes.to_vec(),
    };
    if expect != 0 && out.len() != expect {
        return Err(AppError::Corrupt(format!(
            "解压后长度 {} 与头部声明 {} 不一致",
            out.len(),
            expect
        )));
    }
    Ok(out)
}

/// 边界检查读取器
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> AppResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| AppError::Corrupt("读取长度溢出".into()))?;
        if end > self.data.len() {
            return Err(AppError::Corrupt(format!(
                "数据不足：位置 {} 需要 {} 字节，总长 {}",
                self.pos,
                n,
                self.data.len()
            )));
        }
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> AppResult<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> AppResult<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> AppResult<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// u16 长度前缀的 UTF-8 字符串
    fn short_str(&mut self) -> AppResult<String> {
        let n = self.u16()? as usize;
        if n > MAX_NAME {
            return Err(AppError::Corrupt(format!("字符串过长：{n}")));
        }
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }

    fn count(&mut self, max: u32, what: &str) -> AppResult<u32> {
        let n = self.u32()?;
        if n > max {
            return Err(AppError::Corrupt(format!("{what}数量异常：{n}")));
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 16x16、alpha 全 255 的合成贴图。
    fn tex_a() -> Vec<u8> {
        let mut v = Vec::with_capacity(16 * 16 * 4);
        for i in 0..16 * 16 {
            v.extend_from_slice(&[(i % 251) as u8, (i * 7 % 253) as u8, 9, 255]);
        }
        v
    }

    /// 8x8、带透明像素的合成贴图。
    fn tex_b() -> Vec<u8> {
        vec![7u8; 8 * 8 * 4]
    }

    /// 一个小型合成库：两个版本共享贴图表，路径到贴图的映射各不相同。
    /// 构造结构体后经写入端序列化再读回，覆盖 字符串表/贴图表/像素池/版本
    /// 全链路，比手写字节更能跟着格式演进。
    fn sample_db() -> EntityDb {
        let a = tex_a();
        let b = tex_b();
        let mut pixels = Vec::new();
        pixels.extend_from_slice(&a);
        pixels.extend_from_slice(&b);
        EntityDb {
            paths: vec![
                "entity/chest/ender".into(),
                "entity/chest/normal".into(),
                "entity/shulker/shulker".into(),
            ],
            textures: vec![
                EntityTexture {
                    w: 16,
                    h: 16,
                    offset: 0,
                    len: a.len() as u32,
                    has_alpha: false,
                },
                EntityTexture {
                    w: 8,
                    h: 8,
                    offset: a.len() as u32,
                    len: b.len() as u32,
                    has_alpha: true,
                },
            ],
            versions: vec![
                EntityVersion {
                    data_version: 2225,
                    name: "1.15".into(),
                    by_path: HashMap::from([
                        ("entity/chest/normal".to_string(), 0u32),
                        ("entity/shulker/shulker".to_string(), 1u32),
                    ]),
                },
                EntityVersion {
                    data_version: 3465,
                    name: "1.20.1".into(),
                    by_path: HashMap::from([
                        ("entity/chest/normal".to_string(), 0u32),
                        ("entity/chest/ender".to_string(), 1u32),
                    ]),
                },
            ],
            pixels,
        }
    }

    fn roundtrip(db: &EntityDb) -> EntityDb {
        let bytes = crate::pack::entity_write::serialize(db).expect("序列化失败");
        EntityDb::load_from_bytes(&bytes).expect("写出的库应能读回")
    }

    /// 读回后按路径取到的必须是当初放进去的那张贴图：
    /// 断言尺寸、像素与透明标记，而不只是「取到了」。
    #[test]
    fn roundtrip_preserves_lookups() {
        let db = roundtrip(&sample_db());
        let ver = db.version_for(3465).expect("应有 dataVersion 3465");
        assert_eq!(ver.name, "1.20.1");

        let (t, px) = db
            .pixels_of(ver, "entity/chest/normal")
            .expect("应有普通箱子贴图");
        assert_eq!((t.w, t.h), (16, 16));
        assert!(!t.has_alpha);
        assert_eq!(px, tex_a().as_slice(), "像素应与写入时一致");

        let (t, px) = db
            .pixels_of(ver, "entity/chest/ender")
            .expect("应有末影箱贴图");
        assert_eq!((t.w, t.h), (8, 8));
        assert!(t.has_alpha);
        assert_eq!(px, tex_b().as_slice());
    }

    /// 贴图路径与版本必须一一对得上：同一个 dataVersion 下，
    /// 不同方块实体的贴图不能是同一张。
    #[test]
    fn paths_map_to_distinct_textures() {
        let db = roundtrip(&sample_db());
        let ver = db.version_for(3465).expect("应有 1.20.1");
        let (_, ender) = db.pixels_of(ver, "entity/chest/ender").unwrap();
        let (_, normal) = db.pixels_of(ver, "entity/chest/normal").unwrap();
        assert_ne!(ender, normal, "末影箱与普通箱子的贴图不应相同");
        // 调用两次必须得到同一张（路径 -> 下标的映射是确定的）
        let (a, _) = db.pixels_of(ver, "entity/chest/normal").unwrap();
        let (b, _) = db.pixels_of(ver, "entity/chest/normal").unwrap();
        assert_eq!(a, b);
    }

    /// 同样的像素被两个版本的路径引用时，应指向同一个贴图下标：
    /// 运行时是把整块像素池读进内存的，重复存会让内存随版本数膨胀。
    #[test]
    fn shared_pixels_keep_one_pool_entry() {
        let db = roundtrip(&sample_db());
        let v115 = db.version_for(2225).expect("应有 1.15");
        let v121 = db.version_for(3465).expect("应有 1.20.1");
        assert_eq!(
            v115.by_path["entity/chest/normal"], v121.by_path["entity/chest/normal"],
            "两个版本里相同的贴图应共享下标"
        );
        // 跨版本同一张图（1.15 的潜影盒 == 1.20.1 的末影箱）同样共享
        assert_eq!(
            v115.by_path["entity/shulker/shulker"], v121.by_path["entity/chest/ender"]
        );
        assert_eq!(db.pixels.len(), tex_a().len() + tex_b().len());
    }

    /// 版本查不到、路径查不到都要返回 None，而不是给一张空图。
    #[test]
    fn unknown_version_or_path_yields_nothing() {
        let db = roundtrip(&sample_db());
        assert!(db.version_for(-1).is_none(), "不存在的 dataVersion 应查不到");
        let ver = db.version_for(3465).expect("应有 1.20.1");
        assert!(db.pixels_of(ver, "entity/does_not_exist").is_none());
    }

    /// 损坏数据必须报错，不能返回半截像素或 panic。
    #[test]
    fn corrupt_data_is_rejected() {
        // 魔数正确但被截断
        let mut bad = db_pack_head();
        bad.truncate(bad.len().min(20));
        assert!(EntityDb::load_from_bytes(&bad).is_err(), "截断的数据应报错");

        // 魔数不对
        let mut wrong = db_pack_head();
        wrong[0] = b'X';
        let e = EntityDb::load_from_bytes(&wrong).unwrap_err().to_string();
        assert!(e.contains("魔数"), "{e}");

        // 格式版本不对
        let mut newer = db_pack_head();
        newer[4..8].copy_from_slice(&999u32.to_le_bytes());
        let e = EntityDb::load_from_bytes(&newer).unwrap_err().to_string();
        assert!(e.contains("格式版本"), "{e}");
    }

    /// 一个最小的合法包头（魔数 + 格式版本 + 各段计数为 0）。
    fn db_pack_head() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(MAGIC);
        v.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes()); // 字符串
        v.extend_from_slice(&0u32.to_le_bytes()); // 贴图
        v.extend_from_slice(&0u32.to_le_bytes()); // raw_size
        v.extend_from_slice(&0u32.to_le_bytes()); // comp_size
        v.extend_from_slice(&0u32.to_le_bytes()); // 版本
        v
    }

    /// 空库（各段为 0）是合法输入，不应报错。
    #[test]
    fn empty_pack_is_valid() {
        let db = EntityDb::load_from_bytes(&db_pack_head()).expect("空包应能解析");
        assert!(db.textures.is_empty());
        assert!(db.versions.is_empty());
        assert!(db.pixels.is_empty());
    }
}
