"""Attribute a Luau heap dump (lua_memorydump JSON) to the globals that own it.

Usage: python3 -I attribute.py <dump.json> [top_n]

Roots are the `_G` entries, then the registry entries (script environments and
host anchors), then the main thread. Each root claims what no earlier root
reached, so this is first-claim ownership, not a dominator tree. Totals add up
to the dump's byte count. Back-edges are cut: `parent` (every element points up
at its program, so reaching one element reaches the whole model) and `env`.
"""

import json
import sys
from collections import defaultdict, deque

path = sys.argv[1]
top_n = int(sys.argv[2]) if len(sys.argv) > 2 else 25

with open(path, "r", encoding="utf-8") as f:
    dump = json.load(f)

objects = dump["objects"]
roots = dump["roots"]
total_bytes = sum(o["size"] for o in objects.values())


BLOCKED = {roots["registry"], roots["mainthread"]}


def refs(obj):
    """Addresses an object points at (keys, values, arrays, metatables, protos...)."""
    out = []
    for key, value in obj.items():
        if key in ("type", "cat", "size", "data", "stacknames", "env", "parent"):
            continue
        stack = [value]
        while stack:
            v = stack.pop()
            if isinstance(v, str):
                if v in objects and v not in BLOCKED:
                    out.append(v)
            elif isinstance(v, list):
                stack.extend(v)
    return out


def items_of(table):
    """(key, value) pairs of a table object, keys rendered as text."""
    pairs = table.get("pairs", [])
    out = []
    for i in range(0, len(pairs) - 1, 2):
        k, v = pairs[i], pairs[i + 1]
        out.append((k, v))
    return out


def label(addr):
    o = objects.get(addr)
    if o is None:
        return "?"
    if o["type"] == "string":
        return o.get("data", "")
    return o["type"]


# Registry slot 2 is LUA_RIDX_GLOBALS in Luau; slot 1 is the main thread.
registry = objects[roots["registry"]]
globals_addr = registry["array"][1]
globals_obj = objects[globals_addr]

claimed = {}
rows = []


def claim(start, owner):
    """Claims everything new reachable from `start`; returns the bytes claimed."""
    if start in claimed or start not in objects:
        return 0
    claimed[start] = owner
    added = objects[start]["size"]
    queue = deque([start])
    while queue:
        addr = queue.popleft()
        for child in refs(objects[addr]):
            if child not in claimed:
                claimed[child] = owner
                added += objects[child]["size"]
                queue.append(child)
    return added


def named_children(table_addr):
    """(name, address) for every string-keyed entry of a table, plus its array slots."""
    out = []
    table = objects[table_addr]
    for k, v in items_of(table):
        if isinstance(v, str) and v in objects:
            name = objects[k]["data"] if isinstance(k, str) and k in objects and objects[k]["type"] == "string" else str(k)
            out.append((name, v))
    for i, v in enumerate(table.get("array", []) or []):
        if isinstance(v, str) and v in objects:
            out.append((f"[{i + 1}]", v))
    return out


# Roots, in claim order: the globals, then the registry (script environments,
# host anchors), then the main thread. Each root claims what no earlier root reached.
for name, addr in named_children(globals_addr):
    rows.append((f"_G.{name}", claim(addr, f"_G.{name}")))
for name, addr in named_children(roots["registry"]):
    rows.append((f"registry.{name}", claim(addr, f"registry.{name}")))
rows.append(("mainthread", claim(roots["mainthread"], "mainthread")))

leftover = 0
for addr, o in objects.items():
    if addr not in claimed:
        leftover += o["size"]
        claimed[addr] = "(unreachable from roots)"
rows.append(("(unreachable from roots)", leftover))

rows.sort(key=lambda r: -r[1])
print(f"dump objects: {len(objects)}, bytes: {total_bytes} ({total_bytes / 2**20:.1f} MiB)")
print()
print("| global | MiB | % |")
print("|---|---:|---:|")
for name, b in rows[:top_n]:
    print(f"| `{name}` | {b / 2**20:.2f} | {100.0 * b / total_bytes:.1f} |")
print(f"| (remaining {max(0, len(rows) - top_n)} globals) | {sum(b for _, b in rows[top_n:]) / 2**20:.2f} | |")
print()

# Shape of the tables, over the whole heap: what is static data and what is code.
kinds = defaultdict(lambda: [0, 0])
for addr, o in objects.items():
    if o["type"] != "table":
        kinds[o["type"]][0] += 1
        kinds[o["type"]][1] += o["size"]
        continue
    items = items_of(o)
    if not items:
        kind = "table: empty"
    else:
        all_numbers = all(not (isinstance(k, str) and k in objects) and not (isinstance(v, str) and v in objects) for k, v in items)
        values = [v for _, v in items]
        has_obj = any(isinstance(v, str) and v in objects and objects[v]["type"] != "string" for v in values) or any(
            isinstance(k, str) and k in objects and objects[k]["type"] != "string" for k, _ in items
        )
        has_str = any(isinstance(v, str) and v in objects and objects[v]["type"] == "string" for v in values)
        if has_obj:
            kind = "table: holds objects"
        elif has_str:
            kind = "table: strings only"
        elif all_numbers:
            kind = "table: numbers only (or empty slots)"
        else:
            kind = "table: other scalars"
    kinds[kind][0] += 1
    kinds[kind][1] += o["size"]

print("| kind | objects | MiB | % |")
print("|---|---:|---:|---:|")
for kind, (count, b) in sorted(kinds.items(), key=lambda kv: -kv[1][1]):
    print(f"| {kind} | {count} | {b / 2**20:.2f} | {100.0 * b / total_bytes:.1f} |")
print()

# Largest tables by slot count, with the global that owns them.
biggest = sorted(
    ((len(o.get("pairs", [])) + len(o.get("array", [])), o["size"], addr) for addr, o in objects.items() if o["type"] == "table"),
    reverse=True,
)[:10]
print("| table slots | bytes | owner |")
print("|---:|---:|---|")
for slots, size, addr in biggest:
    print(f"| {slots} | {size} | `{claimed.get(addr, '?')}` |")
