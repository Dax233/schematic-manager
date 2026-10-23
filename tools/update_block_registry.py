"""Export exact release block registries and state domains from misode/mcmeta.

Run from the repository root. Only Python's standard library is required.
Input tags, URLs and input hashes are recorded beside the compressed output.
"""
import concurrent.futures
import gzip
import hashlib
import json
import pathlib
import re
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]


def fetch_release(release):
    data_version, version = release
    inputs = []
    documents = []
    for suffix, path in [('registries', 'block/data.json'), ('summary', 'blocks/data.json')]:
        url = f'https://raw.githubusercontent.com/misode/mcmeta/{version}-{suffix}/{path}'
        with urllib.request.urlopen(url, timeout=60) as response:
            raw = response.read()
        inputs.append({'url': url, 'sha256': hashlib.sha256(raw).hexdigest()})
        documents.append(json.loads(raw))
    names, states = documents
    if not set(states) <= set(names):
        raise ValueError(f'{version}: states contain unregistered blocks')
    blocks = {name: states.get(name, [{}, {}]) for name in names}
    for name, (domain, defaults) in blocks.items():
        if set(domain) != set(defaults) or any(defaults[key] not in values for key, values in domain.items()):
            raise ValueError(f'{version}/{name}: invalid defaults')
    return data_version, version, blocks, inputs


def write_reports(results):
    snapshots, indexes, sources = [], {}, []
    unique = {}
    for data_version, version, blocks, inputs in results:
        canonical = json.dumps(blocks, sort_keys=True, separators=(',', ':'))
        if canonical not in unique:
            unique[canonical] = len(snapshots)
            snapshots.append(blocks)
        indexes[str(data_version)] = unique[canonical]
        sources.append({'data_version': data_version, 'version': version, 'inputs': inputs})
    data = json.dumps({'versions': indexes, 'snapshots': snapshots}, sort_keys=True, separators=(',', ':')).encode()
    dest = ROOT / 'src-tauri/data'
    (dest / 'block-registry.json.gz').write_bytes(gzip.compress(data, compresslevel=9, mtime=0))
    (dest / 'block-registry-sources.json').write_text(json.dumps(sources, indent=2) + '\n', encoding='utf-8')


if __name__ == '__main__':
    text = (ROOT / 'src-tauri/src/base_db.rs').read_text(encoding='utf-8')
    section = text.split('pub const SUPPORTED_RELEASES', 1)[1].split('];', 1)[0]
    # mcmeta has no complete 1.13.2 summary; never substitute a nearby version.
    releases = [(int(dv), version) for dv, version in re.findall(r'\((\d+), "([\d.]+)"\)', section) if int(dv) >= 1952]
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        write_reports(list(pool.map(fetch_release, releases)))
