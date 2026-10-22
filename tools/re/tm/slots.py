#!/usr/bin/env python3
"""slots.py CLASS...: each vtable slot of the class -> decompile header (name, size) at that VA."""
import os
import re, subprocess, sys
from pathlib import Path
from pathlib import Path
SRC = Path(os.environ.get("KONTAKT8_SRC", "/data/dev/workspace/Research/KONTRA/source/kontakt8"))
hdr = {}
for f in SRC.glob("*.c"):
    for m in re.finditer(r"^// ==== ([0-9a-f]+) (\S+) size=(\d+)", f.read_text(errors="replace"), re.M):
        hdr[int(m.group(1), 16)] = (m.group(2), int(m.group(3)), f.name)
out = subprocess.run([sys.executable, str(Path(__file__).with_name("rtti_vt.py")), *sys.argv[1:]],
                     capture_output=True, text=True).stdout
for line in out.splitlines():
    name, *_, slots = line.split("\t")
    print(f"## {name}")
    for i, s in enumerate(slots.split("=", 1)[1].split(",")):
        rva = int(s, 16)
        if 0x1000 <= rva < 0x3000000:
            va = 0x140000000 + rva
            info = hdr.get(va)
            print(f"  [{i:2}] {va:#x} " + (f"{info[0]} size={info[1]} ({info[2]})" if info else "-"))
        else:
            print(f"  [{i:2}] {s} (not a code pointer)")
