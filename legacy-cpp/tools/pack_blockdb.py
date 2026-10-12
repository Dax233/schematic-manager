#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
pack_blockdb.py -- 把多个 Minecraft 版本的方块资源（几何 + 贴图 + 中英文名）
合并成一个紧凑的单一二进制文件，供 C++ 侧 mc::BlockDB::LoadFromMemory() 读取。

用法:
    python pack_blockdb.py                 # 自动发现所有输入，写到 projmgr/data/
    python pack_blockdb.py --verbose
    python pack_blockdb.py --only 26.2     # 只打包指定版本（可重复）
    python pack_blockdb.py --out DIR

输入（两个 data 根都会被搜索，先 workspace/data 后 projmgr/data）:
    <root>/versions.json                             版本清单（可选）
    <root>/versions/<id>/assets_baked.json           主数据
    <root>/versions/<id>/atlas.bin                   RGBA8888 图集
    <root>/versions/<id>/atlas_meta.json             {"atlas_w","atlas_h","entries"}
    <root>/assets_baked.json                         单版本产物（无 version_id 字段则回退 mc_version）
    <root>/atlas.bin
    <root>/atlas_meta.json

输出:
    projmgr/data/blocks.dat
    projmgr/data/blocks.dat.z                         zlib(RFC1950) 压缩
    <workspace>/data/pack_report.json + projmgr/data/pack_report.json

依赖: 仅标准库 + numpy（不安装任何东西）。
"""

import argparse
import hashlib
import json
import os
import struct
import sys
import zlib

try:
    import numpy as np
except ImportError:  # pragma: no cover
    print('需要 numpy', file=sys.stderr)
    raise

MAGIC = b'PMBLKDB\0'
FORMAT_VERSION = 3
NO_STR = 0xFFFFFFFF

# 面序: 0=down 1=up 2=north 3=south 4=west 5=east
FACE_NAMES = ['down', 'up', 'north', 'south', 'west', 'east']
FACE_IDX = {n: i for i, n in enumerate(FACE_NAMES)}
AXIS_IDX = {'x': 0, 'y': 1, 'z': 2}
NO_CULL = 0xFF


# --------------------------------------------------------------------------
# 小工具
# --------------------------------------------------------------------------
def f32(v):
    """JSON 数值 -> float32。None/非法 -> 0.0"""
    if v is None:
        return 0.0
    try:
        return float(v)
    except (TypeError, ValueError):
        return 0.0


def pack_f3(out, a, b, c):
    out += struct.pack('<3f', f32(a), f32(b), f32(c))


def quarter_turns(v, where, warns):
    """blockstate 的 x/y 是 0/90/180/270 度。格式里 rotX/rotY 是 u8，装不下 270，
    所以按“90 度的倍数”存 0..3（C++ 用 rotX*90 还原角度）。"""
    try:
        d = float(v or 0)
    except (TypeError, ValueError):
        d = 0.0
    q = int(round(d / 90.0))
    if abs(d - q * 90.0) > 1e-6 or not (0 <= q <= 3):
        warns.append('%s 的旋转值 %r 不是 0/90/180/270，已按 %d*90 处理' % (where, v, q))
        q = max(0, min(3, q))
    return q & 0xFF


def norm_tex_path(p):
    """贴图路径规范化：去掉 minecraft: 前缀，与版本 textures 列表的键一致。"""
    if p is None:
        return None
    s = str(p).strip()
    if s.startswith('minecraft:'):
        s = s[len('minecraft:'):]
    return s


def norm_when_variant(w):
    """variants[].when: dict -> "a=1,b=2"（属性名字典序）；空/None -> ""。"""
    if not w:
        return ''
    if not isinstance(w, dict):
        return str(w)
    return ','.join('%s=%s' % (k, w[k]) for k in sorted(w.keys()))


def norm_when_multipart(w):
    """multipart[].when: OR 方案用 | 连接，方案内 AND 用 , 连接，条件名排序。
    空列表 -> ""。任一方案为空(即无条件) -> 整体视为无条件 ""。"""
    if not w:
        return ''
    if isinstance(w, dict):          # 防御：某些版本可能直接给 dict
        return norm_when_variant(w)
    plans = []
    for plan in w:
        if not plan:                 # 空方案 = 无条件
            return ''
        if isinstance(plan, dict):
            plans.append(norm_when_variant(plan))
            continue
        conds = []
        for c in plan:
            if isinstance(c, (list, tuple)) and len(c) >= 2:
                conds.append((str(c[0]), str(c[1])))
            elif isinstance(c, dict):
                conds.extend((str(k), str(v)) for k, v in c.items())
        conds.sort(key=lambda kv: kv[0])
        plans.append(','.join('%s=%s' % kv for kv in conds))
    return '|'.join(plans)


# --------------------------------------------------------------------------
# 字符串表
# --------------------------------------------------------------------------
class StringTable(object):
    def __init__(self):
        self._map = {}
        self._list = []
        self._bytes = 0

    def add(self, s):
        if s is None:
            return NO_STR
        s = str(s)
        i = self._map.get(s)
        if i is None:
            i = len(self._list)
            self._map[s] = i
            self._list.append(s)
            self._bytes += len(s.encode('utf-8')) + 1
        return i

    def idx(self, s):
        return self._map.get(s, NO_STR)

    def __len__(self):
        return len(self._list)

    def serialize(self):
        out = bytearray()
        out += struct.pack('<I', len(self._list))
        for s in self._list:
            out += s.encode('utf-8')
            out += b'\x00'
        return bytes(out)


# --------------------------------------------------------------------------
# 输入发现
# --------------------------------------------------------------------------
def find_data_roots(ws, proj):
    roots = []
    for r in (os.path.join(ws, 'data'), os.path.join(proj, 'data')):
        if os.path.isdir(r) and r not in roots:
            roots.append(r)
    return roots


def first_existing(roots, *rel):
    for r in roots:
        p = os.path.join(r, *rel)
        if os.path.isfile(p):
            return p
    return None


def discover_versions(roots, only=None):
    """返回 [(vid, dir_or_None, data_version_hint, source)]，按 (dataVersion, id) 排序。"""
    found = {}   # vid -> dict

    # 1) 目录扫描（versions.json 可能还在生成中）
    dirs = []
    for r in roots:
        vd = os.path.join(r, 'versions')
        if os.path.isdir(vd):
            dirs.append(vd)
    for vd in dirs:
        for name in sorted(os.listdir(vd)):
            sub = os.path.join(vd, name)
            if not os.path.isdir(sub):
                continue
            if not os.path.isfile(os.path.join(sub, 'assets_baked.json')):
                continue
            found.setdefault(name, {'vid': name, 'dir': sub, 'dv': None,
                                    'source': 'versions-dir'})

    # 2) versions.json 清单（补 dataVersion）
    for r in roots:
        mf = os.path.join(r, 'versions.json')
        if not os.path.isfile(mf):
            continue
        try:
            man = json.load(open(mf, encoding='utf-8'))
        except Exception as e:
            print('[warn] versions.json 解析失败 %s: %s' % (mf, e), file=sys.stderr)
            continue
        if isinstance(man, dict):
            man = man.get('versions') or []
        for ent in man:
            if not isinstance(ent, dict):
                continue
            vid = ent.get('id')
            if not vid:
                continue
            sub = None
            for vd in dirs:
                cand = os.path.join(vd, vid)
                if os.path.isfile(os.path.join(cand, 'assets_baked.json')):
                    sub = cand
                    break
            if vid in found:
                if found[vid]['dv'] is None:
                    found[vid]['dv'] = ent.get('dataVersion')
                found[vid]['manifest'] = ent
            elif sub:
                found[vid] = {'vid': vid, 'dir': sub, 'dv': ent.get('dataVersion'),
                              'source': 'versions.json', 'manifest': ent}
            else:
                # 清单里有但烘焙产物还没生成 -> 跳过
                found.setdefault('__missing__', {})
                found['__missing__'].setdefault('ids', []).append(vid)
        break

    # 3) 单版本产物（无 version_id 字段时用 mc_version）
    baked = first_existing(roots, 'assets_baked.json')
    standalone = None
    if baked:
        try:
            head = json.load(open(baked, encoding='utf-8'))
            vid = head.get('version_id') or head.get('mc_version') or 'unknown'
            dv = head.get('data_version')
        except Exception as e:
            print('[warn] %s 解析失败: %s' % (baked, e), file=sys.stderr)
            vid, dv = 'unknown', None
        standalone = {'vid': vid, 'dir': os.path.dirname(baked), 'dv': dv,
                      'source': 'standalone'}
        if vid not in found:
            found[vid] = standalone
        else:
            found[vid].setdefault('standalone_dir', os.path.dirname(baked))

    missing = found.pop('__missing__', {}).get('ids', [])
    ents = list(found.values())
    if only:
        want = set(only)
        ents = [e for e in ents if e['vid'] in want]
    ents.sort(key=lambda e: (e['dv'] if isinstance(e['dv'], int) else 1 << 30, e['vid']))
    return ents, missing


def load_version(ent):
    """读取一个版本的 assets_baked.json / atlas.bin / atlas_meta.json（幂等）。"""
    if 'json' in ent:
        return ent
    d = ent['dir']
    jp = os.path.join(d, 'assets_baked.json')
    ap = os.path.join(d, 'atlas.bin')
    mp = os.path.join(d, 'atlas_meta.json')
    for p in (jp, ap, mp):
        if not os.path.isfile(p):
            raise IOError('缺少输入文件: %s' % p)
    data = json.load(open(jp, encoding='utf-8'))
    meta = json.load(open(mp, encoding='utf-8'))
    raw = open(ap, 'rb').read()
    aw, ah = int(meta['atlas_w']), int(meta['atlas_h'])
    if len(raw) != aw * ah * 4:
        raise ValueError('%s 大小 %d != atlas %dx%d*4=%d'
                         % (ap, len(raw), aw, ah, aw * ah * 4))
    ent['json'] = data
    ent['meta'] = meta
    ent['atlas'] = np.frombuffer(raw, dtype=np.uint8).reshape(ah, aw, 4)
    if ent['dv'] is None:
        ent['dv'] = data.get('data_version')
    return ent


# --------------------------------------------------------------------------
# 贴图池
# --------------------------------------------------------------------------
class TexturePool(object):
    def __init__(self):
        self.by_sha = {}
        self.list = []       # [{'sha','w','h','data','hasAlpha'}]
        self.total_refs = 0

    def add(self, pixels, w, h):
        sha = hashlib.sha1(pixels).hexdigest()
        i = self.by_sha.get(sha)
        if i is None:
            i = len(self.list)
            self.by_sha[sha] = i
            self.list.append({'sha': sha, 'w': w, 'h': h, 'data': pixels,
                              'hasAlpha': bool(np.frombuffer(pixels, dtype=np.uint8)[3::4].min() < 255)})
        self.total_refs += 1
        return i

    def serialize(self):
        raw = bytearray()
        entries = bytearray()
        off = 0
        for t in self.list:
            ln = len(t['data'])
            entries += struct.pack('<HHI I B'.replace(' ', ''),
                                   t['w'], t['h'], off, ln, 1 if t['hasAlpha'] else 0)
            raw += t['data']
            off += ln
        blob = zlib.compress(bytes(raw), 9)
        out = bytearray()
        out += struct.pack('<I', len(self.list))
        out += entries
        out += struct.pack('<II', len(raw), len(blob))
        out += blob
        return bytes(out), len(raw), len(blob)


# --------------------------------------------------------------------------
# 主流程
# --------------------------------------------------------------------------
def build(ents, verbose=False):
    warns = []
    strings = StringTable()
    texpool = TexturePool()
    models = []            # [bytes]
    model_by_sha = {}
    langs = []             # [bytes]
    lang_by_sha = {}
    # 注意: 模型路径只在一个版本内唯一（同名模型在不同版本几何可能不同），
    # 所以查找缓存必须按版本隔离；跨版本去重靠下面的 sha1 内容去重。
    model_cache = {}       # (版本序号, modelKey) -> pool idx
    cur_vidx = [0]

    def add_model(mkey):
        ck = (cur_vidx[0], mkey)
        if ck in model_cache:
            return model_cache[ck]
        m = ent_json['models'].get(mkey)
        if m is None:
            warns.append('模型缺失，已用空模型替代: %s' % mkey)
            m = {'translucent': False, 'elements': []}
        blob = ser_model(m)
        sha = hashlib.sha1(blob).hexdigest()
        idx = model_by_sha.get(sha)
        if idx is None:
            idx = len(models)
            model_by_sha[sha] = idx
            models.append(blob)
        model_cache[ck] = idx
        return idx

    ent_json = None        # 当前版本 json（add_model 闭包用）

    def ser_model(m):
        out = bytearray()
        out += struct.pack('<B', 1 if m.get('translucent') else 0)
        els = m.get('elements') or []
        out += struct.pack('<H', len(els))
        for e in els:
            fr = e.get('from') or [0, 0, 0]
            to = e.get('to') or [16, 16, 16]
            pack_f3(out, fr[0] if len(fr) > 0 else 0, fr[1] if len(fr) > 1 else 0,
                    fr[2] if len(fr) > 2 else 0)
            pack_f3(out, to[0] if len(to) > 0 else 16, to[1] if len(to) > 1 else 16,
                    to[2] if len(to) > 2 else 16)
            r = e.get('rotation')
            if r is None:
                out += b'\x00'
            else:
                out += b'\x01'
                multi = bool(r.get('multiaxis'))
                out += struct.pack('<B', 1 if multi else 0)
                org = r.get('origin') or [0, 0, 0]
                pack_f3(out, org[0] if len(org) > 0 else 0,
                        org[1] if len(org) > 1 else 0, org[2] if len(org) > 2 else 0)
                if multi:
                    out += struct.pack('<B', AXIS_IDX.get(r.get('axis'), 1))
                    out += struct.pack('<f', f32(r.get('angle')))
                else:
                    ax = r.get('axis')
                    out += struct.pack('<B', AXIS_IDX.get(ax, 1))
                    out += struct.pack('<f', f32(r.get('angle')))
                out += struct.pack('<B', 1 if r.get('rescale') else 0)
                if multi:
                    pack_f3(out, r.get('x'), r.get('y'), r.get('z'))
                else:
                    pack_f3(out, 0.0, 0.0, 0.0)
            faces = e.get('faces') or {}
            mask = 0
            for i, fn in enumerate(FACE_NAMES):
                if fn in faces:
                    mask |= (1 << i)
            out += struct.pack('<B', mask)
            for i, fn in enumerate(FACE_NAMES):
                if not (mask >> i) & 1:
                    continue
                f = faces.get(fn) or {}
                tp = norm_tex_path(f.get('texture'))
                ti = strings.add(tp) if tp is not None else NO_STR
                out += struct.pack('<I', ti)
                uv = f.get('uv')
                if not uv or len(uv) < 4:
                    uv = [0, 0, 16, 16]
                out += struct.pack('<4f', f32(uv[0]), f32(uv[1]), f32(uv[2]), f32(uv[3]))
                cf = f.get('cullface')
                out += struct.pack('<B', NO_CULL if cf is None else FACE_IDX.get(cf, NO_CULL))
                tix = f.get('tintindex')
                out += struct.pack('<h', -1 if tix is None else int(tix))
                rot = f.get('rotation')
                out += struct.pack('<H', 0 if rot is None else int(rot) & 0xFFFF)
        return bytes(out)

    def ser_lang(entries):
        out = bytearray()
        out += struct.pack('<I', len(entries))
        for bs, en, zh in entries:
            out += struct.pack('<III', bs, en, zh)
        return bytes(out)

    # ------------------------------------------------------------------
    version_records = []
    for vidx, ent in enumerate(ents):
        cur_vidx[0] = vidx
        load_version(ent)          # 已在 main() 里读过则直接返回
        ent_json = ent['json']
        vid = ent['json'].get('version_id') or ent['vid']
        if vid != ent['vid']:
            warns.append('版本 id 不一致: 目录/清单=%s, json.version_id=%s' % (ent['vid'], vid))
        if ent['dv'] is None:
            warns.append('版本 %s 无 data_version，记为 -1' % vid)

        # ---- 贴图池 + 版本 路径->索引 映射 ----
        meta = ent['meta']
        entries = meta.get('entries') or {}
        atlas = ent['atlas']
        tex_list = list(ent['json'].get('textures') or [])
        extra = [k for k in entries.keys() if k not in set(tex_list)]
        if extra:
            warns.append('版本 %s: atlas_meta 有 %d 个贴图不在 textures 列表中，已一并纳入'
                         % (vid, len(extra)))
        paths = sorted(set(tex_list) | set(entries.keys()))
        texmap = []            # (path, poolIdx)
        no_pixels = []
        for p in paths:
            e = entries.get(p)
            if not e:
                no_pixels.append(p)
                continue
            x, y = int(e['x']), int(e['y'])
            w, h = int(e['w']), int(e['h'])
            if x < 0 or y < 0 or x + w > atlas.shape[1] or y + h > atlas.shape[0]:
                no_pixels.append(p)
                warns.append('版本 %s: 贴图 %s 越界 (%d,%d,%d,%d)' % (vid, p, x, y, w, h))
                continue
            px = np.ascontiguousarray(atlas[y:y + h, x:x + w]).tobytes()
            idx = texpool.add(px, w, h)
            texmap.append((p, idx))
        if no_pixels:
            warns.append('版本 %s: %d 个贴图没有像素，已从 路径->索引 映射剔除: %s'
                         % (vid, len(no_pixels), ', '.join(no_pixels[:8])))
        if not texmap:
            warns.append('版本 %s 没有任何贴图！' % vid)

        # ---- 字符串 / 模型 / 方块 ----
        id_str = strings.add(vid)
        texmap = [(strings.add(p), idx) for p, idx in texmap]   # (pathStrIdx, poolIdx)

        blocks = ent['json'].get('blocks') or {}
        block_recs = []
        total_model_refs = 0
        for bname in sorted(blocks.keys()):
            b = blocks[bname] or {}
            name_str = strings.add(bname)
            var_recs = []
            for v in (b.get('variants') or []):
                when = norm_when_variant(v.get('when'))
                mi = add_model(v.get('model'))
                var_recs.append((strings.add(when), mi,
                                 quarter_turns(v.get('x'), '%s %s variant.x' % (vid, bname), warns),
                                 quarter_turns(v.get('y'), '%s %s variant.y' % (vid, bname), warns),
                                 1 if v.get('uvlock') else 0))
                total_model_refs += 1
            mp_recs = []
            for v in (b.get('multipart') or []):
                when = norm_when_multipart(v.get('when'))
                mi = add_model(v.get('model'))
                mp_recs.append((strings.add(when), mi,
                                quarter_turns(v.get('x'), '%s %s mp.x' % (vid, bname), warns),
                                quarter_turns(v.get('y'), '%s %s mp.y' % (vid, bname), warns),
                                1 if v.get('uvlock') else 0))
                total_model_refs += 1
            block_recs.append((name_str, var_recs, mp_recs))

        # ---- 语言表 ----
        names = ent['json'].get('names') or {}
        lang_entries = []
        for k in sorted(names.keys()):
            v = names[k] or {}
            lang_entries.append((strings.add(k), strings.add(v.get('en') or ''),
                                 strings.add(v.get('zh') or '')))
        lang_blob = ser_lang(lang_entries)
        lsha = hashlib.sha1(lang_blob).hexdigest()
        lidx = lang_by_sha.get(lsha)
        if lidx is None:
            lidx = len(langs)
            lang_by_sha[lsha] = lidx
            langs.append(lang_blob)

        version_records.append({
            'vid': vid, 'idStr': id_str, 'dv': ent['dv'] if ent['dv'] is not None else -1,
            'langIdx': lidx, 'texmap': texmap, 'blocks': block_recs,
            'modelRefs': total_model_refs,
        })

    # ------------------------------------------------------------------
    # 序列化
    # ------------------------------------------------------------------
    out = bytearray()
    out += MAGIC
    out += struct.pack('<IIIII', FORMAT_VERSION, len(version_records), len(texpool.list),
                       len(models), len(langs))

    sec_strings = strings.serialize()
    out += sec_strings
    strings_off = len(out) - len(sec_strings)
    n_strings = len(strings)

    lang_off = len(out)
    out += struct.pack('<I', len(langs))
    for lb in langs:
        out += lb

    tex_sec, raw_size, comp_size = texpool.serialize()
    tex_off = len(out)
    out += tex_sec

    mod_off = len(out)
    out += struct.pack('<I', len(models))
    for mb in models:
        out += mb

    ver_off = len(out)
    out += struct.pack('<I', len(version_records))
    for r in version_records:
        out += struct.pack('<IiI', r['idStr'], r['dv'], r['langIdx'])
        out += struct.pack('<I', len(r['texmap']))
        for p_str, ti in r['texmap']:
            out += struct.pack('<II', p_str, ti)
        out += struct.pack('<I', len(r['blocks']))
        for name_str, var_recs, mp_recs in r['blocks']:
            out += struct.pack('<I', name_str)
            out += struct.pack('<H', len(var_recs))
            for ws, mi, rx, ry, ul in var_recs:
                out += struct.pack('<IIBBB', ws, mi, rx, ry, ul)
            out += struct.pack('<H', len(mp_recs))
            for ws, mi, rx, ry, ul in mp_recs:
                out += struct.pack('<IIBBB', ws, mi, rx, ry, ul)

    data = bytes(out)
    assert len(strings) == n_strings, '字符串表在序列化之后又被修改（会破坏 STRINGS section）'

    report = {
        'formatVersion': FORMAT_VERSION,
        'sections': {'stringsOff': strings_off, 'langsOff': lang_off,
                     'texturesOff': tex_off, 'modelsOff': mod_off, 'versionsOff': ver_off},
        'strings': {'count': n_strings, 'bytes': strings._bytes},
        'langs': {'count': len(langs)},
        'textures': {'unique': len(texpool.list), 'refs': texpool.total_refs,
                     'rawSize': raw_size, 'compressedSize': comp_size,
                     'hasAlpha': sum(1 for t in texpool.list if t['hasAlpha']),
                     'bytesBySize': {}},
        'models': {'unique': len(models), 'refs': sum(r['modelRefs'] for r in version_records)},
        'versions': [],
        'warnings': warns,
    }
    for t in texpool.list:
        k = '%dx%d' % (t['w'], t['h'])
        report['textures']['bytesBySize'][k] = report['textures']['bytesBySize'].get(k, 0) + 1
    seen_tex, seen_mod = set(), set()
    for r in version_records:
        tidx = {ti for _, ti in r['texmap']}
        midx = {mi for _, v, m in r['blocks'] for mi in
                [x[1] for x in v] + [x[1] for x in m]}
        report['versions'].append({
            'id': r['vid'], 'dataVersion': r['dv'], 'langTableIdx': r['langIdx'],
            'textures': len(r['texmap']), 'distinctTextures': len(tidx),
            'newTextures': len(tidx - seen_tex),
            'blocks': len(r['blocks']), 'modelRefs': r['modelRefs'],
            'distinctModels': len(midx), 'newModels': len(midx - seen_mod),
        })
        seen_tex |= tidx
        seen_mod |= midx
    report['dedup'] = {
        'texturePoolUnique': len(texpool.list), 'textureRefs': texpool.total_refs,
        'textureDedupPct': round(100.0 * len(texpool.list) / max(1, texpool.total_refs), 2),
        'modelPoolUnique': len(models),
        'modelRefs': sum(r['modelRefs'] for r in version_records),
        'modelDedupPct': round(100.0 * len(models)
                               / max(1, sum(r['modelRefs'] for r in version_records)), 2),
        'langUnique': len(langs), 'versionCount': len(version_records),
        'stringUnique': n_strings,
    }
    return data, report


# --------------------------------------------------------------------------
FORMAT_SPEC = r"""
=== blocks.dat 二进制格式 (v3, 全部小端序) ===

[Header] 28 字节
  char magic[8]      = "PMBLKDB\0"  (50 4D 42 4C 4B 44 42 00)
  u32  formatVersion = 3
  u32  versionCount
  u32  textureCount        // 贴图池条目数 = TEXTURES.count
  u32  modelCount          // 模型池条目数 = MODELS.count
  u32  langTableCount      // 语言表数量 = LANGS.count

[Section STRINGS]  紧跟 Header
  u32  count
  count 个 NUL 结尾的 UTF-8 字符串，直接首尾拼接。
  第 i 个字符串 = 从字节流开头按 NUL 切分后的第 i 段（不含 NUL）。
  索引 0xFFFFFFFF (NO_STR) 保留为"无效/缺失"哨兵，不会指向真实字符串。

[Section LANGS]
  u32  count                          // == Header.langTableCount
  count 次:
    u32 entryCount
    entryCount 次:
      u32 blockStrIdx                 // "minecraft:xxx"
      u32 enStrIdx
      u32 zhStrIdx

[Section TEXTURES]
  u32  count                          // == Header.textureCount
  count 次:  (每条 13 字节, 非对齐!)
    u16 w
    u16 h
    u32 dataOffset                    // 在解压后的 raw 缓冲区中的字节偏移
    u32 dataLen                       // == w*h*4
    u8  hasAlpha                      // 1 = 该贴图存在 alpha < 255 的像素
  u32  rawSize                        // 所有贴图像素按池索引顺序拼接后的总字节数
  u32  compressedSize
  u8   blob[compressedSize]           // zlib(RFC1950) 压缩的 rawSize 字节
  贴图 i 的像素 = inflate(blob)[dataOffset .. dataOffset+dataLen)，RGBA8888

[Section MODELS]
  u32  count                          // == Header.modelCount
  count 次:
    u8  flags                         // bit0 = translucent
    u16 elementCount
    elementCount 次:
      f32 from[3]
      f32 to[3]
      u8  hasRot                      // 0 / 1
      if hasRot:
        u8  rotKind                   // 0 = axis/angle, 1 = 多轴 rx/ry/rz
        f32 rotOrigin[3]
        u8  axis                      // 0=x 1=y 2=z (rotKind==0 有效)
        f32 angle                     // 度
        u8  rescale
        f32 rx, ry, rz                // rotKind==1 有效, 否则为 0
      u8  faceMask                    // bit0=down bit1=up bit2=north bit3=south bit4=west bit5=east
      按 faceMask 从低位到高位，每个置位的面依次:
        u32 texStrIdx                 // 贴图路径的字符串索引; 0xFFFFFFFF = JSON 中无 texture 字段
        f32 uv[4]                     // 原样保留 JSON 的 0..16 值
        u8  cullface                  // 0xFF = 无; 0=down 1=up 2=north 3=south 4=west 5=east
        i16 tintindex                 // -1 = 无
        u16 rotation                  // 0/90/180/270

[Section VERSIONS]
  u32  count                          // == Header.versionCount
  count 次:
    u32 idStrIdx
    i32 dataVersion
    u32 langTableIdx
    u32 textureMapCount
    textureMapCount 次:
      u32 pathStrIdx                  // 形如 "block/stone"（无 minecraft: 前缀）
      u32 textureIdx                  // 贴图池索引
    u32 blockCount
    blockCount 次:
      u32 nameStrIdx                  // 形如 "minecraft:stone"
      u16 variantCount
      variantCount 次:
        u32 whenStrIdx
        u32 modelIdx
        u8  rotX, u8 rotY, u8 uvlock
      u16 multipartCount
      multipartCount 次:
        u32 whenStrIdx
        u32 modelIdx
        u8  rotX, u8 rotY, u8 uvlock

  *** rotX / rotY 是 "90 度的倍数" 0..3，不是度数 ***
  原始 blockstate 的 x/y 取值是 0/90/180/270，270 装不进 u8，所以这里存的是
  角度/90（0=0° 1=90° 2=180° 3=270°）。C++ 侧请用 rotX*90 / rotY*90 还原角度。
  如果你更想直接存度数，把这两个字段从 u8 改成 u16 并告诉我，我重跑脚本即可。

=== 对齐注意 ===
除 Header 外全部字段都是**紧凑无填充**的：TEXTURES 每条 13 字节、MODELS 内部含 u8 后接
f32，都没有 4 字节对齐。C++ 侧请用 memcpy 逐字段读取，或用 #pragma pack(push,1) 的结构体，
**不要**直接把指针 reinterpret_cast 成未打包的结构体。

=== when 字符串规范化 ===
  variants[].when (dict)  -> "a=1,b=2"，属性名按字典序排序；{} -> ""
  multipart[].when (list) -> OR 方案之间用 '|'，方案内 AND 用 ','；条件名排序
                              [] -> ""；含空方案(无条件) -> 整体 ""
  解析: 先按 '|' 切成 OR 方案，每个方案再按 ',' 切，每项再按第一个 '=' 切成 属性/值。
        空串 = 无条件匹配。

=== 解压 ===
  blocks.dat.z        = zlib.compress(blocks.dat, 9)   // RFC1950: 2 字节头 + adler32 尾
  TEXTURES 的 blob    = zlib.compress(rawPixels, 9)    // 同上
  C++ 直接用 zlib 的 uncompress()/inflateInit() 即可（不要用 -15 raw window）。
"""


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    proj = os.path.dirname(here)
    ws = os.path.dirname(proj)

    ap = argparse.ArgumentParser()
    ap.add_argument('--ws', default=ws, help='workspace 根目录')
    ap.add_argument('--out', default=os.path.join(proj, 'data'), help='输出目录')
    ap.add_argument('--only', action='append', default=None, help='只打包指定版本 id')
    ap.add_argument('--verbose', action='store_true')
    args = ap.parse_args()

    roots = find_data_roots(args.ws, proj)
    ents, missing = discover_versions(roots, args.only)
    if not ents:
        print('[error] 没找到任何可打包的版本。data 根: %s' % roots, file=sys.stderr)
        return 2

    print('数据根: %s' % ', '.join(roots))

    # 先读一遍拿到真实 data_version，再按 (dataVersion, id) 排序，保证输出可复现
    for e in ents:
        load_version(e)
    ents.sort(key=lambda e: (e['dv'] if isinstance(e['dv'], int) else 1 << 30, e['vid']))

    print('发现版本: %s' % ', '.join('%s(dv=%s)' % (e['vid'], e['dv']) for e in ents))
    if missing:
        print('versions.json 中尚未烘焙、已跳过: %s' % ', '.join(missing[:12])
              + (' ...' if len(missing) > 12 else ''))

    data, report = build(ents, args.verbose)

    os.makedirs(args.out, exist_ok=True)
    dat = os.path.join(args.out, 'blocks.dat')
    dtz = os.path.join(args.out, 'blocks.dat.z')
    open(dat, 'wb').write(data)
    comp = zlib.compress(data, 9)
    open(dtz, 'wb').write(comp)

    report['files'] = {
        'blocks.dat': os.path.getsize(dat),
        'blocks.dat.z': os.path.getsize(dtz),
        'blocks.dat.MB': round(os.path.getsize(dat) / 1048576.0, 3),
        'blocks.dat.z.MB': round(os.path.getsize(dtz) / 1048576.0, 3),
        'ratio': round(os.path.getsize(dtz) / max(1.0, os.path.getsize(dat)), 4),
    }
    report['inputs'] = [{'id': e['vid'], 'dir': e['dir'], 'source': e['source'],
                         'standaloneDir': e.get('standalone_dir')} for e in ents]
    report['skippedManifestIds'] = missing
    report['format'] = FORMAT_SPEC.strip()

    for target in (os.path.join(args.ws, 'data', 'pack_report.json'),
                   os.path.join(proj, 'data', 'pack_report.json')):
        try:
            os.makedirs(os.path.dirname(target), exist_ok=True)
            with open(target, 'w', encoding='utf-8') as f:
                json.dump(report, f, ensure_ascii=False, indent=1)
        except OSError as e:
            print('[warn] 报告写入失败 %s: %s' % (target, e), file=sys.stderr)

    tx = report['textures']
    print('')
    print('blocks.dat        = %.3f MB' % report['files']['blocks.dat.MB'])
    print('blocks.dat.z      = %.3f MB  (ratio %.3f)' % (report['files']['blocks.dat.z.MB'],
                                                         report['files']['ratio']))
    print('strings           = %d (%d bytes)' % (report['strings']['count'],
                                                 report['strings']['bytes']))
    print('lang tables       = %d (unique) / %d (versions)' % (report['langs']['count'],
                                                               len(report['versions'])))
    print('textures          = %d unique / %d refs, hasAlpha=%d, raw=%.2f MB -> %.2f MB'
          % (tx['unique'], tx['refs'], tx['hasAlpha'],
             tx['rawSize'] / 1048576.0, tx['compressedSize'] / 1048576.0))
    print('models            = %d unique / %d refs' % (report['models']['unique'],
                                                       report['models']['refs']))
    print('versions          = %d' % len(report['versions']))
    for v in report['versions']:
        print('  %-14s dv=%-6s langIdx=%-3d textures=%-5d(unique %d, new %d) '
              'blocks=%-5d modelRefs=%-6d models(unique %d, new %d)'
              % (v['id'], v['dataVersion'], v['langTableIdx'], v['textures'],
                 v['distinctTextures'], v['newTextures'], v['blocks'], v['modelRefs'],
                 v['distinctModels'], v['newModels']))
    d = report['dedup']
    print('去重              : 贴图 %d/%d (%.1f%%), 模型 %d/%d (%.1f%%), 语言表 %d/%d'
          % (d['texturePoolUnique'], d['textureRefs'], d['textureDedupPct'],
             d['modelPoolUnique'], d['modelRefs'], d['modelDedupPct'],
             d['langUnique'], d['versionCount']))
    if report['warnings']:
        print('warnings (%d):' % len(report['warnings']))
        for w in report['warnings'][:40]:
            print('  - %s' % w)
        if len(report['warnings']) > 40:
            print('  ... 其余见 pack_report.json')
    print('')
    print(FORMAT_SPEC.strip())
    return 0


if __name__ == '__main__':
    sys.exit(main())
