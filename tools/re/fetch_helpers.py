#!/usr/bin/env python3
"""Decompile every stream helper the readers call, transitively, into one C file.

    tools/re/fetch_helpers.py readers.c helpers_out.c

A helper is any function called with the stream (`param_2`) that is not a known
primitive. The Ghidra project allows one process at a time, so this retries on lock.
"""
import re, subprocess, sys, time, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from reader_schemas import PRIM

here = pathlib.Path(__file__).parent
CALL = re.compile(r"\b(FUN_[0-9a-f]+)\(([^;]*)")


def callees(text):
    out = set()
    for name, rest in CALL.findall(text):
        # a helper receives the stream: first or second argument is param_2 / a copy of it
        if name not in PRIM and re.search(r"param_2|\bpuVar\d+\b|plVar\d+", rest):
            out.add(name[4:])
    return out


def have(path):
    return set(re.findall(r"^// ===== depth \d+ FUN_([0-9a-f]+)", pathlib.Path(path).read_text(errors="replace"), re.M)) if pathlib.Path(path).exists() else set()


def query(addrs):
    while True:
        r = subprocess.run([str(here / "query.sh"), "@" + ",".join("0x" + a for a in addrs), "0"],
                           capture_output=True, text=True)
        if r.returncode == 0 and "Unable to lock" not in r.stderr:
            return r.stdout
        time.sleep(20)


def main():
    src, out = sys.argv[1], sys.argv[2]
    known = have(src) | have(out)
    todo = callees(pathlib.Path(src).read_text(errors="replace")) | callees(pathlib.Path(out).read_text(errors="replace") if pathlib.Path(out).exists() else "")
    todo -= known
    while todo:
        text = query(sorted(todo))
        with open(out, "a") as f:
            f.write(text)
        known |= todo
        todo = callees(text) - known
        print(len(known), "functions", file=sys.stderr)


if __name__ == "__main__":
    main()
