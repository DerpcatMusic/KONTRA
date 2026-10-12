#!/usr/bin/env python3
"""Fail-closed, source-free coverage summary for corpus-health inspect-list JSONL.

The manifest is the denominator. This tool never opens library files and never
promotes source inspection to product runtime or native equivalence evidence.
"""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import sys

KINDS = {"kontakt", "kontakt-multi", "uvi-loose", "uvi-program"}
REPORT_KINDS = {
    "kontakt": "kontakt",
    "kontakt-multi": "kontakt-multi",
    "uvi-loose": "uvi-loose",
    "uvi-program": "uvi-bank",
}
STAGE_STATUSES = {"ok", "partial", "failed", "skipped"}
INSPECTION_STATUSES = {"ok", "partial", "failed"}
INITIALIZED_FAULT_CODES = {
    "initialized_identity", "duplicate_initialized_id", "unexpected_initialized_id",
    "initialized_schema", "initialized_stages", "initialized_evidence",
}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def identity(kind: str, item_id: str) -> tuple[str, str, str | None]:
    if kind not in KINDS or not item_id:
        raise ValueError("unknown or empty manifest identity")
    if kind == "uvi-program":
        bank, sep, member = item_id.partition("::")
        if not sep or not bank.lower().endswith(".ufs") or not member:
            raise ValueError("UVI program identity must be BANK.ufs::MEMBER")
        return kind, bank, member
    if kind == "kontakt-multi" and "#" in item_id:
        path, index = item_id.rsplit("#", 1)
        if not path.lower().endswith(".nkm") or not index.isdecimal():
            raise ValueError("selected multi identity must be PATH.nkm#INDEX")
        return kind, path, str(int(index))
    expected = {"kontakt": ".nki", "kontakt-multi": ".nkm", "uvi-loose": ".uvip"}[kind]
    if not item_id.lower().endswith(expected):
        raise ValueError(f"{kind} identity must end in {expected}")
    return kind, item_id, None


def report_identity(report: dict) -> tuple[str, str, str | None]:
    item = report.get("item")
    if not isinstance(item, dict):
        raise ValueError("report item is missing")
    report_kind = item.get("kind")
    kind = {v: k for k, v in REPORT_KINDS.items()}.get(report_kind)
    path = item.get("path")
    program = item.get("program")
    if not isinstance(path, str) or (program is not None and not isinstance(program, str)):
        raise ValueError("report path/program has invalid type")
    if kind == "uvi-program":
        if program is None:
            raise ValueError("UVI bank report has no member")
        return identity(kind, f"{path}::{program}")
    if kind == "kontakt-multi" and program is not None:
        return identity(kind, f"{path}#{program}")
    if kind is None or program is not None:
        raise ValueError("report kind/program is inconsistent")
    return identity(kind, path)


def initialized_identity(report: dict) -> tuple[tuple[str, str, str | None], str | None]:
    """Map a selected Kontakt program to its physical manifest file identity."""
    item = report.get("item")
    if not isinstance(item, dict):
        raise ValueError("initialized report item missing")
    kind, path, program = item.get("kind"), item.get("path"), item.get("program")
    if not isinstance(path, str) or (program is not None and not isinstance(program, (str, int))):
        raise ValueError("initialized report path/program invalid")
    if kind in ("kontakt", "kontakt-multi"):
        if program is None or not str(program).isdecimal():
            raise ValueError("initialized Kontakt report needs selected program index")
        return identity(kind, path), str(program)
    if kind == "uvi-bank":
        if not isinstance(program, str):
            raise ValueError("initialized UVI bank report needs member")
        return identity("uvi-program", f"{path}::{program}"), program
    if kind == "uvi-loose" and program is None:
        return identity("uvi-loose", path), None
    raise ValueError("initialized report kind/program inconsistent")


def library(key: tuple[str, str, str | None]) -> str:
    parts = Path(key[1]).parts
    try:
        pos = parts.index("Libraries")
        family, name = parts[pos + 1 : pos + 3]
    except (ValueError, IndexError) as error:
        raise ValueError("identity is outside Libraries/FAMILY/LIBRARY") from error
    if family not in ("Kontakt", "UVI") or not name:
        raise ValueError("identity has unknown library family")
    if key[0].startswith("kontakt") != (family == "Kontakt"):
        raise ValueError("identity kind and library family disagree")
    return f"{family}/{name}"


def fault(faults: list[dict], code: str, detail: str, key=None) -> None:
    value = {"code": code, "detail": detail}
    if key is not None:
        value["identity_sha256"] = digest(repr(key).encode())
    faults.append(value)


def count_int(value, label: str) -> int:
    if type(value) is not int or value < 0:
        raise ValueError(f"{label} must be a nonnegative integer")
    return value


def validate_report(report: dict, key, faults: list[dict]) -> dict:
    """Return explicit evidence flags. Unknown/malformed values never count as success."""
    result = {"source_inspected": False, "source_semantics_complete": False,
              "ir_converted": False, "ui_projected": False,
              "kontakt_all_programs": False, "script_count": 0,
              "construct_count": 0, "script_status_counts": Counter(),
              "compiler_status_counts": Counter(),
              "runtime": "unknown", "audio": "unknown", "native_equivalence": "unknown"}
    if (report.get("format") != "kontra-library-inspector" or
            type(report.get("schema")) is not int or report["schema"] != 1):
        fault(faults, "schema", "unsupported report format/schema", key)
        return result
    if report.get("status") not in INSPECTION_STATUSES:
        fault(faults, "status", "unknown inspection status", key)
        return result
    mode = report.get("mode")
    if (not isinstance(mode, dict) or mode.get("summary") is not True or
            mode.get("ephemeral_source") is not False or mode.get("initialize") is not False):
        fault(faults, "mode", "expected source-free compact report", key)
        return result
    stages = report.get("stages")
    if not isinstance(stages, list):
        fault(faults, "stages", "stage list missing", key)
        return result
    stage_map = {}
    for stage in stages:
        if not isinstance(stage, dict) or not isinstance(stage.get("name"), str) or stage.get("status") not in STAGE_STATUSES:
            fault(faults, "stages", "malformed or unknown stage", key)
            return result
        if stage["name"] in stage_map:
            fault(faults, "stages", "duplicate stage name", key)
            return result
        stage_map[stage["name"]] = stage["status"]
    scripts = report.get("scripts")
    if not isinstance(scripts, list):
        fault(faults, "scripts", "compact script list missing", key)
        return result
    script_ids = set()
    script_count = 0
    construct_count = 0
    script_status_counts = Counter()
    compiler_status_counts = Counter()
    for script in scripts:
        if not isinstance(script, dict) or not isinstance(script.get("id"), str):
            fault(faults, "scripts", "script identity missing", key)
            return result
        if script["id"] in script_ids:
            fault(faults, "scripts", "duplicate physical script identity", key)
            return result
        script_ids.add(script["id"])
        try:
            n = count_int(script.get("construct_count"), "construct_count")
            counts = script.get("status_counts")
            if not isinstance(counts, dict) or any(not isinstance(k, str) for k in counts):
                raise ValueError("script status_counts missing")
            counts = Counter({k: count_int(v, "status count") for k, v in counts.items()})
            if sum(counts.values()) != n:
                raise ValueError("construct_count and status_counts disagree")
        except ValueError as error:
            fault(faults, "scripts", str(error), key)
            return result
        script_count += 1
        construct_count += n
        script_status_counts.update(counts)
        if key[0].startswith("uvi"):
            compiler = script.get("compiler")
            if not isinstance(compiler, dict) or compiler.get("status") not in ("ok", "failed"):
                fault(faults, "compiler", "UVI script has no compiler-only syntax status", key)
                return result
            compiler_status_counts[compiler["status"]] += 1
    result["script_count"] = script_count
    result["construct_count"] = construct_count
    result["script_status_counts"] = script_status_counts
    result["compiler_status_counts"] = compiler_status_counts
    metadata = report.get("metadata")
    diagnostics = report.get("diagnostics")
    if not isinstance(metadata, dict) or not isinstance(diagnostics, list):
        fault(faults, "report", "metadata or diagnostics missing", key)
        return result
    source_ok = stage_map.get("container") == "ok" and stage_map.get("source_frontend") == "ok"
    source_ok &= not any(isinstance(d, dict) and d.get("stage") == "parse" for d in diagnostics)
    source_ok &= compiler_status_counts.get("failed", 0) == 0
    if key[0].startswith("kontakt") and "source_frontend" in stage_map:
        programs = metadata.get("programs")
        try:
            program_count = count_int(metadata.get("program_count"), "program_count")
            if program_count == 0 or not isinstance(programs, list) or len(programs) != program_count:
                raise ValueError("program_count/programs incomplete")
            indexes = [p.get("program") if isinstance(p, dict) else None for p in programs]
            if indexes != list(range(program_count)):
                raise ValueError("program indices are incomplete")
            if any(type(s.get("program")) is not int or s["program"] not in range(program_count) for s in scripts):
                raise ValueError("script program index is invalid")
            if key[0] == "kontakt-multi" and any(
                type(p.get("rack_slot")) is not int or type(p.get("program_within_slot")) is not int
                for p in programs
            ):
                raise ValueError("multi rack/program coordinates missing")
            result["kontakt_all_programs"] = source_ok
        except ValueError as error:
            fault(faults, "programs", str(error), key)
            source_ok = False
    elif key[0].startswith("uvi") and "source_frontend" in stage_map:
        if type(metadata.get("xml_bytes")) is not int or metadata["xml_bytes"] < 0:
            fault(faults, "metadata", "UVI XML byte count missing", key)
            source_ok = False
    result["source_inspected"] = bool(source_ok)
    result["source_semantics_complete"] = bool(source_ok and all(k == "Implemented" for k in result["script_status_counts"]) and not diagnostics)
    converted = report.get("converted")
    if not isinstance(converted, dict) or not isinstance(converted.get("status"), str):
        fault(faults, "converted", "conversion status missing", key)
    else:
        result["ir_converted"] = bool(key[0].startswith("uvi") and converted["status"] == "ok" and
                                      stage_map.get("translate") == "ok" and
                                      isinstance(converted.get("summary"), dict))
    ui = report.get("ui")
    if not isinstance(ui, dict) or not isinstance(ui.get("status"), str):
        fault(faults, "ui", "UI status missing", key)
    else:
        result["ui_projected"] = False
        if ui["status"] == "ok" or stage_map.get("initialize") == "ok":
            fault(faults, "mode", "static report claims initialized UI", key)
    evidence = report.get("evidence")
    if not isinstance(evidence, dict):
        fault(faults, "evidence", "runtime/audio/native evidence missing", key)
    else:
        for field in ("runtime", "audio", "native_equivalence"):
            value = evidence.get(field)
            if not isinstance(value, str):
                fault(faults, "evidence", f"{field} evidence missing", key)
            else:
                result[field] = value
        for field in ("runtime", "audio"):
            if stage_map.get(field) != "skipped" or result[field] != "not_run":
                fault(faults, "evidence", f"{field} is not a skipped/not_run inspector stage", key)
                result[field] = "unknown"
        if result["native_equivalence"] != "unknown":
            fault(faults, "evidence", "native equivalence must remain unknown", key)
            result["native_equivalence"] = "unknown"
    return result


def summarize(manifest_path: Path, report_paths: list[Path], initialized_paths: list[Path] | None = None) -> dict:
    faults: list[dict] = []
    expected: dict[tuple, str] = {}
    provenance = []
    manifest_bytes = manifest_path.read_bytes()
    provenance.append({"role": "manifest", "name": manifest_path.name, "sha256": digest(manifest_bytes), "bytes": len(manifest_bytes)})
    for line_number, line in enumerate(manifest_bytes.decode("utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            kind, item_id = line.split("\t", 1)
            key = identity(kind, item_id)
            lib = library(key)
            if key in expected:
                fault(faults, "duplicate_manifest_id", f"manifest line {line_number}", key)
            else:
                expected[key] = lib
        except ValueError as error:
            fault(faults, "manifest_identity", f"manifest line {line_number}: {error}")
    if not expected:
        fault(faults, "empty_manifest", "manifest contains no valid identities")
    observed: dict[tuple, dict] = {}
    unexpected = Counter()
    for path in report_paths:
        data = path.read_bytes()
        provenance.append({"role": "report_jsonl", "name": path.name, "sha256": digest(data), "bytes": len(data)})
        for line_number, line in enumerate(data.decode("utf-8").splitlines(), 1):
            if not line.strip():
                continue
            try:
                report = json.loads(line)
                if not isinstance(report, dict):
                    raise ValueError("report is not an object")
                key = report_identity(report)
                lib = library(key)
            except (ValueError, json.JSONDecodeError) as error:
                fault(faults, "report_identity", f"{path.name}:{line_number}: {error}")
                continue
            if key in observed:
                fault(faults, "duplicate_report_id", f"{path.name}:{line_number}", key)
                continue
            if key not in expected:
                unexpected[lib] += 1
                fault(faults, "unexpected_report_id", f"{path.name}:{line_number}", key)
                continue
            observed[key] = report
    initialized = {}
    for path in initialized_paths or []:
        data = path.read_bytes()
        provenance.append({"role": "initialized_report_jsonl", "name": path.name, "sha256": digest(data), "bytes": len(data)})
        for line_number, line in enumerate(data.decode("utf-8").splitlines(), 1):
            if not line.strip():
                continue
            try:
                report = json.loads(line)
                if not isinstance(report, dict):
                    raise ValueError("initialized report is not an object")
                key, selected = initialized_identity(report)
                library(key)
            except (ValueError, json.JSONDecodeError) as error:
                fault(faults, "initialized_identity", f"{path.name}:{line_number}: {error}")
                continue
            if (key, selected) in initialized:
                fault(faults, "duplicate_initialized_id", f"{path.name}:{line_number}", key)
            elif key not in expected:
                fault(faults, "unexpected_initialized_id", f"{path.name}:{line_number}", key)
            else:
                initialized[key, selected] = report
    groups = defaultdict(lambda: {"expected": 0, "observed": 0, "missing": 0,
                                  "kinds": Counter(), "inspection_status": Counter(),
                                  "source_inspected": 0, "source_semantics_complete": 0,
                                  "kontakt_all_programs": 0, "ir_converted": 0,
                                  "ui_projected": 0, "scripts": 0, "constructs": 0,
                                  "script_status_counts": Counter(), "compiler_status_counts": Counter(),
                                  "runtime": Counter(),
                                  "audio": Counter(), "native_equivalence": Counter(),
                                  "initialized_attempted": 0, "initialized_ir_ok": 0,
                                  "initialized_ui_ok": 0, "initialized_failed": 0})
    for key, lib in expected.items():
        group = groups[lib]
        group["expected"] += 1
        group["kinds"][key[0]] += 1
        report = observed.get(key)
        if report is None:
            group["missing"] += 1
            fault(faults, "missing_report_id", "manifest item has no report", key)
            continue
        group["observed"] += 1
        status = report.get("status")
        group["inspection_status"][status if isinstance(status, str) else "malformed"] += 1
        evidence = validate_report(report, key, faults)
        for field in ("source_inspected", "source_semantics_complete", "kontakt_all_programs", "ir_converted", "ui_projected"):
            group[field] += int(evidence[field])
        group["scripts"] += evidence["script_count"]
        group["constructs"] += evidence["construct_count"]
        group["script_status_counts"].update(evidence["script_status_counts"])
        group["compiler_status_counts"].update(evidence["compiler_status_counts"])
        for field in ("runtime", "audio", "native_equivalence"):
            group[field][evidence[field]] += 1
    for (key, selected), report in initialized.items():
        group = groups[expected[key]]
        group["initialized_attempted"] += 1
        mode, converted, ui, stages, evidence = (report.get(k) for k in
                                                 ("mode", "converted", "ui", "stages", "evidence"))
        if (report.get("format") != "kontra-library-inspector" or type(report.get("schema")) is not int or
                report["schema"] != 1 or
                report.get("status") not in INSPECTION_STATUSES or
                not isinstance(mode, dict) or mode.get("initialize") is not True or
                mode.get("ephemeral_source") is not False or not isinstance(stages, list) or
                not isinstance(converted, dict) or not isinstance(ui, dict) or
                not isinstance(evidence, dict)):
            fault(faults, "initialized_schema", "invalid or source-bearing initialized report", key)
            group["initialized_failed"] += 1
            continue
        if any(not isinstance(s, dict) or not isinstance(s.get("name"), str) or
               s.get("status") not in STAGE_STATUSES for s in stages):
            fault(faults, "initialized_stages", "malformed initialized stage", key)
            group["initialized_failed"] += 1
            continue
        stage_map = {s["name"]: s["status"] for s in stages}
        if len(stage_map) != len(stages):
            fault(faults, "initialized_stages", "duplicate or malformed initialized stages", key)
            group["initialized_failed"] += 1
            continue
        translation = "translate" if key[0].startswith("uvi") else "initialize"
        ir_ok = converted.get("status") == "ok" and stage_map.get(translation) == "ok" and isinstance(converted.get("summary"), dict)
        ui_ok = (ui.get("status") == "ok" and stage_map.get("initialize") == "ok" and
                 stage_map.get("ui_projection") == "ok" and
                 isinstance(ui.get("interfaces"), list))
        if ir_ok:
            group["initialized_ir_ok"] += 1
        if ui_ok:
            group["initialized_ui_ok"] += 1
        if report.get("status") == "failed" or not ir_ok or not ui_ok:
            group["initialized_failed"] += 1
        if (evidence.get("runtime") != "not_run" or evidence.get("audio") != "not_run" or
                evidence.get("native_equivalence") != "unknown" or
                stage_map.get("runtime") != "skipped" or stage_map.get("audio") != "skipped"):
            fault(faults, "initialized_evidence", "initialized report claims runtime/audio/native evidence", key)
    libraries = {name: {key: dict(value) if isinstance(value, Counter) else value
                        for key, value in group.items()}
                 for name, group in sorted(groups.items())}
    totals = {key: sum(group[key] for group in groups.values()) for key in
              ("expected", "observed", "missing", "source_inspected", "source_semantics_complete",
               "kontakt_all_programs", "ir_converted", "ui_projected", "scripts", "constructs",
               "initialized_attempted", "initialized_ir_ok", "initialized_ui_ok", "initialized_failed")}
    totals["unexpected"] = sum(unexpected.values())
    totals["libraries"] = len(libraries)
    totals["uvi_expected"] = sum(g["kinds"].get("uvi-program", 0) + g["kinds"].get("uvi-loose", 0)
                                  for g in groups.values())
    totals["script_status_counts"] = dict(sum((g["script_status_counts"] for g in groups.values()), Counter()))
    totals["compiler_status_counts"] = dict(sum((g["compiler_status_counts"] for g in groups.values()), Counter()))
    # A complete manifest report is only a complete static inspection. Even with
    # zero gaps, native parity cannot follow from these reports.
    admission = ("complete_static_catalog" if not faults and
                 totals["source_inspected"] == totals["expected"] and
                 totals["ir_converted"] == totals["uvi_expected"] else "incomplete")
    catalog_records_complete = (totals["expected"] > 0 and totals["observed"] == totals["expected"] and
                                totals["unexpected"] == 0 and
                                not any(f["code"] not in INITIALIZED_FAULT_CODES for f in faults))
    return {"format": "kontra-inspector-audit-summary", "schema": 1,
            "admission": admission, "catalog_records_complete": catalog_records_complete,
            "native_parity": "unknown",
            "provenance": provenance, "totals": totals, "libraries": libraries,
            "unexpected_by_library": dict(sorted(unexpected.items())),
            "fault_counts": dict(Counter(f["code"] for f in faults)), "faults": faults}


def markdown(summary: dict) -> str:
    t = summary["totals"]
    lines = ["# Offline inspector catalog evidence", "",
             f"Catalog record coverage/schema complete: **{str(summary['catalog_records_complete']).lower()}**. "
             f"Static conversion admission: **{summary['admission']}**. Native equivalence: **unknown**.", "",
             "Record completeness means each manifest ID has one structurally valid report; it does not require its compiler, source frontend, IR, or UI stages to succeed.",
             "Static conversion admission additionally requires complete static source inspection, every expected UVI XML-to-IR conversion, and no integrity faults.", "",
             f"Manifest items: {t['expected']}; observed: {t['observed']}; missing: {t['missing']}; unexpected: {t['unexpected']}.",
             f"Static source inspected: {t['source_inspected']}; source semantics without reported gaps: {t['source_semantics_complete']}; "
             f"Static IR converted: {t['ir_converted']}; static UI projected: {t['ui_projected']}.",
             f"UVI compiler-only syntax: {t['compiler_status_counts'].get('ok', 0)} ok, "
             f"{t['compiler_status_counts'].get('failed', 0)} failed. This is Luau syntax evidence, not native Falcon Lua behavior.",
             f"Separate initialized samples: {t['initialized_attempted']} attempted, {t['initialized_ir_ok']} IR, "
             f"{t['initialized_ui_ok']} UI, {t['initialized_failed']} incomplete.",
             "Runtime/audio/native comparison requires independent evidence and is not inferred here.", "",
             "| Library | Expected | Observed | Missing | Source inspected | Kontakt files: all programs | Static IR | Init IR | Init UI | Scripts | Constructs |",
             "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for name, group in summary["libraries"].items():
        safe = name.replace("|", "\\|")
        lines.append(f"| {safe} | {group['expected']} | {group['observed']} | {group['missing']} | "
                     f"{group['source_inspected']} | {group['kontakt_all_programs']} | {group['ir_converted']} | "
                     f"{group['initialized_ir_ok']} | {group['initialized_ui_ok']} | {group['scripts']} | {group['constructs']} |")
    lines.extend(["", "## Provenance", ""])
    for item in summary["provenance"]:
        lines.append(f"- {item['role']} `{item['name']}`: SHA-256 `{item['sha256']}` ({item['bytes']} bytes)")
    lines.extend(["", "## Integrity", ""])
    if summary["fault_counts"]:
        for code, count in sorted(summary["fault_counts"].items()):
            lines.append(f"- {code}: {count}")
    else:
        lines.append("No manifest, identity, or report integrity faults detected.")
    lines.append("")
    return "\n".join(lines)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("reports", nargs="+", type=Path, help="compact inspect-list JSONL shards")
    parser.add_argument("--initialized-report", action="append", default=[], type=Path,
                        help="separate source-free initialized sample JSONL (repeatable)")
    parser.add_argument("--json-out", type=Path, required=True)
    parser.add_argument("--markdown-out", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        summary = summarize(args.manifest, args.reports, args.initialized_report)
    except (OSError, UnicodeError) as error:
        parser.error(str(error))
    args.json_out.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    args.markdown_out.write_text(markdown(summary))
    print(f"records_complete={str(summary['catalog_records_complete']).lower()} "
          f"admission={summary['admission']}: {summary['totals']['observed']}/{summary['totals']['expected']} items; "
          f"{sum(summary['fault_counts'].values())} integrity faults")
    return 0 if summary["admission"] == "complete_static_catalog" else 1


if __name__ == "__main__":
    sys.exit(main())
