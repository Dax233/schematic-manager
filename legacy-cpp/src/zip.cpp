// ZIP 读取器实现 —— 只读，支持 stored(0) / deflate(8)
#include "zip.h"
#include "fs.h"
#include <zlib.h>

namespace zip {
namespace {

const u32 kSigLocal   = 0x04034b50u;  // PK\x03\x04
const u32 kSigCentral = 0x02014b50u;  // PK\x01\x02
const u32 kSigEocd    = 0x06054b50u;  // PK\x05\x06
const u32 kSigZip64EocdLoc = 0x07064b50u; // PK\x06\x07
const u32 kSigZip64Eocd    = 0x06064b50u; // PK\x06\x06

// 单条目解压上限（防御 zip bomb / 畸形长度字段）：512 MiB
const u64 kMaxEntryOut = 512ull * 1024ull * 1024ull;
// 中央目录条目数上限（防御畸形 EOCD）
const u32 kMaxEntries = 2000000u;

// ---------------------------------------------------------------- 小端读取器
struct BinReader {
    const u8* p;
    const u8* end;
    bool ok = true;

    bool need(size_t n) const { return (size_t)(end - p) >= n; }
    bool bad() { ok = false; return false; }
    u8  u8v()  { if (!need(1)) { bad(); return 0; } return *p++; }
    u16 u16v() { if (!need(2)) { bad(); return 0; } u16 v = (u16)((u16)p[0] | ((u16)p[1] << 8)); p += 2; return v; }
    u32 u32v() {
        if (!need(4)) { bad(); return 0; }
        u32 v = (u32)p[0] | ((u32)p[1] << 8) | ((u32)p[2] << 16) | ((u32)p[3] << 24);
        p += 4; return v;
    }
    u64 u64v() {
        if (!need(8)) { bad(); return 0; }
        u64 v = 0;
        for (int i = 0; i < 8; ++i) v |= ((u64)p[i]) << (8 * i);
        p += 8; return v;
    }
    std::string raw(size_t n) {
        if (!need(n)) { bad(); return std::string(); }
        std::string s((const char*)p, n);
        p += n; return s;
    }
    bool skip(size_t n) { if (!need(n)) return bad(); p += n; return true; }
};

// ---------------------------------------------------------------- 名称解码
// 宽松 UTF-8 校验（拒绝 overlong / 越界 / 孤立续字节 / 代理区）
bool IsValidUtf8(const std::string& s) {
    size_t i = 0, n = s.size();
    while (i < n) {
        u8 c = (u8)s[i];
        if (c < 0x80) { ++i; continue; }
        int extra = 0;
        u32 cp = 0;
        if ((c & 0xE0) == 0xC0) { extra = 1; cp = c & 0x1Fu; if (cp < 2) return false; }
        else if ((c & 0xF0) == 0xE0) { extra = 2; cp = c & 0x0Fu; }
        else if ((c & 0xF8) == 0xF0) { extra = 3; cp = c & 0x07u; }
        else return false;
        if (i + extra >= n) return false;
        for (int k = 0; k < extra; ++k) {
            u8 cc = (u8)s[i + 1 + k];
            if ((cc & 0xC0) != 0x80) return false;
            cp = (cp << 6) | (cc & 0x3Fu);
        }
        // overlong / 非法范围
        if (extra == 2 && cp < 0x800) return false;
        if (extra == 3 && cp < 0x10000) return false;
        if (cp > 0x10FFFF) return false;
        if (cp >= 0xD800 && cp <= 0xDFFF) return false;
        i += (size_t)extra + 1;
    }
    return true;
}

// CP437 高位半区 0x80-0xFF -> Unicode 码点（近似即可，MC 里用不到）
const u16 kCp437High[128] = {
    0x00C7,0x00FC,0x00E9,0x00E2,0x00E4,0x00E0,0x00E5,0x00E7,0x00EA,0x00EB,0x00E8,0x00EF,0x00EE,0x00EC,0x00C4,0x00C5,
    0x00C9,0x00E6,0x00C6,0x00F4,0x00F6,0x00F2,0x00FB,0x00F9,0x00FF,0x00D6,0x00DC,0x00A2,0x00A3,0x00A5,0x20A7,0x0192,
    0x00E1,0x00ED,0x00F3,0x00FA,0x00F1,0x00D1,0x00AA,0x00BA,0x00BF,0x2310,0x00AC,0x00BD,0x00BC,0x00A1,0x00AB,0x00BB,
    0x2591,0x2592,0x2593,0x2502,0x2524,0x2561,0x2562,0x2556,0x2555,0x2563,0x2551,0x2557,0x255D,0x255C,0x255B,0x2510,
    0x2514,0x2534,0x252C,0x251C,0x2500,0x253C,0x255E,0x255F,0x255A,0x2554,0x2569,0x2566,0x2560,0x2550,0x256C,0x2567,
    0x2568,0x2564,0x2565,0x2559,0x2558,0x2552,0x2553,0x256B,0x256A,0x2518,0x250C,0x2588,0x2584,0x258C,0x2590,0x2580,
    0x03B1,0x00DF,0x0393,0x03C0,0x03A3,0x03C3,0x00B5,0x03C4,0x03A6,0x0398,0x03A9,0x03B4,0x221E,0x03C6,0x03B5,0x2229,
    0x2261,0x00B1,0x2265,0x2264,0x2320,0x2321,0x00F7,0x2248,0x00B0,0x2219,0x00B7,0x221A,0x207F,0x00B2,0x25A0,0x00A0
};

void AppendUtf8(std::string& out, u32 cp) {
    if (cp < 0x80) out.push_back((char)cp);
    else if (cp < 0x800) {
        out.push_back((char)(0xC0 | (cp >> 6)));
        out.push_back((char)(0x80 | (cp & 0x3F)));
    } else if (cp < 0x10000) {
        out.push_back((char)(0xE0 | (cp >> 12)));
        out.push_back((char)(0x80 | ((cp >> 6) & 0x3F)));
        out.push_back((char)(0x80 | (cp & 0x3F)));
    } else {
        out.push_back((char)(0xF0 | (cp >> 18)));
        out.push_back((char)(0x80 | ((cp >> 12) & 0x3F)));
        out.push_back((char)(0x80 | ((cp >> 6) & 0x3F)));
        out.push_back((char)(0x80 | (cp & 0x3F)));
    }
}

// 文件名解码：EFS(bit11) 或合法 UTF-8 -> 原样；否则按 CP437 近似
std::string DecodeName(const std::string& raw, u16 flags) {
    bool efs = (flags & 0x0800) != 0;
    if (efs || IsValidUtf8(raw)) return raw;
    std::string out;
    out.reserve(raw.size() * 2);
    for (unsigned char c : raw) {
        if (c < 0x80) out.push_back((char)c);
        else AppendUtf8(out, kCp437High[c - 0x80]);
    }
    return out;
}

} // namespace

// DOS 时间 -> "YYYY-MM-DD HH:MM:SS"（Entry::TimeString 的实现）
std::string Entry::TimeString() const {
    u16 t = (u16)(dosTime & 0xFFFF);            // 低 16 位 = MS-DOS 时间
    u16 d = (u16)(dosTime >> 16);               // 高 16 位 = MS-DOS 日期
    if (dosTime == 0) return std::string();     // 未设置
    char buf[32];
    int year = 1980 + ((d >> 9) & 0x7F), mon = (d >> 5) & 0x0F, day = d & 0x1F;
    int hh = (t >> 11) & 0x1F, mm = (t >> 5) & 0x3F, ss = (t & 0x1F) * 2;
    snprintf(buf, sizeof(buf), "%04d-%02d-%02d %02d:%02d:%02d", year, mon, day, hh, mm, ss);
    return std::string(buf);
}

u32 Crc32(const u8* data, size_t size) {
    if (!data || size == 0) return (u32)crc32(0L, Z_NULL, 0);
    return (u32)crc32(crc32(0L, Z_NULL, 0), data, (uInt)size);
}

namespace {

// raw deflate 解压（负窗口 -15，无 zlib/gzip 头）
bool InflateRaw(const u8* src, size_t srcLen, size_t expOut, std::vector<u8>& out, std::string& err) {
    out.clear();
    if (expOut == 0) return true;
    if (expOut > kMaxEntryOut) { err = "条目解压尺寸过大，已拒绝"; return false; }
    out.resize(expOut);
    z_stream zs;
    memset(&zs, 0, sizeof(zs));
    if (inflateInit2(&zs, -15) != Z_OK) { err = "inflateInit2 失败"; return false; }
    zs.next_in = (Bytef*)src;
    zs.avail_in = (uInt)srcLen;
    zs.next_out = out.data();
    zs.avail_out = (uInt)expOut;
    int r = inflate(&zs, Z_FINISH);
    size_t produced = expOut - zs.avail_out;
    inflateEnd(&zs);
    if (r != Z_STREAM_END) {
        out.clear();
        err = "deflate 解压失败 (zlib code " + std::to_string(r) + "，已产出 " +
              std::to_string(produced) + "/" + std::to_string(expOut) + " 字节)";
        return false;
    }
    out.resize(produced);
    return true;
}

} // namespace

// ------------------------------------------------------------------ 打开
void Archive::Warn(const std::string& w) const { warnings_.push_back(w); }

std::string Archive::TakeWarnings() const {
    std::string s;
    for (size_t i = 0; i < warnings_.size(); ++i) {
        if (i) s += " | ";
        s += warnings_[i];
    }
    warnings_.clear();
    return s;
}

bool Archive::Open(const std::wstring& path, std::string& err) {
    Close();
    std::vector<u8> raw;
    if (!fsx::ReadAllBytes(path, raw)) { err = "无法读取文件"; return false; }
    if (raw.empty()) { err = "文件为空"; return false; }
    srcPath_ = path;
    return OpenMemory(raw.data(), raw.size(), err);
}

bool Archive::OpenMemory(const u8* data, size_t size, std::string& err) {
    Close();
    err.clear();
    if (!data || size < 22) { err = "数据过短，不是 ZIP"; return false; }
    data_.assign(data, data + size);
    const u8* base = data_.data();
    const size_t n = data_.size();

    // ---- 1) 从尾部向前找 EOCD（最多回扫 64KiB + 22）
    size_t backLimit = (n > (size_t)(0xFFFF + 22)) ? (n - (size_t)(0xFFFF + 22)) : 0;
    size_t eocdPos = SIZE_MAX;
    size_t loosePos = SIZE_MAX;   // 注释长度字段不可信时的候选
    for (size_t i = n - 22 + 1; i-- > backLimit; ) {
        if (base[i] == 'P' && base[i + 1] == 'K' && base[i + 2] == 0x05 && base[i + 3] == 0x06) {
            u16 clen = (u16)((u16)base[i + 20] | ((u16)base[i + 21] << 8));
            if (i + 22 + (size_t)clen == n) { eocdPos = i; break; }   // 尾部严格吻合，最可信
            if (loosePos == SIZE_MAX) loosePos = i;                   // 继续往前找更吻合的
        }
        if (i == 0) break;
    }
    if (eocdPos == SIZE_MAX) eocdPos = loosePos;
    if (eocdPos == SIZE_MAX) { err = "找不到 ZIP 结束记录 (EOCD)，可能不是 ZIP 文件"; return false; }

    BinReader r{ base + eocdPos + 4, base + n, true };
    u16 diskNum    = r.u16v();
    u16 diskCd     = r.u16v();
    u16 numOnDisk  = r.u16v();
    u16 numTotal   = r.u16v();
    u32 cdSize     = r.u32v();
    u32 cdOffset   = r.u32v();
    u16 commentLen = r.u16v();
    (void)diskNum; (void)diskCd; (void)numOnDisk; (void)commentLen;
    if (!r.ok) { err = "EOCD 结构损坏"; return false; }

    bool needZip64 = (numTotal == 0xFFFF) || (cdOffset == 0xFFFFFFFFu) || (cdSize == 0xFFFFFFFFu);

    if (needZip64) {
        // ---- 2) ZIP64：EOCD 之前 20 字节处是 ZIP64 EOCD 定位器 PK\x06\x07
        std::string z64err;
        bool got = false;
        if (eocdPos >= 20) {
            const u8* loc = base + eocdPos - 20;
            BinReader lr{ loc, base + eocdPos, true };
            u32 sig = lr.u32v();
            if (sig == kSigZip64EocdLoc) {
                u32 zdisk = lr.u32v();
                u64 zoff  = lr.u64v();
                u32 ztotal = lr.u32v();
                (void)zdisk;
                if (lr.ok && zoff <= n && (u64)n - zoff >= 56) {
                    BinReader zr{ base + (size_t)zoff, base + n, true };
                    u32 zsig = zr.u32v();
                    if (zsig == kSigZip64Eocd) {
                        zr.u64v();            // size of zip64 EOCD record
                        zr.u16v(); zr.u16v(); // version made by / needed
                        zr.u32v(); zr.u32v(); // disk numbers
                        zr.u64v(); zr.u64v(); // entries on disk / total
                        u64 zcdSize = zr.u64v();
                        u64 zcdOff  = zr.u64v();
                        if (zr.ok) {
                            if (zcdOff > n || zcdSize > n || zcdOff + zcdSize > n) {
                                z64err = "ZIP64 中央目录范围越界";
                            } else {
                                cdOffset = (u32)zcdOff;
                                cdSize   = (u32)zcdSize;
                                numTotal = (u16)(ztotal > 0xFFFF ? 0xFFFF : ztotal);
                                got = true;
                            }
                        } else z64err = "ZIP64 EOCD 记录字段不完整";
                    } else z64err = "ZIP64 EOCD 记录签名不匹配";
                } else z64err = "ZIP64 EOCD 定位器偏移越界";
            } else z64err = "未找到 ZIP64 EOCD 定位器";
        } else z64err = "文件过短，无法容纳 ZIP64 结构";
        if (!got) {
            err = "需要 ZIP64 但解析失败: " + z64err + "（本实现只做有限 ZIP64 支持）";
            Close();
            return false;
        }
    }

    if (cdOffset > n || cdSize > n || (size_t)cdOffset + (size_t)cdSize > n) {
        err = "中央目录范围越界（EOCD 字段不可信）";
        Close();
        return false;
    }

    // ---- 3) 遍历中央目录 PK\x01\x02
    BinReader c{ base + cdOffset, base + cdOffset + cdSize, true };
    entries_.clear();
    entries_.reserve(numTotal);
    u32 parsed = 0;
    while (c.ok && c.p < c.end) {
        size_t avail = (size_t)(c.end - c.p);
        if (avail < 46) break;
        size_t recStart = (size_t)(c.p - base);
        u32 sig = c.u32v();
        if (sig != kSigCentral) { Warn("中央目录在偏移 " + std::to_string(recStart) + " 处签名异常，停止解析"); break; }
        c.u16v(); c.u16v();            // version made by / needed
        u16 flags  = c.u16v();
        u16 method = c.u16v();
        u16 dosTime = c.u16v();        // 低 16 位 = MS-DOS 时间
        u16 dosDate = c.u16v();        // 高 16 位 = MS-DOS 日期
        u32 crc    = c.u32v();
        u32 csize  = c.u32v();
        u32 usize  = c.u32v();
        u16 nlen   = c.u16v();
        u16 elen   = c.u16v();
        u16 clen   = c.u16v();
        c.u16v(); c.u16v();            // disk start / internal attrs
        c.u32v();                      // external attrs
        u32 lho    = c.u32v();
        if (!c.ok) { Warn("中央目录记录字段被截断"); break; }
        if (!c.need((size_t)nlen + elen + clen)) { Warn("中央目录记录的可变字段被截断"); break; }
        std::string rawName = c.raw(nlen);
        std::string extra   = c.raw(elen);
        c.skip(clen);

        if (method != 0 && method != 8) {
            Warn("条目 '" + DecodeName(rawName, flags) + "' 使用不支持的压缩方法 " +
                 std::to_string(method) + "（已跳过）");
            ++parsed;
            continue;
        }

        // ZIP64 扩展字段（0x0001）：哨兵值出现在这里
        if (csize == 0xFFFFFFFFu || usize == 0xFFFFFFFFu || lho == 0xFFFFFFFFu) {
            BinReader er{ (const u8*)extra.data(), (const u8*)extra.data() + extra.size(), true };
            bool fixed = false;
            bool tooBig = false;
            while (er.ok && (size_t)(er.end - er.p) >= 4) {
                u16 hid = er.u16v();
                u16 hsz = er.u16v();
                if (!er.need(hsz)) break;
                if (hid == 0x0001) {
                    BinReader hr{ er.p, er.p + hsz, true };
                    // 字段顺序固定：uncompressed, compressed, local header offset
                    if (usize == 0xFFFFFFFFu) { u64 v = hr.u64v(); tooBig |= (v > 0xFFFFFFFFull); usize = (u32)v; }
                    if (csize == 0xFFFFFFFFu) { u64 v = hr.u64v(); tooBig |= (v > 0xFFFFFFFFull); csize = (u32)v; }
                    if (lho   == 0xFFFFFFFFu) { u64 v = hr.u64v(); tooBig |= (v > 0xFFFFFFFFull); lho   = (u32)v; }
                    fixed = hr.ok;
                    break;
                }
                er.skip(hsz);
            }
            if (!fixed || tooBig) {
                err = "条目 '" + DecodeName(rawName, flags) + "' 的 ZIP64 扩展字段" +
                      (tooBig ? "超出本实现支持的 4 GiB 上限" : "缺失或不完整");
                Close();
                return false;
            }
        }

        Entry e;
        e.name = DecodeName(rawName, flags);
        e.compressedSize = csize;
        e.uncompressedSize = usize;
        e.crc32 = crc;
        e.method = method;
        e.localHeaderOffset = lho;
        e.flags = flags;
        e.dosTime = ((u32)dosDate << 16) | dosTime;

        if (!e.name.empty() && e.name.back() == '/') { ++parsed; continue; } // 目录条目不入索引
        if (e.localHeaderOffset >= n) {
            Warn("条目 '" + e.name + "' 的本地头偏移越界（已忽略）");
            ++parsed;
            continue;
        }
        index_[e.name] = entries_.size();
        entries_.push_back(std::move(e));
        ++parsed;
        if (parsed > kMaxEntries) { Warn("中央目录条目数异常，停止解析"); break; }
    }

    if (entries_.empty() && parsed == 0) { err = "中央目录为空或无法解析"; Close(); return false; }
    ok_ = true;
    return true;
}

void Archive::Close() {
    data_.clear();
    entries_.clear();
    index_.clear();
    warnings_.clear();
    ok_ = false;
    srcPath_.clear();
}

// ------------------------------------------------------------------ 查询
const Entry* Archive::Find(const std::string& name) const {
    auto it = index_.find(name);
    return (it == index_.end()) ? nullptr : &entries_[it->second];
}

bool Archive::Has(const std::string& name) const { return index_.find(name) != index_.end(); }

void Archive::List(std::vector<std::string>& out, const std::string& prefix) const {
    out.clear();
    for (const auto& e : entries_) {
        if (prefix.empty() || e.name.compare(0, prefix.size(), prefix) == 0) out.push_back(e.name);
    }
}

// ------------------------------------------------------------------ 解压
bool Archive::Read(const std::string& name, std::vector<u8>& out, std::string& err) const {
    out.clear();
    err.clear();
    if (!ok_) { err = "归档未打开"; return false; }
    const Entry* e = Find(name);
    if (!e) { err = "条目不存在: " + name; return false; }

    const u8* base = data_.data();
    const size_t n = data_.size();
    size_t lho = e->localHeaderOffset;

    // ---- 解析本地头（只取 name/extra 长度，尺寸一律以中央目录为准）
    BinReader lr{ base + lho, base + n, true };
    u32 sig = lr.u32v();
    if (sig != kSigLocal) { err = "本地头签名错误: " + name; return false; }
    lr.u16v();                              // version needed
    u16 lflags = lr.u16v();
    lr.u16v();                              // method (以中央目录为准)
    lr.u16v(); lr.u16v();                   // time / date
    lr.u32v();                              // crc (可能为 0，数据描述符)
    lr.u32v();                              // compressed size (可能为 0)
    lr.u32v();                              // uncompressed size (可能为 0)
    u16 nlen = lr.u16v();
    u16 elen = lr.u16v();
    if (!lr.ok) { err = "本地头被截断: " + name; return false; }
    (void)lflags;
    if (!lr.need((size_t)nlen + elen)) { err = "本地头可变字段越界: " + name; return false; }
    lr.skip((size_t)nlen + elen);

    size_t dataOff = (size_t)(lr.p - base);
    if (dataOff > n || (size_t)e->compressedSize > n - dataOff) {
        err = "条目数据范围越界: " + name; return false;
    }
    const u8* src = base + dataOff;
    size_t srcLen = e->compressedSize;

    bool sizeMismatch = false;
    if (e->method == 0) {                       // -------- stored
        if (e->uncompressedSize != 0 && e->uncompressedSize != e->compressedSize) {
            // 以中央目录的 uncompressedSize 为准，但两者都应相等
            sizeMismatch = true;
        }
        size_t take = (e->uncompressedSize != 0) ? (size_t)e->uncompressedSize : srcLen;
        if (take > srcLen) { err = "stored 条目长度不一致: " + name; return false; }
        out.assign(src, src + take);
    } else if (e->method == 8) {                // -------- deflate (raw, -15)
        std::string derr;
        if (!InflateRaw(src, srcLen, e->uncompressedSize, out, derr)) {
            err = name + ": " + derr; return false;
        }
        if (!derr.empty()) Warn(name + ": " + derr);
    } else {
        err = "不支持的压缩方法 " + std::to_string(e->method) + ": " + name;
        return false;
    }

    if (sizeMismatch) Warn("条目 '" + name + "' 的 stored 长度字段不一致（以中央目录为准）");

    // ---- CRC 校验：不匹配只记录，不失败
    if (e->crc32 != 0 && !out.empty()) {
        u32 got = Crc32(out.data(), out.size());
        if (got != e->crc32) {
            char buf[96];
            snprintf(buf, sizeof(buf), "条目 '%s' CRC32 不符（记录 0x%08X 实际 0x%08X）",
                     name.c_str(), e->crc32, got);
            Warn(buf);
        }
    }
    return true;
}

bool Archive::ReadText(const std::string& name, std::string& out, std::string& err) const {
    std::vector<u8> bytes;
    out.clear();
    if (!Read(name, bytes, err)) return false;
    out.assign((const char*)bytes.data(), bytes.size());
    return true;
}

} // namespace zip
