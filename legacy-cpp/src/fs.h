#pragma once
#include "common.h"

struct FileInfo {
    std::wstring path;      // 完整路径
    std::wstring name;      // 文件名（含扩展名）
    std::wstring dir;       // 所在目录
    u64          size = 0;
    FILETIME     mtime = {};
};

namespace fsx {

bool FileExists(const std::wstring& path);
bool DirExists(const std::wstring& path);
bool EnsureDir(const std::wstring& path);
bool EnsureDirForFile(const std::wstring& filePath);

std::wstring GetExePath();
std::wstring GetFileName(const std::wstring& path);
std::wstring GetFileStem(const std::wstring& path);
std::wstring GetDirectoryName(const std::wstring& path);
std::wstring JoinPath(const std::wstring& a, const std::wstring& b);
std::wstring GetExtensionLower(const std::wstring& path);

// 递归枚举指定扩展名的文件
void ListFilesRecursive(const std::wstring& dir, const std::wstring& extLower,
                        std::vector<FileInfo>& out, int maxDepth = 12);

// 读取/写入
bool ReadAllBytes(const std::wstring& path, std::vector<u8>& out);
bool WriteAllBytes(const std::wstring& path, const void* data, size_t size);
bool WriteAllBytes(const std::wstring& path, const std::vector<u8>& data);

// 复制 / 删除 / 移动
bool CopyFileTo(const std::wstring& src, const std::wstring& dst, bool overwrite = true);
bool DeleteFileAt(const std::wstring& path);
bool MoveFileTo(const std::wstring& src, const std::wstring& dst, bool overwrite = true);

// 时间
std::wstring FormatFileTime(const FILETIME& ft);
u64 FileTimeToMillis(const FILETIME& ft);

// 目录文件枚举（不递归）
void ListDir(const std::wstring& dir, std::vector<FileInfo>& out, bool dirsOnly = false);

// 诊断日志：追加写入 EXE 同级的 diag.log（UTF-8），用于排查无控制台的 GUI 问题
void DiagLog(const std::string& line);
} // namespace fsx
