#!/usr/bin/env python3
"""Source/model checks only. Does not compile or execute Rust or KSP."""
import hashlib
import itertools
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
BASE = "13c044d6f9f80576507521a2dc6521e9aad7d9c5"
CORE = "crates/sampler-core/src/script_params.rs"


def pinned(path):
    return subprocess.check_output(["git", "show", f"{BASE}:{path}"], cwd=ROOT).decode()


def read(path):
    return (ROOT / path).read_text()


def replace_once(text, old, new):
    assert text.count(old) == 1, old
    return text.replace(old, new)


expected = pinned(CORE)
for old, new in [
    ("    /// and velocity. An event that ended or never sounded reads 0.",
     "    /// and velocity. Unknown or retired aliases read 0; a live event with no\n"
     "    /// eligible voices reports ZoneId = -1."),
    ("            crate::EventInfo::ZoneId => {\n                let mut family",
     "            crate::EventInfo::ZoneId => {\n                let mut highest = -1;\n                let mut family"),
    ("return Ok(i64::from(state.source_zone));", "highest = highest.max(i64::from(state.source_zone));"),
    ("                    }\n                }\n                0\n            }\n        })\n    }\n\n    pub(crate) fn read_mod_value",
     "                    }\n                }\n                highest\n            }\n        })\n    }\n\n    pub(crate) fn read_mod_value"),
]:
    expected = replace_once(expected, old, new)
assert read(CORE) == expected, "Production changes escaped the narrow ZoneId span"

# Static/dynamic producers and plan-qualified consumers must remain byte-identical.
unchanged = [
    "crates/sampler-ksp/src/builtins.rs", "crates/sampler-ksp/src/lower.rs",
    "crates/sampler-ksp/src/lib.rs", "crates/sampler-core/src/behavior.rs",
    "crates/sampler-core/src/note_event.rs", "crates/sampler-core/src/groups.rs",
    "crates/sampler-core/src/prepare/selection.rs", "crates/sampler-core/src/ownership.rs",
    "crates/sampler-core/src/steal.rs", "crates/sampler-core/src/render.rs",
]
for path in unchanged:
    assert read(path) == pinned(path), path
assert "pub const ZONE_ID: i32 = 10;" in read(unchanged[0])
assert "10 => super::EventInfo::ZoneId," in read("crates/sampler-core/src/behavior.rs")
assert "Some(b::event_par::ZONE_ID) => sampler_core::EventInfo::ZoneId," in read(unchanged[1])

path = "crates/sampler-ksp/tests/selection_contract.rs"
expected = replace_once(pinned(path),
    "    let expected = [123, 0, 0, 0, 1000, 0, 0, 64, 99, 0, 0, -1, 0, 0, 0, 0, 0];",
    "    // Selector 10 is ZoneId: the callback has not forwarded any voices yet.\n"
    "    let expected = [123, 0, 0, 0, 1000, 0, 0, 64, 99, 0, -1, -1, 0, 0, 0, 0, 0];")
assert read(path) == expected
path = "crates/sampler-ksp/tests/strings.rs"
expected = replace_once(pinned(path),
    "/// `get_event_par` reports a sounding event's zone as nonzero and an ignored\n"
    "/// one as 0, and the event's MIDI channel (Solo clears finished events by it).",
    "/// Historical narrow witness: a sounding zone is nonzero, an unknown alias\n"
    "/// reads 0, and MIDI channel reads back. No live-no-zone or highest-zone proof.")
assert read(path) == expected, "Historical executable assertions must stay unchanged"

# The core integration target keeps every existing test byte-identical and appends
# one witness for i64 operands that cannot be represented as signed32 event IDs.
path = "crates/sampler-core/tests/event_ids.rs"
core_before = pinned(path)
core_after = read(path)
assert core_after.startswith(core_before), "Existing core event-ID tests changed"
core_addition = core_after[len(core_before):]
assert re.findall(r"#\[test\]\s*fn (\w+)", core_addition) == [
    "event_zone_instructions_reject_out_of_range_i64_aliases_and_read_live_alias"
]
assert "i64::from(i32::MAX) + 1" in core_addition
assert "i64::from(i32::MIN) - 1" in core_addition
assert core_addition.count("Instruction::ReadEventInfo") == 3
assert core_addition.count("Instruction::ReadEventParameter") == 3

path = "crates/sampler-ksp/tests/event_zone.rs"
ksp_source = read(path)
assert "fn wrapped_negative_literal_is_an_invalid_alias_and_selectors_keep_zero_fallback()" in ksp_source
assert "$wrapped_negative := get_event_par(2147483648,$parameter)" in ksp_source

# Independent bounded model: nested family traversal, excluding stolen voices.
def query(families, valid=True):
    if not valid:
        return 0
    highest = -1
    for family in families:
        for physical_zone, stolen in family:
            if not stolen:
                highest = max(highest, physical_zone)
    return highest


cases = 0
for order in itertools.permutations([(9001, False), (73, False), (501, False)]):
    assert query([order]) == 9001
    cases += 1
for families, valid, result in [
    ([], True, -1), ([[]], True, -1), ([[(9001, True)]], True, -1),
    ([[(73, False)], [(9001, False)]], True, 9001),
    ([[(9001, True), (73, False)]], True, 73),
    ([[(4294967295, False)]], True, 4294967295),
    ([[(9001, False)]], False, 0),
]:
    assert query(families, valid) == result
    cases += 1
witnesses = re.findall(r"#\[test\]\s*fn (\w+)", read("crates/sampler-ksp/tests/event_zone.rs"))
assert len(witnesses) == 7
assert read("crates/sampler-ksp/tests/event_zone.rs").count("support::without_heap(||") == 7
# APIs used by authored witnesses must exist in the pinned source. This is not typechecking.
for path, methods in {
    "crates/sampler-core/src/ownership.rs": ["note_families", "flush_ended"],
    "crates/sampler-core/src/script.rs": ["script_cell"],
    "crates/sampler-core/src/controller_event.rs": ["dispatch_controller"],
    "crates/sampler-core/src/steal.rs": ["set_voice_stealing", "stolen_voices"],
}.items():
    for method in methods:
        assert f"pub fn {method}(" in read(path), method
print(json.dumps({
    "base": BASE, "checks": "PASS: exact narrow diff, producer/alias/lifecycle pins, 13 model cases, witness inventory",
    "model_cases": cases, "authored_witnesses": witnesses,
    "core_boundary_witness": "event_zone_instructions_reject_out_of_range_i64_aliases_and_read_live_alias",
    "rust": "NOT_RUN/NOT_COMPILED", "native": "UNKNOWN",
    "cpu_and_ram_vs_both_references": "UNACHIEVED",
    "core_sha256": hashlib.sha256(read(CORE).encode()).hexdigest(),
}, indent=2))
