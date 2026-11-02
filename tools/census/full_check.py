#!/usr/bin/env python3
"""Re-load every census-loaded instrument with today's build and keep only the
diagnostic surface: diagnostic features, UI asset resolution, init unanswered.

Usage: full_check.py <census.json> <out.jsonl> [workers]
One JSON line per instrument; nothing else is kept.
"""
import json, os, subprocess, sys, tempfile
from concurrent.futures import ThreadPoolExecutor
import os

#: Where the survey's own files live: the census, the extracted scripts and every
#: table written by these scripts. Override with ``CENSUS_WORK``; the default is
#: the directory the 2026-11-01 pass used.
WORK = os.environ.get("CENSUS_WORK", "/var/tmp/gaps")

#: Scratch files for the per-instrument `--json` reports.
SCRATCH = os.path.join(WORK, "tmp")


BIN = os.environ.get("KONTRA_INSPECT", "target/release/kontra-inspect")


def one(rec):
    tmp = tempfile.NamedTemporaryFile(suffix=".json", delete=False, dir=SCRATCH)
    tmp.close()
    try:
        p = subprocess.run([BIN, "load", rec["path"], "--json", tmp.name],
                           capture_output=True, text=True, timeout=600)
        d = json.load(open(tmp.name))
        r = d["result"]
        out = {
            "path": rec["path"], "name": rec.get("name"),
            "diagnostics": sorted({(g.get("feature"), g.get("impact")) for g in r.get("diagnostics", [])}),
            "diag_count": len(r.get("diagnostics", [])),
            "ui_resolved": sum(1 for m in r.get("ui", []) for a in m.get("assets", []) if a.get("read_status") == "resolved"),
            "ui_unresolved": sum(1 for m in r.get("ui", []) for a in m.get("assets", []) if a.get("read_status") != "resolved"),
            "callbacks": r.get("compiled_callback_count"),
            "exit": p.returncode,
        }
    except Exception as e:  # noqa: BLE001 - recorded, not swallowed
        out = {"path": rec["path"], "name": rec.get("name"), "error": f"{type(e).__name__}: {e}"}
    finally:
        try:
            os.unlink(tmp.name)
        except OSError:
            pass
    # keep the raw shape small: pairs -> strings
    out["diagnostics"] = [f"{f} | {i}" for f, i in out.get("diagnostics", [])]
    return out


def main():
    recs = [r for r in json.load(open(sys.argv[1])) if r.get("loaded")]
    out = sys.argv[2]
    workers = int(sys.argv[3]) if len(sys.argv) > 3 else 8
    os.makedirs(SCRATCH, exist_ok=True)
    with open(out, "w") as f, ThreadPoolExecutor(workers) as ex:
        for n, res in enumerate(ex.map(one, recs), 1):
            f.write(json.dumps(res) + "\n")
            f.flush()
            if n % 250 == 0:
                print(f"{n}/{len(recs)}", file=sys.stderr, flush=True)
    print("done", len(recs), file=sys.stderr)


if __name__ == "__main__":
    main()
