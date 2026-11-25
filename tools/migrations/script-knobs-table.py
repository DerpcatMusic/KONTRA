#!/usr/bin/env python3
"""script-knobs-table.py: crates/format/kontakt/data/script_knobs.tsv -> Rust rows.

Rewrites `script_knobs()` in crates/format/kontakt/src/script_knobs.rs so its
rows are a typed Rust table (one line per `$ENGINE_PAR_*`, the compiler
checking every target and law), then deletes the TSV and its string
parsers `parse_target` / `parse_law` (audit KT-18). Numbers are copied as
written, so every law is bit-identical to what the parser built.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2] / "crates/format/kontakt"
TSV = ROOT / "data/script_knobs.tsv"
RS = ROOT / "src/script_knobs.rs"

NAMED = {
    "unit": "lin(0.0, 1.0)",
    "bipolar": "BIPOLAR",
    "on": "on()",
    "six": "six_steps()",
    "mod_hz": "exp(0.05, 8.0)",
    "lim_rel": "ir::Law::Breakpoints(LIMITER_RELEASE_MS.into())",
    "damping": "damping()",
    "tr_in": "tr_in()",
    "lfo_rate": "exp(0.010_415, 0.010_415 * 20_465.0)",
    "lfo_steps": "exp(48.0 / 0.010_415, 48.0 / (0.010_415 * 20_465.0))",
    "speed": "crate::laws::speed()",
}


def target(spec):
    if spec.startswith("Slot:"):
        return f"Slot(S::{spec[5:]})"
    if spec.startswith("Effect:"):
        return f"Effect(E::{spec[7:]})"
    return spec


def law(spec):
    m = re.fullmatch(r"(lin|exp):(.+),(.+)", spec)
    if m:
        return f"{m[1]}({m[2]}, {m[3]})"
    return NAMED[spec]


rows = []
for line in TSV.read_text().splitlines():
    if not line or line.startswith("#"):
        continue
    name, tgt, lw = line.split("\t")
    rows.append(f'        ("{name}", {target(tgt)}, {law(lw)}),')

table = "\n".join(rows)
new_fn = f'''/// What KSP's `$ENGINE_PAR_*` knobs set (value ÷ 1e6 is the knob x), each
/// through Kontakt's law from the knob to the physical value: one row per
/// name; coupled modules contribute their rows directly.
pub(crate) fn script_knobs() -> Vec<(&'static str, ir::KnobTarget, ir::Law)> {{
    use ir::EffectParam as E;
    use ir::KnobTarget::{{Effect, LfoPhase, LfoRate, LfoSteps, LfoWeight, LfoWidth, Slot, Speed}};
    use ir::SlotKnob as S;
    let lin = |lo, hi| ir::Law::Linear {{ lo, hi }};
    let exp = |lo, hi| ir::Law::Exp {{ lo, hi }};
    let on = || ir::Law::Breakpoints([(0.0, 0.0, 0.0), (1e-6, 1.0, 0.0), (1.0, 1.0, 0.0)].into());
    let damping = || {{
        ir::Law::Table(
            (0..=64u8)
                .map(|n| crate::drive::damping_hz(f32::from(n) / 64.0))
                .collect(),
        )
    }};
    let tr_in =
        || ir::Law::Breakpoints([(0.0, -24.0, 0.0), (0.5, 0.0, 0.0), (1.0, 6.0, 0.0)].into());
    #[rustfmt::skip]
    let mut rows = vec![
{table}
    ];
'''

src = RS.read_text()
start = src.index("const KNOB_TABLE")
end = src.index("    rows.extend(crate::dynamics::feedback_rows());")
src = src[:start] + new_fn + src[end:]
RS.write_text(src)
TSV.unlink()
print(f"{len(rows)} rows", file=sys.stderr)
