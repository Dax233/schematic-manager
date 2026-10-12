#pragma once
#include "common.h"
#include "packbuild.h"

// 资源包管理：本机 Minecraft 安装探测 + 资源包清单（packs.tsv）
namespace packmgr {

// ---------------- 本机 Minecraft 安装探测 ----------------
struct Install {
    std::wstring mcDir;        // .minecraft 目录
    std::string  versionId;    // versions 目录名 / version.json 的 id
    std::string  type;         // release / snapshot / ...
    i32          dataVersion = 0;
    std::wstring jarPath;
    u64          jarSize = 0;
};

// 扫描根目录（默认：各盘符根目录 + %APPDATA%），寻找 .minecraft
// extraRoots 为设置里保存的额外扫描根
void ScanInstalls(std::vector<Install>& out,
                  const std::vector<std::wstring>& extraRoots,
                  int maxDepth = 4);

// 只保留正式版
void FilterReleases(const std::vector<Install>& in, std::vector<Install>& out);

// ---------------- 资源包清单 ----------------
struct ManifestEntry {
    bool         enabled = true;
    std::wstring file;          // packs 目录下的 .pmp 文件名
    std::string  name;          // 显示名
    std::string  source;        // 来源 jar 路径
    int          kind = 1;      // 1 = MC 版本包  2 = 模组叠加包
    i32          dataVersion = 0;
};

std::wstring PacksDir();                 // <exeDir>\packs
std::wstring ScannedJarsDir();           // <exeDir>\packs\sources（默认扫描/用户浏览目录）

std::vector<ManifestEntry> LoadManifest();
bool SaveManifest(const std::vector<ManifestEntry>& list);

// 导入一个 jar -> packs/<file>.pmp，并写入清单
bool ImportJar(const std::wstring& jarPath, int kind, i32 dataVersion,
               const std::string& name, std::string& err,
               packbuild::Result* info = nullptr);

// 从清单移除（并删除文件）
bool RemovePack(const std::string& name, bool deleteFile);

// 清单里已经导入过的 dataVersion 集合（用于判断"新版本"）
std::set<i32> ImportedDataVersions();

// 额外的扫描根（保存到 settings）
std::vector<std::wstring> LoadScanRoots();
bool SaveScanRoots(const std::vector<std::wstring>& roots);

// 自动导入开关
bool AutoImportEnabled();
void SetAutoImportEnabled(bool v);

// 备份/日志共用的应用数据目录
std::wstring DataDir();

// 投影扫描目录列表
struct DirEntry { std::wstring path; bool enabled = true; };
std::vector<DirEntry> LoadDirs();
void SaveDirs(const std::vector<DirEntry>& dirs);
std::vector<std::wstring> LoadRecent();
std::wstring LoadLastDir();
void SaveLastDir(const std::wstring& dir);
std::wstring LoadLastExportDir();
float LoadBrightness();
int   LoadExportSize();
void  SaveExportSize(int v);
void  SaveBrightness(float b);
void SaveLastExportDir(const std::wstring& dir);
void SaveRecent(const std::vector<std::wstring>& recent);

} // namespace packmgr
