#pragma once
#include "common.h"
#include "nbt.h"

// .litematic 投影文件模型
//
// 设计要点：
//  * 完整保留解析后的 NBT 树（root），保证任何未修改的字段原样写回，零信息损失
//  * 加载时把 BlockStates 位数组解码成 palette 索引数组，便于统计与编辑
//  * 保存时若被修改，则重新编码位数组写回 NBT
namespace ltm {

// 方块状态：名称 + 属性（保持顺序）
struct BlockState {
    std::string name;                                       // 例如 minecraft:oak_stairs
    std::vector<std::pair<std::string, std::string>> props; // 例如 facing=east, half=bottom

    std::string PropStr() const;          // "facing=east,half=bottom"
    std::string Key() const;              // "minecraft:oak_stairs[facing=east,...]"
    std::string DisplayName() const;      // 去掉命名空间
    std::string Prop(const std::string& k) const;
    void SetProp(const std::string& k, const std::string& v);
    bool SameState(const BlockState& o) const;
    bool operator==(const BlockState& o) const { return SameState(o); }
};

// 单个区域
struct Region {
    std::string name;
    i32 pos[3] = {0, 0, 0};
    i32 size[3] = {0, 0, 0};
    size_t volume = 0;
    int bits = 0;

    std::vector<BlockState> palette;
    std::vector<i32> indices;          // 每个方块的调色板索引，长度 = volume

    size_t entityCount = 0;
    size_t tileEntityCount = 0;
    size_t pendingBlockTicks = 0;
    size_t pendingFluidTicks = 0;

    size_t Index(i32 x, i32 y, i32 z) const {
        return (size_t)x + (size_t)z * (size_t)size[0] + (size_t)y * (size_t)size[0] * (size_t)size[2];
    }
    void PosFromIndex(size_t i, i32 out[3]) const {
        if (size[0] <= 0 || size[2] <= 0) { out[0] = out[1] = out[2] = 0; return; }
        out[0] = (i32)(i % (size_t)size[0]);
        size_t r = i / (size_t)size[0];
        out[2] = (i32)(r % (size_t)size[2]);
        out[1] = (i32)(r / (size_t)size[2]);
    }
    bool Inside(i32 x, i32 y, i32 z) const {
        return x >= 0 && y >= 0 && z >= 0 && x < size[0] && y < size[1] && z < size[2];
    }
};

// 文件摘要（只读元数据，用于列表快速扫描）
struct Summary {
    bool     valid = false;
    std::string error;

    std::wstring path, fileName, dirName;
    u64      fileSize = 0;
    FILETIME fileTime = {};

    i32      formatVersion = 0;
    i32      formatSubVersion = 0;
    i32      dataVersion = 0;
    std::string mcVersionName;

    std::string name, author, description;
    i64      timeCreated = 0, timeModified = 0;
    i32      enclosing[3] = {0, 0, 0};
    i64      totalBlocks = 0, totalVolume = 0;
    i32      regionCount = 0;
    bool     hasPreview = false;
    i64      previewPixels = 0;

    i64      entityCount = 0;
    i64      tileEntityCount = 0;
    i64      pendingTicks = 0;

    // 以下需全量解码后才有意义
    bool     fullyAnalyzed = false;
    int      distinctBlockTypes = 0;
    int      distinctStates = 0;
    std::vector<Region> regions;
};

// 完整分析结果
struct Analysis {
    std::map<std::string, u64> blocksByName;
    std::map<std::string, u64> blocksByState;
    std::map<std::string, u64> entitiesById;
    std::map<std::string, u64> tileEntitiesById;
    u64  nonAirTotal = 0;
    u64  airTotal = 0;
    int  distinctBlockTypes = 0;
    int  distinctStates = 0;
};

// 方块替换规则
struct ReplaceRule {
    std::string fromName;                                        // 源方块名（必填，不含属性）
    std::vector<std::pair<std::string, std::string>> fromProps;  // 可选：限定必须匹配的属性
    std::string toName;                                          // 目标方块名
    std::vector<std::pair<std::string, std::string>> toProps;    // 可选：强制设置的属性
    bool inheritProps = true;                                    // 继承源方块中目标也支持的属性
    bool keepUnknownProps = false;                               // 是否保留目标不支持的属性（一般 false）
};

// 方块知识来源（由方块数据库实现，可为空）
struct IBlockInfo {
    virtual ~IBlockInfo() {}
    // 该方块是否支持某属性取某值
    virtual bool SupportsProp(const std::string& block, const std::string& prop,
                              const std::string& value) const = 0;
    // 该方块的默认属性（用于补齐目标方块必需属性）
    virtual bool DefaultProps(const std::string& block,
                              std::vector<std::pair<std::string, std::string>>& out) const = 0;
};

class Schematic {
public:
    Schematic() {}

    // 只读元数据快速扫描（不解码方块，适合批量列表）
    static bool Peek(const std::wstring& path, Summary& out);

    bool Load(const std::wstring& path, std::string& err);
    bool SaveTo(const std::wstring& path, std::string& err);
    bool ToBytes(std::vector<u8>& out, std::string& err);

    bool IsLoaded() const { return loaded_; }
    bool IsModified() const { return modified_; }
    void MarkModified() { modified_ = true; }

    nbt::Value& Root() { return root_; }
    const nbt::Value& Root() const { return root_; }
    std::vector<Region>& Regions() { return regions_; }
    const std::vector<Region>& Regions() const { return regions_; }
    const std::wstring& Path() const { return path_; }

    Summary GetSummary() const;
    Analysis Analyze() const;

    // --- 编辑操作（成功则自动 MarkModified） ---
    bool SetMetaName(const std::string& v);
    bool SetMetaAuthor(const std::string& v);
    bool SetMetaDescription(const std::string& v);
    bool SetDataVersion(i32 v);
    bool SetFormatVersion(i32 ver, i32 sub);
    bool SetTimeCreated(i64 ms);
    bool SetTimeModified(i64 ms);
    void TouchTimeModified();

    // 方块替换：返回被替换的方块实例数
    u64 ReplaceBlocks(const ReplaceRule& rule, const IBlockInfo* info, u64 limit = 0);

    // 位数组读写（跨 long 连续打包，与 Litematica 一致）
    static i32  ReadBits(const std::vector<i64>& arr, size_t index, int bits);
    static void WriteBits(std::vector<i64>& arr, size_t index, int bits, i32 value);
    static int  BitsForPalette(size_t paletteSize);
    static std::vector<i64> EncodeIndices(const std::vector<i32>& indices, int bits);

private:
    bool FromNbt(const nbt::Value& root);
    bool SyncToNbt();

    nbt::Value root_;
    std::vector<Region> regions_;
    std::wstring path_;
    bool loaded_ = false;
    bool modified_ = false;
};

// dataVersion -> 游戏版本名
std::string MinecraftVersionFromDataVersion(i32 dv);
const std::vector<std::pair<i32, std::string>>& KnownDataVersions();

} // namespace ltm
