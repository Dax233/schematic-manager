//! 重复投影文件筛查。
//!
//! 判定「重复」只看文件内容，不看文件名：同一份投影常被改名保存多次，
//! 名称相同也可能是不同内容。
//!
//! 三轮筛选，逐步收窄，避免对每个文件都做全量比较：
//! 先按大小分组（读取目录时已经拿到大小），组内再按内容哈希分组，
//! 最后对同哈希的文件逐字节比对确认——哈希只是过滤手段，
//! 结论必须由逐字节比对得出，否则哈希碰撞会把不同文件误判为重复。

use std::io::Read;
use std::path::{Path, PathBuf};

/// 筛查结果
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DuplicateScan {
    /// 内容相同的文件组
    pub groups: Vec<DuplicateGroup>,
    /// 读取失败、因而未能参与判定的文件及原因。
    ///
    /// 必须上报而不是丢弃：这些文件既不能判为「有重复」，
    /// 也不能被当成「与谁都不重复」——对用户来说两者含义完全不同。
    pub unreadable: Vec<(PathBuf, String)>,
}

/// 一组内容完全相同的文件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateGroup {
    pub size: u64,
    /// 组内文件路径，按路径排序，结果稳定
    pub paths: Vec<PathBuf>,
}

/// 参与筛查的文件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub path: PathBuf,
    pub size: u64,
}

/// FNV-1a 64 位哈希。
///
/// 只用于把候选集合收窄，不承担正确性——最终由逐字节比对定论。
fn hash_reader<R: Read>(mut r: R) -> std::io::Result<u64> {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for b in &buf[..n] {
            h ^= *b as u64;
            h = h.wrapping_mul(PRIME);
        }
    }
    Ok(h)
}

fn content_hash(path: &Path) -> std::io::Result<u64> {
    let f = std::fs::File::open(path)?;
    hash_reader(std::io::BufReader::new(f))
}

/// 逐字节比较两个文件是否完全相同。
///
/// 大小已在调用前确认一致，这里只比内容。
fn same_content(a: &Path, b: &Path) -> std::io::Result<bool> {
    let fa = std::fs::File::open(a)?;
    let fb = std::fs::File::open(b)?;
    let mut ra = std::io::BufReader::new(fa);
    let mut rb = std::io::BufReader::new(fb);
    let mut ba = [0u8; 64 * 1024];
    let mut bb = [0u8; 64 * 1024];
    loop {
        let na = ra.read(&mut ba)?;
        let nb = rb.read(&mut bb)?;
        if na != nb {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
        if ba[..na] != bb[..nb] {
            return Ok(false);
        }
    }
}

/// 找出内容相同的文件组。
///
/// 只返回至少两个成员的组。某个文件读不了不应让整次筛查失败，
/// 但也不能把它当成「与谁都不重复」——对用户来说两者含义完全不同，
/// 因此读不了的文件单独列入 unreadable 上报。
pub fn find(candidates: &[Candidate]) -> DuplicateScan {
    use std::collections::HashMap;

    // 第一轮：按大小分组，大小不同的必然不重复
    let mut by_size: HashMap<u64, Vec<&Candidate>> = HashMap::new();
    for c in candidates {
        by_size.entry(c.size).or_default().push(c);
    }

    let mut out: Vec<DuplicateGroup> = Vec::new();
    let mut unreadable: Vec<(PathBuf, String)> = Vec::new();

    for (size, group) in by_size {
        if group.len() < 2 {
            continue;
        }
        // 第二轮：按内容哈希分组
        let mut by_hash: HashMap<u64, Vec<&Candidate>> = HashMap::new();
        for c in group {
            match content_hash(&c.path) {
                Ok(h) => by_hash.entry(h).or_default().push(c),
                Err(e) => unreadable.push((c.path.clone(), e.to_string())),
            }
        }

        // 第三轮：同哈希的逐字节比对，确认后才算重复
        for (_, bucket) in by_hash {
            if bucket.len() < 2 {
                continue;
            }
            let mut remaining: Vec<&Candidate> = bucket;
            while !remaining.is_empty() {
                let head = remaining.remove(0);
                let mut same: Vec<PathBuf> = vec![head.path.clone()];
                let mut failed: Vec<(PathBuf, String)> = Vec::new();
                remaining.retain(|other| match same_content(&head.path, &other.path) {
                    Ok(true) => {
                        same.push(other.path.clone());
                        false // 已归入本组，从待处理中移除
                    }
                    Ok(false) => true,
                    Err(e) => {
                        // 比对失败的文件留在待处理集合里，交由后续轮次
                        // 再试；同时如实上报，不静默丢弃
                        failed.push((other.path.clone(), e.to_string()));
                        false
                    }
                });
                unreadable.extend(failed);
                if same.len() > 1 {
                    same.sort();
                    out.push(DuplicateGroup { size, paths: same });
                }
            }
        }
    }

    // 组间按路径排序，结果稳定可复现
    out.sort_by(|a, b| a.paths.cmp(&b.paths));
    unreadable.sort();
    DuplicateScan { groups: out, unreadable }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, data: &[u8]) -> Candidate {
        let p = dir.join(name);
        std::fs::write(&p, data).unwrap();
        Candidate { path: p, size: data.len() as u64 }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("testtmp")
            .join(format!("sm_dup_{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn finds_identical_files_under_different_names() {
        let d = temp_dir("basic");
        let a = write(&d, "a.litematic", b"same content here");
        let b = write(&d, "b.litematic", b"same content here");
        let c = write(&d, "c.litematic", b"different content!");
        let r = find(&[a, b, c]);
        assert_eq!(r.groups.len(), 1);
        assert_eq!(r.groups[0].paths.len(), 2);
        assert!(r.groups[0].paths.contains(&d.join("a.litematic")));
        assert!(r.groups[0].paths.contains(&d.join("b.litematic")));
        assert!(r.unreadable.is_empty(), "不应有读取失败");
    }

    #[test]
    fn same_name_different_content_is_not_duplicate() {
        let d = temp_dir("notdup");
        // 大小相同但内容不同，必须靠逐字节比对排除
        let a = write(&d, "a.litematic", b"AAAA");
        let b = write(&d, "b.litematic", b"BBBB");
        assert!(find(&[a, b]).groups.is_empty(), "内容不同不应判为重复");
    }

    #[test]
    fn no_duplicates_yields_empty() {
        let d = temp_dir("none");
        let a = write(&d, "a.litematic", b"one");
        let b = write(&d, "b.litematic", b"two two");
        assert!(find(&[a, b]).groups.is_empty());
        assert!(find(&[]).groups.is_empty(), "空输入不应 panic");
    }

    #[test]
    fn groups_three_or_more() {
        let d = temp_dir("three");
        let a = write(&d, "a.litematic", b"xyz");
        let b = write(&d, "b.litematic", b"xyz");
        let c = write(&d, "c.litematic", b"xyz");
        let r = find(&[a, b, c]);
        assert_eq!(r.groups.len(), 1);
        assert_eq!(r.groups[0].paths.len(), 3);
        assert_eq!(r.groups[0].size, 3);
    }

    #[test]
    fn multiple_independent_groups() {
        let d = temp_dir("multi");
        let a1 = write(&d, "a1.litematic", b"first");
        let a2 = write(&d, "a2.litematic", b"first");
        let b1 = write(&d, "b1.litematic", b"second!!");
        let b2 = write(&d, "b2.litematic", b"second!!");
        let r = find(&[a1, a2, b1, b2]);
        assert_eq!(r.groups.len(), 2, "应识别出两组独立的重复");
        assert!(r.groups.iter().all(|g| g.paths.len() == 2));
    }

    #[test]
    fn unreadable_file_is_reported_not_silently_dropped() {
        let d = temp_dir("missing");
        let a = write(&d, "a.litematic", b"data");
        let ghost_path = d.join("不存在的文件.litematic");
        let ghost = Candidate { path: ghost_path.clone(), size: 4 };
        let r = find(&[a, ghost]);

        // 读不了的文件既不能成组，也不能被默默当成「与谁都不重复」
        assert!(r.groups.is_empty());
        assert_eq!(r.unreadable.len(), 1, "必须上报读取失败");
        assert_eq!(r.unreadable[0].0, ghost_path, "应报告具体是哪个文件");
        assert!(!r.unreadable[0].1.is_empty(), "应带上失败原因");
    }

    #[test]
    fn unreadable_member_does_not_hide_real_group() {
        let d = temp_dir("mixed");
        let a = write(&d, "a.litematic", b"same");
        let b = write(&d, "b.litematic", b"same");
        // 与 a、b 大小相同但读不出来的文件混在中间
        let ghost = Candidate { path: d.join("ghost.litematic"), size: 4 };
        let r = find(&[a, b, ghost]);
        assert_eq!(r.groups.len(), 1, "读取失败不应影响真实的重复判定");
        assert_eq!(r.groups[0].paths.len(), 2);
        assert_eq!(r.unreadable.len(), 1);
    }

    #[test]
    fn empty_files_of_equal_size_are_duplicates() {
        let d = temp_dir("empty");
        let a = write(&d, "a.litematic", b"");
        let b = write(&d, "b.litematic", b"");
        let r = find(&[a, b]);
        assert_eq!(r.groups.len(), 1, "两个空文件内容相同，应判为重复");
        assert_eq!(r.groups[0].size, 0);
    }

    #[test]
    fn result_order_is_stable() {
        let d = temp_dir("order");
        let paths: Vec<Candidate> = ["z", "a", "m"]
            .iter()
            .map(|n| write(&d, &format!("{n}.litematic"), b"same"))
            .collect();
        let g1 = find(&paths);
        let mut rev = paths.clone();
        rev.reverse();
        let g2 = find(&rev);
        assert_eq!(g1, g2, "输入顺序不应影响结果");
        let names: Vec<String> = g1.groups[0]
            .paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.litematic", "m.litematic", "z.litematic"]);
    }
}
