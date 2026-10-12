#pragma once
#include "common.h"

// 备份与操作日志：任何写操作前先备份，多次修改多次备份
namespace backup {

struct Entry {
    std::wstring backupPath;   // 备份文件完整路径
    std::wstring origPath;     // 原文件完整路径
    std::wstring origName;     // 原文件名
    std::wstring op;           // 操作名（已本地化）
    i64          timeMs = 0;   // 备份时间（Unix 毫秒）
    u64          size = 0;
};

void Init();                                     // 建立备份目录（EXE 同级 backups）
const std::wstring& BaseDir();
std::wstring BackupRoot();

// 备份一个文件；op 为操作描述。成功返回备份路径
bool CreateBackup(const std::wstring& srcFile, const std::wstring& op,
                  std::wstring& outPath, std::string& err);

// 枚举全部备份（按时间倒序）
std::vector<Entry> List();

bool Restore(const Entry& e, std::string& err);   // 还原（会先备份当前文件）
bool DeleteBackup(const Entry& e, std::string& err);

// 每个原文件最多保留 keep 份，返回清理数量
int PrunePerFile(int keep);

// 操作日志
void Log(const std::wstring& file, const std::wstring& op, const std::wstring& detail);
std::vector<std::wstring> ReadLog(int maxLines = 500);

} // namespace backup
