#include "app.h"
#include "str.h"
#include "fs.h"
#include "i18n.h"
#include <ctime>

namespace app {

static State g_state;

State& Get() { return g_state; }

// ------------------------------------------------------------------ 工具
static std::wstring Fmt(const wchar_t* fmt, ...) {
    wchar_t buf[1024];
    va_list ap;
    va_start(ap, fmt);
    _vsnwprintf(buf, 1023, fmt, ap);
    va_end(ap);
    buf[1023] = 0;
    return buf;
}

void SetStatus(const std::wstring& s) { g_state.status = s; }

bool IsScanning();

std::wstring BlockDisplay(const std::string& blockName, bool chinese) {
    State& st = Get();
    if (!st.dbLoaded) return Utf8ToWide(blockName);
    const mc::VersionDef* pref = nullptr;
    if (st.curVersionIdx >= 0 && (size_t)st.curVersionIdx < st.db.VersionCount())
        pref = &st.db.Version((size_t)st.curVersionIdx);
    std::string n = st.db.DisplayName(blockName, chinese, pref);
    if (n == blockName) {
        // 没有翻译：去掉命名空间
        size_t c = blockName.find(':');
        if (c != std::string::npos) n = blockName.substr(c + 1);
    }
    return Utf8ToWide(n);
}

bool IsModBlock(const std::string& blockName) {
    size_t c = blockName.find(':');
    if (c == std::string::npos) return false;
    return blockName.substr(0, c) != "minecraft";
}

const render::Image* GetBlockIcon(const std::string& blockName, int size) {
    State& st = Get();
    std::string key = blockName + "#" + std::to_string(size) + "#" + std::to_string(st.curVersionIdx);
    auto it = st.iconCache.find(key);
    if (it != st.iconCache.end()) return &it->second;
    if (!st.dbLoaded || st.curVersionIdx < 0) return nullptr;

    ltm::BlockState bs;
    bs.name = blockName;
    // 用该方块的默认状态
    std::vector<std::pair<std::string, std::string>> defs;
    if (st.db.DefaultProps(blockName, defs))
        for (const auto& kv : defs) bs.SetProp(kv.first, kv.second);

    render::Image img;
    if (!render::RenderBlockIcon(st.db, st.curVersionIdx, bs, size, img, true)) {
        st.iconCache[key] = render::Image();   // 缓存失败
        return nullptr;
    }
    auto res = st.iconCache.emplace(key, std::move(img));
    return &res.first->second;
}

// ------------------------------------------------------------------ 初始化
bool Init(HWND hwnd) {
    State& st = Get();
    st.hwnd = hwnd;
    HDC hdc = GetDC(hwnd);
    st.dpi = hdc ? GetDeviceCaps(hdc, LOGPIXELSX) : 96;
    if (hdc) ReleaseDC(hwnd, hdc);
    if (st.dpi <= 0) st.dpi = 96;
    st.fonts.Create(st.dpi);
    backup::Init();

    // 载入内置方块库
    std::string err;
    if (st.db.LoadEmbedded(101, err)) {
        st.dbLoaded = true;
    } else {
        st.dbLoaded = false;
        st.dbError = err;
        // 开发期回退：尝试从磁盘加载
        std::wstring p = fsx::JoinPath(fsx::GetDirectoryName(fsx::GetExePath()), L"blocks.dat");
        if (fsx::FileExists(p) && st.db.LoadFromFile(p, err)) {
            st.dbLoaded = true;
            st.dbError.clear();
        }
    }
    if (st.dbLoaded) {
        // 默认扫描目录：EXE 同级 schematics，或用户目录下的 .minecraft/schematics
        std::wstring exeDir = fsx::GetDirectoryName(fsx::GetExePath());
        std::wstring cand = fsx::JoinPath(exeDir, L"schematics");
        if (fsx::DirExists(cand)) st.scanDir = cand;
        else {
            wchar_t appdata[MAX_PATH];
            if (GetEnvironmentVariableW(L"APPDATA", appdata, MAX_PATH)) {
                std::wstring mc = fsx::JoinPath(appdata, L".minecraft\\schematics");
                if (fsx::DirExists(mc)) st.scanDir = mc;
            }
        }
    } else {
        std::wstring exeDir = fsx::GetDirectoryName(fsx::GetExePath());
        std::wstring cand = fsx::JoinPath(exeDir, L"schematics");
        if (fsx::DirExists(cand)) st.scanDir = cand;
    }
    st.exportSize = packmgr::LoadExportSize();
    st.brightness = packmgr::LoadBrightness();
    render::SetBrightness(st.brightness);
    LoadDirsFromSettings();
    std::wstring last = packmgr::LoadLastDir();   // 恢复上次的地址选择
    if (!last.empty()) st.scanDir = last;
    else if (!st.dirs.empty()) st.scanDir = st.dirs[0].path;
    if (st.dirs.empty() && !st.scanDir.empty()) AddDir(st.scanDir, true);
    st.status = i18n::T("status.ready");
    return true;
}

void Shutdown() {
    State& st = Get();
    st.fonts.Destroy();
    st.cur.reset();
    st.files.clear();
    st.iconCache.clear();
}

void SetScanDir(const std::wstring& dir) {
    g_state.scanDir = dir;
    PushRecentDir(dir);          // 记录到浏览历史
    packmgr::SaveLastDir(dir);   // 记住本次选择，下次启动恢复
    AddDir(dir, true);           // 同时纳入多目录扫描
}

// ------------------------------------------------------------------ 扫描（后台线程）
struct ScanCtx {
    std::vector<std::wstring> dirs;
    bool recursive = true;
};

static volatile LONG  g_scanRunning = 0;
static volatile LONG  g_scanDone = 0;
static std::vector<FileItem> g_scanResult;
static CRITICAL_SECTION g_scanCs;
static bool g_scanCsInit = false;

static u64 HashBytes(const std::vector<u8>& d) {
    u64 h = 1469598103934665603ULL;
    for (u8 b : d) { h ^= b; h *= 1099511628211ULL; }
    return h;
}

static DWORD WINAPI ScanThread(LPVOID param) {
    ScanCtx* ctx = (ScanCtx*)param;
    std::vector<FileInfo> found;
    for (const std::wstring& d : ctx->dirs) {
        if (!fsx::DirExists(d)) continue;
        fsx::ListFilesRecursive(d, L".litematic", found, ctx->recursive ? 12 : 0);
    }
    std::sort(found.begin(), found.end(), [](const FileInfo& a, const FileInfo& b) { return a.name < b.name; });
    // 去重
    found.erase(std::unique(found.begin(), found.end(),
                [](const FileInfo& a, const FileInfo& b) { return EqualsNoCase(a.path, b.path); }), found.end());

    std::vector<FileItem> out;
    out.reserve(found.size());
    for (const FileInfo& fi : found) {
        FileItem item;
        ltm::Schematic::Peek(fi.path, item.sum);
        if (item.sum.fileSize == 0) item.sum.fileSize = fi.size;
        item.sum.fileTime = fi.mtime;
        std::vector<u8> raw;
        if (fsx::ReadAllBytes(fi.path, raw)) item.contentHash = HashBytes(raw);
        out.push_back(std::move(item));
    }
    EnterCriticalSection(&g_scanCs);
    g_scanResult.swap(out);
    LeaveCriticalSection(&g_scanCs);
    InterlockedExchange(&g_scanDone, 1);
    InterlockedExchange(&g_scanRunning, 0);
    delete ctx;
    return 0;
}

void RescanAsync() {
    State& st = Get();
    if (!g_scanCsInit) { InitializeCriticalSection(&g_scanCs); g_scanCsInit = true; }
    if (InterlockedCompareExchange(&g_scanRunning, 1, 0) != 0) return;
    if (EnabledDirs().empty()) { InterlockedExchange(&g_scanRunning, 0); return; }
    InterlockedExchange(&g_scanDone, 0);
    ScanCtx* ctx = new ScanCtx{ EnabledDirs(), st.recursive };
    SetStatus(i18n::T("status.scanning"));
    HANDLE h = CreateThread(nullptr, 0, ScanThread, ctx, 0, nullptr);
    if (h) CloseHandle(h);
    else { delete ctx; InterlockedExchange(&g_scanRunning, 0); }
}

void Tick() {
    State& st = Get();
    TickPacks();
    if (InterlockedCompareExchange(&g_scanDone, 0, 1) == 1) {
        EnterCriticalSection(&g_scanCs);
        st.files.swap(g_scanResult);
        g_scanResult.clear();
        LeaveCriticalSection(&g_scanCs);
        st.dupGroups.clear();
        UpdateSearch();
        AttachNotes();
        st.listSel = st.files.empty() ? -1 : 0;
        st.listScroll = 0;
        st.iconCache.clear();
        if (st.listSel >= 0) OpenFile(st.listSel);
        else { st.cur.reset(); st.curAnalysisDone = false; st.preview = render::Image(); }
        // 扫描完成后自动检测本机新正式版（可在设置里关掉）
        if (!PackImportRunning() && st.detected.empty()) StartPackDetect(packmgr::AutoImportEnabled());
        InvalidateRect(st.hwnd, nullptr, FALSE);
    }
    if (g_scanRunning && st.status == i18n::T("status.scanning")) {
        InvalidateRect(st.hwnd, nullptr, FALSE);
    }
}

// ------------------------------------------------------------------ 打开 / 渲染
void OpenFile(int index) {
    State& st = Get();
    if (index < 0 || (size_t)index >= st.files.size()) return;
    if (st.cur && st.cur->IsModified()) {
        // 有未保存修改时先不自动保存，交给用户决定
    }
    st.cur.reset(new ltm::Schematic());
    std::string err;
    if (!st.cur->Load(st.files[(size_t)index].sum.path, err)) {
        st.cur.reset();
        SetStatus(Utf8ToWide(err));
        return;
    }
    st.curAnalysis = st.cur->Analyze();
    st.curAnalysisDone = true;
    st.curVersionIdx = st.dbLoaded ? st.db.PickVersion(st.cur->GetSummary().dataVersion) : -1;
    if (st.dbLoaded && st.curVersionIdx >= 0) st.db.SetActiveVersion(st.curVersionIdx);
    st.blockScroll = st.entityScroll = 0;
    st.blockSel = -1;
    st.blockFromName.clear();
    st.blockToName.clear();
    st.blockPickOpen = false;
    st.infoEdit = -1;
    ++st.curGen;             // 标记换了文件
    st.previewDirty = true;
    st.previewFailed = false;
    RenderPreview();
    st.iconCache.clear();
}

// 跳到某个预设视角（四个快捷按钮用）
void SetViewPreset(int preset) {
    State& st = Get();
    float y, p;
    render::GetViewAngles(preset, y, p);
    st.yaw = y;
    st.pitch = p;
    st.previewView = preset;
    RenderPreview();
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

void RenderPreview() {
    State& st = Get();
    if (!st.cur) { st.preview = render::Image(); return; }
    if (!st.dbLoaded) {
        st.previewFailed = true;
        std::wstring w = i18n::T("err.noaassets");
        if (!st.dbError.empty()) w += L"\n(" + Utf8ToWide(st.dbError) + L")";
        st.previewErr = w;
        st.preview = render::Image();
        return;
    }
    render::Options opt;
    opt.size = st.previewSize;
    opt.view = st.previewView;
    opt.yawOverride = st.yaw;          // 使用自由视角
    opt.pitch = st.pitch;
    opt.bg = 0xFFFFFFFFu;
    opt.supersample = st.rotating ? 1 : 2;   // 拖动中降超采样保证跟手，松手后恢复

    render::Image img;
    std::string err;
    ULONGLONG t0 = GetTickCount64();
    if (render::RenderSchematic(*st.cur, st.db, st.curVersionIdx, opt, img, err)) {
        st.preview = std::move(img);
        st.previewFailed = false;
    } else {
        st.previewFailed = true;
        st.previewErr = Utf8ToWide(err);
        st.preview = render::Image();
    }
    st.previewMs = (double)(GetTickCount64() - t0);
    st.previewDirty = false;
    SetStatus(Fmt(i18n::T("status.rendered"), st.previewMs));
}

// ------------------------------------------------------------------ 保存（带备份）
static std::wstring g_lastSaveInfo;

std::wstring LastSaveInfo() { return g_lastSaveInfo; }
void SetLastSaveInfo(const std::wstring& v) { g_lastSaveInfo = v; }

static std::set<std::wstring> g_metaBackedUp;

// 备注/描述这类元数据编辑：每个文件在本次会话里只备份一次，
// 既避免频繁输入产生大量备份，又保留一层安全网。
void SaveCurrentMetaOnce(const std::wstring& opName) {
    std::wstring pth = g_state.cur ? g_state.cur->Path() : std::wstring();
    bool first = !pth.empty() && g_metaBackedUp.find(pth) == g_metaBackedUp.end();
    if (first) g_metaBackedUp.insert(pth);
    if (first) SaveCurrentWithBackup(opName);
    else       SaveCurrentWithBackup(opName, false);
}

void SaveCurrentWithBackup(const std::wstring& opName, bool allowBackup) {
    g_lastSaveInfo.clear();
    State& st = Get();
    if (!st.cur) return;
    ltm::Summary s = st.cur->GetSummary();
    // 优先用 Schematic 自身记录的真实路径；Summary 里的 path 不保证被填充，
    // 一旦为空会导致 FileExists 判定为假、备份被静默跳过。
    std::wstring orig = st.cur->Path();
    if (orig.empty()) orig = s.path;
    if (orig.empty() && st.listSel >= 0 && (size_t)st.listSel < st.files.size())
        orig = st.files[(size_t)st.listSel].sum.path;      // 第三来源：扫描时记录的路径
    if (orig.empty()) {
        g_lastSaveInfo = L"\u4fdd\u5b58\u5931\u8d25\uff1a\u65e0\u6cd5\u786e\u5b9a\u6295\u5f71\u6587\u4ef6\u8def\u5f84";
        SetStatus(g_lastSaveInfo);
        return;
    }

    fsx::DiagLog("SaveWithBackup 操作=" + WideToUtf8(opName) +
                 " 路径=" + WideToUtf8(orig) +
                 " 允许备份=" + std::to_string((int)allowBackup) +
                 " 文件存在=" + std::to_string((int)fsx::FileExists(orig)));
    std::wstring backupPath;
    if (allowBackup && fsx::FileExists(orig)) {
        std::string err;
        if (!backup::CreateBackup(orig, opName, backupPath, err)) {
            g_lastSaveInfo = L"\u5907\u4efd\u5931\u8d25\uff0c\u5df2\u4e2d\u6b62\u4fdd\u5b58: " + Utf8ToWide(err);
            SetStatus(g_lastSaveInfo);
            return;
        }
    } else {
        g_lastSaveInfo = L"\u6ce8\u610f\uff1a\u6e90\u6587\u4ef6\u4e0d\u5b58\u5728\uff0c\u672a\u751f\u6210\u5907\u4efd\uff08" + orig + L"\uff09";
    }
    std::string err;
    if (!st.cur->SaveTo(orig, err)) {
        g_lastSaveInfo = L"\u4fdd\u5b58\u5931\u8d25: " + Utf8ToWide(err);
        SetStatus(g_lastSaveInfo);
        return;
    }
    backup::Log(orig, opName, backupPath.empty() ? L"saved-no-backup" : L"saved");
    backup::PrunePerFile(50);
    if (!backupPath.empty()) {
        g_lastSaveInfo = L"\u5907\u4efd\uff1a" + backupPath;
        SetStatus(L"\u5df2\u4fdd\u5b58\uff0c" + g_lastSaveInfo);
    } else if (g_lastSaveInfo.empty()) {
        g_lastSaveInfo = L"\u672a\u751f\u6210\u5907\u4efd";
    }

    // 更新列表中的摘要
    for (FileItem& it : st.files) {
        if (it.sum.path == orig) {
            ltm::Schematic::Peek(orig, it.sum);
            std::vector<u8> raw;
            if (fsx::ReadAllBytes(orig, raw)) it.contentHash = HashBytes(raw);
            break;
        }
    }
    st.cur.reset(new ltm::Schematic());
    st.cur->Load(orig, err);
    st.curAnalysis = st.cur->Analyze();
    st.previewDirty = true;
    RenderPreview();
    SetStatus(i18n::T("status.saved"));
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

// ------------------------------------------------------------------ 重复筛查
void RunDuplicateScan() {
    State& st = Get();
    st.dupGroups.clear();
    st.dupSel = -1;
    st.dupScroll = 0;
    for (FileItem& it : st.files) it.dupGroup = -1;

    std::map<std::wstring, std::vector<int>> buckets;
    for (size_t i = 0; i < st.files.size(); ++i) {
        FileItem& it = st.files[i];
        std::wstring key;
        if (st.dupMode == 0) {
            key = L"H" + std::to_wstring(it.contentHash);
        } else {
            wchar_t buf[256];
            swprintf(buf, 256, L"M|%s|%d|%d|%d|%lld",
                     it.sum.name.c_str(), it.sum.enclosing[0], it.sum.enclosing[1],
                     it.sum.enclosing[2], (long long)it.sum.totalBlocks);
            key = buf;
        }
        buckets[key].push_back((int)i);
    }
    for (auto& kv : buckets) {
        if (kv.second.size() < 2) continue;
        int gi = (int)st.dupGroups.size();
        for (int idx : kv.second) st.files[(size_t)idx].dupGroup = gi;
        st.dupGroups.push_back(kv.second);
    }
}

// ------------------------------------------------------------------ 批量替换
// 返回值：被改动的文件数
int ApplyReplaceToFiles(const std::vector<int>& indices, const ltm::ReplaceRule& rule,
                        u64& totalBlocks) {
    State& st = Get();
    totalBlocks = 0;
    int changedFiles = 0;
    for (int idx : indices) {
        if (idx < 0 || (size_t)idx >= st.files.size()) continue;
        const std::wstring& path = st.files[(size_t)idx].sum.path;
        ltm::Schematic sc;
        std::string err;
        if (!sc.Load(path, err)) continue;
        int vIdx = st.dbLoaded ? st.db.PickVersion(sc.GetSummary().dataVersion) : -1;
        if (st.dbLoaded) st.db.SetActiveVersion(vIdx);
        u64 n = sc.ReplaceBlocks(rule, st.dbLoaded ? &st.db : nullptr, 0);
        if (n == 0) continue;

        std::wstring bp;
        std::string berr;
        if (!backup::CreateBackup(path, i18n::T("log.op.replace"), bp, berr)) continue;
        if (!sc.SaveTo(path, err)) continue;
        backup::Log(path, i18n::T("log.op.replace"),
                    L"replace " + Utf8ToWide(rule.fromName) + L" -> " + Utf8ToWide(rule.toName));
        totalBlocks += n;
        ++changedFiles;
        ltm::Schematic::Peek(path, st.files[(size_t)idx].sum);
        std::vector<u8> raw;
        if (fsx::ReadAllBytes(path, raw)) st.files[(size_t)idx].contentHash = HashBytes(raw);
    }
    backup::PrunePerFile(50);
    return changedFiles;
}

bool IsScanning() { return g_scanRunning != 0; }

} // namespace app
