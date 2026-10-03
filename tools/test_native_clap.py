#!/usr/bin/env python3
"""Actual exported Linux CLAP host, original tone; no Rust build or editor.

python3 tools/test_native_clap.py PLUGIN CLI CLAP_SDK_CHECKOUT OUT
SDK headers: https://github.com/free-audio/clap/tree/1.2.2 (no SDK vendoring).
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
from test_native_midi import create_fixture, export_state


def main():
    plugin, cli, sdk, out = [Path(p).resolve() for p in sys.argv[1:]]
    source = Path(__file__).resolve().parents[1]
    nki = create_fixture(cli, out)
    states = [export_state(cli, out, nki, streaming="RamOnly"), export_state(cli, out, nki, "home7", channel=7, streaming="RamOnly"),
              export_state(cli, out, nki, "lower", channel=7,
                           mpe={"zone": "Lower", "members": 2, "bend_range": 48}, streaming="RamOnly")]
    host = out / "native-clap-audio"
    cpp = source / "vendor/moose-clap/tests/native_midi_audio.cpp"
    subprocess.run(["g++", "-std=c++17", "-Wall", "-Wextra", "-Werror", "-pedantic", "-O2",
                    "-I", str(sdk / "include"), str(cpp), "-ldl", "-pthread", "-o", str(host)], check=True)
    env = dict(os.environ, KONTRA_DISABLE_NETWORK="1", KONTRA_REPORT_DIR=str(out / "reports"))
    result = subprocess.run([str(host), str(plugin), *map(str, states)], env=env, capture_output=True, text=True)
    def sha(path):
        return hashlib.sha256(path.read_bytes()).hexdigest()
    report = {"plugin": str(plugin), "plugin_sha256": sha(plugin), "cli_sha256": sha(cli),
              "host_sha256": sha(host), "cpp_sha256": sha(cpp), "driver_sha256": sha(Path(__file__)),
              "fixture_driver_sha256": sha(source / "tools/test_native_midi.py"),
              "sdk_commit": subprocess.check_output(["git", "-C", str(sdk), "rev-parse", "HEAD"], text=True).strip(),
              "states": {p.name: sha(p) for p in states},
              "consumer_build": json.loads(subprocess.check_output([str(cli), "--build-info"], text=True)),
              "test_source_commit": subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip(),
              "test_source_dirty": subprocess.check_output(["git", "-C", str(source), "status", "--porcelain"], text=True),
              "exit_code": result.returncode, "stdout": result.stdout, "stderr": result.stderr}
    (out / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    (out / "native-clap.log").write_text(result.stdout + result.stderr)
    print(result.stdout, end="")
    print(result.stderr, end="", file=sys.stderr)
    raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
