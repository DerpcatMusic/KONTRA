#!/usr/bin/env python3
"""Sequential actual-library playback audits from a private selection plan."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("plan", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--initial", action="store_true", help="Harp, large keyswitch ensemble, prepared piano first")
    parser.add_argument("--realtime", action="store_true")
    args = parser.parse_args()
    binary = args.binary.resolve()
    build = json.loads(subprocess.check_output([binary, "--build-info"], text=True))
    plan = json.loads(args.plan.read_text())
    args.output.mkdir(parents=True, exist_ok=True)
    rows = []
    selected = ["Vista - Harp.nki", "07 Areia - Full Ens - Core Techniques.nki", "Una Corda Pure.nki"]
    for library in plan["libraries"]:
        for patch in library["patches"]:
            if args.initial and Path(patch["path"]).name not in selected:
                continue
            for program in patch["initial_read_audit"]["programs"]:
                identity = f'{patch["path"]}:{program["program"]}:{args.realtime}'
                stem = hashlib.sha256(identity.encode()).hexdigest()[:12]
                report_path = args.output / f"{stem}.json"
                stderr_path = args.output / f"{stem}.stderr"
                result = None
                if report_path.exists():
                    try:
                        old = json.loads(report_path.read_text())
                    except json.JSONDecodeError:
                        old = {}
                    if old.get("build") == build:
                        result = old
                started = time.monotonic()
                status = 0
                if result is None:
                    command = ["nice", "-n", "10", str(binary), "playback-audit", patch["path"], str(program["program"])]
                    if args.realtime:
                        command.append("--realtime")
                    with report_path.open("w") as stdout, stderr_path.open("w") as stderr:
                        status = subprocess.run(command, stdout=stdout, stderr=stderr).returncode
                    if status == 0:
                        result = json.loads(report_path.read_text())
                issues = []
                observations = []
                if result:
                    for case in result["cases"]:
                        if case["nonfinite_samples"]:
                            issues.append(f'{case["case"]}: nonfinite audio')
                        if case["heap_operations_on_render_thread"]:
                            issues.append(f'{case["case"]}: {case["heap_operations_on_render_thread"]} heap operations')
                        if case["dropped_commands"]:
                            issues.append(f'{case["case"]}: dropped script commands')
                        for stage in case["stages"]:
                            if stage["phase"] == "panic-after-declick" and (stage["voices"] or any(stage["held_keys_by_engine_channel"]) or any(stage["pending_commands_writes_releases"])):
                                issues.append(f'{case["case"]}: pending voice/key/work after Panic')
                            if stage["phase"] in ("natural-release-observation", "post-panic-release"):
                                observations.append({"case":case["case"], "phase":stage["phase"],
                                                     "voices":stage["voices"], "unreleased":stage["unreleased_attack_voices"],
                                                     "held":sum(stage["held_keys_by_engine_channel"]),
                                                     "pending":stage["pending_commands_writes_releases"], "rms":stage["rms"]})
                row = {"library":library["name"], "patch":patch["path"], "program":program["program"],
                       "name":program["name"], "exit_status":status, "wall_seconds":time.monotonic()-started,
                       "report":str(report_path), "stderr":str(stderr_path), "issues":issues,
                       "tail_observations":observations}
                rows.append(row)
                summary = {"build":build, "realtime":args.realtime, "programs":rows,
                           "completed":sum(r["exit_status"] == 0 for r in rows)}
                (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
                print(f'{library["name"]} / {program["name"]}: exit={status}; {len(issues)} review issues; report={stem}', flush=True)
    return int(any(r["exit_status"] for r in rows))


if __name__ == "__main__":
    raise SystemExit(main())
