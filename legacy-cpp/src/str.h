#pragma once
#include "common.h"

// UTF-8 <-> UTF-16 转换（不依赖 ATL）
std::wstring Utf8ToWide(const std::string& s);
std::string  WideToUtf8(const std::wstring& s);

// 数字/格式化辅助
std::wstring FormatBytes(u64 bytes);
std::wstring FormatInt(i64 v);
std::wstring FormatFloat(double v, int decimals);

// 字符串工具
std::wstring ToLowerW(std::wstring s);
bool EndsWithNoCase(const std::wstring& s, const std::wstring& suffix);
bool EqualsNoCase(const std::wstring& a, const std::wstring& b);
std::wstring TrimW(const std::wstring& s);
std::vector<std::wstring> SplitW(const std::wstring& s, wchar_t sep);
std::wstring JoinW(const std::vector<std::wstring>& parts, const std::wstring& sep);
std::wstring ReplaceAllW(std::wstring s, const std::wstring& from, const std::wstring& to);
