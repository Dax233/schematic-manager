// 资源包（模组 / 新版本）合并
#include "blockdb.h"
#include "fs.h"
#include "str.h"
#include <zlib.h>

namespace mc {

// ------------------------------------------------------------------ 合并
bool BlockDB::MergeFrom(const BlockDB& o, const PackInfo& info, std::string& err) {
    if (o.versions_.empty()) { err = "资源包里没有版本数据"; return false; }

    const u32 strBase   = (u32)strings_.size();
    const u32 langBase  = (u32)langs_.size();
    const u32 texBase   = (u32)textures_.size();
    const u32 modelBase = (u32)models_.size();
    const u32 pixBase   = (u32)texturePixels_.size();

    // 字符串池
    strings_.insert(strings_.end(), o.strings_.begin(), o.strings_.end());

    // 语言表（LangEntry 里存的是字符串本身，直接追加）
    for (const auto& tbl : o.langs_) langs_.push_back(tbl);

    // 贴图池（像素偏移要加上现有缓冲长度）
    for (TextureInfo ti : o.textures_) {
        ti.offset += pixBase;
        textures_.push_back(ti);
    }
    texturePixels_.insert(texturePixels_.end(), o.texturePixels_.begin(), o.texturePixels_.end());

    // 模型池（面里的贴图路径是字符串下标，要偏移）
    for (Model m : o.models_) {
        for (ModelElement& e : m.elements)
            for (ModelFace& f : e.faces)
                if (f.texStrIdx != 0xFFFFFFFF) f.texStrIdx += strBase;
        models_.push_back(std::move(m));
    }

    // 版本
    const bool overlay = (info.kind == 2);
    for (VersionDef v : o.versions_) {
        v.langIdx += langBase;
        v.isOverlay = overlay;
        v.packName = info.name;
        for (auto& kv : v.textureByPath) kv.second += texBase;
        for (BlockDef& bd : v.blocks) {
            for (VariantEntry& ve : bd.variants)  ve.modelIdx += modelBase;
            for (VariantEntry& ve : bd.multipart) ve.modelIdx += modelBase;
        }
        versions_.push_back(std::move(v));
    }

    PackInfo rec = info;
    rec.bytes = 0;
    rec.versionIdx = (int)versions_.size() - 1;
    rec.dataVersion = versions_.back().dataVersion;
    rec.blocks = versions_.back().blocks.size();
    rec.textures = o.textures_.size();
    rec.models = o.models_.size();
    packs_.push_back(rec);

    RebuildDerived();
    return true;
}

bool BlockDB::LoadPackBytes(const u8* data, size_t size, const PackInfo& info, std::string& err) {
    BlockDB tmp;
    if (!tmp.LoadFromMemory(data, size, err)) return false;
    tmp.packs_.clear();
    return MergeFrom(tmp, info, err);
}

// ------------------------------------------------------------------ 用户包清单
// packs.tsv 每行：enabled \t 文件名 \t 显示名 \t 来源 \t 类型(1=MC版本 2=模组) \t dataVersion
static std::string EscTsv(const std::string& s) {
    std::string o;
    for (char c : s) {
        if (c == '\t') o += "\\t";
        else if (c == '\n') o += "\\n";
        else if (c == '\\') o += "\\\\";
        else o += c;
    }
    return o;
}
static std::string UnescTsv(const std::string& s) {
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

bool BlockDB::LoadUserPacks(const std::wstring& dir, std::string& err) {
    if (!fsx::DirExists(dir)) return true;   // 没有就是空，不算错

    std::wstring manifest = fsx::JoinPath(dir, L"packs.tsv");
    std::vector<u8> raw;
    int loaded = 0, failed = 0;
    if (!fsx::ReadAllBytes(manifest, raw)) return true;

    std::string txt((const char*)raw.data(), raw.size());
    size_t pos = 0;
    while (pos < txt.size()) {
        size_t nl = txt.find('\n', pos);
        std::string line = (nl == std::string::npos) ? txt.substr(pos) : txt.substr(pos, nl - pos);
        pos = (nl == std::string::npos) ? txt.size() : nl + 1;
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.empty() || line[0] == '#') continue;

        std::vector<std::string> f;
        size_t p2 = 0;
        while (p2 <= line.size()) {
            size_t t = line.find('\t', p2);
            f.push_back((t == std::string::npos) ? line.substr(p2) : line.substr(p2, t - p2));
            if (t == std::string::npos) break;
            p2 = t + 1;
        }
        if (f.size() < 6) continue;
        bool enabled = (f[0] == "1");
        if (!enabled) continue;

        std::wstring file = fsx::JoinPath(dir, Utf8ToWide(UnescTsv(f[1])));
        std::vector<u8> bytes;
        if (!fsx::ReadAllBytes(file, bytes)) { ++failed; continue; }

        PackInfo info;
        info.file = file;
        info.name = UnescTsv(f[2]);
        info.source = UnescTsv(f[3]);
        info.kind = atoi(f[4].c_str());
        info.dataVersion = (i32)strtol(f[5].c_str(), nullptr, 10);
        info.enabled = true;
        info.bytes = bytes.size();

        std::string e2;
        if (!LoadPackBytes(bytes.data(), bytes.size(), info, e2)) { ++failed; continue; }
        ++loaded;
    }
    if (failed) err = std::to_string(failed) + " 个资源包加载失败";
    return true;
}

} // namespace mc
