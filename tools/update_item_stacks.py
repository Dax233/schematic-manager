"""Export per-release item stack sizes from PrismarineJS/minecraft-data (MIT)."""
import concurrent.futures
import gzip
import hashlib
import json
import pathlib
import re
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
BASE = 'https://raw.githubusercontent.com/PrismarineJS/minecraft-data/master/data/'

def fetch(path):
    url = BASE + path
    with urllib.request.urlopen(url, timeout=60) as response:
        raw = response.read()
    return json.loads(raw), {'url': url, 'sha256': hashlib.sha256(raw).hexdigest()}

def main():
    paths, provenance = fetch('dataPaths.json')
    text = (ROOT / 'src-tauri/src/pack/base.rs').read_text(encoding='utf-8')
    section = text.split('pub const SUPPORTED_RELEASES', 1)[1].split('];', 1)[0]
    releases = re.findall(r'\((\d+), "([\d.]+)"\)', section)
    selected = {dv: paths['pc'][v]['items'] for dv, v in releases if v in paths['pc'] and 'items' in paths['pc'][v]}
    folders = sorted(set(selected.values()))
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        results = list(pool.map(lambda folder: fetch(folder + '/items.json'), folders))
    snapshots = []
    for rows, _ in results:
        table = {row['name']: row['stackSize'] for row in rows}
        assert table and all(isinstance(n, int) and 1 <= n <= 99 for n in table.values())
        snapshots.append(table)
    data = {'versions': {dv: folders.index(folder) for dv, folder in selected.items()}, 'snapshots': snapshots}
    dest = ROOT / 'src-tauri/data'
    (dest / 'item-stacks.json.gz').write_bytes(gzip.compress(json.dumps(data, sort_keys=True, separators=(',', ':')).encode(), mtime=0))
    (dest / 'item-stacks-sources.json').write_text(json.dumps({'index': provenance, 'inputs': [source for _, source in results], 'versions': selected, 'unavailable': [v for dv, v in releases if dv not in selected]}, indent=2) + '\n', encoding='utf-8')
    print(f'{len(selected)} exact releases, {len(snapshots)} item tables')

if __name__ == '__main__':
    main()
