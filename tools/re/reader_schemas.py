#!/usr/bin/env python3
"""Per-version field layouts of Kontakt's native readers.

    tools/re/reader_schemas.py ser_classes.tsv readers.c > reader_schemas.tsv

readers.c is `tools/re/query.sh "@<reader addresses>" 0` output (kept outside the
repo). Each reader branches on the stored version (`param_3`) via `if (param_3 ==
V)` or `switch (param_3) { case V: ... }`; inside a branch the reads happen in
source order. Primitive readers (Kontakt 8.13.1, identified by decompiling them):

  F  FUN_142a780f0  4 bytes (u32/f32, byte-swapped for big-endian streams)
  B  FUN_142a780c0  u8          H  FUN_142a78220  u16       I  FUN_142a78270  u32
  O  FUN_14093d6c0  nested structured object: u8 type, u16 version, its reader
  S  FUN_14093ff90  string      L  FUN_140cefb80  u32 count + strings
  X  FUN_140ab5da0  common FX header: F F F B
  E  FUN_140cf0360 / 0420 / 0690 / 05a0 / 00f0 / 0080 / 0840, FUN_140a84920 / 47f0:
     u32 enum, validated (unknown values throw)

A layout is the read sequence for one version, e.g. `FFFFFFFB`. Branches that
fall through or jump between versions are resolved by following `goto` labels and
switch fall-through; anything still ambiguous is marked `?` rather than guessed.
"""
import csv
import re
import sys

PRIM = {
    "FUN_142a780f0": "F", "FUN_142a780c0": "B", "FUN_142a78220": "H", "FUN_142a78270": "I",
    "FUN_14093d6c0": "O", "FUN_14093ff90": "S", "FUN_140cefb80": "L", "FUN_140ab5da0": "X",
}
for e in ("140cf0360", "140cf0420", "140cf0690", "140cf05a0", "140cf00f0", "140cf0080",
          "140cf0840", "140a84920", "140a847f0"):
    PRIM["FUN_" + e] = "E"
CALL = re.compile(r"\b(FUN_[0-9a-f]+)\(")


def functions(path):
    cur, body = None, []
    for line in open(path, errors="replace"):
        m = re.match(r"// ===== depth \d+ (FUN_[0-9a-f]+) @ ([0-9a-f]+)", line)
        if m:
            if cur:
                yield cur, body
            cur, body = "0x" + m.group(2), []
        elif cur:
            body.append(line.rstrip("\n"))
    if cur:
        yield cur, body


def layouts(lines):
    """Map version -> read sequence by walking the body from each version entry."""
    labels, entries, order = {}, {}, []
    on_version = False  # inside `switch(param_3)` rather than a switch on a read value
    for i, l in enumerate(lines):
        if re.match(r"^\s*(code_r0x[0-9a-f]+|LAB_[0-9a-f]+):", l):
            labels[l.strip().rstrip(":")] = i
        if "switch(" in l:
            on_version = "param_3" in l
        cases = re.findall(r"^\s*case (0x[0-9a-f]+|\d+):", l) if on_version else []
        for v in re.findall(r"param_3 == (0x[0-9a-f]+|\d+)\)", l) + cases:
            entries.setdefault(int(v, 0), i)
            order.append(int(v, 0))
        # `if (param_3 != V) { throw }` guards a single-version body after the block.
        m = re.search(r"if \(param_3 != (0x[0-9a-f]+|\d+)\) \{", l)
        if m:
            depth, j = 0, i
            while j < len(lines):
                depth += lines[j].count("{") - lines[j].count("}")
                if depth <= 0 and j > i:
                    break
                j += 1
            entries.setdefault(int(m.group(1), 0), j)
    out = {}
    for v, start in entries.items():
        seq, i, depth, steps = [], start + 1, 0, 0
        while i < len(lines) and steps < 4000:
            steps += 1
            l = lines[i]
            # Leaving the branch's own block (if-branch closes, or switch ends).
            if depth + l.count("{") - l.count("}") < 0 and "}" in l and "{" not in l:
                # A closing brace of the branch may be followed by shared code
                # (if/else chains fall into common tail reads); keep walking.
                depth = 0
                i += 1
                continue
            terminal = re.search(r"\bbreak;|\breturn;|_CxxThrowException|^\s*default:", l)
            if terminal and depth == 0:
                break
            g = re.match(r"^\s*goto (\S+);", l)
            if g and g.group(1) in labels and depth == 0:
                i = labels[g.group(1)] + 1
                continue
            other = re.search(r"if \(param_3 [!=]= (0x[0-9a-f]+|\d+)\)", l)
            if other and int(other.group(1), 0) != v and depth == 0:
                break
            if re.match(r"^\s*case (0x[0-9a-f]+|\d+):", l) and seq:
                pass  # switch fall-through: the next case's reads belong to us too
            for name in CALL.findall(l):
                if name in PRIM:
                    cond = depth > 0 or (re.search(r"^\s*if \(", lines[i - 1]) and "{" not in lines[i - 1])
                    seq.append(PRIM[name] + ("?" if cond else ""))
                elif "param_2" in l[l.index(name):]:
                    seq.append("?{" + name[4:] + "}")  # untyped helper reading the stream
            depth += l.count("{") - l.count("}")
            depth = max(depth, 0)
            i += 1
        out[v] = "".join(seq)
    return out


def main():
    classes = {r["reader_va"]: r for r in csv.DictReader(open(sys.argv[1]), delimiter="\t")}
    print("class\tsertype\tnative_version\tstored_version\tlayout\treader_va")
    for addr, body in functions(sys.argv[2]):
        rows = [r for r in classes.values() if int(r["reader_va"], 16) == int(addr, 16)]
        lay = layouts(body)
        for r in rows or [{"class": "?", "sertype": "?", "version": "?", "reader_va": addr}]:
            if not lay:
                print(f"{r['class']}\t{r['sertype']}\t{r['version']}\t-\t{''.join(PRIM.get(n, '') for n in CALL.findall(chr(10).join(body)))}\t{addr}")
            for v in sorted(lay):
                print(f"{r['class']}\t{r['sertype']}\t{r['version']}\t0x{v:X}\t{lay[v]}\t{addr}")


if __name__ == "__main__":
    main()
