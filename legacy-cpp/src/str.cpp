#include "str.h"

std::wstring Utf8ToWide(const std::string& s) {
    if (s.empty()) return std::wstring();
    int n = MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), nullptr, 0);
    if (n <= 0) return std::wstring();
    std::wstring out((size_t)n, L'\0');
    MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), &out[0], n);
    return out;
}

std::string WideToUtf8(const std::wstring& s) {
    if (s.empty()) return std::string();
    int n = WideCharToMultiByte(CP_UTF8, 0, s.data(), (int)s.size(), nullptr, 0, nullptr, nullptr);
    if (n <= 0) return std::string();
    std::string out((size_t)n, '\0');
    WideCharToMultiByte(CP_UTF8, 0, s.data(), (int)s.size(), &out[0], n, nullptr, nullptr);
    return out;
}

std::wstring FormatBytes(u64 bytes) {
    wchar_t buf[64];
    if (bytes < 1024ULL) swprintf(buf, 64, L"%llu B", (unsigned long long)bytes);
    else if (bytes < 1024ULL * 1024) swprintf(buf, 64, L"%.1f KB", (double)bytes / 1024.0);
    else if (bytes < 1024ULL * 1024 * 1024) swprintf(buf, 64, L"%.2f MB", (double)bytes / 1048576.0);
    else swprintf(buf, 64, L"%.2f GB", (double)bytes / 1073741824.0);
    return buf;
}

std::wstring FormatInt(i64 v) {
    wchar_t buf[48];
    swprintf(buf, 48, L"%lld", (long long)v);
    return buf;
}

std::wstring FormatFloat(double v, int decimals) {
    wchar_t buf[64];
    swprintf(buf, 64, L"%.*f", decimals, v);
    return buf;
}

std::wstring ToLowerW(std::wstring s) {
    for (auto& c : s) if (c >= L'A' && c <= L'Z') c = (wchar_t)(c - L'A' + L'a');
    return s;
}

bool EqualsNoCase(const std::wstring& a, const std::wstring& b) {
    if (a.size() != b.size()) return false;
    for (size_t i = 0; i < a.size(); ++i) {
        wchar_t x = a[i], y = b[i];
        if (x >= L'A' && x <= L'Z') x = (wchar_t)(x - L'A' + L'a');
        if (y >= L'A' && y <= L'Z') y = (wchar_t)(y - L'A' + L'a');
        if (x != y) return false;
    }
    return true;
}

bool EndsWithNoCase(const std::wstring& s, const std::wstring& suffix) {
    if (s.size() < suffix.size()) return false;
    return EqualsNoCase(s.substr(s.size() - suffix.size()), suffix);
}

std::wstring TrimW(const std::wstring& s) {
    size_t a = 0, b = s.size();
    while (a < b && (s[a] == L' ' || s[a] == L'\t' || s[a] == L'\r' || s[a] == L'\n')) ++a;
    while (b > a && (s[b-1] == L' ' || s[b-1] == L'\t' || s[b-1] == L'\r' || s[b-1] == L'\n')) --b;
    return s.substr(a, b - a);
}

std::vector<std::wstring> SplitW(const std::wstring& s, wchar_t sep) {
    std::vector<std::wstring> out;
    std::wstring cur;
    for (wchar_t c : s) {
        if (c == sep) { out.push_back(cur); cur.clear(); }
        else cur.push_back(c);
    }
    out.push_back(cur);
    return out;
}

std::wstring JoinW(const std::vector<std::wstring>& parts, const std::wstring& sep) {
    std::wstring out;
    for (size_t i = 0; i < parts.size(); ++i) {
        if (i) out += sep;
        out += parts[i];
    }
    return out;
}

std::wstring ReplaceAllW(std::wstring s, const std::wstring& from, const std::wstring& to) {
    if (from.empty()) return s;
    std::wstring out;
    size_t pos = 0;
    while (true) {
        size_t p = s.find(from, pos);
        if (p == std::wstring::npos) { out.append(s, pos, std::wstring::npos); break; }
        out.append(s, pos, p - pos);
        out += to;
        pos = p + from.size();
    }
    return out;
}
