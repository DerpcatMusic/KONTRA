#!/usr/bin/env python3
"""Summarize selected local patches without retaining scripts, UI content or samples."""
import argparse
import json
from pathlib import Path
import subprocess
import time


def summarize(row):
    if "error" in row:
        return {"program": row.get("program"), "error": row["error"]}
    scripts = []
    for slot, script in enumerate(row.get("scripts", [])):
        init = script.get("initialization", {})
        requirements = script.get("requirements", {})
        scripts.append({
            "slot": script.get("slot", slot),
            "controls": init.get("controls", 0),
            "listeners": init.get("listeners", []),
            "initialization_error": init.get("error"),
            "diagnostics": init.get("diagnostics", []),
            "requirements": requirements,
            "opaque_constants": script.get("opaque_constants", []),
            "warnings": script.get("warnings", []),
        })
    return {
        "program": row.get("program", 0), "name": row.get("name"),
        "groups": row.get("groups", 0), "zones": row.get("zones", 0),
        "complete_groups": row.get("complete_groups", 0),
        "missing_sample_count": len(row.get("missing_samples", [])),
        "warnings": row.get("warnings", []), "scripts": scripts,
        "declared_control_count": sum(s["controls"] for s in scripts),
        "sample_payload_decode_verified": False,
        "gpu_draw_and_interaction_verified": False,
    }


def run(cli, patch):
    start = time.monotonic()
    try:
        result = subprocess.run([cli, "audit", patch["path"]], capture_output=True,
                                text=True, timeout=120)
        if result.returncode:
            return {"error": result.stderr[-4000:], "exit_status": result.returncode}
        rows = json.loads(result.stdout)
        if not isinstance(rows, list) or not rows:
            return {"error": "Selected file produced no audited programs"}
        return {"programs": [summarize(row) for row in rows],
                "import_and_ksp_audit_ms": (time.monotonic() - start) * 1000,
                "timing_scope": "Existing CLI audit; may reuse instrument cache; no Bank load"}
    except (subprocess.TimeoutExpired, json.JSONDecodeError) as error:
        return {"error": str(error)}


def self_test():
    row = {"program": 7, "groups": 2, "zones": 3, "missing_samples": ["missing"],
           "scripts": [{"initialization": {"controls": 4, "diagnostics": ["diagnostic"]},
                        "source": "authored source must not leak"}]}
    summary = summarize(row)
    assert summary["program"] == 7 and summary["missing_sample_count"] == 1
    assert summary["declared_control_count"] == 4
    assert "source" not in summary["scripts"][0]
    assert not summary["sample_payload_decode_verified"]
    assert summarize({"program": 3, "error": "failure"}) == {"program": 3, "error": "failure"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plan", nargs="?", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        print("selection summary check passed")
        return
    if args.plan is None:
        parser.error("a local selection plan is required")
    plan = json.loads(args.plan.read_text())
    for library in plan["libraries"]:
        for patch in library["patches"]:
            patch["initial_read_audit"] = run(plan["cli"], patch)
            rows = patch["initial_read_audit"].get("programs", [])
            errors = [row["error"] for row in rows if "error" in row]
            if "error" in patch["initial_read_audit"]:
                errors.append(patch["initial_read_audit"]["error"])
            patch["checks"]["import_reference_mapping"] = "failed" if errors else "passed"
            script_errors = [s["initialization_error"] for row in rows
                             for s in row.get("scripts", []) if s["initialization_error"]]
            patch["checks"]["ksp_initialization"] = "failed" if errors or script_errors else "passed"
            patch["warnings"] = sorted({warning for row in rows for warning in row.get("warnings", [])})
            args.plan.write_text(json.dumps(plan, indent=2))
            print(library["name"], Path(patch["path"]).name,
                  "programs", len(rows), "errors", len(errors),
                  "controls", sum(row.get("declared_control_count", 0) for row in rows), flush=True)


if __name__ == "__main__":
    main()
