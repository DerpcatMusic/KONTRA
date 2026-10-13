#!/usr/bin/env python3
"""Sequential local-owned-library UI probe; outputs metadata and ignored artifacts.

The test binary must contain the exact ignored historical benchmark below.
Its default renderer measures a headless physical GPU; --cpu selects an explicit
CPU reference and --callbacks adds native-worker evidence. It does not measure
the running host's present rate. Current, differently scoped tests are not substitutes.
"""
import argparse
import json
import math
import os
from pathlib import Path
import re
import subprocess
import time

TEST = "ui::audit::tests::real_instrument_frame_benchmark"
MODES = {"Original": 1, "KONTRA": 2, "Vectorized": 3}


class InvalidProbe(ValueError):
    """The requested benchmark or its evidence was not established."""


def discover(binary, timeout):
    """Refuse an incompatible binary before creating any case artifacts."""
    completed = subprocess.run([binary, "--ignored", "--exact", TEST, "--list"],
                               capture_output=True, text=True, timeout=timeout)
    listed = re.findall(r"^(.+): (test|benchmark)$", completed.stdout, re.MULTILINE)
    summaries = re.findall(r"^(\d+) tests?, (\d+) benchmarks?$", completed.stdout, re.MULTILINE)
    if completed.returncode != 0 or listed != [(TEST, "test")] or summaries != [("1", "0")]:
        raise InvalidProbe(f"binary does not list exactly the required ignored test: {TEST}")


def one_test_passed(output):
    names = re.findall(r"^test (\S+) \.\.\.", output, re.MULTILINE)
    counts = re.findall(r"^running (\d+) tests?$", output, re.MULTILINE)
    summaries = re.findall(
        r"^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; \d+ filtered out;[^\n]*$",
        output, re.MULTILINE)
    return names == [TEST] and counts == ["1"] and summaries == [("ok", "1", "0", "0", "0")]


def cases(plan, libraries, modes, scales, view_scales=(0,), appearances=(0,)):
    seen = set()
    for library in plan["libraries"]:
        if libraries and library["name"] not in libraries:
            continue
        for patch in library["patches"]:
            for program in patch["initial_read_audit"]["programs"]:
                for mode in modes:
                    for scale in scales:
                        for view_scale in view_scales:
                            for appearance in appearances:
                                identity = (patch["path"], patch.get("snapshot"), program["program"], mode, scale, view_scale, appearance)
                                if identity in seen:
                                    continue
                                seen.add(identity)
                                yield {"library": library["name"], "library_root": library["path"], "patch": patch["path"],
                                       "snapshot": patch.get("snapshot"), "program": program["program"], "mode": mode, "view_scale": view_scale,
                                       "appearance": appearance, "device_scale": scale}


def parse(output):
    result = {"stages": []}
    def invalid_constant(value):
        raise InvalidProbe(f"invalid JSON constant: {value}")

    for line in output.splitlines():
        line = line.removeprefix(f"test {TEST} ... ")
        if line.startswith("UI_BENCH_CALLBACK "):
            if "callbacks" in result:
                raise InvalidProbe("duplicate callback report")
            result["callbacks"] = json.loads(line.removeprefix("UI_BENCH_CALLBACK "), parse_constant=invalid_constant)
            if not isinstance(result["callbacks"], dict):
                raise InvalidProbe("callback report must be a JSON object")
        elif line.startswith("UI_BENCH_COST "):
            if "cost_partition" in result:
                raise InvalidProbe("duplicate cost partition")
            result["cost_partition"] = json.loads(line.removeprefix("UI_BENCH_COST "), parse_constant=invalid_constant)
            if not isinstance(result["cost_partition"], dict):
                raise InvalidProbe("cost partition must be a JSON object")
        elif line.startswith("UI_BENCH "):
            fields = dict(re.findall(r"(\w+)=([^\s]+)", line))
            if "stage" in fields:
                result["stages"].append(fields)
            elif "adapter" in fields:
                if "adapter" in result:
                    raise InvalidProbe("duplicate GPU adapter report")
                result["adapter"] = line.removeprefix("UI_BENCH adapter=")
            else:
                if not fields:
                    raise InvalidProbe("empty UI_BENCH report")
                if any(key in result and result[key] != value for key, value in fields.items()):
                    raise InvalidProbe("conflicting UI_BENCH metadata")
                result.update(fields)
    return result


def number(value, label, *, positive=False, integer=False):
    if isinstance(value, bool) or not isinstance(value, (str, int, float)):
        raise InvalidProbe(f"missing or invalid {label}")
    try:
        numeric = float(value)
    except (ValueError, OverflowError):
        raise InvalidProbe(f"missing or invalid {label}") from None
    if not math.isfinite(numeric) or numeric < 0 or (positive and numeric == 0) or (integer and not numeric.is_integer()):
        raise InvalidProbe(f"missing or invalid {label}")
    return numeric


def validate_measurement(measurement, case, cpu, callbacks, frames, edits, host_frames):
    for key, expected in (("program", case["program"]), ("mode", MODES[case["mode"]]),
                          ("device_scale", case["device_scale"]), ("view_scale", case["view_scale"]),
                          ("appearance", case["appearance"])):
        if number(measurement.get(key), key) != expected:
            raise InvalidProbe(f"probe {key} does not match requested case")
    for key in ("import_ms", "setup_ms"):
        number(measurement.get(key), key)
    number(measurement.get("controls"), "controls", positive=True, integer=True)
    number(measurement.get("visible_control"), "visible_control", integer=True)
    renderer = ("cpu_paint", "cpu_raster") if cpu else ("gpu_prepare_upload_submit", "gpu_wait")
    if not cpu and not re.search(r"\bdevice_type: (DiscreteGpu|IntegratedGpu)\b", measurement.get("adapter", "")):
        raise InvalidProbe("missing physical GPU adapter evidence")
    common = ("build", "layout", "scene_clone", "snapshot_lock", *renderer)
    required = {(changing, stage) for changing in ("false", "true") for stage in common}
    required.add(("true", "edit_control"))
    observed = set()
    for stage in measurement["stages"]:
        identity = (stage.get("changing"), stage.get("stage"))
        if identity not in required or identity in observed:
            raise InvalidProbe("unexpected or duplicate frame stage")
        observed.add(identity)
        if number(stage.get("n"), "stage sample count", positive=True, integer=True) != min(120, max(4, frames)):
            raise InvalidProbe("frame stage sample count does not match requested frames")
        for key in ("mean_ms", "median_ms", "p99_ms"):
            number(stage.get(key), f"{identity} {key}")
    if observed != required:
        raise InvalidProbe("missing required frame stage measurements")
    if callbacks:
        report = measurement.get("callbacks")
        if not isinstance(report, dict) or not isinstance(report.get("worker"), dict):
            raise InvalidProbe("missing native-worker callback report")
        worker = report["worker"]
        observer_frames = number(report.get("observer_frames"), "observer_frames", positive=True, integer=True)
        if number(worker.get("editor_frames"), "editor_frames", positive=True, integer=True) != observer_frames:
            raise InvalidProbe("worker and observer frame counts differ")
        number(worker.get("latest_edits_published"), "latest_edits_published", positive=True, integer=True)
        if number(worker.get("host_frames"), "host_frames", integer=True) != host_frames:
            raise InvalidProbe("worker host frames do not match request")
        if number(worker.get("edits_delivered"), "edits_delivered", positive=True, integer=True) != min(120, max(4, edits)):
            raise InvalidProbe("worker edits do not match request")
        number(report.get("changed_gpu_frames"), "changed_gpu_frames", positive=not cpu, integer=True)
        for key in ("live_publication_ms", "callback_published_render_ms"):
            timings = report.get(key)
            if not isinstance(timings, dict):
                raise InvalidProbe(f"missing {key}")
            for statistic in ("mean", "p99"):
                number(timings.get(statistic), f"{key}.{statistic}")


def admit(output, returncode, case, *, cpu=False, callbacks=False, frames=24, edits=24, host_frames=128):
    """Keep process failures distinct from successful but unproven probes."""
    try:
        measurement = parse(output)
    except ValueError as error:
        return {"stages": [], "probe_error": f"invalid probe output: {error}"}, "failed" if returncode else "invalid_probe"
    if returncode:
        return measurement, "failed"
    try:
        if not one_test_passed(output):
            raise InvalidProbe("requested test did not run and pass exactly once")
        if measurement.get("status") == "skipped":
            if not measurement.get("reason") or number(measurement.get("program"), "skipped program") != case["program"]:
                raise InvalidProbe("missing or mismatched skip evidence")
            return measurement, "skipped"
        if "status" in measurement:
            raise InvalidProbe("unexpected probe status")
        validate_measurement(measurement, case, cpu, callbacks, frames, edits, host_frames)
    except InvalidProbe as error:
        measurement["probe_error"] = str(error)
        return measurement, "invalid_probe"
    return measurement, "passed"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--library", action="append", default=[])
    parser.add_argument("--mode", choices=MODES, action="append")
    parser.add_argument("--device-scale", type=float, choices=[1, 1.5, 2], action="append")
    parser.add_argument("--view-scale", type=float, choices=[0, .5, 1, 2], action="append", help="0 fits the panel")
    parser.add_argument("--appearance", type=int, choices=[0, 1, 2], action="append", help="Plain=0, Color=1, Artwork=2")
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
    selected = list(cases(plan, args.library, args.mode or list(MODES), args.device_scale or [1, 2], args.view_scale or [0], args.appearance or [0]))
    if args.dry_run:
        print(json.dumps({"cases": len(selected), "libraries": sorted({c["library"] for c in selected}),
                          "programs": len({(c["patch"], c["snapshot"], c["program"]) for c in selected})}, indent=2))
        return
    binary = str(args.binary.resolve(strict=True))
    try:
        discover(binary, args.timeout)
    except (InvalidProbe, OSError, subprocess.TimeoutExpired) as error:
        parser.exit(1, f"UI probe preflight failed: {error}\n")
    # Never overwrite prior evidence, nor write into a caller's ordinary folder.
    args.output.mkdir(parents=True, exist_ok=False)
    records = []
    for index, case in enumerate(selected):
        name = f"{index:03d}"
        env = os.environ.copy()
        env.update(KONTRA_UI_BENCH_PATCH=case["patch"], KONTRA_UI_BENCH_PROGRAM=str(case["program"]),
                   KONTRA_UI_BENCH_MODE=str(MODES[case["mode"]]),
                   KONTRA_UI_BENCH_DEVICE_SCALE=str(case["device_scale"]),
                   KONTRA_UI_BENCH_VIEW_SCALE=str(case["view_scale"]), KONTRA_UI_BENCH_APPEARANCE=str(case["appearance"]),
                   KONTRA_UI_BENCH_LIBRARY_ROOT=case["library_root"], KONTRA_UI_BENCH_LIBRARY_NAME=case["library"],
                   KONTRA_UI_BENCH_FRAMES=str(args.frames), KONTRA_UI_BENCH_EDITS=str(args.edits),
                   KONTRA_UI_BENCH_HOST_FRAMES=str(args.host_frames), RUST_MIN_STACK="16777216")
        # Every case has isolated preferences; the user's live editor is untouched.
        env["XDG_CONFIG_HOME"] = str((args.output / f"{name}-config").resolve())
        for key in ["KONTRA_UI_BENCH_GPU", "KONTRA_UI_BENCH_CALLBACKS", "KONTRA_UI_BENCH_SHOT", "KONTRA_UI_BENCH_CONTROL_VARIABLE", "KONTRA_UI_BENCH_SNAPSHOT", "KONTRA_UI_BENCH_PAGE"]:
            env.pop(key, None)
        if case["snapshot"]:
            env["KONTRA_UI_BENCH_SNAPSHOT"] = case["snapshot"]
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
            measurement, status = admit(output, completed.returncode, case, cpu=args.cpu, callbacks=args.callbacks,
                                        frames=args.frames, edits=args.edits, host_frames=args.host_frames)
        except subprocess.TimeoutExpired as error:
            output = (error.stdout or b"") + (error.stderr or b"")
            if isinstance(output, bytes):
                output = output.decode(errors="replace")
            status = "timeout"
            measurement = {"stages": []}
        (args.output / f"{name}.log").write_text(output)
        record = {**case, **measurement, "status": status, "wall_seconds": time.monotonic() - start}
        records.append(record)
        (args.output / "results.json").write_text(json.dumps({"scope": "headless UI; one selected control, not all controls or notes",
            "binary": binary, "records": records}, indent=2))
        print(f"{index + 1}/{len(selected)} {case['library']} program {case['program']} {case['mode']} {case['device_scale']}x: {status}", flush=True)
    failed = sum(r["status"] in ("failed", "timeout", "invalid_probe") for r in records)
    raise SystemExit(bool(failed))


if __name__ == "__main__":
    main()
