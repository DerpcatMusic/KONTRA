#!/usr/bin/env python3
"""Source/custody and independent numeric checks; never runs Rust, Lua or a host."""
import hashlib
import html
import json
import math
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BASE = "bac3c616824cf26e2a888c09cb21433b727d9079"
SOURCE = "10c29088a85776f50f395d5d58ec4ba87d3eb465"
CORE = "crates/sampler-core/src/engine_parameters.rs"
HOST = "crates/sampler-uvi/tests/host_parameters.rs"


def git_file(commit, path):
    return subprocess.check_output(["git", "show", f"{commit}:{path}"], cwd=ROOT)


def digest(data):
    return hashlib.sha256(data).hexdigest()


checks = []
old, new = (git_file(commit, CORE).decode() for commit in (BASE, SOURCE))
# Production diff is only the two decode arms. No admission, type, queue or graph edit.
start = "            Self::Exponential { low, high } =>"
end = "            Self::AhdsrCurve =>"
old_decode = old.split("    pub fn decode(", 1)[1].split("    pub fn encode(", 1)[0]
new_decode = new.split("    pub fn decode(", 1)[1].split("    pub fn encode(", 1)[0]
assert old_decode.split(start, 1)[0] == new_decode.split(start, 1)[0]
assert old_decode.split(end, 1)[1] == new_decode.split(end, 1)[1]
assert old.split("    pub fn decode(", 1)[0] == new.split("    pub fn decode(", 1)[0]
assert old.split("    pub fn encode(", 1)[1].split("#[cfg(test)]", 1)[0] == new.split("    pub fn encode(", 1)[1].split("#[cfg(test)]", 1)[0]
assert "(low.ln() + (high.ln() - low.ln()) * v / 1e6).exp()" in new_decode
assert "Self::Exponential { low, high }.decode(value).max(low) - offset" in new_decode
checks.append({"check": "all_other_laws_and_strict_admission_unchanged", "status": "PASS_SOURCE_ONLY"})
for path in [CORE, HOST]:
    assert git_file(SOURCE, path) == (ROOT / path).read_bytes(), path
for path in ["crates/sampler-uvi/src/script.rs", "crates/sampler-uvi/src/script_prelude.lua",
             "crates/sampler-uvi/src/engine_parameters.rs", "crates/sampler-uvi/tests/fixture.rs"]:
    assert git_file(BASE, path) == git_file(SOURCE, path), path
old_host, new_host = (git_file(commit, HOST).decode() for commit in (BASE, SOURCE))
marker = "#[test]\nfn unknown_parameter_names_and_ids_are_silently_ignored_on_cold_and_cached_elements()"
assert old_host.split(marker, 1)[1] == new_host.split(marker, 1)[1]
checks.append({"check": "passing_host_neighbors_pcm_fixture_and_uvi_production_sources_unchanged", "status": "PASS_SOURCE_ONLY"})

# This models the pinned arithmetic, not a compiled Rust or ScriptHost execution.
def before(low, high, normalized):
    v = float(max(0, min(1_000_000, normalized)))
    return math.exp(math.log(low) + (math.log(high) - math.log(low)) * v / 1e6)


def after(low, high, normalized):
    v = float(max(0, min(1_000_000, normalized)))
    if v == 0:
        return low
    if v == 1e6:
        return high
    return before(low, high, normalized)


counterexample = before(20., 20_000., 1_000_000)
assert counterexample < 20_000.
checks.append({"check": "old_physical_max_vs_decoded_bound", "declared_max": 20_000.,
               "decoded_max": counterexample, "strict_admission_rejects_declared_max": True,
               "classification": "INDEPENDENT_PYTHON_ARITHMETIC_NOT_RUST_EXECUTION"})
for low, high, offset in [(20., 20_000., 0.), (20., 20., 0.),
                          (2., 15_002., 2.), (2., 25_002., 2.)]:
    bounds = [low - offset, high - offset]
    for n, expected in [(0, bounds[0]), (1_000_000, bounds[1]),
                        (-2_147_483_648, bounds[0]), (2_147_483_647, bounds[1])]:
        assert max(after(low, high, n), low) - offset == expected
    for n in [1, 123456, 500000, 999999]:
        assert max(before(low, high, n), low) - offset == max(after(low, high, n), low) - offset
    for value in [math.nextafter(bounds[0], -math.inf), math.nextafter(bounds[1], math.inf),
                  math.nan, math.inf, -math.inf]:
        assert not (math.isfinite(value) and bounds[0] <= value <= bounds[1])
checks.append({"check": "intended_exact_endpoints_exterior_neighbors_and_unchanged_interior_arithmetic",
               "status": "PASS_INDEPENDENT_MODEL_ONLY_NOT_RUST_TEST"})

spec = json.loads((ROOT / "audits/uvi-setter-consistency-next/vendor-spec.json").read_text())
for document in [spec["existing_archives_rehashed"][0],
                 {"file": spec["new_elements_archive"]["archive"], "sha256": spec["new_elements_archive"]["sha256"]}]:
    assert digest(Path(document["file"]).read_bytes()) == document["sha256"]
    checks.append({"check": "immutable_official_spec_rehash", **document})

elements = Path(spec["new_elements_archive"]["archive"]).read_text()
section = elements.split('id="OnePole"', 1)[1].split("</table>", 1)[0]
rows = [[html.unescape(re.sub(r"<[^>]+>", "", cell)).strip()
         for cell in re.findall(r"<td>(.*?)</td>", row, re.S)]
        for row in re.findall(r"<tr>(.*?)</tr>", section, re.S)]
freq = next(row for row in rows if row and row[0] == "Freq")
assert freq == ["Freq", "Freq", "float", "20.000", "20000.000", "1000.000", "Hz", "yes", "Filter cutoff frequency"]
checks.append({"check": "exact_official_OnePole_Freq_row", "row": freq,
               "authority": "REQUIREMENTS_EQUIVALENT_ONLY_NATIVE_EXECUTION_UNKNOWN"})

receipts = Path("/mnt/Windows11/DEV_WORKSPACE/kontra-runs/pi-integration-round5")
for name in ["uvi-host-parameters", "uvi-script-unit-neighbors",
             "uvi-fixture-initialized_controller_and_widget_cutoff_writes_change_production_pcm",
             "uvi-fixture-streamed_bus_gain_has_live_catalog_defaults_and_controller_readback",
             "uvi-fixture-set_parameter_reaches_the_runtime"]:
    record = json.loads((receipts / (name + ".json")).read_text())
    assert record["source_sha"] == BASE
    assert digest(Path(record["log"]).read_bytes()) == record["log_sha256"]
    checks.append({"check": "prior_receipt_rehash_not_new_execution", "run": name,
                   "exit_code": record["exit_code"], "log_sha256": record["log_sha256"]})
for args in [["git", "diff", "--check", BASE, SOURCE],
             ["rustfmt", "--edition", "2024", "--check", CORE, HOST]]:
    result = subprocess.run(args, cwd=ROOT, capture_output=True, text=True)
    assert result.returncode == 0, result.stderr
    checks.append({"check": "source_format_or_whitespace_only", "command": args, "exit_code": result.returncode})

print(json.dumps({"base_commit": BASE, "source_commit": SOURCE,
                  "classification": "SOURCE_AND_INDEPENDENT_ARITHMETIC_ONLY",
                  "rust_tests": "NOT_RUN", "native_execution": "UNKNOWN",
                  "performance_acceptance": "UNACHIEVED", "checks": checks}, indent=2))
