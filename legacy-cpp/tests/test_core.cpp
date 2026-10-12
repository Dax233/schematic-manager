// 核心模块验证：NBT 无损往返 + litematic 解析/统计/编辑/写回 + ZIP/PNG 模块
#include "../src/nbt.h"
#include "../src/litematic.h"
#include "../src/fs.h"
#include "../src/str.h"
#include "test_common.h"
#include <cstdio>

static const wchar_t* kSchematics = L"E:\\\u6e38\u620f\\@\\26.2F\\.minecraft\\schematics";
static const wchar_t* kTmpOut = L"C:\\Users\\hsds\\Documents\\deepseek-harness\\default-workspace\\projmgr\\build\\_test_out";

static void TestNbtRoundTrip(const std::vector<FileInfo>& files) {
    printf("=== 1) NBT 无损往返 ===\n");
    for (const auto& fi : files) {
        std::vector<u8> raw;
        if (!fsx::ReadAllBytes(fi.path, raw)) { printf("  [读取失败] %ls\n", fi.name.c_str()); ++g_fail; continue; }
        nbt::Value root; std::string err;
        if (!nbt::ParseMaybeGzip(raw, root, err)) { printf("  [解析失败] %s\n", err.c_str()); ++g_fail; continue; }
        std::vector<u8> plain, reser;
        nbt::Gunzip(raw.data(), raw.size(), plain);
        nbt::Serialize(root, reser);
        bool byteExact = reser.size() == plain.size() &&
                         (reser.empty() || memcmp(reser.data(), plain.data(), reser.size()) == 0);
        printf("  %-6s 字节一致=%s  (%zu B)\n", byteExact ? "OK" : "FAIL", byteExact ? "是" : "否", reser.size());
        Check(byteExact, "NBT 字节级往返");
    }
    printf("\n");
}

static void TestAnalyze(const std::vector<FileInfo>& files) {
    printf("=== 2) litematic 解析与统计 ===\n");
    for (const auto& fi : files) {
        ltm::Summary s;
        if (!ltm::Schematic::Peek(fi.path, s)) { printf("  [Peek失败] %s\n", s.error.c_str()); ++g_fail; continue; }
        ltm::Schematic sc; std::string err;
        if (!sc.Load(fi.path, err)) { printf("  [Load失败] %s\n", err.c_str()); ++g_fail; continue; }
        ltm::Analysis a = sc.Analyze();

        printf("  %s\n", s.name.c_str());
        printf("    MC版本=%s(dataVersion=%d) 格式v%d.%d  作者=%s\n",
               s.mcVersionName.c_str(), s.dataVersion, s.formatVersion, s.formatSubVersion, s.author.c_str());
        printf("    尺寸=%dx%dx%d  体积=%lld  方块=%lld  区域=%d  非空气=%llu  种类=%d\n",
               s.enclosing[0], s.enclosing[1], s.enclosing[2],
               (long long)s.totalVolume, (long long)s.totalBlocks, s.regionCount,
               (unsigned long long)a.nonAirTotal, a.distinctBlockTypes);
        printf("    实体=%lld 方块实体=%lld  实体种类:", (long long)s.entityCount, (long long)s.tileEntityCount);
        for (const auto& kv : a.entitiesById) printf(" %s×%llu", kv.first.c_str(), (unsigned long long)kv.second);
        printf("\n");

        // 与元数据交叉校验
        Check((i64)a.nonAirTotal == s.totalBlocks, "非空气方块数 == Metadata.TotalBlocks");
        // 找出最多的方块
        std::vector<std::pair<std::string, u64>> top;
        for (const auto& kv : a.blocksByName)
            if (kv.first != "minecraft:air") top.emplace_back(kv.first, kv.second);
        std::sort(top.begin(), top.end(), [](auto& x, auto& y) { return x.second > y.second; });
        printf("    Top方块:");
        for (size_t i = 0; i < top.size() && i < 5; ++i)
            printf(" %s×%llu", top[i].first.c_str(), (unsigned long long)top[i].second);
        printf("\n\n");
    }
}

static void TestEdit(const std::vector<FileInfo>& files) {
    printf("=== 3) 编辑与写回验证 ===\n");
    // 找 white_concrete 数量最多的那个文件
    const FileInfo* target = nullptr;
    ltm::Analysis ta;
    for (const auto& fi : files) {
        ltm::Schematic sc; std::string err;
        if (!sc.Load(fi.path, err)) continue;
        ltm::Analysis a = sc.Analyze();
        auto it = a.blocksByName.find("minecraft:white_concrete");
        if (it != a.blocksByName.end() && it->second > 10) { target = &fi; ta = a; break; }
    }
    if (!target) { printf("  未找到含 white_concrete 的样本\n\n"); return; }

    ltm::Schematic sc; std::string err;
    Check(sc.Load(target->path, err), "加载样本");
    ltm::Analysis before = sc.Analyze();
    u64 beforeWhite = before.blocksByName.count("minecraft:white_concrete") ? before.blocksByName.at("minecraft:white_concrete") : 0;
    u64 beforeBlue  = before.blocksByName.count("minecraft:blue_concrete") ? before.blocksByName.at("minecraft:blue_concrete") : 0;
    i32 beforeDv = sc.GetSummary().dataVersion;
    printf("  样本: %s\n", target->name.empty() ? "?" : WideToUtf8(target->name).c_str());
    printf("  编辑前: white_concrete=%llu blue_concrete=%llu dataVersion=%d\n",
           (unsigned long long)beforeWhite, (unsigned long long)beforeBlue, beforeDv);

    // 1) 替换方块（整类替换）
    ltm::ReplaceRule rule;
    rule.fromName = "minecraft:white_concrete";
    rule.toName   = "minecraft:blue_concrete";
    u64 replaced = sc.ReplaceBlocks(rule, nullptr, 0);
    // 2) 改版本号
    sc.SetDataVersion(4903);
    // 3) 改作者
    sc.SetMetaAuthor("DSH-Manager-Test");

    printf("  替换了 %llu 个方块；已修改标记=%s\n", (unsigned long long)replaced, sc.IsModified() ? "是" : "否");
    Check(replaced == beforeWhite, "替换数量 == 原 white_concrete 数量");
    Check(sc.IsModified(), "修改标记已置位");

    fsx::EnsureDir(kTmpOut);
    std::wstring outPath = fsx::JoinPath(kTmpOut, L"edited.litematic");
    Check(sc.SaveTo(outPath, err), "保存文件");

    // 重新加载验证
    ltm::Schematic sc2;
    Check(sc2.Load(outPath, err), "重新加载");
    ltm::Analysis after = sc2.Analyze();
    u64 afterWhite = after.blocksByName.count("minecraft:white_concrete") ? after.blocksByName.at("minecraft:white_concrete") : 0;
    u64 afterBlue  = after.blocksByName.count("minecraft:blue_concrete") ? after.blocksByName.at("minecraft:blue_concrete") : 0;
    ltm::Summary s2 = sc2.GetSummary();
    printf("  编辑后: white_concrete=%llu blue_concrete=%llu dataVersion=%d 作者=%s\n",
           (unsigned long long)afterWhite, (unsigned long long)afterBlue, s2.dataVersion, s2.author.c_str());
    Check(afterWhite == 0 && afterBlue == beforeBlue + beforeWhite, "替换结果正确");
    Check(s2.dataVersion == 4903, "版本号已修改");
    Check(s2.author == "DSH-Manager-Test", "作者已修改");
    Check(after.nonAirTotal == before.nonAirTotal, "方块总数未变");

    // 体积检查
    ltm::Summary s1 = sc.GetSummary();
    printf("  文件大小: 原始 %llu B -> 编辑后 %llu B\n",
           (unsigned long long)s1.fileSize, (unsigned long long)s2.fileSize);
    printf("\n");
}

static void TestPartialReplace(const std::vector<FileInfo>& files) {
    printf("=== 4) 逐实例替换（属性限定） ===\n");
    for (const auto& fi : files) {
        ltm::Schematic sc; std::string err;
        if (!sc.Load(fi.path, err)) continue;
        ltm::Analysis a = sc.Analyze();
        // 找一个含 powered_rail 的文件
        bool has = false;
        for (const auto& kv : a.blocksByState)
            if (kv.first.rfind("minecraft:powered_rail[", 0) == 0 || kv.first == "minecraft:powered_rail") { has = true; break; }
        if (!has) continue;

        ltm::ReplaceRule rule;
        rule.fromName = "minecraft:powered_rail";
        rule.fromProps.push_back({"powered", "false"});   // 只替换未激活的
        rule.toName = "minecraft:gold_block";
        rule.inheritProps = false;
        u64 n = sc.ReplaceBlocks(rule, nullptr, 0);
        printf("  %s : powered_rail[powered=false] -> gold_block 共 %llu 个\n",
               WideToUtf8(fi.name).c_str(), (unsigned long long)n);
        if (n) {
            std::vector<u8> bytes; std::string e2;
            Check(sc.ToBytes(bytes, e2) && !bytes.empty(), "序列化成功");
            printf("    序列化后 %zu B\n", bytes.size());
        }
        break;
    }
    printf("\n");
}

int main() {
    printf("=== 投影文件管理器 核心模块测试 ===\n\n");
    std::vector<FileInfo> files;
    fsx::ListFilesRecursive(kSchematics, L".litematic", files);
    std::sort(files.begin(), files.end(), [](const FileInfo& a, const FileInfo& b) { return a.name < b.name; });
    printf("找到 %zu 个投影文件\n\n", files.size());
    if (files.empty()) { printf("没有测试样本\n"); return 1; }

    TestNbtRoundTrip(files);
    TestAnalyze(files);
    TestEdit(files);
    TestPartialReplace(files);
    TestZipPng();

    printf("=== 结果: %d 通过 / %d 失败 ===\n", g_pass, g_fail);
    return g_fail == 0 ? 0 : 1;
}
