#!/usr/bin/env python3
"""为 tests/test_zip_png.cpp 生成参考数据与测试样本。

用法（使用捆绑 Python，Pillow 已就绪）：
    python tools/gen_zip_png_refs.py

产物（全部落在 projmgr/build/_zip_png_test/ 下）：
    pillow_all_png.txt   游戏 jar 内每一张 PNG 的 "名字<TAB>宽<TAB>高<TAB>RGBA文本MD5"
    zip64_forced.zip     Python 强制 ZIP64 的归档（尺寸放在 ZIP64 扩展字段里）
    interlaced.png       把 stone.png 的 IHDR 交错位置 1 后的样本
    badcrc.png           IDAT 的 CRC 被改坏、数据完好的样本
    truncated.png        截断的 PNG 样本

测试代码在参考文件缺失时会自动跳过对应用例，不会失败。
"""
import hashlib
import io
import os
import struct
import zipfile
import zlib

try:
    from PIL import Image
except ImportError:  # pragma: no cover
    raise SystemExit("需要 Pillow（使用捆绑 Python 运行本脚本）")

GAME_JAR = r"E:\游戏\@\26.2F\.minecraft\versions\26.2-Fabric\26.2-Fabric.jar"
OUT_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                       "build", "_zip_png_test")


def rgba_text_md5(im):
    """RGBA 逐像素文本的 MD5 —— 必须与 C++ 侧 RgbaToText 完全一致。"""
    payload = "".join("%d,%d,%d,%d\n" % px for px in im.convert("RGBA").getdata()).encode()
    return hashlib.md5(payload).hexdigest()


def gen_pillow_ref():
    with zipfile.ZipFile(GAME_JAR) as z:
        pngs = [n for n in z.namelist() if n.lower().endswith(".png")]
        lines = []
        for n in pngs:
            im = Image.open(io.BytesIO(z.read(n)))
            lines.append("%s\t%d\t%d\t%s" % (n, im.size[0], im.size[1], rgba_text_md5(im)))
    path = os.path.join(OUT_DIR, "pillow_all_png.txt")
    with open(path, "w", encoding="utf-8", newline="\r\n") as f:
        f.write("\n".join(lines) + "\n")
    print("写出 %s（%d 张 PNG）" % (path, len(lines)))


def gen_zip64():
    path = os.path.join(OUT_DIR, "zip64_forced.zip")
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED, allowZip64=True) as z:
        with z.open(zipfile.ZipInfo("data/hello.json"), "w", force_zip64=True) as f:
            f.write(b'{"hello":"zip64","n":123}\n' * 10)
        z.writestr("data/plain.txt", "plain-text-body")
    print("写出 %s（force_zip64 归档）" % path)


def gen_bad_pngs():
    with zipfile.ZipFile(GAME_JAR) as z:
        src = z.read("assets/minecraft/textures/block/stone.png")

    # 交错：IHDR 第 13 字节置 1，并修好 CRC
    d = bytearray(src)
    assert d[12:16] == b"IHDR"
    d[28] = 1
    struct.pack_into(">I", d, 29, zlib.crc32(bytes(d[12:29])) & 0xFFFFFFFF)
    p = os.path.join(OUT_DIR, "interlaced.png")
    open(p, "wb").write(bytes(d))
    print("写出 %s（IHDR interlace=1）" % p)

    # IDAT 的 CRC 改坏，数据完好
    d2 = bytearray(src)
    i = d2.find(b"IDAT")
    length = struct.unpack(">I", d2[i - 4:i])[0]
    struct.pack_into(">I", d2, i + 4 + length, 0xDEADBEEF)
    p2 = os.path.join(OUT_DIR, "badcrc.png")
    open(p2, "wb").write(bytes(d2))
    print("写出 %s（IDAT CRC 损坏）" % p2)

    # 截断
    p3 = os.path.join(OUT_DIR, "truncated.png")
    open(p3, "wb").write(bytes(d2[:-20]))
    print("写出 %s（截断 20 字节）" % p3)


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    if not os.path.isfile(GAME_JAR):
        raise SystemExit("找不到游戏 jar: %s" % GAME_JAR)
    gen_pillow_ref()
    gen_zip64()
    gen_bad_pngs()
    print("完成。")


if __name__ == "__main__":
    main()
