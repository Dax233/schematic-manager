#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
verify_blockdb.py -- 独立读取并完整解析 projmgr/data/blocks.dat。

这个脚本**不 import pack_blockdb**：解析逻辑完全按 blockdb 的格式说明重写一遍，
用于交叉验证打包器的输出。

用法:
    python verify_blockdb.py                        # 解析 + 摘要 + 还原 3 个方块
    python verify_blockdb.py --cross-check           # 再和输入 json/atlas 逐字节对拍
    python verify_blockdb.py --dump-tex DIR          # 导出贴图 PNG
    python verify_blockdb.py --dat PATH --blocks A,B --max-variants 40
"""

import argparse
import json
import os
import struct
import sys
import zlib

MAGIC = b'PMBLKDB\0'
NO_STR = 0xFFFFFFFF
NO_CULL = 0xFF
FACE_NAMES = ['down', 'up', 'north', 'south', 'west', 'east']
FACE_IDX = {n: i for i, n in enumerate(FACE_NAMES)}
AXIS_NAMES = ['x', 'y', 'z']
AXIS_IDX = {'x': 0, 'y': 1, 'z': 2}


class Reader(object):
    def __init__(self, buf, pos=0):
        self.b = buf
        self.p = pos

    def u8(self):
        v = self.b[self.p]
        self.p += 1
        return v

    def u16(self):
        v = struct.unpack_from('<H', self.b, self.p)[0]
        self.p += 2
        return v

    def i16(self):
        v = struct.unpack_from('<h', self.b, self.p)[0]
        self.p += 2
        return v

    def u32(self):
        v = struct.unpack_from('<I', self.b, self.p)[0]
        self.p += 4
        return v

    def i32(self):
        v = struct.unpack_from('<i', self.b, self.p)[0]
        self.p += 4
        return v

    def f32(self):
        v = struct.unpack_from('<f', self.b, self.p)[0]
        self.p += 4
        return v

    def fN(self, n):
        return [self.f32() for _ in range(n)]

    def raw(self, n):
        v = self.b[self.p:self.p + n]
        assert len(v) == n, 'read past end of buffer'
        self.p += n
        return v


def parse(data):
    db = {}
    r = Reader(data)
    magic = r.raw(8)
    assert magic == MAGIC, 'bad magic: %r' % magic
    db['formatVersion'] = r.u32()
    versionCount = r.u32()
    textureCount = r.u32()
    modelCount = r.u32()
    langTableCount = r.u32()

    # ---- STRINGS ----
    n = r.u32()
    strings = []
    for _ in range(n):
        end = data.index(b'\x00', r.p)
        strings.append(data[r.p:end].decode('utf-8'))
        r.p = end + 1
    db['strings'] = strings

    # ---- LANGS ----
    langs = []
    nlang = r.u32()
    for _ in range(nlang):
        ec = r.u32()
        langs.append([(r.u32(), r.u32(), r.u32()) for _ in range(ec)])
    db['langs'] = langs

    # ---- TEXTURES ----
    tex = []
    ntex = r.u32()
    for _ in range(ntex):
        w = r.u16()
        h = r.u16()
        off = r.u32()
        ln = r.u32()
        ha = r.u8()
        tex.append({'w': w, 'h': h, 'offset': off, 'len': ln, 'hasAlpha': bool(ha)})
    rawSize = r.u32()
    compSize = r.u32()
    raw = zlib.decompress(r.raw(compSize))
    assert len(raw) == rawSize, 'texture raw size mismatch'
    for t in tex:
        assert t['len'] == t['w'] * t['h'] * 4, 'texture len mismatch'
        assert t['offset'] + t['len'] <= rawSize, 'texture out of range'
    db['textures'] = tex
    db['textureRaw'] = raw

    # ---- MODELS ----
    models = []
    nmodel = r.u32()
    for _ in range(nmodel):
        flags = r.u8()
        ec = r.u16()
        els = []
        for _ in range(ec):
            frm = r.fN(3)
            to = r.fN(3)
            hasRot = r.u8()
            rot = None
            if hasRot:
                rot = {'kind': r.u8(), 'origin': r.fN(3), 'axis': r.u8(),
                       'angle': r.f32(), 'rescale': bool(r.u8()), 'xyz': r.fN(3)}
            mask = r.u8()
            faces = []
            for bit in range(6):
                if not (mask >> bit) & 1:
                    continue
                faces.append({'face': FACE_NAMES[bit], 'texStrIdx': r.u32(),
                              'uv': r.fN(4), 'cullface': r.u8(), 'tintindex': r.i16(),
                              'rotation': r.u16()})
            els.append({'from': frm, 'to': to, 'hasRot': bool(hasRot), 'rot': rot,
                        'faceMask': mask, 'faces': faces})
        models.append({'translucent': bool(flags & 1), 'flags': flags, 'elements': els})
    db['models'] = models

    # ---- VERSIONS ----
    versions = []
    nver = r.u32()
    for _ in range(nver):
        v = {'idStrIdx': r.u32(), 'dataVersion': r.i32(), 'langTableIdx': r.u32()}
        tm = r.u32()
        v['textureMap'] = [(r.u32(), r.u32()) for _ in range(tm)]
        bc = r.u32()
        blocks = []
        for _ in range(bc):
            name = r.u32()
            vc = r.u16()
            variants = [(r.u32(), r.u32(), r.u8(), r.u8(), r.u8()) for _ in range(vc)]
            mc = r.u16()
            multipart = [(r.u32(), r.u32(), r.u8(), r.u8(), r.u8()) for _ in range(mc)]
            blocks.append({'nameStrIdx': name, 'variants': variants, 'multipart': multipart})
        v['blocks'] = blocks
        versions.append(v)
    db['versions'] = versions
    db['endPos'] = r.p
    db['size'] = len(data)
    db['headerCounts'] = {'versionCount': versionCount, 'textureCount': textureCount,
                          'modelCount': modelCount, 'langTableCount': langTableCount}
    return db


def S(db, i):
    return '<NO_STR>' if i == NO_STR else db['strings'][i]


# --------------------------------------------------------------------------
# 与输入数据对拍
# --------------------------------------------------------------------------
def norm_tex_path(p):
    if p is None:
        return None
    s = str(p).strip()
    return s[len('minecraft:'):] if s.startswith('minecraft:') else s


def norm_when_variant(w):
    if not w:
        return ''
    if not isinstance(w, dict):
        return str(w)
    return ','.join('%s=%s' % (k, w[k]) for k in sorted(w.keys()))


def norm_when_multipart(w):
    if not w:
        return ''
    if isinstance(w, dict):
        return norm_when_variant(w)
    plans = []
    for plan in w:
        if not plan:
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


def near(a, b, eps=1e-4):
    return abs(float(a) - float(b)) <= eps


def near3(a, b, eps=1e-4):
    return all(near(a[i], b[i], eps) for i in range(3))


def model_mismatch(src, pm):
    """返回不一致原因列表（空 = 一致）。"""
    bad = []
    if bool(src.get('translucent')) != pm['translucent']:
        bad.append('translucent %s != %s' % (src.get('translucent'), pm['translucent']))
    sels = src.get('elements') or []
    if len(sels) != len(pm['elements']):
        bad.append('elementCount %d != %d' % (len(sels), len(pm['elements'])))
        return bad
    for i, (se, pe) in enumerate(zip(sels, pm['elements'])):
        if not near3(se.get('from') or [0, 0, 0], pe['from']):
            bad.append('elem%d from %s != %s' % (i, se.get('from'), pe['from']))
        if not near3(se.get('to') or [16, 16, 16], pe['to']):
            bad.append('elem%d to %s != %s' % (i, se.get('to'), pe['to']))
        sr = se.get('rotation')
        if (sr is None) != (not pe['hasRot']):
            bad.append('elem%d hasRot %s != %s' % (i, sr is not None, pe['hasRot']))
        elif sr is not None:
            pr = pe['rot']
            want_kind = 1 if sr.get('multiaxis') else 0
            if pr['kind'] != want_kind:
                bad.append('elem%d rotKind %d != %d' % (i, pr['kind'], want_kind))
            elif not near3(sr.get('origin') or [0, 0, 0], pr['origin']):
                bad.append('elem%d rotOrigin %s != %s' % (i, sr.get('origin'), pr['origin']))
            elif want_kind == 0 and pr['axis'] != AXIS_IDX.get(sr.get('axis'), 1):
                bad.append('elem%d axis %s != %s' % (i, sr.get('axis'), pr['axis']))
            elif want_kind == 0 and not near(sr.get('angle') or 0, pr['angle']):
                bad.append('elem%d angle %s != %s' % (i, sr.get('angle'), pr['angle']))
            elif want_kind == 1 and not near3([sr.get('x') or 0, sr.get('y') or 0, sr.get('z') or 0],
                                              pr['xyz']):
                bad.append('elem%d xyz mismatch' % i)
            if bool(sr.get('rescale')) != pr['rescale']:
                bad.append('elem%d rescale mismatch' % i)
        sfaces = se.get('faces') or {}
        mask = 0
        for k, fn in enumerate(FACE_NAMES):
            if fn in sfaces:
                mask |= (1 << k)
        if mask != pe['faceMask']:
            bad.append('elem%d faceMask 0x%02X != 0x%02X' % (i, mask, pe['faceMask']))
            continue
        for f in pe['faces']:
            sf = sfaces[f['face']]
            if norm_tex_path(sf.get('texture')) != S(_DB, f['texStrIdx']):
                bad.append('elem%d/%s tex %s != %s' % (i, f['face'],
                                                       norm_tex_path(sf.get('texture')),
                                                       S(_DB, f['texStrIdx'])))
            suv = sf.get('uv') or [0, 0, 16, 16]
            if not all(near(suv[j], f['uv'][j]) for j in range(4)):
                bad.append('elem%d/%s uv %s != %s' % (i, f['face'], suv, f['uv']))
            scf = sf.get('cullface')
            wcf = NO_CULL if scf is None else FACE_IDX.get(scf, NO_CULL)
            if wcf != f['cullface']:
                bad.append('elem%d/%s cullface %s != %s' % (i, f['face'], wcf, f['cullface']))
            sti = sf.get('tintindex')
            if (-1 if sti is None else int(sti)) != f['tintindex']:
                bad.append('elem%d/%s tintindex %s != %s' % (i, f['face'], sti, f['tintindex']))
            srot = sf.get('rotation')
            if (0 if srot is None else int(srot)) != f['rotation']:
                bad.append('elem%d/%s rotation %s != %s' % (i, f['face'], srot, f['rotation']))
    return bad


_DB = None


def find_inputs(ws, proj):
    """返回 {vid: dir}。"""
    out = {}
    for root in (os.path.join(ws, 'data'), os.path.join(proj, 'data')):
        vd = os.path.join(root, 'versions')
        if os.path.isdir(vd):
            for name in sorted(os.listdir(vd)):
                sub = os.path.join(vd, name)
                if os.path.isfile(os.path.join(sub, 'assets_baked.json')):
                    out.setdefault(name, sub)
    for root in (os.path.join(ws, 'data'), os.path.join(proj, 'data')):
        jp = os.path.join(root, 'assets_baked.json')
        if os.path.isfile(jp):
            try:
                h = json.load(open(jp, encoding='utf-8'))
            except Exception:
                continue
            vid = h.get('version_id') or h.get('mc_version') or 'unknown'
            out.setdefault(vid, root)
    return out


def cross_check(db, ws, proj, verbose=False):
    import numpy as np
    from PIL import Image  # noqa: F401  (仅为统一依赖检查)

    global _DB
    _DB = db
    inputs = find_inputs(ws, proj)
    byid = {S(db, v['idStrIdx']): v for v in db['versions']}
    problems = []
    checked = {'versions': 0, 'textures': 0, 'blocks': 0, 'variants': 0,
               'models': 0, 'langs': 0}

    print('')
    print('=' * 78)
    print('交叉对拍：blocks.dat  vs  输入 assets_baked.json / atlas.bin')
    print('=' * 78)
    print('输入版本: %s' % ', '.join(sorted(inputs.keys())))
    print('包内版本: %s' % ', '.join(sorted(byid.keys())))

    for vid, d in sorted(inputs.items()):
        v = byid.get(vid)
        if v is None:
            print('  !! 输入版本 %s 不在 blocks.dat 中' % vid)
            problems.append('version-missing\t缺少版本 %s' % vid)
            continue
        src = json.load(open(os.path.join(d, 'assets_baked.json'), encoding='utf-8'))
        meta = json.load(open(os.path.join(d, 'atlas_meta.json'), encoding='utf-8'))
        raw = open(os.path.join(d, 'atlas.bin'), 'rb').read()
        atlas = np.frombuffer(raw, dtype=np.uint8).reshape(int(meta['atlas_h']),
                                                           int(meta['atlas_w']), 4)
        checked['versions'] += 1
        print('')
        print('--- %s (dv=%s) ---' % (vid, v['dataVersion']))
        if src.get('data_version') is not None and int(src['data_version']) != v['dataVersion']:
            problems.append('dataVersion\t%s dataVersion %s != %s'
                            % (vid, src['data_version'], v['dataVersion']))
            print('    !! dataVersion %s != %s' % (src['data_version'], v['dataVersion']))

        # 贴图像素逐字节
        texbyidx = {}
        tmis = 0
        ntex_checked = 0
        newtex = set()
        for pidx, tidx in v['textureMap']:
            texbyidx[S(db, pidx)] = tidx
        for path, ent in (meta.get('entries') or {}).items():
            tidx = texbyidx.get(path)
            if tidx is None:
                continue
            newtex.add(tidx)
            t = db['textures'][tidx]
            x, y, w, h = int(ent['x']), int(ent['y']), int(ent['w']), int(ent['h'])
            want = atlas[y:y + h, x:x + w].tobytes()
            got = db['textureRaw'][t['offset']:t['offset'] + t['len']]
            checked['textures'] += 1
            ntex_checked += 1
            if want != got:
                tmis += 1
                if tmis <= 3:
                    problems.append('tex-pixels\t%s 贴图像素不符: %s' % (vid, path))
            if (t['w'], t['h']) != (w, h):
                problems.append('tex-size\t%s 贴图尺寸不符: %s %dx%d != %dx%d'
                                % (vid, path, t['w'], t['h'], w, h))
            want_a = bool(np.frombuffer(want, dtype=np.uint8)[3::4].min() < 255)
            if want_a != t['hasAlpha']:
                problems.append('tex-hasalpha\t%s hasAlpha 不符: %s' % (vid, path))
        print('    贴图: %d 张逐字节比对，%d 张不符；该版本引用 %d 个池条目'
              % (ntex_checked, tmis, len(newtex)))

        # 方块 / 变体 / 模型
        srcblocks = src.get('blocks') or {}
        pblocks = {S(db, b['nameStrIdx']): b for b in v['blocks']}
        if set(srcblocks) != set(pblocks):
            only_src = sorted(set(srcblocks) - set(pblocks))[:5]
            only_p = sorted(set(pblocks) - set(srcblocks))[:5]
            problems.append('block-set\t%s 方块集合不一致 src_only=%s packed_only=%s'
                            % (vid, only_src, only_p))
        mis = 0
        for bn, sb in srcblocks.items():
            pb = pblocks.get(bn)
            if pb is None:
                continue
            checked['blocks'] += 1
            for kind, sl, pl in (('variant', sb.get('variants') or [], pb['variants']),
                                 ('multipart', sb.get('multipart') or [], pb['multipart'])):
                if len(sl) != len(pl):
                    problems.append('variant-count\t%s %s %s 条数 %d != %d'
                                    % (vid, bn, kind, len(sl), len(pl)))
                    mis += 1
                    continue
                for se, pe in zip(sl, pl):
                    checked['variants'] += 1
                    ws_ = (norm_when_variant(se.get('when')) if kind == 'variant'
                           else norm_when_multipart(se.get('when')))
                    if ws_ != S(db, pe[0]):
                        problems.append('%s\t%s %s %s when %r != %r'
                                        % ('when', vid, bn, kind, ws_, S(db, pe[0])))
                        mis += 1
                    # rotX/rotY 存的是 90 度的倍数
                    if (int(round(float(se.get('x') or 0) / 90.0)),
                            int(round(float(se.get('y') or 0) / 90.0)),
                            1 if se.get('uvlock') else 0) != (pe[2], pe[3], pe[4]):
                        problems.append('%s\t%s %s %s rot/uvlock (%s,%s,%s) != (%d,%d,%d)'
                                        % ('rot-uvlock', vid, bn, kind, se.get('x'), se.get('y'),
                                           se.get('uvlock'), pe[2], pe[3], pe[4]))
                        mis += 1
                    smod = (src.get('models') or {}).get(se.get('model'))
                    if smod is None:
                        continue
                    checked['models'] += 1
                    b2 = model_mismatch(smod, db['models'][pe[1]])
                    if b2:
                        mis += 1
                        for x in b2[:2]:
                            problems.append('%s\t%s %s %s: %s'
                                            % ('model', vid, bn, se.get('model'), x))
        print('    方块: %d 个，变体/模型引用 %d 条，%d 条不符'
              % (checked['blocks'], checked['variants'], mis))

        # 语言表
        lg = db['langs'][v['langTableIdx']]
        langmap = {S(db, a): (S(db, b), S(db, c)) for a, b, c in lg}
        srcnames = src.get('names') or {}
        lmis = 0
        for k, val in srcnames.items():
            got = langmap.get(k)
            if got != (val.get('en') or '', val.get('zh') or ''):
                lmis += 1
                if lmis <= 3:
                    problems.append('lang\t%s 名称不符: %s %s != %s'
                                    % (vid, k, got, (val.get('en'), val.get('zh'))))
        checked['langs'] += len(srcnames)
        if lmis:
            problems.append('lang-count\t%s 语言表 %d/%d 条不符' % (vid, lmis, len(srcnames)))
        print('    语言表: %d 条，%d 条不符' % (len(srcnames), lmis))

    print('')
    print('对拍总计: 版本 %d, 贴图 %d, 方块 %d, 变体 %d, 模型 %d, 名称 %d'
          % (checked['versions'], checked['textures'], checked['blocks'],
             checked['variants'], checked['models'], checked['langs']))
    if problems:
        groups = {}
        for p in problems:
            cat = p.split('\t', 1)[0]
            groups.setdefault(cat, []).append(p)
        print('对拍发现 %d 个问题，分类:' % len(problems))
        for cat in sorted(groups, key=lambda c: -len(groups[c])):
            lst = groups[cat]
            print('  [%s] %d 处' % (cat, len(lst)))
            for p in lst[:3]:
                print('      %s' % p.split('\t', 1)[-1])
    else:
        print('对拍结果: 全部一致 ✓')
    return problems


# --------------------------------------------------------------------------
def main():
    here = os.path.dirname(os.path.abspath(__file__))
    proj = os.path.dirname(here)
    ws = os.path.dirname(proj)

    ap = argparse.ArgumentParser()
    ap.add_argument('--dat', default=os.path.join(proj, 'data', 'blocks.dat'))
    ap.add_argument('--blocks', default='minecraft:stone,minecraft:oak_stairs,minecraft:hopper')
    ap.add_argument('--version', default=None, help='用哪个版本还原方块（默认最后一个=最新）')
    ap.add_argument('--dump-tex', default=None, help='导出 PNG 的目录')
    ap.add_argument('--dump-tex-names',
                    default='block/stone,block/oak_planks,block/hopper_outside')
    ap.add_argument('--dump-scale', type=int, default=8)
    ap.add_argument('--max-variants', type=int, default=4,
                    help='每个方块最多打印多少条 variant/multipart 的完整几何（0=全部）')
    ap.add_argument('--cross-check', action='store_true',
                    help='和输入 assets_baked.json / atlas.bin 逐项对拍')
    args = ap.parse_args()

    data = open(args.dat, 'rb').read()
    db = parse(data)

    print('=' * 78)
    print('blocks.dat 解析成功')
    print('=' * 78)
    print('文件大小        : %d bytes (%.3f MB)' % (db['size'], db['size'] / 1048576.0))
    print('magic           : %r  formatVersion=%d' % (MAGIC, db['formatVersion']))
    print('解析消耗字节    : %d / %d  (剩余 %d)'
          % (db['endPos'], db['size'], db['size'] - db['endPos']))
    hc = db['headerCounts']
    print('Header          : versionCount=%d textureCount=%d modelCount=%d langTableCount=%d'
          % (hc['versionCount'], hc['textureCount'], hc['modelCount'], hc['langTableCount']))
    ok = (hc['versionCount'] == len(db['versions'])
          and hc['textureCount'] == len(db['textures'])
          and hc['modelCount'] == len(db['models'])
          and hc['langTableCount'] == len(db['langs']))
    print('Header 与 section 计数一致: %s' % ('YES' if ok else 'NO !!!'))

    print('')
    print('--- sections ---')
    print('STRINGS : %d 条, 字节 %d'
          % (len(db['strings']), sum(len(s.encode('utf-8')) + 1 for s in db['strings'])))
    print('LANGS   : %d 张' % len(db['langs']))
    for i, lg in enumerate(db['langs']):
        print('            #%d entryCount=%d' % (i, len(lg)))
    print('TEXTURES: %d 张 (raw=%d bytes, zlib 解压校验 OK), hasAlpha=%d'
          % (len(db['textures']), len(db['textureRaw']),
             sum(1 for t in db['textures'] if t['hasAlpha'])))
    print('MODELS  : %d 个' % len(db['models']))
    print('VERSIONS: %d 个' % len(db['versions']))
    for i, v in enumerate(db['versions']):
        print('  [%d] id=%-14s dv=%-6d langIdx=%-2d textures=%-5d blocks=%d'
              % (i, S(db, v['idStrIdx']), v['dataVersion'], v['langTableIdx'],
                 len(v['textureMap']), len(v['blocks'])))
        dup = len(v['textureMap']) - len({p for p, _ in v['textureMap']})
        if dup:
            print('      !! textureMap 有 %d 个重复路径' % dup)
        bad = [ti for _, ti in v['textureMap'] if ti >= len(db['textures'])]
        if bad:
            print('      !! %d 个贴图索引越界' % len(bad))
        badn = [b for b in v['blocks'] if b['nameStrIdx'] >= len(db['strings'])]
        if badn:
            print('      !! %d 个方块名索引越界' % len(badn))
        badm = [mi for b in v['blocks'] for mi in
                [x[1] for x in b['variants']] + [x[1] for x in b['multipart']]
                if mi >= len(db['models'])]
        if badm:
            print('      !! %d 个模型索引越界' % len(badm))
        badw = [w for b in v['blocks'] for w in
                [x[0] for x in b['variants']] + [x[0] for x in b['multipart']]
                if w >= len(db['strings'])]
        if badw:
            print('      !! %d 个 when 字符串索引越界' % len(badw))

    totalref = sum(len(v['textureMap']) for v in db['versions'])
    totalmref = sum(len(b['variants']) + len(b['multipart'])
                    for v in db['versions'] for b in v['blocks'])
    print('')
    print('--- 去重效果 ---')
    print('贴图池 : %d unique / %d 引用  (%.2f%%)'
          % (len(db['textures']), totalref, 100.0 * len(db['textures']) / max(1, totalref)))
    print('模型池 : %d unique / %d 引用  (%.2f%%)'
          % (len(db['models']), totalmref, 100.0 * len(db['models']) / max(1, totalmref)))
    print('语言表 : %d unique / %d 版本' % (len(db['langs']), len(db['versions'])))
    print('字符串 : %d unique' % len(db['strings']))

    zpath = args.dat + '.z'
    if os.path.isfile(zpath):
        z = open(zpath, 'rb').read()
        try:
            dec = zlib.decompress(z)
            print('')
            print('blocks.dat.z  : %d bytes (%.3f MB), zlib 解压后与 blocks.dat %s'
                  % (len(z), len(z) / 1048576.0,
                     '完全一致' if dec == data else '不一致 !!!'))
        except Exception as e:
            print('blocks.dat.z 解压失败: %s' % e)

    # ---- 还原方块 ----
    if args.version:
        vi = next(i for i, v in enumerate(db['versions'])
                  if S(db, v['idStrIdx']) == args.version)
    else:
        vi = len(db['versions']) - 1
    v = db['versions'][vi]
    print('')
    print('=' * 78)
    print('还原方块（版本 %s）' % S(db, v['idStrIdx']))
    print('=' * 78)

    texByPath = {S(db, p): ti for p, ti in v['textureMap']}
    texByStrIdx = {p: ti for p, ti in v['textureMap']}
    byname = {S(db, b['nameStrIdx']): b for b in v['blocks']}
    cap = args.max_variants
    for want in [s.strip() for s in args.blocks.split(',') if s.strip()]:
        b = byname.get(want)
        if not b:
            print('!! 方块 %s 不在版本 %s 中' % (want, S(db, v['idStrIdx'])))
            continue
        print('')
        print('### %s  (%d variants, %d multipart)'
              % (want, len(b['variants']), len(b['multipart'])))
        shown = 0
        for kind, lst in (('variant', b['variants']), ('multipart', b['multipart'])):
            for ws_, mi, rx, ry, ul in lst:
                m = db['models'][mi]
                if cap and shown >= cap:
                    print('  %-9s when=%-58r model#%-5d x=%-3d y=%-3d uvlock=%d translucent=%d'
                          % (kind, S(db, ws_), mi, rx, ry, ul, 1 if m['translucent'] else 0))
                    continue
                shown += 1
                print('  %-9s when=%-58r model#%-5d x=%-3d y=%-3d uvlock=%d translucent=%d'
                      % (kind, S(db, ws_), mi, rx, ry, ul, 1 if m['translucent'] else 0))
                for ei, e in enumerate(m['elements']):
                    rs = ''
                    if e['rot']:
                        rr = e['rot']
                        rs = (' rot(kind=%d origin=%s axis=%s angle=%g rescale=%d xyz=%s)'
                              % (rr['kind'], rr['origin'],
                                 AXIS_NAMES[rr['axis']] if rr['axis'] < 3 else '?',
                                 rr['angle'], rr['rescale'], rr['xyz']))
                    print('      elem[%d] from=%s to=%s faceMask=0x%02X%s'
                          % (ei, e['from'], e['to'], e['faceMask'], rs))
                    for f in e['faces']:
                        tn = S(db, f['texStrIdx'])
                        ti = texByStrIdx.get(f['texStrIdx'])
                        px = ('pool#%d' % ti) if ti is not None else 'NOT-IN-MAP'
                        cf = 'none' if f['cullface'] == NO_CULL else FACE_NAMES[f['cullface']]
                        print('          %-6s tex=%-28s uv=%-24s cull=%-6s tint=%-3d rot=%d  [%s]'
                              % (f['face'], tn, str([round(x, 3) for x in f['uv']]),
                                 cf, f['tintindex'], f['rotation'], px))
        if cap:
            n = len(b['variants']) + len(b['multipart'])
            if n > cap:
                print('  ... 共 %d 条，其余 %d 条只打印摘要（--max-variants 0 看全部）'
                      % (n, n - cap))

    # ---- 导出贴图 ----
    if args.dump_tex:
        from PIL import Image
        os.makedirs(args.dump_tex, exist_ok=True)
        raw = db['textureRaw']
        for name in [s.strip() for s in args.dump_tex_names.split(',') if s.strip()]:
            ti = texByPath.get(name)
            if ti is None:
                print('!! 贴图 %s 不在版本 %s 的映射里' % (name, S(db, v['idStrIdx'])))
                continue
            t = db['textures'][ti]
            px = raw[t['offset']:t['offset'] + t['len']]
            img = Image.frombytes('RGBA', (t['w'], t['h']), px)
            safe = name.replace('/', '_')
            img.save(os.path.join(args.dump_tex, '%s.png' % safe))
            s = max(1, args.dump_scale)
            p2 = os.path.join(args.dump_tex, '%s_x%d.png' % (safe, s))
            img.resize((t['w'] * s, t['h'] * s), Image.NEAREST).save(p2)
            colors = img.getcolors(maxcolors=100000)
            amin = min(px[3::4]) if px else 255
            print('导出 %-24s pool#%-5d %dx%d hasAlpha=%d minAlpha=%d 颜色数=%d -> %s'
                  % (name, ti, t['w'], t['h'], t['hasAlpha'], amin, len(colors or []), p2))

    # ---- 对拍 ----
    if args.cross_check:
        cross_check(db, ws, proj)
    return 0


if __name__ == '__main__':
    sys.exit(main())
