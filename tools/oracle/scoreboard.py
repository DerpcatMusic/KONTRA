#!/usr/bin/env python3
"""DSP parity scoreboard: the user libraries in native Kontakt against v3, case by case.

    tools/oracle/scoreboard.py capture ROOT [--only NAME ...]     native + v3 captures (Kontakt, slow)
    tools/oracle/scoreboard.py score ROOT [--inspect BIN] [--tag T] [--only NAME ...]   v3 only

`capture` plays one scenario per library through `kontra-inspect play --oracle` (one oracle-slot
Kontakt session each, one at a time, nice 19) and keeps ROOT/<name>.capture.json: the artifact
directory with native.wav and the v3 render of the same events. Recordings are commercial
libraries' output: keep ROOT outside the repo (default PluginArtifacts/KONTRA/parity-refs).

`score` re-renders v3 with BIN (default: the newest release kontra-inspect) at Kontakt's measured
note positions and scores every case; it never starts Kontakt.
Scores are cached per library in ROOT/score-cache.json, keyed by the binary's content hash, --spread,
the key-up fit, ROOT and this script's hash: a main-side board is rendered once and reused. Kontakt's MIDI reaches it through
Wine 0-3 ms off the grid (and a capture can step once in time), so identical overlapping samples
(repeats, chords) sum differently from a render on the grid: each group of simultaneous note-ons is
moved to the lag (+-40 ms) that best correlates 0.5 s of native audio with v3's, and v3 plays again.
Note-offs jitter too (up to 15 ms): each cluster of them is moved within +-15 ms to where v3's
release best matches Kontakt's (one render per 2.5 ms step; the fitted shifts print per library;
a best shift at the bound counts as unfitted and prints as unfit(..); --no-keyup turns it off).
Fast repeats are scored statistically against v3's own spread under per-note key-up and note-on
jitter (JITTER_CASES).

The scenario starts after LEAD seconds of silence: Kontakt keeps loading samples after it reports
the instrument loaded, and a note that arrives first plays late or not at all.

Cases (13 per library): keys 48/60/72 x velocities 40/90/127 held 1.5 s, a 5-note chord, 12 repeats
at 1/12 s, a held note under a CC1 sweep, a pedalled phrase. Tolerance: RMS level 1 dB; each
third-octave band (25 Hz-16 kHz, within 30 dB of the loudest) 1 dB after the level offset; onset 5 ms;
first 60 ms of envelope 1.5 dB; release tail (+100..+700 ms after key-up over the 300 ms before)
1.5 dB; L/R balance 1 dB. Both engines silent (key out of range) counts as within.
"""
import argparse
import hashlib
import json
import math
import os
import random
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import compare  # noqa: E402  (RATE, PRE, load)

R = compare.RATE
LEAD = 30.0
KONTAKT = Path(os.environ.get("KONTRA_KONTAKT_LIBRARIES", "/data/main/Libraries/Kontakt"))
LIBRARIES = {
    "analog": "ANALOG STRINGS/Instruments/ANALOG STRINGS.nki",
    "trumpets": "Afflatus Chapter II Brass/Instruments/1. Ensembles/Single Instruments/3 Trumpets/3 Trumpets Legato.nki",
    "horn": "Afflatus Chapter II Brass/Instruments/2. Solos/Single Instruments/Solo French Horn/Solo French Horn Legato.nki",
    "pacific": "Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Legato Sustains.nki",
    "chorus": "Audio Imperia CHORUS/Instruments/01 Multi Patches/01 Chorus - Women - Traditional Articulations.nki",
    "areia": "Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/01 Areia - 16 Violins - Core Techniques.nki",
    "dolce": "Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 01 7 1st Violins - Legato.nki",
    "unacorda": "Una Corda Library/Instruments/Una Corda Felt.nki",
}
TOL = dict(level=1.0, band=1.0, onset_ms=5.0, attack=1.5, release=1.5, balance=1.0)
WIN, SEARCH, NEAR = 24000, 40, 5  # replay: window (frames), lag search and near search (ms)
# Key-up fit: Kontakt's note-off reaches it through MIDI and its block with up to 15 ms of jitter
# (two captures of one note agree at onset, r 0.999, and differ by 0-15 ms after key-up), and a
# release that sums a tail with a release sample changes by dB with a 5 ms shift.
KEYUP_MS, KEYUP_STEP, KEYUP_TAIL = 15.0, 2.5, 0.7
# Fast repeats are scored statistically: twelve 42 ms notes overlap, and the per-note key-up jitter
# alone swings each slot by +-3-5 dB with no consistent sign (v3 against itself with note-offs moved
# 0-15 ms: Dolce slot sd 2.3-2.4 dB, native 2.7; Chorus 0.8-1.2, native 0.9; slots 3+ r 0.4-0.8).
# Note-ons too: Wine's 0-3 ms spread is a full period of a 300 Hz fundamental, so twelve copies
# of one sample on the exact 1/12 s grid cancel or add by their fixed phase (Pacific key 62: 24.5
# periods apart, -60 dB flat), where Kontakt's add in random phase and build up (-53..-56 dB from
# the 6th; v3 with note-ons moved +-1.5 ms: -53..-57; Una Corda's +-2 dB slot swing likewise).
# One cluster shift cannot follow that, so these cases also allow v3's own spread over
# JITTER_RENDERS renders with each note-off moved by its own random +-KEYUP_MS/2 and each note-on
# by its own +-NOTE_ON_MS/2.
JITTER_CASES, JITTER_RENDERS, NOTE_ON_MS = ("repeats",), 4, 3.0


def scenario():
    """(events, cases): the input and the window each case is scored over."""
    ev, cases = [], []
    t = LEAD

    def note(at, key, vel, length):
        ev.append({"frame": round(at * R), "type": "note_on", "key": key, "velocity": vel})
        ev.append({"frame": round((at + length) * R), "type": "note_off", "key": key})

    for key in (48, 60, 72):
        for vel in (40, 90, 127):
            note(t, key, vel, 1.5)
            cases.append({"name": f"k{key}v{vel}", "start": t, "len": 4.0, "hold": 1.5})
            t += 4.0
    for key in (48, 52, 55, 60, 64):
        note(t, key, 100, 1.5)
    cases.append({"name": "chord", "start": t, "len": 4.0, "hold": 1.5})
    t += 4.0
    for i in range(12):
        note(t + i / 12, 62, 100, 1 / 24)
    cases.append({"name": "repeats", "start": t, "len": 4.0, "hold": 1.0})
    t += 4.0
    note(t, 60, 100, 3.0)
    for i in range(33):
        v = i * 127 // 16 if i <= 16 else (32 - i) * 127 // 16
        ev.append({"frame": round((t + 0.1 + 2.8 * i / 32) * R), "type": "cc", "cc": 1, "value": min(127, v)})
    cases.append({"name": "modsweep", "start": t, "len": 5.0, "hold": 3.0})
    t += 5.0
    ev.append({"frame": round(t * R), "type": "cc", "cc": 64, "value": 127})
    for i, key in enumerate((60, 64, 67, 72)):
        note(t + 0.3 * i, key, 90, 0.25)
    ev.append({"frame": round((t + 2.0) * R), "type": "cc", "cc": 64, "value": 0})
    cases.append({"name": "pedal", "start": t, "len": 5.0, "hold": 2.0})
    ev.sort(key=lambda e: e["frame"])
    return ev, cases


def inspect_binary(given):
    """`--inspect`, else this worktree's own release build where build-slot puts it (never the newest
    build of some other worktree: a total from another lane's branch is worse than none)."""
    if given:
        return given
    tree = subprocess.run(["git", "rev-parse", "--show-toplevel"], cwd=HERE, capture_output=True,
                          text=True).stdout.strip()
    common = subprocess.run(["git", "rev-parse", "--path-format=absolute", "--git-common-dir"], cwd=HERE,
                            capture_output=True, text=True).stdout.strip()
    target = (Path("/data/dev/workspace/Toolchains/User/cargo-target") / Path(common).parent.name
              / f"{Path(tree).name}-{hashlib.sha1(tree.encode()).hexdigest()[:8]}")
    found = target / "release" / "kontra-inspect"
    if not tree or not found.exists():
        sys.exit(f"no kontra-inspect for this worktree at {found}: "
                 "build-slot cargo build --release -p kontra-inspect, or pass --inspect BIN")
    return str(found)


def play(binary, nki, events, wav=None, oracle=False, report=None):
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "scenario.json"
        path.write_text(json.dumps({"events": events}))
        cmd = ["nice", "-n", "19", binary, "play", nki, "--scenario", str(path)]
        cmd += ["--oracle"] if oracle else []
        cmd += ["--json", str(report)] if report else []
        env = dict(os.environ, **({"KONTRA_INSPECT_WAV": str(wav)} if wav else {}))
        env.setdefault("KONTRA_ORACLE_DIR", str(HERE))
        return subprocess.run(cmd, env=env, capture_output=True, text=True)


# --- metrics -------------------------------------------------------------------------------

def db(x):
    return 20 * math.log10(max(float(x), 1e-12))


def env_ms(x):
    """1 ms RMS envelope of a (2, n) block."""
    m = (x * x).sum(axis=0)
    n = len(m) // 48
    return np.sqrt(m[: n * 48].reshape(n, 48).mean(axis=1) + 1e-24)


def smooth(e, w=10):
    return np.convolve(e, np.ones(w) / w, mode="same")


def global_lag(nat, ours):
    """Frames native lags v3, from 1 ms log envelopes (+-100 ms)."""
    a, b = np.log(env_ms(nat) + 1e-6), np.log(env_ms(ours) + 1e-6)
    n = min(len(a), len(b))
    a, b = a[:n] - a[:n].mean(), b[:n] - b[:n].mean()
    best = max(range(-100, 101), key=lambda L: float((a[max(0, L):n + min(0, L)] * b[max(0, -L):n - max(0, L)]).sum()))
    return best * 48


def thirds(x):
    spec = (np.abs(np.fft.rfft(x * np.hanning(x.shape[1]), axis=1)) ** 2).sum(axis=0)
    f = np.fft.rfftfreq(x.shape[1], 1 / R)
    centres = [1000 * 2 ** (k / 3) for k in range(-16, 13)]
    return centres, [float(spec[(f >= c * 2 ** (-1 / 6)) & (f < c * 2 ** (1 / 6))].sum()) for c in centres]


def case_metrics(x, hold, first=None):
    """`first`: ms from the case start to its second note-on. The onset is measured against the first
    note's own peak (5 % of the loudest point before the second note), so a build-up later in the
    case does not move it."""
    e = env_ms(x)
    s = smooth(e)
    own = s[:first] if first else s
    onset = int(np.argmax(own > own.max() * 0.05)) if own.max() > 1e-5 else None
    off = int(hold * 1000)
    before = float(np.mean(e[max(0, off - 300):off] ** 2))
    tail = (10 * math.log10(float(np.mean(e[off + 100:off + 700] ** 2)) / before + 1e-20)
            if off + 700 < len(e) and before > 1e-10 else None)
    centres, bands = thirds(x)
    return dict(rms=db(np.sqrt((x * x).mean())), env=s, onset=onset, tail=tail, centres=centres, bands=bands,
                balance=db(np.sqrt((x[0] ** 2).mean())) - db(np.sqrt((x[1] ** 2).mean())))


def score_case(n, o, hold, spread=None, first=None):
    """`o` against native `n`. With `spread` (this function's result for a second native take),
    each metric's tolerance is the larger of TOL and Kontakt's own run-to-run difference, band by band."""
    a, b = case_metrics(n, hold, first), case_metrics(o, hold, first)
    if a["rms"] < -80 and b["rms"] < -80:
        return dict(ok=True, silent="both")
    if a["rms"] < -80 or b["rms"] < -80:
        return dict(ok=False, silent="native" if a["rms"] < -80 else "v3")
    level = a["rms"] - b["rms"]
    top = max(max(a["bands"]), max(b["bands"]))
    bands = [(c, 10 * math.log10(x / y) - level) for c, x, y in zip(a["centres"], a["bands"], b["bands"])
             if max(x, y) > top * 1e-3 and x > 0 and y > 0]
    worst = max(bands, key=lambda t: abs(t[1])) if bands else (0.0, 0.0)
    onset = attack = None
    if a["onset"] is not None and b["onset"] is not None:
        onset = a["onset"] - b["onset"]
        o0 = a["onset"]
        attack = float(np.mean(20 * np.log10(a["env"][o0:o0 + 60] + 1e-9) - 20 * np.log10(b["env"][o0:o0 + 60] + 1e-9))) - level
    tail = None if a["tail"] is None or b["tail"] is None else a["tail"] - b["tail"]
    balance = a["balance"] - b["balance"]
    sp = spread if spread and "level" in spread else {}

    def tol(key, x):
        own = sp.get(key)
        return abs(x) > max(TOL[key if key != "onset" else "onset_ms"], abs(own) if own is not None else 0.0)

    own_band = dict(sp.get("bands", []))
    loud = [(c, d) for c, d in bands if abs(d) > max(TOL["band"], abs(own_band.get(c, 0.0)))]
    worst_out = max(loud, key=lambda t: abs(t[1])) if loud else None
    fails = [f for f, bad in (
        (f"level {level:+.1f}", tol("level", level)),
        (f"band {worst_out[0] if worst_out else 0:.0f}Hz {worst_out[1] if worst_out else 0:+.1f}", worst_out is not None),
        (f"onset {onset}ms", onset is not None and tol("onset", onset)),
        (f"attack {attack or 0:+.1f}", attack is not None and tol("attack", attack)),
        (f"release {tail or 0:+.1f}", tail is not None and tol("release", tail)),
        (f"balance {balance:+.1f}", tol("balance", balance))) if bad]
    return dict(ok=not fails, fails=fails, level=level, band=worst, bands=bands, onset=onset, attack=attack,
                release=tail, balance=balance)


# --- commands ------------------------------------------------------------------------------

def capture(args):
    root = Path(args.root)
    root.mkdir(parents=True, exist_ok=True)
    events, cases = scenario()
    os.environ.setdefault("KONTRA_INSPECT_ORACLE_DIR", str(root))
    for name in args.only or LIBRARIES:
        nki = str(KONTAKT / LIBRARIES[name])
        report = root / f"{name}.report.json"
        p = play(inspect_binary(args.inspect), nki, events, oracle=True, report=report)
        try:
            artifacts = json.loads(report.read_text())["result"]["oracle"]["artifacts"]
        except (OSError, KeyError, TypeError, ValueError):
            print(f"{name}: capture failed: {p.stderr.strip().splitlines()[-1:]}")
            continue
        finally:
            report.unlink(missing_ok=True)  # the full trace is hundreds of MB; the WAVs are what count
        (root / f"{name}.capture.json").write_text(json.dumps(
            {"nki": nki, "artifacts": artifacts, "events": events, "cases": cases}, indent=1))
        print(f"{name}: captured -> {artifacts}")


def replayed(nat, ours, events, base):
    """`events` moved, per group of simultaneous note-ons, to where Kontakt played them.

    The lag comes from the 1 ms log envelopes (+-40 ms, or +-5 ms around the previous group's
    shift when that group is under a second back), then the waveform refines it within +-1 ms
    so overlapping copies sum as Kontakt's did. Waveform correlation alone locks 1/12 s repeats
    onto the neighbouring note (Horn: +39 ms). A group with no clear match keeps the shift."""
    ne, oe = (np.log(env_ms(x) + 1e-6) for x in (nat, ours))
    nat, ours = nat.sum(0), ours.sum(0)
    shift, out, last_on = 0, [], None

    def pearson(a, b):
        if len(a) != len(b) or len(a) < 2:
            return -1.0
        a, b = a - a.mean(), b - b.mean()
        return float(np.dot(a, b)) / (math.sqrt(float(np.dot(a, a)) * float(np.dot(b, b))) + 1e-20)

    for e in events:
        f = e["frame"]
        if e["type"] == "note_on" and f != last_on:
            near = last_on is not None and f - last_on < R
            last_on = f
            at, w = f // 48, WIN // 48
            ref = oe[at:at + w]
            ms = shift // 48
            span = range(ms - NEAR, ms + NEAR + 1) if near else range(-SEARCH, SEARCH + 1)
            score = {L: pearson(ne[max(0, at + (base // 48) + L):at + (base // 48) + L + w], ref) for L in span}
            lag = max(score, key=score.get)
            if score[lag] > 0.5:
                wave = ours[f:f + WIN]
                def corr(x):
                    a = f + base + x
                    return pearson(nat[a:a + WIN], wave) if a >= 0 else -1.0
                shift = max(range(lag * 48 - 48, lag * 48 + 49, 2), key=corr)
        out.append(dict(e, frame=max(0, f + shift)))
    return sorted(out, key=lambda e: e["frame"])


def keyups(binary, nki, events, nat, base):
    """`events` with each cluster of note-offs (less than a second apart) moved by the shift in
    +-KEYUP_MS that best matches Kontakt's 10 ms envelope over the following KEYUP_TAIL seconds.
    One v3 render per candidate shift; returns (events, shifts in ms)."""
    offs = sorted({e["frame"] for e in events if e["type"] == "note_off"})
    clusters = []
    for f in offs:
        if clusters and f - clusters[-1][-1] < R:
            clusters[-1].append(f)
        else:
            clusters.append([f])
    steps = np.arange(-KEYUP_MS, KEYUP_MS + 1e-9, KEYUP_STEP)
    err = np.full((len(clusters), len(steps)), np.inf)
    n = R // 100
    for j, d in enumerate(steps):
        moved = [dict(e, frame=e["frame"] + round(d * R / 1000)) if e["type"] == "note_off" else e for e in events]
        with tempfile.TemporaryDirectory() as tmp:
            wav = Path(tmp) / "keyup.wav"
            play(binary, nki, moved, wav=wav)
            if not wav.exists():
                continue
            ours = compare.load(wav)
        for i, cl in enumerate(clusters):
            a, z = cl[0], cl[-1] + round(KEYUP_TAIL * R)
            if a + base < 0 or z + base > nat.shape[1] or z > ours.shape[1]:
                continue
            x, y = ((v[:, a + o:z + o] ** 2).sum(0) for v, o in ((nat, base), (ours, 0)))
            k = len(x) // n
            x, y = (10 * np.log10(v[: k * n].reshape(k, n).mean(1) + 1e-12) for v in (x, y))
            live = (x > -80) | (y > -80)
            if live.any():
                err[i, j] = float(np.mean((x - y)[live] ** 2))
    shift, shown = {}, []
    for i, cl in enumerate(clusters):
        d = float(steps[int(np.argmin(err[i]))]) if np.isfinite(err[i]).any() else 0.0
        # A best shift at the bound is a miss (the true one may lie beyond it): leave the cluster unfitted.
        shown.append(f"{d:+g}" if abs(d) < KEYUP_MS else f"unfit({d:+g})")
        d = d if abs(d) < KEYUP_MS else 0.0
        shift.update({f: d for f in cl})
    out = [dict(e, frame=e["frame"] + round(shift[e["frame"]] * R / 1000)) if e["type"] == "note_off" else e
           for e in events]
    return sorted(out, key=lambda e: e["frame"]), shown


def jittered(events, case, seed):
    """`events` with each note-off inside `case` moved by its own random +-KEYUP_MS/2 and each
    note-on by its own +-NOTE_ON_MS/2."""
    rnd, half = random.Random(seed), {"note_off": KEYUP_MS / 2, "note_on": NOTE_ON_MS / 2}
    a, z = case["start"] * R, (case["start"] + case["len"]) * R
    out = [dict(e, frame=e["frame"] + round(rnd.uniform(-half[e["type"]], half[e["type"]]) * R / 1000))
           if e["type"] in half and a <= e["frame"] < z else e for e in events]
    return sorted(out, key=lambda e: e["frame"])


def widest(*spreads):
    """Per metric (and band), the largest |difference| among score_case results: the tolerance they allow."""
    out, bands = {}, {}
    for s in spreads:
        if not s or "level" not in s:
            continue
        for k in ("level", "onset", "attack", "release", "balance"):
            if s.get(k) is not None:
                out[k] = max(abs(out.get(k, 0.0)), abs(s[k]))
        for c, d in s["bands"]:
            bands[c] = max(bands.get(c, 0.0), abs(d))
    return dict(out, bands=list(bands.items())) if out else None


def score(args):
    root = Path(args.root)
    binary = inspect_binary(args.inspect)
    table = {}
    # Per-library scores cached beside the refs, keyed by what decides them: the binary (content
    # hash), the spread and reference sets, and this script. A main-side board renders once.
    cache_file = root / "score-cache.json"
    cache = json.loads(cache_file.read_text()) if cache_file.exists() else {}
    digest = lambda path: hashlib.sha256(Path(path).read_bytes()).hexdigest()[:16]
    key = f"{digest(binary)}|spread={args.spread or ''}|keyup={not args.no_keyup}|refs={root}|script={digest(__file__)}"
    if key in cache:
        print(f"cache {cache_file}: {sorted(cache[key])} reused for this key")

    def library(name, cap):
        """Renders and scores one library: (rows, fit text), or None when v3 fails to render."""
        c = json.loads(cap.read_text())
        art = Path(c["artifacts"])
        nat = compare.load(art / "native.wav")[:, int(compare.PRE * R):]
        grid = compare.load(art / "v3.wav")
        base = global_lag(nat, grid)
        wav = root / f"{name}.{args.tag}.wav"
        events = replayed(nat, grid, c["events"], base)
        fit = ""
        if not args.no_keyup:
            events, shifts = keyups(binary, c["nki"], events, nat, base)
            fit = f" (key-up fit, bound +-{KEYUP_MS:g} ms, a fit at the bound is unfitted: " + " ".join(shifts) + ")"
        p = play(binary, c["nki"], events, wav=wav)
        if not wav.exists():
            print(f"{name}: v3 render failed: {p.stderr.strip().splitlines()[-1:]}")
            return None
        ours = compare.load(wav)
        nat = nat[:, base:] if base > 0 else nat
        ours = ours if base > 0 else ours[:, -base:]
        jits = {}
        for case in c["cases"]:
            if case["name"] not in JITTER_CASES:
                continue
            for k in range(JITTER_RENDERS):
                jw = root / f"{name}.{args.tag}.j{k}.wav"
                play(binary, c["nki"], jittered(events, case, k), wav=jw)
                if jw.exists():
                    j = compare.load(jw)
                    jw.unlink()
                    jits.setdefault(case["name"], []).append(j if base > 0 else j[:, -base:])
        nat2 = None
        if args.spread and (Path(args.spread) / f"{name}.capture.json").exists():
            c2 = json.loads((Path(args.spread) / f"{name}.capture.json").read_text())
            nat2 = compare.load(Path(c2["artifacts"]) / "native.wav")[:, int(compare.PRE * R):]
            lag2 = global_lag(nat2, nat)
            nat2 = nat2[:, lag2:] if lag2 >= 0 else np.pad(nat2, ((0, 0), (-lag2, 0)))
        rows = []
        for case in c["cases"]:
            a, z = round(case["start"] * R), round((case["start"] + case["len"]) * R)
            ons = sorted({e["frame"] for e in c["events"] if e["type"] == "note_on" and a <= e["frame"] < z})
            first = (ons[1] - a) // 48 if len(ons) > 1 else None
            if z > min(nat.shape[1], ours.shape[1]):
                rows.append(dict(case=case["name"], ok=False, silent="short"))
                continue
            spread = None
            if nat2 is not None and z <= nat2.shape[1]:
                spread = score_case(nat[:, a:z], nat2[:, a:z], case["hold"], first=first)
            own = [score_case(ours[:, a:z], j[:, a:z], case["hold"], first=first) for j in jits.get(case["name"], [])
                   if z <= j.shape[1]]
            if own:
                spread = widest(spread, *own)
            r = score_case(nat[:, a:z], ours[:, a:z], case["hold"], spread, first=first)
            r["jitter"] = len(own)
            plain = score_case(nat[:, a:z], ours[:, a:z], case["hold"], first=first)
            r["plain"], r["grid"] = plain["ok"], plain.get("fails", [])
            r["spread"] = spread and {k: v for k, v in spread.items() if k in ("level", "onset", "attack", "release", "balance", "band")}
            rows.append(dict(case=case["name"], **r))
        wav.unlink()
        return rows, fit

    for name in args.only or LIBRARIES:
        cap = root / f"{name}.capture.json"
        if not cap.exists():
            print(f"{name}: no capture (run `capture` first)")
            continue
        hit = cache.get(key, {}).get(name)
        got = (hit["rows"], hit["fit"]) if hit else library(name, cap)
        if got is None:
            continue
        rows, fit = got
        if not hit:
            cache.setdefault(key, {})[name] = {"rows": rows, "fit": fit}
            cache_file.write_text(json.dumps(cache, default=float))
        table[name] = rows
        ok = sum(r["ok"] for r in rows)
        print(f"{name}: {ok}/{len(rows)}{fit}")
        for r in rows:
            # A row scored against v3's own jitter spread also shows its score on the exact grid,
            # so a regression in repeat handling cannot hide inside the spread.
            grid = f" | exact grid: {', '.join(r['grid']) or 'ok'}" if r.get("jitter") else ""
            if not r["ok"] or grid:
                print(f"  {r['case']:9} " + (f"silent {r['silent']}" if "silent" in r else ", ".join(r["fails"]) or "ok") + grid)
    total = sum(r["ok"] for rows in table.values() for r in rows)
    if args.spread:
        inside = sum(r["ok"] and not r.get("plain", True) and not r.get("jitter") for rows in table.values() for r in rows)
        print(f"inside Kontakt's own run-to-run spread (fail the fixed tolerance only): {inside}")
    stat = [r for rows in table.values() for r in rows if r.get("jitter")]
    if stat:
        print(f"scored statistically ({', '.join(JITTER_CASES)}: tolerance includes v3's own spread over "
              f"{JITTER_RENDERS} renders with per-note key-up jitter +-{KEYUP_MS / 2:g} ms, note-on +-{NOTE_ON_MS / 2:g} ms): "
              f"{sum(r['ok'] for r in stat)}/{len(stat)}, of which inside only that spread "
              f"{sum(r['ok'] and not r.get('plain', True) for r in stat)}")
    cases = sum(len(rows) for rows in table.values())
    # The default binary is the newest build in ANY worktree's target dir: name it, so two totals
    # are only compared when binary, build time and --spread match.
    built = time.strftime("%Y-%m-%d %H:%M", time.localtime(Path(binary).stat().st_mtime))
    print(f"total: {total}/{cases} within tolerance (binary {binary}, built {built}; "
          f"spread {args.spread or 'off'})")
    (root / f"score-{args.tag}.json").write_text(json.dumps(table, indent=1, default=float))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    for cmd in ("capture", "score"):
        p = sub.add_parser(cmd)
        p.add_argument("root", nargs="?", default="/data/dev/workspace/PluginArtifacts/KONTRA/parity-refs")
        p.add_argument("--only", nargs="*", choices=sorted(LIBRARIES))
        p.add_argument("--inspect")
        p.add_argument("--tag", default="v3")
        p.add_argument("--no-keyup", action="store_true", help="score at the replayed note-offs, no key-up fit")
        p.add_argument("--spread", help="a second capture ROOT: tolerances widen to native-vs-native per case")
    args = ap.parse_args()
    capture(args) if args.cmd == "capture" else score(args)


if __name__ == "__main__":
    main()
