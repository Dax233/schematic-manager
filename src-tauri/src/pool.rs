//! 资源池：按内容去重。
//!
//! 用途：构建资源包时，同一个模型会被大量方块与变体引用，同一张贴图也会
//! 被多个模型引用。按内容去重能显著缩小产物体积，也避免读取端重复解析。
//!
//! 对照 legacy-cpp/packbuild.cpp 的 texByHash 与 modelByHash。
//!
//! 去重键必须是**内容**而非名称：不同路径的贴图内容可能相同（复制粘贴的
//! 资源包很常见），同名不同内容的情形也存在。
//!
//! 纯逻辑，不涉及文件 IO，因此可完整测试。

use std::collections::HashMap;

/// 内容指纹：64 位 FNV-1a。
///
/// 只用于在哈希表里快速定位候选，**不承担相等判定**——真正的相等由
/// 内容本身逐字节比较得出。哈希碰撞若被当成相等，会让某个模型用错贴图，
/// 这种错误不会崩溃、只会让画面出错，因此必须避免。
fn fingerprint(data: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// 按内容去重的资源池。
///
/// 池里同时保存「内容的字节表示」与「解析后的项」，因此相等判定是
/// 逐字节比较内容，而不是比较指针或指纹。
#[derive(Debug, Clone)]
pub struct ContentPool<T> {
    items: Vec<T>,
    /// 与 items 一一对应的内容字节
    keys: Vec<Vec<u8>>,
    /// 指纹 -> 该指纹下的项下标（可能多个，指纹可能碰撞）
    by_hash: HashMap<u64, Vec<usize>>,
}

impl<T> Default for ContentPool<T> {
    fn default() -> Self {
        Self { items: Vec::new(), keys: Vec::new(), by_hash: HashMap::new() }
    }
}

impl<T> ContentPool<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// 取内容对应的下标；不存在则调用 `make` 构造并加入。
    ///
    /// 相等判定是 `key` 的逐字节比较：同指纹只说明「可能相同」，
    /// 必须再比内容才能确认。
    pub fn intern_by_key<F>(&mut self, key: &[u8], make: F) -> usize
    where
        F: FnOnce() -> T,
    {
        let h = fingerprint(key);
        if let Some(cands) = self.by_hash.get(&h) {
            for &i in cands {
                if self.keys[i] == key {
                    return i;
                }
            }
        }
        let idx = self.items.len();
        self.items.push(make());
        self.keys.push(key.to_vec());
        self.by_hash.entry(h).or_default().push(idx);
        idx
    }

    /// 取内容对应的下标；不存在则加入 `item` 本身
    pub fn intern(&mut self, key: &[u8], item: T) -> usize {
        self.intern_by_key(key, || item)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }

    #[cfg(test)]
    pub fn get(&self, i: usize) -> Option<&T> {
        self.items.get(i)
    }

    /// 内容对应的下标（只查不建）
    #[cfg(test)]
    pub fn find(&self, key: &[u8]) -> Option<usize> {
        let h = fingerprint(key);
        let cands = self.by_hash.get(&h)?;
        cands.iter().copied().find(|&i| self.keys[i] == key)
    }

    /// 池中全部内容的总字节数，供调用方判断体积
    #[cfg(test)]
    pub fn total_key_bytes(&self) -> usize {
        self.keys.iter().map(|k| k.len()).sum()
    }
}

/// 把模型编码成用于去重的字节串。
///
/// 对照 legacy-cpp 的 ModelHash：逐元素、逐面地把影响渲染结果的字段
/// 全部纳入。**小数点后保留 3 位**是原实现的做法，也符合模型精度的
/// 实际量级（Minecraft 模型坐标为 1/16 方块，3 位足够区分）。
///
/// 未纳入的字段会影响去重正确性：漏一个就会把本应不同的两个模型合并，
/// 表现为某些方块的朝向或贴图错误。因此这里逐一列出所有渲染相关字段。
pub fn model_key(m: &crate::blockdb::Model) -> Vec<u8> {
    let mut s = String::new();
    s.push(if m.translucent { '1' } else { '0' });
    for e in &m.elements {
        // 位置与尺寸
        s.push_str(&format!(
            "|{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}",
            e.from[0], e.from[1], e.from[2], e.to[0], e.to[1], e.to[2]
        ));
        // 旋转
        s.push_str(&format!(
            ",{},{}", if e.has_rotation { 1 } else { 0 }, e.rot_kind
        ));
        if e.has_rotation {
            s.push_str(&format!(
                ",{:.3},{:.3},{:.3},{},{:.3},{}",
                e.origin[0], e.origin[1], e.origin[2], e.axis, e.angle,
                if e.rescale { 1 } else { 0 }
            ));
            s.push_str(&format!(
                ",{:.3},{:.3},{:.3}",
                e.rot_vec[0], e.rot_vec[1], e.rot_vec[2]
            ));
        }
        s.push_str(&format!(",{}", e.face_mask));
        // 面：按顺序参与，因为读取端就是按 faceMask 的位序取面的
        for f in &e.faces {
            s.push_str(&format!(
                "#{},{}", f.texture, if f.cullface == 0xFF { -1 } else { f.cullface as i32 }
            ));
            s.push_str(&format!(
                ",{:.3},{:.3},{:.3},{:.3},{},{}",
                f.uv[0], f.uv[1], f.uv[2], f.uv[3], f.tintindex, f.rotation
            ));
        }
    }
    s.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_content_is_stored_once() {
        let mut p: ContentPool<String> = ContentPool::new();
        let a = p.intern(b"same", "first".to_string());
        let b = p.intern(b"same", "second".to_string());
        assert_eq!(a, b, "内容相同应复用同一下标");
        assert_eq!(p.len(), 1);
        assert_eq!(p.get(a).unwrap(), "first", "应保留先加入的那份");
    }

    #[test]
    fn different_content_gets_separate_slots() {
        let mut p: ContentPool<String> = ContentPool::new();
        let a = p.intern(b"one", "one".to_string());
        let b = p.intern(b"two", "two".to_string());
        assert_ne!(a, b);
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn same_length_but_different_content_is_not_merged() {
        // 关键：长度相同、仅个别字节不同的内容绝不能被当成同一份。
        // 若只比长度或指纹，这里会错误复用，进而让模型用错贴图。
        let mut p: ContentPool<u32> = ContentPool::new();
        let a = p.intern(b"abcd", 1);
        let b = p.intern(b"abce", 2);
        assert_ne!(a, b, "仅末字节不同也必须是两项");
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn intern_by_key_constructs_lazily() {
        let mut p: ContentPool<Vec<u8>> = ContentPool::new();
        let mut built = 0;
        for _ in 0..3 {
            p.intern_by_key(b"k", || {
                built += 1;
                vec![1, 2, 3]
            });
        }
        assert_eq!(built, 1, "内容相同只应构造一次");
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn indices_are_stable_across_insertions() {
        let mut p: ContentPool<u32> = ContentPool::new();
        let first = p.intern(b"a", 1);
        let _ = p.intern(b"b", 2);
        let _ = p.intern(b"c", 3);
        assert_eq!(p.intern(b"a", 99), first);
        assert_eq!(p.get(first), Some(&1));
    }

    #[test]
    fn find_does_not_insert() {
        let mut p: ContentPool<u32> = ContentPool::new();
        p.intern(b"a", 1);
        assert_eq!(p.find(b"a"), Some(0));
        assert_eq!(p.find(b"missing"), None);
        assert_eq!(p.len(), 1, "find 不应改变池");
    }

    #[test]
    fn empty_pool_is_empty() {
        let p: ContentPool<u32> = ContentPool::new();
        assert!(p.is_empty());
        assert_eq!(p.len(), 0);
        assert!(p.items().is_empty());
        assert!(p.get(0).is_none());
        assert_eq!(p.total_key_bytes(), 0);
    }

    #[test]
    fn empty_key_is_still_deduplicated() {
        let mut p: ContentPool<u32> = ContentPool::new();
        let a = p.intern(b"", 1);
        let b = p.intern(b"", 2);
        assert_eq!(a, b);
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn fingerprint_is_stable_and_distinguishes_content() {
        assert_eq!(fingerprint(b"abc"), fingerprint(b"abc"));
        assert_ne!(fingerprint(b"abc"), fingerprint(b"abd"));
        // 与 FNV-1a 64 位的标准值一致
        assert_eq!(fingerprint(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fingerprint(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn many_distinct_items_keep_correct_mapping() {
        let mut p: ContentPool<usize> = ContentPool::new();
        let mut expect = Vec::new();
        for i in 0..200usize {
            let key = format!("key-{i}");
            expect.push(p.intern(key.as_bytes(), i));
        }
        assert_eq!(p.len(), 200);
        for i in 0..200usize {
            let key = format!("key-{i}");
            assert_eq!(p.intern(key.as_bytes(), 9999), expect[i], "第 {i} 项未命中");
        }
        assert_eq!(p.len(), 200, "重复查询不应增长池");
    }

    #[test]
    fn items_preserve_insertion_order() {
        let mut p: ContentPool<&str> = ContentPool::new();
        p.intern(b"1", "one");
        p.intern(b"2", "two");
        p.intern(b"3", "three");
        assert_eq!(p.items(), &["one", "two", "three"]);
    }

    #[test]
    fn total_key_bytes_sums_contents() {
        let mut p: ContentPool<u32> = ContentPool::new();
        p.intern(b"abc", 1);
        p.intern(b"de", 2);
        assert_eq!(p.total_key_bytes(), 5);
    }

    // ---- 模型去重键 ----

    fn face(texture: u32, cullface: u8) -> crate::blockdb::ModelFace {
        crate::blockdb::ModelFace {
            texture,
            uv: [0.0, 0.0, 16.0, 16.0],
            cullface,
            tintindex: -1,
            rotation: 0,
        }
    }

    fn model(translucent: bool, faces: Vec<crate::blockdb::ModelFace>, mask: u8) -> crate::blockdb::Model {
        crate::blockdb::Model {
            translucent,
            elements: vec![crate::blockdb::ModelElement {
                from: [0.0, 0.0, 0.0],
                to: [16.0, 16.0, 16.0],
                has_rotation: false,
                rot_kind: 0,
                origin: [0.0; 3],
                axis: 0,
                angle: 0.0,
                rescale: false,
                rot_vec: [0.0; 3],
                face_mask: mask,
                faces,
            }],
        }
    }

    #[test]
    fn model_key_is_equal_for_equal_models() {
        let a = model(false, vec![face(1, 0xFF)], 0b1);
        let b = model(false, vec![face(1, 0xFF)], 0b1);
        assert_eq!(model_key(&a), model_key(&b));
    }

    #[test]
    fn model_key_distinguishes_every_rendered_field() {
        let base = model(false, vec![face(1, 0xFF)], 0b1);
        let base_key = model_key(&base);

        // 半透明标志
        let mut m = base.clone();
        m.translucent = true;
        assert_ne!(model_key(&m), base_key, "translucent 应影响去重键");

        // 贴图下标
        let mut m = base.clone();
        m.elements[0].faces[0].texture = 2;
        assert_ne!(model_key(&m), base_key, "贴图下标应影响去重键");

        // cullface
        let mut m = base.clone();
        m.elements[0].faces[0].cullface = 2;
        assert_ne!(model_key(&m), base_key, "cullface 应影响去重键");

        // UV
        let mut m = base.clone();
        m.elements[0].faces[0].uv[2] = 8.0;
        assert_ne!(model_key(&m), base_key, "UV 应影响去重键");

        // tintindex
        let mut m = base.clone();
        m.elements[0].faces[0].tintindex = 0;
        assert_ne!(model_key(&m), base_key, "tintindex 应影响去重键");

        // 面旋转
        let mut m = base.clone();
        m.elements[0].faces[0].rotation = 90;
        assert_ne!(model_key(&m), base_key, "面旋转应影响去重键");

        // faceMask
        let mut m = base.clone();
        m.elements[0].face_mask = 0b11;
        assert_ne!(model_key(&m), base_key, "faceMask 应影响去重键");

        // 几何
        let mut m = base.clone();
        m.elements[0].to = [8.0, 16.0, 16.0];
        assert_ne!(model_key(&m), base_key, "几何尺寸应影响去重键");
    }

    #[test]
    fn model_key_distinguishes_element_rotation() {
        let mut a = model(false, vec![face(1, 0xFF)], 0b1);
        a.elements[0].has_rotation = true;
        a.elements[0].axis = 1;
        a.elements[0].angle = 45.0;
        let ka = model_key(&a);

        let mut b = a.clone();
        b.elements[0].angle = 90.0;
        assert_ne!(model_key(&b), ka, "旋转角度应影响去重键");

        let mut c = a.clone();
        c.elements[0].rescale = true;
        assert_ne!(model_key(&c), ka, "rescale 应影响去重键");

        let mut d = a.clone();
        d.elements[0].rot_kind = 1;
        d.elements[0].rot_vec = [90.0, 0.0, 0.0];
        assert_ne!(model_key(&d), ka, "多轴旋转应影响去重键");
    }

    #[test]
    fn model_key_is_usable_as_pool_key() {
        // 两个内容相同的模型应合并，不同的应分开
        let m1 = model(false, vec![face(1, 0xFF)], 0b1);
        let m2 = model(false, vec![face(1, 0xFF)], 0b1);
        let m3 = model(false, vec![face(2, 0xFF)], 0b1);

        let mut pool: ContentPool<usize> = ContentPool::new();
        let i1 = pool.intern(&model_key(&m1), 1);
        let i2 = pool.intern(&model_key(&m2), 2);
        let i3 = pool.intern(&model_key(&m3), 3);
        assert_eq!(i1, i2, "相同模型应合并");
        assert_ne!(i1, i3, "不同模型应分开");
        assert_eq!(pool.len(), 2);
    }
}
