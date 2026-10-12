#pragma once
// 投影文件管理器 - 公共定义
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#ifndef UNICODE
#define UNICODE
#endif
#ifndef _UNICODE
#define _UNICODE
#endif

#include <windows.h>
#include <cstdint>
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>
#include <map>
#include <set>
#include <unordered_map>
#include <unordered_set>
#include <algorithm>
#include <memory>
#include <functional>

typedef uint8_t  u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;
typedef int8_t   i8;
typedef int16_t  i16;
typedef int32_t  i32;
typedef int64_t  i64;

#define APP_NAME_EN  L"Schematic Manager"
#define APP_NAME_ZH  L"投影文件管理器"
#define APP_AUTHOR   L"bilibili\u9ed1\u5c71\u5927\u53d4"
#define APP_VERSION  L"1.0.0"

// 扁平化配色（ARGB）
namespace Theme {
    const COLORREF Border        = RGB(0x8A, 0x8A, 0x8A); // 2px 灰色边框
    const COLORREF BgWindow      = RGB(0xF5, 0xF6, 0xF7);
    const COLORREF BgPanel       = RGB(0xFF, 0xFF, 0xFF);
    const COLORREF BgHeader      = RGB(0xEC, 0xEF, 0xF1);
    const COLORREF BgListRow     = RGB(0xFF, 0xFF, 0xFF);
    const COLORREF BgListRowAlt  = RGB(0xF7, 0xF8, 0xFA);
    const COLORREF BgListSel     = RGB(0xDC, 0xE8, 0xF6);
    const COLORREF BgHover       = RGB(0xE9, 0xF1, 0xFB);
    const COLORREF Accent        = RGB(0x2F, 0x6F, 0xED);
    const COLORREF Text          = RGB(0x20, 0x24, 0x28);
    const COLORREF TextDim       = RGB(0x77, 0x7D, 0x84);
    const COLORREF TextOnAccent  = RGB(0xFF, 0xFF, 0xFF);
    const COLORREF Separator     = RGB(0xDD, 0xE0, 0xE3);
    const COLORREF BtnFace       = RGB(0xF2, 0xF3, 0xF5);
    const COLORREF BtnFaceHot    = RGB(0xE6, 0xEA, 0xEF);
    const COLORREF BtnFaceDown   = RGB(0xD8, 0xDE, 0xE5);
    const COLORREF CloseHot      = RGB(0xE8, 0x11, 0x23);
    const COLORREF Danger        = RGB(0xC4, 0x2B, 0x1C);
}

inline int ColorDistance(COLORREF a, COLORREF b) {
    int dr = (int)GetRValue(a) - (int)GetRValue(b);
    int dg = (int)GetGValue(a) - (int)GetGValue(b);
    int db = (int)GetBValue(a) - (int)GetBValue(b);
    return dr * dr + dg * dg + db * db;
}
