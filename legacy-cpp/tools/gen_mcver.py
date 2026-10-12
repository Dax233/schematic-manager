"""Generate src/mcver_table.inc from the malilib-extracted data version table."""
import json, os

HERE = os.path.dirname(os.path.abspath(__file__))
PROJ = os.path.dirname(HERE)
WS = os.path.dirname(PROJ)
SRC = os.path.join(WS, 'data_version_table.json')

# 从 Minecraft Wiki 补充 malilib 0.29.3 之后的新版本
EXTRA = [
    (4998, "26.3-snapshot-1"), (4999, "26.3-snapshot-2"), (5001, "26.3-snapshot-3"),
    (5003, "26.3-snapshot-4"), (5004, "26.3-snapshot-5"), (5005, "26.3-snapshot-6"),
    (5009, "26.3-snapshot-7"), (5010, "26.3-snapshot-8"), (5011, "26.3-snapshot-9"),
    (5015, "26.3-snapshot-10"), (5017, "26.3-pre-1"), (5018, "26.3-pre-2"),
    (5019, "26.3-pre-3"), (5020, "26.3-rc-1"), (5021, "26.3-rc-2"), (5022, "26.3-rc-3"),
    (5023, "26.3"),
    (5119, "26.4-snapshot-1"), (5120, "26.4-snapshot-2"), (5122, "26.4-snapshot-3"),
]

def main():
    table = {}
    if os.path.exists(SRC):
        table = {int(k): v for k, v in json.load(open(SRC, encoding='utf-8')).items()}
    for dv, name in EXTRA:
        table[dv] = name
    table.pop(9999, None)          # 去掉 FUTURE 占位
    items = sorted(table.items())
    out = os.path.join(PROJ, 'src', 'mcver_table.inc')
    with open(out, 'w', encoding='utf-8', newline='\n') as f:
        f.write('// 自动生成，请勿手工编辑。来源: malilib Schema 枚举 + Minecraft Wiki\n')
        f.write('static const DataVerEntry kDataVers[] = {\n')
        for dv, name in items:
            f.write('    { %d, "%s" },\n' % (dv, name.replace('\\', '\\\\').replace('"', '\\"')))
        f.write('};\n')
        f.write('static const size_t kDataVersCount = sizeof(kDataVers) / sizeof(kDataVers[0]);\n')
    print('generated %s : %d entries, range %d..%d' % (out, len(items), items[0][0], items[-1][0]))

if __name__ == '__main__':
    main()
