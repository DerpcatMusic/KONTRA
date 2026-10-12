#!/usr/bin/env python3
"""Validate SDK Brightness5 and PolyPressure through the actual exported VST3.

python3 tools/test_native_note_expression.py PLUGIN CLI OUTPUT_DIRECTORY
Builds only a tiny C++ host; root controls plugin/CLI compilation and execution.
"""
import copy
import hashlib
import json
import math
import os
import shutil
from pathlib import Path
import struct
import subprocess
import sys
import wave

from test_native_midi import export_state


def create_states(cli, out):
    samples = out / "samples"
    samples.mkdir(parents=True, exist_ok=True)
    with wave.open(str(samples / "Tone C3.wav"), "wb") as audio:
        audio.setparams((2, 2, 48000, 0, "NONE", "not compressed"))
        audio.writeframes(b"".join(struct.pack("<hh", *[int(math.sin(n * math.tau * 261.625565 / 48000) * 8192)] * 2)
                                  for n in range(144000)))
    subprocess.run([str(cli), "create-library", str(samples), "--name", "NativeExpression",
                    "--vendor", "AuthoredTest", "--out", str(out / "library")], check=True)
    source = next((out / "library").rglob("*.kontra-instrument"))
    nki = next((out / "library").rglob("*.nki"))
    original = json.loads(source.read_text())
    states = []
    for name, modulation in [("brightness", {"MidiCc": 74}), ("pressure", "MonoAftertouch"), ("oracle", None)]:
        instrument = copy.deepcopy(original)
        # Generated children exercise inheritance and released-family targeting.
        instrument["scripts"] = ["on note\nwait(1000)\nplay_note($EVENT_NOTE+12,127,0,-1)\nend on\non poly_at\nend on"]
        for group in instrument["groups"]:
            group["volume_env"]["release_ms"] = 50.0
            # All three have the same base gain/velocity/source/envelope. The
            # oracle has no CC74 or pressure assignment, only SDK Volume input.
            group["mods"] = [] if modulation is None else [{
                "name": "ABSOLUTE_NORMALIZED_SOURCE", "source": modulation,
                "cc_default": 127 if name == "brightness" else None,
                "target": "Volume", "intensity": 1.0, "invert": False,
                "lag_ms": 0, "shaper": None,
            }]
        authored = source.with_name(f"{name}.kontra-instrument")
        authored.write_text(json.dumps(instrument, indent=2) + "\n")
        # The migration exporter deliberately accepts only NKI/NKM. Export a
        # real NKI base, then author a host-state selection for Native JSON,
        # which the actual plugin loader explicitly supports. Equal byte length
        # preserves every keyed-field/envelope frame; no codec is duplicated.
        suffix_padding = len(".kontra-instrument") - len(".nki")
        accepted = authored.with_name(name + "_" * suffix_padding + ".nki")
        shutil.copyfile(nki, accepted)
        base_name = name + "-nki-base"
        try:
            base_state = export_state(cli, out, accepted, base_name)
        except subprocess.CalledProcessError as error:
            (out / (base_name + "-export.stdout")).write_text(error.stdout or "")
            (out / (base_name + "-export.stderr")).write_text(error.stderr or "")
            raise RuntimeError(f"actual NKI state export failed: {error.stderr or error.stdout}") from error
        old, new = str(accepted).encode(), str(authored).encode()
        assert len(old) == len(new), "fixture paths must have identical UTF8 lengths"
        original_state = base_state.read_bytes()
        occurrences = original_state.count(old)
        assert occurrences == 1, f"expected one exact selection path, found {occurrences}"
        changed = original_state.replace(old, new)
        assert len(changed) == len(original_state) and changed.replace(new, old) == original_state
        state = out / (name + ".state")
        state.write_bytes(changed)
        parsed = subprocess.run([str(cli), "compare-plugin-state", str(state), str(state)],
                                capture_output=True, text=True)
        (out / (name + "-state-parse.stdout")).write_text(parsed.stdout)
        (out / (name + "-state-parse.stderr")).write_text(parsed.stderr)
        if parsed.returncode:
            raise RuntimeError(f"actual plugin state parser rejected authored fixture: {parsed.stderr or parsed.stdout}")
        report = json.loads(parsed.stdout)
        assert report["status"] == "semantically_matching"
        assert report["expected"]["parts"][0]["path"] == str(authored)
        (out / (name + "-state-authorship.json")).write_text(json.dumps({
            "base_exported_nki": str(accepted), "authored_native": str(authored),
            "replaced_path_occurrences": occurrences, "byte_length_preserved": True,
            "reversible_path_edit": True, "actual_plugin_state_parser_accepted": True,
            "exporter_native_support_claimed": False,
        }, indent=2) + "\n")
        states.append(state)
    return states


def main():
    plugin, cli, out = [Path(p).resolve() for p in sys.argv[1:]]
    source = Path(__file__).resolve().parents[1]
    states = create_states(cli, out)
    host = out / "native-note-expression-audio"
    subprocess.run(["g++", "-std=c++17", "-O2", str(source / "vendor/moose-vst3/tests/native_note_expression_audio.cpp"),
                    "-ldl", "-pthread", "-o", str(host)], check=True)
    env = dict(os.environ, KONTRA_DISABLE_NETWORK="1", KONTRA_REPORT_DIR=str(out / "reports"))
    result = subprocess.run([str(host), str(plugin), *map(str, states)], env=env, capture_output=True, text=True)
    (out / "native-note-expression.log").write_text(result.stdout + result.stderr)
    report = {"plugin": str(plugin), "plugin_sha256": hashlib.sha256(plugin.read_bytes()).hexdigest(),
              "host_sha256": hashlib.sha256(host.read_bytes()).hexdigest(), "exit_code": result.returncode,
              "stdout": result.stdout, "stderr": result.stderr}
    (out / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(result.stdout, end="")
    print(result.stderr, end="", file=sys.stderr)
    raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
