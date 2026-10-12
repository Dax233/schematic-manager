#include "packmgr.h"
#include "zip.h"
#include "json.h"
#include "fs.h"
#include "str.h"
#include "i18n.h"

namespace packmgr {

// ------------------------------------------------------------------ 路径
std::wstring DataDir() {
    return fsx::JoinPath(fsx::GetDirectoryName(fsx::GetExePath()), L"packs");
}
std::wstring PacksDir() {
    std::wstring d = DataDir();
    fsx::EnsureDir(d);
    return d;
}
std::wstring ScannedJarsDir() {
    std::wstring d = fsx::JoinPath(PacksDir(), L"sources");
    fsx::EnsureDir(d);
    return d;
}

static std::wstring ManifestPath() { return fsx::JoinPath(PacksDir(), L"packs.tsv"); }
static std::wstring SettingsPath() { return fsx::JoinPath(PacksDir(), L"settings.tsv"); }

// ------------------------------------------------------------------ 小工具
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
static std::vector<std::string> SplitTab(const std::string& line) {
    std::vector<std::string> f;
    size_t p = 0;
    while (p <= line.size()) {
        size_t t = line.find('\t', p);
        f.push_back((t == std::string::npos) ? line.substr(p) : line.substr(p, t - p));
        if (t == std::string::npos) break;
        p = t + 1;
    }
    return f;
}

// ------------------------------------------------------------------ 清单
std::vector<ManifestEntry> LoadManifest() {
    std::vector<ManifestEntry> out;
    std::vector<u8> raw;
    if (!fsx::ReadAllBytes(ManifestPath(), raw)) return out;
    std::string txt((const char*)raw.data(), raw.size());
    size_t pos = 0;
    while (pos < txt.size()) {
        size_t nl = txt.find('\n', pos);
        std::string line = (nl == std::string::npos) ? txt.substr(pos) : txt.substr(pos, nl - pos);
        pos = (nl == std::string::npos) ? txt.size() : nl + 1;
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.empty() || line[0] == '#') continue;
        auto f = SplitTab(line);
        if (f.size() < 6) continue;
        ManifestEntry e;
        e.enabled = (f[0] == "1");
        e.file = Utf8ToWide(Unesc(f[1]));
        e.name = Unesc(f[2]);
        e.source = Unesc(f[3]);
        e.kind = atoi(f[4].c_str());
        e.dataVersion = (i32)strtol(f[5].c_str(), nullptr, 10);
        out.push_back(e);
    }
    return out;
}

bool SaveManifest(const std::vector<ManifestEntry>& list) {
    std::string txt = "# enabled\tfile\tname\tsource\tkind(1=MC,2=mod)\tdataVersion\n";
    for (const auto& e : list) {
        char buf[64];
        snprintf(buf, sizeof(buf), "%d\t", e.enabled ? 1 : 0);
        txt += buf;
        txt += Esc(WideToUtf8(e.file)); txt += '\t';
        txt += Esc(e.name);            txt += '\t';
        txt += Esc(e.source);          txt += '\t';
        snprintf(buf, sizeof(buf), "%d\t", e.kind);
        txt += buf;
        snprintf(buf, sizeof(buf), "%d\n", (int)e.dataVersion);
        txt += buf;
    }
    return fsx::WriteAllBytes(ManifestPath(), txt.data(), txt.size());
}

std::set<i32> ImportedDataVersions() {
    std::set<i32> s;
    for (const auto& e : LoadManifest()) s.insert(e.dataVersion);
    return s;
}

// ------------------------------------------------------------------ 设置
static std::vector<std::pair<std::string, std::string>> LoadSettings() {
    std::vector<std::pair<std::string, std::string>> out;
    std::vector<u8> raw;
    if (!fsx::ReadAllBytes(SettingsPath(), raw)) return out;
    std::string txt((const char*)raw.data(), raw.size());
    size_t pos = 0;
    while (pos < txt.size()) {
        size_t nl = txt.find('\n', pos);
        std::string line = (nl == std::string::npos) ? txt.substr(pos) : txt.substr(pos, nl - pos);
        pos = (nl == std::string::npos) ? txt.size() : nl + 1;
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.empty() || line[0] == '#') continue;
        auto f = SplitTab(line);
        if (f.size() >= 2) out.emplace_back(Unesc(f[0]), Unesc(f[1]));
    }
    return out;
}
static void SaveSettings(const std::vector<std::pair<std::string, std::string>>& kv) {
    std::string txt;
    for (const auto& p : kv) { txt += Esc(p.first); txt += '\t'; txt += Esc(p.second); txt += '\n'; }
    fsx::WriteAllBytes(SettingsPath(), txt.data(), txt.size());
}

std::vector<std::wstring> LoadScanRoots() {
    std::vector<std::wstring> out;
    for (const auto& kv : LoadSettings())
        if (kv.first == "scanroot") out.push_back(Utf8ToWide(kv.second));
    return out;
}
bool SaveScanRoots(const std::vector<std::wstring>& roots) {
    auto kv = LoadSettings();
    kv.erase(std::remove_if(kv.begin(), kv.end(),
             [](const std::pair<std::string, std::string>& p) { return p.first == "scanroot"; }), kv.end());
    for (const auto& r : roots) kv.emplace_back("scanroot", WideToUtf8(r));
    SaveSettings(kv);
    return true;
}
std::vector<DirEntry> LoadDirs() {
    std::vector<DirEntry> out;
    for (const auto& kv : LoadSettings()) {
        if (kv.first != "dir") continue;
        DirEntry e;
        if (kv.second.size() > 2 && kv.second[1] == '|') {
            e.enabled = (kv.second[0] == '1');
            e.path = Utf8ToWide(kv.second.substr(2));
        } else {
            e.enabled = true;
            e.path = Utf8ToWide(kv.second);
        }
        out.push_back(e);
    }
    return out;
}
void SaveDirs(const std::vector<DirEntry>& dirs) {
    auto kv = LoadSettings();
    kv.erase(std::remove_if(kv.begin(), kv.end(),
             [](const std::pair<std::string, std::string>& p) { return p.first == "dir"; }), kv.end());
    for (const auto& d : dirs)
        kv.emplace_back("dir", std::string(d.enabled ? "1|" : "0|") + WideToUtf8(d.path));
    SaveSettings(kv);
}
std::wstring LoadLastDir() {
    for (const auto& kv : LoadSettings()) if (kv.first == "lastdir") return Utf8ToWide(kv.second);
    return std::wstring();
}
void SaveLastDir(const std::wstring& dir) {
    auto kv = LoadSettings();
    kv.erase(std::remove_if(kv.begin(), kv.end(),
             [](const std::pair<std::string, std::string>& p) { return p.first == "lastdir"; }), kv.end());
    if (!dir.empty()) kv.emplace_back("lastdir", WideToUtf8(dir));
    SaveSettings(kv);
}

int LoadExportSize() {
    for (const auto& kv : LoadSettings())
        if (kv.first == "exportsize") {
            int v = atoi(kv.second.c_str());
            if (v == 512 || v == 1024 || v == 2048 || v == 3072) return v;
        }
    return 512;
}
void SaveExportSize(int v) {
    auto kv = LoadSettings();
    kv.erase(std::remove_if(kv.begin(), kv.end(),
             [](const std::pair<std::string, std::string>& p) { return p.first == "exportsize"; }), kv.end());
    kv.emplace_back("exportsize", std::to_string(v));
    SaveSettings(kv);
}

float LoadBrightness() {
    for (const auto& kv : LoadSettings())
        if (kv.first == "brightness") {
            float v = (float)atof(kv.second.c_str());
            if (v >= 0.5f && v <= 2.0f) return v;
        }
    return 1.45f;                     // 默认 145%
}
void SaveBrightness(float b) {
    auto kv = LoadSettings();
    kv.erase(std::remove_if(kv.begin(), kv.end(),
             [](const std::pair<std::string, std::string>& p) { return p.first == "brightness"; }), kv.end());
    char buf[32]; snprintf(buf, sizeof(buf), "%.3f", b);
    kv.emplace_back("brightness", buf);
    SaveSettings(kv);
}

std::wstring LoadLastExportDir() {
    for (const auto& kv : LoadSettings()) if (kv.first == "exportdir") return Utf8ToWide(kv.second);
    return std::wstring();
}
void SaveLastExportDir(const std::wstring& dir) {
    auto kv = LoadSettings();
    kv.erase(std::remove_if(kv.begin(), kv.end(),
             [](const std::pair<std::string, std::string>& p) { return p.first == "exportdir"; }), kv.end());
    if (!dir.empty()) kv.emplace_back("exportdir", WideToUtf8(dir));
    SaveSettings(kv);
}

std::vector<std::wstring> LoadRecent() {
    std::vector<std::wstring> out;
    for (const auto& kv : LoadSettings())
        if (kv.first == "recent") out.push_back(Utf8ToWide(kv.second));
    return out;
}
void SaveRecent(const std::vector<std::wstring>& recent) {
    auto kv = LoadSettings();
    kv.erase(std::remove_if(kv.begin(), kv.end(),
             [](const std::pair<std::string, std::string>& p) { return p.first == "recent"; }), kv.end());
    for (const auto& r : recent) kv.emplace_back("recent", WideToUtf8(r));
    SaveSettings(kv);
}

bool AutoImportEnabled() {
    for (const auto& kv : LoadSettings())
        if (kv.first == "autoimport") return kv.second == "1";
    return true;   // 默认开启
}
void SetAutoImportEnabled(bool v) {
    auto kv = LoadSettings();
    kv.erase(std::remove_if(kv.begin(), kv.end(),
             [](const std::pair<std::string, std::string>& p) { return p.first == "autoimport"; }), kv.end());
    kv.emplace_back("autoimport", v ? "1" : "0");
    SaveSettings(kv);
}

// ------------------------------------------------------------------ 安装探测
static bool LooksLikeMcDir(const std::wstring& dir) {
    return fsx::DirExists(fsx::JoinPath(dir, L"versions"));
}

static void ScanRec(const std::wstring& dir, int depth, int maxDepth,
                    std::vector<std::wstring>& found, int& visited) {
    if (depth > maxDepth || visited > 4000) return;
    WIN32_FIND_DATAW fd;
    HANDLE h = FindFirstFileW(fsx::JoinPath(dir, L"*").c_str(), &fd);
    if (h == INVALID_HANDLE_VALUE) return;
    do {
        if (!(fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY)) continue;
        if (wcscmp(fd.cFileName, L".") == 0 || wcscmp(fd.cFileName, L"..") == 0) continue;
        if (fd.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) continue;
        std::wstring full = fsx::JoinPath(dir, fd.cFileName);
        ++visited;
        if (wcscmp(fd.cFileName, L".minecraft") == 0) {
            if (LooksLikeMcDir(full)) found.push_back(full);
            continue;                     // 不再深入 .minecraft 内部
        }
        if (fd.dwFileAttributes & FILE_ATTRIBUTE_HIDDEN) continue;
        ScanRec(full, depth + 1, maxDepth, found, visited);
    } while (FindNextFileW(h, &fd) && visited <= 4000);
    FindClose(h);
}

void ScanInstalls(std::vector<Install>& out, const std::vector<std::wstring>& extraRoots, int maxDepth) {
    std::vector<std::wstring> candidates;

    // %APPDATA%\.minecraft
    wchar_t buf[MAX_PATH * 2];
    if (GetEnvironmentVariableW(L"APPDATA", buf, MAX_PATH * 2)) {
        std::wstring p = fsx::JoinPath(buf, L".minecraft");
        if (LooksLikeMcDir(p)) candidates.push_back(p);
    }
    std::vector<std::wstring> deepRoots;   // 需要深度扫描的游戏目录
    // 各盘符根目录 + 少量常见子目录
    DWORD drives = GetLogicalDrives();
    for (int i = 0; i < 26; ++i) {
        if (!(drives & (1u << i))) continue;
        wchar_t root[8];
        swprintf(root, 8, L"%c:\\", (wchar_t)(L'A' + i));
        UINT t = GetDriveTypeW(root);
        if (t != DRIVE_FIXED && t != DRIVE_REMOVABLE) continue;
        std::wstring r(root);
        if (LooksLikeMcDir(fsx::JoinPath(r, L".minecraft")))
            candidates.push_back(fsx::JoinPath(r, L".minecraft"));
        // 常见游戏目录名
        const wchar_t* names[] = { L"games", L"Games", L"game", L"\u6e38\u620f", L"Minecraft",
                                   L"minecraft", L"\u6e38\u620f\\Minecraft" };
        for (const wchar_t* n : names) {
            std::wstring sub = fsx::JoinPath(r, n);
            if (!fsx::DirExists(sub)) continue;
            if (LooksLikeMcDir(sub)) candidates.push_back(sub);
            else deepRoots.push_back(sub);       // 例如 E:\游戏 —— 游戏装在更深层
        }
    }
    for (const auto& r : extraRoots) {
        if (!fsx::DirExists(r)) continue;
        if (LooksLikeMcDir(r)) candidates.push_back(r);
    }
    // 深度扫描（常见游戏目录 + 用户指定的根）
    for (const auto& r : deepRoots) {
        int visited = 0;
        std::vector<std::wstring> f;
        ScanRec(r, 0, maxDepth, f, visited);
        for (auto& x : f) candidates.push_back(x);
    }
    for (const auto& r : extraRoots) {
        int visited = 0;
        std::vector<std::wstring> f;
        ScanRec(r, 0, maxDepth, f, visited);
        for (auto& x : f) candidates.push_back(x);
    }

    std::sort(candidates.begin(), candidates.end());
    candidates.erase(std::unique(candidates.begin(), candidates.end()), candidates.end());

    for (const std::wstring& mc : candidates) {
        std::wstring vdir = fsx::JoinPath(mc, L"versions");
        std::vector<FileInfo> subs;
        fsx::ListDir(vdir, subs, true);
        for (const FileInfo& s : subs) {
            // versions/<id>/<id>.jar
            std::vector<FileInfo> fs2;
            fsx::ListDir(s.path, fs2, false);
            for (const FileInfo& f : fs2) {
                if (!EndsWithNoCase(f.name, L".jar")) continue;
                if (f.name.find(L"sources") != std::wstring::npos) continue;
                if (f.size < 500000) continue;

                Install ins;
                ins.mcDir = mc;
                ins.versionId = WideToUtf8(s.name);
                ins.jarPath = f.path;
                ins.jarSize = f.size;
                ins.type = "release";

                // 读 version.json（jar 内），老版本可能没有
                zip::Archive z;
                std::string err;
                if (z.Open(f.path, err)) {
                    std::string txt;
                    if (z.ReadText("version.json", txt, err)) {
                        Json j = JsonParse(txt);
                        if (const Json* id = j.find("id")) ins.versionId = id->getStr();
                        if (const Json* wv = j.find("world_version")) ins.dataVersion = (i32)wv->getInt(0);
                    }
                    std::string n2 = z.Has("assets/minecraft/blockstates/stone.json") ? "" : "";
                    (void)n2;
                }
                // 读 launcher profile 的 type
                std::wstring prof = fsx::JoinPath(s.path, s.name + L".json");
                std::vector<u8> praw;
                if (fsx::ReadAllBytes(prof, praw)) {
                    Json pj = JsonParse((const char*)praw.data(), praw.size());
                    if (const Json* t = pj.find("type")) ins.type = t->getStr("release");
                    if (const Json* i2 = pj.find("id")) if (ins.versionId.empty()) ins.versionId = i2->getStr();
                }
                out.push_back(std::move(ins));
            }
        }
    }

    // 去重（同一 dataVersion 只保留第一个）
    std::sort(out.begin(), out.end(), [](const Install& a, const Install& b) {
        if (a.dataVersion != b.dataVersion) return a.dataVersion < b.dataVersion;
        return a.versionId < b.versionId;
    });
    std::vector<Install> uniq;
    std::set<i32> seen;
    for (auto& i : out) {
        if (i.dataVersion > 0) {
            if (seen.count(i.dataVersion)) continue;
            seen.insert(i.dataVersion);
        }
        uniq.push_back(i);
    }
    out.swap(uniq);
}

void FilterReleases(const std::vector<Install>& in, std::vector<Install>& out) {
    for (const auto& i : in) {
        if (i.type == "release") out.push_back(i);
    }
}

// ------------------------------------------------------------------ 导入
bool ImportJar(const std::wstring& jarPath, int kind, i32 dataVersion,
               const std::string& name, std::string& err, packbuild::Result* info) {
    packbuild::Result res;
    if (!packbuild::BuildFromJar(jarPath, name, kind, dataVersion, res)) {
        err = res.error.empty() ? "转换失败" : res.error;
        return false;
    }
    // 文件名：<kind>_<id>_<hash>.pmp
    std::string base = name;
    for (char& c : base) {
        if (c == '\\' || c == '/' || c == ':' || c == '*' || c == '?' ||
            c == '"' || c == '<' || c == '>' || c == '|' || c == ' ') c = '_';
    }
    u64 hh = 1469598103934665603ULL;
    for (u8 b : res.data) { hh ^= b; hh *= 1099511628211ULL; }
    char hb[32];
    snprintf(hb, sizeof(hb), "%08x", (unsigned)(hh & 0xFFFFFFFFu));
    std::string fn = (kind == 2 ? "mod_" : "mc_") + base + "_" + hb + ".pmp";

    std::wstring path = fsx::JoinPath(PacksDir(), Utf8ToWide(fn));
    if (!fsx::WriteAllBytes(path, res.data)) { err = "写入资源包文件失败"; return false; }

    // 清单
    auto list = LoadManifest();
    bool found = false;
    for (auto& e : list) {
        if (e.name == name) { e.enabled = true; e.file = Utf8ToWide(fn); e.dataVersion = res.dataVersion;
                              e.kind = kind; e.source = WideToUtf8(jarPath); found = true; break; }
    }
    if (!found) {
        ManifestEntry e;
        e.enabled = true;
        e.file = Utf8ToWide(fn);
        e.name = name;
        e.source = WideToUtf8(jarPath);
        e.kind = kind;
        e.dataVersion = res.dataVersion;
        list.push_back(e);
    }
    SaveManifest(list);

    if (info) *info = res;
    return true;
}

bool RemovePack(const std::string& name, bool deleteFile) {
    auto list = LoadManifest();
    for (size_t i = 0; i < list.size(); ++i) {
        if (list[i].name != name) continue;
        if (deleteFile) {
            std::wstring p = fsx::JoinPath(PacksDir(), list[i].file);
            fsx::DeleteFileAt(p);
        }
        list.erase(list.begin() + (ptrdiff_t)i);
        return SaveManifest(list);
    }
    return false;
}

} // namespace packmgr
