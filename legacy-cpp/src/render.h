#pragma once
#include "common.h"
#include "litematic.h"
#include "blockdb.h"

// 软件光栅化 3D 预览渲染器（不依赖 GPU / 不依赖游戏）
namespace render {

struct Image {
    int w = 0, h = 0;
    std::vector<u32> px;    // ARGB, 行优先

    void Resize(int width, int height, u32 fill = 0x00000000);
    bool Inside(int x, int y) const { return x >= 0 && y >= 0 && x < w && y < h; }
    u32& At(int x, int y) { return px[(size_t)y * (size_t)w + (size_t)x]; }
    const u32& At(int x, int y) const { return px[(size_t)y * (size_t)w + (size_t)x]; }
};

enum ViewPreset { VIEW_FRONT = 0, VIEW_LEFT45, VIEW_RIGHT45, VIEW_BACK, VIEW_PRESET_COUNT };

// 中文视角名在 i18n 层给出；这里给英文/内部名
const char* ViewIdName(ViewPreset v);

struct Options {
    int   size = 200;            // 输出图片边长
    int   view = VIEW_FRONT;     // 预设视角
    float yawOverride = -9999;   // 若不为 -9999 则使用自定义 yaw
    float pitch = 30.0f;
    u32   bg = 0xFFFFFFFF;       // 背景色（ARGB）
    bool  transparentBg = false;
    int   supersample = 2;       // 超采样倍数（>=1），提升小图质量
};

bool RenderSchematic(const ltm::Schematic& sc, const mc::BlockDB& db, int versionIdx,
                     const Options& opt, Image& out, std::string& err);

// 单方块图标（方块列表用），以等轴测视角渲染
bool RenderBlockIcon(const mc::BlockDB& db, int versionIdx, const ltm::BlockState& st,
                     int size, Image& out, bool transparentBg = true);

// 用一个"紫黑格"贴图表示缺失
u32 MissingTextureColor(int x, int y);

bool SavePng(const std::wstring& path, const Image& img);
void SetBrightness(float b);
// 取某个预设视角的 yaw/pitch（界面按钮跳转用）
void GetViewAngles(int preset, float& yaw, float& pitch);   // 全局亮度系数，1.0 = 原始
float GetBrightness();

} // namespace render
