// ZIP 读取器 + PNG 解码器验证（由 tests/test_core.cpp 的 main 调用）
#include "../src/zip.h"
#include "../src/png.h"
#include "../src/fs.h"
#include "../src/str.h"
#include "test_common.h"
#include <cstdio>
#include <random>
#include <map>
#include <zlib.h>

namespace {

const wchar_t* kGameJar = L"E:\\\u6e38\u620f\\@\\26.2F\\.minecraft\\versions\\26.2-Fabric\\26.2-Fabric.jar";
const wchar_t* kModJar  = L"E:\\\u6e38\u620f\\@\\26.2F\\.minecraft\\mods\\fabric-api-0.155.0+26.2.jar";
const wchar_t* kOutDir  = L"C:\\Users\\hsds\\Documents\\deepseek-harness\\default-workspace\\projmgr\\build\\_zip_png_test";

// ---------------------------------------------------------------- MD5（自实现）
struct Md5 {
    u32 a = 0x67452301, b = 0xefcdab89, c = 0x98badcfe, d = 0x10325476;
    u64 total = 0;
    u8 buf[64];
    size_t bufLen = 0;

    static u32 rol(u32 x, int s) { return (x << s) | (x >> (32 - s)); }

    void block(const u8* p) {
        static const u32 K[64] = {
            0xd76aa478,0xe8c7b756,0x242070db,0xc1bdceee,0xf57c0faf,0x4787c62a,0xa8304613,0xfd469501,
            0x698098d8,0x8b44f7af,0xffff5bb1,0x895cd7be,0x6b901122,0xfd987193,0xa679438e,0x49b40821,
            0xf61e2562,0xc040b340,0x265e5a51,0xe9b6c7aa,0xd62f105d,0x02441453,0xd8a1e681,0xe7d3fbc8,
            0x21e1cde6,0xc33707d6,0xf4d50d87,0x455a14ed,0xa9e3e905,0xfcefa3f8,0x676f02d9,0x8d2a4c8a,
            0xfffa3942,0x8771f681,0x6d9d6122,0xfde5380c,0xa4beea44,0x4bdecfa9,0xf6bb4b60,0xbebfbc70,
            0x289b7ec6,0xeaa127fa,0xd4ef3085,0x04881d05,0xd9d4d039,0xe6db99e5,0x1fa27cf8,0xc4ac5665,
            0xf4292244,0x432aff97,0xab9423a7,0xfc93a039,0x655b59c3,0x8f0ccc92,0xffeff47d,0x85845dd1,
            0x6fa87e4f,0xfe2ce6e0,0xa3014314,0x4e0811a1,0xf7537e82,0xbd3af235,0x2ad7d2bb,0xeb86d391 };
        static const int S[64] = {
            7,12,17,22,7,12,17,22,7,12,17,22,7,12,17,22,
            5,9,14,20,5,9,14,20,5,9,14,20,5,9,14,20,
            4,11,16,23,4,11,16,23,4,11,16,23,4,11,16,23,
            6,10,15,21,6,10,15,21,6,10,15,21,6,10,15,21 };
        u32 M[16];
        for (int i = 0; i < 16; ++i)
            M[i] = (u32)p[i * 4] | ((u32)p[i * 4 + 1] << 8) | ((u32)p[i * 4 + 2] << 16) | ((u32)p[i * 4 + 3] << 24);
        u32 A = a, B = b, C = c, D = d;
        for (int i = 0; i < 64; ++i) {
            u32 F; int g;
            if (i < 16)      { F = (B & C) | (~B & D);   g = i; }
            else if (i < 32) { F = (D & B) | (~D & C);   g = (5 * i + 1) & 15; }
            else if (i < 48) { F = B ^ C ^ D;            g = (3 * i + 5) & 15; }
            else             { F = C ^ (B | ~D);         g = (7 * i) & 15; }
            F = F + A + K[i] + M[g];
            A = D; D = C; C = B;
            B = B + rol(F, S[i]);
        }
        a += A; b += B; c += C; d += D;
    }

    void Update(const u8* p, size_t n) {
        total += n;
        while (n) {
            size_t take = 64 - bufLen;
            if (take > n) take = n;
            memcpy(buf + bufLen, p, take);
            bufLen += take; p += take; n -= take;
            if (bufLen == 64) { block(buf); bufLen = 0; }
        }
    }

    std::string Hex() {
        u64 bits = total * 8;          // 必须在补位之前取
        u8 pad = 0x80;
        Update(&pad, 1);
        u8 zero = 0;
        while (bufLen != 56) Update(&zero, 1);
        u8 lenb[8];
        for (int i = 0; i < 8; ++i) lenb[i] = (u8)(bits >> (8 * i));
        Update(lenb, 8);
        char s[33];
        u32 h[4] = { a, b, c, d };
        for (int i = 0; i < 4; ++i)
            for (int k = 0; k < 4; ++k)
                snprintf(s + i * 8 + k * 2, 3, "%02x", (unsigned)((h[i] >> (8 * k)) & 0xFF));
        return std::string(s, 32);
    }
};

std::string Md5Of(const std::vector<u8>& v) { Md5 m; m.Update(v.data(), v.size()); return m.Hex(); }

// 把 RGBA 铺成 "R,G,B,A\n" 文本，与 Pillow 端逐像素对拍用
std::vector<u8> RgbaToText(const png::Image& im) {
    std::vector<u8> out;
    char line[32];
    for (int y = 0; y < im.h; ++y)
        for (int x = 0; x < im.w; ++x) {
            const u8* p = im.rgba.data() + ((size_t)y * im.w + x) * 4;
            int n = snprintf(line, sizeof(line), "%u,%u,%u,%u\n", p[0], p[1], p[2], p[3]);
            out.insert(out.end(), line, line + n);
        }
    return out;
}

bool DeflateRaw(const std::string& in, std::vector<u8>& out) {
    z_stream zs;
    memset(&zs, 0, sizeof(zs));
    if (deflateInit2(&zs, 6, Z_DEFLATED, -15, 8, Z_DEFAULT_STRATEGY) != Z_OK) return false;
    out.assign(in.size() / 2 + 64, 0);
    zs.next_in = (Bytef*)in.data();
    zs.avail_in = (uInt)in.size();
    zs.next_out = out.data();
    zs.avail_out = (uInt)out.size();
    int r = deflate(&zs, Z_FINISH);
    size_t produced = out.size() - zs.avail_out;
    deflateEnd(&zs);
    if (r != Z_STREAM_END) return false;
    out.resize(produced);
    return true;
}

} // namespace

void TestZipPng() {
    printf("=== 5) ZIP 读取器 + PNG 解码器验证 ===\n\n");
    fsx::EnsureDir(kOutDir);

    // ---------------------------------------------------------- 1) 打开游戏 jar
    printf("--- 1) 打开 Minecraft jar ---\n");
    zip::Archive jar;
    std::string err;
    bool opened = jar.Open(kGameJar, err);
    Check(opened, "打开 26.2-Fabric.jar");
    if (!opened) { printf("    打开失败: %s\n", err.c_str()); return; }
    printf("    条目总数 = %zu\n", jar.Count());
    Check(jar.Has("assets/minecraft/blockstates/stone.json"), "包含 blockstates/stone.json");
    Check(jar.Has("assets/minecraft/models/block/stone.json"), "包含 models/block/stone.json");
    Check(jar.Count() > 10000, "条目数 > 10000");
    {
        std::vector<std::string> names;
        jar.List(names, "assets/minecraft/blockstates/");
        printf("    blockstates/ 前缀条目数 = %zu（示例: %s）\n",
               names.size(), names.empty() ? "-" : names[0].c_str());
        Check(!names.empty(), "按前缀列举非空");
    }
    {
        std::string w = jar.TakeWarnings();
        printf("    打开警告: %s\n", w.empty() ? "无" : w.c_str());
    }
    {
        const zip::Entry* e = jar.Find("assets/minecraft/blockstates/stone.json");
        std::string ts = e ? e->TimeString() : std::string();
        printf("    stone.json 的 DOS 时间 = \"%s\"\n", ts.c_str());
        // 该条目在 jar 里的 DOS 日期是 1980-02-01 00:00:00
        Check(ts == "1980-02-01 00:00:00", "Entry::TimeString 解析 DOS 时间正确");
    }

    // ---------------------------------------------------------- 2) 解出 blockstate JSON
    printf("\n--- 2) 解出 blockstates/stone.json ---\n");
    std::string json;
    bool gotJson = jar.ReadText("assets/minecraft/blockstates/stone.json", json, err);
    Check(gotJson, "解出 stone.json");
    if (gotJson) {
        const zip::Entry* e = jar.Find("assets/minecraft/blockstates/stone.json");
        printf("    压缩后=%u B 解压后=%u B method=%u CRC32=%08X\n",
               e ? e->compressedSize : 0, e ? e->uncompressedSize : 0,
               e ? (unsigned)e->method : 0, e ? e->crc32 : 0);
        printf("    前 200 字节:\n");
        std::string head = json.substr(0, 200);
        printf("    ------------------------------------------------------------\n");
        for (size_t i = 0; i < head.size(); i += 100)
            printf("    %s\n", head.substr(i, 100).c_str());
        printf("    ------------------------------------------------------------\n");
        Check(json.find('{') != std::string::npos && json.find('}') != std::string::npos, "是合法 JSON 文本");
        Check(json.find("variants") != std::string::npos, "包含 variants 键");
        u32 crc = zip::Crc32((const u8*)json.data(), json.size());
        Check(e && crc == e->crc32, "自算 CRC32 与中央目录一致");
    }

    // ---------------------------------------------------------- 3) stone.png
    printf("\n--- 3) 解码 textures/block/stone.png 并回写 ---\n");
    std::vector<u8> stonePng;
    Check(jar.Read("assets/minecraft/textures/block/stone.png", stonePng, err), "读取 stone.png 原始字节");
    png::Image stone;
    bool okStone = png::Decode(stonePng.data(), stonePng.size(), stone, err);
    Check(okStone, "解码 stone.png");
    if (okStone) {
        printf("    解码结果: %dx%d  (%zu 字节 RGBA)\n", stone.w, stone.h, stone.rgba.size());
        std::vector<u8> reenc;
        Check(png::Encode(stone, reenc), "PNG 编码");
        std::wstring outPng = fsx::JoinPath(kOutDir, L"stone.png");
        Check(fsx::WriteAllBytes(outPng, reenc), "写出 stone.png");
        printf("    已写出: %s (%zu B)\n", WideToUtf8(outPng).c_str(), reenc.size());

        png::Image back;
        std::string e2;
        Check(png::Decode(reenc.data(), reenc.size(), back, e2), "回读自己编码的 PNG");
        Check(back.w == stone.w && back.h == stone.h && back.rgba == stone.rgba, "编码->解码 像素完全一致");
    } else {
        printf("    解码失败: %s\n", err.c_str());
    }

    // ---------------------------------------------------------- 4) water_still.png（动画）
    printf("\n--- 4) 解码动画贴图 water_still.png（只取第一帧）---\n");
    std::vector<u8> waterPng;
    Check(jar.Read("assets/minecraft/textures/block/water_still.png", waterPng, err), "读取 water_still.png");
    png::Image waterFull;
    std::string e3;
    bool okFull = png::Decode(waterPng.data(), waterPng.size(), waterFull, e3);
    Check(okFull, "整幅解码 water_still.png");
    if (okFull)
        printf("    整幅 = %dx%d（纵向 %d 帧）\n", waterFull.w, waterFull.h,
               waterFull.w ? waterFull.h / waterFull.w : 0);
    png::Image water;
    std::string e4;
    bool okWater = png::DecodeFirstFrame(waterPng.data(), waterPng.size(), water, e4);
    Check(okWater, "DecodeFirstFrame(water_still.png)");
    if (okWater) {
        printf("    DecodeFirstFrame = %dx%d\n", water.w, water.h);
        Check(water.w == 16 && water.h == 16, "第一帧裁剪为 16x16");
        std::vector<u8> re;
        Check(png::Encode(water, re), "编码水贴图第一帧");
        std::wstring outPng = fsx::JoinPath(kOutDir, L"water_still_frame0.png");
        Check(fsx::WriteAllBytes(outPng, re), "写出 water_still_frame0.png");
        printf("    已写出: %s\n", WideToUtf8(outPng).c_str());
    } else {
        printf("    失败: %s\n", e4.c_str());
    }
    if (okFull) {
        std::vector<u8> re;
        if (png::Encode(waterFull, re))
            fsx::WriteAllBytes(fsx::JoinPath(kOutDir, L"water_still_full.png"), re);
    }

    // ---------------------------------------------------------- 5) 与 Pillow 对拍
    printf("\n--- 5) 与 Pillow 对拍（RGBA 文本逐像素）---\n");
    if (okStone) {
        std::vector<u8> txt = RgbaToText(stone);
        std::wstring txtPath = fsx::JoinPath(kOutDir, L"stone_cpp_rgba.txt");
        fsx::WriteAllBytes(txtPath, txt);
        std::string cppMd5 = Md5Of(txt);
        printf("    C++ RGBA 文本: %s\n", WideToUtf8(txtPath).c_str());
        printf("    C++ 端 MD5 = %s\n", cppMd5.c_str());
        std::wstring sigPath = fsx::JoinPath(kOutDir, L"stone_cpp_md5.txt");
        fsx::WriteAllBytes(sigPath, std::vector<u8>(cppMd5.begin(), cppMd5.end()));
    }

    // ---------------------------------------------------------- 6) 模组 jar
    printf("\n--- 6) 模组 jar ---\n");
    zip::Archive mod;
    std::string merr;
    bool mok = mod.Open(kModJar, merr);
    Check(mok, "打开 mods/fabric-api-0.155.0+26.2.jar");
    if (mok) {
        printf("    条目总数 = %zu\n", mod.Count());
        std::vector<std::string> names;
        mod.List(names);
        printf("    前 5 条:");
        for (size_t i = 0; i < names.size() && i < 5; ++i) printf(" %s", names[i].c_str());
        printf("\n");
        std::string target;
        for (const auto& n : names)
            if (n.size() > 5 && n.compare(n.size() - 5, 5, ".json") == 0) { target = n; break; }
        Check(!target.empty(), "模组 jar 中含 .json");
        if (!target.empty()) {
            std::string body, je;
            bool jok = mod.ReadText(target, body, je);
            printf("    解出 %s -> %s (%zu B)\n", target.c_str(), jok ? "成功" : je.c_str(), body.size());
            Check(jok, "解出模组 jar 中的 json");
            if (jok && !body.empty()) printf("    开头: %.80s\n", body.c_str());
        }
        std::string mw = mod.TakeWarnings();
        printf("    警告: %s\n", mw.empty() ? "无" : mw.c_str());
        {
            const zip::Entry* e = mod.Find("META-INF/MANIFEST.MF");
            std::string ts = e ? e->TimeString() : std::string();
            printf("    MANIFEST.MF 的 DOS 时间 = \"%s\"\n", ts.c_str());
            Check(ts.size() == 19, "Entry::TimeString 对真实时间戳格式正确");
        }
    } else {
        printf("    打开失败: %s\n", merr.c_str());
    }

    // ---------------------------------------------------------- 7) 畸形输入
    printf("\n--- 7) 畸形输入（必须返回 false，不得崩溃）---\n");
    std::mt19937 rng(20260426u);
    std::vector<u8> junk(100);
    for (auto& b : junk) b = (u8)(rng() & 0xFF);

    png::Image bad;
    std::string perr;
    bool pr = png::Decode(junk.data(), junk.size(), bad, perr);
    printf("    png::Decode(随机 100B)   = %s  err=\"%s\"\n", pr ? "true" : "false", perr.c_str());
    Check(!pr, "随机数据 PNG 返回 false");

    zip::Archive zbad;
    std::string zerr;
    bool zr = zbad.OpenMemory(junk.data(), junk.size(), zerr);
    printf("    zip::OpenMemory(随机 100B) = %s  err=\"%s\"\n", zr ? "true" : "false", zerr.c_str());
    Check(!zr, "随机数据 ZIP 返回 false");

    struct Case { const char* name; std::vector<u8> d; };
    std::vector<Case> cases;
    cases.push_back({ "空输入", {} });
    {
        std::vector<u8> v = { 0x89,'P','N','G',0x0D,0x0A,0x1A,0x0A };
        cases.push_back({ "仅 PNG 签名", v });
    }
    {
        std::vector<u8> v = { 0x89,'P','N','G',0x0D,0x0A,0x1A,0x0A, 0xFF,0xFF,0xFF,0xFF,'I','H','D','R' };
        cases.push_back({ "chunk 长度越界", v });
    }
    {
        std::vector<u8> v(22, 0);
        v[0] = 'P'; v[1] = 'K'; v[2] = 0x05; v[3] = 0x06;
        v[10] = 1;   // 声称 1 个条目但没有中央目录
        cases.push_back({ "EOCD 声称有条目但无中央目录", v });
    }
    for (const auto& c : cases) {
        png::Image im; std::string e;
        bool r = png::Decode(c.d.data(), c.d.size(), im, e);
        Check(!r, c.name);
        zip::Archive za; std::string ze;
        bool rz = za.OpenMemory(c.d.data(), c.d.size(), ze);
        Check(!rz, c.name);
        printf("    [%s] png=%s zip=%s\n", c.name, r ? "true" : "false", rz ? "true" : "false");
    }

    // ---------------------------------------------------------- 8) stored 方法
    printf("\n--- 8) method 0 (stored) 真实用例 ---\n");
    {
        // 真实 jar 里 method 0 只出现在目录条目（'.../'），本实现对目录条目不建索引。
        // 因此这里先统计真实 jar 的 stored 分布，再用合成归档验证 stored 读取路径。
        const wchar_t* kJars[2] = {
            L"E:\\\u6e38\u620f\\@\\26.2F\\.minecraft\\mods\\iris-fabric-1.11.2+mc26.2.jar",
            L"E:\\\u6e38\u620f\\@\\26.2F\\.minecraft\\mods\\fabric-api-0.155.0+26.2.jar" };
        for (int j = 0; j < 2; ++j) {
            zip::Archive z;
            std::string ze;
            if (!z.Open(kJars[j], ze)) { printf("    打开失败: %s\n", ze.c_str()); continue; }
            std::vector<std::string> all;
            z.List(all);
            int storedN = 0;
            for (const auto& n : all) {
                const zip::Entry* e = z.Find(n);
                if (e && e->method == 0) ++storedN;
            }
            printf("    %s: 索引条目 %zu，其中 method=0 的 %d 个\n",
                   WideToUtf8(fsx::GetFileName(kJars[j])).c_str(), z.Count(), storedN);
            // 真 jar 里 method 0 均为目录条目（已被过滤），因此 0 是预期结果
        }
        {
            // 合成归档：stored + deflate 各一条，验证两条路径都正确
            auto le16 = [](std::vector<u8>& v, u16 x) { v.push_back((u8)x); v.push_back((u8)(x >> 8)); };
            auto le32 = [](std::vector<u8>& v, u32 x) { for (int i = 0; i < 4; ++i) v.push_back((u8)(x >> (8 * i))); };
            const std::string sName = "stored.txt", dName = "deflate.txt";
            const std::string sBody = "HELLO-STORED-BODY-0123456789", dBody = std::string(400, 'D') + "-DEFLATE";

            std::vector<u8> out;
            std::vector<u32> offsets, crcs, csize;
            std::vector<std::pair<std::string, std::string>> items = { {sName, sBody}, {dName, dBody} };
            std::vector<u16> meths = { 0, 8 };
            for (size_t k = 0; k < items.size(); ++k) {
                offsets.push_back((u32)out.size());
                const std::string& nm = items[k].first;
                const std::string& bd = items[k].second;
                std::vector<u8> comp((const u8*)bd.data(), (const u8*)bd.data() + bd.size());
                if (meths[k] == 8) {
                    // 注意：必须用 raw deflate（windowBits = -15）——ZIP 里没有 zlib 头
                    if (!DeflateRaw(bd, comp)) { printf("    !! 测试归档 deflate 失败\n"); }
                }
                u32 crc = (u32)crc32(crc32(0L, Z_NULL, 0), (const Bytef*)bd.data(), (uInt)bd.size());
                crcs.push_back(crc);
                csize.push_back((u32)comp.size());
                le32(out, 0x04034b50u); le16(out, 20); le16(out, 0x0800); le16(out, meths[k]);
                le16(out, 0); le16(out, 0);
                le32(out, crc); le32(out, (u32)comp.size()); le32(out, (u32)bd.size());
                le16(out, (u16)nm.size()); le16(out, 0);
                out.insert(out.end(), nm.begin(), nm.end());
                out.insert(out.end(), comp.begin(), comp.end());
            }
            u32 cdOff = (u32)out.size();
            for (size_t k = 0; k < items.size(); ++k) {
                const std::string& nm = items[k].first;
                le32(out, 0x02014b50u); le16(out, 20); le16(out, 20); le16(out, 0x0800); le16(out, meths[k]);
                le16(out, 0); le16(out, 0);
                le32(out, crcs[k]); le32(out, csize[k]); le32(out, (u32)items[k].second.size());
                le16(out, (u16)nm.size()); le16(out, 0); le16(out, 0);
                le16(out, 0); le16(out, 0); le32(out, 0);
                le32(out, offsets[k]);
                out.insert(out.end(), nm.begin(), nm.end());
            }
            u32 cdSize = (u32)out.size() - cdOff;
            le32(out, 0x06054b50u); le16(out, 0); le16(out, 0);
            le16(out, (u16)items.size()); le16(out, (u16)items.size());
            le32(out, cdSize); le32(out, cdOff); le16(out, 0);

            zip::Archive syn;
            std::string se;
            bool sok = syn.OpenMemory(out.data(), out.size(), se);
            Check(sok, "打开合成归档");
            if (sok) {
                std::string a, b;
                std::string e1, e2;
                bool ra = syn.ReadText(sName, a, e1);
                bool rb = syn.ReadText(dName, b, e2);
                Check(ra && a == sBody, "stored 条目内容正确");
                Check(rb && b == dBody, "deflate 条目内容正确");
                printf("    合成归档: stored=%s(%zuB) deflate=%s(%zuB)\n",
                       ra ? "OK" : e1.c_str(), a.size(), rb ? "OK" : e2.c_str(), b.size());
                Check(syn.TakeWarnings().empty(), "合成归档无 CRC 警告");
                // 落盘，便于外部工具（Python zipfile）复核这是标准 ZIP
                Check(fsx::WriteAllBytes(fsx::JoinPath(kOutDir, L"synthetic.zip"), out), "写出合成归档");
                printf("    合成归档已写出: %s (%zu B)\n",
                       WideToUtf8(fsx::JoinPath(kOutDir, L"synthetic.zip")).c_str(), out.size());
            }
        }
    }

    // ---------------------------------------------------------- 9) 全量 PNG 与 Pillow 对拍
    printf("\n--- 9) jar 内全部 PNG 与 Pillow 对拍 ---\n");
    {
        std::wstring refPath = fsx::JoinPath(kOutDir, L"pillow_all_png.txt");
        std::vector<u8> refBytes;
        if (!fsx::ReadAllBytes(refPath, refBytes)) {
            printf("    参考文件不存在，跳过（先生成 %s）\n", WideToUtf8(refPath).c_str());
        } else {
            std::string ref((const char*)refBytes.data(), refBytes.size());
            std::map<std::string, std::string> refMap;   // name -> "w\th\tmd5"（真实 TAB 分隔，行尾 CRLF）
            size_t p0 = 0;
            while (p0 <= ref.size()) {
                size_t nl = ref.find('\n', p0);
                std::string line = (nl == std::string::npos) ? ref.substr(p0) : ref.substr(p0, nl - p0);
                if (!line.empty() && line.back() == '\r') line.pop_back();
                if (!line.empty()) {
                    size_t t1 = line.find('\t');
                    if (t1 != std::string::npos) refMap[line.substr(0, t1)] = line.substr(t1 + 1);
                }
                if (nl == std::string::npos) break;
                p0 = nl + 1;
            }
            printf("    Pillow 参考条目数 = %zu\n", refMap.size());
            std::vector<std::string> all;
            jar.List(all);
            int tested = 0, matched = 0, failed = 0;
            std::string firstFail;
            for (const auto& n : all) {
                if (n.size() < 4 || n.compare(n.size() - 4, 4, ".png") != 0) continue;
                auto it = refMap.find(n);
                if (it == refMap.end()) continue;
                std::vector<u8> bytes;
                std::string e;
                ++tested;
                if (!jar.Read(n, bytes, e)) { ++failed; if (firstFail.empty()) firstFail = n + " 读取失败: " + e; continue; }
                png::Image im;
                std::string de;
                if (!png::Decode(bytes.data(), bytes.size(), im, de)) {
                    ++failed;
                    if (firstFail.empty()) firstFail = n + " 解码失败: " + de;
                    continue;
                }
                char wh[48];
                snprintf(wh, sizeof(wh), "%d\t%d", im.w, im.h);
                std::string md5 = Md5Of(RgbaToText(im));
                std::string mine = std::string(wh) + "\t" + md5;
                if (it->second == mine) ++matched;
                else {
                    ++failed;
                    if (firstFail.empty()) {
                        // 把制表符显示成可见形式，避免日志里看不出差异
                        auto vis = [](std::string s) {
                            std::string r;
                            for (char c : s) {
                                if (c == '\t') r += "<TAB>";
                                else if (c == '\n') r += "<LF>";
                                else if (c == '\r') r += "<CR>";
                                else r += c;
                            }
                            return r;
                        };
                        char dbg[512];
                        snprintf(dbg, sizeof(dbg), "%s\n      C++   =[%s] (len %zu)\n      Pillow=[%s] (len %zu)",
                                 n.c_str(), vis(mine).c_str(), mine.size(), vis(it->second).c_str(), it->second.size());
                        firstFail = dbg;
                    }
                }
            }
            printf("    对拍: 测试 %d 张，一致 %d 张，不一致 %d 张\n", tested, matched, failed);
            if (!firstFail.empty()) printf("    首个不一致: %s\n", firstFail.c_str());
            Check(tested > 3000, "对拍张数 > 3000");
            Check(failed == 0, "全部 PNG 与 Pillow 逐像素一致");
        }
    }

    // ---------------------------------------------------------- 10) ZIP64 与嵌套 jar
    printf("\n--- 10) ZIP64 容忍度 / 嵌套 jar ---\n");
    {
        // 构造一个只有 ZIP64 结构、中央目录缺失的归档：必须报清晰错误而不是崩溃
        auto le16 = [](std::vector<u8>& v, u16 x) { v.push_back((u8)x); v.push_back((u8)(x >> 8)); };
        auto le32 = [](std::vector<u8>& v, u32 x) { for (int i = 0; i < 4; ++i) v.push_back((u8)(x >> (8 * i))); };
        auto le64 = [](std::vector<u8>& v, u64 x) { for (int i = 0; i < 8; ++i) v.push_back((u8)(x >> (8 * i))); };

        std::vector<u8> z64;
        // ZIP64 EOCD 记录 (PK\x06\x06)
        le32(z64, 0x06064b50u); le64(z64, 44); le16(z64, 45); le16(z64, 45);
        le32(z64, 0); le32(z64, 0);
        le64(z64, 0); le64(z64, 0);          // 条目数 = 0
        le64(z64, 0); le64(z64, 0);          // 中央目录尺寸 0 / 偏移 0
        // ZIP64 EOCD 定位器 (PK\x06\x07)
        le32(z64, 0x07064b50u); le32(z64, 0); le64(z64, 0); le32(z64, 1);
        // 传统 EOCD，全部哨兵值
        le32(z64, 0x06054b50u); le16(z64, 0); le16(z64, 0);
        le16(z64, 0xFFFF); le16(z64, 0xFFFF);
        le32(z64, 0xFFFFFFFFu); le32(z64, 0xFFFFFFFFu); le16(z64, 0);
        zip::Archive za;
        std::string ze;
        bool zr = za.OpenMemory(z64.data(), z64.size(), ze);
        printf("    ZIP64 归档: open=%s err=\"%s\"\n", zr ? "true" : "false", ze.c_str());
        Check(!zr || za.Count() == 0, "ZIP64 空归档不崩溃");
        Check(zr || ze.find("ZIP64") != std::string::npos || ze.find("中央目录") != std::string::npos,
              "ZIP64 失败时有清晰错误信息");

        // Python 生成的 force_zip64 归档：local header 里尺寸为哨兵值，
        // 真实尺寸在 ZIP64 扩展字段 (0x0001) 中 —— 必须能正确解出。
        std::wstring zip64Path = fsx::JoinPath(kOutDir, L"zip64_forced.zip");
        std::vector<u8> z64raw;
        if (fsx::ReadAllBytes(zip64Path, z64raw)) {
            zip::Archive zz;
            std::string zze;
            bool zok = zz.OpenMemory(z64raw.data(), z64raw.size(), zze);
            Check(zok, "打开 force_zip64 归档");
            if (zok) {
                std::string body, be;
                bool ok1 = zz.ReadText("data/hello.json", body, be);
                bool ok2 = false;
                std::string b2, be2;
                ok2 = zz.ReadText("data/plain.txt", b2, be2);
                const zip::Entry* e = zz.Find("data/hello.json");
                printf("    force_zip64: 条目 %zu，hello.json=%s(%zu B, usize=%u) plain.txt=%s\n",
                       zz.Count(), ok1 ? "OK" : be.c_str(), body.size(),
                       e ? e->uncompressedSize : 0, ok2 ? "OK" : be2.c_str());
                Check(ok1 && body.size() == 260, "ZIP64 扩展字段里的尺寸被正确使用");
                Check(ok2 && b2 == "plain-text-body", "force_zip64 归档中普通条目正确");
            }
        } else {
            printf("    （未找到 %s，跳过 force_zip64 用例）\n", WideToUtf8(zip64Path).c_str());
        }

        // 嵌套 jar：从内存打开 fabric-api 里内嵌的某个 jar，并读它的 json
        zip::Archive outer;
        std::string oe;
        if (outer.Open(kModJar, oe)) {
            std::string innerName;
            for (const auto& nn : outer.Entries()) {
                if (nn.name.size() > 4 && nn.name.compare(nn.name.size() - 4, 4, ".jar") == 0 &&
                    nn.name.rfind("META-INF/jars/", 0) == 0) { innerName = nn.name; break; }
            }
            if (!innerName.empty()) {
                std::vector<u8> innerBytes;
                std::string ie;
                bool rok = outer.Read(innerName, innerBytes, ie);
                Check(rok, "从外层 jar 解出内嵌 jar");
                zip::Archive nested;
                std::string ne;
                bool nok = rok && nested.OpenMemory(innerBytes.data(), innerBytes.size(), ne);
                Check(nok, "OpenMemory 打开内嵌 jar");
                if (nok) {
                    printf("    内嵌 jar '%s': %zu 条目\n", innerName.c_str(), nested.Count());
                    std::string tgt;
                    for (const auto& nn : nested.Entries())
                        if (nn.name == "fabric.mod.json") { tgt = nn.name; break; }
                    if (tgt.empty())
                        for (const auto& nn : nested.Entries())
                            if (nn.name.size() > 5 && nn.name.compare(nn.name.size() - 5, 5, ".json") == 0) { tgt = nn.name; break; }
                    std::string body, be;
                    bool bok = !tgt.empty() && nested.ReadText(tgt, body, be);
                    Check(bok, "从内嵌 jar 解出 json");
                    printf("    内嵌 %s -> %s (%zu B)\n", tgt.c_str(), bok ? "成功" : be.c_str(), body.size());
                } else {
                    printf("    内嵌 jar 打开失败: %s\n", ne.c_str());
                }
            } else {
                printf("    未找到 META-INF/jars/*.jar\n");
            }
        }
    }

    // ---------------------------------------------------------- 11) PNG 健壮性细节
    printf("\n--- 11) PNG 健壮性细节 ---\n");
    {
        auto tryFile = [&](const wchar_t* fn, const char* label, bool expectOk) {
            std::wstring p = fsx::JoinPath(kOutDir, fn);
            std::vector<u8> d;
            if (!fsx::ReadAllBytes(p, d)) { printf("    [%s] 文件不存在，跳过\n", label); return; }
            png::Image im;
            std::string e;
            bool ok = png::Decode(d.data(), d.size(), im, e);
            printf("    [%s] %s  %dx%d  err=\"%s\"\n", label, ok ? "OK" : "false", im.w, im.h, e.c_str());
            Check(ok == expectOk, label);
            if (!ok && expectOk == false && e.empty()) Check(false, "失败时必须有错误信息");
        };
        // 交错 PNG：必须明确报错而不是解出错误像素
        tryFile(L"interlaced.png", "Adam7 交错被拒绝", false);
        // IDAT 的 CRC 被改坏：本实现跳过 chunk CRC 校验，数据本身没坏，应仍能解出
        tryFile(L"badcrc.png", "chunk CRC 损坏仍可解（数据未坏）", true);
        // 截断文件
        tryFile(L"truncated.png", "截断文件被拒绝", false);
    }

    printf("\n");
}
