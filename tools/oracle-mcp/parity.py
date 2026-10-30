#!/usr/bin/env python3
"""One parity datum: the same note through real Kontakt and through KONTRA.

    tools/oracle-mcp/parity.py [--nki PATH] [--key 60] [--velocity 100] [--hold 1.5]
                               [--window 2.0] [--out DIR]

Kontakt side : oracle-mcp's oracle_render (aplaymidi -> Kontakt -> PipeWire null sink ->
               pw-record), MIDI written here: note on at 0, note off at --hold.
KONTRA side  : `cargo run --release -p kontra-host --example render` in this repo, whose
               default note is velocity 100 held 1.5 s (the oracle's note) - the same
               signal. `kontra-inspect play` cannot be used for this: it reports playback
               observations and only writes a WAV in --oracle mode, which needs
               ~/.wine-oracle slots and a VST3 Kontakt in ~/.wine, neither of which is on
               this machine (see README).

Then compare.py's own functions align the two and report level, timing, 1/3-octave bands
and waveform correlation. Prints everything as JSON, plus the two sha256s.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(REPO / "tools" / "oracle"))

import numpy as np  # noqa: E402  (compare.py needs it anyway)


def load_mod(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


server = load_mod("oracle_mcp_server", HERE / "server.py")
compare = load_mod("oracle_compare", REPO / "tools/oracle/compare.py")
wavutil = load_mod("oracle_wavutil", REPO / "tools/kontakt-oracle/wavutil.py")
client_mod = load_mod("oracle_mcp_client", HERE / "client.py")

RENDER_BIN = Path("/var/tmp/tgt-mcp/release/examples/render")


def write_float_wav(path: Path, audio: np.ndarray, rate: int = 48000) -> None:
    """Interleaved float32 stereo, same layout as hound/pw-record."""
    import struct
    a = np.asarray(audio, dtype="<f4")
    payload = a.tobytes()
    hdr = (b"RIFF" + (36 + len(payload)).to_bytes(4, "little") + b"WAVEfmt "
           + (16).to_bytes(4, "little") + (3).to_bytes(2, "little")
           + (2).to_bytes(2, "little") + rate.to_bytes(4, "little")
           + (rate * 8).to_bytes(4, "little") + (8).to_bytes(2, "little")
           + (32).to_bytes(2, "little") + b"data" + len(payload).to_bytes(4, "little"))
    path.write_bytes(hdr + payload)


def read_float_wav(path: Path) -> np.ndarray:
    _, _, chans = wavutil.read(str(path))
    return np.stack([np.asarray(c, dtype=float) for c in chans])


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--nki", default="/home/dracula/Armory/VSTs/Kontakt1/Native Instruments "
                                     "Session Guitarist Electric Mint 1.1.0/Session Guitarist - "
                                     "Electric Mint Library/Instruments/Electric Mint.nki")
    ap.add_argument("--key", type=int, default=60)
    ap.add_argument("--velocity", type=int, default=100)
    ap.add_argument("--hold", type=float, default=1.5)
    ap.add_argument("--window", type=float, default=2.0)
    ap.add_argument("--out", default="/tmp/opencode/oracle-mcp/parity")
    args = ap.parse_args()

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    nki = Path(args.nki).resolve()
    if not nki.exists():
        print(f"nki not found: {nki}", file=sys.stderr)
        return 2
    midi = out / "parity.mid"
    meta = server.write_midi(midi, [{"start_s": 0.0, "length_s": args.hold,
                                     "key": args.key, "velocity": args.velocity}])
    print(f"midi: {midi} {meta['notes']} note(s) {meta['duration_s']}s", file=sys.stderr)

    # ---- Kontakt: through the oracle MCP, exactly as an agent would call it
    kontakt_wav = out / "kontakt.wav"
    c = client_mod.Client()
    try:
        c.handshake()
        steps = [
            ("oracle_up", {}),
            ("oracle_load", {"nki": str(nki)}),
            ("oracle_render", {"midi": str(midi)}),
            ("oracle_down", {}),
        ]
        render = None
        for name, arguments in steps:
            resp = c.tool(name, arguments)
            res = resp.get("result") or {}
            body = client_mod.text_of(resp)
            if res.get("isError"):
                print(f"{name} failed: {body}", file=sys.stderr)
                return 1
            if name == "oracle_render":
                render = json.loads(body)
            print(f"{name}: ok", file=sys.stderr)
    finally:
        c.close()
    kontakt_wav = Path(render["wav"])

    # ---- KONTRA: the repo's own WAV renderer, same note (velocity 100 held 1.5 s default)
    if not RENDER_BIN.exists():
        print(f"missing {RENDER_BIN}; build it with:\n"
              f"  CARGO_TARGET_DIR=/var/tmp/tgt-mcp cargo build --release -p kontra-host "
              f"--example render", file=sys.stderr)
        return 2
    kontra_wav = out / "kontra.wav"
    env = {"KONTRA_VELOCITY": str(args.velocity), "KONTRA_HOLD": str(args.hold),
           "PATH": "/usr/bin:/bin", "HOME": str(Path.home())}
    p = subprocess.run([str(RENDER_BIN), str(nki), str(args.key), str(kontra_wav)], env=env,
                       capture_output=True, text=True, timeout=900)
    if p.returncode != 0 or not kontra_wav.exists():
        print(f"kontra render failed rc={p.returncode}: {p.stderr[-600:]}", file=sys.stderr)
        return 1
    print("kontra render: ok", file=sys.stderr)

    # ---- compare.py expects a capture.py-style file: PRE=0.5 s before the note
    k = read_float_wav(kontakt_wav)
    onsets = np.abs(k).max(axis=0) > 10 ** (-60 / 20)
    first = int(np.argmax(onsets)) if onsets.any() else 0
    pre = int(compare.PRE * compare.RATE)
    start = first - pre
    if start >= 0:
        k = k[:, start:]
    else:
        k = np.concatenate([np.zeros((2, -start)), k], axis=1)
    aligned = out / "kontakt-as-capture.wav"
    write_float_wav(aligned, k.T)

    row = compare.compare_one("parity", aligned, kontra_wav, args.window)
    summary = {
        "nki": str(nki),
        "note": {"key": args.key, "velocity": args.velocity, "hold_s": args.hold},
        "window_s": args.window,
        "kontakt": {"wav": str(kontakt_wav), "sha256": render["sha256"],
                    "duration_s": render["duration_s"], "peak_dbfs": render["peak_dbfs"],
                    "onset_s": render["onset_s"], "frames": render["frames"]},
        "kontra": {"wav": str(kontra_wav),
                   "sha256": hashlib.sha256(kontra_wav.read_bytes()).hexdigest(),
                   "frames": int(read_float_wav(kontra_wav).shape[1])},
        "identical_waveforms": None,
        "compare": row,
        "hashes_equal": None,
    }
    summary["hashes_equal"] = summary["kontakt"]["sha256"] == summary["kontra"]["sha256"]
    same_len = None
    a, b = read_float_wav(kontakt_wav), read_float_wav(kontra_wav)
    if a.shape == b.shape:
        same_len = float(np.abs(a - b).max())
    summary["identical_waveforms"] = same_len  # max |difference| when lengths match, else null
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
