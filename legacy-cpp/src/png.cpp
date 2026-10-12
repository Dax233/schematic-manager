// PNG 解码器实现 —— stored/deflate 无关；只解 IDAT（zlib）
#include "png.h"
#include <zlib.h>

namespace png {
namespace {

const u8 kSig[8] = { 0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A };

// 防御上限
const u64 kMaxPixels    = 64ull * 1024ull * 1024ull;   // 64M 像素
const u64 kMaxRawBytes  = 512ull * 1024ull * 1024ull;  // 解压后 IDAT 上限

u32 Be32(const u8* p) {
    return ((u32)p[0] << 24) | ((u32)p[1] << 16) | ((u32)p[2] << 8) | (u32)p[3];
}

// 每个像素的通道数
int ChannelsOf(int colorType) {
    switch (colorType) {
        case 0: return 1;  // 灰度
        case 2: return 3;  // RGB
        case 3: return 1;  // 调色板索引
        case 4: return 2;  // 灰度 + alpha
        case 6: return 4;  // RGBA
        default: return 0;
    }
}

bool ValidDepth(int colorType, int depth) {
    switch (colorType) {
        case 0: return depth == 1 || depth == 2 || depth == 4 || depth == 8 || depth == 16;
        case 2: return depth == 8 || depth == 16;
        case 3: return depth == 1 || depth == 2 || depth == 4 || depth == 8;
        case 4: return depth == 8 || depth == 16;
        case 6: return depth == 8 || depth == 16;
        default: return false;
    }
}

// 取第 q 个亚字节样本（depth<8，1 字节内打包）
inline u8 SubSample(const u8* row, size_t q, int depth) {
    if (depth == 1) return (row[q >> 3] >> (7 - (q & 7))) & 0x01;
    if (depth == 2) return (row[q >> 2] >> (6 - 2 * (q & 3))) & 0x03;
    return (row[q >> 1] >> (4 - 4 * (q & 1))) & 0x0F;  // depth == 4
}

inline int Paeth(int a, int b, int c) {
    int p = a + b - c;
    int pa = p > a ? p - a : a - p;
    int pb = p > b ? p - b : b - p;
    int pc = p > c ? p - c : c - p;
    if (pa <= pb && pa <= pc) return a;
    if (pb <= pc) return b;
    return c;
}

// 逐行反过滤（原地）
bool Unfilter(int bpp, int rowBytes, int h, std::vector<u8>& raw, std::string& err) {
    if (raw.empty()) return true;
    size_t need = (size_t)(rowBytes + 1) * (size_t)h;
    if (raw.size() < need) { err = "IDAT 解压数据不足（反过滤所需字节缺失）"; return false; }
    std::vector<u8> prev((size_t)rowBytes, 0);
    std::vector<u8> cur((size_t)rowBytes);
    const u8* src = raw.data();
    u8* dst = raw.data();
    for (int y = 0; y < h; ++y) {
        const u8* in = src + (size_t)y * (size_t)(rowBytes + 1);
        u8 ft = in[0];
        const u8* d = in + 1;
        switch (ft) {
            case 0:
                memcpy(cur.data(), d, (size_t)rowBytes);
                break;
            case 1:
                for (int i = 0; i < rowBytes; ++i)
                    cur[i] = (u8)(d[i] + (i >= bpp ? cur[i - bpp] : 0));
                break;
            case 2:
                for (int i = 0; i < rowBytes; ++i)
                    cur[i] = (u8)(d[i] + prev[i]);
                break;
            case 3:
                for (int i = 0; i < rowBytes; ++i) {
                    int a = (i >= bpp) ? cur[i - bpp] : 0;
                    cur[i] = (u8)(d[i] + ((a + prev[i]) >> 1));
                }
                break;
            case 4:
                for (int i = 0; i < rowBytes; ++i) {
                    int a = (i >= bpp) ? cur[i - bpp] : 0;
                    int c = (i >= bpp) ? prev[i - bpp] : 0;
                    cur[i] = (u8)(d[i] + Paeth(a, prev[i], c));
                }
                break;
            default: {
                char buf[128];
                snprintf(buf, sizeof(buf), "第 %d 行的 filter 类型 %u 非法（若是 Adam7 交错，本实现不支持）", y, ft);
                err = buf;
                return false;
            }
        }
        memcpy(dst, cur.data(), (size_t)rowBytes);
        memcpy(prev.data(), cur.data(), (size_t)rowBytes);
        dst += rowBytes;
    }
    raw.resize((size_t)rowBytes * (size_t)h);
    return true;
}

// raw 反过滤数据 -> RGBA
bool ToRgba(const std::vector<u8>& raw, int w, int h, int depth, int colorType,
            const std::vector<u8>& plte, const std::vector<u8>& trns, Image& out, std::string& err) {
    const int ch = ChannelsOf(colorType);
    const size_t rowBytes = ((size_t)w * (size_t)ch * (size_t)depth + 7) / 8;
    size_t need = rowBytes * (size_t)h;
    if (raw.size() < need) { err = "像素数据不足"; return false; }
    out.rgba.assign((size_t)w * (size_t)h * 4, 0);

    const bool gray = (colorType == 0 || colorType == 4);
    const bool rgb  = (colorType == 2 || colorType == 6);
    const bool pal  = (colorType == 3);
    const bool hasA = (colorType == 4 || colorType == 6);

    u16 trnsGray = 0, trnsR = 0, trnsG = 0, trnsB = 0;
    bool hasTrnsKey = false;
    if (colorType == 0 && trns.size() >= 2) { trnsGray = (u16)((trns[0] << 8) | trns[1]); hasTrnsKey = true; }
    if (colorType == 2 && trns.size() >= 6) {
        trnsR = (u16)((trns[0] << 8) | trns[1]);
        trnsG = (u16)((trns[2] << 8) | trns[3]);
        trnsB = (u16)((trns[4] << 8) | trns[5]);
        hasTrnsKey = true;
    }

    for (int y = 0; y < h; ++y) {
        const u8* row = raw.data() + (size_t)y * rowBytes;
        u8* dst = out.rgba.data() + (size_t)y * (size_t)w * 4;
        for (int x = 0; x < w; ++x) {
            u8 r = 0, g = 0, b = 0, a = 255;

            if (depth == 8) {
                if (gray) {
                    u8 v = row[x * ch];
                    r = g = b = v;
                    if (hasA) a = row[x * ch + 1];
                    else if (hasTrnsKey && v == (u8)trnsGray) a = 0;
                } else if (rgb) {
                    r = row[x * ch]; g = row[x * ch + 1]; b = row[x * ch + 2];
                    if (hasA) a = row[x * ch + 3];
                    else if (hasTrnsKey && r == (u8)trnsR && g == (u8)trnsG && b == (u8)trnsB) a = 0;
                } else { // pal
                    u8 idx = row[x];
                    if ((size_t)idx * 3 + 2 < plte.size()) { r = plte[idx * 3]; g = plte[idx * 3 + 1]; b = plte[idx * 3 + 2]; }
                    if (idx < trns.size()) a = trns[idx];
                }
            } else if (depth == 16) {
                if (gray) {
                    u8 v = row[x * ch * 2];                 // 取高字节（降采样）
                    r = g = b = v;
                    if (hasA) a = row[x * ch * 2 + 2];
                    else if (hasTrnsKey) {
                        u16 s = (u16)((row[x * ch * 2] << 8) | row[x * ch * 2 + 1]);
                        if (s == trnsGray) a = 0;
                    }
                } else if (rgb) {
                    r = row[x * ch * 2]; g = row[x * ch * 2 + 2]; b = row[x * ch * 2 + 4];
                    if (hasA) a = row[x * ch * 2 + 6];
                    else if (hasTrnsKey) {
                        u16 s0 = (u16)((row[x * ch * 2] << 8) | row[x * ch * 2 + 1]);
                        u16 s1 = (u16)((row[x * ch * 2 + 2] << 8) | row[x * ch * 2 + 3]);
                        u16 s2 = (u16)((row[x * ch * 2 + 4] << 8) | row[x * ch * 2 + 5]);
                        if (s0 == trnsR && s1 == trnsG && s2 == trnsB) a = 0;
                    }
                } else { err = "调色板图不支持 16 位深度"; return false; }
            } else {
                // depth 1/2/4：只可能是灰度或调色板
                u8 s = SubSample(row, (size_t)x * (size_t)ch, depth);   // ch == 1
                if (pal) {
                    if ((size_t)s * 3 + 2 < plte.size()) { r = plte[s * 3]; g = plte[s * 3 + 1]; b = plte[s * 3 + 2]; }
                    if (s < trns.size()) a = trns[s];
                } else {
                    int maxv = (1 << depth) - 1;
                    u8 v = (u8)((s * 255 + maxv / 2) / maxv);           // 0->0, max->255
                    r = g = b = v;
                    if (hasTrnsKey && s == (u8)trnsGray) a = 0;
                }
            }

            dst[x * 4 + 0] = r;
            dst[x * 4 + 1] = g;
            dst[x * 4 + 2] = b;
            dst[x * 4 + 3] = a;
        }
    }
    return true;
}

// 原始（未过滤）deflate 数据解压
bool InflateAll(const u8* src, size_t srcLen, size_t expOut, std::vector<u8>& out, std::string& err) {
    if (expOut == 0 || expOut > kMaxRawBytes) { err = "IDAT 期望尺寸非法"; return false; }
    std::vector<u8> in;
    in.assign(src, src + srcLen);
    out.assign(expOut, 0);
    z_stream zs;
    memset(&zs, 0, sizeof(zs));
    if (inflateInit(&zs) != Z_OK) { err = "inflateInit 失败"; return false; }
    zs.next_in = in.data();
    zs.avail_in = (uInt)in.size();
    zs.next_out = out.data();
    zs.avail_out = (uInt)expOut;
    int r = inflate(&zs, Z_FINISH);
    size_t produced = expOut - zs.avail_out;
    inflateEnd(&zs);
    if (r != Z_STREAM_END) {
        char buf[160];
        snprintf(buf, sizeof(buf), "IDAT zlib 解压失败 (code %d，产出 %zu/%zu 字节)", r, produced, expOut);
        err = buf;
        out.clear();
        return false;
    }
    out.resize(produced);
    return true;
}

// 追加 chunk 到 PNG 输出
void PutChunk(std::vector<u8>& out, const char type[4], const u8* data, size_t len) {
    u8 hdr[8];
    hdr[0] = (u8)(len >> 24); hdr[1] = (u8)(len >> 16); hdr[2] = (u8)(len >> 8); hdr[3] = (u8)len;
    memcpy(hdr + 4, type, 4);
    out.insert(out.end(), hdr, hdr + 8);
    if (len) out.insert(out.end(), data, data + len);
    u32 crc = (u32)crc32(crc32(0L, Z_NULL, 0), out.data() + out.size() - len - 4, (uInt)(len + 4));
    u8 cb[4] = { (u8)(crc >> 24), (u8)(crc >> 16), (u8)(crc >> 8), (u8)crc };
    out.insert(out.end(), cb, cb + 4);
}

} // namespace

// ------------------------------------------------------------------ 解码
bool Decode(const u8* data, size_t size, Image& out, std::string& err) {
    out.w = 0; out.h = 0; out.rgba.clear();
    err.clear();
    if (!data || size < 8) { err = "数据过短，不是 PNG"; return false; }
    if (memcmp(data, kSig, 8) != 0) { err = "PNG 签名不匹配"; return false; }

    int w = 0, h = 0, depth = 0, colorType = 0, interlace = 0;
    bool sawIhdr = false, sawIend = false;
    std::vector<u8> plte, trns, idat;
    size_t pos = 8;

    while (pos + 8 <= size) {
        u32 len = Be32(data + pos);
        const char* type = (const char*)(data + pos + 4);
        size_t dataOff = pos + 8;
        if ((u64)dataOff + (u64)len + 4u > (u64)size) { err = "chunk 长度越界（数据被截断）"; return false; }
        const u8* cd = data + dataOff;
        pos = dataOff + (size_t)len + 4;   // 跳过 chunk 数据 + CRC（CRC 不做强制校验）

        if (memcmp(type, "IHDR", 4) == 0) {
            if (len != 13) { err = "IHDR 长度非法"; return false; }
            w = (int)Be32(cd);
            h = (int)Be32(cd + 4);
            depth = cd[8];
            colorType = cd[9];
            int comp = cd[10], filt = cd[11];
            interlace = cd[12];
            sawIhdr = true;
            if (w <= 0 || h <= 0) { err = "IHDR 尺寸非法"; return false; }
            if ((u64)w * (u64)h > kMaxPixels) { err = "图像尺寸过大，已拒绝"; return false; }
            if (comp != 0) { err = "不支持的压缩方法（IHDR 第 10 字节 != 0）"; return false; }
            if (filt != 0) { err = "不支持的 filter 方法（IHDR 第 11 字节 != 0）"; return false; }
            if (interlace == 1) { err = "不支持 Adam7 交错 PNG"; return false; }
            if (interlace != 0) { err = "IHDR 交错方法非法"; return false; }
            if (ChannelsOf(colorType) == 0) { err = "不支持的 color type " + std::to_string(colorType); return false; }
            if (!ValidDepth(colorType, depth)) {
                err = "color type " + std::to_string(colorType) + " 不支持 bit depth " + std::to_string(depth);
                return false;
            }
        } else if (memcmp(type, "PLTE", 4) == 0) {
            plte.assign(cd, cd + len);
        } else if (memcmp(type, "tRNS", 4) == 0) {
            trns.assign(cd, cd + len);
        } else if (memcmp(type, "IDAT", 4) == 0) {
            if (!sawIhdr) { err = "IDAT 出现在 IHDR 之前"; return false; }
            if (idat.size() + len > kMaxRawBytes) { err = "IDAT 数据过多，已拒绝"; return false; }
            idat.insert(idat.end(), cd, cd + len);
        } else if (memcmp(type, "IEND", 4) == 0) {
            sawIend = true;
            break;
        }
        // 其它 chunk（gAMA/cHRM/iCCP/tEXt/...）一律忽略
    }

    if (!sawIhdr) { err = "缺少 IHDR"; return false; }
    if (!sawIend) { err = "缺少 IEND（文件可能被截断）"; return false; }
    if (idat.empty()) { err = "缺少 IDAT 数据"; return false; }
    if (colorType == 3) {
        if (plte.empty() || plte.size() % 3 != 0) { err = "调色板 PNG 缺少合法的 PLTE"; return false; }
        if (trns.size() > plte.size() / 3) trns.resize(plte.size() / 3);
    }

    const int ch = ChannelsOf(colorType);
    const int bpp = (ch * depth + 7) / 8;                 // 每像素字节数，最小 1
    const size_t rowBytes = ((size_t)w * (size_t)ch * (size_t)depth + 7) / 8;
    u64 expected = ((u64)rowBytes + 1ull) * (u64)h;
    if (expected > kMaxRawBytes) { err = "解压后数据过大，已拒绝"; return false; }

    std::vector<u8> raw;
    if (!InflateAll(idat.data(), idat.size(), (size_t)expected, raw, err)) return false;
    if (!Unfilter(bpp, (int)rowBytes, h, raw, err)) return false;
    if (!ToRgba(raw, w, h, depth, colorType, plte, trns, out, err)) { out.w = 0; out.h = 0; return false; }
    out.w = w; out.h = h;
    return true;
}

bool DecodeFirstFrame(const u8* data, size_t size, Image& out, std::string& err) {
    Image full;
    if (!Decode(data, size, full, err)) return false;
    if (full.Empty()) { err = "解码结果为空"; return false; }
    if (full.h <= full.w) { out = std::move(full); return true; }   // 已是单帧（或非纵向拼接）

    // 纵向多帧：裁出顶部 w x w 的第一帧
    int w = full.w;
    Image one;
    one.w = w; one.h = w;
    one.rgba.resize((size_t)w * (size_t)w * 4);
    for (int y = 0; y < w; ++y)
        memcpy(one.rgba.data() + (size_t)y * w * 4, full.rgba.data() + (size_t)y * w * 4, (size_t)w * 4);
    out = std::move(one);
    return true;
}

// ------------------------------------------------------------------ 编码（极简：RGBA8 / filter 0）
bool Encode(const Image& img, std::vector<u8>& out) {
    out.clear();
    if (img.Empty() || (size_t)img.w * (size_t)img.h * 4 != img.rgba.size()) return false;
    const int w = img.w, h = img.h;

    out.insert(out.end(), kSig, kSig + 8);

    u8 ihdr[13];
    ihdr[0] = (u8)(w >> 24); ihdr[1] = (u8)(w >> 16); ihdr[2] = (u8)(w >> 8); ihdr[3] = (u8)w;
    ihdr[4] = (u8)(h >> 24); ihdr[5] = (u8)(h >> 16); ihdr[6] = (u8)(h >> 8); ihdr[7] = (u8)h;
    ihdr[8] = 8; ihdr[9] = 6; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;
    PutChunk(out, "IHDR", ihdr, 13);

    // 每行前置 filter 0，然后 zlib 压缩
    std::vector<u8> raw;
    raw.reserve(((size_t)w * 4 + 1) * (size_t)h);
    for (int y = 0; y < h; ++y) {
        raw.push_back(0);
        const u8* src = img.rgba.data() + (size_t)y * (size_t)w * 4;
        raw.insert(raw.end(), src, src + (size_t)w * 4);
    }
    uLongf bound = compressBound((uLong)raw.size());
    std::vector<u8> z(bound);
    if (compress2(z.data(), &bound, raw.data(), (uLong)raw.size(), 6) != Z_OK) return false;
    z.resize(bound);
    PutChunk(out, "IDAT", z.data(), z.size());
    PutChunk(out, "IEND", nullptr, 0);
    return true;
}

} // namespace png
