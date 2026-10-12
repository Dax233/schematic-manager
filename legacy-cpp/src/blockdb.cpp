#include "blockdb.h"
#include "fs.h"
#include "str.h"
#include <zlib.h>

namespace mc {

const char* FaceName(int f) {
    static const char* n[] = { "down", "up", "north", "south", "west", "east" };
    return (f >= 0 && f < F_COUNT) ? n[f] : "?";
}

const ModelFace* ModelElement::Face(int f) const {
    int cnt = 0;
    for (int i = 0; i < F_COUNT; ++i) {
        if (faceMask & (1u << i)) {
            if (i == f) return &faces[(size_t)cnt];
            ++cnt;
        }
    }
    return nullptr;
}

const std::vector<std::string>* BlockDef::PropValues(const std::string& p) const {
    for (const auto& kv : props) if (kv.first == p) return &kv.second;
    return nullptr;
}
std::string BlockDef::DefaultProp(const std::string& p) const {
    const std::vector<std::string>* v = PropValues(p);
    return (v && !v->empty()) ? (*v)[0] : std::string();
}

// ------------------------------------------------------------------ 二进制读取
namespace {

struct BinReader {
    const u8* p;
    const u8* end;
    bool ok = true;

    bool need(size_t n) { if ((size_t)(end - p) < n) { ok = false; return false; } return true; }
    u8  u8v()  { if (!need(1)) return 0; return *p++; }
    u16 u16v() { if (!need(2)) return 0; u16 v = (u16)((u16)p[0] | ((u16)p[1] << 8)); p += 2; return v; }
    i16 i16v() { return (i16)u16v(); }
    u32 u32v() {
        if (!need(4)) return 0;
        u32 v = (u32)p[0] | ((u32)p[1] << 8) | ((u32)p[2] << 16) | ((u32)p[3] << 24);
        p += 4; return v;
    }
    i32 i32v() { return (i32)u32v(); }
    float f32v() { u32 v = u32v(); float f; memcpy(&f, &v, 4); return f; }
    void raw(void* dst, size_t n) { if (!need(n)) return; memcpy(dst, p, n); p += n; }
};

// 解析 "a=1,b=2" 形式的 AND 条件
static void ParseConds(const std::string& when, std::vector<std::pair<std::string, std::string>>& out) {
    if (when.empty()) return;
    size_t pos = 0;
    while (pos <= when.size()) {
        size_t comma = when.find(',', pos);
        std::string one = (comma == std::string::npos) ? when.substr(pos) : when.substr(pos, comma - pos);
        size_t eq = one.find('=');
        if (eq != std::string::npos && eq > 0)
            out.emplace_back(one.substr(0, eq), one.substr(eq + 1));
        if (comma == std::string::npos) break;
        pos = comma + 1;
    }
}

static bool MatchAndConds(const std::vector<std::pair<std::string, std::string>>& conds,
                          const ltm::BlockState& st) {
    for (const auto& kv : conds)
        if (st.Prop(kv.first) != kv.second) return false;
    return true;
}

static bool MatchWhen(const std::string& when, const ltm::BlockState& st) {
    if (when.empty()) return true;
    std::vector<std::pair<std::string, std::string>> c;
    ParseConds(when, c);
    return MatchAndConds(c, st);
}

// 记录属性取值（用于方块替换时的合法性判断）
static void AddProps(BlockDef& bd, const std::string& when) {
    if (when.empty()) return;
    size_t altPos = 0;
    while (altPos <= when.size()) {
        size_t bar = when.find('|', altPos);
        std::string alt = (bar == std::string::npos) ? when.substr(altPos) : when.substr(altPos, bar - altPos);
        std::vector<std::pair<std::string, std::string>> conds;
        ParseConds(alt, conds);
        for (const auto& kv : conds) {
            bool found = false;
            for (auto& p : bd.props) {
                if (p.first == kv.first) {
                    found = true;
                    if (std::find(p.second.begin(), p.second.end(), kv.second) == p.second.end())
                        p.second.push_back(kv.second);
                    break;
                }
            }
            if (!found) bd.props.emplace_back(kv.first, std::vector<std::string>{ kv.second });
        }
        if (bar == std::string::npos) break;
        altPos = bar + 1;
    }
}

} // namespace

// ------------------------------------------------------------------ 加载
bool BlockDB::LoadFromMemory(const u8* data, size_t size, std::string& err) {
    versions_.clear(); models_.clear(); textures_.clear(); strings_.clear();
    langs_.clear(); langIndex_.clear(); texturePixels_.clear(); allBlockNames_.clear();
    resolveCache_.clear();
    packs_.clear(); overlaysByVersion_.clear(); globalTextureByPath_.clear();
    activeVersion_ = 0;

    BinReader r{ data, data + size, true };
    char magic[8] = {0};
    r.raw(magic, 8);
    if (memcmp(magic, "PMBLKDB", 7) != 0) { err = "方块数据库魔数不匹配"; return false; }
    u32 fmt = r.u32v();
    if (fmt != 3) { err = "方块数据库格式版本不支持 (" + std::to_string(fmt) + ")"; return false; }
    u32 versionCount = r.u32v();
    u32 textureCount = r.u32v();
    u32 modelCount   = r.u32v();
    u32 langCount    = r.u32v();
    if (!r.ok) { err = "方块数据库头部损坏"; return false; }
    if (versionCount > 100000 || textureCount > 2000000 || modelCount > 5000000 || langCount > 100000) {
        err = "方块数据库头部数值异常"; return false;
    }

    // ---- 字符串表
    u32 strCount = r.u32v();
    if (strCount > 20000000) { err = "字符串表数量异常"; return false; }
    strings_.reserve(strCount);
    for (u32 i = 0; i < strCount && r.ok; ++i) {
        const u8* start = r.p;
        while (r.p < r.end && *r.p != 0) ++r.p;
        if (r.p >= r.end) { r.ok = false; break; }
        strings_.emplace_back((const char*)start, (size_t)(r.p - start));
        ++r.p;
    }
    if (!r.ok) { err = "字符串表损坏"; return false; }

    // ---- 语言表（每个 section 前都有一个与 Header 重复的 u32 count）
    { u32 sec = r.u32v(); (void)sec; }
    langs_.resize(langCount);
    langIndex_.resize(langCount);
    for (u32 i = 0; i < langCount && r.ok; ++i) {
        u32 n = r.u32v();
        if (n > 5000000) { r.ok = false; break; }
        langs_[i].resize(n);
        langIndex_[i].reserve(n * 2);
        for (u32 k = 0; k < n && r.ok; ++k) {
            u32 bi = r.u32v(), ei = r.u32v(), zi = r.u32v();
            LangEntry& e = langs_[i][k];
            if (bi < strings_.size()) e.blockName = strings_[bi];
            if (ei < strings_.size()) e.en = strings_[ei];
            if (zi < strings_.size()) e.zh = strings_[zi];
            langIndex_[i][e.blockName] = k;
        }
    }
    if (!r.ok) { err = "语言表损坏"; return false; }

    // ---- 贴图
    { u32 sec = r.u32v(); (void)sec; }
    textures_.resize(textureCount);
    for (u32 i = 0; i < textureCount && r.ok; ++i) {
        textures_[i].w = r.u16v();
        textures_[i].h = r.u16v();
        textures_[i].offset = r.u32v();
        textures_[i].len = r.u32v();
        textures_[i].hasAlpha = r.u8v() != 0;
    }
    u32 rawSize = r.u32v();
    u32 compSize = r.u32v();
    if (!r.ok) { err = "贴图表头损坏"; return false; }
    if (compSize) {
        std::vector<u8> comp((size_t)compSize);
        r.raw(comp.data(), compSize);
        if (!r.ok) { err = "贴图数据截断"; return false; }
        texturePixels_.resize((size_t)rawSize ? rawSize : 1);
        uLongf outLen = (uLongf)texturePixels_.size();
        if (uncompress(texturePixels_.data(), &outLen, comp.data(), compSize) != Z_OK) {
            err = "贴图数据解压失败"; return false;
        }
        texturePixels_.resize(outLen);
    }

    // ---- 模型
    { u32 sec = r.u32v(); (void)sec; }
    models_.resize(modelCount);
    for (u32 i = 0; i < modelCount && r.ok; ++i) {
        Model& m = models_[i];
        m.translucent = (r.u8v() & 1) != 0;
        u16 ec = r.u16v();
        m.elements.resize(ec);
        for (u16 e = 0; e < ec && r.ok; ++e) {
            ModelElement& el = m.elements[e];
            for (int k = 0; k < 3; ++k) el.from[k] = r.f32v();
            for (int k = 0; k < 3; ++k) el.to[k] = r.f32v();
            el.hasRot = r.u8v() != 0;
            if (el.hasRot) {
                el.rotKind = r.u8v();
                for (int k = 0; k < 3; ++k) el.rotOrigin[k] = r.f32v();
                el.axis = r.u8v();
                el.angle = r.f32v();
                el.rescale = r.u8v() != 0;
                el.rx = r.f32v(); el.ry = r.f32v(); el.rz = r.f32v();
            }
            el.faceMask = r.u8v();
            int nf = 0;
            for (int b = 0; b < F_COUNT; ++b) if (el.faceMask & (1u << b)) ++nf;
            el.faces.resize((size_t)nf);
            for (int f = 0; f < nf && r.ok; ++f) {
                ModelFace& mf = el.faces[(size_t)f];
                mf.texStrIdx = r.u32v();
                for (int k = 0; k < 4; ++k) mf.uv[k] = r.f32v();
                mf.cullface = r.u8v();
                mf.tintindex = r.i16v();
                mf.rotation = r.u16v();
            }
        }
    }
    if (!r.ok) { err = "模型数据损坏"; return false; }

    // ---- 版本
    { u32 sec = r.u32v(); (void)sec; }
    versions_.resize(versionCount);
    for (u32 i = 0; i < versionCount && r.ok; ++i) {
        VersionDef& v = versions_[i];
        u32 idIdx = r.u32v();
        if (idIdx < strings_.size()) v.id = strings_[idIdx];
        v.dataVersion = r.i32v();
        v.langIdx = r.u32v();
        u32 tmc = r.u32v();
        if (tmc > 200000) { r.ok = false; break; }
        v.textureByPath.reserve(tmc * 2);
        for (u32 k = 0; k < tmc && r.ok; ++k) {
            u32 pIdx = r.u32v(), tIdx = r.u32v();
            if (pIdx < strings_.size()) v.textureByPath[strings_[pIdx]] = tIdx;
        }
        u32 bc = r.u32v();
        if (bc > 500000) { r.ok = false; break; }
        v.blocks.resize(bc);
        for (u32 b = 0; b < bc && r.ok; ++b) {
            BlockDef& bd = v.blocks[b];
            u32 nIdx = r.u32v();
            if (nIdx < strings_.size()) bd.name = strings_[nIdx];
            u16 vc = r.u16v();
            bd.variants.resize(vc);
            for (u16 k = 0; k < vc && r.ok; ++k) {
                VariantEntry& ve = bd.variants[k];
                u32 wIdx = r.u32v();
                if (wIdx < strings_.size()) ve.when = strings_[wIdx];
                ve.modelIdx = r.u32v();
                // 打包器存的是 90 度的倍数（0..3），这里换算成角度
                ve.rotX = (u8)(r.u8v() * 90);
                ve.rotY = (u8)(r.u8v() * 90);
                ve.uvlock = r.u8v() != 0;
            }
            u16 mc2 = r.u16v();
            bd.multipart.resize(mc2);
            for (u16 k = 0; k < mc2 && r.ok; ++k) {
                VariantEntry& ve = bd.multipart[k];
                u32 wIdx = r.u32v();
                if (wIdx < strings_.size()) ve.when = strings_[wIdx];
                ve.modelIdx = r.u32v();
                ve.rotX = (u8)(r.u8v() * 90);
                ve.rotY = (u8)(r.u8v() * 90);
                ve.uvlock = r.u8v() != 0;
            }
            v.blockByName[bd.name] = b;
        }
    }
    if (!r.ok) { err = "版本数据损坏"; return false; }

    // ---- 收尾：派生索引
    RebuildDerived();
    return true;
}

// 重建全部派生索引（首次加载与合并资源包后都要调用）
void BlockDB::RebuildDerived() {
    // 语言表索引
    langIndex_.assign(langs_.size(), {});
    for (size_t i = 0; i < langs_.size(); ++i) {
        langIndex_[i].clear();
        langIndex_[i].reserve(langs_[i].size() * 2);
        for (size_t k = 0; k < langs_[i].size(); ++k)
            langIndex_[i][langs_[i][k].blockName] = (u32)k;
    }

    // 属性集合 + 全局方块名表 + 全局贴图索引
    globalTextureByPath_.clear();
    std::set<std::string> names;
    for (size_t vi = 0; vi < versions_.size(); ++vi) {
        VersionDef& v = versions_[vi];
        for (BlockDef& bd : v.blocks) {
            names.insert(bd.name);
            for (const VariantEntry& ve : bd.variants)  AddProps(bd, ve.when);
            for (const VariantEntry& ve : bd.multipart) AddProps(bd, ve.when);
        }
        for (const auto& kv : v.textureByPath)
            if (!globalTextureByPath_.count(kv.first)) globalTextureByPath_[kv.first] = kv.second;
    }
    allBlockNames_.assign(names.begin(), names.end());

    // 模组叠加关系：叠加包挂到 dataVersion 最接近的基础版本上
    overlaysByVersion_.assign(versions_.size(), {});
    for (size_t i = 0; i < versions_.size(); ++i) {
        if (!versions_[i].isOverlay) continue;
        int base = -1;
        i64 best = -1;
        for (size_t j = 0; j < versions_.size(); ++j) {
            if (versions_[j].isOverlay) continue;
            i64 d = (i64)versions_[j].dataVersion - (i64)versions_[i].dataVersion;
            if (d < 0) d = -d;
            if (best < 0 || d < best) { best = d; base = (int)j; }
        }
        if (base >= 0) overlaysByVersion_[(size_t)base].push_back((u32)i);
    }

    resolveCache_.clear();
    activeVersion_ = 0;
}

bool BlockDB::LoadFromFile(const std::wstring& path, std::string& err) {
    std::vector<u8> raw;
    if (!fsx::ReadAllBytes(path, raw)) { err = "无法读取方块数据库文件"; return false; }
    return LoadFromMemory(raw.data(), raw.size(), err);
}

bool BlockDB::LoadEmbedded(int resId, std::string& err) {
    HRSRC hr = FindResourceW(nullptr, MAKEINTRESOURCEW(resId), RT_RCDATA);
    if (!hr) { err = "找不到内置方块数据库资源"; return false; }
    HGLOBAL hg = LoadResource(nullptr, hr);
    if (!hg) { err = "加载资源失败"; return false; }
    const u8* p = (const u8*)LockResource(hg);
    DWORD sz = SizeofResource(nullptr, hr);
    if (!p || sz < 16) { err = "资源为空"; return false; }

    std::vector<u8> plain((size_t)sz * 24 + (1u << 22));
    uLongf outLen = (uLongf)plain.size();
    int zr = uncompress(plain.data(), &outLen, p, sz);
    for (int tries = 0; zr == Z_BUF_ERROR && tries < 4; ++tries) {
        plain.resize(plain.size() * 3 + (1u << 22));
        outLen = (uLongf)plain.size();
        zr = uncompress(plain.data(), &outLen, p, sz);
    }
    if (zr != Z_OK) { err = "内置方块数据库解压失败 (" + std::to_string(zr) + ")"; return false; }
    plain.resize(outLen);
    return LoadFromMemory(plain.data(), plain.size(), err);
}

// ------------------------------------------------------------------ 查询
int BlockDB::PickVersion(i32 dv) const {
    if (versions_.empty()) return -1;
    int best = -1;
    i64 bestDiff = -1;
    for (size_t i = 0; i < versions_.size(); ++i) {
        if (versions_[i].isOverlay) continue;      // 叠加包不参与版本选择
        i64 d = (i64)versions_[i].dataVersion - (i64)dv;
        if (d < 0) d = -d;
        if (bestDiff < 0 || d < bestDiff) { bestDiff = d; best = (int)i; }
    }
    return best;
}

int BlockDB::FindBaseVersion(i32 dv) const { return PickVersion(dv); }

// 查找贴图：本版本 -> 叠加包 -> 全局
u32 BlockDB::FindTexture(const VersionDef& v, const std::string& path) const {
    auto it = v.textureByPath.find(path);
    if (it != v.textureByPath.end()) return it->second;
    size_t vi = (size_t)(&v - &versions_[0]);
    if (vi < overlaysByVersion_.size()) {
        for (u32 ov : overlaysByVersion_[vi]) {
            if (ov >= versions_.size()) continue;
            auto o = versions_[ov].textureByPath.find(path);
            if (o != versions_[ov].textureByPath.end()) return o->second;
        }
    }
    auto g = globalTextureByPath_.find(path);
    if (g != globalTextureByPath_.end()) return g->second;
    return 0xFFFFFFFFu;
}

bool BlockDB::IsFromPack(const std::string& blockName) const {
    size_t c = blockName.find(':');
    if (c != std::string::npos && blockName.substr(0, c) != "minecraft") return true;
    // 是否只存在于叠加包里
    bool inBase = false;
    for (const VersionDef& v : versions_) {
        if (v.isOverlay) continue;
        if (v.blockByName.count(blockName)) { inBase = true; break; }
    }
    if (inBase) return false;
    for (const VersionDef& v : versions_)
        if (v.isOverlay && v.blockByName.count(blockName)) return true;
    return false;
}

const Model& BlockDB::ModelAt(u32 idx) const {
    static Model empty;
    return (idx < models_.size()) ? models_[idx] : empty;
}
const TextureInfo& BlockDB::TextureAt(u32 idx) const {
    static TextureInfo empty;
    return (idx < textures_.size()) ? textures_[idx] : empty;
}
const std::string& BlockDB::Str(u32 idx) const {
    static std::string empty;
    return (idx < strings_.size()) ? strings_[idx] : empty;
}

const LangEntry* BlockDB::FindLang(u32 langIdx, const std::string& blockName) const {
    if (langIdx >= langs_.size()) return nullptr;
    const auto& idx = langIndex_[langIdx];
    auto it = idx.find(blockName);
    if (it == idx.end()) return nullptr;
    if (it->second >= langs_[langIdx].size()) return nullptr;
    return &langs_[langIdx][it->second];
}

std::string BlockDB::DisplayName(const std::string& blockName, bool chinese,
                                 const VersionDef* prefer) const {
    if (prefer) {
        if (const LangEntry* e = FindLang(prefer->langIdx, blockName)) {
            const std::string& s = chinese ? e->zh : e->en;
            if (!s.empty()) return s;
        }
    }
    // 回退：任意一个有该名字的版本
    for (size_t i = 0; i < langs_.size(); ++i) {
        if (const LangEntry* e = FindLang((u32)i, blockName)) {
            const std::string& s = chinese ? e->zh : e->en;
            if (!s.empty()) return s;
        }
    }
    return blockName;
}

// ------------------------------------------------------------------ 模型解析
void BlockDB::ResolveModels(const VersionDef& v, const ltm::BlockState& st,
                            std::vector<ResolvedModel>& out) const {
    out.clear();
    if (versions_.empty()) return;
    size_t vIdx = (size_t)(&v - &versions_[0]);
    if (vIdx >= versions_.size()) vIdx = 0;
    std::string key = std::to_string(vIdx) + "|" + st.Key();
    auto cit = resolveCache_.find(key);
    if (cit != resolveCache_.end()) { out = cit->second; return; }

    const BlockDef* bdp = nullptr;
    {
        auto bit = v.blockByName.find(st.name);
        if (bit != v.blockByName.end()) bdp = &v.blocks[bit->second];
        else {
            // 叠加包（模组）提供的方块
            size_t vi = (size_t)(&v - &versions_[0]);
            if (vi < overlaysByVersion_.size()) {
                for (u32 ov : overlaysByVersion_[vi]) {
                    if (ov >= versions_.size()) continue;
                    auto o = versions_[ov].blockByName.find(st.name);
                    if (o != versions_[ov].blockByName.end()) { bdp = &versions_[ov].blocks[o->second]; break; }
                }
            }
            if (!bdp) {
                for (const VersionDef& ov : versions_) {
                    if (!ov.isOverlay) continue;
                    auto o = ov.blockByName.find(st.name);
                    if (o != ov.blockByName.end()) { bdp = &ov.blocks[o->second]; break; }
                }
            }
        }
    }
    if (bdp) {
        const BlockDef& bd = *bdp;
        // variants：取第一个完全匹配的（Minecraft 语义）
        for (const VariantEntry& ve : bd.variants) {
            if (MatchWhen(ve.when, st)) {
                if (ve.modelIdx < models_.size()) {
                    ResolvedModel rm;
                    rm.model = &models_[ve.modelIdx];
                    rm.rotX = ve.rotX; rm.rotY = ve.rotY; rm.uvlock = ve.uvlock;
                    out.push_back(rm);
                }
                break;
            }
        }
        // multipart：所有匹配的条目叠加
        for (const VariantEntry& ve : bd.multipart) {
            bool hit = false;
            if (ve.when.empty()) {
                hit = true;
            } else {
                size_t pos = 0;
                while (pos <= ve.when.size() && !hit) {
                    size_t bar = ve.when.find('|', pos);
                    std::string alt = (bar == std::string::npos) ? ve.when.substr(pos)
                                                                 : ve.when.substr(pos, bar - pos);
                    if (MatchWhen(alt, st)) hit = true;
                    if (bar == std::string::npos) break;
                    pos = bar + 1;
                }
            }
            if (hit && ve.modelIdx < models_.size()) {
                ResolvedModel rm;
                rm.model = &models_[ve.modelIdx];
                rm.rotX = ve.rotX; rm.rotY = ve.rotY; rm.uvlock = ve.uvlock;
                out.push_back(rm);
            }
        }
    }
    resolveCache_[key] = out;
}

bool BlockDB::HasGeometry(const VersionDef& v, const std::string& blockName) const {
    const auto it = v.blockByName.find(blockName);
    if (it == v.blockByName.end()) return false;
    const BlockDef& bd = v.blocks[it->second];
    auto ok = [&](u32 mi) { return mi < models_.size() && !models_[mi].elements.empty(); };
    for (const VariantEntry& ve : bd.variants) if (ok(ve.modelIdx)) return true;
    for (const VariantEntry& ve : bd.multipart) if (ok(ve.modelIdx)) return true;
    return false;
}

// ------------------------------------------------------------------ IBlockInfo
bool BlockDB::SupportsProp(const std::string& block, const std::string& prop,
                           const std::string& value) const {
    if (activeVersion_ < 0 || (size_t)activeVersion_ >= versions_.size()) return false;
    const VersionDef& v = versions_[(size_t)activeVersion_];
    const auto it = v.blockByName.find(block);
    if (it == v.blockByName.end()) return false;
    const std::vector<std::string>* vals = v.blocks[it->second].PropValues(prop);
    if (!vals) return false;
    return std::find(vals->begin(), vals->end(), value) != vals->end();
}

bool BlockDB::DefaultProps(const std::string& block,
                           std::vector<std::pair<std::string, std::string>>& out) const {
    out.clear();
    if (activeVersion_ < 0 || (size_t)activeVersion_ >= versions_.size()) return false;
    const VersionDef& v = versions_[(size_t)activeVersion_];
    const auto it = v.blockByName.find(block);
    if (it == v.blockByName.end()) return false;
    const BlockDef& bd = v.blocks[it->second];
    if (!bd.variants.empty() && !bd.variants[0].when.empty()) {
        ParseConds(bd.variants[0].when, out);
        if (!out.empty()) return true;
    }
    for (const auto& kv : bd.props)
        if (!kv.second.empty()) out.emplace_back(kv.first, kv.second[0]);
    return true;
}

} // namespace mc
