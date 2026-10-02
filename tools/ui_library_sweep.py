#!/usr/bin/env python3
"""Sequential local-owned-library UI probe; outputs metadata and ignored artifacts.

The test binary must contain real_instrument_frame_benchmark. This measures a
headless physical GPU and native worker, not the running host's present rate.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import time

TEST = "ui::audit::tests::real_instrument_frame_benchmark"
MODES = {"Original": 1, "KONTRA": 2, "Vectorized": 3}


def cases(plan, libraries, modes, scales):
    seen = set()
    for library in plan["libraries"]:
        if libraries and library["name"] not in libraries:
            continue
        for patch in library["patches"]:
            for program in patch["initial_read_audit"]["programs"]:
                for mode in modes:
                    for scale in scales:
                        identity = (patch["path"], program["program"], mode, scale)
                        if identity in seen:
                            continue
                        seen.add(identity)
                        yield {"library": library["name"], "patch": patch["path"],
                               "program": program["program"], "mode": mode,
                               "device_scale": scale}


def parse(output):
    result = {"stages": []}
    for line in output.splitlines():
        if line.startswith("UI_BENCH_CALLBACK "):
            result["callbacks"] = json.loads(line.removeprefix("UI_BENCH_CALLBACK "))
        elif line.startswith("UI_BENCH "):
            fields = dict(re.findall(r"(\w+)=([^\s]+)", line))
            if "stage" in fields:
                result["stages"].append(fields)
            elif "adapter" in fields:
                result["adapter"] = line.removeprefix("UI_BENCH adapter=")
            else:
                result.update(fields)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--library", action="append", default=[])
    parser.add_argument("--mode", choices=MODES, action="append")
    parser.add_argument("--device-scale", type=int, choices=[1, 2], action="append")
    parser.add_argument("--frames", type=int, default=24)
    parser.add_argument("--edits", type=int, default=24)
    parser.add_argument("--host-frames", type=int, choices=[64, 128, 512], default=128)
    parser.add_argument("--timeout", type=int, default=300)
    parser.add_argument("--cpu", action="store_true", help="explicit CPU reference, not physical GPU")
    parser.add_argument("--callbacks", action="store_true")
    parser.add_argument("--shots", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    plan = json.loads(args.plan.read_text())
    selected = list(cases(plan, args.library, args.mode or list(MODES), args.device_scale or [1, 2]))
    if args.dry_run:
        print(json.dumps({"cases": len(selected), "libraries": sorted({c["library"] for c in selected}),
                          "programs": len({(c["patch"], c["program"]) for c in selected})}, indent=2))
        return
    # Never overwrite prior evidence, nor write into a caller's ordinary folder.
    args.output.mkdir(parents=True, exist_ok=False)
    binary = str(args.binary.resolve(strict=True))
    records = []
    for index, case in enumerate(selected):
        name = f"{index:03d}"
        env = os.environ.copy()
        env.update(KONTRA_UI_BENCH_PATCH=case["patch"], KONTRA_UI_BENCH_PROGRAM=str(case["program"]),
                   KONTRA_UI_BENCH_MODE=str(MODES[case["mode"]]),
                   KONTRA_UI_BENCH_DEVICE_SCALE=str(case["device_scale"]),
                   KONTRA_UI_BENCH_FRAMES=str(args.frames), KONTRA_UI_BENCH_EDITS=str(args.edits),
                   KONTRA_UI_BENCH_HOST_FRAMES=str(args.host_frames), RUST_MIN_STACK="16777216")
        # Every case has isolated preferences; the user's live editor is untouched.
        env["XDG_CONFIG_HOME"] = str((args.output / f"{name}-config").resolve())
        for key in ["KONTRA_UI_BENCH_GPU", "KONTRA_UI_BENCH_CALLBACKS", "KONTRA_UI_BENCH_SHOT", "KONTRA_UI_BENCH_CONTROL_VARIABLE"]:
            env.pop(key, None)
        if not args.cpu:
            env["KONTRA_UI_BENCH_GPU"] = "1"
        if args.callbacks:
            env["KONTRA_UI_BENCH_CALLBACKS"] = "1"
            if case["library"] == "ANALOG STRINGS":
                env["KONTRA_UI_BENCH_CONTROL_VARIABLE"] = "$macros__intensity0"
        if args.shots:
            env["KONTRA_UI_BENCH_SHOT"] = str((args.output / f"{name}.png").resolve())
        start = time.monotonic()
        try:
            completed = subprocess.run([binary, "--ignored", "--exact", TEST, "--nocapture", "--test-threads=1"],
                                       env=env, capture_output=True, text=True, timeout=args.timeout)
            output = completed.stdout + completed.stderr
            status = "passed" if completed.returncode == 0 else "failed"
        except subprocess.TimeoutExpired as error:
            output = (error.stdout or b"") + (error.stderr or b"")
            if isinstance(output, bytes):
                output = output.decode(errors="replace")
            status = "timeout"
        (args.output / f"{name}.log").write_text(output)
        measurement = parse(output)
        if status == "passed" and measurement.get("status") == "skipped":
            status = "skipped"
        record = {**case, **measurement, "status": status, "wall_seconds": time.monotonic() - start}
        records.append(record)
        (args.output / "results.json").write_text(json.dumps({"scope": "headless UI; one selected control, not all controls or notes",
            "binary": binary, "records": records}, indent=2))
        print(f"{index + 1}/{len(selected)} {case['library']} program {case['program']} {case['mode']} {case['device_scale']}x: {status}", flush=True)
    failed = sum(r["status"] in ("failed", "timeout") for r in records)
    raise SystemExit(bool(failed))


if __name__ == "__main__":
    main()
