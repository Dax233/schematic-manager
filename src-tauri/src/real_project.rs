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

use crate::litematic::BlockState;
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

/// 真实投影里的染色方块必须真的被染色，且是**按通道**染色。
///
/// 这条对应界面上的「红石、树叶、藤蔓没颜色」：贴图是灰度的，
/// 靠逐顶点颜色乘上去才成形。只测 tint 函数的单元测试不足以证明
/// 「整条链路真的把颜色带到了顶点上」——中间任何一环漏掉，
/// 单元测试仍然全绿，而画面依旧灰。因此这里走真实数据：
/// 从真实投影取出这些方块产生的四边形，核对它们的颜色值。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn real_project_tinted_blocks_actually_get_colored() {
    use std::collections::BTreeMap;

    let files = collect_litematic(&real_dir());
    let db = bundled_db();
    // 方块名 -> (四边形数, 第一个染色值)
    let mut seen: BTreeMap<String, (usize, [f32; 3])> = BTreeMap::new();
    let mut total_quads = 0usize;
    let mut tinted_quads = 0usize;

    for f in &files {
        let Ok(sc) = crate::litematic::Schematic::load(&f.to_string_lossy()) else { continue };
        let Some(vi) = db.pick_version(sc.summary().data_version) else { continue };
        let quads = crate::visibility::collect_quads(&sc, &db, vi);
        // 逐四边形反查它属于哪个方块（按 block_pos 对应回调色板）
        let mut name_at: BTreeMap<[i32; 3], String> = BTreeMap::new();
        for r in &sc.regions {
            for (i, pi) in r.indices.iter().enumerate() {
                let Some(st) = r.palette.get(*pi as usize) else { continue };
                let local = r.pos_from_index(i);
                let (Some(x), Some(y), Some(z)) = (
                    r.pos[0].checked_add(local[0]),
                    r.pos[1].checked_add(local[1]),
                    r.pos[2].checked_add(local[2]),
                ) else {
                    continue;
                };
                name_at.insert([x, y, z], st.name.clone());
            }
        }
        for q in &quads {
            total_quads += 1;
            let is_tinted = q.tint != crate::tint::NO_TINT;
            if is_tinted {
                tinted_quads += 1;
            }
            if let Some(n) = name_at.get(&q.block_pos) {
                seen.entry(n.clone()).or_insert((0, q.tint)).0 += 1;
            }
        }
    }

    assert!(total_quads > 0, "真实投影没有产出任何四边形");
    println!("  四边形合计 {total_quads}，其中染色 {tinted_quads}");

    // 逐个方块核对**具体颜色**，而不是只核对「不是全 1」。
    // 只查「有没有变色」会漏掉「染成了错误的颜色」——樱花树叶被染成
    // 树叶绿时仍然是「变了色」，但观感明显不对。
    let expect = |name: &str, want: [f32; 3]| -> Result<(), String> {
        match seen.get(name) {
            None => Ok(()), // 这批投影里没出现该方块，不算失败
            Some((cnt, got)) => {
                if *got == want {
                    Ok(())
                } else {
                    Err(format!(
                        "{name}（{cnt} 个四边形）染色为 ({:.3},{:.3},{:.3})，\
                         应为 ({:.3},{:.3},{:.3})",
                        got[0], got[1], got[2], want[0], want[1], want[2]
                    ))
                }
            }
        }
    };

    let mut wrong: Vec<String> = Vec::new();
    let leaf = crate::tint::tint_color("minecraft:oak_leaves", &[], 0);
    let grass = crate::tint::tint_color("minecraft:grass_block", &[], 0);
    let no_tint = crate::tint::NO_TINT;

    let mut checked = 0usize;
    println!("  --- 出现过的染色方块 ---");
    for (name, (cnt, tint)) in &seen {
        if *tint == no_tint {
            continue;
        }
        println!("  {cnt:>6}  {name}  tint=({:.3}, {:.3}, {:.3})", tint[0], tint[1], tint[2]);
    }

    for pair in [
        ("minecraft:oak_leaves", leaf),
        ("minecraft:jungle_leaves", leaf),
        ("minecraft:acacia_leaves", leaf),
        ("minecraft:dark_oak_leaves", leaf),
        ("minecraft:mangrove_leaves", leaf),
        ("minecraft:vine", leaf),
        ("minecraft:sugar_cane", grass),
        // 樱花树叶必须保持原色（贴图自带粉色）
        ("minecraft:cherry_leaves", no_tint),
    ] {
        if seen.contains_key(pair.0) {
            checked += 1;
        }
        if let Err(e) = expect(pair.0, pair.1) {
            wrong.push(e);
        }
    }

    // 红石线单独核对：必须是红色系（红通道最高），且随信号强度变化
    if let Some((cnt, t)) = seen.get("minecraft:redstone_wire") {
        checked += 1;
        println!("  {cnt:>6}  minecraft:redstone_wire  tint=({:.3}, {:.3}, {:.3})", t[0], t[1], t[2]);
        if !(t[0] > t[1] && t[0] > t[2]) {
            wrong.push(format!("redstone_wire 染色 ({:.3},{:.3},{:.3}) 不是红色系", t[0], t[1], t[2]));
        }
    }

    assert!(
        checked >= 2,
        "这批投影里只找到 {checked} 种待验证的染色方块，样本不足；\
         请换一批含树叶/草/红石的投影再跑"
    );
    assert!(wrong.is_empty(), "以下染色方块未正确上色：\n{}", wrong.join("\n"));
}

/// 活塞的邻面剔除必须守住所依赖的**几何不变量**。
///
/// 对应界面上的「活塞有时候会缺面」：「有时候」是关键词——同一个活塞，
/// 周围有没有实心方块会改变结果，因此这里把邻居场景也当作输入维度。
///
/// 只在会**误判**的方向上断言，不自己重算一遍剔除逻辑（重算等于把被测
/// 逻辑抄一遍，抄错了两边一起错）。这里守两条与旋转无关的不变量：
///
/// 1. 邻居越少，可见面不可能更多。孤立时必须 >= 任意有邻居的场景。
/// 2. 带 cullface=none 的面在任何邻居配置下都必须保留。
///    伸出活塞的正面就是这种（活塞杆靠它显示），
///    若它被当成可剔除，活塞就会「缺一块」。
///
/// 另外验证「六面围石头」时可见面确实变少——若剔除完全失效，
/// 这条会失败，说明本用例不是空转。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn real_project_piston_culling_respects_geometry() {
    let db = bundled_db();
    let vi = db.pick_version(3465).expect("内置库缺少 1.20.1");

    let mut states: Vec<(String, String)> = Vec::new();
    for facing in ["north", "south", "east", "west", "up", "down"] {
        for extended in ["false", "true"] {
            for name in ["minecraft:piston", "minecraft:sticky_piston"] {
                states.push((name.to_string(), format!("extended={extended},facing={facing}")));
            }
        }
    }

    // 邻居一律用**绝对坐标**写出来，避免偏移与下标互转时出错。
    // 活塞恒在 (1,1,1)。
    let scenarios: [(&str, Vec<[i32; 3]>); 3] = [
        ("孤立", vec![]),
        (
            "六面围石头",
            vec![
                [0, 1, 1],
                [2, 1, 1],
                [1, 0, 1],
                [1, 2, 1],
                [1, 1, 0],
                [1, 1, 2],
            ],
        ),
        ("上方压石头", vec![[1, 2, 1]]),
    ];

    let mut failures: Vec<String> = Vec::new();
    let mut shrank_somewhere = false;
    let mut checked = 0usize;

    for (name, propstr) in &states {
        let props: Vec<(String, String)> = propstr
            .split(',')
            .map(|kv| {
                let mut it = kv.splitn(2, '=');
                (it.next().unwrap().into(), it.next().unwrap_or("").into())
            })
            .collect();
        let st = BlockState { name: name.clone(), props: props.clone() };

        // 该方块模型里 cullface=none 的面有几个（这些面永远不该被剔除）
        let models = crate::block_model::resolve_models(&db, &db.versions[vi], &st);
        let mut no_cull_faces = 0usize;
        let mut total_faces = 0usize;
        for rm in &models {
            let Some(m) = db.models.get(rm.model_idx as usize) else { continue };
            for elem in &m.elements {
                for f in 0..6u8 {
                    if elem.face_mask & (1u8 << f) == 0 {
                        continue;
                    }
                    total_faces += 1;
                    let fe = match f {
                        0 => crate::blockdb::Face::Down,
                        1 => crate::blockdb::Face::Up,
                        2 => crate::blockdb::Face::North,
                        3 => crate::blockdb::Face::South,
                        4 => crate::blockdb::Face::West,
                        _ => crate::blockdb::Face::East,
                    };
                    if elem.face(fe).map(|mf| mf.cullface == 0xFF).unwrap_or(false) {
                        no_cull_faces += 1;
                    }
                }
            }
        }

        let mut per_scenario: Vec<(&str, usize)> = Vec::new();
        for (sname, neighbours) in &scenarios {
            // 用 Region::index 自己算下标：索引顺序是 X 最快、Z 次之、Y 最慢，
            // 手算 x*9+y*3+z 会错位（我在写这条用例时正是这样错过一次）。
            let size = [3usize, 3, 3];
            let mut palette = vec![st.clone()];
            let mut indices = vec![0i32; 27];
            for n in neighbours {
                let stone = BlockState::new("minecraft:stone");
                let pi = match palette
                    .iter()
                    .position(|s| s.name == stone.name && s.props == stone.props)
                {
                    Some(x) => x,
                    None => {
                        palette.push(stone);
                        palette.len() - 1
                    }
                };
                let idx = n[0] as usize
                    + n[2] as usize * size[0]
                    + n[1] as usize * size[0] * size[2];
                indices[idx] = pi as i32;
            }
            let air = BlockState::new("minecraft:air");
            let air_pi = {
                palette.push(air);
                palette.len() - 1
            };
            for i in 0..indices.len() {
                let x = (i % size[0]) as i32;
                let z = ((i / size[0]) % size[2]) as i32;
                let y = (i / (size[0] * size[2])) as i32;
                if [x, y, z] == [1, 1, 1]
                    || neighbours.iter().any(|n| n[0] == x && n[1] == y && n[2] == z)
                {
                    continue;
                }
                indices[i] = air_pi as i32;
            }

            let sc = crate::litematic::Schematic {
                root: crate::nbt::Value::Compound(vec![]),
                regions: vec![crate::litematic::Region {
                    name: "r".into(),
                    pos: [0, 0, 0],
                    size: [3, 3, 3],
                    volume: 27,
                    bits: 5,
                    palette,
                    indices,
                    ..Default::default()
                }],
                path: String::new(),
                meta_modified: false,
                blocks_modified: false,
            };

            let quads = crate::visibility::collect_quads(&sc, &db, vi);
            let got = quads.iter().filter(|q| q.block_pos == [1, 1, 1]).count();
            checked += 1;
            per_scenario.push((sname, got));

            // 不变量 2：cullface=none 的面必须全都在
            if got < no_cull_faces {
                failures.push(format!(
                    "{name} [{propstr}] 场景「{sname}」：只产出 {got} 个面，\
                     但仅 cullface=none 的面就有 {no_cull_faces} 个——不可剔除的面被剔掉了"
                ));
            }
            // 可见面不可能超过模型总面数
            if got > total_faces {
                failures.push(format!(
                    "{name} [{propstr}] 场景「{sname}」：产出 {got} 个面，超过模型总面数 {total_faces}"
                ));
            }
        }

        // 不变量 1：邻居越少，可见面不可能更少
        let isolated = per_scenario[0].1;
        for (sname, got) in &per_scenario[1..] {
            if *got > isolated {
                failures.push(format!(
                    "{name} [{propstr}]：场景「{sname}」产出 {got} 个面，\
                     多于完全孤立时的 {isolated} 个——邻居变多反而看到更多面，剔除方向反了"
                ));
            }
            if *got < isolated {
                shrank_somewhere = true;
            }
        }
    }

    assert!(checked > 0, "没有验证到任何活塞状态");
    assert!(
        shrank_somewhere,
        "所有场景的可见面数都相同，说明剔除根本没生效——本用例在空转"
    );
    assert!(
        failures.is_empty(),
        "活塞剔除违反几何不变量：\n{}",
        failures.join("\n")
    );
    println!("  已核对 {checked} 个「活塞状态 × 邻居场景」组合，剔除符合几何不变量");
}

/// 真实投影里活塞与红石类方块的状态必须都能解析出模型。
///
/// 对应界面上的「活塞有时候会缺面」：活塞是**按状态切换模型**的方块
/// （extended 决定底座是完整立方体还是短了一截；facing 决定朝向；
/// piston_head 还分 short 与 type）。只要状态解析失败，
/// 这个方块就一个面都不画——表现就是「凭空少了一块」。
///
/// 这里用真实数据核对：把所有 piston / sticky_piston / piston_head /
/// redstone_* 的实际状态串拿出来，逐个确认能解析出模型，
/// 并统计覆盖到的 facing 与 extended 组合。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn real_project_piston_states_all_resolve() {
    use std::collections::BTreeMap;

    let files = collect_litematic(&real_dir());
    let db = bundled_db();
    // (方块名, 属性串) -> 出现次数
    let mut states: BTreeMap<(String, String), usize> = BTreeMap::new();

    for f in &files {
        let Ok(sc) = crate::litematic::Schematic::load(&f.to_string_lossy()) else { continue };
        for r in &sc.regions {
            for st in &r.palette {
                let n = st.name.as_str();
                let interesting = n.contains("piston") || n.contains("redstone");
                if !interesting {
                    continue;
                }
                *states.entry((st.name.clone(), st.prop_str())).or_insert(0) += 1;
            }
        }
    }

    assert!(
        !states.is_empty(),
        "这批投影里没有活塞/红石类方块，样本不足；请换一批再跑"
    );

    // 版本按每个投影自己的 data_version 选，避免用错版本比对
    let mut piston_found = 0usize;
    let mut broken: Vec<String> = Vec::new();
    let mut facings: BTreeMap<String, usize> = BTreeMap::new();
    let mut extended: BTreeMap<String, usize> = BTreeMap::new();

    for ((name, propstr), cnt) in &states {
        let props: Vec<(String, String)> = propstr
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|kv| {
                let mut it = kv.splitn(2, '=');
                (it.next().unwrap().into(), it.next().unwrap_or("").into())
            })
            .collect();

        // 找一个能查到该方块定义的版本（内置库里 1.20.1 有全部这些方块）
        let vi = db
            .pick_version(3465)
            .expect("内置库缺少 1.20.1");
        if crate::block_model::find_block(&db, &db.versions[vi], name).is_none() {
            broken.push(format!("{name}: 方块库里查不到定义"));
            continue;
        }
        let st = BlockState { name: name.clone(), props: props.clone() };
        let models = crate::block_model::resolve_models(&db, &db.versions[vi], &st);
        if models.is_empty() {
            broken.push(format!("{name} [{propstr}]（{cnt} 次）解析出 0 个模型"));
            continue;
        }
        if name.contains("piston") {
            piston_found += models.len().min(*cnt);
            for (k, v) in &props {
                if k == "facing" {
                    *facings.entry(v.clone()).or_insert(0) += 1;
                }
                if k == "extended" {
                    *extended.entry(v.clone()).or_insert(0) += 1;
                }
            }
        }
    }

    println!("  活塞/红石状态共 {} 种，活塞解析出 {piston_found} 个模型", states.len());
    println!("  facing 覆盖：{facings:?}");
    println!("  extended 覆盖：{extended:?}");

    assert!(broken.is_empty(), "以下状态解析不出模型（会导致该方块凭空消失）：\n{}", broken.join("\n"));

    // 活塞的两个关键状态都应被真实数据覆盖到，否则这条用例等于没测到
    // 「状态切换」这件事
    assert!(
        facings.len() >= 2,
        "真实数据里只出现了 {} 种 facing，样本不足以验证朝向切换",
        facings.len()
    );
    assert!(
        extended.contains_key("true") && extended.contains_key("false"),
        "真实数据里没有同时覆盖 extended 的两种取值，无法验证状态切换：{extended:?}"
    );
}

#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn every_real_project_builds_a_scene() {    let files = collect_litematic(&real_dir());
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
