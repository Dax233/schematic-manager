#pragma once
// 测试公共设施（多测试文件共享的计数器与断言）
#include <cstdio>

inline int g_pass = 0;
inline int g_fail = 0;

inline void Check(bool cond, const char* what) {
    if (cond) { ++g_pass; }
    else { ++g_fail; printf("    !! 失败: %s\n", what); }
}

// ZIP / PNG 验证入口（tests/test_zip_png.cpp）
void TestZipPng();
