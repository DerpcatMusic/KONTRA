#!/usr/bin/env python3
"""Static/model checks only. Does not execute Rust or a native host."""
import hashlib
import json
import math
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
BASE = "9075da681fbca5019640619822a4540bfd2a57a1"
SOURCE = "63a05d3745ef0ab4b5f8a1ee51feeebbe4590d8f"
HANDOFF = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(
    "/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff"
)

def git(*args):
    return subprocess.check_output(["git", "-C", str(ROOT), *args], text=True)


def source(path, sha=SOURCE):
    return git("show", f"{sha}:{path}")


def span(text, start, end):
    return text.split(start, 1)[1].split(end, 1)[0]


render_path = "crates/sampler-core/src/render.rs"
old = span(source(render_path, BASE), "pub(super) fn ramp_mix(", "pub(super) fn render_source")
new = span(source(render_path), "pub(super) fn ramp_mix(", "pub(super) fn render_source")
assert "script_fade" not in old
assert "fade.at(now + i as u64 + 1)" in new
assert new.count(" * gain;") == 2
old_loop = re.search(r"    for \(i, \(out, frame\)\).*?\n    }", old, re.S)[0]
legacy = span(new, "    } else {\n", "\n    }\n}")
assert legacy == "\n".join("    " + line for line in old_loop.splitlines())
assert "ramp_mix(segment, target, ramp, at);" in source(render_path)
parallel = source("crates/sampler-core/src/parallel.rs")
assert "super::render::ramp_mix(&scratch[0][..frames], target, ramp, at);" in parallel
assert "rt.source_event_id(note)" in parallel
assert "rt.resolve_source_event(plan, event), Ok(Some(note))" in parallel
assert "assert_ne!(expected, linear" in parallel
assert "fade_curve_parallel_plain_mix_matches_serial_and_differs_from_linear" in parallel
behavior = source("crates/sampler-core/src/behavior.rs")
checked = span(behavior, "            Instruction::FadeEvent {\n                event,\n                frames,\n                out,\n                stop,\n                curve,\n            } => {", "            Instruction::WriteControl")
assert checked.index("FadeCurve::from_index(index).ok_or(Error::InvalidInput)?") < checked.index("self.fade_event(")
failed = span(behavior, "    fn fail_behavior(", "#[cfg(test)]")
assert "BehaviorOwner::Note(note)" in failed and "ReleaseCause::BehaviorFault" in failed
for path in ["crates/sampler-core/tests/fade_curves.rs", "crates/sampler-core/src/script_params.rs"]:
    assert "vec![[0.5; 2]; 512].into_boxed_slice()" in source(path)
params = source("crates/sampler-ksp/tests/params.rs")
for witness in ["assert_ne!(audio[15], unfaded[15])", "for selector in [-1, 5, 99]", "rt.now(), origin + 256", "rt.note(faulty), Ok((60, 1., false))", "rt.note(unaffected), Ok((61, 1., true))"]:
    assert witness in params
assert git("diff", BASE, SOURCE, "--", "crates/sampler-ksp/src/lower.rs", "crates/sampler-core/src/behavior.rs", "crates/sampler-core/src/voice_mod.rs", "crates/sampler-core/src/dsp.rs") == ""

html = HANDOFF / "official-docs/ksp-events.html"
assert hashlib.sha256(html.read_bytes()).hexdigest() == "57f8f69b4deec41cc0eb98fd1cc5196f3c02b72e3d222faff7b91fe8551626ce"
text = (HANDOFF / "official-docs/ksp-events.txt").read_text()
for term in ["introduced in Kontakt 8.12", "time-mirror", "sin(πt/2)", "½(1 − cos πt)", "t²", "1 − (1 − t)²"]:
    assert term in text
shapes = {
    "Linear": lambda t: t,
    "EqualPower": lambda t: math.sin(math.pi * t / 2),
    "SCurve": lambda t: (1 - math.cos(math.pi * t)) / 2,
    "Exponential": lambda t: t * t,
    "Logarithmic": lambda t: 1 - (1 - t) ** 2,
}
fixture = source("crates/sampler-core/tests/fade_curves.rs")
for name, shape in shapes.items():
    for out in [False, True]:
        pattern = rf"\(FadeCurve::{name}, {str(out).lower()}\) => ([\d.]+)"
        constant = float(re.search(pattern, fixture)[1])
        expected = shape(23 / 24 if out else 1 / 24)
        assert abs(constant - expected) < 1e-15
# Independent normalized-clock model: deliberately not Rust replay/native PCM.
model = []
for origin in [0, 128]:
    for name, shape in shapes.items():
        for out in [False, True]:
            by_block = []
            for block in [1, 17, 128]:
                audio = []
                for start in range(0, 385, block):
                    for index in range(start, min(start + block, 385)):
                        elapsed = origin + index + 1 - origin
                        t = min(elapsed / 384, 1)
                        audio.append(0.5 * shape(1 - t if out else t))
                by_block.append(audio)
            assert by_block[0] == by_block[1] == by_block[2]
            assert by_block[0][383] == (0 if out else 0.5)
            model.append({"origin": origin, "curve": name, "out": out, "frame16": by_block[0][15]})
expected_inside = 0.5 * (16 / 384) ** 2
endpoint_interpolation = 0.5 * (64 / 384) ** 2 * (16 / 64)
assert abs(expected_inside - 0.5) > 1e-7
assert abs(expected_inside - endpoint_interpolation) > 1e-7
print(json.dumps({
    "status": "PASS_STATIC_AND_INDEPENDENT_MODEL_ONLY",
    "source_sha": SOURCE, "base_sha": BASE,
    "source_tree": git("rev-parse", f"{SOURCE}^{{tree}}").strip(),
    "legacy_loop": "exact source preserved under else",
    "original_consumer_prediction": {"quarter_equal_power_in_actual_from_log": 0.5, "inside_exponential_in_actual_from_log": 0.5, "reason": "no script_fade read in frozen plain mixer"},
    "independent_discriminant": {"frame16": expected_inside, "cell_endpoint_interpolation": endpoint_interpolation},
    "model_cases": model,
    "rust": "UNRUN; integration sole validation owner",
    "native_runtime": "UNKNOWN; immutable official specification equivalent check only",
    "cpu_ram_acceptance": "UNACHIEVED"
}, indent=2))
