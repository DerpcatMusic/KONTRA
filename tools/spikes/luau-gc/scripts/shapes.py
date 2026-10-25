import json, sys
from collections import Counter, defaultdict
d = json.load(open(sys.argv[1]))
o = d['objects']
bytes_by = defaultdict(int); count_by = Counter()
for a, x in o.items():
    if x['type'] != 'table': continue
    keys = []
    p = x.get('pairs', [])
    for i in range(0, len(p), 2):
        k = p[i]
        if k in o and o[k]['type'] == 'string': keys.append(o[k]['data'])
    sig = tuple(sorted(set(keys)))[:8]
    kinds = 'arr' if x.get('array') else ''
    key = (sig, kinds) if sig else ('(no string keys)', kinds, len(p)//2 > 0)
    bytes_by[key] += x['size']; count_by[key] += 1
tot = sum(bytes_by.values())
print('| shape (string keys, first 8) | tables | MiB | % |')
print('|---|---:|---:|---:|')
for key, b in sorted(bytes_by.items(), key=lambda kv: -kv[1])[:25]:
    print(f"| {','.join(key[0])[:110]} {key[1]} | {count_by[key]} | {b/2**20:.2f} | {100*b/tot:.1f} |")
