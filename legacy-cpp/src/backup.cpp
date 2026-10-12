#include "backup.h"
#include "fs.h"
#include "str.h"
#include <ctime>

namespace backup {

static std::wstring g_base;

static std::wstring IndexPath() { return fsx::JoinPath(g_base, L"index.tsv"); }
static std::wstring LogPath()   { return fsx::JoinPath(g_base, L"operations.log"); }

// 惰性初始化：所有对外函数都必须先调用它。
// 之前 List() 漏了这一步，导致"启动后直接打开备份管理"时 g_base 还是空的，
// IndexPath() 退化成相对路径，读不到索引，界面显示为空。
static void EnsureInit() { if (g_base.empty()) Init(); }

void Init() {
    g_base = fsx::JoinPath(fsx::GetDirectoryName(fsx::GetExePath()), L"backups");
    fsx::EnsureDir(g_base);
}

const std::wstring& BaseDir() { EnsureInit(); return g_base; }
std::wstring BackupRoot() { EnsureInit(); return g_base; }

static std::wstring NowStamp() {
    SYSTEMTIME st;
    GetLocalTime(&st);
    wchar_t buf[64];
    swprintf(buf, 64, L"%04d%02d%02d_%02d%02d%02d_%03d",
             st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond, st.wMilliseconds);
    return buf;
}

static i64 NowMs() {
    FILETIME ft;
    GetSystemTimeAsFileTime(&ft);
    ULARGE_INTEGER u; u.LowPart = ft.dwLowDateTime; u.HighPart = ft.dwHighDateTime;
    return (i64)(u.QuadPart / 10000ULL - 11644473600000ULL);
}

// 转义 TSV 中的制表符与换行
static std::string Esc(const std::string& s) {
    std::string o;
    for (char c : s) {
        if (c == '\t') o += "\\t";
        else if (c == '\n') o += "\\n";
        else if (c == '\\') o += "\\\\";
        else o += c;
    }
    return o;
}
static std::string Unesc(const std::string& s) {
    std::string o;
    for (size_t i = 0; i < s.size(); ++i) {
        if (s[i] == '\\' && i + 1 < s.size()) {
            char n = s[++i];
            if (n == 't') o += '\t';
            else if (n == 'n') o += '\n';
            else o += n;
        } else o += s[i];
    }
    return o;
}

static void AppendIndex(const Entry& e) {
    fsx::DiagLog("AppendIndex -> " + WideToUtf8(IndexPath()) +
                 " backup=" + WideToUtf8(e.backupPath));
    // 索引里必须存"相对 g_base 的完整路径"（含 <投影名>\ 子目录）。
    // 以前只存文件名，List() 用它直接拼 g_base 会少一层目录，导致 FileExists 为假、
    // 条目全部被过滤 —— 表现为"备份了但备份管理看不到"。
    std::wstring rel = e.backupPath;
    if (rel.size() > g_base.size() && rel.compare(0, g_base.size(), g_base) == 0) {
        rel = rel.substr(g_base.size());
        while (!rel.empty() && (rel[0] == L'\\' || rel[0] == L'/')) rel.erase(rel.begin());
    }
    std::string line = Esc(WideToUtf8(rel)) + "\t" +
                       Esc(WideToUtf8(e.origPath)) + "\t" +
                       Esc(WideToUtf8(e.op)) + "\t" +
                       std::to_string(e.timeMs) + "\t" +
                       std::to_string(e.size) + "\n";
    HANDLE h = CreateFileW(IndexPath().c_str(), FILE_APPEND_DATA, FILE_SHARE_READ, nullptr,
                           OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h != INVALID_HANDLE_VALUE) {
        DWORD wrote = 0;
        WriteFile(h, line.data(), (DWORD)line.size(), &wrote, nullptr);
        CloseHandle(h);
    }
}

std::vector<Entry> List() {
    EnsureInit();
    std::vector<Entry> out;
    std::vector<u8> raw;
    if (!fsx::ReadAllBytes(IndexPath(), raw)) return out;
    std::string txt((const char*)raw.data(), raw.size());
    size_t pos = 0;
    while (pos < txt.size()) {
        size_t nl = txt.find('\n', pos);
        std::string line = (nl == std::string::npos) ? txt.substr(pos) : txt.substr(pos, nl - pos);
        pos = (nl == std::string::npos) ? txt.size() : nl + 1;
        if (line.empty()) continue;
        std::vector<std::string> f;
        size_t p2 = 0;
        while (p2 <= line.size()) {
            size_t t = line.find('\t', p2);
            f.push_back((t == std::string::npos) ? line.substr(p2) : line.substr(p2, t - p2));
            if (t == std::string::npos) break;
            p2 = t + 1;
        }
        if (f.size() < 5) continue;
        Entry e;
        e.backupPath = fsx::JoinPath(g_base, Utf8ToWide(Unesc(f[0])));
        // 兼容历史索引（只存了文件名）：在 backups 下的一层子目录里找同名文件
        if (!fsx::FileExists(e.backupPath)) {
            std::wstring fn = fsx::GetFileName(e.backupPath);
            std::vector<FileInfo> subs;
            fsx::ListDir(g_base, subs, true);
            for (const auto& sd : subs) {
                std::wstring cand = fsx::JoinPath(sd.path, fn);
                if (fsx::FileExists(cand)) { e.backupPath = cand; break; }
            }
        }
        e.origPath = Utf8ToWide(Unesc(f[1]));
        e.origName = fsx::GetFileName(e.origPath);
        e.op = Utf8ToWide(Unesc(f[2]));
        e.timeMs = _strtoi64(f[3].c_str(), nullptr, 10);
        e.size = (u64)_strtoi64(f[4].c_str(), nullptr, 10);
        if (fsx::FileExists(e.backupPath)) out.push_back(e);
    }
    fsx::DiagLog("Backup::List index=" + WideToUtf8(IndexPath()) +
                 " 索引存在=" + std::to_string((int)fsx::FileExists(IndexPath())) +
                 " 有效条目=" + std::to_string(out.size()));
    std::sort(out.begin(), out.end(), [](const Entry& a, const Entry& b) { return a.timeMs > b.timeMs; });
    return out;
}

bool CreateBackup(const std::wstring& srcFile, const std::wstring& op,
                  std::wstring& outPath, std::string& err) {
    EnsureInit();
    fsx::DiagLog("CreateBackup 源=" + WideToUtf8(srcFile) + " 操作=" + WideToUtf8(op) +
                 " 目录=" + WideToUtf8(g_base) +
                 " 源存在=" + std::to_string((int)fsx::FileExists(srcFile)));
    if (!fsx::FileExists(srcFile)) { err = "源文件不存在"; return false; }

    std::wstring stem = fsx::GetFileStem(srcFile);
    std::wstring dir = fsx::JoinPath(g_base, stem);
    fsx::EnsureDir(dir);

    std::wstring name = stem + L"__" + NowStamp() + L"__" + op + L".litematic";
    outPath = fsx::JoinPath(dir, name);
    int suffix = 1;
    while (fsx::FileExists(outPath)) {
        wchar_t buf[32];
        swprintf(buf, 32, L"(%d)", suffix++);
        outPath = fsx::JoinPath(dir, stem + L"__" + NowStamp() + buf + L"__" + op + L".litematic");
    }
    if (!fsx::EnsureDir(dir)) fsx::DiagLog("CreateBackup 警告：建目录失败 " + WideToUtf8(dir));
    fsx::DiagLog("CreateBackup 目标=" + WideToUtf8(outPath));
    if (!fsx::CopyFileTo(srcFile, outPath, false)) {
        err = "备份复制失败";
        fsx::DiagLog("CreateBackup 失败：复制失败 -> " + WideToUtf8(outPath));
        return false;
    }
    if (!fsx::FileExists(outPath)) fsx::DiagLog("CreateBackup 异常：复制后文件仍不存在");
    else fsx::DiagLog("CreateBackup 成功");

    Entry e;
    e.backupPath = outPath;
    e.origPath = srcFile;
    e.op = op;
    e.timeMs = NowMs();
    std::vector<u8> tmp;
    if (fsx::ReadAllBytes(outPath, tmp)) e.size = tmp.size();
    AppendIndex(e);
    return true;
}

bool Restore(const Entry& e, std::string& err) {
    EnsureInit();
    if (!fsx::FileExists(e.backupPath)) { err = "备份文件已不存在"; return false; }
    // 先备份当前文件
    if (fsx::FileExists(e.origPath)) {
        std::wstring bp;
        std::string be;
        CreateBackup(e.origPath, L"restore-before", bp, be);
    }
    if (!fsx::CopyFileTo(e.backupPath, e.origPath, true)) { err = "还原失败"; return false; }
    return true;
}

bool DeleteBackup(const Entry& e, std::string& err) {
    EnsureInit();
    if (!fsx::DeleteFileAt(e.backupPath)) { err = "删除失败"; return false; }
    return true;
}

int PrunePerFile(int keep) {
    std::vector<Entry> all = List();
    std::map<std::wstring, std::vector<Entry>> byOrig;
    for (const Entry& e : all) byOrig[e.origPath].push_back(e);
    int removed = 0;
    for (auto& kv : byOrig) {
        auto& v = kv.second;
        if ((int)v.size() <= keep) continue;
        for (size_t i = (size_t)keep; i < v.size(); ++i) {
            if (fsx::DeleteFileAt(v[i].backupPath)) ++removed;
        }
    }
    return removed;
}

void Log(const std::wstring& file, const std::wstring& op, const std::wstring& detail) {
    EnsureInit();
    SYSTEMTIME st;
    GetLocalTime(&st);
    wchar_t ts[64];
    swprintf(ts, 64, L"%04d-%02d-%02d %02d:%02d:%02d", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond);
    std::wstring line = std::wstring(ts) + L"\t" + file + L"\t" + op + L"\t" + detail + L"\r\n";
    std::string u8 = WideToUtf8(line);
    HANDLE h = CreateFileW(LogPath().c_str(), FILE_APPEND_DATA, FILE_SHARE_READ, nullptr,
                           OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h != INVALID_HANDLE_VALUE) {
        DWORD wrote = 0;
        WriteFile(h, u8.data(), (DWORD)u8.size(), &wrote, nullptr);
        CloseHandle(h);
    }
}

std::vector<std::wstring> ReadLog(int maxLines) {
    EnsureInit();
    std::vector<std::wstring> out;
    std::vector<u8> raw;
    if (!fsx::ReadAllBytes(LogPath(), raw)) return out;
    std::wstring txt = Utf8ToWide(std::string((const char*)raw.data(), raw.size()));
    for (const std::wstring& l : SplitW(txt, L'\n')) {
        std::wstring t = TrimW(l);
        if (!t.empty()) out.push_back(t);
    }
    if (maxLines > 0 && (int)out.size() > maxLines)
        out.erase(out.begin(), out.end() - maxLines);
    std::reverse(out.begin(), out.end());
    return out;
}

} // namespace backup
