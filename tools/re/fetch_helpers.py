#!/usr/bin/env python3
"""Decompile the functions Kontakt's serializable readers need, transitively.

    tools/re/fetch_helpers.py ser_classes.tsv decompiled.c

Seeds are the reader addresses of ser_classes.tsv. A callee is fetched when it
receives the stream (a primitive reader or helper) or when its result decides a
branch (predicates such as "is this module type X"). Allocators and exception glue
are never needed: they cannot change what is read. Output is appended to
decompiled.c, which stays outside the repository because it is Kontakt's code.
The Ghidra project admits one process at a time, so a locked query is retried.
"""
import csv
import re
import subprocess
import sys
import time
from pathlib import Path

here = Path(__file__).parent
sys.path.insert(0, str(here))
from cparse import parse_functions  # noqa: E402
from reader_lower import EXTRA_SEEDS, OVERRIDES, PRIMS, PRIM_STREAM_ARG, load_functions, mentions, stream_params, vars_in  # noqa: E402

NOISE = re.compile(r"FUN_(1443c|140517|1407fa|1407f6|140714|1404f6|1407039d|140709720|140711)")


def calls_in(node, out):
    if isinstance(node, tuple):
        if node[0] == "call" and node[1][0] == "var" and node[1][1].startswith("FUN_"):
            out.append(node)
        for x in node[1:]:
            calls_in(x, out)
    elif isinstance(node, list):
        for x in node:
            calls_in(x, out)


def condition_exprs(node, out):
    if isinstance(node, tuple):
        k = node[0]
        if k in ("if", "while"):
            out.append(node[1])
        elif k == "do":
            out.append(node[2])
        elif k == "for" and node[2] is not None:
            out.append(node[2])
        elif k == "switch":
            out.append(node[1])
        elif k == "tern":
            out.append(node[1])
        for x in node[1:]:
            condition_exprs(x, out)
    elif isinstance(node, list):
        for x in node:
            condition_exprs(x, out)


def wanted_callees(fn, streams):
    """Callees of fn that take the stream or feed one of its branches."""
    _, _, _, body, _ = fn
    conds = []
    condition_exprs(body, conds)
    cond_vars = set(v for c in conds for v in vars_in(c))
    stream = streams.get(fn[0], set())
    found = set()
    seen_calls = []
    calls_in(body, seen_calls)
    for c in seen_calls:
        name, args = c[1][1], c[2]
        if name in PRIMS or NOISE.match(name):
            continue
        if any(mentions(a, stream) for a in args):
            found.add(name)
    # callees fed a value just read from the stream (a validating/mapping converter)
    read_vars = set()

    def reads(n):
        if isinstance(n, tuple):
            if n[0] == "assign" and n[2][0] == "var" and n[3][0] == "call" and n[3][1][0] == "var" \
                    and n[3][1][1] in PRIMS:
                read_vars.add(n[2][1])
            for x in n[1:]:
                reads(x)
        elif isinstance(n, list):
            for x in n:
                reads(x)

    reads(body)
    for c in seen_calls:
        if c[1][1] not in PRIMS and not NOISE.match(c[1][1]) and any(mentions(a, read_vars) for a in c[2]):
            found.add(c[1][1])
    for c in conds:
        inner = []
        calls_in(c, inner)
        found |= {x[1][1] for x in inner if not NOISE.match(x[1][1])}

    def walk(n):
        if isinstance(n, tuple):
            if n[0] == "assign" and n[2][0] == "var" and n[2][1] in cond_vars and n[3][0] == "call" \
                    and n[3][1][0] == "var" and n[3][1][1].startswith("FUN_") and not NOISE.match(n[3][1][1]):
                found.add(n[3][1][1])
            for x in n[1:]:
                walk(x)
        elif isinstance(n, list):
            for x in n:
                walk(x)

    walk(body)
    return {n for n in found if n not in PRIMS}


def query(addrs):
    while True:
        r = subprocess.run([str(here / "query.sh"), "@" + ",".join(addrs), "0"], capture_output=True, text=True)
        if r.returncode == 0 and "Unable to lock" not in r.stdout + r.stderr:
            return r.stdout
        time.sleep(15)


def main():
    classes, out = sys.argv[1], sys.argv[2]
    seeds = {"FUN_" + r["reader_va"][2:] for r in csv.DictReader(open(classes), delimiter="\t")} | set(EXTRA_SEEDS)
    tried = set()
    while True:
        funcs = load_functions([out]) if Path(out).exists() else {}
        todo = seeds - set(funcs) - tried
        if funcs:
            streams = stream_params(funcs)
            for fn in funcs.values():
                todo |= wanted_callees(fn, streams)
        todo -= set(funcs) | tried | set(OVERRIDES)
        if not todo:
            break
        tried |= todo
        text = query(["0x" + n[4:] for n in sorted(todo)])
        with open(out, "a") as f:
            f.write(text)
        print(len(funcs), "functions;", len(todo), "fetched", file=sys.stderr)
    print("done:", len(funcs), "functions", file=sys.stderr)


if __name__ == "__main__":
    main()
