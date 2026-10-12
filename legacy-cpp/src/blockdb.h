#pragma once
#include "common.h"
#include "litematic.h"

// 内置多版本方块资源数据库（blocks.dat）
// 由 tools/pack_blockdb.py 生成，通过资源段嵌入 EXE，运行时完全脱离 Minecraft 本体。
namespace mc {

// 面序：0=down 1=up 2=north 3=south 4=west 5=east
enum FaceId { F_DOWN = 0, F_UP, F_NORTH, F_SOUTH, F_WEST, F_EAST, F_COUNT };
const char* FaceName(int f);

struct ModelFace {
    u32 texStrIdx = 0xFFFFFFFF;   // 贴图路径字符串索引；0xFFFFFFFF = 缺失
    float uv[4] = {0, 0, 16, 16}; // 方块模型 0..16 空间
    u8  cullface = 0xFF;          // 0xFF = 无
    i16 tintindex = -1;
    u16 rotation = 0;             // 0/90/180/270
};

struct ModelElement {
    float from[3] = {0, 0, 0};
    float to[3]   = {16, 16, 16};
    bool  hasRot = false;
    u8    rotKind = 0;            // 0=axis/angle 1=multiaxis
    float rotOrigin[3] = {0, 0, 0};
    u8    axis = 1;
    float angle = 0;
    bool  rescale = false;
    float rx = 0, ry = 0, rz = 0;
    u8    faceMask = 0;
    std::vector<ModelFace> faces; // 顺序对应 faceMask 中自低到高置位的面
    const ModelFace* Face(int f) const;
};

struct Model {
    bool translucent = false;
    std::vector<ModelElement> elements;
};

// 变体 / multipart 条目
//   variants : when 形如 "facing=east,half=bottom"（逗号=AND），空串=无条件
//   multipart: when 形如 "a=1,b=2|c=3"（竖线=OR，逗号=AND），空串=无条件
struct VariantEntry {
    std::string when;
    u32 modelIdx = 0;
    u8  rotX = 0;
    u8  rotY = 0;
    bool uvlock = false;
};

struct BlockDef {
    std::string name;
    std::vector<VariantEntry> variants;
    std::vector<VariantEntry> multipart;
    // 由 variants/multipart 推导出的属性值集合：prop -> 出现过的值（按首次出现顺序）
    std::vector<std::pair<std::string, std::vector<std::string>>> props;
    const std::vector<std::string>* PropValues(const std::string& p) const;
    std::string DefaultProp(const std::string& p) const;
};

struct TextureInfo {
    u16 w = 0, h = 0;
    u32 offset = 0, len = 0;
    bool hasAlpha = false;
};

struct LangEntry { std::string blockName, en, zh; };

struct VersionDef {
    std::string id;
    i32 dataVersion = 0;
    u32 langIdx = 0;
    bool isOverlay = false;          // true = 模组叠加包（叠加到同 dataVersion 的基础版本上）
    std::string packName;            // 来自哪个资源包
    std::unordered_map<std::string, u32> textureByPath;  // "block/stone" -> 贴图池索引
    std::unordered_map<std::string, u32> blockByName;    // "minecraft:stone" -> blocks 下标
    std::vector<BlockDef> blocks;
};

// 资源包元信息
struct PackInfo {
    std::wstring file;           // 磁盘上的 .pmp（内置基础库为空）
    std::string  name;           // 显示名
    std::string  source;         // 来源 jar 路径
    int          kind = 0;       // 0=内置基础库 1=MC 版本包 2=模组包
    i32          dataVersion = 0;
    int          versionIdx = -1;   // 合并后在 versions_ 中的下标
    size_t       blocks = 0, textures = 0, models = 0;
    u64          bytes = 0;
    bool         enabled = true;
};

class BlockDB : public ltm::IBlockInfo {
public:
    BlockDB() {}
    ~BlockDB() override {}

    bool LoadFromMemory(const u8* data, size_t size, std::string& err);
    bool LoadFromFile(const std::wstring& path, std::string& err);
    bool LoadEmbedded(int resId, std::string& err);

    // ---- 外部资源包（模组 / 新版本） ----
    // 把另一个数据库（1 个版本）的内容合并进来；info 描述这个包
    bool MergeFrom(const BlockDB& other, const PackInfo& info, std::string& err);
    // 从 .pmp 字节加载一个资源包并合并
    bool LoadPackBytes(const u8* data, size_t size, const PackInfo& info, std::string& err);
    // 扫描目录下的全部 .pmp 并按 manifest 里的启用状态加载
    bool LoadUserPacks(const std::wstring& dir, std::string& err);

    const std::vector<PackInfo>& Packs() const { return packs_; }
    std::vector<PackInfo>& PacksMutable() { return packs_; }

    // 查找贴图：优先本版本，其次叠加包，最后全局
    u32 FindTexture(const VersionDef& v, const std::string& path) const;
    // 该方块是否由某个模组包提供
    bool IsFromPack(const std::string& blockName) const;
    // 按 dataVersion 找一个模组包的叠加目标版本下标（没有则 -1）
    int FindBaseVersion(i32 dataVersion) const;

    bool Empty() const { return versions_.empty(); }
    size_t VersionCount() const { return versions_.size(); }
    const VersionDef& Version(size_t i) const { return versions_[i]; }
    const std::vector<VersionDef>& Versions() const { return versions_; }

    // 选最接近 dataVersion 的版本包（按绝对差）
    int PickVersion(i32 dataVersion) const;

    const Model& ModelAt(u32 idx) const;
    const TextureInfo& TextureAt(u32 idx) const;
    // 整个贴图像素缓冲的基址（RGBA8888）；单个贴图的数据在 base + TextureInfo.offset
    const u8* PixelBase() const { return texturePixels_.data(); }
    const std::string& Str(u32 idx) const;
    const LangEntry& Lang(const VersionDef& v, const std::string& blockName) const;
    const LangEntry* FindLang(u32 langIdx, const std::string& blockName) const;

    // 中文/英文显示名；优先用指定版本的语言表，缺失时回退到其它版本
    std::string DisplayName(const std::string& blockName, bool chinese,
                            const VersionDef* prefer) const;

    // 解析一个方块状态对应的模型（multipart 可能返回多个）
    struct ResolvedModel {
        const Model* model = nullptr;
        u8 rotX = 0, rotY = 0;
        bool uvlock = false;
    };
    void ResolveModels(const VersionDef& v, const ltm::BlockState& st,
                       std::vector<ResolvedModel>& out) const;

    // 该方块是否有可用几何体
    bool HasGeometry(const VersionDef& v, const std::string& blockName) const;

    // 全部方块名（用于"方块替换"下拉列表）
    const std::vector<std::string>& AllBlockNames() const { return allBlockNames_; }

    // IBlockInfo 实现
    bool SupportsProp(const std::string& block, const std::string& prop,
                      const std::string& value) const override;
    bool DefaultProps(const std::string& block,
                      std::vector<std::pair<std::string, std::string>>& out) const override;

    // 由外部指定"当前版本"用于 IBlockInfo（替换方块时用）
    void SetActiveVersion(int idx) { activeVersion_ = idx; }
    int  ActiveVersion() const { return activeVersion_; }

private:
    std::vector<VersionDef> versions_;
    std::vector<Model> models_;
    std::vector<TextureInfo> textures_;
    std::vector<std::string> strings_;
    std::vector<std::vector<LangEntry>> langs_;
    std::vector<u8> texturePixels_;
    std::vector<std::string> allBlockNames_;
    int activeVersion_ = 0;
    std::vector<PackInfo> packs_;
    // 模组叠加：versionIdx -> 该版本上的叠加包版本下标
    std::vector<std::vector<u32>> overlaysByVersion_;
    // 全局贴图路径索引（跨所有版本，用于模组贴图回退）
    std::unordered_map<std::string, u32> globalTextureByPath_;
    // ResolveModels 的缓存：键 = 版本号 + 完整方块状态
    mutable std::unordered_map<std::string, std::vector<ResolvedModel>> resolveCache_;
    // 语言表索引（与 langs_ 一一对应）
    std::vector<std::unordered_map<std::string, u32>> langIndex_;
    void RebuildDerived();
};

} // namespace mc
