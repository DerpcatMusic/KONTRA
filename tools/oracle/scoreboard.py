#!/usr/bin/env python3
"""DSP parity scoreboard: the user libraries in native Kontakt against v3, case by case.

    tools/oracle/scoreboard.py capture ROOT [--only NAME ...]     native + v3 captures (Kontakt, slow)
    tools/oracle/scoreboard.py score ROOT [--inspect BIN] [--tag T] [--only NAME ...]   v3 only

`capture` plays one scenario per library through `kontra-inspect play --oracle` (one oracle-slot
Kontakt session each, one at a time, nice 19) and keeps ROOT/<name>.capture.json: the artifact
directory with native.wav and the v3 render of the same events. Recordings are commercial
libraries' output: keep ROOT outside the repo (default PluginArtifacts/KONTRA/parity-refs).

`score` re-renders v3 with BIN (default: the newest release kontra-inspect) at Kontakt's measured
note positions and scores every case; it never starts Kontakt. Kontakt's MIDI reaches it through
Wine 0-3 ms off the grid (and a capture can step once in time), so identical overlapping samples
(repeats, chords) sum differently from a render on the grid: each group of simultaneous note-ons is
moved to the lag (+-40 ms) that best correlates 0.5 s of native audio with v3's, and v3 plays again.

The scenario starts after LEAD seconds of silence: Kontakt keeps loading samples after it reports
the instrument loaded, and a note that arrives first plays late or not at all.

Cases (13 per library): keys 48/60/72 x velocities 40/90/127 held 1.5 s, a 5-note chord, 12 repeats
at 1/12 s, a held note under a CC1 sweep, a pedalled phrase. Tolerance: RMS level 1 dB; each
third-octave band (25 Hz-16 kHz, within 30 dB of the loudest) 1 dB after the level offset; onset 5 ms;
first 60 ms of envelope 1.5 dB; release tail (+100..+700 ms after key-up over the 300 ms before)
1.5 dB; L/R balance 1 dB. Both engines silent (key out of range) counts as within.
"""
import argparse
import json
import math
import os
import subprocess
import sys
import tempfile
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
WIN, SEARCH = 24000, 1920  # replay: correlation window and lag search, frames


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
    if given:
        return given
    root = Path("/data/dev/workspace/Toolchains/User/cargo-target/KONTRA")
    found = sorted(root.glob("*/release/kontra-inspect"), key=lambda p: p.stat().st_mtime, reverse=True)
    if not found:
        sys.exit("kontra-inspect not built: build-slot cargo build --release -p kontra-inspect")
    return str(found[0])


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


def case_metrics(x, hold):
    e = env_ms(x)
    s = smooth(e)
    onset = int(np.argmax(s > s.max() * 0.05)) if s.max() > 1e-5 else None
    off = int(hold * 1000)
    before = float(np.mean(e[max(0, off - 300):off] ** 2))
    tail = (10 * math.log10(float(np.mean(e[off + 100:off + 700] ** 2)) / before + 1e-20)
            if off + 700 < len(e) and before > 1e-10 else None)
    centres, bands = thirds(x)
    return dict(rms=db(np.sqrt((x * x).mean())), env=s, onset=onset, tail=tail, centres=centres, bands=bands,
                balance=db(np.sqrt((x[0] ** 2).mean())) - db(np.sqrt((x[1] ** 2).mean())))


def score_case(n, o, hold):
    a, b = case_metrics(n, hold), case_metrics(o, hold)
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
    fails = [f for f, bad in (
        (f"level {level:+.1f}", abs(level) > TOL["level"]),
        (f"band {worst[0]:.0f}Hz {worst[1]:+.1f}", abs(worst[1]) > TOL["band"]),
        (f"onset {onset}ms", onset is not None and abs(onset) > TOL["onset_ms"]),
        (f"attack {attack or 0:+.1f}", attack is not None and abs(attack) > TOL["attack"]),
        (f"release {tail or 0:+.1f}", tail is not None and abs(tail) > TOL["release"]),
        (f"balance {balance:+.1f}", abs(balance) > TOL["balance"])) if bad]
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
    """`events` moved, per group of simultaneous note-ons, to where Kontakt played them."""
    nat, ours = nat.sum(0), ours.sum(0)
    shift, out, last_on = 0, [], None
    for e in events:
        f = e["frame"]
        if e["type"] == "note_on" and f != last_on:
            last_on = f
            ref = ours[f:f + WIN]
            best, score = 0, 0.5
            for lag in range(-SEARCH, SEARCH + 1, 4):
                a = f + base + lag
                if a < 0 or a + WIN > len(nat) or len(ref) < WIN:
                    continue
                n = nat[a:a + WIN]
                c = float(np.dot(n, ref)) / (math.sqrt(float(np.dot(n, n)) * float(np.dot(ref, ref))) + 1e-20)
                if c > score:
                    best, score = lag, c
            shift = best
        out.append(dict(e, frame=max(0, f + shift)))
    return sorted(out, key=lambda e: e["frame"])


def score(args):
    root = Path(args.root)
    binary = inspect_binary(args.inspect)
    table = {}
    for name in args.only or LIBRARIES:
        cap = root / f"{name}.capture.json"
        if not cap.exists():
            print(f"{name}: no capture (run `capture` first)")
            continue
        c = json.loads(cap.read_text())
        art = Path(c["artifacts"])
        nat = compare.load(art / "native.wav")[:, int(compare.PRE * R):]
        grid = compare.load(art / "v3.wav")
        base = global_lag(nat, grid)
        wav = root / f"{name}.{args.tag}.wav"
        p = play(binary, c["nki"], replayed(nat, grid, c["events"], base), wav=wav)
        if not wav.exists():
            print(f"{name}: v3 render failed: {p.stderr.strip().splitlines()[-1:]}")
            continue
        ours = compare.load(wav)
        nat = nat[:, base:] if base > 0 else nat
        ours = ours if base > 0 else ours[:, -base:]
        rows = []
        for case in c["cases"]:
            a, z = round(case["start"] * R), round((case["start"] + case["len"]) * R)
            if z > min(nat.shape[1], ours.shape[1]):
                rows.append(dict(case=case["name"], ok=False, silent="short"))
                continue
            rows.append(dict(case=case["name"], **score_case(nat[:, a:z], ours[:, a:z], case["hold"])))
        wav.unlink()
        table[name] = rows
        ok = sum(r["ok"] for r in rows)
        print(f"{name}: {ok}/{len(rows)}")
        for r in rows:
            if not r["ok"]:
                print(f"  {r['case']:9} " + (f"silent {r['silent']}" if "silent" in r else ", ".join(r["fails"])))
    total = sum(r["ok"] for rows in table.values() for r in rows)
    cases = sum(len(rows) for rows in table.values())
    print(f"total: {total}/{cases} within tolerance")
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
    args = ap.parse_args()
    capture(args) if args.cmd == "capture" else score(args)


if __name__ == "__main__":
    main()
