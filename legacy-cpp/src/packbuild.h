#pragma once
#include "common.h"

// 从 Minecraft 客户端 jar / 模组 jar 构建内部资源包（blocks.dat 同格式，单版本）
namespace packbuild {

struct Result {
    std::vector<u8> data;          // 生成的资源包字节（若 useGzip 则已压缩）
    std::string versionId;         // MC 版本 id（模组包为 ""）
    std::string packName;          // 建议的包名
    i32 dataVersion = 0;
    size_t blockCount = 0;
    size_t modelCount = 0;
    size_t textureCount = 0;
    size_t failedModels = 0;
    size_t namespaces = 0;
    std::string error;
    std::string warning;           // 非致命问题汇总
};

// kind: 1 = MC 版本包  2 = 模组叠加包
// forcedDataVersion: 模组包必须显式给（表示要叠加到哪个游戏版本）；<=0 时从 jar 的 version.json 读
bool BuildFromJar(const std::wstring& jarPath,
                  const std::string& packName,
                  int kind,
                  i32 forcedDataVersion,
                  Result& out);

} // namespace packbuild
