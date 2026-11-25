#!/usr/bin/env python3
"""Lower single Kontakt 8.13.1 reader functions for native_schema/curated.rs.

    tools/re/lower_curated.py <kontakt8 decompile dir> <addr>...
    tools/re/lower_curated.py --versions <addr> <current> <SerType> [element SerType]

The decompile dir is the per-64-KiB Ghidra export (`<base>.c`, functions headed
`// ==== <addr> <name> size=<n>`). Prints one `ProgramSrc` entry per address, in the
form gen_native_schemas.py writes, from reader_lower.py's lowering. A function that
does not lower cleanly (parse error, or callees the IR would need) is reported and
not printed: it stays out of the table. Callees programs.rs already holds are used
from there and not printed again. An address programs.rs holds is lowered anew when
named: its curated entry replaces the generated one (programs.rs cannot be regenerated).
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import reader_lower as rl  # noqa: E402
from cparse import parse_functions  # noqa: E402
from gen_native_schemas import esc  # noqa: E402


def extract(root, addr):
    """The function at `addr`, headed as reader_lower.py reads it."""
    a = int(addr, 16)
    text = (Path(root) / f"{a & ~0xffff:010x}.c").read_text(errors="replace")
    parts = re.split(r"^// ==== ([0-9a-f]+) (\S+) size=\d+\s*$", text, flags=re.M)
    for i in range(1, len(parts), 3):
        if int(parts[i], 16) == a:
            name, body = parts[i + 1], parts[i + 2]
            body = re.sub(r"\b" + re.escape(name) + r"\(", f"FUN_{a:x}(", body, count=1)
            return f"// ===== depth 0 FUN_{a:x} @ {a:x}\n" + body.replace("::", "__")
    sys.exit(f"no function at {addr}")


GENERATED = Path(__file__).parents[2] / "vendor/ni-file/src/kontakt/native_schema/programs.rs"


UNREAD = re.compile(r"\(callx unread-helper-FUN_([0-9a-f]+)\)")
CALL = re.compile(r"\(call (FUN_[0-9a-f]+) ")


def lower(root, wanted, have):
    """Lower `wanted` with the callees programs.rs holds present, so calls resolve by name."""
    text = "".join(extract(root, a) for a in sorted(wanted))
    known = {n for n in re.findall(r"\bFUN_([0-9a-f]+)\(", text) if f"FUN_{n}" in have} - wanted
    text += "".join(extract(root, n) for n in sorted(known))
    funcs = {}
    for name, addr, params, body, decls in parse_functions(text):
        if body[0] == "error":
            print(f"left out {name}: parse error {body[1][:100]}", file=sys.stderr)
        else:
            funcs[name] = (name, addr, params, body, decls)
    return rl.lower_all(funcs, set(funcs))


def main():
    have = set(re.findall(r'name: "(FUN_[0-9a-f]+)"', GENERATED.read_text()))
    root, addrs = sys.argv[1], [a.removeprefix("0x").lower() for a in sys.argv[2:]]
    for a in addrs:
        if f"FUN_{a}" in have:
            print(f"FUN_{a}: replaces programs.rs's", file=sys.stderr)
    have -= {f"FUN_{a}" for a in addrs}
    # Follow helpers the lowering cannot see into until none is new (as fetch_helpers.py
    # does against Ghidra), so a reader comes with the stream helpers it calls.
    wanted = {a for a in addrs if f"FUN_{a}" not in have}
    while True:
        progs = lower(root, wanted, have)
        new = {h for n in wanted for op in progs.get(f"FUN_{n}", {"ops": []})["ops"]
               for h in UNREAD.findall(op) if f"FUN_{h}" not in have} - wanted
        if not new or len(wanted) > 400:
            break
        wanted |= new
    # A program is left out when it is not lowered, still reads through a helper, or calls
    # one that is left out.
    bad = {}
    for a in wanted:
        n = f"FUN_{a}"
        p = progs.get(n)
        if p is None:
            bad[n] = "not lowered"
        elif unread := sorted({op for op in p["ops"] if op.startswith("(callx unread-helper")}):
            bad[n] = ", ".join(unread)
        elif missing := {c for op in p["ops"] if (m := CALL.match(op)) for c in [m.group(1)]} - set(progs) - have:
            bad[n] = f"calls unlowered {sorted(missing)}"
    changed = True
    while changed:
        changed = False
        for a in wanted:
            n = f"FUN_{a}"
            if n in bad:
                continue
            for op in progs[n]["ops"]:
                m = CALL.match(op)
                if m and m.group(1) in bad:
                    bad[n] = f"calls left-out {m.group(1)}"
                    changed = True
                    break
    for n, why in sorted(bad.items()):
        print(f"left out {n}: {why[:300]}", file=sys.stderr)
    order = [a for a in addrs if a in wanted] + sorted(wanted - set(addrs))
    for a in order:
        n = f"FUN_{a}"
        if n in bad:
            continue
        p = progs[n]
        params = ", ".join(f'"{x}"' for x in p["params"])
        body = "\n".join(p["ops"])
        print(f'    ProgramSrc {{ name: "{n}", params: &[{params}], frame: {p["frame"]}, ops: "\\\n{esc(body)}" }},')


def load_rust_programs():
    """Every ProgramSrc of programs.rs and curated.rs, as ir_sim programs."""
    progs = {}
    entry = re.compile(r'name: "(\w+)",\s*params: &\[([^\]]*)\],\s*frame: (\d+),\s*ops: "\\\n(.*?)",?\s*}', re.S)
    for f in (GENERATED, GENERATED.with_name("curated.rs")):
        for name, params, frame, ops in entry.findall(f.read_text()):
            progs[name] = {"params": re.findall(r'"(\w+)"', params), "frame": int(frame),
                           "ops": ops.replace('\\"', '"').replace("\\\\", "\\").split("\n")}
    return progs


def versions(addr, current, sertype, element=None):
    """The stored versions the reader accepts and how completely they decode, found as
    gen_native_schemas.py finds them (reader_schemas.analyze over v in 0..=current+2)."""
    import ir_sim
    import reader_schemas as rs
    progs = load_rust_programs()
    readers = dict(re.findall(r'sertype: (0x[0-9a-fA-F]+|\d+),\s*class: "[^"]*",\s*program: "(\w*)"',
                              GENERATED.read_text() + GENERATED.with_name("curated.rs").read_text()))
    readers = {int(k, 0): v for k, v in readers.items() if v}
    if element is not None:
        rs.ELEMENT_CLASS[sertype] = element
    ir_sim.CURRENT.setdefault(sertype, current)
    row = {"reader_va": "0x" + addr.removeprefix("0x").lower(), "sertype": hex(sertype), "version": hex(current)}
    info = rs.analyze([row], progs, readers)[f"FUN_{row['reader_va'][2:]}"]
    print({v: i["status"] for v, i in info["versions"].items()})


if __name__ == "__main__":
    if sys.argv[1] == "--versions":
        versions(sys.argv[2], int(sys.argv[3], 0), int(sys.argv[4], 0),
                 int(sys.argv[5], 0) if len(sys.argv) > 5 else None)
    else:
        main()
