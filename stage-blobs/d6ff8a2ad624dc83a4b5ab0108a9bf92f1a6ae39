#!/usr/bin/env python3
"""No-build source/archive/model assertions. Not Rust or native execution proof."""
import hashlib
import json
import pathlib
import re
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[3]
MANIFEST = json.loads(pathlib.Path(__file__).with_name("source-manifest.json").read_text())
EVIDENCE = pathlib.Path("/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff")


def git_blob(sha, path):
    return subprocess.check_output(["git", "show", f"{sha}:{path}"], cwd=ROOT)


def body(source, marker):
    start = source.index(marker)
    opening = source.index("{", start)
    end, depth = opening + 1, 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


for record in MANIFEST["files"]:
    data = git_blob(MANIFEST["source_sha"], record["path"])
    assert hashlib.sha256(data).hexdigest() == record["sha256"], record["path"]

path = "crates/sampler-core/src/voice_mod.rs"
old = git_blob(MANIFEST["base_sha"], path).decode()
new = git_blob(MANIFEST["source_sha"], path).decode()
markers = ["pub(crate) struct ModShape", "pub(crate) struct VoiceModState", "pub fn bytes_per_voice"]
for marker in markers:
    assert body(old, marker) == body(new, marker), marker
assert "pub controls: Vec<(usize, [crate::ControlId; 2])>" in new
assert "controls: Box<[(usize, [usize; 2])]>" in new

path = "crates/sampler-core/src/engine_parameters.rs"
old = git_blob(MANIFEST["base_sha"], path).decode()
new = git_blob(MANIFEST["source_sha"], path).decode()
assert body(old, "pub fn normalized_value") == body(new, "pub fn normalized_value")
assert body(new, "pub fn decode").replace("            Self::Switch => f64::from(value != 0),\n", "") == body(old, "pub fn decode")
# UVI's separate endpoint work is deliberately NOT included in this source SHA.

archives = {
    "kontakt-modulation.html": "ca330e3207a333061ebca4ee28b0545b5e93c223624ff69ca608c7a85d897c9d",
    "ksp-engine.html": "4a15e3b10a9da8993c90ddc7681fee9fd62a7eafc5f3685f47b0c5ab37d1d16d",
    "ksp-engine-commands.html": "df2fe7192f32df7c4a738c713663bafdfec3630c2821fd9afe1887c7d5ed006d",
}
for filename, digest in archives.items():
    assert hashlib.sha256((EVIDENCE / "official-docs" / filename).read_bytes()).hexdigest() == digest

# Independent scalar examples of the existing attenuation/pan arithmetic.
# These are test requirements, not outputs obtained by running the Rust renderer.
def gains(source, amount, pan, bypass=False):
    gain = 0.5 * (1 - (0 if bypass else amount) * (1 - source))
    position = 0.25 + (0 if bypass else source * pan)
    return [gain * (1 - max(position, 0)), gain * (1 + min(position, 0))]

assert gains(1, 0, 0.5) == [0.125, 0.5]
assert gains(1, 0, 0.25) == [0.25, 0.5]
assert gains(1, 0, 0.25, True) == [0.375, 0.5]
assert gains(0.5, 0.5, 0.5) == [0.1875, 0.375]
assert gains(0.5, 0, 0.5) == [0.25, 0.5]

source = git_blob(MANIFEST["source_sha"], "crates/sampler-kontakt/tests/production_modulation.rs").decode()
assert len(re.findall(r"^fn serialized_\w+\(", source, re.M)) == 10
assert len(MANIFEST["authored_tests"]) == 10
print(json.dumps({
    "status": "PASS_SOURCE_MODEL_ONLY",
    "source_sha": MANIFEST["source_sha"],
    "source_files_rehashed": len(MANIFEST["files"]),
    "public_requirements_archives_rehashed": archives,
    "unchanged_per_voice_source_declarations": markers,
    "analytic_pcm_requirements_checked": 5,
    "authored_rust_tests": 10,
    "rust_compile_and_runtime": "NOT_RUN",
    "native_ownership_and_live_mutation": "UNKNOWN",
    "cpu_ram_acceptance": "UNACHIEVED",
    "memory_limitation": "Source declarations only; no compiler layout, allocator, RSS or performance measurement."
}, indent=2))
