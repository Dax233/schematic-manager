// 无界面自检模式：验证内置方块库与渲染管线（--selftest）
#include "common.h"
#include "fs.h"
#include "str.h"
#include "litematic.h"
#include "blockdb.h"
#include "render.h"
#include "str.h"
#include <cstdio>

// GUI 子系统程序没有控制台，这里把 stdout 重定向到一个日志文件，保证自检结果一定可见。
static std::wstring g_logPath;

static void AttachOut() {
    std::wstring exeDir = fsx::GetDirectoryName(fsx::GetExePath());
    std::wstring outDir = fsx::JoinPath(exeDir, L"_selftest_out");
    fsx::EnsureDir(outDir);
    g_logPath = fsx::JoinPath(outDir, L"selftest.log");
    std::string u8 = WideToUtf8(g_logPath);
    FILE* f = nullptr;
    freopen_s(&f, u8.c_str(), "w", stdout);
    freopen_s(&f, u8.c_str(), "w", stderr);
    setvbuf(stdout, nullptr, _IONBF, 0);
}

namespace {

struct Rec { std::string file; bool ok = false; int blocks = 0; int types = 0; double ms = 0; std::string err; };

int TestOne(const std::wstring& path, const std::wstring& outDir, mc::BlockDB& db, int versionIdx,
            std::vector<Rec>& recs) {
    ltm::Schematic sc;
    std::string err;
    if (!sc.Load(path, err)) {
        Rec r; r.file = WideToUtf8(fsx::GetFileName(path)); r.err = err;
        recs.push_back(r);
        return 1;
    }
    ltm::Analysis a = sc.Analyze();
    int vi = (versionIdx >= 0) ? versionIdx : db.PickVersion(sc.GetSummary().dataVersion);

    int fail = 0;
    const int views[4] = { render::VIEW_FRONT, render::VIEW_LEFT45, render::VIEW_RIGHT45, render::VIEW_BACK };
    const wchar_t* vn[4] = { L"front", L"left45", L"right45", L"back" };
    for (int i = 0; i < 4; ++i) {
        render::Options opt;
        opt.size = 200;
        opt.view = views[i];
        opt.supersample = 2;
        render::Image img;
        std::string e2;
        ULONGLONG t0 = GetTickCount64();
        bool ok = render::RenderSchematic(sc, db, vi, opt, img, e2);
        double ms = (double)(GetTickCount64() - t0);

        // 统计非背景像素，确认真的画出了东西
        int painted = 0;
        for (u32 c : img.px) if (((c >> 24) & 0xFF) > 0 && (c & 0x00FFFFFF) != 0x00FFFFFF) ++painted;

        Rec r;
        r.file = WideToUtf8(fsx::GetFileName(path)) + " [" + WideToUtf8(vn[i]) + "]";
        r.ok = ok && painted > 200;
        r.blocks = (int)a.nonAirTotal;
        r.types = a.distinctBlockTypes;
        r.ms = ms;
        if (!ok) r.err = e2;
        else if (painted <= 200) r.err = "渲染结果几乎为空 (painted=" + std::to_string(painted) + ")";
        recs.push_back(r);

        if (ok && !outDir.empty()) {
            std::wstring fn = fsx::GetFileStem(path) + L"_" + vn[i] + L".png";
            render::SavePng(fsx::JoinPath(outDir, fn), img);
        }
        if (!r.ok) ++fail;
    }
    return fail;
}

} // namespace

int RunSelfTest() {
    AttachOut();
    printf("\n===== 投影文件管理器 自检 =====\n");

    // 1) 内置方块库
    mc::BlockDB db;
    std::string err;
    bool ok = db.LoadEmbedded(101, err);
    if (!ok) {
        std::wstring p = fsx::JoinPath(fsx::GetDirectoryName(fsx::GetExePath()), L"blocks.dat");
        if (fsx::FileExists(p) && db.LoadFromFile(p, err)) ok = true;
    }
    printf("[方块库] %s\n", ok ? "加载成功" : "加载失败");
    if (!ok) { printf("  错误: %s\n", err.c_str()); return 2; }

    size_t totalBlocks = 0, totalTex = 0;
    for (const auto& v : db.Versions()) totalBlocks += v.blocks.size();
    printf("  版本数: %zu\n", db.VersionCount());
    printf("  方块定义条目: %zu\n", totalBlocks);
    printf("  方块名总数(并集): %zu\n", db.AllBlockNames().size());
    printf("  可用版本: ");
    for (size_t i = 0; i < db.VersionCount(); ++i) {
        if (i) printf(", ");
        printf("%s", db.Version(i).id.c_str());
    }
    printf("\n");

    // 抽样检查几个方块能否解析出模型
    const char* probe[] = { "minecraft:stone", "minecraft:oak_stairs", "minecraft:hopper",
                            "minecraft:glass", "minecraft:chest", "minecraft:white_concrete" };
    printf("\n[模型解析抽样]\n");
    for (const char* blk : probe) {
        int vi = db.PickVersion(4903);
        const mc::VersionDef& v = db.Version((size_t)vi);
        ltm::BlockState bs; bs.name = blk;
        std::vector<mc::BlockDB::ResolvedModel> res;
        db.ResolveModels(v, bs, res);
        size_t elems = 0, faces = 0;
        for (auto& r : res) if (r.model) { elems += r.model->elements.size(); for (auto& e : r.model->elements) faces += e.faces.size(); }
        std::string nm = db.DisplayName(blk, true, &v);
        std::string en = db.DisplayName(blk, false, &v);
        printf("  %-28s 模型%zu 元素%zu 面%zu  中文='%s' 英文='%s'\n",
               blk, res.size(), elems, faces, nm.c_str(), en.c_str());
    }

    // 2) 渲染自检
    std::wstring exeDir = fsx::GetDirectoryName(fsx::GetExePath());
    std::wstring schDir = fsx::JoinPath(exeDir, L"schematics");
    if (!fsx::DirExists(schDir)) {
        std::wstring alt = L"E:\\\u6e38\u620f\\@\\26.2F\\.minecraft\\schematics";
        if (fsx::DirExists(alt)) schDir = alt;
    }
    std::wstring outDir = fsx::JoinPath(exeDir, L"_selftest_out");
    fsx::EnsureDir(outDir);

    std::vector<FileInfo> files;
    fsx::ListFilesRecursive(schDir, L".litematic", files);
    std::sort(files.begin(), files.end(), [](const FileInfo& a, const FileInfo& b) { return a.name < b.name; });
    printf("\n[渲染自检] 目录: %s  文件数: %zu\n", WideToUtf8(schDir).c_str(), files.size());

    std::vector<Rec> recs;
    for (const FileInfo& fi : files) TestOne(fi.path, outDir, db, -1, recs);

    int pass = 0, fail = 0;
    for (const Rec& r : recs) {
        printf("  [%s] %-46s 方块=%-6d 种类=%-4d %6.0f ms %s\n",
               r.ok ? "OK  " : "FAIL", r.file.c_str(), r.blocks, r.types, r.ms,
               r.err.empty() ? "" : r.err.c_str());
        r.ok ? ++pass : ++fail;
    }
    printf("\n[结果] %d 通过 / %d 失败   图片输出: %s\n",
           pass, fail, WideToUtf8(outDir).c_str());
    printf("===== 自检结束 =====\n\n");
    return fail == 0 ? 0 : 1;
}


// ---------------------------------------------------------------- 资源包转换测试
#include "packbuild.h"
#include "packmgr.h"
#include <shlobj.h>

static void AttachOut2() {
    std::wstring exeDir = fsx::GetDirectoryName(fsx::GetExePath());
    std::wstring outDir = fsx::JoinPath(exeDir, L"_selftest_out");
    fsx::EnsureDir(outDir);
    std::string u8 = WideToUtf8(fsx::JoinPath(outDir, L"packtest.log"));
    FILE* f = nullptr;
    freopen_s(&f, u8.c_str(), "w", stdout);
    freopen_s(&f, u8.c_str(), "w", stderr);
    setvbuf(stdout, nullptr, _IONBF, 0);
}

int RunPackTest(const wchar_t* jarArg) {
    AttachOut2();
    printf("===== 资源包转换测试 =====\n");

    std::wstring jar = jarArg && *jarArg ? std::wstring(jarArg)
        : std::wstring(L"E:\\\u6e38\u620f\\@\\26.2F\\.minecraft\\versions\\26.2-Fabric\\26.2-Fabric.jar");
    printf("jar: %s\n", WideToUtf8(jar).c_str());

    packbuild::Result r;
    ULONGLONG t0 = GetTickCount64();
    bool ok = packbuild::BuildFromJar(jar, "test", 1, 0, r);
    double ms = (double)(GetTickCount64() - t0);
    printf("转换结果: %s  (%.0f ms)\n", ok ? "成功" : "失败", ms);
    if (!ok) { printf("错误: %s\n", r.error.c_str()); return 1; }
    printf("  版本 id=%s dataVersion=%d 命名空间=%zu\n", r.versionId.c_str(), r.dataVersion, r.namespaces);
    printf("  方块=%zu 模型=%zu 贴图=%zu 变体失败=%zu\n",
           r.blockCount, r.modelCount, r.textureCount, r.failedModels);
    printf("  包大小=%.2f MB\n", r.data.size() / 1048576.0);
    if (!r.warning.empty()) printf("  警告: %s\n", r.warning.substr(0, 300).c_str());

    // 独立加载这个包
    mc::BlockDB db;
    std::string err;
    if (!db.LoadFromMemory(r.data.data(), r.data.size(), err)) {
        printf("  !! 独立加载失败: %s\n", err.c_str());
        return 2;
    }
    printf("  独立加载成功: %zu 版本, %zu 方块名\n", db.VersionCount(), db.AllBlockNames().size());

    // 抽样检查
    const char* probe[] = { "minecraft:stone", "minecraft:oak_stairs", "minecraft:hopper", "minecraft:glass" };
    for (const char* b : probe) {
        ltm::BlockState bs; bs.name = b;
        std::vector<std::pair<std::string,std::string>> defs;
        db.DefaultProps(b, defs);
        for (auto& kv : defs) bs.SetProp(kv.first, kv.second);
        std::vector<mc::BlockDB::ResolvedModel> res;
        db.ResolveModels(db.Version(0), bs, res);
        size_t elems = 0, faces = 0;
        for (auto& rm : res) if (rm.model) { elems += rm.model->elements.size(); for (auto& e : rm.model->elements) faces += e.faces.size(); }
        printf("    %-24s 模型=%zu 元素=%zu 面=%zu 中文='%s'\n",
               b, res.size(), elems, faces, db.DisplayName(b, true, &db.Version(0)).c_str());
    }

    // 渲染一个方块图标，确认贴图真的解出来了
    {
        ltm::BlockState bs; bs.name = "minecraft:stone";
        std::vector<std::pair<std::string,std::string>> defs;
        db.DefaultProps("minecraft:stone", defs);
        for (auto& kv : defs) bs.SetProp(kv.first, kv.second);
        render::Image img;
        if (render::RenderBlockIcon(db, 0, bs, 64, img, true)) {
            std::wstring outDir = fsx::JoinPath(fsx::GetDirectoryName(fsx::GetExePath()), L"_selftest_out");
            fsx::EnsureDir(outDir);
            std::wstring f = fsx::JoinPath(outDir, L"packtest_stone.png");
            render::SavePng(f, img);
            int painted = 0;
            for (u32 px : img.px) if (((px >> 24) & 0xFF) > 128) ++painted;
            printf("  石头图标渲染: 非透明像素=%d  已存 %s\n", painted, WideToUtf8(f).c_str());
        } else {
            printf("  !! 石头图标渲染失败\n");
        }
    }

    // 把包合并进内置库，验证叠加机制
    {
        mc::BlockDB base;
        std::string e2;
        std::wstring exeDir = fsx::GetDirectoryName(fsx::GetExePath());
        bool baseOk = base.LoadEmbedded(101, e2);
        if (!baseOk) base.LoadFromFile(fsx::JoinPath(exeDir, L"blocks.dat"), e2);
        printf("\n基库: %s  版本=%zu\n", baseOk ? "OK" : "FAIL", base.VersionCount());
        mc::PackInfo pi;
        pi.name = "test-mc";
        pi.kind = 1;
        std::string e3;
        bool mok = base.LoadPackBytes(r.data.data(), r.data.size(), pi, e3);
        printf("合并版本包: %s  合并后版本=%zu  (错误: %s)\n",
               mok ? "成功" : "失败", base.VersionCount(), e3.c_str());
        if (mok) {
            int vi = base.PickVersion(4903);
            printf("  dataVersion 4903 -> 版本下标 %d (%s)\n", vi,
                   vi >= 0 ? base.Version((size_t)vi).id.c_str() : "?");
        }
    }
    // 扫描全部模组 jar，找出真正带方块的
    {
        std::wstring modDir = L"E:\\\u6e38\u620f\\@\\26.2F\\.minecraft\\mods";
        std::vector<FileInfo> mods;
        fsx::ListDir(modDir, mods, false);
        int withBlocks = 0, scanned = 0;
        printf("\n模组 jar 扫描 (共 %zu 个):\n", mods.size());
        for (const FileInfo& m : mods) {
            if (!EndsWithNoCase(m.name, L".jar")) continue;
            ++scanned;
            packbuild::Result mr;
            bool mok = packbuild::BuildFromJar(m.path, "modtest", 2, 4903, mr);
            if (!mok) {
                if (scanned <= 40) printf("  %-52s 无方块\n", WideToUtf8(m.name).c_str());
                continue;
            }
            ++withBlocks;
            printf("  >>> %-48s 方块=%-5zu 模型=%-5zu 贴图=%-5zu\n",
                   WideToUtf8(m.name).c_str(), mr.blockCount, mr.modelCount, mr.textureCount);
            if (withBlocks >= 3) break;
        }
        printf("扫描 %d 个 jar，其中 %d 个含方块\n", scanned, withBlocks);
    }

    printf("===== 结束 =====\n");
    return 0;
}


// 亮度对比测试：同一投影渲染多档亮度，输出 PNG 供挑选
namespace app {
void RunBrightTest(const std::wstring& path) {
    fsx::DiagLog("=== 开始透明背景测试 ===");
    mc::BlockDB db;
    std::string err;
    if (!db.LoadEmbedded(101, err)) return;
    ltm::Schematic sc;
    if (!sc.Load(path, err)) return;
    int vi = db.FindBaseVersion(sc.GetSummary().dataVersion);
    if (vi < 0) vi = 0;
    db.SetActiveVersion(vi);

    std::wstring dir = fsx::GetDirectoryName(path);
    const float levels[] = { 1.00f, 1.10f, 1.20f, 1.30f, 1.45f, 1.60f };
    for (float b : levels) {
        render::SetBrightness(b);
        render::Options opt;
        opt.size = 460;
        opt.view = render::VIEW_LEFT45;
        opt.transparentBg = true;
        opt.bg = 0x00000000u;
        opt.supersample = 2;
        render::Image img;
        std::string e2;
        ULONGLONG t0 = GetTickCount64();
        bool okr = render::RenderSchematic(sc, db, vi, opt, img, e2);
        fsx::DiagLog("render size=" + std::to_string(opt.size) +
                     " ss=" + std::to_string(opt.supersample) +
                     " transparent=1 ok=" + std::to_string((int)okr) +
                     " out=" + std::to_string(img.w) + "x" + std::to_string(img.h) +
                     " ms=" + std::to_string((int)(GetTickCount64() - t0)) +
                     (e2.empty() ? "" : (" err=" + e2)));
        if (!okr) continue;
        wchar_t nm[64];
        swprintf(nm, 64, L"bright_%03d.png", (int)(b * 100.0f + 0.5f));
        bool sok = render::SavePng(fsx::JoinPath(dir, nm), img);
        fsx::DiagLog(std::string("save ") + WideToUtf8(fsx::JoinPath(dir, nm)) +
                     (sok ? " ok" : " FAIL"));
    }
    fsx::DiagLog("=== 测试结束 ===");
    render::SetBrightness(1.0f);
}
} // namespace app
