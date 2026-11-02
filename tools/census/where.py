#!/usr/bin/env python3
"""Print the source line for each error in a compile.tsv-like stream."""
import re
import sys

path = sys.argv[1]
want = sys.argv[2] if len(sys.argv) > 2 else None
seen = 0
for line in open(path):
    parts = line.rstrip("\n").split("\t")
    if parts[0] != "err":
        continue
    if want and want not in parts[2]:
        continue
    m = re.search(r"at byte (\d+)", parts[2])
    if not m:
        print(parts[1], "??", parts[2])
        continue
    off = int(m.group(1))
    src = open(parts[1], "rb").read()
    pre = src[:off]
    ln = pre.count(b"\n") + 1
    col = off - (pre.rfind(b"\n") + 1)
    text = src.split(b"\n")[ln - 1].decode("utf-8", "replace")
    print(f"{parts[1]}:{ln}:{col + 1}: {parts[2]}\n    {text.strip()}")
    seen += 1
    if want and seen >= int(sys.argv[3] if len(sys.argv) > 3 else 10):
        break
