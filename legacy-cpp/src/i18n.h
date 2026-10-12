#pragma once
#include "common.h"

// 双语界面：中文 / 英文。启动时按系统语言自动选择。
namespace i18n {

enum Lang { LANG_ZH = 0, LANG_EN = 1 };

void SetLang(Lang l);
Lang GetLang();
void DetectFromSystem();           // 读取 GetUserDefaultUILanguage
bool IsChineseSystem();

// 取字符串（不存在的 key 返回 key 本身，便于发现遗漏）
const wchar_t* T(const char* key);

} // namespace i18n
