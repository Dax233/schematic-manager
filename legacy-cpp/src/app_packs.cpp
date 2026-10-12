// 贴图与资源包管理界面 + 本机游戏自动检测导入
#include "app_internal.h"
#include "packbuild.h"
#include "packmgr.h"
#include "render.h"
#include "i18n.h"
#include "str.h"
#include "fs.h"
#include <commdlg.h>

namespace app {

// ------------------------------------------------------------------ 后台检测/导入
static volatile LONG g_pkRunning = 0;
static volatile LONG g_pkDone = 0;
static std::vector<packmgr::Install> g_pkFound;
static std::vector<std::wstring> g_pkLog;
static CRITICAL_SECTION g_pkCs;
static bool g_pkCsInit = false;

struct PkCtx {
    std::vector<std::wstring> roots;
    std::set<i32> alreadyHave;      // 内置库 + 已导入的 dataVersion
    bool autoImport;
};

static DWORD WINAPI PackThread(LPVOID param) {
    PkCtx* ctx = (PkCtx*)param;
    std::vector<std::wstring> log;

    std::vector<packmgr::Install> all;
    packmgr::ScanInstalls(all, ctx->roots, 4);
    std::vector<packmgr::Install> rel;
    packmgr::FilterReleases(all, rel);

    int imported = 0, skipped = 0;
    for (const auto& ins : rel) {
        if (ins.dataVersion > 0 && ctx->alreadyHave.count(ins.dataVersion)) { ++skipped; continue; }
        if (!ctx->autoImport) continue;
        std::string name = ins.versionId;
        std::string err;
        packbuild::Result info;
        if (packmgr::ImportJar(ins.jarPath, 1, ins.dataVersion, name, err, &info)) {
            wchar_t buf[256];
            swprintf(buf, 256, L"%s: %zu \u65b9\u5757 / %zu \u6a21\u578b / %zu \u8d34\u56fe",
                     Utf8ToWide(name).c_str(), info.blockCount, info.modelCount, info.textureCount);
            log.push_back(buf);
            ++imported;
        } else {
            log.push_back(Utf8ToWide(name) + L": " + Utf8ToWide(err));
        }
    }
    if (imported)
        log.insert(log.begin(), FmtW(L"\u5df2\u81ea\u52a8\u5bfc\u5165 %d \u4e2a\u65b0\u7248\u672c", imported));

    EnterCriticalSection(&g_pkCs);
    g_pkFound.swap(rel);
    g_pkLog.swap(log);
    LeaveCriticalSection(&g_pkCs);
    InterlockedExchange(&g_pkDone, 1);
    InterlockedExchange(&g_pkRunning, 0);
    delete ctx;
    return 0;
}

bool PackImportRunning() { return g_pkRunning != 0; }

void StartPackDetect(bool autoImport) {
    State& st = Get();
    if (!g_pkCsInit) { InitializeCriticalSection(&g_pkCs); g_pkCsInit = true; }
    if (InterlockedCompareExchange(&g_pkRunning, 1, 0) != 0) return;

    PkCtx* ctx = new PkCtx();
    ctx->roots = packmgr::LoadScanRoots();
    ctx->autoImport = autoImport;
    for (size_t i = 0; i < st.db.VersionCount(); ++i)
        if (!st.db.Version(i).isOverlay) ctx->alreadyHave.insert(st.db.Version(i).dataVersion);
    for (const auto& e : packmgr::LoadManifest()) ctx->alreadyHave.insert(e.dataVersion);

    st.detecting = true;
    st.detectMsg = L"\u6b63\u5728\u68c0\u6d4b\u672c\u673a Minecraft \u5b89\u88c5\u2026";
    InvalidateRect(st.hwnd, nullptr, FALSE);

    InterlockedExchange(&g_pkDone, 0);
    HANDLE h = CreateThread(nullptr, 0, PackThread, ctx, 0, nullptr);
    if (h) CloseHandle(h);
    else { delete ctx; InterlockedExchange(&g_pkRunning, 0); st.detecting = false; }
}

void TickPacks() {
    State& st = Get();
    if (InterlockedCompareExchange(&g_pkDone, 0, 1) != 1) return;
    EnterCriticalSection(&g_pkCs);
    st.detected.swap(g_pkFound);
    std::vector<std::wstring> log;
    log.swap(g_pkLog);
    LeaveCriticalSection(&g_pkCs);
    st.detecting = false;

    bool importedSomething = false;
    for (const auto& l : log)
        if (l.find(L"\u5df2\u81ea\u52a8\u5bfc\u5165") != std::wstring::npos) importedSomething = true;

    if (importedSomething) {
        std::string err;
        ReloadPacks();
    }
    if (!log.empty()) st.detectMsg = log[0];
    else st.detectMsg = FmtW(L"\u68c0\u6d4b\u5230 %zu \u4e2a\u6b63\u5f0f\u7248", st.detected.size());
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

// 重新加载内置库 + 全部用户资源包
void ReloadPacks() {
    State& st = Get();
    std::string err;
    st.db = mc::BlockDB();
    if (!st.db.LoadEmbedded(101, err)) {
        std::wstring p = fsx::JoinPath(fsx::GetDirectoryName(fsx::GetExePath()), L"blocks.dat");
        if (fsx::FileExists(p)) st.db.LoadFromFile(p, err);
    }
    st.dbLoaded = !st.db.Empty();
    st.db.LoadUserPacks(packmgr::PacksDir(), err);
    st.iconCache.clear();
    if (st.cur) {
        st.curVersionIdx = st.dbLoaded ? st.db.PickVersion(st.cur->GetSummary().dataVersion) : -1;
        if (st.dbLoaded && st.curVersionIdx >= 0) st.db.SetActiveVersion(st.curVersionIdx);
        st.previewDirty = true;
        RenderPreview();
    }
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

// ------------------------------------------------------------------ 导入文件选择
static bool PickFile(std::wstring& out, const wchar_t* filter, const wchar_t* title) {
    wchar_t buf[MAX_PATH * 2] = L"";
    OPENFILENAMEW ofn = {};
    ofn.lStructSize = sizeof(ofn);
    ofn.hwndOwner = Get().hwnd;
    ofn.lpstrFilter = filter;
    ofn.lpstrFile = buf;
    ofn.nMaxFile = MAX_PATH * 2;
    ofn.lpstrTitle = title;
    ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_EXPLORER;
    if (!GetOpenFileNameW(&ofn)) return false;
    out = buf;
    return true;
}

static void DoImportJar(int kind) {
    State& st = Get();
    std::wstring jar;
    const wchar_t* filter = (kind == 2)
        ? L"模组 / 资源包 (*.jar;*.zip)\0*.jar;*.zip\0所有文件\0*.*\0\0"
        : L"Minecraft \u5ba2\u6237\u7aef (*.jar)\0*.jar\0\u6240\u6709\u6587\u4ef6\0*.*\0\0";
    const wchar_t* title = (kind == 2) ? L"\u9009\u62e9\u6a21\u7ec4 jar" : L"\u9009\u62e9 Minecraft \u5ba2\u6237\u7aef jar";
    if (!PickFile(jar, filter, title)) return;

    std::string name = WideToUtf8(fsx::GetFileStem(jar));
    i32 dv = 0;
    if (kind == 1) {
        // 客户端 jar：dataVersion 由 jar 内 version.json 决定（forcedDataVersion=0 时自动读）
        dv = 0;
    } else {
        // 模组：叠加到当前选中投影的版本
        if (st.cur) dv = st.cur->GetSummary().dataVersion;
        else if (st.dbLoaded && st.db.VersionCount()) dv = st.db.Version(st.db.VersionCount() - 1).dataVersion;
    }

    std::string err;
    packbuild::Result info;
    if (!packmgr::ImportJar(jar, kind, dv, name, err, &info)) {
        st.detectMsg = L"\u5bfc\u5165\u5931\u8d25: " + Utf8ToWide(err);
        InvalidateRect(st.hwnd, nullptr, FALSE);
        return;
    }
    st.detectMsg = FmtW(L"\u5df2\u5bfc\u5165 %s\uff1a%zu \u65b9\u5757 / %zu \u6a21\u578b / %zu \u8d34\u56fe",
                        Utf8ToWide(name).c_str(), info.blockCount, info.modelCount, info.textureCount);
    ReloadPacks();
}

// ------------------------------------------------------------------ 界面
void DrawPackModal(ui::Ctx& c, const ui::Rect& r) {
    State& st = Get();
    ModalFrame(c, r, L"\u8d34\u56fe\u4e0e\u8d44\u6e90\u5305\u7ba1\u7406");

    // 右侧固定一块"使用说明"面板，把选择规则直接摊开，避免用户猜
    const int helpW = 384;
    int x = r.x + 18, y = r.y + 48;
    int w = r.w - 36 - helpW - 14;          // 主内容区宽度（不含右侧说明）

    {
        ui::Rect hp = { r.Right() - 18 - helpW, y, helpW, r.h - 48 - 18 };
        c.p->Fill(hp, RGB(0xF7, 0xF9, 0xFC));
        c.p->Frame(hp, Theme::Separator, 1);
        c.p->Fill({ hp.x, hp.y, hp.w, 28 }, Theme::BgHeader);
        c.p->Text(L"\u4f7f\u7528\u8bf4\u660e", { hp.x + 12, hp.y, hp.w - 24, 28 },
                  Theme::Text, c.f->uiBold, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        std::wstring h =
            L"\u3010\u5bfc\u5165\u5ba2\u6237\u7aef jar\u2026\u3011\n"
            L"\u9009\u62e9\u5177\u4f53\u7684 .jar \u6587\u4ef6\uff0c\u4e0d\u662f\u6587\u4ef6\u5939\uff1a\n"
            L"  .minecraft\\versions\\<\u7248\u672c>\\<\u7248\u672c>.jar\n\n"
            L"\u3010\u5bfc\u5165\u6a21\u7ec4\u8d34\u56fe\u2026\u3011\n"
            L"\u9009\u62e9\u6a21\u7ec4 jar \u6587\u4ef6\uff08Fabric / Forge \u5747\u53ef\uff09\n\n"
            L"\u3010\u6dfb\u52a0\u626b\u63cf\u76ee\u5f55\u2026\u3011\n"
            L"\u9009\u62e9\u6587\u4ef6\u5939\uff0c\u7a0b\u5e8f\u4f1a\u9012\u5f52\u626b\u63cf\u5176\u4e2d\u6240\u6709 jar\uff1a\n"
            L"  .minecraft \u6216\u6e38\u620f\u6839\u76ee\u5f55\u5373\u53ef\n\n"
            L"\u3010\u68c0\u6d4b\u672c\u673a\u6e38\u620f\u3011\n"
            L"\u81ea\u52a8\u5728\u5e38\u89c1\u4f4d\u7f6e\u67e5\u627e\u6e38\u620f\u5b89\u88c5\n\n"
            L"\u26a0 \u8981\u9009\u5ba2\u6237\u7aef jar\uff0c\u4e0d\u8981\u670d\u52a1\u7aef\n"
            L"  \u5ba2\u6237\u7aef\u542b assets/  models/  textures/\n"
            L"  \u670d\u52a1\u7aef\u6ca1\u6709\u8d34\u56fe\u8d44\u6e90\n\n"
            L"\u63d0\u793a\uff1a1.13.2 ~ 26.3 \u7684 49 \u4e2a\u6b63\u5f0f\u7248\n"
            L"\u8d34\u56fe\u5df2\u5185\u7f6e\uff0c\u901a\u5e38\u65e0\u9700\u624b\u52a8\u5bfc\u5165\u3002";
        c.p->Text(h, { hp.x + 12, hp.y + 36, hp.w - 24, hp.h - 44 },
                  Theme::Text, c.f->small, DT_LEFT | DT_TOP | DT_WORDBREAK);
    }

    // 工具条
    int bw = 132;
    if (ui::Button(c, CMD_PACKS_IMPORT_MOD, { x, y, bw, 28 }, L"\u5bfc\u5165\u6a21\u7ec4\u8d34\u56fe\u2026"))
        DoImportJar(2);
    if (ui::Button(c, CMD_PACKS_IMPORT_MC, { x + bw + 6, y, bw, 28 }, L"\u5bfc\u5165\u5ba2\u6237\u7aef jar\u2026"))
        DoImportJar(1);
    if (ui::Button(c, CMD_PACKS_DETECT, { x + (bw + 6) * 2, y, bw, 28 }, L"\u68c0\u6d4b\u672c\u673a\u6e38\u620f"))
        StartPackDetect(false);
    if (ui::Button(c, CMD_PACKS_ADDROOT, { x + (bw + 6) * 3, y, bw, 28 }, L"\u6dfb\u52a0\u626b\u63cf\u76ee\u5f55\u2026"))
        ExecuteCommand(CMD_PACKS_ADDROOT);
    if (ui::Button(c, CMD_PACKS_IMPORT_ALL, { x + (bw + 6) * 4, y, bw, 28 },
                   L"\u5168\u90e8\u5bfc\u5165", !st.detected.empty()))
        ExecuteCommand(CMD_PACKS_IMPORT_ALL);
    y += 34;

    // 自动导入开关
    {
        ui::Rect cb = { x, y, 320, 22 };
        bool ai = packmgr::AutoImportEnabled();
        if (ui::Checkbox(c, CMD_PACKS_AUTOIMPORT, cb, ai)) packmgr::SetAutoImportEnabled(ai);
        c.p->Text(L"\u626b\u63cf\u76ee\u5f55\u65f6\u81ea\u52a8\u8bc6\u522b\u5e76\u5bfc\u5165\u65b0\u6b63\u5f0f\u7248",
                  { cb.x + 20, cb.y, cb.w, cb.h }, Theme::Text, c.f->small);
        c.p->Text(st.detectMsg, { x + 340, y, w - 340, 22 }, Theme::TextDim, c.f->small,
                  DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    }
    y += 30;

    // ---- 已加载资源包
    c.p->Text(L"\u5df2\u52a0\u8f7d\u8d44\u6e90\u5305", { x, y, w, 20 }, Theme::Text, c.f->uiBold);
    y += 22;
    ui::Rect list1 = { x, y, w, 190 };
    c.p->Fill(list1, RGB(255, 255, 255));
    c.p->Frame(list1, Theme::Separator, 1);

    struct Row { std::wstring name, info; bool enabled; bool fromBase; };
    std::vector<Row> rows;
    {
        size_t blocks = 0, overlays = 0;
        for (size_t i = 0; i < st.db.VersionCount(); ++i) {
            blocks += st.db.Version(i).blocks.size();
            if (st.db.Version(i).isOverlay) ++overlays;
        }
        Row baseRow;
        baseRow.name = L"\u5185\u7f6e\u65b9\u5757\u5e93";
        baseRow.info = FmtW(L"%zu \u4e2a\u7248\u672c   %zu \u6761\u65b9\u5757\u5b9a\u4e49   %zu \u6a21\u7ec4\u53e0\u52a0\u5305",
                            st.db.VersionCount() - overlays, blocks, overlays);
        baseRow.enabled = true;
        baseRow.fromBase = true;
        rows.push_back(baseRow);
    }
    for (const auto& e : packmgr::LoadManifest()) {
        Row row;
        row.name = Utf8ToWide(e.name);
        row.info = FmtW(L"%s   dataVersion=%d",
                        e.kind == 2 ? L"\u6a21\u7ec4\u53e0\u52a0\u5305" : L"MC \u7248\u672c\u5305",
                        (int)e.dataVersion);
        row.enabled = e.enabled;
        row.fromBase = false;
        rows.push_back(row);
    }
    if (st.packSel >= (int)rows.size()) st.packSel = -1;

    int rowH = 30;
    int total = (int)rows.size() * rowH;
    int maxScroll = std::max(0, total - (list1.h - 2));
    if (st.packScroll > maxScroll) st.packScroll = maxScroll;
    ui::Rect inner1 = { list1.x + 1, list1.y + 1, list1.w - 15, list1.h - 2 };
    c.p->PushClip(inner1);
    for (int i = 0; i < (int)rows.size(); ++i) {
        ui::Rect row = { inner1.x, inner1.y + i * rowH - st.packScroll, inner1.w, rowH };
        if (row.Bottom() < inner1.y || row.y > inner1.Bottom()) continue;
        bool sel = (st.packSel == i);
        bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
        if (sel) c.p->Fill(row, Theme::BgListSel);
        else if (hov) c.p->Fill(row, Theme::BgHover);
        if (rows[(size_t)i].fromBase) {
            c.p->Text(L"\u25A0", { row.x + 8, row.y, 18, row.h }, Theme::Accent, c.f->ui);
        } else {
            bool en = rows[(size_t)i].enabled;
            ui::Rect cb = { row.x + 8, row.y + 7, 16, 16 };
            if (ui::Checkbox(c, ui::Id("packchk", i), cb, en)) {
                auto list = packmgr::LoadManifest();
                if (i - 1 >= 0 && (size_t)(i - 1) < list.size()) {
                    list[(size_t)(i - 1)].enabled = en;
                    packmgr::SaveManifest(list);
                    ReloadPacks();
                }
            }
        }
        c.p->Text(rows[(size_t)i].name, { row.x + 32, row.y, 340, row.h }, Theme::Text, c.f->ui);
        c.p->Text(rows[(size_t)i].info, { row.x + 380, row.y, row.w - 390, row.h },
                  Theme::TextDim, c.f->small, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        if (!rows[(size_t)i].fromBase) {
            if (ui::IconButton(c, ui::Id("packdel", i), { row.Right() - 30, row.y + 4, 24, 22 }, L"\u2715")) {
                auto list = packmgr::LoadManifest();
                if ((size_t)(i - 1) < list.size()) {
                    packmgr::RemovePack(list[(size_t)(i - 1)].name, true);
                    st.packSel = -1;
                    ReloadPacks();
                    return;
                }
            }
        }
        if (hov && c.in->mouseReleased && !c.in->usedMouse) st.packSel = i;
        if (hov && c.in->mousePressed) c.in->usedMouse = true;
    }
    c.p->PopClip();
    ui::ScrollBar(c, CMD_NONE + 3000, { inner1.Right() + 2, inner1.y, 12, inner1.h },
                  st.packScroll, total, inner1.h);
    y = list1.Bottom() + 12;

    // ---- 检测到的本机安装
    c.p->Text(L"\u68c0\u6d4b\u5230\u7684\u672c\u673a Minecraft \u5b89\u88c5", { x, y, w, 20 }, Theme::Text, c.f->uiBold);
    y += 22;
    ui::Rect list2 = { x, y, w, r.Bottom() - y - 54 };
    c.p->Fill(list2, RGB(255, 255, 255));
    c.p->Frame(list2, Theme::Separator, 1);
    ui::Rect inner2 = { list2.x + 1, list2.y + 1, list2.w - 15, list2.h - 2 };

    std::set<i32> have;
    for (size_t i = 0; i < st.db.VersionCount(); ++i)
        if (!st.db.Version(i).isOverlay) have.insert(st.db.Version(i).dataVersion);

    if (st.detected.empty()) {
        c.p->Text(st.detecting ? L"\u6b63\u5728\u68c0\u6d4b\u2026"
                               : L"\u70b9\u51fb\u4e0a\u65b9\u201c\u68c0\u6d4b\u672c\u673a\u6e38\u620f\u201d\u5f00\u59cb\u626b\u63cf",
                  inner2, Theme::TextDim, c.f->ui, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    } else {
        int rowH2 = 24;
        int total2 = (int)st.detected.size() * rowH2;
        int maxScroll2 = std::max(0, total2 - inner2.h);
        if (st.installScroll > maxScroll2) st.installScroll = maxScroll2;
        c.p->PushClip(inner2);
        int first = st.installScroll / rowH2;
        int last = std::min((int)st.detected.size(), first + inner2.h / rowH2 + 2);
        for (int i = first; i < last; ++i) {
            const auto& ins = st.detected[(size_t)i];
            ui::Rect row = { inner2.x, inner2.y + i * rowH2 - st.installScroll, inner2.w, rowH2 };
            bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
            if (hov) c.p->Fill(row, Theme::BgHover);
            bool has = have.count(ins.dataVersion) != 0;
            c.p->Text(Utf8ToWide(ins.versionId), { row.x + 8, row.y, 200, row.h }, Theme::Text, c.f->small);
            c.p->Text(FmtW(L"dv=%d", ins.dataVersion), { row.x + 214, row.y, 90, row.h },
                      Theme::TextDim, c.f->small);
            c.p->Text(FormatBytes(ins.jarSize), { row.x + 310, row.y, 80, row.h },
                      Theme::TextDim, c.f->small);
            c.p->Text(has ? L"\u5df2\u5bfc\u5165" : L"\u65b0\u589e", { row.x + 400, row.y, 70, row.h },
                      has ? Theme::TextDim : Theme::Accent, c.f->smallBold);
            if (hov && c.in->mouseReleased && !c.in->usedMouse) st.installSel = i;
            if (hov && c.in->mousePressed) c.in->usedMouse = true;
        }
        c.p->PopClip();
        ui::ScrollBar(c, CMD_NONE + 3010, { inner2.Right() + 2, inner2.y, 12, inner2.h },
                      st.installScroll, total2, inner2.h);
    }

    // 底部按钮
    int by = r.Bottom() - 44;
    bool hasSel = (st.packSel > 0);
    if (ui::Button(c, CMD_PACKS_DELETE, { x, by, 150, 30 },
                   L"\u5220\u9664\u9009\u4e2d\u8d44\u6e90\u5305", hasSel)) {
        auto list = packmgr::LoadManifest();
        if (st.packSel - 1 >= 0 && (size_t)(st.packSel - 1) < list.size()) {
            packmgr::RemovePack(list[(size_t)(st.packSel - 1)].name, true);
            st.packSel = -1;
            ReloadPacks();
        }
    }
    if (ui::Button(c, CMD_PACKS_RELOAD, { x + 158, by, 120, 30 }, L"\u91cd\u65b0\u52a0\u8f7d"))
        ReloadPacks();
    if (ui::Button(c, CMD_MODAL_CANCEL, { r.Right() - 110, by, 92, 30 }, i18n::T("btn.close")))
        ExecuteCommand(CMD_MODAL_CANCEL);
}

} // namespace app
