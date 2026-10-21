#!/usr/bin/env python3
"""Decompile every .pdata function of a PE with radare2 + Ghidra decompiler (pdg).

Resumable: each batch of functions runs in its own `r2 -i` process and writes a
`.done` marker; re-running skips finished batches. Output:

  OUT/manifest.tsv          rva, end_rva, size, section (one row per function)
  OUT/fn/<rva:08x>.c        pseudocode for one function (pdg output, plain text)
  OUT/batches/<n>.done      JSON status per batch (ok / failed addresses)

Usage:
  corpus_build.py --binary FILE --out DIR [--workers 6] [--batch 150] [--limit N]
"""
import argparse
import json
import os
import re
import struct
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

IMAGE_BASE = 0x180000000
BEGIN = "=== BEGIN {va:x} ==="
END = "=== END {va:x} ==="
R2_TIMEOUT = 1800  # seconds per batch process
RETRY = False


def pe_sections(data):
    e = struct.unpack_from("<I", data, 0x3C)[0]
    nsec = struct.unpack_from("<H", data, e + 6)[0]
    optsz = struct.unpack_from("<H", data, e + 20)[0]
    so = e + 24 + optsz
    secs = []
    for i in range(nsec):
        off = so + i * 40
        name = data[off:off + 8].rstrip(b"\0").decode("latin-1")
        vsize, vaddr, rsize, rptr = struct.unpack_from("<IIII", data, off + 8)
        secs.append({"name": name, "rva": vaddr, "vsize": vsize, "rptr": rptr, "rsize": rsize})
    return secs


def section_of(secs, rva):
    for s in secs:
        if s["rva"] <= rva < s["rva"] + max(s["vsize"], s["rsize"]):
            return s["name"]
    return "?"


def pdata_functions(data, secs):
    """Return sorted [(begin_rva, end_rva, section)] from the exception directory."""
    pd = next(s for s in secs if s["name"] == ".pdata")
    out = []
    for i in range(pd["rsize"] // 12):
        begin, end, unwind = struct.unpack_from("<III", data, pd["rptr"] + i * 12)
        # Chained entries (end <= begin) are unwind continuations, not functions.
        if begin == 0 or end <= begin:
            continue
        out.append((begin, end, section_of(secs, begin)))
    seen = set()
    uniq = []
    for row in sorted(out):
        if row[0] not in seen:
            seen.add(row[0])
            uniq.append(row)
    return uniq


def write_manifest(path, funcs):
    with open(path, "w") as f:
        f.write("rva\tend_rva\tsize\tsection\n")
        for b, e, s in funcs:
            f.write(f"{b:08x}\t{e:08x}\t{e - b}\t{s}\n")


def make_script(path, rvas):
    lines = ["aa", "e scr.color=0", "e scr.interactive=false", "e scr.utf8=false"]
    for rva in rvas:
        va = IMAGE_BASE + rva
        lines.append(f"?e {BEGIN.format(va=va)}")
        lines.append(f"af @ 0x{va:x}")
        lines.append(f"pdg @ 0x{va:x}")
        lines.append(f"?e {END.format(va=va)}")
    Path(path).write_text("\n".join(lines) + "\n")


def parse_output(text):
    """Split r2 output into {rva: body} for functions whose BEGIN/END markers both appear."""
    res = {}
    cur = None
    buf = []
    for line in text.splitlines():
        m = re.match(r"^=== BEGIN ([0-9a-f]+) ===$", line)
        if m:
            cur = int(m.group(1), 16) - IMAGE_BASE
            buf = []
            continue
        m = re.match(r"^=== END ([0-9a-f]+) ===$", line)
        if m and cur is not None and int(m.group(1), 16) - IMAGE_BASE == cur:
            res[cur] = "\n".join(buf).strip("\n") + "\n"
            cur = None
            continue
        if cur is not None:
            buf.append(line)
    return res


def run_batch(binary, out, idx, rvas):
    tag = "retry-" if RETRY else ""
    done_path = out / "batches" / f"{tag}{idx:05d}.done"
    if done_path.exists():
        return "skip"
    with tempfile.NamedTemporaryFile("w", suffix=".r2", delete=False, dir=out / "tmp") as tf:
        script = tf.name
    make_script(script, rvas)
    try:
        proc = subprocess.run(
            ["r2", "-q", "-i", script, binary],
            capture_output=True, text=True, timeout=R2_TIMEOUT,
        )
        stdout = proc.stdout
        status = "ok" if proc.returncode == 0 else f"rc={proc.returncode}"
    except subprocess.TimeoutExpired as ex:
        stdout = (ex.stdout or b"").decode("utf-8", "replace") if isinstance(ex.stdout, bytes) else (ex.stdout or "")
        status = "timeout"
    finally:
        os.unlink(script)
    got = parse_output(stdout)
    for rva, body in got.items():
        (out / "fn" / f"{rva:08x}.c").write_text(body)
    missing = [r for r in rvas if r not in got]
    done_path.write_text(json.dumps({"batch": idx, "status": status,
                                     "count": len(rvas), "ok": len(got),
                                     "missing": [f"{r:08x}" for r in missing][:200],
                                     "missing_count": len(missing)}, indent=1))
    return f"{status} ok={len(got)} missing={len(missing)}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--workers", type=int, default=6)
    ap.add_argument("--batch", type=int, default=150)
    ap.add_argument("--limit", type=int, default=0, help="only the first N functions (smoke test)")
    ap.add_argument("--sections", default=".text,IPPCODE,.text001",
                    help="comma list of sections whose functions are decompiled")
    ap.add_argument("--retry-missing", action="store_true",
                    help="only functions without fn/<rva>.c (after timeouts/crashes); "
                         "uses --batch small and separate done markers")
    args = ap.parse_args()
    global RETRY
    RETRY = args.retry_missing

    out = Path(args.out)
    for sub in ("fn", "batches", "tmp"):
        (out / sub).mkdir(parents=True, exist_ok=True)
    data = Path(args.binary).read_bytes()
    secs = pe_sections(data)
    want = set(args.sections.split(","))
    funcs = [f for f in pdata_functions(data, secs) if f[2] in want]
    if args.retry_missing:
        funcs = [f for f in funcs if not (out / "fn" / f"{f[0]:08x}.c").exists()]
        print(f"retry: {len(funcs)} functions without output", flush=True)
    if args.limit:
        funcs = funcs[: args.limit]
    if not args.retry_missing:  # retry runs cover a subset; never overwrite the full manifest
        write_manifest(out / "manifest.tsv", funcs)
    rvas = [b for b, _, _ in funcs]
    batches = [rvas[i:i + args.batch] for i in range(0, len(rvas), args.batch)]
    print(f"functions={len(rvas)} batches={len(batches)} workers={args.workers}", flush=True)

    jobs = [(i, b) for i, b in enumerate(batches)]
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futs = {pool.submit(run_batch, args.binary, out, i, b): i for i, b in jobs}
        for fut in futs:
            i = futs[fut]
            try:
                res = fut.result()
            except Exception as ex:  # keep going; the batch stays unfinished and is retried next run
                res = f"error {ex}"
            print(f"batch {i}: {res}", flush=True)


if __name__ == "__main__":
    sys.exit(main())
