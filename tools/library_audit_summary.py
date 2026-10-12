#!/usr/bin/env python3
"""Summarize scanner telemetry without treating missing evidence as zero or parity.

Example:
  python3 tools/library_audit_summary.py --scan fresh=/tmp/scan/results.tsv \
    --scan historic-v1=$HOME/.cache/kontakto-survey/out-v1/results.tsv \
    --inventory /tmp/all-items.tsv --inventory-status unknown --out /tmp/evidence

Inputs and outputs contain only paths, fixed status categories, numeric counts,
MIDI audition plans and artifact digests. No authored script/resource values are
copied. The helper never executes an engine or modifies a scan/cache directory.
"""
import argparse
from collections import Counter, defaultdict
import csv
import hashlib
import json
from pathlib import Path
import re


UNKNOWN = "schema_unknown"
STATUS_FIELDS = ("loads", "ui", "plays_note", "audition_status", "family_match")
DROP_FIELDS = ("fx_slots_dropped", "filter_slots_dropped", "mod_slots_dropped")
SHA256 = re.compile(r"[0-9a-f]{64}\Z")


def digest(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def library(item):
    path = item.split("::", 1)[0]
    for marker in ("/Libraries/Kontakt/", "/Libraries/UVI/"):
        if marker in path:
            return path.split(marker, 1)[1].split("/", 1)[0]
    return Path(path).parent.name


def item_kind(item):
    if "::" in item:
        return "uvi-program"
    return {".nki": "kontakt-instrument", ".nkm": "kontakt-multi",
            ".ufs": "uvi-bank-unenumerated", ".uvip": "uvi-loose-program"}.get(
                Path(item).suffix.lower(), "unknown")


def load_inventory(path):
    if path is None:
        return {}
    result = defaultdict(set)
    for line in path.read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        item = line.split("\t", 1)[-1]
        result[library(item)].add(item)
    return dict(result)


def parse_json(value):
    if not isinstance(value, str):
        return value
    try:
        return json.loads(value)
    except (TypeError, ValueError):
        return None


def drop_counts(value):
    value = parse_json(value)
    if not isinstance(value, dict) or any(
            type(value.get(key)) is not int or value[key] < 0
            for key in ("enabled", "bypassed")):
        return None
    return {key: value[key] for key in ("enabled", "bypassed")}


def read_scan(path):
    with path.open(newline="") as stream:
        reader = csv.DictReader(stream, delimiter="\t")
        if not reader.fieldnames or "path" not in reader.fieldnames:
            raise ValueError(f"scanner TSV lacks path column: {path}")
        rows = list(reader)
    if any(not row.get("path") for row in rows):
        raise ValueError(f"scanner TSV contains an empty path: {path}")
    if len({row["path"] for row in rows}) != len(rows):
        raise ValueError(f"scanner TSV contains duplicate item IDs: {path}")
    cache_by_path = defaultdict(list)
    for cached in sorted((path.parent / "cache").glob("*.json")):
        record = json.loads(cached.read_text())
        if isinstance(record, dict) and record.get("path"):
            cache_by_path[record["path"]].append((cached, record))
    evidence = {}
    for row in rows:
        # A resumable output can retain cache entries from multiple revisions.
        # Never silently choose an arbitrary cache record for one TSV row.
        matches = [(p, r) for p, r in cache_by_path.get(row["path"], [])
                   if all(str(r.get(key, "")) == row.get(key, "")
                          for key in ("loads", "ui", "plays_note", "audition_status"))]
        if len(matches) == 1:
            p, record = matches[0]
            evidence[row["path"]] = {
                "record": record, "cache_path": str(p), "cache_sha256": digest(p),
                "cache_status": "unique-matching-record"}
        else:
            evidence[row["path"]] = {
                "record": {}, "cache_status": "ambiguous-cache-records" if matches else "cache-record-absent"}
    return rows, evidence


def pair_eligibility(left, right, left_record, right_record):
    """An identical, witnessed single audition permits telemetry comparison only."""
    reasons = []
    for label, row, record in (("left", left, left_record), ("right", right, right_record)):
        if row.get("audition_status") != "matched-note-plan":
            reasons.append(label + ":audition-not-matched")
        if row.get("fallback_note") != "0":
            reasons.append(label + ":fallback-or-unknown")
        picks = parse_json(row.get("note_picked"))
        if not isinstance(picks, dict) or not picks or any(
                not isinstance(value, list) or len(value) != 2 or any(
                    type(x) is not int or not 0 <= x <= 127 for x in value)
                for value in picks.values()):
            reasons.append(label + ":missing-or-partial-program-picks")
        origins = parse_json(row.get("pick_source"))
        if not isinstance(origins, dict) or not origins or any(
                value not in ("native_declared", "zone_coverage") for value in origins.values()):
            reasons.append(label + ":selection-origin-unknown-or-fallback")
        programs = record.get("programs")
        if not isinstance(programs, list) or not programs:
            reasons.append(label + ":program-evidence-absent")
        elif any(program.get("loaded") is not True for program in programs):
            reasons.append(label + ":program-admission-not-complete")
    if parse_json(left.get("note_picked")) != parse_json(right.get("note_picked")):
        reasons.append("different-note-plans")
    if parse_json(left.get("keyswitch_picked")) != parse_json(right.get("keyswitch_picked")):
        reasons.append("different-keyswitch-plans")
    for field in ("held_key", "switch", "keyswitch"):
        lp = [p.get(field) for p in left_record.get("programs", [])]
        rp = [p.get(field) for p in right_record.get("programs", [])]
        if lp != rp:
            reasons.append("different-" + field + "-plans")
    return sorted(set(reasons))


def summarize(name, path, inventory):
    rows, cache = read_scan(path)
    grouped = defaultdict(list)
    for row in rows:
        grouped[row.get("library") or library(row["path"])].append(row)
    libraries = {}
    for name_ in sorted(set(inventory) | set(grouped)):
        entries = grouped[name_]
        seen = {row["path"] for row in entries}
        expected = inventory.get(name_, set())
        coverage = "enumeration-unknown"
        if expected:
            coverage = "partial" if expected - seen else "all-manifest-items-observed"
        drops = {}
        for field in DROP_FIELDS:
            counts = [drop_counts(row.get(field)) for row in entries]
            valid = [value for value in counts if value is not None]
            drops[field] = {
                "known_items": len(valid), "unknown_items": len(entries) - len(valid),
                "schema_unknown_items": sum(field not in row or row[field] == "" for row in entries),
                "enabled": sum(value["enabled"] for value in valid),
                "bypassed": sum(value["bypassed"] for value in valid),
                "totals_complete": bool(entries) and len(valid) == len(entries)}
        programs = [p for row in entries for p in cache[row["path"]]["record"].get("programs", [])]
        libraries[name_] = {
            "inventory_items": len(expected) if expected else None, "observed_items": len(entries),
            "inventory_kinds": dict(sorted(Counter(item_kind(item) for item in expected).items())),
            "coverage": coverage, "missing_items": sorted(expected - seen),
            "outside_inventory_items": sorted(seen - expected) if inventory else [],
            "statuses": {field: dict(sorted(Counter(
                row.get(field) if row.get(field) not in (None, "") else UNKNOWN for row in entries).items()))
                for field in STATUS_FIELDS},
            "programs_observed": len(programs),
            "programs_loaded": sum(p.get("loaded") is True for p in programs),
            "programs_without_audition_pick": sum("pick" in p and p["pick"] is None for p in programs),
            "dropped_slots": drops,
            "cache_statuses": dict(sorted(Counter(cache[row["path"]]["cache_status"] for row in entries).items()))}
    revisions = Counter()
    for row in rows:
        revision = cache[row["path"]]["record"].get("revision")
        revisions[revision if isinstance(revision, str) and SHA256.fullmatch(revision) else UNKNOWN] += 1
    result = {
        "name": name, "input_path": str(path), "input_sha256": digest(path),
        "observed_items": len(rows), "libraries": libraries,
        "engine_sha256_counts": dict(sorted(revisions.items())),
        "scope": "Load/UI/single audition/static family telemetry; native full-patch parity unverified",
        "timing_comparison": "uncontrolled-and-unmatched-host-state; no performance winner inferred"}
    return result, {row["path"]: row for row in rows}, cache


def compact(mapping):
    return "; ".join(f"{key}={value}" for key, value in mapping.items()) or "no observations"


def summarize_corpus(name, path, inventory):
    records, started = {}, set()
    with path.open() as stream:
        for line_number, line in enumerate(stream, 1):
            if not line.strip():
                continue
            try:
                record = json.loads(line)
            except ValueError as error:
                raise ValueError(f"invalid corpus JSONL line {line_number}: {path}") from error
            if not isinstance(record, dict) or not record.get("id"):
                raise ValueError(f"corpus record lacks item ID at line {line_number}: {path}")
            if record.get("status") == "started":
                started.add(record["id"])
            else:
                records[record["id"]] = record
    grouped = defaultdict(list)
    for record in records.values():
        grouped[library(record["id"])].append(record)
    libraries = {}
    for name_ in sorted(set(inventory) | set(grouped)):
        entries = grouped[name_]
        seen = {record["id"] for record in entries}
        expected = inventory.get(name_, set())
        libraries[name_] = {
            "inventory_items": len(expected) if expected else None, "observed_items": len(entries),
            "coverage": "partial" if expected - seen else "all-manifest-items-observed" if expected else "enumeration-unknown",
            "missing_items": sorted(expected - seen),
            "statuses": dict(sorted(Counter(record.get("status", UNKNOWN) for record in entries).items())),
            "tiers": dict(sorted(Counter(record.get("tier", UNKNOWN) for record in entries).items())),
            "stages": dict(sorted(Counter(record.get("stage", UNKNOWN) for record in entries).items())),
            "load_kinds": dict(sorted(Counter(record.get("load", {}).get("kind") or UNKNOWN for record in entries).items())),
            "load_ok": sum(record.get("load", {}).get("ok") is True for record in entries),
            "load_failed": sum(record.get("load", {}).get("ok") is False for record in entries),
            "load_unknown": sum(type(record.get("load", {}).get("ok")) is not bool for record in entries),
            "audio_witnessed": sum(isinstance(record.get("sound"), dict) for record in entries),
            "multis_observed": sum(isinstance(record.get("multi"), dict) for record in entries),
            "embedded_programs_reported": sum(record.get("multi", {}).get("programs", 0) for record in entries),
            "embedded_programs_checked": sum(record.get("multi", {}).get("checked", 0) for record in entries),
            "multis_with_partial_program_checks": sum(
                isinstance(record.get("multi"), dict) and
                record["multi"].get("checked", 0) < record["multi"].get("programs", 0) for record in entries)}
    return {
        "name": name, "input_path": str(path), "input_sha256": digest(path),
        "finished_items": len(records), "started_without_finished_items": sorted(started - set(records)),
        "libraries": libraries,
        "scope": "Parse-tier metadata translation has no PCM audition, authored UI, live host, or native parity proof. Parse multis check at most the first embedded program; NoZone can represent a controller/empty program. Interpret other tiers separately."}


def markdown(report):
    lines = ["# Library scanner evidence", "", report["scope"], "",
             "Missing fields remain `schema_unknown`. Drop totals cover witnessed rows only. "
             "All manifest items observed does not prove complete installed-bank enumeration. "
             "One audible multi program does not certify every embedded program. "
             "A family MATCH checks source selection/cursor/loops/cycle only, not native audio parity.", "",
             f"Inventory enumeration: `{report['inventory_status']}`.", ""]
    for scan in report["scans"]:
        lines += [f"## {scan['name']}", "", f"Observed items: {scan['observed_items']}. "
                  f"Engine digest counts: `{compact(scan['engine_sha256_counts'])}`.", "",
                  "| Library | Items observed / manifest | Coverage | Load | UI | Audition audio | Note plan | Family |",
                  "|---|---:|---|---|---|---|---|---|"]
        for name, info in scan["libraries"].items():
            cells = [name, f"{info['observed_items']} / {info['inventory_items'] if info['inventory_items'] is not None else '?'}",
                     info["coverage"]] + [compact(info["statuses"][field]) for field in STATUS_FIELDS]
            lines.append("| " + " | ".join(str(cell).replace("|", "/") for cell in cells) + " |")
        lines += ["", "| Library | FX dropped enabled / bypassed | Filters dropped enabled / bypassed | Mods dropped enabled / bypassed | Unknown rows FX / filters / mods |",
                  "|---|---:|---:|---:|---|"]
        for name, info in scan["libraries"].items():
            drops = [info["dropped_slots"][field] for field in DROP_FIELDS]
            cells = [name] + [f"{value['enabled']} / {value['bypassed']}" if value["known_items"] else "unknown"
                             for value in drops] + [" / ".join(str(value["unknown_items"]) for value in drops)]
            lines.append("| " + " | ".join(cells) + " |")
        lines.append("")
    for corpus in report["corpora"]:
        lines += [f"## {corpus['name']}", "", corpus["scope"], "",
                  f"Finished items: {corpus['finished_items']}; pending starts: {len(corpus['started_without_finished_items'])}.", "",
                  "| Library | Items observed / manifest | Coverage | Tier | Stage | Load OK / failed / unknown | Audio records |",
                  "|---|---:|---|---|---|---:|---:|"]
        for name, info in corpus["libraries"].items():
            cells = [name, f"{info['observed_items']} / {info['inventory_items'] if info['inventory_items'] is not None else '?'}",
                     info["coverage"], compact(info["tiers"]), compact(info["stages"]),
                     f"{info['load_ok']} / {info['load_failed']} / {info['load_unknown']}", str(info["audio_witnessed"])]
            lines.append("| " + " | ".join(str(cell).replace("|", "/") for cell in cells) + " |")
        multis = [info for info in corpus["libraries"].values() if info["multis_observed"]]
        if multis:
            lines += ["", "| Library | Embedded programs checked / reported | Multis with partial checks |",
                      "|---|---:|---:|"]
            for name, info in corpus["libraries"].items():
                if info["multis_observed"]:
                    lines.append(f"| {name.replace('|', '/')} | {info['embedded_programs_checked']} / {info['embedded_programs_reported']} | {info['multis_with_partial_program_checks']} |")
        lines.append("")
    for pair in report["comparisons"]:
        lines += [f"## {pair['left']} / {pair['right']}", "",
                  f"Shared items: {pair['shared_items']}; same-note telemetry eligible: {pair['same_note_telemetry_eligible']}. "
                  "Eligibility is not a native parity verdict. Timings are not calibrated.", "",
                  "Excluded reasons: " + compact(pair["excluded_reason_counts"]) + ".", ""]
    return "\n".join(lines)


def assignment(value):
    if "=" not in value:
        raise argparse.ArgumentTypeError("expected NAME=PATH")
    name, path = value.split("=", 1)
    if not name or not path:
        raise argparse.ArgumentTypeError("expected nonempty NAME=PATH")
    return name, Path(path).expanduser().resolve()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scan", type=assignment, action="append", default=[])
    parser.add_argument("--corpus", type=assignment, action="append", default=[])
    parser.add_argument("--inventory", type=Path)
    parser.add_argument("--inventory-status", default="unknown", choices=("unknown", "partial", "complete-installed-program-manifest"))
    parser.add_argument("--artifact", type=assignment, action="append", default=[])
    parser.add_argument("--out", type=Path, required=True, help="output prefix for .json and .md")
    args = parser.parse_args()
    if not args.scan and not args.corpus:
        parser.error("at least one --scan or --corpus is required")
    if len({name for name, _ in args.scan}) != len(args.scan):
        parser.error("scan names must be unique")
    inventory = load_inventory(args.inventory)
    scans = [summarize(name, path, inventory) for name, path in args.scan]
    report = {
        "schema": "library-audit-telemetry-v1",
        "scope": "Installed owner-local scanner telemetry; no samples/scripts/resources exported; native Kontakt/Falcon full-patch parity remains unverified.",
        "inventory_path": str(args.inventory) if args.inventory else None,
        "inventory_sha256": digest(args.inventory) if args.inventory else None,
        "inventory_status": args.inventory_status,
        "artifacts": [{"name": name, "path": str(path), "sha256": digest(path)} for name, path in args.artifact],
        "scans": [summary for summary, _, _ in scans],
        "corpora": [summarize_corpus(name, path, inventory) for name, path in args.corpus], "comparisons": []}
    for i, (left, left_rows, left_cache) in enumerate(scans):
        for right, right_rows, right_cache in scans[i + 1:]:
            shared = sorted(set(left_rows) & set(right_rows))
            exclusions = {}
            for item in shared:
                reasons = pair_eligibility(left_rows[item], right_rows[item], left_cache[item]["record"], right_cache[item]["record"])
                if reasons:
                    exclusions[item] = reasons
            report["comparisons"].append({
                "left": left["name"], "right": right["name"], "shared_items": len(shared),
                "same_note_telemetry_eligible": len(shared) - len(exclusions),
                "excluded_reason_counts": dict(sorted(Counter(reason for reasons in exclusions.values() for reason in reasons).items())),
                "excluded_items": exclusions, "native_audio_parity": "unverified"})
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.with_suffix(".json").write_text(json.dumps(report, indent=2) + "\n")
    args.out.with_suffix(".md").write_text(markdown(report) + "\n")
    print(f"Wrote {args.out.with_suffix('.json')} and {args.out.with_suffix('.md')}")


if __name__ == "__main__":
    main()
