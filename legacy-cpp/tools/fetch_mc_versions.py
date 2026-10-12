"""枚举本地 Minecraft 客户端 jar，并下载缺失的正式版，产出 versions.json。

只处理正式版 (type == release)，范围从 1.13.2 到最新正式版。
"""
import json, os, re, sys, time, urllib.request, zipfile, hashlib, io

HERE = os.path.dirname(os.path.abspath(__file__))
PROJ = os.path.dirname(HERE)
WS   = os.path.dirname(PROJ)
CACHE = os.path.join(WS, 'cache', 'mcjars')
DATA  = os.path.join(PROJ, 'data')
MANIFEST_CACHE = os.path.join(CACHE, 'version_manifest_v2.json')

BASE_VERSION = '1.13.2'          # Litematica 支持的最低版本
SEARCH_ROOTS = [r'E:\游戏']
HTTP = lambda u: u.replace('https://', 'http://', 1)


def ensure_dirs():
    os.makedirs(CACHE, exist_ok=True)
    os.makedirs(DATA, exist_ok=True)


def fetch(url, timeout=180):
    req = urllib.request.Request(HTTP(url), headers={'User-Agent': 'Mozilla/5.0'})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read()


def get_manifest():
    if os.path.exists(MANIFEST_CACHE) and os.path.getsize(MANIFEST_CACHE) > 100000:
        return json.load(open(MANIFEST_CACHE, encoding='utf-8'))
    print('下载版本清单 ...')
    raw = fetch('https://piston-meta.mojang.com/mc/game/version_manifest_v2.json')
    open(MANIFEST_CACHE, 'wb').write(raw)
    return json.loads(raw.decode('utf-8'))


def scan_local_jars():
    """扫描本地所有客户端 jar，读取其内部 version.json 得到真实版本 id。"""
    found = {}
    for root in SEARCH_ROOTS:
        if not os.path.isdir(root):
            continue
        for dirpath, dirnames, filenames in os.walk(root):
            if os.path.basename(dirpath).lower() != 'versions':
                continue
            for d in list(dirnames):
                vdir = os.path.join(dirpath, d)
                for f in os.listdir(vdir):
                    if not f.lower().endswith('.jar') or 'sources' in f.lower():
                        continue
                    p = os.path.join(vdir, f)
                    try:
                        with zipfile.ZipFile(p) as z:
                            if 'version.json' not in z.namelist():
                                continue
                            info = json.loads(z.read('version.json').decode('utf-8'))
                            if 'assets/minecraft/blockstates/stone.json' not in z.namelist():
                                continue
                            vid = info.get('id')
                            if vid and vid not in found:
                                found[vid] = {'jar': p, 'dataVersion': info.get('world_version'),
                                              'source': 'local'}
                    except Exception:
                        pass
    return found


def target_versions(manifest):
    rels = [v for v in manifest['versions'] if v['type'] == 'release']
    ids = [v['id'] for v in rels]
    if BASE_VERSION not in ids:
        raise SystemExit('清单里找不到基准版本 ' + BASE_VERSION)
    cut = ids.index(BASE_VERSION)          # 清单从新到旧
    return rels[:cut + 1]


def main():
    ensure_dirs()
    manifest = get_manifest()
    targets = target_versions(manifest)
    print('目标正式版数量: %d  (%s .. %s)' % (len(targets), targets[-1]['id'], targets[0]['id']))

    local = scan_local_jars()
    print('本地已发现: %d 个版本' % len(local))
    for v in sorted(local, key=lambda k: local[k]['dataVersion'] or 0):
        print('   %-10s dv=%-6s %s' % (v, local[v]['dataVersion'], local[v]['jar'][:70]))

    todo = [v for v in targets if v['id'] not in local]
    print('\n需要下载: %d 个版本' % len(todo))

    result = {}
    total_bytes = 0
    for i, ent in enumerate(targets):
        vid = ent['id']
        if vid in local:
            result[vid] = {'id': vid, 'dataVersion': local[vid]['dataVersion'],
                           'jar': local[vid]['jar'], 'source': 'local'}
            continue
        dest = os.path.join(CACHE, vid + '-client.jar')
        if not (os.path.exists(dest) and os.path.getsize(dest) > 1000000):
            try:
                vmeta = json.loads(fetch(ent['url']).decode('utf-8'))
                curl = vmeta['downloads']['client']['url']
                data = fetch(curl, timeout=900)
                open(dest, 'wb').write(data)
                total_bytes += len(data)
                print('  [%2d/%2d] 下载 %-10s %6.1f MB' % (i + 1, len(targets), vid, len(data) / 1048576))
            except Exception as e:
                print('  [%2d/%2d] 失败 %-10s %s' % (i + 1, len(targets), vid, e))
                continue
        try:
            with zipfile.ZipFile(dest) as z:
                if 'version.json' not in z.namelist():
                    print('  跳过 %s（非客户端 jar）' % vid); continue
                info = json.loads(z.read('version.json').decode('utf-8'))
                if 'assets/minecraft/blockstates/stone.json' not in z.namelist():
                    print('  跳过 %s（无 blockstates）' % vid); continue
            result[vid] = {'id': vid, 'dataVersion': info.get('world_version'),
                           'jar': dest, 'source': 'download'}
        except Exception as e:
            print('  解析失败 %s: %s' % (vid, e))

    out = os.path.join(DATA, 'versions.json')
    ordered = sorted(result.values(), key=lambda r: r['dataVersion'] or 0)
    json.dump(ordered, open(out, 'w', encoding='utf-8'), ensure_ascii=False, indent=1)
    print('\n本次新下载: %.1f MB' % (total_bytes / 1048576))
    print('可用版本总数: %d -> %s' % (len(ordered), out))
    for r in ordered:
        print('   %-10s dv=%-6s %s' % (r['id'], r['dataVersion'], r['source']))


if __name__ == '__main__':
    main()
