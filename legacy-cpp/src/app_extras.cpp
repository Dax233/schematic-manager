// 新增功能的实现（多目录、搜索、格式化、实体名、内联编辑提交）
#include "app_internal.h"
#include "packmgr.h"
#include "i18n.h"
#include "render.h"
#include "str.h"
#include "fs.h"
#include <algorithm>
#include <shellapi.h>
#include <shlobj.h>

namespace app {

// ---------------------------------------------------------------- 格式化
std::wstring FormatStackCount(u64 n) {
    if (n == 0) return L"0";
    u64 stacks = n / 64, rem = n % 64;
    if (stacks == 0) return std::to_wstring(rem);
    if (rem == 0) return FmtW(L"64\u00D7%llu", (unsigned long long)stacks);
    return FmtW(L"64\u00D7%llu+%llu", (unsigned long long)stacks, (unsigned long long)rem);
}

std::wstring FormatVersionHint(i32 fv, i32 sub) {
    (void)sub;
    if (fv >= 7) return L"MC 1.16 \u53CA\u4EE5\u540E\uFF08Litematica 0.2x \u7CFB\u5217\uFF09";
    if (fv == 6) return L"MC 1.14 ~ 1.15 \u65F6\u671F";
    if (fv == 5) return L"MC 1.13.2 \u65F6\u671F";
    if (fv > 0)  return L"MC 1.12 \u53CA\u66F4\u65E9";
    return L"\u672A\u77E5";
}

// ---------------------------------------------------------------- 实体中文名
struct EntName { const char* id; const wchar_t* zh; };

static const EntName kEnts[] = {
    { "minecraft:item_frame",        L"\u7269\u54C1\u5C55\u793A\u6846" },
    { "minecraft:glow_item_frame",   L"\u53D1\u5149\u7269\u54C1\u5C55\u793A\u6846" },
    { "minecraft:painting",          L"\u753B" },
    { "minecraft:armor_stand",       L"\u76D4\u7532\u67B6" },
    { "minecraft:minecart",          L"\u77FF\u8F66" },
    { "minecraft:chest_minecart",    L"\u7BB1\u5B50\u77FF\u8F66" },
    { "minecraft:hopper_minecart",   L"\u6F0F\u6597\u77FF\u8F66" },
    { "minecraft:tnt_minecart",      L"TNT \u77FF\u8F66" },
    { "minecraft:furnace_minecart",  L"\u7194\u7089\u77FF\u8F66" },
    { "minecraft:command_block_minecart", L"\u547D\u4EE4\u65B9\u5757\u77FF\u8F66" },
    { "minecraft:spawner_minecart",  L"\u5237\u602A\u7B3C\u77FF\u8F66" },
    { "minecraft:boat",              L"\u8239" },
    { "minecraft:oak_boat",          L"\u6A61\u6728\u8239" },
    { "minecraft:chest_boat",        L"\u7BB1\u8239" },
    { "minecraft:sheep",             L"\u7F8A" },
    { "minecraft:cow",               L"\u725B" },
    { "minecraft:mooshroom",         L"\u83CC\u4E1D\u725B" },
    { "minecraft:pig",               L"\u732A" },
    { "minecraft:chicken",           L"\u9E21" },
    { "minecraft:rabbit",            L"\u5154\u5B50" },
    { "minecraft:horse",             L"\u9A6C" },
    { "minecraft:donkey",            L"\u9A74" },
    { "minecraft:mule",              L"\u9AA1" },
    { "minecraft:llama",             L"\u7F8A\u9A7C" },
    { "minecraft:wolf",              L"\u72FC" },
    { "minecraft:cat",               L"\u732B" },
    { "minecraft:ocelot",            L"\u8C79\u732B" },
    { "minecraft:parrot",            L"\u9E66\u9E49" },
    { "minecraft:fox",               L"\u72D0\u72F8" },
    { "minecraft:panda",             L"\u718A\u732B" },
    { "minecraft:polar_bear",        L"\u5317\u6781\u718A" },
    { "minecraft:bee",               L"\u871C\u8702" },
    { "minecraft:turtle",            L"\u6D77\u9F9F" },
    { "minecraft:dolphin",           L"\u6D77\u8C5A" },
    { "minecraft:squid",             L"\u9C81\u9C7C" },
    { "minecraft:glow_squid",        L"\u53D1\u5149\u9C81\u9C7C" },
    { "minecraft:bat",               L"\u8759\u8760" },
    { "minecraft:villager",          L"\u6751\u6C11" },
    { "minecraft:wandering_trader",  L"\u6D41\u6D6A\u5546\u4EBA" },
    { "minecraft:iron_golem",        L"\u94C1\u5080\u5121" },
    { "minecraft:snow_golem",        L"\u96EA\u5080\u5121" },
    { "minecraft:zombie",            L"\u50F5\u5C38" },
    { "minecraft:husk",              L"\u5C38\u58F3" },
    { "minecraft:drowned",           L"\u6EBA\u5C38" },
    { "minecraft:skeleton",          L"\u9AB7\u9AC5" },
    { "minecraft:stray",             L"\u6D41\u6D6A\u8005" },
    { "minecraft:wither_skeleton",   L"\u51CB\u7075\u9AB7\u9AC5" },
    { "minecraft:creeper",           L"\u82E6\u529B\u6015" },
    { "minecraft:spider",            L"\u8718\u86DB" },
    { "minecraft:cave_spider",       L"\u6D1E\u7A74\u8718\u86DB" },
    { "minecraft:enderman",          L"\u672B\u5F71\u4EBA" },
    { "minecraft:endermite",         L"\u672B\u5F71\u87F2" },
    { "minecraft:silverfish",        L"\u8821\u866B" },
    { "minecraft:slime",             L"\u53F2\u83B1\u59C6" },
    { "minecraft:magma_cube",        L"\u9B54\u529B\u53F2\u83B1\u59C6" },
    { "minecraft:blaze",             L"\u70C8\u7130\u4EBA" },
    { "minecraft:ghast",             L"\u6076\u9B42" },
    { "minecraft:zombified_piglin",  L"\u50F5\u5C38\u732A\u7075" },
    { "minecraft:piglin",            L"\u732A\u7075" },
    { "minecraft:piglin_brute",      L"\u732A\u7075\u86EE\u5175" },
    { "minecraft:hoglin",            L"\u75B4\u732A\u517D" },
    { "minecraft:zoglin",            L"\u50F5\u5C38\u75B4\u732A\u517D" },
    { "minecraft:wither",            L"\u51CB\u7075" },
    { "minecraft:ender_dragon",      L"\u672B\u5F71\u9F99" },
    { "minecraft:witch",             L"\u5973\u5DEB" },
    { "minecraft:evoker",            L"\u5524\u9B54\u8005" },
    { "minecraft:vindicator",        L"\u536B\u9053\u58EB" },
    { "minecraft:pillager",          L"\u63A0\u593A\u8005" },
    { "minecraft:ravager",           L"\u52AB\u63A0\u517D" },
    { "minecraft:vex",               L"\u607C\u9B3C" },
    { "minecraft:phantom",           L"\u5E7B\u7FFC" },
    { "minecraft:shulker",           L"\u6F5C\u5F71\u8D1D" },
    { "minecraft:guardian",          L"\u5B88\u536B\u8005" },
    { "minecraft:elder_guardian",    L"\u8FDC\u53E4\u5B88\u536B\u8005" },
    { "minecraft:allay",             L"\u60A6\u7075" },
    { "minecraft:frog",              L"\u9752\u86D9" },
    { "minecraft:tadpole",           L"\u8759\u868C" },
    { "minecraft:axolotl",           L"\u7F8E\u897F\u87F2" },
    { "minecraft:goat",              L"\u5C71\u7F8A" },
    { "minecraft:strider",           L"\u70BD\u8DB3\u517D" },
    { "minecraft:camel",             L"\u9A86\u9A7C" },
    { "minecraft:sniffer",           L"\u95FB\u55C5\u517D" },
    { "minecraft:breeze",            L"\u98CE\u88AD" },
    { "minecraft:armadillo",         L"\u72B7\u72F4" },
    { "minecraft:creaking",          L"\u560E\u679D\u602A" },
    { "minecraft:warden",            L"\u5B88\u671B\u8005" },
    { "minecraft:iron_golem",        L"\u94C1\u5080\u5121" },
    { "minecraft:falling_block",     L"\u4E0B\u843D\u4E2D\u7684\u65B9\u5757" },
    { "minecraft:tnt",               L"TNT" },
    { "minecraft:experience_orb",    L"\u7ECF\u9A8C\u7403" },
    { "minecraft:item",              L"\u7269\u54C1" },
    { "minecraft:arrow",             L"\u7BAD" },
    { "minecraft:firework_rocket",   L"\u70DF\u82B1\u706B\u7BAD" },
    { "minecraft:leash_knot",        L"\u62F4\u7EF3\u7ED3" },
    { "minecraft:end_crystal",       L"\u672B\u5F71\u6C34\u6676" },
};

// 去掉命名空间，只留 path 部分
static std::string EntPath(const std::string& id) {
    size_t c = id.find(':');
    std::string p = (c == std::string::npos) ? id : id.substr(c + 1);
    // 模组实体可能是 ns:path，统一只比较 path
    return p;
}

std::wstring EntityDisplay(const std::string& id, bool chinese) {
    std::string path = EntPath(id);
    if (!chinese) return Utf8ToWide(path);
    // 先按完整 id 匹配，再按去命名空间的 path 匹配（兼容模组与无命名空间写法）
    for (const EntName& e : kEnts)
        if (id == e.id) return e.zh;
    for (const EntName& e : kEnts)
        if (path == EntPath(e.id)) return e.zh;

    // 未收录：把下划线换成空格，至少可读
    std::wstring w = Utf8ToWide(path);
    for (wchar_t& ch : w) if (ch == L'_') ch = L' ';
    return w;
}

// ---------------------------------------------------------------- 多目录
std::vector<std::wstring> EnabledDirs() {
    std::vector<std::wstring> v;
    for (const auto& d : Get().dirs) if (d.enabled) v.push_back(d.path);
    if (v.empty() && !Get().scanDir.empty()) v.push_back(Get().scanDir);
    return v;
}

void AddDir(const std::wstring& dir, bool enabled) {
    State& st = Get();
    if (dir.empty()) return;
    PushRecentDir(dir);          // 任何路径进入目录都记入历史
    for (auto& d : st.dirs) if (EqualsNoCase(d.path, dir)) { d.enabled = enabled; return; }
    st.dirs.push_back({ dir, enabled });
    packmgr::SaveDirs(st.dirs);
    RescanAsync();
}

void RemoveDir(const std::wstring& dir) {
    State& st = Get();
    st.dirs.erase(std::remove_if(st.dirs.begin(), st.dirs.end(),
                  [&](const packmgr::DirEntry& d) { return EqualsNoCase(d.path, dir); }), st.dirs.end());
    packmgr::SaveDirs(st.dirs);
    if (EqualsNoCase(st.scanDir, dir)) {
        st.scanDir = st.dirs.empty() ? std::wstring() : st.dirs[0].path;
    }
    RescanAsync();
}

void PushRecentDir(const std::wstring& dir);

void PushRecentDir(const std::wstring& dir) {
    if (dir.empty()) return;
    State& st = Get();
    st.recentDirs.erase(std::remove_if(st.recentDirs.begin(), st.recentDirs.end(),
                        [&](const std::wstring& d) { return EqualsNoCase(d, dir); }), st.recentDirs.end());
    st.recentDirs.insert(st.recentDirs.begin(), dir);
    if (st.recentDirs.size() > 12) st.recentDirs.resize(12);
    packmgr::SaveRecent(st.recentDirs);
}

void LoadDirsFromSettings() {
    State& st = Get();
    auto list = packmgr::LoadDirs();
    st.dirs.clear();
    for (const auto& e : list) st.dirs.push_back(e);
    st.recentDirs = packmgr::LoadRecent();
}

// ---------------------------------------------------------------- 搜索
static bool FuzzyHit(const std::wstring& hay, const std::wstring& needle) {
    if (needle.empty()) return false;
    std::wstring h = ToLowerW(hay), n = ToLowerW(needle);
    if (h.find(n) != std::wstring::npos) return true;
    size_t j = 0;
    for (size_t i = 0; i < h.size() && j < n.size(); ++i)
        if (h[i] == n[j]) ++j;
    return j == n.size();
}

void ApplySort() {
    State& st = Get();
    if (st.sortCol < 0 || st.files.size() < 2) return;
    std::wstring keep;
    if (st.listSel >= 0 && (size_t)st.listSel < st.files.size())
        keep = st.files[(size_t)st.listSel].sum.path;
    int c = st.sortCol;
    bool asc = st.sortAsc;
    std::stable_sort(st.files.begin(), st.files.end(), [&](const FileItem& a, const FileItem& b) {
        int r = 0;
        if (c == 0)      r = ToLowerW(Utf8ToWide(a.sum.name)).compare(ToLowerW(Utf8ToWide(b.sum.name)));
        else if (c == 1) r = (a.sum.dataVersion < b.sum.dataVersion) ? -1 : (a.sum.dataVersion > b.sum.dataVersion ? 1 : 0);
        else if (c == 2) r = (a.sum.totalBlocks < b.sum.totalBlocks) ? -1 : (a.sum.totalBlocks > b.sum.totalBlocks ? 1 : 0);
        else if (c == 3) r = ToLowerW(a.sum.dirName).compare(ToLowerW(b.sum.dirName));
        if (r == 0) r = ToLowerW(a.sum.fileName).compare(ToLowerW(b.sum.fileName));   // 次序稳定
        return asc ? (r < 0) : (r > 0);
    });
    st.listSel = -1;
    for (size_t i = 0; i < st.files.size(); ++i)
        if (!keep.empty() && EqualsNoCase(st.files[i].sum.path, keep)) { st.listSel = (int)i; break; }
    UpdateSearch();
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

void UpdateSearch() {
    State& st = Get();
    st.searchHits.clear();
    st.searchCur = -1;
    if (st.searchText.empty()) return;
    for (size_t i = 0; i < st.files.size(); ++i) {
        const FileItem& f = st.files[i];
        bool hit = FuzzyHit(Utf8ToWide(f.sum.name), st.searchText)
                || FuzzyHit(Utf8ToWide(f.sum.author), st.searchText)
                || FuzzyHit(f.sum.fileName, st.searchText)
                || FuzzyHit(f.sum.dirName, st.searchText);
        if (hit) st.searchHits.push_back((int)i);
    }
    if (!st.searchHits.empty()) st.searchCur = 0;
    SetStatus(st.searchHits.empty()
                  ? std::wstring(L"\u672a\u627e\u5230\u5339\u914d\u7684\u6295\u5f71")
                  : FmtW(L"\u641c\u7d22\u5230 %d \u4e2a\u6295\u5f71", (int)st.searchHits.size()));
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

void RunSearch(bool jumpToFirst) {
    UpdateSearch();
    if (jumpToFirst) SearchNext(0);
}

void SearchNext(int dir) {
    State& st = Get();
    if (st.searchHits.empty()) { UpdateSearch(); if (st.searchHits.empty()) return; }
    int n = (int)st.searchHits.size();
    if (dir == 0) st.searchCur = 0;
    else st.searchCur = ((st.searchCur + dir) % n + n) % n;
    int idx = st.searchHits[(size_t)st.searchCur];
    st.listSel = idx;
    OpenFile(idx);
    // 标记"需要把这一行滚入可见区"，由 DrawList 用它的真实几何来算。
    // 之前在这里自己算高度，漏掉了底部选择栏，导致目标项被挡住看不见。
    st.scrollToSel = true;
    st.scrollToIdx = idx;
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

// ---------------------------------------------------------------- 重命名文件
bool RenameCurrentFile(const std::wstring& newBaseName, std::string& err) {
    State& st = Get();
    if (!st.cur) { err = "没有打开的文件"; return false; }
    std::wstring oldPath = st.cur->Path();
    if (newBaseName.empty()) { err = "文件名不能为空"; return false; }
    std::wstring dir = fsx::GetDirectoryName(oldPath);
    std::wstring newPath = fsx::JoinPath(dir, newBaseName + L".litematic");
    if (EqualsNoCase(oldPath, newPath)) return true;
    if (fsx::FileExists(newPath)) { err = "同名文件已存在"; return false; }

    std::wstring bp;
    backup::CreateBackup(oldPath, L"rename", bp, err);
    if (!fsx::MoveFileTo(oldPath, newPath, false)) { err = "重命名失败"; return false; }

    // 备份目录也重命名，保持一致
    backup::Log(newPath, L"rename", newBaseName);
    for (auto& f : st.files) {
        if (EqualsNoCase(f.sum.path, oldPath)) {
            ltm::Schematic::Peek(newPath, f.sum);
            break;
        }
    }
    for (size_t i = 0; i < st.files.size(); ++i) {
        if (EqualsNoCase(st.files[i].sum.path, newPath)) { st.listSel = (int)i; OpenFile((int)i); break; }
    }
    return true;
}

// ---------------------------------------------------------------- 列表批量操作
void DeleteSelected() {
    State& st = Get();
    std::vector<int> sel;
    for (size_t i = 0; i < st.files.size(); ++i) if (st.files[i].checked) sel.push_back((int)i);
    if (sel.empty()) { SetStatus(L"\u8bf7\u5148\u52fe\u9009\u6295\u5f71\u6587\u4ef6"); InvalidateRect(st.hwnd, nullptr, FALSE); return; }
    wchar_t msg[256];
    swprintf(msg, 256, L"\u786e\u5b9a\u5220\u9664 %d \u4e2a\u6295\u5f71\u6587\u4ef6\uff1f\n\u5220\u9664\u524d\u4f1a\u81ea\u52a8\u5907\u4efd\u3002", (int)sel.size());
    if (MessageBoxW(st.hwnd, msg, i18n::T("app.title"), MB_YESNO | MB_ICONQUESTION) != IDYES) return;

    int n = 0;
    for (int idx : sel) {
        std::wstring path = st.files[(size_t)idx].sum.path;
        std::wstring bp; std::string err;
        backup::CreateBackup(path, i18n::T("log.op.delete"), bp, err);
        if (fsx::DeleteFileAt(path)) { ++n; backup::Log(path, i18n::T("log.op.delete"), L"ok"); }
    }
    SetStatus(FmtW(L"\u5df2\u5220\u9664 %d \u4e2a\u6587\u4ef6", n));
    RescanAsync();
}

void CopySelectedToClipboard() {
    State& st = Get();
    std::vector<std::wstring> paths;
    for (const auto& f : st.files) if (f.checked) paths.push_back(f.sum.path);
    if (paths.empty()) { SetStatus(L"\u8bf7\u5148\u52fe\u9009\u6295\u5f71\u6587\u4ef6"); InvalidateRect(st.hwnd, nullptr, FALSE); return; }

    size_t bytes = sizeof(DROPFILES) + 16;
    for (const auto& p : paths) bytes += (p.size() + 1) * sizeof(wchar_t);
    HGLOBAL hMem = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes);
    if (!hMem) return;
    BYTE* base = (BYTE*)GlobalLock(hMem);
    DROPFILES* df = (DROPFILES*)base;
    df->pFiles = sizeof(DROPFILES);
    df->fWide = TRUE;
    wchar_t* dst = (wchar_t*)(base + sizeof(DROPFILES));
    for (const auto& p : paths) { memcpy(dst, p.c_str(), (p.size() + 1) * sizeof(wchar_t)); dst += p.size() + 1; }
    *dst = 0;
    GlobalUnlock(hMem);
    if (OpenClipboard(st.hwnd)) {
        EmptyClipboard();
        SetClipboardData(CF_HDROP, hMem);
        CloseClipboard();
        SetStatus(FmtW(L"\u5df2\u5c06 %d \u4e2a\u6295\u5f71\u6dfb\u52a0\u5230\u526a\u8d34\u677f", (int)paths.size()));
        MessageBoxW(st.hwnd, L"\u5df2\u6dfb\u52a0\u5230\u526a\u8d34\u677f", i18n::T("app.title"), MB_OK | MB_ICONINFORMATION);
    } else {
        GlobalFree(hMem);
    }
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

void MoveSelected() {
    State& st = Get();
    std::vector<int> sel;
    for (size_t i = 0; i < st.files.size(); ++i) if (st.files[i].checked) sel.push_back((int)i);
    if (sel.empty()) { SetStatus(L"\u8bf7\u5148\u52fe\u9009\u6295\u5f71\u6587\u4ef6"); InvalidateRect(st.hwnd, nullptr, FALSE); return; }

    BROWSEINFOW bi = {};
    bi.hwndOwner = st.hwnd;
    bi.lpszTitle = L"\u9009\u62e9\u79fb\u52a8\u5230\u7684\u76ee\u6807\u6587\u4ef6\u5939";
    bi.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_USENEWUI;
    LPITEMIDLIST pidl = SHBrowseForFolderW(&bi);
    if (!pidl) return;
    wchar_t dir[MAX_PATH * 2] = L"";
    SHGetPathFromIDListW(pidl, dir);
    CoTaskMemFree(pidl);
    if (!dir[0]) return;

    int n = 0;
    for (int idx : sel) {
        std::wstring oldPath = st.files[(size_t)idx].sum.path;
        std::wstring newPath = fsx::JoinPath(dir, st.files[(size_t)idx].sum.fileName);
        if (EqualsNoCase(oldPath, newPath)) continue;
        if (fsx::FileExists(newPath)) continue;
        std::wstring bp; std::string err;
        backup::CreateBackup(oldPath, L"move", bp, err);
        if (fsx::MoveFileTo(oldPath, newPath, false)) {
            RebaseNotePath(oldPath, newPath);     // 备注跟着走
            backup::Log(newPath, L"move", oldPath);
            ++n;
        }
    }
    SetStatus(FmtW(L"\u5df2\u79fb\u52a8 %d \u4e2a\u6587\u4ef6", n));
    RescanAsync();
}

void OpenSelectedFolder() {
    State& st = Get();
    std::wstring dir;
    for (const auto& f : st.files) if (f.checked) { dir = f.sum.dirName; break; }
    if (dir.empty() && st.listSel >= 0 && (size_t)st.listSel < st.files.size())
        dir = st.files[(size_t)st.listSel].sum.dirName;
    if (!dir.empty())
        ShellExecuteW(nullptr, L"open", dir.c_str(), nullptr, nullptr, SW_SHOWNORMAL);
}


// ---------------------------------------------------------------- 导出预览图
// 按指定边长渲染当前投影到独立图像，不触碰界面预览用的 st.preview
static void RenderCurrentTo(int size, render::Image& out, std::string& err) {
    State& st = Get();
    out = render::Image();
    if (!st.cur) { err = "no schematic"; return; }
    if (!st.dbLoaded) { err = "block database not loaded"; return; }
    render::Options opt;
    opt.size = size;
    opt.view = st.previewView;
    opt.yawOverride = st.yaw;           // 导出当前视角（含手动旋转的角度）
    opt.pitch = st.pitch;
    opt.transparentBg = true;                  // 导出透明背景
    opt.bg = 0x00000000u;
    opt.supersample = (size > 1024) ? 1 : 2;   // 大图降低超采样，避免过慢
    ULONGLONG t0 = GetTickCount64();
    bool rokV = render::RenderSchematic(*st.cur, st.db, st.curVersionIdx, opt, out, err);
    fsx::DiagLog("RenderCurrentTo size=" + std::to_string(size) +
                 " ss=" + std::to_string(opt.supersample) +
                 " transparent=1 view=" + std::to_string(opt.view) +
                 " ok=" + std::to_string((int)rokV) +
                 " out=" + std::to_string(out.w) + "x" + std::to_string(out.h) +
                 " ms=" + std::to_string((int)(GetTickCount64() - t0)) +
                 (err.empty() ? "" : (" err=" + err)));
}

void ExportPreviews() {
    State& st = Get();
    std::vector<int> sel;
    for (size_t i = 0; i < st.files.size(); ++i) if (st.files[i].checked) sel.push_back((int)i);
    if (sel.empty() && st.listSel >= 0 && st.cur) sel.push_back(st.listSel);
    if (sel.empty()) {
        SetStatus(L"\u8bf7\u5148\u5728\u5de6\u4fa7\u52fe\u9009\u8981\u5bfc\u51fa\u7684\u6295\u5f71\u6587\u4ef6");
        InvalidateRect(st.hwnd, nullptr, FALSE);
        return;
    }
    if (!st.dbLoaded) { SetStatus(i18n::T("err.noaassets")); InvalidateRect(st.hwnd, nullptr, FALSE); return; }

    // 关键：先清掉本次点击状态，避免选择框阻塞期间重入 WM_PAINT 又触发一次导出
    st.input.mousePressed = false;
    st.input.mouseReleased = false;
    st.input.activeId = 0;
    st.input.usedMouse = true;

    BROWSEINFOW bi = {};
    bi.hwndOwner = ModalHosted() ? ModalHwnd() : st.hwnd;
    bi.lpszTitle = L"\u9009\u62e9\u9884\u89c8\u56fe\u7684\u8f93\u51fa\u6587\u4ef6\u5939";
    bi.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_USENEWUI;
    LPITEMIDLIST pidl = SHBrowseForFolderW(&bi);
    if (!pidl) return;
    wchar_t dir[MAX_PATH * 2] = L"";
    SHGetPathFromIDListW(pidl, dir);
    CoTaskMemFree(pidl);
    if (!dir[0]) return;
    packmgr::SaveLastExportDir(dir);
    bool dirOk = fsx::DirExists(dir);

    int keepSel = st.listSel;
    int ok = 0, rFail = 0, wFail = 0;
    std::wstring lastErr, hint;
    for (int idx : sel) {
        if (idx < 0 || idx >= (int)st.files.size()) { ++rFail; continue; }
        OpenFile(idx);
        // 导出用高分辨率单独渲染，界面预览尺寸不受影响
        render::Image img;
        std::string rerr;
        RenderCurrentTo(st.exportSize, img, rerr);
        if (img.px.empty()) {
            ++rFail;
            lastErr = rerr.empty() ? std::wstring(L"\u6e32\u67d3\u4e3a\u7a7a") : Utf8ToWide(rerr);
            continue;
        }
        std::wstring stem = fsx::GetFileStem(st.files[(size_t)idx].sum.fileName);
        if (stem.empty()) stem = L"preview";
        std::wstring out = fsx::JoinPath(dir, stem + L"_preview.png");
        ULONGLONG ts = GetTickCount64();
        bool sok = render::SavePng(out, img);
        fsx::DiagLog(std::string("SavePng ") + (sok ? "ok " : "FAIL ") + WideToUtf8(out) +
                     " ms=" + std::to_string((int)(GetTickCount64() - ts)));
        if (sok) { ++ok; continue; }
        DWORD e = GetLastError();
        if (e == ERROR_ACCESS_DENIED) {
            // 常见原因：目标文件只读（旧文件属性残留）——清掉属性再试一次
            SetFileAttributesW(out.c_str(), FILE_ATTRIBUTE_NORMAL);
            ULONGLONG ts = GetTickCount64();
        bool sok = render::SavePng(out, img);
        fsx::DiagLog(std::string("SavePng ") + (sok ? "ok " : "FAIL ") + WideToUtf8(out) +
                     " ms=" + std::to_string((int)(GetTickCount64() - ts)));
        if (sok) { ++ok; continue; }
            e = GetLastError();
            hint = L"\u62d2\u7edd\u8bbf\u95ee(5)\uff1a\u76ee\u6807\u76ee\u5f55\u4e0d\u5141\u8bb8\u5199\u5165\u3002"
                   L"\u5e38\u89c1\u539f\u56e0\uff1a\u5fae\u8f6f Defender \u7684\u201c\u53d7\u63a7\u6587\u4ef6\u5939\u8bbf\u95ee\u201d"
                   L"\u4fdd\u62a4\u4e86\u684c\u9762/\u6587\u6863\uff0c\u6216\u76ee\u5f55\u4e3a\u53ea\u8bfb\u3002"
                   L"\n\u8bf7\u6539\u9009\u4e00\u4e2a\u666e\u901a\u76ee\u5f55\uff08\u5982 D:\\ \u6216 E:\\ \u4e0b\u7684\u6587\u4ef6\u5939\uff09\u91cd\u8bd5\u3002";
        }
        ++wFail;
        lastErr = out + FmtW(L"  (Win32 err=%lu)", (unsigned long)e);
    }
    if (keepSel >= 0 && keepSel < (int)st.files.size()) OpenFile(keepSel);

    std::wstring msg = FmtW(L"\u5bfc\u51fa\u5b8c\u6210\uff1a\u6210\u529f %d\uff0c\u6e32\u67d3\u5931\u8d25 %d\uff0c\u5199\u6587\u4ef6\u5931\u8d25 %d\uff5c%s",
                            ok, rFail, wFail, dir);
    if (!lastErr.empty()) msg += L"\n\n\u6700\u540e\u4e00\u6761\u9519\u8bef\uff1a\n" + lastErr;
    if (!dirOk) msg += L"\n\n\u6ce8\u610f\uff1a\u8f93\u51fa\u76ee\u5f55\u4e0d\u5b58\u5728";
    if (!hint.empty()) msg += L"\n\n" + hint;
    msg += L"\n\n\u53d7\u6d4b\u8bd5\u9879\uff1a" + std::to_wstring((int)sel.size());
    SetStatus(L"\u5bfc\u51fa\u5b8c\u6210");
    // 完整结果用弹框显示，避免状态栏截断
    MessageBoxW(ModalHosted() ? ModalHwnd() : st.hwnd, msg.c_str(),
                L"\u5bfc\u51fa\u9884\u89c8\u56fe\u7ed3\u679c", MB_OK | MB_ICONINFORMATION);
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

} // namespace app
