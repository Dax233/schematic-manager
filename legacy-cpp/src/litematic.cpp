#include "litematic.h"
#include "fs.h"
#include "str.h"

namespace ltm {

// ---------------------------------------------------------------- 版本表
struct DataVerEntry { i32 dv; const char* name; };
#include "mcver_table.inc"

std::string MinecraftVersionFromDataVersion(i32 dv) {
    if (dv <= 0) return std::string();
    const DataVerEntry* best = nullptr;
    for (size_t i = 0; i < kDataVersCount; ++i) {
        if (kDataVers[i].dv == dv) return kDataVers[i].name;
        if (kDataVers[i].dv < dv) best = &kDataVers[i];
    }
    if (best) return std::string(best->name) + "+";
    return std::string();
}

const std::vector<std::pair<i32, std::string>>& KnownDataVersions() {
    static std::vector<std::pair<i32, std::string>> v;
    if (v.empty()) {
        for (size_t i = 0; i < kDataVersCount; ++i)
            v.emplace_back(kDataVers[i].dv, kDataVers[i].name);
        std::reverse(v.begin(), v.end());
    }
    return v;
}

// ---------------------------------------------------------------- BlockState
std::string BlockState::PropStr() const {
    std::string out;
    for (size_t i = 0; i < props.size(); ++i) {
        if (i) out += ",";
        out += props[i].first + "=" + props[i].second;
    }
    return out;
}
std::string BlockState::Key() const {
    std::string p = PropStr();
    return p.empty() ? name : (name + "[" + p + "]");
}
std::string BlockState::DisplayName() const {
    size_t c = name.find(':');
    return (c == std::string::npos) ? name : name.substr(c + 1);
}
std::string BlockState::Prop(const std::string& k) const {
    for (const auto& kv : props) if (kv.first == k) return kv.second;
    return std::string();
}
void BlockState::SetProp(const std::string& k, const std::string& v) {
    for (auto& kv : props) if (kv.first == k) { kv.second = v; return; }
    props.emplace_back(k, v);
}
bool BlockState::SameState(const BlockState& o) const {
    if (name != o.name || props.size() != o.props.size()) return false;
    for (size_t i = 0; i < props.size(); ++i)
        if (props[i].first != o.props[i].first || props[i].second != o.props[i].second) return false;
    return true;
}

// ---------------------------------------------------------------- 位数组
static inline u64 MaskOf(int bits) { return (bits >= 64) ? ~0ULL : ((1ULL << bits) - 1ULL); }

int Schematic::BitsForPalette(size_t n) {
    if (n <= 2) return 2;
    int bits = 0;
    size_t v = n - 1;
    while (v) { ++bits; v >>= 1; }
    return std::max(2, bits);
}

i32 Schematic::ReadBits(const std::vector<i64>& arr, size_t index, int bits) {
    if (bits <= 0) return 0;
    u64 off = (u64)index * (u64)bits;
    int startBit = (int)(off & 63);
    size_t startLong = (size_t)(off >> 6);
    if (startLong >= arr.size()) return 0;
    u64 mask = MaskOf(bits);
    u64 a = (u64)arr[startLong];
    if (startBit + bits <= 64) return (i32)((a >> startBit) & mask);
    int endBits = 64 - startBit;
    u64 v = a >> startBit;
    u64 b = (startLong + 1 < arr.size()) ? (u64)arr[startLong + 1] : 0ULL;
    v |= (b & MaskOf(bits - endBits)) << endBits;
    return (i32)(v & mask);
}

void Schematic::WriteBits(std::vector<i64>& arr, size_t index, int bits, i32 value) {
    if (bits <= 0) return;
    u64 off = (u64)index * (u64)bits;
    int startBit = (int)(off & 63);
    size_t startLong = (size_t)(off >> 6);
    if (startLong >= arr.size()) return;
    u64 mask = MaskOf(bits);
    u64 v = (u64)(u32)value & mask;
    u64 a = (u64)arr[startLong];
    if (startBit + bits <= 64) {
        a = (a & ~(mask << startBit)) | (v << startBit);
        arr[startLong] = (i64)a;
    } else {
        int endBits = 64 - startBit;
        a = (a & MaskOf(startBit)) | ((v & MaskOf(endBits)) << startBit);
        arr[startLong] = (i64)a;
        if (startLong + 1 < arr.size()) {
            int hi = bits - endBits;
            u64 b = (u64)arr[startLong + 1];
            b = (b & ~MaskOf(hi)) | ((v >> endBits) & MaskOf(hi));
            arr[startLong + 1] = (i64)b;
        }
    }
}

std::vector<i64> Schematic::EncodeIndices(const std::vector<i32>& indices, int bits) {
    if (bits <= 0) return std::vector<i64>();
    size_t n = ((size_t)indices.size() * (size_t)bits + 63) / 64;
    std::vector<i64> arr(n, 0);
    for (size_t i = 0; i < indices.size(); ++i)
        WriteBits(arr, i, bits, indices[i]);
    return arr;
}

// ---------------------------------------------------------------- 解析
static bool ReadPalette(const nbt::Value* pal, std::vector<BlockState>& out) {
    out.clear();
    if (!pal || !pal->isList()) return false;
    out.reserve(pal->list.size());
    for (const auto& e : pal->list) {
        BlockState bs;
        if (const nbt::Value* n = e.find("Name")) bs.name = n->asString();
        if (const nbt::Value* p = e.find("Properties")) {
            if (p->isCompound())
                for (const auto& kv : p->fields) bs.props.emplace_back(kv.first, kv.second.asString());
        }
        out.push_back(std::move(bs));
    }
    return true;
}

static void ReadVec3(const nbt::Value* v, i32 out[3]) {
    out[0] = out[1] = out[2] = 0;
    if (v) v->asXYZ(out);
}

bool Schematic::FromNbt(const nbt::Value& root) {
    regions_.clear();
    const nbt::Value* regions = root.find("Regions");
    if (!regions || !regions->isCompound()) return false;
    for (const auto& kv : regions->fields) {
        const nbt::Value& rv = kv.second;
        Region r;
        r.name = kv.first;
        ReadVec3(rv.find("Position"), r.pos);
        ReadVec3(rv.find("Size"), r.size);
        // Litematica 允许 Size 分量为负数，表示该区域从 Position 往负方向延伸。
        // 负数不是非法数据：不做归一化会导致整个 region 被跳过（表现为"没有可渲染的方块"）。
        // 归一化时 Position 也要跟着移到区域的负方向最小角。
        for (int a = 0; a < 3; ++a) {
            if (r.size[a] < 0) {
                r.pos[a] = r.pos[a] + r.size[a] + 1;
                r.size[a] = -r.size[a];
            }
        }
        if (r.size[0] <= 0 || r.size[1] <= 0 || r.size[2] <= 0) continue;
        r.volume = (size_t)r.size[0] * (size_t)r.size[1] * (size_t)r.size[2];

        ReadPalette(rv.find("BlockStatePalette"), r.palette);
        r.bits = BitsForPalette(r.palette.size());

        const nbt::Value* bs = rv.find("BlockStates");
        if (bs && bs->type == nbt::TAG_LongArray) {
            r.indices.resize(r.volume);
            for (size_t i = 0; i < r.volume; ++i) {
                i32 idx = ReadBits(bs->longs, i, r.bits);
                if (idx < 0 || (size_t)idx >= r.palette.size()) idx = 0;
                r.indices[i] = idx;
            }
        } else {
            r.indices.assign(r.volume, 0);
        }

        if (const nbt::Value* e = rv.find("Entities")) r.entityCount = e->list.size();
        if (const nbt::Value* e = rv.find("TileEntities")) r.tileEntityCount = e->list.size();
        if (const nbt::Value* e = rv.find("PendingBlockTicks")) r.pendingBlockTicks = e->list.size();
        if (const nbt::Value* e = rv.find("PendingFluidTicks")) r.pendingFluidTicks = e->list.size();
        regions_.push_back(std::move(r));
    }
    return true;
}

bool Schematic::SyncToNbt() {
    nbt::Value* regions = root_.find("Regions");
    if (!regions) return false;
    for (Region& r : regions_) {
        nbt::Value* rv = regions->find(r.name);
        if (!rv) continue;
        // 调色板
        nbt::Value pal = nbt::Value::MakeList(nbt::TAG_Compound);
        for (const BlockState& bs : r.palette) {
            nbt::Value e = nbt::Value::MakeCompound();
            e.set("Name", nbt::Value::MakeString(bs.name));
            if (!bs.props.empty()) {
                nbt::Value pr = nbt::Value::MakeCompound();
                for (const auto& kv : bs.props)
                    pr.set(kv.first, nbt::Value::MakeString(kv.second));
                e.set("Properties", pr);
            }
            pal.list.push_back(std::move(e));
        }
        rv->set("BlockStatePalette", std::move(pal));
        // 位数组
        r.bits = BitsForPalette(r.palette.size());
        std::vector<i64> longs = EncodeIndices(r.indices, r.bits);
        rv->set("BlockStates", nbt::Value::MakeLongArray(std::move(longs)));
    }
    return true;
}

// ---------------------------------------------------------------- Peek
bool Schematic::Peek(const std::wstring& path, Summary& out) {
    out = Summary();
    out.path = path;
    out.fileName = fsx::GetFileName(path);
    out.dirName = fsx::GetDirectoryName(path);

    WIN32_FILE_ATTRIBUTE_DATA fad;
    if (GetFileAttributesExW(path.c_str(), GetFileExInfoStandard, &fad)) {
        out.fileSize = ((u64)fad.nFileSizeHigh << 32) | fad.nFileSizeLow;
        out.fileTime = fad.ftLastWriteTime;
    }

    std::vector<u8> raw;
    if (!fsx::ReadAllBytes(path, raw)) { out.error = "无法读取文件"; return false; }

    nbt::Value root;
    std::string err;
    if (!nbt::ParseMaybeGzip(raw, root, err)) { out.error = err; return false; }

    const nbt::Value* v;
    if ((v = root.find("Version"))) out.formatVersion = v->asInt32();
    if ((v = root.find("SubVersion"))) out.formatSubVersion = v->asInt32();
    if ((v = root.find("MinecraftDataVersion"))) out.dataVersion = v->asInt32();
    out.mcVersionName = MinecraftVersionFromDataVersion(out.dataVersion);

    if (const nbt::Value* md = root.find("Metadata")) {
        auto str = [&](const char* k) -> std::string {
            const nbt::Value* x = md->find(k);
            return x ? x->asString() : std::string();
        };
        auto num = [&](const char* k) -> i64 {
            const nbt::Value* x = md->find(k);
            return x ? x->asInt() : 0;
        };
        out.name = str("Name");
        out.author = str("Author");
        out.description = str("Description");
        out.timeCreated = num("TimeCreated");
        out.timeModified = num("TimeModified");
        out.totalBlocks = num("TotalBlocks");
        out.totalVolume = num("TotalVolume");
        out.regionCount = (i32)num("RegionCount");
        ReadVec3(md->find("EnclosingSize"), out.enclosing);
        if (const nbt::Value* p = md->find("PreviewImageData")) {
            out.hasPreview = (p->ints.size() > 0);
            out.previewPixels = (i64)p->ints.size();
        }
    }
    if (out.regionCount == 0) {
        const nbt::Value* rs = root.find("Regions");
        out.regionCount = (rs && rs->isCompound()) ? (i32)rs->fields.size() : 0;
    }
    if (const nbt::Value* rs = root.find("Regions")) {
        if (rs->isCompound()) {
            for (const auto& kv : rs->fields) {
                const nbt::Value& rv = kv.second;
                if (const nbt::Value* e = rv.find("Entities")) out.entityCount += (i64)e->list.size();
                if (const nbt::Value* e = rv.find("TileEntities")) out.tileEntityCount += (i64)e->list.size();
                if (const nbt::Value* e = rv.find("PendingBlockTicks")) out.pendingTicks += (i64)e->list.size();
                if (const nbt::Value* e = rv.find("PendingFluidTicks")) out.pendingTicks += (i64)e->list.size();
            }
        }
    }
    out.valid = true;
    return true;
}

// ---------------------------------------------------------------- Load / Save
bool Schematic::Load(const std::wstring& path, std::string& err) {
    path_ = path;
    loaded_ = false;
    modified_ = false;
    std::vector<u8> raw;
    if (!fsx::ReadAllBytes(path, raw)) { err = "无法读取文件"; return false; }
    if (!nbt::ParseMaybeGzip(raw, root_, err)) return false;
    if (!FromNbt(root_)) { err = "不是有效的 litematic 文件（缺少 Regions）"; return false; }
    loaded_ = true;
    return true;
}

bool Schematic::ToBytes(std::vector<u8>& out, std::string& err) {
    if (modified_) {
        if (!SyncToNbt()) { err = "同步到 NBT 失败"; return false; }
    }
    return nbt::SerializeGzip(root_, out, 6);
}

bool Schematic::SaveTo(const std::wstring& path, std::string& err) {
    std::vector<u8> out;
    if (!ToBytes(out, err)) return false;
    if (!fsx::WriteAllBytes(path, out)) { err = "写入文件失败"; return false; }
    path_ = path;
    modified_ = false;
    return true;
}

// ---------------------------------------------------------------- Summary / Analyze
Summary Schematic::GetSummary() const {
    Summary s;
    s.path = path_;
    s.fileName = fsx::GetFileName(path_);
    s.dirName = fsx::GetDirectoryName(path_);
    WIN32_FILE_ATTRIBUTE_DATA fad;
    if (GetFileAttributesExW(path_.c_str(), GetFileExInfoStandard, &fad)) {
        s.fileSize = ((u64)fad.nFileSizeHigh << 32) | fad.nFileSizeLow;
        s.fileTime = fad.ftLastWriteTime;
    }
    s.valid = loaded_;
    const nbt::Value* v;
    if ((v = root_.find("Version"))) s.formatVersion = v->asInt32();
    if ((v = root_.find("SubVersion"))) s.formatSubVersion = v->asInt32();
    if ((v = root_.find("MinecraftDataVersion"))) s.dataVersion = v->asInt32();
    s.mcVersionName = MinecraftVersionFromDataVersion(s.dataVersion);
    if (const nbt::Value* md = root_.find("Metadata")) {
        auto str = [&](const char* k) -> std::string { const nbt::Value* x = md->find(k); return x ? x->asString() : std::string(); };
        auto num = [&](const char* k) -> i64 { const nbt::Value* x = md->find(k); return x ? x->asInt() : 0; };
        s.name = str("Name"); s.author = str("Author"); s.description = str("Description");
        s.timeCreated = num("TimeCreated"); s.timeModified = num("TimeModified");
        s.totalBlocks = num("TotalBlocks"); s.totalVolume = num("TotalVolume");
        s.regionCount = (i32)num("RegionCount");
        ReadVec3(md->find("EnclosingSize"), s.enclosing);
        if (const nbt::Value* p = md->find("PreviewImageData")) {
            s.hasPreview = !p->ints.empty();
            s.previewPixels = (i64)p->ints.size();
        }
    }
    if (s.regionCount == 0) s.regionCount = (i32)regions_.size();
    for (const Region& r : regions_) {
        s.entityCount += (i64)r.entityCount;
        s.tileEntityCount += (i64)r.tileEntityCount;
        s.pendingTicks += (i64)(r.pendingBlockTicks + r.pendingFluidTicks);
    }
    s.regions = regions_;
    return s;
}

Analysis Schematic::Analyze() const {
    Analysis a;
    std::map<std::string, u64> byName;
    std::map<std::string, u64> byState;
    for (const Region& r : regions_) {
        // 先按调色板统计（快），再对实际使用的索引累加
        std::vector<u64> counts(r.palette.size(), 0);
        for (size_t i = 0; i < r.indices.size(); ++i) {
            i32 idx = r.indices[i];
            if (idx >= 0 && (size_t)idx < counts.size()) counts[(size_t)idx]++;
        }
        for (size_t i = 0; i < r.palette.size(); ++i) {
            if (!counts[i]) continue;
            const std::string& nm = r.palette[i].name;
            byName[nm] += counts[i];
            byState[r.palette[i].Key()] += counts[i];
            if (nm == "minecraft:air" || nm == "minecraft:cave_air" || nm == "minecraft:void_air")
                a.airTotal += counts[i];
            else
                a.nonAirTotal += counts[i];
        }
        // 实体
        const nbt::Value* rv = nullptr;
        if (const nbt::Value* regions = root_.find("Regions")) rv = regions->find(r.name);
        if (rv) {
            if (const nbt::Value* e = rv->find("Entities")) {
                for (const auto& ent : e->list) {
                    const nbt::Value* id = ent.find("id");
                    std::string sid = id ? id->asString() : "unknown";
                    if (sid.empty()) sid = "unknown";
                    a.entitiesById[sid]++;
                }
            }
            if (const nbt::Value* e = rv->find("TileEntities")) {
                for (const auto& ent : e->list) {
                    const nbt::Value* id = ent.find("id");
                    std::string sid = id ? id->asString() : "unknown";
                    if (sid.empty()) sid = "unknown";
                    a.tileEntitiesById[sid]++;
                }
            }
        }
    }
    // 去掉空气之外的种类统计由调用方决定
    a.blocksByName = byName;
    a.blocksByState = byState;
    int distinct = 0;
    for (const auto& kv : byName) if (kv.first != "minecraft:air") ++distinct;
    a.distinctBlockTypes = distinct;
    a.distinctStates = (int)byState.size();
    return a;
}

// ---------------------------------------------------------------- 编辑
static nbt::Value* EnsureMetadata(nbt::Value& root) {
    nbt::Value* md = root.find("Metadata");
    if (!md) { root.set("Metadata", nbt::Value::MakeCompound()); md = root.find("Metadata"); }
    return md;
}

bool Schematic::SetMetaName(const std::string& v) {
    if (nbt::Value* md = EnsureMetadata(root_)) { md->set("Name", nbt::Value::MakeString(v)); MarkModified(); return true; }
    return false;
}
bool Schematic::SetMetaAuthor(const std::string& v) {
    if (nbt::Value* md = EnsureMetadata(root_)) { md->set("Author", nbt::Value::MakeString(v)); MarkModified(); return true; }
    return false;
}
bool Schematic::SetMetaDescription(const std::string& v) {
    if (nbt::Value* md = EnsureMetadata(root_)) { md->set("Description", nbt::Value::MakeString(v)); MarkModified(); return true; }
    return false;
}
bool Schematic::SetDataVersion(i32 v) {
    root_.set("MinecraftDataVersion", nbt::Value::MakeInt(v));
    MarkModified();
    return true;
}
bool Schematic::SetFormatVersion(i32 ver, i32 sub) {
    root_.set("Version", nbt::Value::MakeInt(ver));
    root_.set("SubVersion", nbt::Value::MakeInt(sub));
    MarkModified();
    return true;
}
bool Schematic::SetTimeCreated(i64 ms) {
    if (nbt::Value* md = EnsureMetadata(root_)) { md->set("TimeCreated", nbt::Value::MakeLong(ms)); MarkModified(); return true; }
    return false;
}
bool Schematic::SetTimeModified(i64 ms) {
    if (nbt::Value* md = EnsureMetadata(root_)) { md->set("TimeModified", nbt::Value::MakeLong(ms)); MarkModified(); return true; }
    return false;
}
void Schematic::TouchTimeModified() {
    FILETIME ft;
    GetSystemTimeAsFileTime(&ft);
    ULARGE_INTEGER u; u.LowPart = ft.dwLowDateTime; u.HighPart = ft.dwHighDateTime;
    SetTimeModified((i64)(u.QuadPart / 10000ULL - 11644473600000ULL));
}

u64 Schematic::ReplaceBlocks(const ReplaceRule& rule, const IBlockInfo* info, u64 limit) {
    if (rule.fromName.empty() || rule.toName.empty()) return 0;
    u64 total = 0;
    for (Region& r : regions_) {
        // 索引查找表
        std::map<std::string, i32> lookup;
        for (size_t i = 0; i < r.palette.size(); ++i) lookup[r.palette[i].Key()] = (i32)i;

        bool changed = false;
        for (size_t i = 0; i < r.indices.size(); ++i) {
            if (limit && total >= limit) break;
            i32 idx = r.indices[i];
            if (idx < 0 || (size_t)idx >= r.palette.size()) continue;
            const BlockState& src = r.palette[(size_t)idx];
            if (src.name != rule.fromName) continue;
            // 限定属性必须全部匹配
            if (!rule.fromProps.empty()) {
                bool ok = true;
                for (const auto& kv : rule.fromProps)
                    if (src.Prop(kv.first) != kv.second) { ok = false; break; }
                if (!ok) continue;
            }
            // 构造目标状态
            BlockState dst;
            dst.name = rule.toName;
            if (info) {
                std::vector<std::pair<std::string, std::string>> defaults;
                info->DefaultProps(rule.toName, defaults);
                for (const auto& kv : defaults) dst.SetProp(kv.first, kv.second);
                if (rule.inheritProps) {
                    for (const auto& kv : src.props)
                        if (info->SupportsProp(rule.toName, kv.first, kv.second))
                            dst.SetProp(kv.first, kv.second);
                }
            } else if (rule.inheritProps) {
                dst.props = src.props;
            }
            for (const auto& kv : rule.toProps) dst.SetProp(kv.first, kv.second);

            std::string key = dst.Key();
            auto it = lookup.find(key);
            i32 newIdx;
            if (it != lookup.end()) {
                newIdx = it->second;
            } else {
                newIdx = (i32)r.palette.size();
                r.palette.push_back(dst);
                lookup[key] = newIdx;
            }
            if (r.indices[i] != newIdx) {
                r.indices[i] = newIdx;
                changed = true;
                ++total;
            }
        }
        if (changed) MarkModified();
    }
    return total;
}

} // namespace ltm
