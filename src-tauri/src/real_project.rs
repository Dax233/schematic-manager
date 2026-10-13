//! 针对**任意真实投影集合**的端到端核对（按需运行）。
//!
//! 为什么单独一个模块
//!   仓库内只随源码分发一个小体积样本（见 tests/data/README.md，出于体积与
//!   许可证考虑）。而「真实投影 + 随包方块库」这个组合才最接近用户的实际
//!   使用：方块库的版本表或贴图池与投影对不上时，各模块自己的单元测试照样
//!   全绿，界面却显示不出方块——而界面无法在无图形环境下自动测试。
//!
//! 怎么运行
//!   SM_REAL_DIR=/path/to/projects cargo test --lib -- --ignored --nocapture
//!
//! 这些用例都标了 `#[ignore]`，而不是「读不到环境变量就返回」：后者在测试
//! 报告里会显示为通过，看不出其实一次都没跑。标了 ignore 会如实显示为
//! 「未运行」，需要跑时用 `--ignored` 显式调用。

use std::path::{Path, PathBuf};

/// 递归收集目录下的 `.litematic`
fn collect_litematic(dir: &str) -> Vec<PathBuf> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().map(|x| x == "litematic").unwrap_or(false) {
                    out.push(p);
                }
            }
        }
    }
    let mut files = Vec::new();
    walk(Path::new(dir), &mut files);
    files.sort();
    assert!(!files.is_empty(), "SM_REAL_DIR 下没有 .litematic");
    files
}

fn real_dir() -> String {
    std::env::var("SM_REAL_DIR").expect("需设置 SM_REAL_DIR 指向真实投影所在目录")
}

/// 随包分发的方块库
fn bundled_db() -> crate::blockdb::BlockDb {
    crate::blockdb::BlockDb::load(
        concat!(env!("CARGO_MANIFEST_DIR"), "/resources/blocks.dat.z"),
        false,
    )
    .expect("内置方块库载入失败")
}

fn file_name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().to_string()
}

/// 临时工作目录，测试结束即删除。
///
/// 用 `std::env::temp_dir()`（即 TEMP/TMP 指向的位置）。注意在本仓库的
/// Windows 开发环境里系统临时目录不可写，需要先把 TEMP 指到仓库内的
/// `_temp`（`tools/typecheck/run.ps1` 也是这么做的），否则这里会失败——
/// 因此把实际路径与底层错误一并写进提示，而不是只说「无法创建」。
fn work_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sm_real_{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap_or_else(|e| {
        panic!(
            "无法创建临时目录 {}：{e}\n（系统临时目录不可写时，请把 TEMP/TMP 指向可写目录）",
            d.display()
        )
    });
    d
}

/// 每个真实投影：载入 → 区域结构自检 → NBT 往返零损失 → 方块统计。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn every_real_project_parses_and_roundtrips() {
    let files = collect_litematic(&real_dir());
    let mut failed: Vec<String> = Vec::new();

    for f in &files {
        let name = file_name(f);

        let sc = match crate::litematic::Schematic::load(&f.to_string_lossy()) {
            Ok(sc) => sc,
            Err(e) => {
                failed.push(format!("{name}: 载入失败 {e}"));
                continue;
            }
        };

        // 区域结构：归一化后尺寸全正，索引数等于体积
        for r in &sc.regions {
            if !r.size.iter().all(|&v| v > 0) {
                failed.push(format!("{name}: 尺寸非正 {:?}", r.size));
                break;
            }
            let expect = (r.size[0] as usize) * (r.size[1] as usize) * (r.size[2] as usize);
            if r.indices.len() != expect {
                failed.push(format!("{name}: 索引数 {} != 体积 {}", r.indices.len(), expect));
                break;
            }
        }

        // 写出路径必须零信息损失
        let roundtrip = (|| -> Result<(), String> {
            let bytes = std::fs::read(f).map_err(|e| e.to_string())?;
            let root = crate::nbt::parse_maybe_gzip(&bytes).map_err(|e| e.to_string())?;
            let out = crate::nbt::serialize_gzip(&root, "", 6).map_err(|e| e.to_string())?;
            let back = crate::nbt::parse_maybe_gzip(&out).map_err(|e| e.to_string())?;
            if root != back {
                return Err("NBT 往返前后不一致".into());
            }
            Ok(())
        })();
        if let Err(e) = roundtrip {
            failed.push(format!("{name}: {e}"));
        }

        let an = sc.analyze();
        println!(
            "  [OK]   {name}  区域 {}  非空气 {}  状态种类 {}",
            sc.regions.len(),
            an.non_air_total,
            an.blocks_by_state.len()
        );
    }

    println!("\n  共 {} 个文件，失败 {} 个", files.len(), failed.len());
    assert!(failed.is_empty(), "以下文件未通过：\n{}", failed.join("\n"));
}

/// 每个真实投影：用随包方块库构建 3D 场景，且必须真的产出网格。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn every_real_project_builds_a_scene() {
    let files = collect_litematic(&real_dir());
    let db = bundled_db();
    let mut failed: Vec<String> = Vec::new();

    for f in &files {
        let name = file_name(f);
        match crate::scene::build_with_db(&f.to_string_lossy(), &db) {
            Ok(sc) => {
                println!(
                    "  [OK]   {name}  版本 {}  批次 {}  顶点 {}  三角 {}  未解析 {}",
                    sc.mc_version_name,
                    sc.batches.len(),
                    sc.total_vertices,
                    sc.total_triangles,
                    sc.unresolved_blocks
                );
                if sc.batches.is_empty() || sc.total_vertices == 0 {
                    failed.push(format!("{name}: 没有产出任何网格"));
                }
            }
            Err(e) => failed.push(format!("{name}: 建场景失败 {e}")),
        }
    }

    println!("\n  共 {} 个文件，未通过 {} 个", files.len(), failed.len());
    assert!(failed.is_empty(), "以下文件未通过：\n{}", failed.join("\n"));
}

/// 每个真实投影：把出现最多的方块替换成石头，写出后再读回核对。
///
/// 这是**会改写用户文件**的功能，出错代价最大。因此在真实数据上验证
/// 「替换 + 写出 + 读回」这条完整链路，而不只测内存里的替换函数：
/// 序列化、位打包、调色板增长等环节的问题只在写出后才暴露。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn every_real_project_survives_a_block_replacement() {
    let files = collect_litematic(&real_dir());
    let db = bundled_db();
    let dir = work_dir("replace");
    let mut failed: Vec<String> = Vec::new();
    let mut checked = 0;

    for f in &files {
        let name = file_name(f);
        let mut sc = match crate::litematic::Schematic::load(&f.to_string_lossy()) {
            Ok(sc) => sc,
            Err(e) => {
                failed.push(format!("{name}: 载入失败 {e}"));
                continue;
            }
        };

        // 取出现最多的方块作为替换源，保证一定命中
        let before = sc.analyze();
        let Some((state_key, _)) = before.blocks_by_state.iter().max_by_key(|(_, n)| **n)
        else {
            continue; // 空投影
        };
        // 去掉属性部分，只留方块名
        let from_name = state_key.split('[').next().unwrap_or(state_key).to_string();
        if from_name.is_empty() || from_name.ends_with(":air") || from_name == "minecraft:air" {
            continue;
        }

        let vi = match db.pick_version(sc.summary().data_version) {
            Some(v) => v,
            None => {
                failed.push(format!("{name}: 方块库无可用版本"));
                continue;
            }
        };
        let info = crate::block_model::DbBlockInfo { db: &db, version: &db.versions[vi] };

        let volume_before: u64 = sc.regions.iter().map(|r| r.indices.len() as u64).sum();
        let non_air_before = before.non_air_total;
        let air_before = before.air_total;

        let rule = crate::replace::ReplaceRule {
            from_name: from_name.clone(),
            to_name: "minecraft:stone".to_string(),
            ..Default::default()
        };
        let out = crate::replace::replace_blocks(&mut sc, &rule, &info, 0);
        if out.replaced == 0 {
            println!("  [跳过] {name}（{from_name} → stone 无变化）");
            continue;
        }

        // 写出到临时目录再读回
        let dst = dir.join(format!("replaced_{name}"));
        let dst_s = dst.to_string_lossy().to_string();
        if let Err(e) = sc.save_to(&dst_s) {
            failed.push(format!("{name}: 写出失败 {e}"));
            continue;
        }
        let back = match crate::litematic::Schematic::load(&dst_s) {
            Ok(v) => v,
            Err(e) => {
                failed.push(format!("{name}: 读回失败 {e}"));
                continue;
            }
        };

        // 读回后：体积不变、空气与非空气总数不变、石头确实变多
        let volume_after: u64 = back.regions.iter().map(|r| r.indices.len() as u64).sum();
        let after = back.analyze();
        let stone_after: u64 = after
            .blocks_by_state
            .iter()
            .filter(|(k, _)| k.as_str() == "minecraft:stone" || k.starts_with("minecraft:stone["))
            .map(|(_, n)| *n)
            .sum();

        if volume_after != volume_before {
            failed.push(format!("{name}: 体积 {volume_before} → {volume_after}"));
        }
        if after.non_air_total != non_air_before || after.air_total != air_before {
            failed.push(format!(
                "{name}: 方块总数变化（非空气 {non_air_before}→{}，空气 {air_before}→{}）",
                after.non_air_total, after.air_total
            ));
        }
        if stone_after == 0 {
            failed.push(format!("{name}: 替换后没有石头，写出可能没生效"));
        }

        checked += 1;
        println!(
            "  [OK]   {name}  {from_name} → stone  替换 {} 个  调色板项 +{}  石头 {}",
            out.replaced, out.palette_changed, stone_after
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
    println!("\n  实际替换 {checked} 个，未通过 {} 个", failed.len());
    assert!(checked > 0, "没有一个投影发生了替换，这组用例没验到东西");
    assert!(failed.is_empty(), "以下文件未通过：\n{}", failed.join("\n"));
}

/// 每个真实投影：转换到另一个版本，写出后再读回核对。
///
/// 与替换同样是会改写文件的功能。转换会重建方块状态与 dataVersion，
/// 因此重点核对「体积不变、方块总数不变、能再次读回」。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn every_real_project_survives_a_version_conversion() {
    let files = collect_litematic(&real_dir());
    let db = bundled_db();
    let dir = work_dir("convert");
    let mut failed: Vec<String> = Vec::new();
    let mut checked = 0;

    for f in &files {
        let name = file_name(f);
        let src = f.to_string_lossy().to_string();
        let sc = match crate::litematic::Schematic::load(&src) {
            Ok(sc) => sc,
            Err(e) => {
                failed.push(format!("{name}: 载入失败 {e}"));
                continue;
            }
        };

        let summary = sc.summary();
        let targets = crate::convert::targets(&db, summary.data_version);
        let Some(t) = targets.first() else {
            println!("  [跳过] {name}（没有可转换的目标版本）");
            continue;
        };
        let source_idx = match db.pick_version(summary.data_version) {
            Some(v) => v,
            None => {
                failed.push(format!("{name}: 方块库无可用源版本"));
                continue;
            }
        };

        // 缺替代品的项填石头，避免整项被跳过而验不到东西
        let items: Vec<crate::convert::ConvertItem> = crate::convert::analyze(&sc, &db, t.index)
            .into_iter()
            .map(|mut i| {
                if i.chosen.is_empty() && i.suggested.is_empty() {
                    i.chosen = "minecraft:stone".to_string();
                }
                i
            })
            .collect();

        let volume_before: u64 = sc.regions.iter().map(|r| r.indices.len() as u64).sum();
        let non_air_before = sc.analyze().non_air_total;
        let source_name = summary.mc_version_name.clone();

        let mut work = match crate::litematic::Schematic::load(&src) {
            Ok(v) => v,
            Err(e) => {
                failed.push(format!("{name}: 二次载入失败 {e}"));
                continue;
            }
        };
        let out = crate::convert::convert(&mut work, &db, source_idx, t.index, &items);

        let dst = dir.join(format!("converted_{name}"));
        let dst_s = dst.to_string_lossy().to_string();
        if let Err(e) = work.save_to(&dst_s) {
            failed.push(format!("{name}: 写出失败 {e}"));
            continue;
        }
        let back = match crate::litematic::Schematic::load(&dst_s) {
            Ok(v) => v,
            Err(e) => {
                failed.push(format!("{name}: 读回失败 {e}"));
                continue;
            }
        };

        let volume_after: u64 = back.regions.iter().map(|r| r.indices.len() as u64).sum();
        let after = back.analyze();
        if volume_after != volume_before {
            failed.push(format!("{name}: 体积 {volume_before} → {volume_after}"));
        }
        if after.non_air_total != non_air_before {
            failed.push(format!(
                "{name}: 非空气方块数 {non_air_before} → {}",
                after.non_air_total
            ));
        }
        // 转换后 dataVersion 必须已改成目标版本
        if back.summary().data_version != t.data_version {
            failed.push(format!(
                "{name}: dataVersion 仍为 {}，应为 {}",
                back.summary().data_version,
                t.data_version
            ));
        }

        checked += 1;
        println!(
            "  [OK]   {name}  {source_name} → {}  dataVersion {} → {}  改写 {} 块  非空气 {}",
            t.name,
            summary.data_version,
            t.data_version,
            out.replaced_blocks,
            after.non_air_total
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
    println!("\n  实际转换 {checked} 个，未通过 {} 个", failed.len());
    assert!(checked > 0, "没有一个投影完成转换，这组用例没验到东西");
    assert!(failed.is_empty(), "以下文件未通过：\n{}", failed.join("\n"));
}

/// 查重：副本必须被判为重复，内容不同的投影必须互不相同。
///
/// 用产品自己的 `duplicate::find`，而不是另写一份比较逻辑——
/// 否则验的是测试里的那份实现。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn duplicate_scan_recognises_copies_but_not_distinct_projects() {
    let files = collect_litematic(&real_dir());
    let dir = work_dir("dup");

    // 每个真实文件复制一份到临时目录，**原件与副本都作为候选**：
    // 只放副本的话每个副本各属不同原件，两两内容不同，自然凑不出重复组
    // （第一版就是这样，报了「重复组 0 个」，其实是测试自己写错了）。
    let mut candidates: Vec<crate::duplicate::Candidate> = Vec::new();
    let mut copies: Vec<String> = Vec::new();
    for f in &files {
        let bytes = std::fs::read(f).expect("读取真实文件失败");
        candidates.push(crate::duplicate::Candidate {
            path: f.clone(),
            size: bytes.len() as u64,
        });
        // 副本：不同目录、不同文件名，内容完全相同
        let copy = dir.join(format!("copy_{}", file_name(f)));
        std::fs::write(&copy, &bytes).expect("写入副本失败");
        copies.push(file_name(&copy));
        candidates.push(crate::duplicate::Candidate {
            path: copy.clone(),
            size: bytes.len() as u64,
        });
    }
    // 干扰项：内容与任何文件都不同、但体积恰好等于第一个文件
    if let Some(first) = files.first() {
        let n = std::fs::metadata(first).expect("取大小失败").len() as usize;
        let filler = dir.join("干扰项.litematic");
        std::fs::write(&filler, vec![0xABu8; n]).expect("写入干扰项失败");
        candidates.push(crate::duplicate::Candidate { path: filler, size: n as u64 });
    }

    let scan = crate::duplicate::find(&candidates);

    // 每个副本都应与它的原件成组，且该组恰好这两个文件
    let mut failed: Vec<String> = Vec::new();
    for copy in &copies {
        let group = scan.groups.iter().find(|g| {
            g.paths.iter().any(|p| {
                p.file_name().map(|n| n.to_string_lossy() == *copy).unwrap_or(false)
            })
        });
        match group {
            None => failed.push(format!("{copy} 未被识别为某个重复组的一员")),
            Some(g) if g.paths.len() != 2 => {
                failed.push(format!("{copy} 所在组有 {} 个文件，应为 2", g.paths.len()))
            }
            Some(_) => {}
        }
    }
    // 干扰项不应与任何真实文件同组
    let filler_grouped = scan.groups.iter().any(|g| {
        g.paths.iter().any(|p| {
            p.file_name().map(|n| n.to_string_lossy() == "干扰项.litematic").unwrap_or(false)
        })
    });
    if filler_grouped {
        failed.push("干扰项（内容不同）被判为重复".to_string());
    }

    println!(
        "  候选 {} 个，重复组 {} 个，副本 {} 个",
        candidates.len(),
        scan.groups.len(),
        copies.len()
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(failed.is_empty(), "以下判定不符预期：\n{}", failed.join("\n"));
}

/// 备份与还原：改坏文件后必须能**逐字节**还原回原样。
///
/// 这是所有破坏性操作的唯一退路（替换、版本转换、改元数据都会先备份）。
/// 备份坏了不会立刻暴露——用户要等到某次替换出错、想还原时才发现回不去，
/// 那时原件已经没了。因此在真实文件上验证「备份 → 破坏 → 还原 → 逐字节比对」。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn every_real_project_backs_up_and_restores_byte_for_byte() {
    let files = collect_litematic(&real_dir());
    let exe_dir = work_dir("backup_exe");
    let work = work_dir("backup_src");
    let mut failed: Vec<String> = Vec::new();
    let mut checked = 0;

    for src in &files {
        let name = file_name(src);
        // 在实际会被改写的那个文件上操作：先拷到工作目录，避免动到原始收藏
        let target = work.join(&name);
        std::fs::copy(src, &target).expect("拷贝到工作目录失败");
        let target_s = target.to_string_lossy().to_string();
        let original_bytes = std::fs::read(&target).expect("读取失败");

        // 1) 备份
        let outcome = match crate::backup::create(&exe_dir, &target, "test") {
            Ok(o) => o,
            Err(e) => {
                failed.push(format!("{name}: 备份失败 {e}"));
                continue;
            }
        };
        if !outcome.backup_path.is_file() {
            failed.push(format!("{name}: 备份文件不存在"));
            continue;
        }
        if std::fs::read(&outcome.backup_path).expect("读备份失败") != original_bytes {
            failed.push(format!("{name}: 备份内容与原件不一致"));
            continue;
        }

        // 2) 让它出现在列表里（备份管理的入口就是 list）
        let listed = crate::backup::list(&crate::backup::backup_root(&exe_dir));
        if !listed.iter().any(|e| e.path == outcome.backup_path) {
            failed.push(format!("{name}: 备份未出现在 list() 里，用户看不到"));
        }

        // 3) 破坏文件：真的跑一遍替换并写回，模拟用户的正常操作
        if let Ok(mut sc) = crate::litematic::Schematic::load(&target_s) {
            let db = bundled_db();
            if let Some(vi) = db.pick_version(sc.summary().data_version) {
                let info = crate::block_model::DbBlockInfo { db: &db, version: &db.versions[vi] };
                let before = sc.analyze();
                if let Some((key, _)) = before.blocks_by_state.iter().max_by_key(|(_, n)| **n) {
                    let from = key.split('[').next().unwrap_or(key).to_string();
                    if !from.is_empty() && !from.ends_with(":air") {
                        let rule = crate::replace::ReplaceRule {
                            from_name: from,
                            to_name: "minecraft:stone".into(),
                            ..Default::default()
                        };
                        let out = crate::replace::replace_blocks(&mut sc, &rule, &info, 0);
                        if out.replaced > 0 {
                            let _ = sc.save_to(&target_s);
                        }
                    }
                }
            }
        }
        let damaged = std::fs::read(&target).expect("读改写后文件失败");
        if damaged == original_bytes {
            // 没能改成也不影响还原结论，但这样就没验到「被破坏后能还原」
            println!("  [跳过] {name}（未能造成改动，无法验证还原）");
            let _ = crate::backup::delete(&outcome.backup_path);
            continue;
        }

        // 4) 还原，必须逐字节回到原样
        if let Err(e) = crate::backup::restore(&outcome.backup_path, &target) {
            failed.push(format!("{name}: 还原失败 {e}"));
            continue;
        }
        let restored = std::fs::read(&target).expect("读还原后文件失败");
        if restored != original_bytes {
            failed.push(format!(
                "{name}: 还原后与原件不一致（{} 字节 vs {} 字节）",
                restored.len(),
                original_bytes.len()
            ));
            continue;
        }
        // 还原出来的文件必须仍是合法投影，而不是「字节对了但读不了」
        if crate::litematic::Schematic::load(&target_s).is_err() {
            failed.push(format!("{name}: 还原后无法解析"));
        }

        // 5) 删除备份
        if let Err(e) = crate::backup::delete(&outcome.backup_path) {
            failed.push(format!("{name}: 删除备份失败 {e}"));
        }

        checked += 1;
        println!("  [OK]   {name}  备份 → 改写 → 还原，逐字节一致");
    }

    let _ = std::fs::remove_dir_all(&exe_dir);
    let _ = std::fs::remove_dir_all(&work);
    println!("\n  实际验证 {checked} 个，未通过 {}", failed.len());
    assert!(checked > 0, "没有一个文件走完备份还原，这组用例没验到东西");
    assert!(failed.is_empty(), "以下文件未通过：\n{}", failed.join("\n"));
}
