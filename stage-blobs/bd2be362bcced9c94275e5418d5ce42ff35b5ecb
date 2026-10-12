#!/usr/bin/env python3
"""Source-selector/model audit only. Never compiles or executes Rust/KSP."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

BASE = "2a243bcfa2262abb789265aa0951fd2a85854023"
UI = "crates/sampler-ksp/src/ui.rs"


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def git(*args):
    return subprocess.check_output(["git", *args])


def selector_model(source, cases):
    """Read only the actual scalar/index assignment selectors, not expected data.

    This small independent model does not implement the Rust decoder, request
    replay, ownership, or parser. It detects operand reversal, not runtime parity.
    """
    source = source[source.index("fn waveform("):]
    legacy = "w.cursor_us = i64::from(int(" in source
    if legacy:
        value_at = int(re.search(r"w.cursor_us = i64::from\(int\((\d)\)\)", source)[1])
        index_at = int(re.search(r"usize::try_from\(int\((\d)\)\)", source)[1])
        assert f"w.table[index] = int({value_at})" in source
    else:
        assert "Value::Text(property), Value::Int(index), Value::Int(value)" in source
        assert "w.cursor_us = i64::from(*value)" in source
        assert "w.flags = *value as u32" in source
        assert "usize::try_from(*index)" in source
        assert "w.table[index] = *value" in source
        assert "(*value).clamp(0, 127)" in source
        assert "u32::try_from(*index).ok()" in source
        value_at, index_at = 3, 2
    results = []
    for case in cases:
        args = [None, case["property"], case["index"], case["value"]]
        value, index = args[value_at], args[index_at]
        prop = case["property"]
        if prop == "TABLE_VAL":
            actual = {"index": index, "value": value} if 0 <= index < 65536 else None
        elif prop == "TABLE_IDX_HIGHLIGHT":
            # Both baseline and corrected code use operand 3 for the slice.
            actual = args[2] if args[2] >= 0 else None
        elif prop == "MIDI_DRAG_START_NOTE":
            actual = min(127, max(0, value))
        else:
            actual = value
        results.append({**case, "actual": actual, "matches": actual == case["expected"]})
    return results


def pin(sha, path, start, end):
    raw = git("show", f"{sha}:{path}")
    lines = raw.splitlines(keepends=True)
    assert 1 <= start <= end <= len(lines), (path, start, end, len(lines))
    return {
        "sha": sha, "path": path, "start_line": start, "end_line": end,
        "blob": git("rev-parse", f"{sha}:{path}").decode().strip(),
        "file_sha256": digest(raw),
        "span_sha256": digest(b"".join(lines[start - 1:end])),
        "inspection": "Immutable source bytes/full inclusive LF span ONLY; not execution",
    }


def main():
    args = argparse.ArgumentParser()
    args.add_argument("--handoff", type=Path, required=True)
    options = args.parse_args()
    req = options.handoff / "round11-zone-ui-consumer-requirements.json"
    original = json.loads(req.read_bytes())
    vendor = []
    for entry in original["requirements"]["waveform"]["authority"]:
        if "zone-commands" in entry["url"]:
            continue  # Out of scope: this lane does not implement zones.
        archive = Path(entry["archive"])
        if not archive.is_absolute():
            archive = options.handoff / "official-docs" / archive.name
        excerpt = Path(entry["owned_excerpt_file"])
        assert digest(archive.read_bytes()) == entry["document_sha256"], archive
        assert digest(excerpt.read_bytes()) == entry["excerpt_sha256"], excerpt
        assert excerpt.read_text() == entry["excerpt"], excerpt
        vendor.append({k: entry[k] for k in [
            "url", "section", "document_sha256", "excerpt_sha256", "archive",
            "owned_excerpt_file", "authority", "applicability",
        ]})
    data = Path("docs/evidence/waveform-operands-cases.json")
    cases = json.loads(data.read_bytes())["cases"]
    baseline = selector_model(git("show", f"{BASE}:{UI}").decode(), cases)
    current = selector_model(Path(UI).read_text(), cases)
    assert any(not item["matches"] for item in baseline), "No red-capable baseline case"
    assert all(item["matches"] for item in current), current
    assert "SetUiWfProperty \"set_ui_wf_property\" [V I I I]" in Path("crates/sampler-ksp/src/builtins.rs").read_text()
    head = git("rev-parse", "HEAD").decode().strip()
    for path in [UI, "crates/sampler-ksp/tests/ui.rs", "crates/sampler-ksp/tests/waveform.rs", "crates/sampler-ksp/tests/ui_callbacks.rs"]:
        assert Path(path).read_bytes() == git("show", f"{head}:{path}"), ("Uncommitted source", path)
    # Source pins cover the real request -> effect -> UI consumer and the
    # existing instance-owned store/seeding seam for the next coordinated lane.
    spans = [
        (UI, 715, 782),
        ("crates/sampler-ksp/src/eval.rs", 897, 920),
        ("crates/sampler-ksp/src/eval.rs", 1665, 1690),
        ("crates/sampler-ksp/src/builtins.rs", 257, 269),
        ("crates/sampler-ksp/src/lower.rs", 1351, 1365),
        ("crates/sampler-ksp/src/lower.rs", 1404, 1493),
        ("crates/sampler-ksp/src/lower.rs", 2730, 2775),
        ("crates/sampler-ksp/src/lower.rs", 3684, 3734),
        ("crates/sampler-ksp/src/lib.rs", 296, 518),
        ("crates/sampler-ksp/src/lib.rs", 1368, 1455),
        ("crates/sampler-core/src/ops.rs", 572, 639),
        ("crates/sampler-core/src/ops.rs", 1530, 1541),
        ("crates/sampler-ui-ir/src/lib.rs", 300, 310),
        ("src/plugin.rs", 666, 787),
        ("src/plugin.rs", 1727, 1795),
        ("src/sound/waveform.rs", 26, 95),
        ("src/sound/mod.rs", 259, 275),
        ("src/ui/ir_view.rs", 1610, 1616),
        ("src/ui/render_art.rs", 14, 85),
    ]
    baseline_pin = pin(BASE, UI, 715, 778)
    frozen_pin = pin("366515c96b583747d1bd77c881b1f6909e86cf0d", UI, 715, 778)
    assert baseline_pin["span_sha256"] == frozen_pin["span_sha256"]
    pins = [pin(head, *span) for span in spans]
    for path in ["crates/sampler-ksp/tests/ui.rs", "crates/sampler-ksp/tests/waveform.rs", "crates/sampler-ksp/tests/ui_callbacks.rs"]:
        raw = git("show", f"{head}:{path}")
        pins.append(pin(head, path, 1, len(raw.splitlines())))
    print(json.dumps({
        "kind": "SOURCE_SELECTOR_MODEL_AND_DIGEST_CHECKS_ONLY",
        "baseline": BASE, "code_sha": head,
        "same_frozen_operand_defect_source_pins": [baseline_pin, frozen_pin],
        "baseline_selector_cases": baseline, "corrected_selector_cases": current,
        "input_case_sha256": digest(data.read_bytes()),
        "root_requirement_json_sha256": digest(req.read_bytes()),
        "root_requirement_md_sha256": digest((options.handoff / "round11-zone-ui-consumer-requirements.md").read_bytes()),
        "official_cached_spec_verified": vendor,
        "immutable_source_pins": pins,
        "rust_tests": "NOT_RUN", "runtime_state": "UNIMPLEMENTED",
        "native_runtime": "UNKNOWN", "native_receipts": [],
        "cpu_and_ram_below_both_references": "UNACHIEVED",
    }, indent=2))


if __name__ == "__main__":
    main()
