// jar -> 内部资源包 转换器
// 把 Python 烘焙脚本的逻辑移植到 C++，使其能在用户机器上运行时执行。
#include "packbuild.h"
#include "zip.h"
#include "png.h"
#include "json.h"
#include "str.h"
#include "fs.h"
#include <zlib.h>

namespace packbuild {

namespace {

// ---------------------------------------------------------------- 数据结构
struct Face {
    std::string tex;        // 原始贴图引用（可能是 #var）
    float uv[4] = {0, 0, 16, 16};
    int cullface = -1;
    int tintindex = -1;
    int rotation = 0;
    bool hasUv = false;
};

struct Elem {
    float from[3] = {0, 0, 0};
    float to[3] = {16, 16, 16};
    bool hasRot = false;
    int rotKind = 0;                 // 0=axis/angle 1=多轴
    float origin[3] = {8, 8, 8};
    int axis = 1;
    float angle = 0;
    bool rescale = false;
    float rx = 0, ry = 0, rz = 0;
    unsigned faceMask = 0;
    std::vector<Face> faces;         // 顺序对应 faceMask 低位到高位
};

struct BModel {
    bool translucent = false;
    std::vector<Elem> elements;
};

struct BVariant {
    std::string when;
    u32 modelIdx = 0;
    u8 rotX = 0, rotY = 0;
    bool uvlock = false;
};

struct BBlock {
    std::string name;
    std::vector<BVariant> variants;
    std::vector<BVariant> multipart;
};

// ---------------------------------------------------------------- 工具
static const char* kFaces[6] = { "down", "up", "north", "south", "west", "east" };

static int FaceIndex(const std::string& n) {
    for (int i = 0; i < 6; ++i) if (n == kFaces[i]) return i;
    return -1;
}

static void ReadVec3(const Json& j, float out[3], float def) {
    out[0] = out[1] = out[2] = def;
    if (!j.isArr()) return;
    for (int i = 0; i < 3 && i < (int)j.arr.size(); ++i)
        out[i] = (float)j.arr[(size_t)i].getNum(def);
}

static void ReadVec3i(const Json& j, int out[3], int def) {
    out[0] = out[1] = out[2] = def;
    if (!j.isArr()) return;
    for (int i = 0; i < 3 && i < (int)j.arr.size(); ++i)
        out[i] = (int)j.arr[(size_t)i].getInt(def);
}

// 贴图变量值：可能是字符串，也可能是 {"sprite":..., "force_translucent":true}
struct TexVal { std::string sprite; bool forceTranslucent = false; };

static TexVal ParseTexVal(const Json& j) {
    TexVal t;
    if (j.isStr()) { t.sprite = j.str; }
    else if (j.isObj()) {
        if (const Json* s = j.find("sprite")) t.sprite = s->getStr();
        if (const Json* f = j.find("force_translucent")) t.forceTranslucent = f->getBool();
    }
    return t;
}

// 按官方 FaceBakery 规则计算默认 UV（0..16 空间）
static void DefaultUv(int face, const float from[3], const float to[3], float uv[4]) {
    float x1 = from[0], y1 = from[1], z1 = from[2];
    float x2 = to[0],   y2 = to[1],   z2 = to[2];
    switch (face) {
        case 0: uv[0] = x1;      uv[1] = 16 - z2; uv[2] = x2;      uv[3] = 16 - z1; break; // down
        case 1: uv[0] = x1;      uv[1] = z1;      uv[2] = x2;      uv[3] = z2;      break; // up
        case 2: uv[0] = 16 - x2; uv[1] = 16 - y2; uv[2] = 16 - x1; uv[3] = 16 - y1; break; // north
        case 3: uv[0] = x1;      uv[1] = 16 - y2; uv[2] = x2;      uv[3] = 16 - y1; break; // south
        case 4: uv[0] = z1;      uv[1] = 16 - y2; uv[2] = z2;      uv[3] = 16 - y1; break; // west
        default:uv[0] = 16 - z2; uv[1] = 16 - y2; uv[2] = 16 - z1; uv[3] = 16 - y1; break; // east
    }
}

// 把 "a=1,b=2" 形式的 when 规范成属性名排序后的串
static std::string NormalizeWhenDict(const Json& when) {
    if (!when.isObj()) return std::string();
    std::vector<std::pair<std::string, std::string>> kv;
    for (const auto& p : when.obj) {
        if (p.second.isStr()) kv.emplace_back(p.first, p.second.str);
        else if (p.second.isBool()) kv.emplace_back(p.first, p.second.b ? "true" : "false");
        else if (p.second.isNum()) kv.emplace_back(p.first, p.second.getStr(""));
    }
    std::sort(kv.begin(), kv.end());
    std::string out;
    for (size_t i = 0; i < kv.size(); ++i) {
        if (i) out += ",";
        out += kv[i].first + "=" + kv[i].second;
    }
    return out;
}

// multipart 的 when -> "a=1,b=2|c=3"（OR 用 | ，AND 用 ,）；无条件返回 ""
static std::string NormalizeWhenMultipart(const Json& when) {
    if (when.isNull()) return std::string();
    if (when.isObj()) {
        const Json* orr = when.find("OR");
        const Json* andd = when.find("AND");
        if (orr && orr->isArr()) {
            std::vector<std::string> alts;
            for (const Json& a : orr->arr) {
                std::string s = NormalizeWhenMultipart(a);
                if (s.empty()) return std::string();   // 有空方案 = 无条件
                alts.push_back(s);
            }
            std::string out;
            for (size_t i = 0; i < alts.size(); ++i) { if (i) out += "|"; out += alts[i]; }
            return out;
        }
        if (andd && andd->isArr()) {
            std::vector<std::string> parts;
            for (const Json& a : andd->arr) {
                std::string s = NormalizeWhenMultipart(a);
                if (s.empty()) continue;
                parts.push_back(s);
            }
            std::string out;
            for (size_t i = 0; i < parts.size(); ++i) { if (i) out += ","; out += parts[i]; }
            return out;
        }
        return NormalizeWhenDict(when);
    }
    return std::string();
}

// ---------------------------------------------------------------- 构建器
struct Builder {
    // 字符串池
    std::vector<std::string> strings;
    std::unordered_map<std::string, u32> stringIdx;

    // 贴图池
    struct TexRec { std::string path; u16 w = 0, h = 0; u32 offset = 0, len = 0; bool hasAlpha = false; };
    std::vector<TexRec> textures;
    std::unordered_map<std::string, u32> texByPath;      // 路径 -> 贴图池下标
    std::unordered_map<std::string, u32> texByHash;      // 内容哈希 -> 贴图池下标
    std::vector<u8> pixels;

    // 模型池
    std::vector<BModel> models;
    std::unordered_map<std::string, u32> modelByHash;

    // 方块
    std::vector<BBlock> blocks;

    // 语言
    std::vector<std::pair<std::string, std::pair<std::string, std::string>>> lang; // block, (en, zh)

    size_t failedModels = 0;
    std::string warning;

    u32 Intern(const std::string& s) {
        auto it = stringIdx.find(s);
        if (it != stringIdx.end()) return it->second;
        u32 i = (u32)strings.size();
        strings.push_back(s);
        stringIdx[s] = i;
        return i;
    }

    void Warn(const std::string& w) {
        if (warning.size() < 4000) { warning += w; warning += "; "; }
    }
};

// ---------------------------------------------------------------- 模型解析
struct ModelSource {
    const zip::Archive* ar = nullptr;
    std::string ns;                                  // 当前命名空间
    std::unordered_map<std::string, const Json*>* modelCache = nullptr;
    std::vector<Json>* storage = nullptr;
    std::unordered_map<std::string, std::string>* idToEntry = nullptr;   // 模型id -> jar 内路径
};

// 把模型 id 规范成 "<ns>:<path>"，并给出 jar 内条目路径
static bool ResolveModelId(const std::string& raw, const std::string& curNs,
                           std::string& canonical, std::string& entry) {
    std::string ns = curNs, path = raw;
    size_t c = raw.find(':');
    if (c != std::string::npos) { ns = raw.substr(0, c); path = raw.substr(c + 1); }
    if (path.empty()) return false;
    canonical = ns + ":" + path;
    entry = "assets/" + ns + "/models/" + path + ".json";
    return true;
}

struct Chain {
    std::unordered_map<std::string, TexVal> textures;   // 原始变量表（含 # 引用）
    const Json* elements = nullptr;                     // 最近的 elements
    bool forceTranslucent = false;
    bool ok = false;
};

// 解析父链：textures 逐级合并（子覆盖父），elements 取最近的一个
static bool BuildChain(ModelSource& ms, const std::string& startId,
                       std::unordered_map<std::string, Chain>& cache, Chain& out, int depth = 0) {
    if (depth > 16) return false;
    auto ci = cache.find(startId);
    if (ci != cache.end()) { out = ci->second; return out.ok; }

    std::string canonical, entry;
    if (!ResolveModelId(startId, ms.ns, canonical, entry)) return false;
    auto it = ms.idToEntry->find(canonical);
    if (it == ms.idToEntry->end()) return false;

    std::vector<u8> raw;
    std::string err;
    if (!ms.ar->Read(it->second, raw, err)) return false;
    ms.storage->emplace_back(JsonParse((const char*)raw.data(), raw.size()));
    const Json* j = &ms.storage->back();
    if (!j->isObj()) return false;

    Chain ch;
    // 1) 先处理父级
    if (const Json* par = j->find("parent")) {
        if (par->isStr() && !par->str.empty()) {
            Chain pc;
            if (BuildChain(ms, par->str, cache, pc, depth + 1)) {
                ch.textures = pc.textures;
                ch.elements = pc.elements;
                ch.forceTranslucent = pc.forceTranslucent;
            }
        }
    }
    // 2) 本级的 textures 覆盖父级（保留原始 # 引用，稍后统一迭代替换）
    if (const Json* tex = j->find("textures")) {
        if (tex->isObj()) {
            for (const auto& kv : tex->obj) {
                TexVal tv = ParseTexVal(kv.second);
                if (tv.forceTranslucent) ch.forceTranslucent = true;
                ch.textures[kv.first] = tv;
            }
        }
    }
    // 3) 本级的 elements 覆盖父级
    if (const Json* el = j->find("elements")) {
        if (el->isArr()) ch.elements = el;
    }
    ch.ok = true;
    cache[canonical] = ch;
    out = ch;
    return true;
}

// 迭代替换贴图变量到最终路径
static std::string ResolveTexRef(const std::unordered_map<std::string, TexVal>& map,
                                 const std::string& raw) {
    std::string cur = raw;
    std::vector<std::string> seen;
    for (int i = 0; i < 16; ++i) {
        std::string key = cur;
        if (!key.empty() && key[0] == '#') key = key.substr(1);
        auto it = map.find(key);
        if (it == map.end()) break;                       // 不是变量 -> 字面路径
        if (std::find(seen.begin(), seen.end(), key) != seen.end()) return std::string();  // 成环
        seen.push_back(key);
        cur = it->second.sprite;
        if (cur.empty()) return std::string();
    }
    if (!cur.empty() && cur[0] == '#') return std::string();
    return cur;
}

// 把模型链解析成可写入的 BModel
static bool BuildModel(Builder& b, ModelSource& ms, const std::string& modelId,
                       const std::string& ownerNs, BModel& out,
                       std::unordered_map<std::string, Chain>& cache) {
    Chain ch;
    if (!BuildChain(ms, modelId, cache, ch)) return false;
    if (!ch.elements) { out.elements.clear(); out.translucent = ch.forceTranslucent; return true; }

    out.translucent = ch.forceTranslucent;
    for (const Json& e : ch.elements->arr) {
        if (!e.isObj()) continue;
        Elem el;
        if (const Json* f = e.find("from")) ReadVec3(*f, el.from, 0);
        if (const Json* t = e.find("to"))   ReadVec3(*t, el.to, 16);
        if (const Json* r = e.find("rotation")) {
            if (r->isObj()) {
                el.hasRot = true;
                if (const Json* o = r->find("origin")) ReadVec3(*o, el.origin, 8);
                if (const Json* rs = r->find("rescale")) el.rescale = rs->getBool();
                if (const Json* ax = r->find("axis")) {
                    el.rotKind = 0;
                    std::string a = ax->getStr("y");
                    el.axis = (a == "x") ? 0 : (a == "z" ? 2 : 1);
                    if (const Json* an = r->find("angle")) el.angle = (float)an->getNum(0);
                } else {
                    el.rotKind = 1;
                    if (const Json* v = r->find("x")) el.rx = (float)v->getNum(0);
                    if (const Json* v = r->find("y")) el.ry = (float)v->getNum(0);
                    if (const Json* v = r->find("z")) el.rz = (float)v->getNum(0);
                }
            }
        }
        const Json* faces = e.find("faces");
        if (!faces || !faces->isObj()) continue;
        for (int fi = 0; fi < 6; ++fi) {
            const Json* fc = faces->find(kFaces[fi]);
            if (!fc || !fc->isObj()) continue;
            Face f;
            if (const Json* t = fc->find("texture")) {
                TexVal tv = ParseTexVal(*t);
                if (tv.forceTranslucent) out.translucent = true;
                // 关键：把 #变量 迭代替换到最终贴图路径
                std::string resolved = ResolveTexRef(ch.textures, tv.sprite);
                if (!resolved.empty()) {
                    // 规范化：minecraft 用裸路径，其它命名空间加前缀，避免不同命名空间同名冲突
                    std::string ns2 = ownerNs, path2 = resolved;
                    size_t cc = resolved.find(':');
                    if (cc != std::string::npos) { ns2 = resolved.substr(0, cc); path2 = resolved.substr(cc + 1); }
                    f.tex = (ns2 == "minecraft") ? path2 : (ns2 + ":" + path2);
                }
            }
            if (f.tex.empty()) continue;
            if (const Json* uv = fc->find("uv")) {
                if (uv->isArr() && uv->arr.size() >= 4) {
                    for (int k = 0; k < 4; ++k) f.uv[k] = (float)uv->arr[(size_t)k].getNum(0);
                    f.hasUv = true;
                }
            }
            if (!f.hasUv) DefaultUv(fi, el.from, el.to, f.uv);
            if (const Json* cf = fc->find("cullface")) f.cullface = FaceIndex(cf->getStr());
            if (const Json* ti = fc->find("tintindex")) f.tintindex = (int)ti->getInt(-1);
            if (const Json* rt = fc->find("rotation")) f.rotation = (int)rt->getInt(0);
            el.faceMask |= (1u << fi);
            el.faces.push_back(std::move(f));
        }
        if (el.faceMask) out.elements.push_back(std::move(el));
    }
    return true;
}

// 模型内容哈希（用于去重）
static std::string ModelHash(const BModel& m) {
    std::string s;
    s.reserve(m.elements.size() * 64);
    s.push_back(m.translucent ? '1' : '0');
    char buf[256];
    for (const Elem& e : m.elements) {
        snprintf(buf, sizeof(buf), "|%.3f,%.3f,%.3f,%.3f,%.3f,%.3f,%d,%d,%.3f,%d,%.3f,%.3f,%.3f,%d,%u",
                 e.from[0], e.from[1], e.from[2], e.to[0], e.to[1], e.to[2],
                 e.hasRot, e.rotKind, e.angle, e.axis, e.rx, e.ry, e.rz, e.rescale, e.faceMask);
        s += buf;
        for (const Face& f : e.faces) {
            snprintf(buf, sizeof(buf), "#%s,%.3f,%.3f,%.3f,%.3f,%d,%d,%d",
                     f.tex.c_str(), f.uv[0], f.uv[1], f.uv[2], f.uv[3],
                     f.cullface, f.tintindex, f.rotation);
            s += buf;
        }
    }
    return s;
}

// ---------------------------------------------------------------- 序列化
struct BinWriter {
    std::vector<u8>& out;
    explicit BinWriter(std::vector<u8>& o) : out(o) {}
    void u8v(u8 v) { out.push_back(v); }
    void u16v(u16 v) { out.push_back((u8)v); out.push_back((u8)(v >> 8)); }
    void u32v(u32 v) { for (int i = 0; i < 4; ++i) out.push_back((u8)(v >> (i * 8))); }
    void i32v(i32 v) { u32v((u32)v); }
    void i16v(i16 v) { u16v((u16)v); }
    void f32v(float f) { u32 v; memcpy(&v, &f, 4); u32v(v); }
    void bytes(const void* p, size_t n) { const u8* b = (const u8*)p; out.insert(out.end(), b, b + n); }
};

static void SerializePack(Builder& b, i32 dataVersion, const std::string& versionId,
                          std::vector<u8>& out) {
    // 预先登记版本相关字符串
    u32 idIdx = b.Intern(versionId);
    std::vector<u32> blockNameIdx(b.blocks.size());
    for (size_t i = 0; i < b.blocks.size(); ++i) blockNameIdx[i] = b.Intern(b.blocks[i].name);
    std::vector<std::vector<u32>> variantWhenIdx(b.blocks.size()), mpWhenIdx(b.blocks.size());
    for (size_t i = 0; i < b.blocks.size(); ++i) {
        for (const BVariant& v : b.blocks[i].variants)  variantWhenIdx[i].push_back(b.Intern(v.when));
        for (const BVariant& v : b.blocks[i].multipart) mpWhenIdx[i].push_back(b.Intern(v.when));
    }
    std::vector<u32> texPathIdx(b.textures.size());
    for (size_t i = 0; i < b.textures.size(); ++i) texPathIdx[i] = b.Intern(b.textures[i].path);
    std::vector<std::vector<u32>> langIdx(b.lang.size());
    for (size_t i = 0; i < b.lang.size(); ++i) {
        langIdx[i].push_back(b.Intern(b.lang[i].first));
        langIdx[i].push_back(b.Intern(b.lang[i].second.first));
        langIdx[i].push_back(b.Intern(b.lang[i].second.second));
    }
    // 模型里的贴图路径字符串
    std::vector<std::vector<std::vector<u32>>> modelTexIdx(b.models.size());
    for (size_t mi = 0; mi < b.models.size(); ++mi) {
        modelTexIdx[mi].resize(b.models[mi].elements.size());
        for (size_t ei = 0; ei < b.models[mi].elements.size(); ++ei) {
            for (const Face& f : b.models[mi].elements[ei].faces)
                modelTexIdx[mi][ei].push_back(b.Intern(f.tex));
        }
    }

    BinWriter w(out);
    const char magic[8] = { 'P','M','B','L','K','D','B', 0 };
    w.bytes(magic, 8);
    w.u32v(3);                                   // formatVersion
    w.u32v(1);                                   // versionCount
    w.u32v((u32)b.textures.size());
    w.u32v((u32)b.models.size());
    w.u32v(1);                                   // langTableCount

    // STRINGS
    w.u32v((u32)b.strings.size());
    for (const std::string& s : b.strings) {
        w.bytes(s.data(), s.size());
        w.u8v(0);
    }
    // LANGS
    w.u32v(1);
    w.u32v((u32)b.lang.size());
    for (size_t i = 0; i < b.lang.size(); ++i) {
        w.u32v(langIdx[i][0]); w.u32v(langIdx[i][1]); w.u32v(langIdx[i][2]);
    }
    // TEXTURES
    w.u32v((u32)b.textures.size());
    for (const auto& t : b.textures) {
        w.u16v(t.w); w.u16v(t.h); w.u32v(t.offset); w.u32v(t.len); w.u8v(t.hasAlpha ? 1 : 0);
    }
    w.u32v((u32)b.pixels.size());
    {
        uLongf bound = compressBound((uLong)b.pixels.size());
        std::vector<u8> comp(bound);
        if (compress2(comp.data(), &bound, b.pixels.data(), (uLong)b.pixels.size(), 9) != Z_OK) {
            comp.clear(); bound = 0;
        }
        w.u32v((u32)bound);
        w.bytes(comp.data(), bound);
    }
    // MODELS
    w.u32v((u32)b.models.size());
    for (size_t mi = 0; mi < b.models.size(); ++mi) {
        const BModel& m = b.models[mi];
        w.u8v(m.translucent ? 1 : 0);
        w.u16v((u16)m.elements.size());
        for (size_t ei = 0; ei < m.elements.size(); ++ei) {
            const Elem& e = m.elements[ei];
            for (int k = 0; k < 3; ++k) w.f32v(e.from[k]);
            for (int k = 0; k < 3; ++k) w.f32v(e.to[k]);
            w.u8v(e.hasRot ? 1 : 0);
            if (e.hasRot) {
                w.u8v((u8)e.rotKind);
                for (int k = 0; k < 3; ++k) w.f32v(e.origin[k]);
                w.u8v((u8)e.axis);
                w.f32v(e.angle);
                w.u8v(e.rescale ? 1 : 0);
                w.f32v(e.rx); w.f32v(e.ry); w.f32v(e.rz);
            }
            w.u8v((u8)e.faceMask);
            size_t fi = 0;
            for (int bit = 0; bit < 6; ++bit) {
                if (!(e.faceMask & (1u << bit))) continue;
                const Face& f = e.faces[fi];
                w.u32v(modelTexIdx[mi][ei][fi]);
                for (int k = 0; k < 4; ++k) w.f32v(f.uv[k]);
                w.u8v((u8)(f.cullface < 0 ? 0xFF : f.cullface));
                w.i16v((i16)f.tintindex);
                w.u16v((u16)(((f.rotation % 360) + 360) % 360));
                ++fi;
            }
        }
    }
    // VERSIONS
    w.u32v(1);
    w.u32v(idIdx);
    w.i32v(dataVersion);
    w.u32v(0);                                   // langTableIdx
    w.u32v((u32)b.textures.size());
    for (size_t i = 0; i < b.textures.size(); ++i) { w.u32v(texPathIdx[i]); w.u32v((u32)i); }
    w.u32v((u32)b.blocks.size());
    for (size_t i = 0; i < b.blocks.size(); ++i) {
        const BBlock& bd = b.blocks[i];
        w.u32v(blockNameIdx[i]);
        w.u16v((u16)bd.variants.size());
        for (size_t k = 0; k < bd.variants.size(); ++k) {
            const BVariant& v = bd.variants[k];
            w.u32v(variantWhenIdx[i][k]);
            w.u32v(v.modelIdx);
            w.u8v((u8)(v.rotX / 90));
            w.u8v((u8)(v.rotY / 90));
            w.u8v(v.uvlock ? 1 : 0);
        }
        w.u16v((u16)bd.multipart.size());
        for (size_t k = 0; k < bd.multipart.size(); ++k) {
            const BVariant& v = bd.multipart[k];
            w.u32v(mpWhenIdx[i][k]);
            w.u32v(v.modelIdx);
            w.u8v((u8)(v.rotX / 90));
            w.u8v((u8)(v.rotY / 90));
            w.u8v(v.uvlock ? 1 : 0);
        }
    }
}

} // namespace

// ---------------------------------------------------------------- 主流程
bool BuildFromJar(const std::wstring& jarPath, const std::string& packName,
                  int kind, i32 forcedDataVersion, Result& out) {
    zip::Archive ar;
    std::string err;
    if (!ar.Open(jarPath, err)) { out.error = "打开 jar 失败: " + err; return false; }

    // 版本信息
    std::string versionId;
    i32 dataVersion = forcedDataVersion;
    {
        std::string txt;
        if (ar.ReadText("version.json", txt, err)) {
            Json j = JsonParse(txt);
            if (const Json* id = j.find("id")) versionId = id->getStr();
            if (const Json* wv = j.find("world_version")) {
                i32 dv = (i32)wv->getInt(0);
                if (dataVersion <= 0) dataVersion = dv;
                if (!versionId.empty() && dataVersion != dv && kind != 1) {
                    // 模组包：dataVersion 由用户指定（要叠加到哪个版本）
                } else if (kind == 1) {
                    dataVersion = dv;
                }
            }
        }
    }
    if (versionId.empty()) versionId = packName;
    if (dataVersion <= 0) dataVersion = 0;

    // 收集命名空间
    std::vector<std::string> all;
    ar.List(all, "assets/");
    std::set<std::string> nsSet;
    for (const std::string& n : all) {
        // assets/<ns>/blockstates/xxx.json
        if (n.compare(0, 7, "assets/") != 0) continue;
        size_t p = n.find('/', 7);
        if (p == std::string::npos) continue;
        if (n.compare(p + 1, 11, "blockstates") != 0) continue;
        nsSet.insert(n.substr(7, p - 7));
    }
    if (nsSet.empty()) { out.error = "jar 里没有找到任何 blockstates（不是客户端 jar 或模组 jar？）"; return false; }

    Builder b;
    size_t totalFailed = 0;

    for (const std::string& ns : nsSet) {
        // 1) 模型索引
        std::unordered_map<std::string, std::string> idToEntry;
        std::string modelsPrefix = "assets/" + ns + "/models/";
        for (const std::string& n : all) {
            if (n.compare(0, modelsPrefix.size(), modelsPrefix) != 0) continue;
            if (n.size() < 5 || n.compare(n.size() - 5, 5, ".json") != 0) continue;
            std::string path = n.substr(modelsPrefix.size(), n.size() - modelsPrefix.size() - 5);
            idToEntry[ns + ":" + path] = n;
            if (ns == "minecraft") idToEntry[path] = n;
        }
        if (idToEntry.empty()) continue;

        std::vector<Json> storage;
        storage.reserve(idToEntry.size() * 2);
        std::unordered_map<std::string, Chain> chainCache;
        ModelSource ms;
        ms.ar = &ar; ms.ns = ns; ms.storage = &storage; ms.idToEntry = &idToEntry;

        // 2) 逐个 blockstate
        std::string bsPrefix = "assets/" + ns + "/blockstates/";
        std::vector<std::string> bsFiles;
        for (const std::string& n : all) {
            if (n.compare(0, bsPrefix.size(), bsPrefix) != 0) continue;
            if (n.size() < 5 || n.compare(n.size() - 5, 5, ".json") != 0) continue;
            bsFiles.push_back(n);
        }
        std::sort(bsFiles.begin(), bsFiles.end());

        for (const std::string& bf : bsFiles) {
            std::string body;
            if (!ar.ReadText(bf, body, err)) continue;
            Json j = JsonParse(body);
            if (!j.isObj()) continue;

            std::string shortName = bf.substr(bsPrefix.size(), bf.size() - bsPrefix.size() - 5);
            BBlock blk;
            blk.name = ns + ":" + shortName;

            // 缓存本方块用到的模型 -> 池下标
            std::unordered_map<std::string, u32> localModel;

            auto modelIdxFor = [&](const std::string& mid, u8 rotX, u8 rotY, bool uvlock) -> u32 {
                auto it = localModel.find(mid);
                if (it != localModel.end()) return it->second;
                BModel m;
                if (!BuildModel(b, ms, mid, ns, m, chainCache)) {
                    localModel[mid] = 0xFFFFFFFFu;
                    return 0xFFFFFFFFu;
                }
                std::string h = ModelHash(m);
                u32 idx;
                auto hit = b.modelByHash.find(h);
                if (hit != b.modelByHash.end()) idx = hit->second;
                else {
                    idx = (u32)b.models.size();
                    b.models.push_back(m);
                    b.modelByHash[h] = idx;
                }
                localModel[mid] = idx;
                return idx;
            };

            if (const Json* vs = j.find("variants")) {
                if (vs->isObj()) {
                    for (const auto& kv : vs->obj) {
                        // 变体键 -> 规范化的 when 字符串（属性名排序）
                        std::string when;
                        {
                            std::vector<std::pair<std::string, std::string>> parts;
                            size_t p = 0;
                            while (p <= kv.first.size()) {
                                size_t c2 = kv.first.find(',', p);
                                std::string one = (c2 == std::string::npos) ? kv.first.substr(p)
                                                                           : kv.first.substr(p, c2 - p);
                                size_t eq = one.find('=');
                                if (eq != std::string::npos) parts.emplace_back(one.substr(0, eq), one.substr(eq + 1));
                                if (c2 == std::string::npos) break;
                                p = c2 + 1;
                            }
                            std::sort(parts.begin(), parts.end());
                            for (size_t z = 0; z < parts.size(); ++z) {
                                if (z) when += ",";
                                when += parts[z].first + "=" + parts[z].second;
                            }
                        }
                        const Json& def = kv.second;
                        auto addOne = [&](const Json& d) {
                            if (!d.isObj()) return;
                            const Json* mj = d.find("model");
                            if (!mj || !mj->isStr()) return;
                            u8 rx = (u8)(d.find("x") ? d.find("x")->getInt(0) : 0);
                            u8 ry = (u8)(d.find("y") ? d.find("y")->getInt(0) : 0);
                            bool uv = d.find("uvlock") ? d.find("uvlock")->getBool() : false;
                            u32 mi = modelIdxFor(mj->str, rx, ry, uv);
                            if (mi == 0xFFFFFFFFu) { ++totalFailed; return; }
                            BVariant v;
                            v.when = when;
                            v.modelIdx = mi;
                            v.rotX = rx; v.rotY = ry; v.uvlock = uv;
                            blk.variants.push_back(v);
                        };
                        if (def.isArr()) { for (const Json& d : def.arr) addOne(d); }
                        else addOne(def);
                    }
                }
            }
            if (const Json* mp = j.find("multipart")) {
                if (mp->isArr()) {
                    for (const Json& part : mp->arr) {
                        if (!part.isObj()) continue;
                        std::string when = part.find("when") ? NormalizeWhenMultipart(*part.find("when")) : std::string();
                        const Json* ap = part.find("apply");
                        if (!ap) continue;
                        auto addOne = [&](const Json& d) {
                            if (!d.isObj()) return;
                            const Json* mj = d.find("model");
                            if (!mj || !mj->isStr()) return;
                            u8 rx = (u8)(d.find("x") ? d.find("x")->getInt(0) : 0);
                            u8 ry = (u8)(d.find("y") ? d.find("y")->getInt(0) : 0);
                            bool uv = d.find("uvlock") ? d.find("uvlock")->getBool() : false;
                            u32 mi = modelIdxFor(mj->str, rx, ry, uv);
                            if (mi == 0xFFFFFFFFu) { ++totalFailed; return; }
                            BVariant v; v.when = when; v.modelIdx = mi;
                            v.rotX = rx; v.rotY = ry; v.uvlock = uv;
                            blk.multipart.push_back(v);
                        };
                        if (ap->isArr()) { for (const Json& d : ap->arr) addOne(d); }
                        else addOne(*ap);
                    }
                }
            }
            if (!blk.variants.empty() || !blk.multipart.empty()) b.blocks.push_back(std::move(blk));
        }

        // 3) 语言文件
        {
            std::string enTxt, zhTxt;
            ar.ReadText("assets/" + ns + "/lang/en_us.json", enTxt, err);
            ar.ReadText("assets/" + ns + "/lang/zh_cn.json", zhTxt, err);
            Json en = enTxt.empty() ? Json() : JsonParse(enTxt);
            Json zh = zhTxt.empty() ? Json() : JsonParse(zhTxt);
            std::string pfx = "block." + ns + ".";
            for (const BBlock& blk : b.blocks) {
                std::string shortName = blk.name.substr(blk.name.find(':') + 1);
                std::string key = pfx + shortName;
                std::string e, z;
                if (const Json* v = en.find(key)) e = v->getStr();
                if (e.empty()) { if (const Json* v = en.find("block." + shortName)) e = v->getStr(); }
                if (const Json* v = zh.find(key)) z = v->getStr();
                if (e.empty()) e = shortName;
                std::pair<std::string, std::string> p(e, z);
                b.lang.emplace_back(blk.name, std::move(p));
            }
        }

        // 4) 贴图：把模型里用到的全部收集出来解码
        {
            std::set<std::string> wanted;
            for (const BModel& m : b.models)
                for (const Elem& e : m.elements)
                    for (const Face& f : e.faces)
                        if (!f.tex.empty()) wanted.insert(f.tex);

            for (const std::string& rawPath : wanted) {
                if (b.texByPath.count(rawPath)) continue;
                std::string tns = ns, tpath = rawPath;
                size_t c = rawPath.find(':');
                if (c != std::string::npos) { tns = rawPath.substr(0, c); tpath = rawPath.substr(c + 1); }
                std::string entry = "assets/" + tns + "/textures/" + tpath + ".png";
                std::vector<u8> pb;
                if (!ar.Read(entry, pb, err)) continue;
                png::Image img;
                if (!png::DecodeFirstFrame(pb.data(), pb.size(), img, err)) continue;
                if (img.Empty()) continue;

                // 内容去重
                std::string h = std::to_string(img.w) + "x" + std::to_string(img.h) + ":";
                u64 hh = 1469598103934665603ULL;
                for (u8 px : img.rgba) { hh ^= px; hh *= 1099511628211ULL; }
                h += std::to_string(hh);
                auto hit = b.texByHash.find(h);
                u32 idx;
                if (hit != b.texByHash.end()) {
                    idx = hit->second;
                } else {
                    Builder::TexRec t;
                    t.path = rawPath;
                    t.w = (u16)img.w; t.h = (u16)img.h;
                    t.offset = (u32)b.pixels.size();
                    t.len = (u32)img.rgba.size();
                    bool alpha = false;
                    for (size_t i = 3; i < img.rgba.size(); i += 4)
                        if (img.rgba[i] != 255) { alpha = true; break; }
                    t.hasAlpha = alpha;
                    idx = (u32)b.textures.size();
                    b.textures.push_back(t);
                    b.pixels.insert(b.pixels.end(), img.rgba.begin(), img.rgba.end());
                    b.texByHash[h] = idx;
                }
                b.texByPath[rawPath] = idx;
            }
        }
    }

    if (b.blocks.empty()) { out.error = "没有解析出任何方块"; return false; }

    SerializePack(b, dataVersion, versionId, out.data);

    out.versionId = versionId;
    out.packName = packName;
    out.dataVersion = dataVersion;
    out.blockCount = b.blocks.size();
    out.modelCount = b.models.size();
    out.textureCount = b.textures.size();
    out.failedModels = totalFailed;
    out.namespaces = nsSet.size();
    out.warning = b.warning;
    return true;
}

} // namespace packbuild
