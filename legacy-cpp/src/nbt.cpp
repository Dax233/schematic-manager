#include "nbt.h"
#include <zlib.h>

namespace nbt {

const char* TagTypeName(TagType t) {
    static const char* names[] = { "End","Byte","Short","Int","Long","Float","Double",
                                   "ByteArray","String","List","Compound","IntArray","LongArray" };
    return (t <= TAG_LongArray) ? names[t] : "?";
}

Value* Value::find(const std::string& key) {
    for (auto& kv : fields) if (kv.first == key) return &kv.second;
    return nullptr;
}
const Value* Value::find(const std::string& key) const {
    for (const auto& kv : fields) if (kv.first == key) return &kv.second;
    return nullptr;
}
void Value::set(const std::string& key, Value v) {
    for (auto& kv : fields) if (kv.first == key) { kv.second = std::move(v); return; }
    fields.emplace_back(key, std::move(v));
}
i64 Value::asInt(i64 def) const {
    switch (type) {
        case TAG_Byte: case TAG_Short: case TAG_Int: case TAG_Long: return i;
        case TAG_Float: case TAG_Double: return (i64)d;
        case TAG_String: return (i64)strtoll(s.c_str(), nullptr, 10);
        default: return def;
    }
}
double Value::asDouble(double def) const {
    switch (type) {
        case TAG_Float: case TAG_Double: return d;
        case TAG_Byte: case TAG_Short: case TAG_Int: case TAG_Long: return (double)i;
        default: return def;
    }
}
std::string Value::asString(const std::string& def) const {
    return type == TAG_String ? s : def;
}
bool Value::asBool(bool def) const {
    switch (type) {
        case TAG_Byte: case TAG_Short: case TAG_Int: case TAG_Long: return i != 0;
        case TAG_Float: case TAG_Double: return d != 0;
        default: return def;
    }
}
void Value::asXYZ(i32 out[3]) const {
    out[0] = out[1] = out[2] = 0;
    if (type != TAG_Compound) return;
    if (const Value* v = find("x")) out[0] = v->asInt32();
    if (const Value* v = find("y")) out[1] = v->asInt32();
    if (const Value* v = find("z")) out[2] = v->asInt32();
}

// ------------------------------------------------------------------ reader
namespace {

struct Reader {
    const u8* p;
    const u8* end;
    bool ok = true;

    bool need(size_t n) { if ((size_t)(end - p) < n) { ok = false; return false; } return true; }
    u8  u8v()  { if (!need(1)) return 0; return *p++; }
    i8  i8v()  { return (i8)u8v(); }
    i16 i16v() { if (!need(2)) return 0; i16 v = (i16)((p[0] << 8) | p[1]); p += 2; return v; }
    i32 i32v() { if (!need(4)) return 0; u32 v = ((u32)p[0] << 24) | ((u32)p[1] << 16) | ((u32)p[2] << 8) | p[3]; p += 4; return (i32)v; }
    i64 i64v() {
        if (!need(8)) return 0;
        u64 v = 0;
        for (int k = 0; k < 8; ++k) v = (v << 8) | p[k];
        p += 8;
        return (i64)v;
    }
    float  f32v() { i32 v = i32v(); float f; memcpy(&f, &v, 4); return f; }
    double f64v() { i64 v = i64v(); double d; memcpy(&d, &v, 8); return d; }

    std::string str() {
        u16 n = (u16)i16v();
        if (!need(n)) return std::string();
        std::string s((const char*)p, n);
        p += n;
        return s;
    }

    Value payload(TagType t, int depth) {
        Value v;
        v.type = t;
        if (depth > 512) { ok = false; return v; }
        switch (t) {
            case TAG_Byte:   v.i = i8v();  break;
            case TAG_Short:  v.i = i16v(); break;
            case TAG_Int:    v.i = i32v(); break;
            case TAG_Long:   v.i = i64v(); break;
            case TAG_Float:  v.d = f32v(); break;
            case TAG_Double: v.d = f64v(); break;
            case TAG_ByteArray: {
                i32 n = i32v();
                if (n < 0 || !need((size_t)n)) { ok = false; break; }
                v.bytes.assign(p, p + n);
                p += n;
                break;
            }
            case TAG_String: v.s = str(); break;
            case TAG_List: {
                TagType et = (TagType)u8v();
                i32 n = i32v();
                if (n < 0) { ok = false; break; }
                v.listElem = et;
                v.list.reserve((size_t)std::min<i32>(n, 1 << 20));
                for (i32 k = 0; k < n && ok; ++k) v.list.push_back(payload(et, depth + 1));
                break;
            }
            case TAG_Compound: {
                while (ok) {
                    TagType ft = (TagType)u8v();
                    if (!ok) break;
                    if (ft == TAG_End) break;
                    std::string name = str();
                    if (!ok) break;
                    v.fields.emplace_back(std::move(name), payload(ft, depth + 1));
                }
                break;
            }
            case TAG_IntArray: {
                i32 n = i32v();
                if (n < 0 || !need((size_t)n * 4)) { ok = false; break; }
                v.ints.resize((size_t)n);
                for (i32 k = 0; k < n; ++k) v.ints[(size_t)k] = i32v();
                break;
            }
            case TAG_LongArray: {
                i32 n = i32v();
                if (n < 0 || !need((size_t)n * 8)) { ok = false; break; }
                v.longs.resize((size_t)n);
                for (i32 k = 0; k < n; ++k) v.longs[(size_t)k] = i64v();
                break;
            }
            default: ok = false; break;
        }
        return v;
    }
};

// ------------------------------------------------------------------ writer
struct Writer {
    std::vector<u8>& out;
    explicit Writer(std::vector<u8>& o) : out(o) {}

    void u8v(u8 v) { out.push_back(v); }
    void i16v(i16 v) { out.push_back((u8)(v >> 8)); out.push_back((u8)v); }
    void i32v(i32 v) { for (int k = 3; k >= 0; --k) out.push_back((u8)(v >> (k * 8))); }
    void i64v(i64 v) { for (int k = 7; k >= 0; --k) out.push_back((u8)(v >> (k * 8))); }
    void f32v(float f) { i32 v; memcpy(&v, &f, 4); i32v(v); }
    void f64v(double d) { i64 v; memcpy(&v, &d, 8); i64v(v); }
    void str(const std::string& s) {
        size_t n = std::min<size_t>(s.size(), 65535);
        i16v((i16)n);
        out.insert(out.end(), s.begin(), s.begin() + n);
    }

    void payload(const Value& v) {
        switch (v.type) {
            case TAG_Byte:   u8v((u8)(i8)v.i); break;
            case TAG_Short:  i16v((i16)v.i); break;
            case TAG_Int:    i32v((i32)v.i); break;
            case TAG_Long:   i64v(v.i); break;
            case TAG_Float:  f32v((float)v.d); break;
            case TAG_Double: f64v(v.d); break;
            case TAG_ByteArray:
                i32v((i32)v.bytes.size());
                out.insert(out.end(), v.bytes.begin(), v.bytes.end());
                break;
            case TAG_String: str(v.s); break;
            case TAG_List:
                u8v((u8)v.listElem);
                i32v((i32)v.list.size());
                for (const auto& it : v.list) payload(it);
                break;
            case TAG_Compound:
                for (const auto& kv : v.fields) {
                    if (kv.second.type == TAG_End) continue;
                    u8v((u8)kv.second.type);
                    str(kv.first);
                    payload(kv.second);
                }
                u8v(TAG_End);
                break;
            case TAG_IntArray:
                i32v((i32)v.ints.size());
                for (i32 x : v.ints) i32v(x);
                break;
            case TAG_LongArray:
                i32v((i32)v.longs.size());
                for (i64 x : v.longs) i64v(x);
                break;
            default: break;
        }
    }
};

} // namespace

bool Parse(const u8* data, size_t size, Value& out, std::string& err) {
    Reader r;
    r.p = data;
    r.end = data + size;
    TagType t = (TagType)r.u8v();
    if (!r.ok || t != TAG_Compound) { err = "根标签不是 Compound"; return false; }
    r.str(); // 根名称（通常为空）
    out = r.payload(TAG_Compound, 0);
    if (!r.ok) { err = "NBT 数据损坏或提前结束"; return false; }
    return true;
}

bool Serialize(const Value& root, std::vector<u8>& out) {
    out.clear();
    Writer w(out);
    w.u8v(TAG_Compound);
    w.str(std::string());
    w.payload(root);
    return true;
}

bool Gunzip(const u8* in, size_t inLen, std::vector<u8>& out) {
    z_stream zs;
    memset(&zs, 0, sizeof(zs));
    if (inflateInit2(&zs, 15 + 32) != Z_OK) return false;   // 自动识别 gzip/zlib
    zs.next_in = (Bytef*)in;
    zs.avail_in = (uInt)inLen;
    out.clear();
    std::vector<u8> buf(1 << 16);
    int ret = Z_OK;
    do {
        zs.next_out = buf.data();
        zs.avail_out = (uInt)buf.size();
        ret = inflate(&zs, Z_NO_FLUSH);
        if (ret != Z_OK && ret != Z_STREAM_END && ret != Z_BUF_ERROR) { inflateEnd(&zs); return false; }
        out.insert(out.end(), buf.data(), buf.data() + (buf.size() - zs.avail_out));
        if (ret == Z_BUF_ERROR && zs.avail_in == 0) break;
    } while (ret != Z_STREAM_END);
    inflateEnd(&zs);
    return ret == Z_STREAM_END || !out.empty();
}

bool Gzip(const u8* in, size_t inLen, std::vector<u8>& out, int level) {
    z_stream zs;
    memset(&zs, 0, sizeof(zs));
    if (deflateInit2(&zs, level, Z_DEFLATED, 15 + 16, 8, Z_DEFAULT_STRATEGY) != Z_OK) return false;
    zs.next_in = (Bytef*)in;
    zs.avail_in = (uInt)inLen;
    out.clear();
    std::vector<u8> buf(1 << 16);
    int ret = Z_OK;
    do {
        zs.next_out = buf.data();
        zs.avail_out = (uInt)buf.size();
        ret = deflate(&zs, Z_FINISH);
        if (ret == Z_STREAM_ERROR) { deflateEnd(&zs); return false; }
        out.insert(out.end(), buf.data(), buf.data() + (buf.size() - zs.avail_out));
    } while (ret != Z_STREAM_END);
    deflateEnd(&zs);
    return ret == Z_STREAM_END;
}

bool ParseMaybeGzip(const std::vector<u8>& fileData, Value& out, std::string& err) {
    const u8* data = fileData.data();
    size_t size = fileData.size();
    std::vector<u8> plain;
    if (size >= 2 && data[0] == 0x1F && data[1] == 0x8B) {
        if (!Gunzip(data, size, plain)) { err = "gzip 解压失败"; return false; }
        data = plain.data();
        size = plain.size();
    }
    return Parse(data, size, out, err);
}

bool SerializeGzip(const Value& root, std::vector<u8>& out, int level) {
    std::vector<u8> plain;
    Serialize(root, plain);
    return Gzip(plain.data(), plain.size(), out, level);
}

} // namespace nbt
