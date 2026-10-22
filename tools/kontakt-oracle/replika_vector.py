#!/usr/bin/env python3
"""Native Kontakt 8 vector for the Replika Delay (Effects/RDelay, saved v0x12): one Kontakt session.

    replika_vector.py OUT_PREFIX
        -> OUT_PREFIX.tsv           the vector: L and R output per frame (float32, %.9g), header comments
        -> OUT_PREFIX.controls.tsv  one row per step: repeats, levels, change against the baseline

Parameter path: "reload". The earlier capture (docs/re/engine/k-delays.md) wrote the values during playback
with a KSP on controller, and no echo changed. Here every step is built and saved before playback:
  1. a fresh instrument gets the INIT effect in insert slot 0 (kt.load_fx_preset, as probe_fx.py);
  2. an init script runs set_engine_par($ENGINE_PAR_RDL_x, v, -1, 0, 0), the call probe_fx.py makes;
  3. the script is replaced by an empty one and kt.save_instrument() writes a .nki, so the file holds the values only.
Then reset_multi() and kt.load_instrument() of each file. The effect state therefore reaches the core through the
saved-file loader only. Each step is one MIDI note (60 + step number) held for the stimulus length; the recording
is cut at each step's impulse onset.

render.py runs the single Kontakt session (private PipeWire null sink). No other Kontakt or Wine process may run.

Result of the first two renders (2026-10-22, Kontakt 8.13.1): no step moved the output. The committed capture is
docs/re/engine/vectors/replika-init-reload-noop.tsv (INIT output), not a vector for the listed parameters.
"""
import subprocess
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import eqrender  # noqa: E402
import stimrender  # noqa: E402
import wavutil  # noqa: E402

RATE = 48000
LEAD = 2.0      # s of silence before the first note
PERIOD = 6.0    # s from one step's onset to the next (a repeat train at FEEDBACK 0.5 is below -35 dB by then)
HOLD = 6.0      # s: note length = stimulus length (impulse, then silence)
WINDOW = 2.5    # s of each step written to the vector TSV (covers two repeats at 1000 ms)
ANALYSE = 5.5   # s per step used for the controls metrics

# (name, [(RDL name, raw value 0..1000000 or the TYPE integer 0..4), ...]); the first step is the INIT preset.
STEPS = [
    ("baseline", []),
    ("vector", [("RDL_TYPE", 0), ("RDL_TIME", 500000), ("RDL_FEEDBACK", 500000)]),
    ("time_only", [("RDL_TIME", 500000)]),
    ("feedback_0", [("RDL_FEEDBACK", 0)]),
    ("saturation_max", [("RDL_SATURATION", 1000000)]),
    ("highcut_0", [("RDL_HIGHCUT", 0)]),
]
# PINGPONG is not in the first render (one vector plus controls only); add a step when a second render is justified.
VECTOR = "vector"


def lua_s(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"'


def gen_lua(dirwin: str, wavwin: str, initwin: str) -> str:
    steps = []
    for n, (name, sets) in enumerate(STEPS, 1):
        ksp = "".join(f"  set_engine_par($ENGINE_PAR_{p}, {v}, -1, 0, 0)\n" for p, v in sets)
        steps.append(f"{{{n}, {lua_s(name)}, {lua_s(ksp)}}}")
    return f"""
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local DIR = {lua_s(dirwin)}
local WAV = {lua_s(wavwin)}
local INIT = {lua_s(initwin)}
local STEPS = {{{", ".join(steps)}}}
local meta = assert(io.open(DIR .. "meta.txt", "w"))
local function try(name, f, ...)
  local ok, r = pcall(f, ...)
  meta:write(name, " ", tostring(ok), " ", tostring(r), "\\n")
  return ok, r
end
meta:write("kontakt_version ", tostring(kt.version), "\\n")
for _, s in ipairs(STEPS) do
  try("reset", kt.reset_multi)
  local inst = kt.add_instrument()
  local z = kt.add_zone(inst, 0, WAV)
  kt.set_zone_geometry(inst, z, {{ root_key = 59 + s[1], low_key = 59 + s[1], high_key = 59 + s[1] }})
  kt.set_instrument_volume(inst, 0)
  try("load_fx " .. s[2], kt.load_fx_preset, INIT, inst, -1, 0, kt.insert_fx)
  if s[3] ~= "" then
    try("script " .. s[2], kt.set_instrument_script_source, inst, 0, "on init\\n" .. s[3] .. "end on\\n")
    try("clear " .. s[2], kt.set_instrument_script_source, inst, 0, "on init\\nend on\\n")
  end
  try("save " .. s[2], kt.save_instrument, inst, DIR .. "step_" .. s[1] .. ".nki", {{}})
end
try("reset", kt.reset_multi)
for _, s in ipairs(STEPS) do
  try("load " .. s[2], kt.load_instrument, DIR .. "step_" .. s[1] .. ".nki")
end
meta:close()
out:write('{{"ready":true}}\\n')
out:close()
"""


def make_midi(path: Path) -> None:
    ev = []
    for n in range(1, len(STEPS) + 1):
        # The reloaded instrument of step n sits on MIDI channel n (kt.load_instrument returned index 128*(n-1)
        # in the first render), so step n's note goes out on channel n, not on channel 1.
        t, note, ch = LEAD + (n - 1) * PERIOD, 59 + n, n - 1
        ev += [(t, bytes([0x90 | ch, note, 100])), (t + HOLD, bytes([0x80 | ch, note, 0]))]
    trk, last = b"\x00\xff\x51\x03" + (500000).to_bytes(3, "big"), 0
    for t, m in sorted(ev, key=lambda e: e[0]):
        tick = round(t * 960)
        trk += eqrender.vlq(tick - last) + m
        last = tick
    trk += b"\x00\xff\x2f\x00"
    path.write_bytes(b"MThd" + (6).to_bytes(4, "big") + bytes([0, 0, 0, 1]) + (480).to_bytes(2, "big") +
                     b"MTrk" + len(trk).to_bytes(4, "big") + trk)


def onset(L, R, pred: int) -> tuple[int, float]:
    """Impulse onset: the largest single-sample jump of L+R in [pred - 20 ms, pred + 100 ms]."""
    lo, hi = max(pred - int(0.02 * RATE), 0), pred + int(0.1 * RATE)
    jump = np.abs(np.diff((L + R)[lo:hi]))
    k = int(np.argmax(jump))
    return lo + k + 1, float(jump[k])


def metrics(L, R, s: int, bs: int) -> dict:
    """Controls of the step whose onset is s, against the baseline whose onset is bs (same recording)."""
    n = int(ANALYSE * RATE)
    l, r = L[s:s + n], R[s:s + n]
    env = np.maximum(np.abs(l), np.abs(r))
    peak = float(env.max())
    cand = np.nonzero((env[1:-1] > env[:-2]) & (env[1:-1] >= env[2:]) & (env[1:-1] > 0.01 * peak))[0] + 1
    reps = []
    for k in cand:
        if not reps or k - reps[-1] > 0.005 * RATE:
            reps.append(int(k))
        elif env[k] > env[reps[-1]]:
            reps[-1] = int(k)
    rms = lambda a: float(np.sqrt(np.mean(a.astype(np.float64) ** 2)) + 1e-30)  # noqa: E731
    tail = l[reps[0]:] + r[reps[0]:] if reps else None
    centroid = float("nan")
    if tail is not None and len(tail) > 0:
        spec = np.abs(np.fft.rfft(tail * np.hanning(len(tail)), 1 << 17))
        freqs = np.fft.rfftfreq(1 << 17, 1 / RATE)
        centroid = float((spec * freqs).sum() / max(spec.sum(), 1e-30))
    bl, br = L[bs:bs + n], R[bs:bs + n]
    return {
        "n_repeats": len(reps),
        "repeat_ms": [round(k / RATE * 1000, 1) for k in reps[:4]],
        "repeat_amp": [round(float(env[k] / peak), 4) for k in reps[:4]],
        "rms_L_db": round(20 * np.log10(rms(l)), 2),
        "rms_R_db": round(20 * np.log10(rms(r)), 2),
        "R_over_L_db": round(20 * np.log10(rms(r) / rms(l)), 2),
        "centroid_hz": round(centroid, 0),
        "rel_diff_L": round(rms(l - bl) / rms(bl), 4),
        "rel_diff_R": round(rms(r - br) / rms(br), 4),
        "dry_peak": round(peak, 5),
    }


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    prefix = Path(sys.argv[1]).resolve()
    scratch = eqrender.scratch_dir()
    stim = scratch / "stimulus.wav"
    x = np.zeros(int(HOLD * RATE))
    x[0] = 0.5
    stimrender.write_wav(stim, x)
    lua, midi, wav = scratch / "replika_vector.lua", scratch / "notes.mid", scratch / "render.wav"
    lua.write_text(gen_lua(eqrender.winpath(scratch) + "\\", eqrender.winpath(stim),
                           eqrender.winpath(eqrender.init_preset("Effects/RDelay"))))
    make_midi(midi)
    seconds = LEAD + len(STEPS) * PERIOD + 2.0
    subprocess.run([sys.executable, str(HERE / "render.py"), str(lua), str(wav), str(seconds), str(midi)], check=True)

    _, rate, (L, R) = wavutil.read(wav)
    assert rate == RATE, rate
    L, R = np.array(L, dtype=np.float64), np.array(R, dtype=np.float64)
    first = int(np.nonzero(np.maximum(np.abs(L), np.abs(R)) > 1e-4)[0][0])
    onsets, jumps = [], []
    for n in range(len(STEPS)):
        pred = first + int(n * PERIOD * RATE) if n else first
        s, j = onset(L, R, pred)
        onsets.append(s)
        jumps.append(j)
    rows = []
    for n, (name, sets) in enumerate(STEPS):
        m = metrics(L, R, onsets[n], onsets[0])
        rows.append([name, " ".join(f"{p}={v}" for p, v in sets) or "INIT", str(onsets[n]), f"{jumps[n]:.5f}",
                     str(m["n_repeats"]), " ".join(map(str, m["repeat_ms"])), " ".join(map(str, m["repeat_amp"])),
                     str(m["rms_L_db"]), str(m["rms_R_db"]), str(m["R_over_L_db"]), str(m["centroid_hz"]),
                     str(m["rel_diff_L"]), str(m["rel_diff_R"]), str(m["dry_peak"])])
    head = ["step", "params", "onset_frame", "onset_jump", "n_repeats", "repeat_ms", "repeat_amp", "rms_L_db",
            "rms_R_db", "R_over_L_db", "centroid_hz", "rel_diff_L", "rel_diff_R", "dry_peak"]
    Path(str(prefix) + ".controls.tsv").write_text("\t".join(head) + "\n" + "".join("\t".join(r) + "\n" for r in rows))

    v = [name for name, _ in STEPS].index(VECTOR)
    s0 = onsets[v]
    n_frames = int(WINDOW * RATE)
    meta = (scratch / "meta.txt").read_text().strip().splitlines() if (scratch / "meta.txt").exists() else []
    version = next((l.split(" ", 1)[1] for l in meta if l.startswith("kontakt_version")), "unknown")
    log = "; ".join(l for l in meta if not l.startswith("kontakt_version")) or "no session log"
    header = [
        f"# Replika Delay (module 0x5A, Effects/RDelay, saved v0x12) native Kontakt 8 vector; Kontakt version {version}.",
        "# Parameter path: reload. Per step, a fresh instrument gets the INIT preset in insert slot 0, an init script runs",
        "#   set_engine_par($ENGINE_PAR_RDL_x, raw, -1, 0, 0), the script is cleared, save_instrument writes a .nki, and the",
        "#   session reloads the files with kt.load_instrument. No playback-time parameter writes.",
        "# Vector parameters: RDL_TYPE 0 (integer 0..4), RDL_TIME 500000 (raw 0..1000000), RDL_FEEDBACK 500000 (raw).",
        "#   Every other control keeps the INIT preset value. Controls for the other parameters: <name>.controls.tsv.",
        "# Stimulus: one 0.5 unit impulse at sample 0 (left channel, float32), then silence; the note is held for 6 s.",
        "#   Recorded at 48000 Hz, float32 stereo, from Kontakt's PipeWire null sink monitor. Kontakt's group pan law",
        "#   (0.7677) and instrument volume (0 dB) are in the path. Block size: not set by this harness, not recorded.",
        f"# Frame 0 = impulse onset: largest single-sample jump of L+R near the scheduled note (onset_frame {s0} in the",
        "#   recording; the jump is reported in controls.tsv). Frames 0.. are the output from that onset.",
        f"# Frames: {n_frames} at {RATE} Hz ({WINDOW} s). Columns: frame, L, R.",
        "# Session log (kt calls, ok flag, result): " + log,
    ]
    body = [f"{i}\t{l:.9g}\t{r:.9g}" for i, (l, r) in enumerate(zip(L[s0:s0 + n_frames], R[s0:s0 + n_frames]))]
    Path(str(prefix) + ".tsv").write_text("\n".join(header) + "\nframe\tL\tR\n" + "\n".join(body) + "\n")
    print("onsets", onsets, "jumps", [round(j, 5) for j in jumps])
    print(f"{prefix}.tsv {n_frames} frames; {prefix}.controls.tsv; scratch {scratch}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
