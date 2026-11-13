#!/usr/bin/env python3
"""Merge controls.py sessions into one table per patch and one ranking by mechanism.

    controls_rank.py OUT [LIBRARY=]SESSION_DIR...

LIBRARY= names the session's library where its verdicts carry another name. Each SESSION_DIR is a controls.py OUTDIR (verdicts.json, steps.json, writes.json). Sessions of the
same patch merge, a measured row winning over a 'not reachable' one, an earlier session over a later.
Writes OUT/controls.json (every control: library, patch, control, verdict, reason, mechanism) and
OUT/ranking.json + OUT/RANKING.md: for each mechanism, how many controls it breaks (dead, different,
or 'ours no change' where Kontakt could not be compared, unless it only moves script state) in
how many libraries, most first.
"""
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import controls  # noqa: E402

BROKEN = ("dead", "different", "ours no change")


def session(d):
    """One session's verdicts, with the mechanism recomputed from its writes when missing."""
    rows = json.loads((d / "verdicts.json").read_text())
    if all("mechanism" in v for v in rows):
        return rows
    steps = json.loads((d / "steps.json").read_text())
    writes = json.loads((d / "writes.json").read_text())
    by = {}
    for k, (name, _, tag) in enumerate(steps):
        if name and tag != "context":
            by.setdefault(name, {})[tag] = k
    for v in rows:
        v.setdefault("mechanism", controls.mechanism(controls.moved(writes, by.get(v["control"], {}))))
    return rows


def main():
    out, merged = Path(sys.argv[1]), {}
    for arg in sys.argv[2:]:
        name, _, d = arg.rpartition("=")
        for v in session(Path(d)):
            v["library"] = name or v["library"]
            key = (v["library"], v["patch"], v["control"])
            have = merged.get(key)
            if have is None or (have["verdict"] == "not reachable" and v["verdict"] != "not reachable"):
                merged[key] = v
    rows = sorted(merged.values(), key=lambda v: (v["library"], v["patch"], v["control"]))
    rank = {}
    for v in rows:
        # Without Kontakt, a control that only moves script state may be silent there too.
        unproven = v["verdict"] == "ours no change" and v["mechanism"] == "script state"
        if v["verdict"] in BROKEN and not unproven:
            m = rank.setdefault(v["mechanism"], {"mechanism": v["mechanism"], "controls": 0, "libraries": set(),
                                                 "examples": []})
            m["controls"] += 1
            m["libraries"].add(v["library"])
            if len(m["examples"]) < 3:
                m["examples"].append(f'{v["library"]}: {v["control"]} ({v["verdict"]})')
    ranking = sorted(rank.values(), key=lambda m: (-len(m["libraries"]), -m["controls"], m["mechanism"]))
    for m in ranking:
        m["libraries"] = sorted(m["libraries"])
    totals = {}
    for v in rows:
        t = totals.setdefault(v["library"], {})
        t[v["verdict"]] = t.get(v["verdict"], 0) + 1
    out.mkdir(parents=True, exist_ok=True)
    (out / "controls.json").write_text(json.dumps(rows, indent=1))
    (out / "ranking.json").write_text(json.dumps({"totals": totals, "ranking": ranking}, indent=1))
    lines = ["# Panel controls: what breaks, by mechanism", "",
             "Each control moved min → max through its own panel code (tools/oracle/controls.py). A control "
             "counts against its mechanism when it is dead, different from Kontakt, or (no Kontakt to compare) "
             "does nothing in KONTRA (unless it only moves script state).", "", "| library | " + " | ".join(sorted({k for t in totals.values() for k in t}))
             + " |"]
    keys = sorted({k for t in totals.values() for k in t})
    lines.append("|---" * (len(keys) + 1) + "|")
    lines += [f"| {lib} | " + " | ".join(str(t.get(k, 0)) for k in keys) + " |" for lib, t in sorted(totals.items())]
    lines += ["", "| mechanism | controls | libraries | examples |", "|---|---|---|---|"]
    lines += [f'| {m["mechanism"]} | {m["controls"]} | {len(m["libraries"])} | {"; ".join(m["examples"])} |'
              for m in ranking]
    (out / "RANKING.md").write_text("\n".join(lines) + "\n")
    print(out / "RANKING.md")


if __name__ == "__main__":
    main()
