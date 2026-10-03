#!/usr/bin/env python3
"""Load an original tone through a real exported Linux VST3 process entrypoint.

python3 tools/test_native_midi.py /absolute/libkontakto.so /absolute/kontakto OUT
Requires g++; does not build Rust or open an editor.
"""
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess
import sys
import wave


def create_fixture(cli, out):
    samples = out / "samples"
    samples.mkdir(parents=True, exist_ok=True)
    with wave.open(str(samples / "Tone C3.wav"), "wb") as audio:
        audio.setparams((2, 2, 48000, 0, "NONE", "not compressed"))
        audio.writeframes(b"".join(struct.pack("<hh", *[int(math.sin(n * math.tau * 261.625565 / 48000) * 8192)] * 2)
                                   for n in range(144000)))
    subprocess.run([str(cli), "create-library", str(samples), "--name", "NativeMIDI",
                    "--vendor", "AuthoredTest", "--out", str(out / "library"), "--kontakt-only"], check=True)
    nki = next((out / "library").rglob("*.nki"))
    return nki


def export_state(cli, out, nki, name="tone", **routing):
    multi = out / f"{name}.kontra-multi"
    part = {"path": str(nki), "port": 0, "channel": -1, "output": 0,
            "aux": -1, "aux_gain": -60., "output_manual": True, "mic_buses": [], "mic_names": []}
    part.update(routing)
    multi.write_text(json.dumps({"format": "kontra-multi", "version": 1, "name": "Native MIDI fixture",
                                "parts": [part]}))
    state = out / f"{name}.state"
    exported = subprocess.run([str(cli), "export-multi-state", str(multi), str(state)], check=True,
                              capture_output=True, text=True)
    (out / ("state-export.json" if name == "tone" else f"{name}-state-export.json")).write_text(exported.stdout)
    return state


def main():
    plugin, cli, out = [Path(p).resolve() for p in sys.argv[1:]]
    source = Path(__file__).resolve().parents[1]
    nki = create_fixture(cli, out)
    state = export_state(cli, out, nki)
    host = out / "native-midi-audio"
    subprocess.run(["g++", "-std=c++17", "-O2", str(source / "vendor/moose-vst3/tests/native_midi_audio.cpp"),
                    "-ldl", "-pthread", "-o", str(host)], check=True)
    import os
    environment = dict(os.environ, KONTRA_DISABLE_NETWORK="1", KONTRA_REPORT_DIR=str(out / "reports"))
    result = subprocess.run([str(host), str(plugin), str(state)], env=environment, capture_output=True, text=True)
    (out / "native-midi.log").write_text(result.stdout + result.stderr)
    report = {"plugin": str(plugin), "plugin_sha256": hashlib.sha256(plugin.read_bytes()).hexdigest(),
              "host_sha256": hashlib.sha256(host.read_bytes()).hexdigest(), "exit_code": result.returncode,
              "stdout": result.stdout, "stderr": result.stderr}
    (out / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(result.stdout, end="")
    print(result.stderr, end="", file=sys.stderr)
    raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
