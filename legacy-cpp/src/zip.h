#pragma once
// ZIP 读取器（只读）——零第三方依赖，仅使用 zlib 解 deflate
// 用法:
//   zip::Archive a;
//   std::string err;
//   if (a.Open(L"foo.jar", err)) { ... a.ReadText("manifest.json", s, err); }
#include "common.h"

namespace zip {

struct Entry {
    std::string name;              // 条目路径，UTF-8，正斜杠
    u32 compressedSize = 0;
    u32 uncompressedSize = 0;
    u32 crc32 = 0;
    u16 method = 0;                // 0=stored 8=deflate
    u32 localHeaderOffset = 0;
    u16 flags = 0;                 // 通用位标志（bit11 = UTF-8 名称）
    u32 dosTime = 0;               // MS-DOS 时间（低16位日期，高16位时间）

    // DOS 时间转 "YYYY-MM-DD HH:MM:SS"（ZIP 不带时区，仅作展示）
    std::string TimeString() const;
};

class Archive {
public:
    // 从文件打开（内部一次性读入内存）
    bool Open(const std::wstring& path, std::string& err);
    // 从内存打开（用于嵌套 jar，例如 META-INF/jars/*.jar）
    bool OpenMemory(const u8* data, size_t size, std::string& err);
    void Close();
    bool Ok() const { return ok_; }

    const Entry* Find(const std::string& name) const;   // 精确匹配
    bool Has(const std::string& name) const;
    // 列出条目名；prefix 非空时按前缀过滤
    void List(std::vector<std::string>& out, const std::string& prefix = std::string()) const;
    // 解出条目内容（自动处理 stored / deflate）
    bool Read(const std::string& name, std::vector<u8>& out, std::string& err) const;
    // 便捷：解出并当 UTF-8 文本返回
    bool ReadText(const std::string& name, std::string& out, std::string& err) const;
    size_t Count() const { return entries_.size(); }

    // 全部条目（只读）
    const std::vector<Entry>& Entries() const { return entries_; }

    // 诊断信息：最近一次操作产生的非致命警告（CRC 不符、非 ASCII 路径等）
    // 取出后清空。
    std::string TakeWarnings() const;

private:
    void Warn(const std::string& w) const;

    std::vector<u8> data_;
    std::vector<Entry> entries_;
    std::unordered_map<std::string, size_t> index_;
    bool ok_ = false;
    std::wstring srcPath_;
    mutable std::vector<std::string> warnings_;
};

// 底层工具（供测试与嵌套使用）
u32 Crc32(const u8* data, size_t size);

} // namespace zip
