#pragma once
// PNG 解码器（只读）——零第三方依赖，仅使用 zlib 解 IDAT
// 支持: bit depth 1/2/4/8/16, color type 0/2/3/4/6, 5 种 filter, tRNS
// 不支持: Adam7 交错（返回明确错误）、sBIT/gAMA 等辅助块的精细处理（忽略）
#include "common.h"

namespace png {

struct Image {
    int w = 0, h = 0;
    std::vector<u8> rgba;      // 长度 = w*h*4，顺序 R,G,B,A
    bool Empty() const { return w <= 0 || h <= 0; }
};

// 解码为 RGBA8888
bool Decode(const u8* data, size_t size, Image& out, std::string& err);

// 解码**动画贴图的第一帧**：Minecraft 的动画贴图是纵向多帧拼接（w x (h*frames)），
// 该函数在解码后裁剪为顶部 w x w 的正方形（若 h < w 则退回整幅）。
bool DecodeFirstFrame(const u8* data, size_t size, Image& out, std::string& err);

// 极简 PNG 编码器：color type 6 / bit depth 8 / filter 0
bool Encode(const Image& img, std::vector<u8>& out);

} // namespace png
