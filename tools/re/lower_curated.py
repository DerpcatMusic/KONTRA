#!/usr/bin/env python3
"""Lower single Kontakt 8.13.1 reader functions for native_schema/curated.rs.

    tools/re/lower_curated.py <kontakt8 decompile dir> <addr>...

The decompile dir is the per-64-KiB Ghidra export (`<base>.c`, functions headed
`// ==== <addr> <name> size=<n>`). Prints one `ProgramSrc` entry per address, in the
form gen_native_schemas.py writes, from reader_lower.py's lowering. A function that
does not lower cleanly (parse error, or callees the IR would need) is reported and
not printed: it stays out of the table.
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


def main():
    root, addrs = sys.argv[1], [a.removeprefix("0x").lower() for a in sys.argv[2:]]
    text = "".join(extract(root, a) for a in addrs)
    funcs = {}
    for name, addr, params, body, decls in parse_functions(text):
        if body[0] == "error":
            print(f"left out {name}: parse error {body[1][:100]}", file=sys.stderr)
        else:
            funcs[name] = (name, addr, params, body, decls)
    progs = rl.lower_all(funcs, set(funcs))
    for a in addrs:
        n = f"FUN_{a}"
        p = progs.get(n)
        if p is None:
            print(f"left out {n}: not lowered", file=sys.stderr)
            continue
        called = {m.group(1) for op in p["ops"] if (m := re.match(r"\(call (\w+) ", op))}
        if missing := called - set(progs):
            print(f"left out {n}: calls unlowered {sorted(missing)}", file=sys.stderr)
            continue
        params = ", ".join(f'"{x}"' for x in p["params"])
        body = "\n".join(p["ops"])
        print(f'    ProgramSrc {{ name: "{n}", params: &[{params}], frame: {p["frame"]}, ops: "\\\n{esc(body)}" }},')


if __name__ == "__main__":
    main()
