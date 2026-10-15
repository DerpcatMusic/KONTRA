#!/usr/bin/env python3
"""Bounded, private launcher for the explicit production preset-switch audit.

Only numeric AUDIT/head worker records are retained from stderr. Arbitrary
authored logs and image/sample payloads are never copied into the report.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import selectors
import subprocess
import time

OUTPUT_LIMIT = 16 << 20
LINE_LIMIT = 1 << 20
AUDIT_LIMIT = 4096


def file_hash(path):
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def finite_number(value):
    return type(value) in (int, float) and math.isfinite(value) and value >= 0


def valid_stage(name):
    return (isinstance(name, str) and 0 < len(name) <= 96
            and all(c in "abcdefghijklmnopqrstuvwxyz0123456789_" for c in name))


def collect_audit(lines):
    """Assign existing spans to explicit sequential action/phase delimiters."""
    step, phase = None, None
    stages = {}
    heads = {}
    omitted = 0
    malformed = 0
    seen = 0
    pending = {}
    begins_seen = begins_omitted = 0
    for line in lines:
        prefix, _, payload = line.partition(" ")
        if prefix not in ("PRESET_SWITCH", "AUDIT_BEGIN", "AUDIT", "head_prepare_audit"):
            continue
        try:
            record = json.loads(payload)
        except (ValueError, TypeError):
            malformed += 1
            continue
        if not isinstance(record, dict):
            malformed += 1
            continue
        if prefix == "PRESET_SWITCH":
            if type(record.get("step")) is not int or record["step"] < 0 or record.get("phase") not in (
                "begin", "prepare", "publish", "activation", "post_publish_held", "ui_projection_assets",
                "fresh_note", "retained_owner_audio", "ui_edit_drain", "end",
            ):
                malformed += 1
                continue
            step, phase = record["step"], record["phase"]
            if phase == "end":
                step, phase = None, None
            continue
        owner_step, owner_phase = step, phase
        if prefix == "AUDIT_BEGIN":
            begins_seen += 1
            if begins_seen > AUDIT_LIMIT:
                begins_omitted += 1
                continue
            name, span = record.get("stage"), record.get("span")
            if (owner_step is None or owner_phase is None or not valid_stage(name)
                    or type(span) is not int or not 0 < span < 1 << 64 or span in pending):
                malformed += 1
            else:
                pending[span] = (owner_step, owner_phase, name)
            continue
        seen += 1
        if seen > AUDIT_LIMIT:
            omitted += 1
            continue
        if prefix == "AUDIT" and record.get("span") is not None:
            span = record["span"]
            if type(span) is not int or span not in pending:
                malformed += 1
                continue
            owner_step, owner_phase, begin_name = pending.pop(span)
            if begin_name != record.get("stage"):
                malformed += 1
                continue
        if owner_step is None or owner_phase is None:
            malformed += 1
            continue
        if prefix == "AUDIT":
            name, ms = record.get("stage"), record.get("ms")
            # Stage labels are code-owned; reject unbounded or authored-looking
            # labels rather than retaining arbitrary stderr text.
            if not valid_stage(name) or not finite_number(ms):
                malformed += 1
                continue
            cpu = record.get("thread_cpu_ms")
            if cpu is not None and not finite_number(cpu):
                malformed += 1
                continue
            bucket = stages.setdefault(owner_step, {}).setdefault((owner_phase, name), {"calls": 0, "ms_sum": 0.0, "ms_max": 0.0,
                "thread_cpu_calls": 0, "thread_cpu_ms_sum": None, "thread_cpu_ms_max": None})
            wall_sum = bucket["ms_sum"] + ms
            cpu_sum = None if cpu is None else (bucket["thread_cpu_ms_sum"] or 0.0) + cpu
            if not finite_number(wall_sum) or (cpu_sum is not None and not finite_number(cpu_sum)):
                malformed += 1
                continue
            bucket["calls"] += 1
            bucket["ms_sum"] = wall_sum
            bucket["ms_max"] = max(bucket["ms_max"], ms)
            if cpu is not None:
                bucket["thread_cpu_calls"] += 1
                bucket["thread_cpu_ms_sum"] = cpu_sum
                bucket["thread_cpu_ms_max"] = max(bucket["thread_cpu_ms_max"] or 0.0, cpu)
        else:
            # Raw worker records include sample identifiers and per-job probes;
            # aggregate only their numeric counts, never their payloads.
            if any(type(record.get(key)) is not int or record[key] < 0 for key in ("jobs", "failures", "dropped_probes")):
                malformed += 1
                continue
            bucket = heads.setdefault(owner_step, {"workers": 0, "jobs": 0, "failures": 0, "dropped_probes": 0})
            bucket["workers"] += 1
            for key in ("jobs", "failures", "dropped_probes"):
                bucket[key] += record[key]
    open_rows = [{"span":span,"step":a,"phase":b,"stage":c} for span,(a,b,c) in sorted(pending.items())[:16]]
    return stages, heads, {"records_seen": seen, "records_omitted": omitted, "malformed_or_unassigned": malformed,
        "breadcrumb_records_seen":begins_seen,"breadcrumb_records_omitted":begins_omitted,
        "unfinished_spans":open_rows,"unfinished_spans_omitted":max(0,len(pending)-len(open_rows))}


def augment(report, stderr):
    stages, heads, coverage = collect_audit(stderr.splitlines())
    complete = (coverage["records_omitted"] == 0 and coverage["malformed_or_unassigned"] == 0
                and coverage["breadcrumb_records_omitted"] == 0 and not coverage["unfinished_spans"] and coverage["unfinished_spans_omitted"] == 0)
    for step in report.get("steps", []):
        buckets = stages.get(step["step"], {})
        step["loader_audit_spans"] = [dict(phase=phase, stage=stage, **counts) for (phase, stage), counts in sorted(buckets.items())]
        calls = lambda stage: sum(count["calls"] for (phase, name), count in buckets.items() if phase == "prepare" and name == stage)
        projection = step.get("ui_projection")
        step["work_counts"] = {
            "container_read_spans": calls("file_read_ni_container") if complete else None,
            "container_decode_spans": calls("ni_chunk_parse") if complete else None,
            "base_object_parse_spans": calls("ni_objects_parse") if complete else None,
            "script_initialization_spans": calls("translate_ksp_init") if complete else None,
            "uvi_script_initialization_spans": calls("uvi_lua_init") if complete else None,
            "sample_header_batches": calls("sample_headers_latency_probe") if complete else None,
            "sample_preload_batches": calls("sample_preload") if complete else None,
            "head_prepare_batches": calls("sample_head_prepare") if complete else None,
            "head_worker_jobs": heads.get(step["step"]) if complete else None,
            "ui_projection_calls": len(projection) if isinstance(projection, list) else None,
            "asset_decode_jobs": projection_count(projection, "decodes"),
            "asset_decode_successes": projection_count(projection, "decode_ok"),
            "persistence_callback_programs_observed": step.get("activation", {}).get("completed_callback_programs_observed", {}).get("persistence_programs"),
        }
        step["work_count_scope"] = "Observed span executions, including resolver/identity/cache work in prepare; no inferred per-file open counts. Begins assign cross-phase spans to their start phase; legacy records use the end phase. Nested wall/caller CPU overlap; CPU coverage counts are explicit. Off-CPU time includes scheduling and waits, not proved IO. Zero spans means no such span emitted, not zero total source work."
    report["audit_capture"] = dict(coverage, complete=complete, enabled="KONTRA_AUDIT_LOAD=1", limit=AUDIT_LIMIT)
    return report


def projection_count(projection, key):
    if not isinstance(projection, list):
        return None
    total = 0
    for row in projection:
        if not isinstance(row, dict) or not isinstance(row.get("asset_summary"), dict):
            return None
        count = row["asset_summary"].get(key)
        if type(count) is not int or count < 0:
            return None
        total += count
    return total


def invoke(command, env, timeout):
    """Drain both pipes, cap retained bytes, and kill/wait the child on timeout."""
    started = time.monotonic()
    child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    selector = selectors.DefaultSelector()
    buffers = {"stdout": bytearray(), "stderr": bytearray()}
    counts = {"stdout": 0, "stderr": 0}
    timed_out = False
    overflow = False
    try:
        for name, pipe in (("stdout", child.stdout), ("stderr", child.stderr)):
            selector.register(pipe, selectors.EVENT_READ, name)
        while selector.get_map():
            if time.monotonic() - started >= timeout:
                timed_out = True
                child.kill()
                break
            for key, _ in selector.select(min(0.1, max(0, timeout - (time.monotonic() - started)))):
                chunk = os.read(key.fileobj.fileno(), 65536)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                name = key.data
                counts[name] += len(chunk)
                keep = max(0, OUTPUT_LIMIT - len(buffers[name]))
                buffers[name].extend(chunk[:keep])
                if counts[name] > OUTPUT_LIMIT:
                    overflow = True
                    child.kill()
                    break
            if overflow:
                break
        child.wait(timeout=5)
    finally:
        if child.poll() is None:
            child.kill()
            child.wait()
        selector.close()
        child.stdout.close()
        child.stderr.close()
    return {"exit_code": child.returncode, "timed_out": timed_out, "output_limit_exceeded": overflow,
            "elapsed_ms": (time.monotonic() - started) * 1000, "captured_bytes": counts}, bytes(buffers["stdout"]), bytes(buffers["stderr"])


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plan", type=Path)
    parser.add_argument("--binary", type=Path, required=True, help="Existing corpus-health built with ui-render; never builds/installs a plugin")
    parser.add_argument("--output", type=Path, required=True, help="Private JSON evidence path; no raw stderr or PCM/artwork is saved")
    parser.add_argument("--timeout", type=float, default=180, help="Whole-process wall seconds, 1..1800")
    args = parser.parse_args(argv)
    if not math.isfinite(args.timeout) or not 1 <= args.timeout <= 1800:
        parser.error("--timeout must be 1..1800 finite seconds")
    binary = args.binary.resolve(strict=True)
    plan = args.plan.resolve(strict=True)
    output = args.output.resolve()
    if output in (binary, plan, Path(__file__).resolve()):
        parser.error("--output must not overwrite the binary, plan or launcher")
    before = file_hash(binary)
    env = dict(os.environ, KONTRA_AUDIT_LOAD="1", KONTRA_AUDIT_BEGIN="1", KONTRA_DISABLE_NETWORK="1")
    process, stdout, stderr = invoke([str(binary), "preset-switch", str(plan)], env, args.timeout)
    after = file_hash(binary)
    try:
        report = json.loads(stdout)
        if not isinstance(report, dict) or report.get("format") != "kontra-preset-switch-probe" or report.get("schema") != 1:
            raise ValueError("unexpected report")
        if any(len(line) > LINE_LIMIT for line in stderr.splitlines()):
            raise ValueError("audit line bound")
        report = augment(report, stderr.decode("utf-8", errors="replace"))
    except (ValueError, KeyError, TypeError):
        report = {"format": "kontra-preset-switch-launch", "schema": 1, "probe_report": "unavailable",
                  "scope": "Process failed, unsupported helper command, malformed or bounded-output failure; authored stderr omitted",
                  "native_equivalence": "unknown", "universal_compatibility": "not_established"}
    if report.get("probe_report") == "unavailable":
        _, _, coverage = collect_audit(stderr.decode("utf-8", errors="replace").splitlines())
        report["audit_capture"] = dict(coverage, complete=False, enabled="KONTRA_AUDIT_LOAD=1", limit=AUDIT_LIMIT)
    report["process"] = process
    report["provenance"] = {"binary_path": str(binary), "binary_sha256_before": before, "binary_sha256_after": after,
                            "binary_unchanged": before == after, "plan_sha256": file_hash(plan), "launcher_sha256": file_hash(__file__)}
    # A complete JSON write with a private mode; refuse existing evidence rather
    # than overwriting a prior run or a library file supplied by mistake.
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("x", encoding="utf-8") as stream:
        os.chmod(output, 0o600)
        json.dump(report, stream, indent=2, sort_keys=True)
        stream.write("\n")
    print(json.dumps({"evidence_path": str(output), "process_exit": process["exit_code"], "probe_report": report.get("probe_report", "available"),
                      "binary_sha256": before, "timed_out": process["timed_out"]}))
    return int(process["exit_code"] != 0 or process["timed_out"] or process["output_limit_exceeded"] or before != after or report.get("probe_report") == "unavailable")


if __name__ == "__main__":
    raise SystemExit(main())
