#!/usr/bin/env python3
"""Does each control on a library's panel change our audio the way it changes Kontakt's?

    tools/oracle/controls.py NKI OUTDIR [--only REGEX] [--with NAME=V,...] [--src DIR] [--inspect BIN]

Lists every KSP ui control of the instrument's script 0 (its range, menu items, saved default, and
what its `on ui_control` body writes), then moves each through min/max/default, ONE session per
side. The mechanism is the library's own code, in two passes:
  A (ours only): a copy of script 0 gains a step table in its `on controller` callback;
    CC 103·128 + CC 102 = step k sets that control and runs a copy of its ui_control body. The
    trace records, by name, every set_engine_par each step makes.
  B (ours and Kontakt): the instrument as saved, plus a probe in the next free script slot that
    replays step k's set_engine_par calls on the same CC. Library scripts are usually protected
    (Kontakt refuses to replace their source), so this is how Kontakt sees the same moves.
After each step the same phrase plays (three single notes, low/mid/high, then a chord). Per row,
the level, octave-band spectrum, attack, release tail, width and balance of the max step minus the
min step are compared, ours against Kontakt's (pass B). A control whose body sets no engine
parameter (state read in note callbacks) is "not reachable" for Kontakt here.

--with NAME=V sets those controls (through their bodies) before any row, e.g. the page's on switch,
so a knob behind a bypassed effect can be heard. Writes OUTDIR/controls.json (all numbers),
OUTDIR/verdicts.json (library, patch, control, verdict, reason) and OUTDIR/CONTROLS.md. Verdicts:
same / different / dead (Kontakt changes, ours does not) / not reachable (not driven).
--plan writes the script and scenario only (no renders). --src: a KONTRA_SCRIPT_DUMP of the
instrument (default: dumped here with the render example).
"""
import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import compare  # noqa: E402  (wav loading; the native capture starts PRE seconds early)
import scoreboard as sb  # noqa: E402  (binary lookup, metrics)

R = 48000
PERIOD, PHRASE_AT = 5.0, 0.5  # seconds per step; the phrase starts this long after the step's CC
SINGLES = [(36, 0.0, 0.7), (60, 0.8, 0.7), (84, 1.6, 0.7)]  # key, start, length (s after PHRASE_AT)
CHORD = ([48, 55, 64, 72], 2.4, 1.4)
VELOCITY = 96
DECLARE = re.compile(r"declare\s+ui_(switch|slider|knob|menu|value_edit|button)\s+\$(\w+)\s*(?:\(([^)]*)\))?",
                     re.I)
BODY = re.compile(r"^on\s+ui_control\s*\(\s*\$(\w+)\s*\)(.*?)^end\s+on", re.I | re.M | re.S)
FUNCTION = re.compile(r"^function\s+(\w+)(.*?)^end\s+function", re.I | re.M | re.S)
ITEM = re.compile(r"add_menu_item\s*\(\s*\$(\w+)\s*,\s*\"[^\"]*\"\s*,\s*(-?\d+)\s*\)", re.I)
ACTIONS = ("load_ir_sample", "purge_group", "change_vol", "change_tune", "set_voice_par", "_set_engine_par",
           "set_key_type", "load_array", "set_snapshot_type", "set_skin_offset")


def library(nki):
    """The library folder an instrument sits in: the one under the libraries root ('Kontakt')."""
    for d in nki.parents:
        if d.parent.name == "Kontakt":
            return re.sub(r"\s*\[.*", "", d.name)
    return nki.parent.name

def strip(src):
    return re.sub(r"\{[^}]*\}", "", src.replace("\r", ""))


def number(text, consts):
    text = text.strip()
    if re.fullmatch(r"-?\d+", text):
        return int(text)
    return consts.get(text.lstrip("$").lower())


def reached(body, functions):
    """The body plus every function it calls, transitively."""
    seen, todo, text = set(), [body], []
    while todo:
        b = todo.pop()
        text.append(b)
        for c in re.findall(r"\bcall\s+(\w+)", b, re.I):
            if c.lower() not in seen and c.lower() in functions:
                seen.add(c.lower())
                todo.append(functions[c.lower()])
    return "\n".join(text)


def listed(binary, nki, out):
    """Script 0's controls as our engine builds them, including those a performance view declares
    (KONTRA_INSPECT_UI_LIST): {name: (kind, values)}."""
    env = dict(os.environ, KONTRA_INSPECT_UI_LIST="1")
    subprocess.run(["nice", "-n", "19", binary, "ui", str(nki), "--json", str(out)], env=env,
                   capture_output=True)
    if not out.exists():
        return {}
    rows = json.loads(out.read_text())["result"]["controls"]
    return {r["name"].lstrip("$").lower(): (r["kind"], r["targets"]) for r in rows if r["slot"] == 0}


def controls(src, saved, view=None):
    """Rows: name, page, kind, values to visit, default, what the body writes. `view` adds the
    controls a performance view declares (their callbacks are in the script, their declarations
    are not)."""
    code = strip(src)
    consts = {m[0].lower(): int(m[1]) for m in re.findall(r"declare\s+(?:const\s+)?\$(\w+)\s*:=\s*(-?\d+)", code)}
    init = code[: code.lower().find("end on")]
    items, bodies = {}, {}
    for name, value in ITEM.findall(code):
        items.setdefault(name.lower(), []).append(int(value))
    for name, body in BODY.findall(code):
        bodies[name.lower()] = body
    functions = {n.lower(): b for n, b in FUNCTION.findall(code)}
    declared = DECLARE.findall(code)
    known = {n.lower() for _, n, _ in declared}
    for key, (kind, values) in (view or {}).items():
        name = next((n for n in re.findall(r"on\s+ui_control\s*\(\s*\$(\w+)", code, re.I)
                     if n.lower() == key), None)
        if name and key not in known and values:
            k = {"Knob": "knob", "Slider": "slider", "ValueEdit": "value_edit", "Switch": "switch",
                 "Button": "button", "Menu": "menu"}.get(kind, "slider")
            items[key] = [int(v) for v in values] if k == "menu" else items.get(key, [])
            declared.append((k, name, f"{int(min(values))},{int(max(values))}" if k not in ("menu",) else ""))
    rows = []
    for kind, name, args in declared:
        key, kind = name.lower(), kind.lower()
        parts = [number(a, consts) for a in args.split(",")] if args else []
        if kind in ("switch", "button"):
            lo, hi = 0, 1
        elif kind == "menu":
            vals = items.get(key, [])
            lo, hi = (min(vals), max(vals)) if vals else (None, None)
        else:
            lo, hi = (parts + [None, None])[:2]
        assigned = re.search(rf"\${name}\s*:=\s*(-?\d+)\s*$", init, re.I | re.M)
        default = saved.get(key, int(assigned[1]) if assigned else lo)
        body = bodies.get(key)
        reach = reached(body or "", functions)
        writes = sorted(set(re.findall(r"\$(ENGINE_PAR_\w+)", reach)))
        writes += sorted({"call " + c for c in re.findall(r"\bcall\s+(\w+)", body or "", re.I)})
        writes += [a for a in ACTIONS if re.search(rf"\b{a}\s*\(", reach, re.I)]
        note = ""
        if body is None:
            note = "no ui_control callback (read in other callbacks)"
        elif name.lower().startswith("dev_"):
            note = "developer control, not on the panel"
        elif re.search(r"(Prev|Next)$", name):
            note = "steps a menu (measured on the menu)"
        elif lo is None or hi is None:
            note = "range not static (menu filled at run time)"
        rows.append(dict(name=name, page=name.split("_")[0], kind=kind, min=lo, max=hi, default=default,
                         writes=writes or ["script state only"], note=note, body=body))
    return rows


def visits(row):
    """min, max, default; a switch visits the other value, then its default."""
    if row["kind"] in ("switch", "button"):
        return [1 - row["default"], row["default"]]
    return [row["min"], row["max"], row["default"]]


def plan(src, rows, context):
    """The transformed script 0, the step list and the scenario events."""
    steps = [(r, v, "context") for r in rows for n, v in context.items() if r["name"].lower() == n]
    steps.append((None, None, "baseline"))
    for r in rows:
        if r["measure"]:
            steps += [(r, v, tag) for v, tag in zip(visits(r), ("a", "b", "default"))]
    # A control's default is the value it holds when the first step arrives (its init may read
    # it from the engine): every measured control is captured then and restored to it.
    measured = [r for r in rows if r["measure"]]
    slot = {id(r): i for i, r in enumerate(measured)}
    capture = "".join(f"      %kontra_orig[{i}] := ${r['name']}\n" for i, r in enumerate(measured))
    cases = []
    for k, (r, v, tag) in enumerate(steps):
        if r is not None:
            value = f"%kontra_orig[{slot[id(r)]}]" if tag == "default" else v
            cases.append(f"    case {k}\n      ${r['name']} := {value}\n{r['body']}\n")
    table = ("  if ($CC_NUM = 102)\n    if ($kontra_taken = 0)\n" + capture
             + "      $kontra_taken := 1\n    end if\n    select (%CC[103] * 128 + %CC[102])\n"
             + "".join(cases) + "    end select\n    exit\n  end if\n")
    init = re.search(r"^on\s+init\s*$", src, re.I | re.M)
    decl = f"\n  declare %kontra_orig[{max(len(measured), 1)}]\n  declare $kontra_taken\n"
    src = src[: init.end()] + decl + src[init.end():]
    head = re.search(r"^on\s+controller\s*$", src, re.I | re.M)
    script = (src[: head.end()] + "\n" + table + src[head.end():] if head
              else src + "\non controller\n" + table + "end on\n")
    events, at = [], 1.0
    for k in range(len(steps)):
        f = lambda t: round((at + t) * R)  # noqa: E731
        events += [{"frame": f(0), "type": "cc", "cc": 103, "value": k >> 7},
                   {"frame": f(0.01), "type": "cc", "cc": 102, "value": k & 127}]
        for key, start, length in SINGLES:
            events += [{"frame": f(PHRASE_AT + start), "type": "note_on", "key": key, "velocity": VELOCITY},
                       {"frame": f(PHRASE_AT + start + length), "type": "note_off", "key": key}]
        keys, start, length = CHORD
        events += [{"frame": f(PHRASE_AT + start), "type": "note_on", "key": key, "velocity": VELOCITY}
                   for key in keys]
        events += [{"frame": f(PHRASE_AT + start + length), "type": "note_off", "key": key} for key in keys]
        at += PERIOD
    events.sort(key=lambda e: e["frame"])
    return script, steps, events


def features(x):
    """Level, octave bands, attack (first 50 ms over the next 400 ms), release tail, width, balance."""
    db = sb.db
    rms = lambda y: float(np.sqrt((y * y).mean())) if y.size else 0.0  # noqa: E731
    out = {"level": db(rms(x))}
    centres, bands = sb.thirds(x)
    for k in range(0, len(bands) - 2, 3):
        out[f"{centres[k + 1]:.0f} Hz"] = 10 * np.log10(sum(bands[k:k + 3]) + 1e-20)
    onsets = [round((PHRASE_AT + s) * R) for _, s, _ in SINGLES] + [round((PHRASE_AT + CHORD[1]) * R)]
    out["attack"] = float(np.mean([db(rms(x[:, o:o + 2400])) - db(rms(x[:, o + 4800:o + 24000])) for o in onsets]))
    end = round((PHRASE_AT + CHORD[1] + CHORD[2]) * R)
    out["tail"] = db(rms(x[:, end + 4800:end + 24000])) - db(rms(x[:, end - 24000:end]))
    mid, side = (x[0] + x[1]) / 2, (x[0] - x[1]) / 2
    out["width"] = db(rms(side)) - db(rms(mid))
    out["balance"] = db(rms(x[0])) - db(rms(x[1]))
    return out


def deltas(fa, fb, ref):
    """b − a per feature; bands under −60 dB of the loudest band (in `ref`) are left out."""
    top = max(v for k, v in ref.items() if k.endswith("Hz"))
    return {k: float(fb[k] - fa[k]) for k in fa if not k.endswith("Hz") or ref[k] > top - 60}


def classify(dk, do, silent):
    """same / different / dead (Kontakt changes, ours does not, or ours is silent)."""
    if silent:
        return "dead", 0.0, "our render is silent"
    ek, eo = max(map(abs, dk.values())), max(map(abs, do.values()))
    err = max(abs(do[k] - dk[k]) for k in dk if k in do)
    if ek < 1 and eo < 1:
        return "same", err, "no audible change in either"
    if eo < max(0.5, 0.2 * ek):
        return "dead", err, f"Kontakt {top(dk, 2)}; ours under {eo:.1f} dB"
    if err <= max(1.5, 0.25 * ek):
        return "same", err, f"both {top(dk, 2)}"
    return "different", err, f"Kontakt {top(dk, 2)}; ours {top(do, 2)}"


def analyse(native, ours, steps, rows):
    n = compare.load(native)[:, int(compare.PRE * R):].astype(np.float64)
    o = compare.load(ours).astype(np.float64)
    lag = sb.global_lag(n[:, : 20 * R], o[:, : 20 * R])  # frames Kontakt lags ours
    feats = []
    for k in range(len(steps)):
        a = round((1.0 + k * PERIOD) * R)
        span = slice(a, a + round(PERIOD * R))
        nk, ok = n[:, a + lag: a + lag + round(PERIOD * R)], o[:, span]
        feats.append((features(nk), features(ok), sb.db(np.abs(ok).max()) < -80))
    by = {}
    for k, (r, v, tag) in enumerate(steps):
        if r is not None and tag in ("a", "b", "default"):
            by.setdefault(r["name"], {})[tag] = k
    for r in rows:
        r.pop("body", None)
        idx = by.get(r["name"])
        if not r["measure"] or not idx:
            r["class"], r["reason"] = "not reachable", r["note"] or "not in this run (--only)"
            continue
        ka, kb = idx["a"], idx["b"]
        if r["kind"] in ("switch", "button") and visits(r)[0] == 1:  # always report 0 → 1
            ka, kb = kb, ka
        (na, oa, sa), (nb, ob, sbl) = feats[ka], feats[kb]
        dk, do = deltas(na, nb, na), deltas(oa, ob, na)
        r["class"], r["error_db"], r["reason"] = classify(dk, do, sa or sbl)
        r["kontakt_delta"], r["ours_delta"] = dk, do
        d = feats[idx["default"]] if "default" in idx else feats[kb]
        r["default_level_ours_minus_kontakt"] = d[1]["level"] - d[0]["level"]
    return lag


def top(d, n=3):
    return ", ".join(f"{k} {v:+.1f}" for k, v in sorted(d.items(), key=lambda kv: -abs(kv[1]))[:n])


def markdown(rows, title, context):
    counts = {}
    for r in rows:
        counts[r["class"]] = counts.get(r["class"], 0) + 1
    out = [f"# {title}", "", "Context: " + (", ".join(f"{k}={v}" for k, v in context.items()) or "saved state"),
           "", " · ".join(f"**{c}** {n}" for c, n in sorted(counts.items())), "",
           "| control | kind | min..max (default) | writes | what Kontakt does (max−min, dB) | what KONTRA does | "
           "verdict | reason |", "|---|---|---|---|---|---|---|---|"]
    for r in rows:
        kd, od = r.get("kontakt_delta"), r.get("ours_delta")
        w = [x.replace("ENGINE_PAR_", "") for x in r["writes"]]
        writes = "; ".join(w[:3]) + (f" (+{len(w) - 3})" if len(w) > 3 else "")
        out.append(f"| {r['name']} | {r['kind']} | {r['min']}..{r['max']} ({r['default']}) | "
                   f"{writes} | {top(kd) if kd else ''} | {top(od) if od else ''} | "
                   f"{r['class']} | {r.get('cause') or r['reason']} |")
    return "\n".join(out) + "\n"


def writes_per_step(report, steps):
    """Step k's engine writes from pass A's trace: set_engine_par as (name, value, group, slot,
    generic), purge_group as ("purge_group", group, mode). An IR file load is not replayed."""
    res = json.loads(report.read_text())["result"]
    names, base = res["script_symbols"][0], res["symbol_base"]
    cc102 = [e["callback_id"] for e in res["stage_events"] if e.get("type") == "callback"
             and e.get("callback_name") == "controller" and e.get("controller") == 102
             and e.get("script_slot") == 0]
    step = {cb: k for k, cb in enumerate(cc102)}
    out = [[] for _ in steps]
    for e in res["stage_events"]:
        if e.get("script_slot") != 0 or e.get("callback_id") not in step:
            continue
        if e.get("action") == "SetEnginePar":
            par, *rest = e["args"]
            name = names[par - base] if 0 <= par - base < len(names) else None
            if name:
                out[step[e["callback_id"]]].append((name, *rest))
        elif e.get("action") == "PurgeGroup":
            out[step[e["callback_id"]]].append(("purge_group", *e["args"][:2]))
    return out, len(cc102)


EFFECT_FAMILIES = {"COMP", "LIM", "SCOMP", "FCOMP", "SEQ", "SK", "SKDLX", "TP", "IRC", "RV", "RV2", "DL", "CH",
                   "FL", "PH", "RDL", "TDL", "PSYDL", "RAUM", "JMP", "EPP", "FREAK", "FLAIR", "PHASIS", "CHORAL",
                   "KOLOR", "DIRT", "BITE", "BASSPRO", "TR", "WT", "STEREO", "GN", "SHAPE", "LP", "HP", "BP"}
SLOT_GENERIC = {"EFFECT_BYPASS", "INSERT_EFFECT_OUTPUT_GAIN", "SEND_EFFECT_BYPASS", "SEND_EFFECT_OUTPUT_GAIN",
                "SEND_EFFECT_DRY_LEVEL", "EFFECT_TYPE", "EFFECT_SUBTYPE"}


def mechanism(writes):
    """The engine mechanism a row drives, from its steps' writes: the parameter family (an effect's
    own parameters as one, e.g. COMP_*) and where it acts (group, insert, send, main or bus chain),
    most written first; a slot's bypass or output level counts only when nothing else is written.
    A row with no engine writes drives script state; one that only loads groups, purge_group."""
    if not writes:
        return "script state"
    def where(w):
        if w[0] == "purge_group":
            return "purge_group"
        name = re.sub(r"_?\d+$", "", w[0].removeprefix("$ENGINE_PAR_"))
        head = name.split("_")[0]
        family = f"{head}_*" if head in EFFECT_FAMILIES and "_" in name else name
        chain = "send" if name.startswith("SEND_EFFECT") else \
            {0: "send", 1: "insert", -1: "insert", 2: "main"}.get(w[4], "bus")
        return f"{family} @ {'group' if w[2] >= 0 else chain}"
    counts = {}
    for w in writes:
        counts[where(w)] = counts.get(where(w), 0) + 1
    own = {k: n for k, n in counts.items() if k.split(" @ ")[0] not in SLOT_GENERIC} or counts
    return ", ".join(sorted(own, key=lambda k: (-own[k], k))[:2])


def row_writes(rows, steps, writes):
    """Each row's writes that its min and max steps set differently (all of them when none differ):
    the parameters the row really moves, by row name."""
    out = {}
    for r in rows:
        ks = {t: k for k, (sr, _, t) in enumerate(steps) if sr is r and t != "context"}
        out[r["name"]] = moved(writes, ks)
    return out


def moved(writes, ks):
    """The writes of steps `ks` ({tag: step}) whose value differs between the a and b steps."""
    every = [w for k in ks.values() for w in writes[k]]
    if "a" not in ks or "b" not in ks:
        return every
    a = {tuple(w[:1] + w[2:]): w[1] for w in writes[ks["a"]]}
    b = {tuple(w[:1] + w[2:]): w[1] for w in writes[ks["b"]]}
    diff = [w for w in writes[ks["b"]] if a.get(tuple(w[:1] + w[2:])) != w[1]]
    diff += [w for w in writes[ks["a"]] if tuple(w[:1] + w[2:]) not in b]
    return diff or every


def probe(writes, context=()):
    """A script that replays step k's engine writes on CC 103·128 + CC 102 = k. Kontakt runs
    purge_group only in UI and persistence callbacks, so the context steps' purges (`context`:
    their indices) run once in on persistence_changed and other purges are left out."""
    def line(w):
        return "      set_engine_par({}, {}, {}, {}, {})\n".format(*w)
    sets = [[x for x in w if x[0] != "purge_group"] for w in writes]
    cases = "".join(f"    case {k}\n" + "".join(map(line, w)) for k, w in enumerate(sets) if w)
    loads = "".join(f"  purge_group({x[1]}, {x[2]})\n" for k in context for x in writes[k] if x[0] == "purge_group")
    return ("on init\n  declare $step\nend on\n"
            + (f"on persistence_changed\n{loads}end on\n" if loads else "")
            + "on controller\n  if ($CC_NUM = 102)\n"
            "    $step := %CC[103] * 128 + %CC[102]\n    select ($step)\n" + cases
            + "    end select\n  end if\nend on\n")


def play(binary, nki, out, scenario, script_dir, report, oracle):
    env = dict(os.environ, KONTRA_SCRIPT_SOURCE=str(script_dir), KONTRA_ORACLE_DIR=str(HERE),
               KONTRA_INSPECT_ORACLE_DIR=str(out / "oracle"), KONTRA_INSPECT_WAV=str(out / f"{report}.wav"))
    cmd = ["nice", "-n", "19", binary, "play", str(nki), "--scenario", str(scenario),
           "--json", str(out / f"{report}.json")] + (["--oracle"] if oracle else [])
    run = subprocess.run(cmd, env=env, capture_output=True, text=True)
    (out / f"{report}.log").write_text(run.stdout + run.stderr)
    if run.returncode:
        sys.exit(f"render failed ({run.returncode}); see {out / f'{report}.log'}")


def saved_values(path):
    vals = {}
    if path.exists():
        for line in path.read_text().splitlines():
            parts = line.split()
            if len(parts) == 2 and parts[0].startswith("$") and re.fullmatch(r"-?\d+", parts[1]):
                vals[parts[0][1:].lower()] = int(parts[1])
    return vals


def dump(nki, out):
    """Script sources via the render example (KONTRA_SCRIPT_DUMP), built next to kontra-inspect."""
    render = Path(sb.inspect_binary(None)).parent / "examples" / "render"
    if not render.exists():
        sys.exit(f"no {render}: build-slot cargo build --release -p kontra-host --example render, or pass --src")
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, KONTRA_SCRIPT_DUMP=str(out), KONTRA_INIT_ONLY="1")
    subprocess.run(["nice", "-n", "19", str(render), str(nki), "60", str(out / "init.wav")], env=env,
                   capture_output=True, check=True)
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("nki")
    ap.add_argument("out")
    ap.add_argument("--only", default="", help="regex on control names to measure (all listed anyway)")
    ap.add_argument("--with", dest="context", default="", help="NAME=V,... set before any row")
    ap.add_argument("--src", help="KONTRA_SCRIPT_DUMP directory of this instrument")
    ap.add_argument("--inspect", help="kontra-inspect binary")
    ap.add_argument("--plan", action="store_true", help="write the script and scenario, render nothing")
    ap.add_argument("--view", action="store_true", help="also take controls from our engine's UI model")
    ap.add_argument("--rescore", action="store_true", help="score OUTDIR's existing renders again")
    ap.add_argument("--ours-only", action="store_true",
                    help="no Kontakt (no licence in the oracle): judge each row on our pass A alone")
    args = ap.parse_args()
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    src_dir = Path(args.src) if args.src else dump(Path(args.nki), out / "src")
    src = (src_dir / "0.ksp").read_text(errors="replace")
    binary, nki = sb.inspect_binary(args.inspect), Path(args.nki).resolve()
    view = listed(binary, nki, out / "ui.json") if not DECLARE.search(src) or args.view else None
    rows = controls(src, saved_values(src_dir / "0-saved.txt"), view)
    context = {k.lower(): int(v) for k, v in (p.split("=") for p in args.context.split(",") if p)}
    for r in rows:
        r["measure"] = bool(re.search(args.only, r["name"], re.I)) and not r["note"]
        if r["name"].lower() in context:
            r["default"] = context[r["name"].lower()]
    script, steps, events = plan(src, rows, context)
    (out / "script").mkdir(exist_ok=True)
    (out / "script" / "0.ksp").write_text(script)
    (out / "scenario.json").write_text(json.dumps({"events": events}))
    (out / "steps.json").write_text(json.dumps([[r and r["name"], v, t] for r, v, t in steps]))
    print(f"{sum(r['measure'] for r in rows)} rows to measure, {len(steps)} steps, "
          f"{len(steps) * PERIOD / 60:.1f} min per side", flush=True)
    if args.plan:
        return
    (out / "oracle").mkdir(exist_ok=True)
    if not args.rescore:
        play(binary, nki, out, out / "scenario.json", out / "script", "a", oracle=False)
    writes, seen = writes_per_step(out / "a.json", steps)
    if seen != len(steps):
        sys.exit(f"pass A ran {seen} of {len(steps)} steps; see {out / 'a.log'}")
    slot = len(list(src_dir.glob("[0-9].ksp")))
    (out / "probe").mkdir(exist_ok=True)
    (out / "probe" / f"{slot}.ksp").write_text(probe(writes, [k for k, s in enumerate(steps) if s[2] == "context"]))
    (out / "writes.json").write_text(json.dumps(writes))
    if args.ours_only:
        return ours_only(args, out, rows, steps, writes, context)
    if not args.rescore:
        play(binary, nki, out, out / "scenario.json", out / "probe", "b", oracle=True)
    native = sorted((out / "oracle").glob("inspect-*/native.wav"), key=lambda p: p.stat().st_mtime)
    if not native:
        sys.exit(f"no Kontakt capture; see {out / 'b.log'}")
    for r in rows:  # a row whose steps write no engine parameter cannot be replayed in Kontakt
        ks = [k for k, (sr, _, t) in enumerate(steps) if sr is r and t != "context"]
        if r["measure"] and any(x[0] == "purge_group" for k in ks for x in writes[k]):
            r["measure"], r["note"] = False, "loads or unloads groups (purge_group), which Kontakt runs only from its panel"
        elif r["measure"] and ks and not any(writes[k] for k in ks):
            r["measure"], r["note"] = False, "sets script state only (read by other callbacks); not replayable"
        elif r["measure"] and len(ks) >= 2 and writes[ks[0]] == writes[ks[1]]:
            r["measure"], r["note"] = False, "min and max write the same engine values; its effect is script state"
    lag = analyse(native[-1], out / "b.wav", steps, rows)
    title = f"{Path(args.nki).stem}: panel controls, ours vs Kontakt"
    (out / "controls.json").write_text(json.dumps({"nki": args.nki, "context": context, "lag_frames": lag,
                                                   "rows": rows}, indent=1))
    (out / "CONTROLS.md").write_text(markdown(rows, title, context))
    verdicts(args, out, rows, row_writes(rows, steps, writes))
    print(out / "CONTROLS.md")


def verdicts(args, out, rows, by_row):
    """verdicts.json: small and stable, one entry per control (library, patch, control, verdict,
    reason, mechanism); controls_rank.py merges sessions into the ranking by mechanism."""
    nki = Path(args.nki)
    (out / "verdicts.json").write_text(json.dumps([
        {"library": library(nki), "patch": nki.stem, "control": r["name"], "verdict": r["class"],
         "reason": r.get("cause") or r["reason"], "mechanism": mechanism(by_row[r["name"]])}
        for r in rows], indent=1))


def ours_only(args, out, rows, steps, writes, context):
    """Verdicts from our pass A alone: 'ours changes' when any feature moves 1 dB or more between the
    row's min and max steps, else 'ours no change'. Kontakt is not compared."""
    o = compare.load(out / "a.wav").astype(np.float64)
    feats = [features(o[:, round((1.0 + k * PERIOD) * R):round((1.0 + (k + 1) * PERIOD) * R)])
             for k in range(len(steps))]
    by = {}
    for k, (r, _, tag) in enumerate(steps):
        if r is not None and tag in ("a", "b"):
            by.setdefault(r["name"], {})[tag] = k
    for r in rows:
        r.pop("body", None)
        idx = by.get(r["name"], {})
        if not r["measure"] or "a" not in idx or "b" not in idx:
            r["class"], r["reason"] = "not reachable", r["note"] or "not in this run (--only)"
            continue
        d = deltas(feats[idx["a"]], feats[idx["b"]], feats[idx["a"]])
        moved = max(map(abs, d.values()))
        r["class"] = "ours changes" if moved >= 1.0 else "ours no change"
        r["reason"] = f"ours {top(d, 2)}; Kontakt not compared" if moved >= 1.0 else "ours under 1 dB; Kontakt not compared"
        r["ours_delta"] = d
    (out / "controls.json").write_text(json.dumps({"nki": args.nki, "context": context, "rows": rows}, indent=1))
    verdicts(args, out, rows, row_writes(rows, steps, writes))
    print(out / "verdicts.json")


if __name__ == "__main__":
    main()
