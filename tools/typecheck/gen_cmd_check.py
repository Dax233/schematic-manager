"""为命令层生成可离线编译的检查副本。

命令层依赖 tauri 与 serde 的过程宏，而过程宏无法用普通 Rust 源码替代，
因此这里把宏属性剥掉，只保留函数体与类型定义——**风险恰恰在函数体**
（跨模块接口调用、DTO 转换），那部分剥掉属性后仍会被完整检查。

被剥掉的内容：
  #[tauri::command]            -> 删除
  #[serde(...)]                -> 删除
  #[derive(... Serialize, Deserialize ...)] -> 去掉这两个名字，保留其余派生

用法：python gen_cmd_check.py
"""

import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "..", "..", "src-tauri", "src")
OUT = os.path.join(HERE, "generated")


def strip_macros(text: str) -> str:
    # #[tauri::command] 与 #[tauri::command(...)]
    text = re.sub(r"^[ \t]*#\[tauri::command[^\]]*\]\s*\n", "", text, flags=re.M)
    # #[serde(...)] 可能跨行
    text = re.sub(r"^[ \t]*#\[serde\([^\n]*?\)\]\s*\n", "", text, flags=re.M)
    # #[cfg_attr(mobile, tauri::mobile_entry_point)] 之类
    text = re.sub(r"^[ \t]*#\[cfg_attr\(mobile[^\n]*?\)\]\s*\n", "", text, flags=re.M)

    # derive 里去掉 Serialize / Deserialize（这两个 trait 由 serde 提供）
    def fix_derive(m: "re.Match") -> str:
        inner = m.group(1)
        parts = [p.strip() for p in inner.split(",")]
        keep = [p for p in parts if p not in ("Serialize", "Deserialize")]
        if not keep:
            return ""
        return "#[derive(" + ", ".join(keep) + ")]"

    text = re.sub(r"#\[derive\(([^)]*)\)\]", fix_derive, text)
    return text


def main() -> None:
    os.makedirs(OUT, exist_ok=True)
    for name in ("commands.rs", "dto.rs"):
        src = os.path.join(SRC, name)
        with open(src, encoding="utf-8") as f:
            text = f.read()
        out = strip_macros(text)
        # 记录剥掉了多少，便于发现规则失效（规则失效会导致检查悄悄退化）
        removed = text.count("#[tauri::command]") + text.count("#[serde(")
        with open(os.path.join(OUT, name), "w", encoding="utf-8", newline="\n") as f:
            f.write(out)
        print("%-12s 剥离属性 %d 处 -> generated/%s" % (name, removed, name))


if __name__ == "__main__":
    main()
