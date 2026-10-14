#!/usr/bin/env python3
"""把各版本 client jar 里的实体贴图打包成 entities.dat / entities.dat.z。

与 pack_blockdb.py 的分工：那个打方块模型与方块贴图（blocks.dat.z），
这个只打 `assets/minecraft/textures/entity/**`。分成两个包是因为两者用途
不同——方块贴图要拼进图集供模型采样，实体贴图是按名字单独取用的整张图。

容器格式（小端）：

    magic            b"SMET"
    u32  format_version         当前为 1
    u32  string_count
        重复：u16 len + utf8 字节
    u32  texture_count
        重复：u16 w, u16 h, u32 offset, u32 len, u8 has_alpha
    u32  raw_size               像素池解压后大小
    u32  comp_size
        重复：comp_size 字节（zlib 压缩的像素池）
    u32  version_count
        重复：
          i32 data_version
          u16 name_len + utf8 版本名
          u32 tex_count
            重复：u32 string_idx, u32 pool_idx

像素池是一整块 RGBA8888，按内容去重后拼接；每个贴图用 offset/len 引用它。
去重的收益很大：实测 49 个版本累计 18799 张贴图共 345.6 MB（原始 RGBA），
按内容去重后只剩 1366 张、24.4 MB——实体贴图跨版本几乎不变。
"""
import argparse
import hashlib
import io
import json
import os
import struct
import sys
import zipfile
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

ENTITY_PREFIX = "assets/minecraft/textures/entity/"
FORMAT_VERSION = 1
MAGIC = b"SMET"


def die(msg):
    print("[error] %s" % msg, file=sys.stderr)
    sys.exit(2)


def norm_path(name):
    """把 jar 内的完整路径压成贴图引用名。

    去掉 `assets/minecraft/textures/` 前缀与扩展名，结果形如
    `entity/chest/normal`——与方块贴图的 `block/stone` 同一套写法，
    便于阅读与跨包一致。
    """
    rel = name[len("assets/minecraft/textures/"):]
    if rel.endswith(".png"):
        rel = rel[:-4]
    return rel


class StringTable(object):
    def __init__(self):
        self.items = []
        self.index = {}

    def add(self, s):
        i = self.index.get(s)
        if i is None:
            i = len(self.items)
            self.index[s] = i
            self.items.append(s)
        return i


def version_data(name):
    """从烘焙产物里取该版本的真实 data_version。

    版本目录名与 data_version 不是一回事（26.x 这类名字尤其），
    因此一律以 assets_baked.json 里的字段为准。
    """
    p = os.path.join(name[1], "assets_baked.json")
    if not os.path.isfile(p):
        return None
    try:
        j = json.load(open(p, encoding="utf-8"))
    except Exception as e:
        print("[warn] %s 解析失败: %s" % (p, e), file=sys.stderr)
        return None
    dv = j.get("data_version")
    return dv if isinstance(dv, int) else None


def collect_versions(ws):
    """返回 [(版本名, 版本目录, jar 路径)]，只含两边都存在的版本。"""
    vers_dir = os.path.join(ws, "data", "versions")
    jars_dir = os.path.join(ws, "cache", "mcjars")
    if not os.path.isdir(vers_dir):
        die("找不到版本目录：%s" % vers_dir)
    if not os.path.isdir(jars_dir):
        die("找不到 client jar 目录：%s" % jars_dir)

    out, skipped = [], []
    for name in sorted(os.listdir(vers_dir)):
        d = os.path.join(vers_dir, name)
        if not os.path.isdir(d):
            continue
        jar = os.path.join(jars_dir, "%s-client.jar" % name)
        if not os.path.isfile(jar):
            skipped.append(name)
            continue
        out.append((name, d, jar))
    return out, skipped


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ws", required=True, help="工作区根目录（含 data/ 与 cache/mcjars/）")
    ap.add_argument("--out", required=True, help="输出目录")
    args = ap.parse_args()

    from PIL import Image

    versions, skipped = collect_versions(args.ws)
    if not versions:
        die("没有任何版本同时具备烘焙产物与 client jar")
    if skipped:
        print("缺少 client jar、已跳过: %s" % ", ".join(skipped))

    strings = StringTable()
    textures = []            # (w, h, pool_offset, len, has_alpha)
    pool = bytearray()
    # 两级去重。第一级按 PNG 字节：多数版本之间贴图**字节完全相同**，
    # 命中即可跳过解码，省掉绝大部分解码开销。第二级按像素内容：
    # 同样的图案用不同 PNG 编码保存时字节不同，只靠第一级会重复存一份，
    # 而运行时要把整块像素池读进内存，重复是有代价的
    # （实测 35.7 MB -> 24.4 MB）。
    by_png = {}              # png 字节 sha1 -> 贴图池下标
    by_rgba = {}             # 像素内容 sha1 -> 贴图池下标

    ver_records = []
    total_png = 0
    for name, vdir, jar in versions:
        dv = version_data((name, vdir))
        if dv is None:
            print("[warn] %s 取不到 data_version，已跳过" % name)
            continue
        entries = {}
        with zipfile.ZipFile(jar) as z:
            names = sorted(n for n in z.namelist()
                           if n.startswith(ENTITY_PREFIX) and n.endswith(".png"))
            for n in names:
                raw = z.read(n)
                total_png += 1
                key = hashlib.sha1(raw).hexdigest()
                idx = by_png.get(key)
                if idx is None:
                    try:
                        im = Image.open(io.BytesIO(raw)).convert("RGBA")
                    except Exception as e:
                        print("[warn] %s 解码失败，已跳过: %s" % (n, e), file=sys.stderr)
                        continue
                    w, h = im.size
                    data = im.tobytes()
                    if len(data) != w * h * 4:
                        print("[warn] %s 像素数与尺寸不符，已跳过" % n, file=sys.stderr)
                        continue
                    # 第二级：同样的像素是否已经存过
                    ck = hashlib.sha1(data).hexdigest()
                    idx = by_rgba.get(ck)
                    if idx is None:
                        # 全不透明的贴图不需要逐像素 alpha，但仍按 RGBA 统一存放：
                        # 混用两种布局会让读取端多一条分支，收益不值这个复杂度。
                        has_alpha = any(data[i] != 255 for i in range(3, len(data), 4))
                        off = len(pool)
                        pool.extend(data)
                        idx = len(textures)
                        textures.append((w, h, off, len(data), 1 if has_alpha else 0))
                        by_rgba[ck] = idx
                    by_png[key] = idx
                entries[norm_path(n)] = idx
        ver_records.append((dv, name, entries))
        print("  %-10s dv=%-6s 贴图 %d 张" % (name, dv, len(entries)))

    if not textures:
        die("没有收集到任何实体贴图")

    ver_records.sort(key=lambda v: (v[0], v[1]))

    # ---- 序列化 ----
    out = bytearray()
    out += MAGIC
    out += struct.pack("<I", FORMAT_VERSION)

    path_of = {}
    for _, _, entries in ver_records:
        for p in entries:
            path_of.setdefault(p, strings.add(p))
    # 与版本无关地按名字排序写字符串表，保证输出可复现
    ordered_paths = sorted(path_of.keys())
    strings = StringTable()
    path_idx = {}
    for p in ordered_paths:
        path_idx[p] = strings.add(p)

    out += struct.pack("<I", len(strings.items))
    for s in strings.items:
        b = s.encode("utf-8")
        out += struct.pack("<H", len(b)) + b

    out += struct.pack("<I", len(textures))
    for w, h, off, ln, ha in textures:
        out += struct.pack("<HHIIB", w, h, off, ln, ha)

    comp = zlib.compress(bytes(pool), 9)
    out += struct.pack("<II", len(pool), len(comp)) + comp

    out += struct.pack("<I", len(ver_records))
    for dv, name, entries in ver_records:
        nb = name.encode("utf-8")
        out += struct.pack("<iH", dv, len(nb)) + nb
        out += struct.pack("<I", len(entries))
        for p in sorted(entries):
            out += struct.pack("<II", path_idx[p], entries[p])

    os.makedirs(args.out, exist_ok=True)
    dat = os.path.join(args.out, "entities.dat")
    dtz = os.path.join(args.out, "entities.dat.z")
    open(dat, "wb").write(out)
    z = zlib.compress(bytes(out), 9)
    open(dtz, "wb").write(z)

    print()
    print("实体贴图: jar 内 %d 张 -> 去重后 %d 张" % (total_png, len(textures)))
    print("版本数: %d" % len(ver_records))
    print("路径数: %d" % len(strings.items))
    print("像素池: %.1f MB 原始 -> %.1f MB zlib" % (len(pool) / 1048576.0, len(comp) / 1048576.0))
    print("entities.dat    : %.3f MB" % (len(out) / 1048576.0))
    print("entities.dat.z  : %.3f MB" % (len(z) / 1048576.0))
    return 0


if __name__ == "__main__":
    sys.exit(main())
