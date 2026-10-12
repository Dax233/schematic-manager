// 投影备注：本地存储，按内容哈希关联，文件移动/改名后仍能对齐
#include "app_internal.h"
#include "packmgr.h"
#include "i18n.h"
#include "fs.h"
#include "str.h"
#include <algorithm>

namespace app {

struct NoteRec {
    u64 hash = 0;
    std::wstring path;
    std::wstring name;
    std::wstring text;
};

static std::wstring NotesPath() {
    return fsx::JoinPath(packmgr::PacksDir(), L"notes.tsv");
}

static std::string Esc(const std::string& s) {
    std::string o;
    for (char c : s) {
        if (c == '\t') o += "\\t";
        else if (c == '\n') o += "\\n";
        else if (c == '\r') o += "\\r";
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
            else if (n == 'r') o += '\r';
            else o += n;
        } else o += s[i];
    }
    return o;
}

static std::vector<NoteRec> LoadNotes() {
    std::vector<NoteRec> out;
    std::vector<u8> raw;
    if (!fsx::ReadAllBytes(NotesPath(), raw)) return out;
    std::string txt((const char*)raw.data(), raw.size());
    size_t pos = 0;
    while (pos < txt.size()) {
        size_t nl = txt.find('\n', pos);
        std::string line = (nl == std::string::npos) ? txt.substr(pos) : txt.substr(pos, nl - pos);
        pos = (nl == std::string::npos) ? txt.size() : nl + 1;
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.empty() || line[0] == '#') continue;
        std::vector<std::string> f;
        size_t p = 0;
        while (p <= line.size()) {
            size_t t = line.find('\t', p);
            f.push_back((t == std::string::npos) ? line.substr(p) : line.substr(p, t - p));
            if (t == std::string::npos) break;
            p = t + 1;
        }
        if (f.size() < 4) continue;
        NoteRec n;
        n.hash = strtoull(f[0].c_str(), nullptr, 10);
        n.path = Utf8ToWide(Unesc(f[1]));
        n.name = Utf8ToWide(Unesc(f[2]));
        n.text = Utf8ToWide(Unesc(f[3]));
        out.push_back(n);
    }
    return out;
}

static void SaveNotes(const std::vector<NoteRec>& notes) {
    std::string txt = "# hash\tpath\tname\tnote\n";
    for (const NoteRec& n : notes) {
        char hb[32];
        snprintf(hb, sizeof(hb), "%llu", (unsigned long long)n.hash);
        txt += hb; txt += '\t';
        txt += Esc(WideToUtf8(n.path)); txt += '\t';
        txt += Esc(WideToUtf8(n.name)); txt += '\t';
        txt += Esc(WideToUtf8(n.text)); txt += '\n';
    }
    fsx::WriteAllBytes(NotesPath(), txt.data(), txt.size());
}

// 扫描结束后：把备注贴到文件上；能对齐的对齐，对不上的清理
void AttachNotes() {
    State& st = Get();
    std::vector<NoteRec> notes = LoadNotes();
    if (notes.empty()) {
        for (auto& f : st.files) f.note.clear();
        return;
    }
    std::vector<bool> used(notes.size(), false);
    int relinked = 0;

    for (FileItem& f : st.files) {
        f.note.clear();
        // 1) 内容哈希精确匹配（文件移动/改名后仍能对上）
        for (size_t i = 0; i < notes.size(); ++i) {
            if (used[i]) continue;
            if (notes[i].hash == f.contentHash && f.contentHash != 0) {
                f.note = notes[i].text;
                used[i] = true;
                break;
            }
        }
        if (!f.note.empty()) continue;
        // 2) 路径匹配（文件被本程序编辑过，哈希变了）
        for (size_t i = 0; i < notes.size(); ++i) {
            if (used[i]) continue;
            if (EqualsNoCase(notes[i].path, f.sum.path)) {
                f.note = notes[i].text;
                used[i] = true;
                notes[i].hash = f.contentHash;      // 重新对齐到新哈希
                ++relinked;
                break;
            }
        }
    }

    // 3) 清理找不到文件的备注
    std::vector<NoteRec> keep;
    int dropped = 0;
    for (size_t i = 0; i < notes.size(); ++i) {
        if (used[i]) { keep.push_back(notes[i]); continue; }
        if (notes[i].text.empty()) continue;
        ++dropped;      // 文件已不存在，备注作废
    }
    if (dropped || relinked) {
        SaveNotes(keep);
        if (dropped)
            SetStatus(FmtW(L"\u5DF2\u6E05\u7406 %d \u6761\u65E0\u6548\u5907\u6CE8", dropped));
        backup::Log(L"*", L"notes", FmtW(L"relinked=%d dropped=%d", relinked, dropped));
    }
}

void SetNoteFor(int fileIndex, const std::wstring& text) {
    State& st = Get();
    if (fileIndex < 0 || (size_t)fileIndex >= st.files.size()) return;
    FileItem& f = st.files[(size_t)fileIndex];
    f.note = text;

    std::vector<NoteRec> notes = LoadNotes();
    // 先删掉同一文件（哈希或路径）的旧记录
    notes.erase(std::remove_if(notes.begin(), notes.end(),
        [&](const NoteRec& n) {
            return (n.hash != 0 && n.hash == f.contentHash) || EqualsNoCase(n.path, f.sum.path);
        }), notes.end());
    if (!text.empty()) {
        NoteRec n;
        n.hash = f.contentHash;
        n.path = f.sum.path;
        n.name = f.sum.fileName;
        n.text = text;
        notes.push_back(n);
    }
    SaveNotes(notes);
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

std::wstring GetNoteFor(int fileIndex) {
    State& st = Get();
    if (fileIndex < 0 || (size_t)fileIndex >= st.files.size()) return std::wstring();
    return st.files[(size_t)fileIndex].note;
}

// 移动/复制后同步备注路径
void RebaseNotePath(const std::wstring& oldPath, const std::wstring& newPath) {
    std::vector<NoteRec> notes = LoadNotes();
    bool changed = false;
    for (NoteRec& n : notes) {
        if (EqualsNoCase(n.path, oldPath)) { n.path = newPath; changed = true; }
    }
    if (changed) SaveNotes(notes);
}

} // namespace app
