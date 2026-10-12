#include "fs.h"
#include "str.h"

namespace fsx {

bool FileExists(const std::wstring& path) {
    DWORD a = GetFileAttributesW(path.c_str());
    return a != INVALID_FILE_ATTRIBUTES && !(a & FILE_ATTRIBUTE_DIRECTORY);
}

bool DirExists(const std::wstring& path) {
    DWORD a = GetFileAttributesW(path.c_str());
    return a != INVALID_FILE_ATTRIBUTES && (a & FILE_ATTRIBUTE_DIRECTORY);
}

bool EnsureDir(const std::wstring& path) {
    if (path.empty()) return false;
    if (DirExists(path)) return true;
    std::wstring parent = GetDirectoryName(path);
    if (!parent.empty() && parent != path && !DirExists(parent)) EnsureDir(parent);
    if (CreateDirectoryW(path.c_str(), nullptr)) return true;
    return DirExists(path);
}

bool EnsureDirForFile(const std::wstring& filePath) {
    return EnsureDir(GetDirectoryName(filePath));
}

std::wstring GetExePath() {
    wchar_t buf[MAX_PATH * 2];
    DWORD n = GetModuleFileNameW(nullptr, buf, MAX_PATH * 2);
    return std::wstring(buf, n);
}

std::wstring GetFileName(const std::wstring& path) {
    size_t p = path.find_last_of(L"\\/");
    return (p == std::wstring::npos) ? path : path.substr(p + 1);
}

std::wstring GetFileStem(const std::wstring& path) {
    std::wstring name = GetFileName(path);
    size_t p = name.find_last_of(L'.');
    return (p == std::wstring::npos) ? name : name.substr(0, p);
}

std::wstring GetDirectoryName(const std::wstring& path) {
    size_t p = path.find_last_of(L"\\/");
    if (p == std::wstring::npos) return std::wstring();
    if (p == 2 && path.size() > 2 && path[1] == L':') return path.substr(0, 3);
    return path.substr(0, p);
}

std::wstring JoinPath(const std::wstring& a, const std::wstring& b) {
    if (a.empty()) return b;
    if (b.empty()) return a;
    wchar_t last = a[a.size() - 1];
    if (last == L'\\' || last == L'/') return a + b;
    return a + L"\\" + b;
}

std::wstring GetExtensionLower(const std::wstring& path) {
    std::wstring name = GetFileName(path);
    size_t p = name.find_last_of(L'.');
    if (p == std::wstring::npos) return std::wstring();
    return ToLowerW(name.substr(p));
}

static void ListRec(const std::wstring& dir, const std::wstring& extLower,
                    std::vector<FileInfo>& out, int depth, int maxDepth) {
    if (depth > maxDepth) return;
    std::wstring pattern = JoinPath(dir, L"*");
    WIN32_FIND_DATAW fd;
    HANDLE h = FindFirstFileW(pattern.c_str(), &fd);
    if (h == INVALID_HANDLE_VALUE) return;
    do {
        if (wcscmp(fd.cFileName, L".") == 0 || wcscmp(fd.cFileName, L"..") == 0) continue;
        std::wstring full = JoinPath(dir, fd.cFileName);
        if (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) {
            ListRec(full, extLower, out, depth + 1, maxDepth);
        } else {
            if (!extLower.empty()) {
                std::wstring e = GetExtensionLower(fd.cFileName);
                if (e != extLower) continue;
            }
            FileInfo fi;
            fi.path = full;
            fi.name = fd.cFileName;
            fi.dir = dir;
            fi.size = ((u64)fd.nFileSizeHigh << 32) | fd.nFileSizeLow;
            fi.mtime = fd.ftLastWriteTime;
            out.push_back(fi);
        }
    } while (FindNextFileW(h, &fd));
    FindClose(h);
}

void ListFilesRecursive(const std::wstring& dir, const std::wstring& extLower,
                        std::vector<FileInfo>& out, int maxDepth) {
    ListRec(dir, extLower, out, 0, maxDepth);
}

void ListDir(const std::wstring& dir, std::vector<FileInfo>& out, bool dirsOnly) {
    std::wstring pattern = JoinPath(dir, L"*");
    WIN32_FIND_DATAW fd;
    HANDLE h = FindFirstFileW(pattern.c_str(), &fd);
    if (h == INVALID_HANDLE_VALUE) return;
    do {
        if (wcscmp(fd.cFileName, L".") == 0 || wcscmp(fd.cFileName, L"..") == 0) continue;
        bool isDir = (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
        if (dirsOnly != isDir) continue;
        FileInfo fi;
        fi.path = JoinPath(dir, fd.cFileName);
        fi.name = fd.cFileName;
        fi.dir = dir;
        fi.size = ((u64)fd.nFileSizeHigh << 32) | fd.nFileSizeLow;
        fi.mtime = fd.ftLastWriteTime;
        out.push_back(fi);
    } while (FindNextFileW(h, &fd));
    FindClose(h);
}

bool ReadAllBytes(const std::wstring& path, std::vector<u8>& out) {
    HANDLE h = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr,
                           OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h == INVALID_HANDLE_VALUE) return false;
    LARGE_INTEGER li;
    if (!GetFileSizeEx(h, &li)) { CloseHandle(h); return false; }
    out.resize((size_t)li.QuadPart);
    size_t total = 0;
    while (total < out.size()) {
        DWORD chunk = (DWORD)std::min<size_t>(out.size() - total, 1u << 24);
        DWORD got = 0;
        if (!ReadFile(h, out.data() + total, chunk, &got, nullptr) || got == 0) break;
        total += got;
    }
    CloseHandle(h);
    out.resize(total);
    return true;
}

bool WriteAllBytes(const std::wstring& path, const void* data, size_t size) {
    EnsureDirForFile(path);
    HANDLE h = CreateFileW(path.c_str(), GENERIC_WRITE, 0, nullptr,
                           CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h == INVALID_HANDLE_VALUE) return false;
    const u8* p = (const u8*)data;
    size_t total = 0;
    bool ok = true;
    while (total < size) {
        DWORD chunk = (DWORD)std::min<size_t>(size - total, 1u << 24);
        DWORD wrote = 0;
        if (!WriteFile(h, p + total, chunk, &wrote, nullptr) || wrote == 0) { ok = false; break; }
        total += wrote;
    }
    CloseHandle(h);
    return ok;
}

bool WriteAllBytes(const std::wstring& path, const std::vector<u8>& data) {
    return WriteAllBytes(path, data.data(), data.size());
}

bool CopyFileTo(const std::wstring& src, const std::wstring& dst, bool overwrite) {
    EnsureDirForFile(dst);
    return CopyFileW(src.c_str(), dst.c_str(), overwrite ? FALSE : TRUE) != 0;
}

bool DeleteFileAt(const std::wstring& path) {
    if (!FileExists(path)) return true;
    SetFileAttributesW(path.c_str(), FILE_ATTRIBUTE_NORMAL);
    return DeleteFileW(path.c_str()) != 0;
}

bool MoveFileTo(const std::wstring& src, const std::wstring& dst, bool overwrite) {
    EnsureDirForFile(dst);
    return MoveFileExW(src.c_str(), dst.c_str(),
                       overwrite ? MOVEFILE_REPLACE_EXISTING : 0) != 0;
}

std::wstring FormatFileTime(const FILETIME& ft) {
    FILETIME local;
    FileTimeToLocalFileTime(&ft, &local);
    SYSTEMTIME st;
    FileTimeToSystemTime(&local, &st);
    wchar_t buf[64];
    swprintf(buf, 64, L"%04d-%02d-%02d %02d:%02d:%02d",
             st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond);
    return buf;
}

u64 FileTimeToMillis(const FILETIME& ft) {
    ULARGE_INTEGER u;
    u.LowPart = ft.dwLowDateTime;
    u.HighPart = ft.dwHighDateTime;
    return u.QuadPart / 10000ULL - 11644473600000ULL; // 100ns since 1601 -> ms since 1970
}

void DiagLog(const std::string& line) {
    static std::wstring p;
    if (p.empty()) p = JoinPath(GetDirectoryName(GetExePath()), L"diag.log");
    HANDLE h = CreateFileW(p.c_str(), FILE_APPEND_DATA, FILE_SHARE_READ, nullptr,
                           OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h == INVALID_HANDLE_VALUE) return;
    std::string s = line + "\r\n";
    DWORD w = 0;
    WriteFile(h, s.data(), (DWORD)s.size(), &w, nullptr);
    CloseHandle(h);
}

} // namespace fsx
