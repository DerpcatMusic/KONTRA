#!/usr/bin/env python3
"""Read-only syntax/integrity checks; does not compile or execute Rust/ScriptHost."""
import hashlib
import json
import re
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BASE = "366515c96b583747d1bd77c881b1f6909e86cf0d"
SOURCE = "0df29cdc63d66ea7ba6e98da64c81fae3e0201e0"

def run(*args):
    p = subprocess.run(args, cwd=ROOT, capture_output=True, text=True)
    return {"command": list(args), "status": p.returncode, "stdout": p.stdout, "stderr": p.stderr}

def git_file(sha, path):
    return subprocess.check_output(["git", "show", f"{sha}:{path}"], cwd=ROOT)

checks = []
for path in ["crates/sampler-uvi/src/script.rs", "crates/sampler-uvi/src/script_prelude.lua",
             "crates/sampler-uvi/tests/host_parameters.rs", "crates/sampler-uvi/tests/fixture.rs"]:
    assert git_file(SOURCE, path) == (ROOT / path).read_bytes()
    checks.append({"check": "source_commit_bytes", "file": path,
                   "sha256": hashlib.sha256((ROOT / path).read_bytes()).hexdigest()})
path = "crates/sampler-uvi/src/script/async_data.rs"
assert git_file(BASE, path) == git_file(SOURCE, path)
checks.append({"check": "async_data_unchanged_from_base", "sha256": hashlib.sha256(git_file(SOURCE, path)).hexdigest()})
old = git_file(BASE, "crates/sampler-uvi/src/script.rs").decode()
new = git_file(SOURCE, "crates/sampler-uvi/src/script.rs").decode()
# Everything outside the command helper and native setter is byte-identical.
command_start = "    fn command(&self, command: Command)"
command_end = "\npub struct ScriptHost"
setter_start = '        native.set(\n            "setParam",'
setter_end = '        native.set(\n            "param",'
def untouched(text):
    return (text.split(command_start)[0], text.split(command_end)[1].split(setter_start)[0],
            text.split(setter_end)[1])
assert untouched(old) == untouched(new)
assert old.count("s.command(") == 14
assert old.split(setter_end)[1].count("s.command(") == 12
checks.append({"check": "outside_two_rust_seams_byte_identical", "direct_callsites": 14,
               "unchanged_ignored_return_statements": 12, "async_data_install_unchanged": True})
for command in [("git", "diff", "--check", BASE, SOURCE),
                ("rustfmt", "--edition", "2024", "--check", "crates/sampler-uvi/tests/host_parameters.rs", "crates/sampler-uvi/tests/fixture.rs"),
                ("luac", "-p", "crates/sampler-uvi/src/script_prelude.lua")]:
    receipt = run(*command); assert receipt["status"] == 0, receipt; checks.append(receipt)
# Parse Rust without compiling or rewriting inherited unformatted source.
receipt = run("rustfmt", "--edition", "2024", "--emit", "stdout", "--config", "skip_children=true", "crates/sampler-uvi/src/script.rs")
assert receipt["status"] == 0, receipt
receipt["stdout"] = "OMITTED: formatted text only; source file was not rewritten"
checks.append(receipt)
names = [
    "unknown_parameter_names_and_ids_are_silently_ignored_on_cold_and_cached_elements",
    "unsupported_catalog_writes_leave_authored_readback_and_production_queue_unchanged",
    "accepted_parameters_and_rejected_types_share_the_production_queue_seam",
    "a_full_production_queue_does_not_accept_parameter_readback_or_insert_overrides",
    "retained_connection_and_string_parameters_have_typed_descriptor_identities",
    "initialized_controller_and_widget_cutoff_writes_change_production_pcm",
    "streamed_bus_gain_has_live_catalog_defaults_and_controller_readback",
]
for name in names:
    path = "crates/sampler-uvi/tests/fixture.rs" if name in names[-2:] else "crates/sampler-uvi/tests/host_parameters.rs"
    text = (ROOT / path).read_text().split(f"fn {name}()", 1)[1].split("\n#[test]", 1)[0]
    scripts = re.findall(r"<script>(.*?)</script>", text, re.S)
    assert len(scripts) == 1, name
    with tempfile.NamedTemporaryFile(suffix=".lua", mode="w") as f:
        f.write(scripts[0]); f.flush(); receipt = run("luac", "-p", f.name)
    assert receipt["status"] == 0, receipt
    checks.append({"check": "embedded_lua_syntax_only", "test": name, "status": 0,
                   "limitation": "System Lua parser only; production Luau and Rust test execution UNRUN"})
for verb, argument in [("grep", "setParam"), ("callers", "setParameter")]:
    receipt = run("graft", "--dir", str(ROOT / "graft"), verb, argument)
    assert receipt["status"] != 0 and "no graph" in receipt["stderr"] + receipt["stdout"], receipt
    checks.append(receipt)
print(json.dumps({"source_commit": SOURCE, "base_commit": BASE,
    "classification": "SOURCE_SYNTAX_AND_INTEGRITY_ONLY_NOT_RUST_TEST_PASS",
    "new_rust_tests": "UNRUN", "native_execution": "UNKNOWN", "checks": checks}, indent=2))
