//! The 3x2 Versatile (filter type 19) as an [`ir::Cascade`]. Laws read from
//! Kontakt 8: the cutoff setter `FUN_14095db50` (note `111x + 20`, stages 1
//! and 2 up by `80x` semitones each, `g = 440·2^((note − 69)/12)·6.283/rate`
//! up to the stage's limit), the resonance setters `FUN_140966230`,
//! `FUN_1409663f0` and `FUN_1409665c0`, the normaliser `FUN_14096f840` and
//! process `FUN_140aef6e0` (docs/re/engine/filt-multi-versatile.md; the DSP
//! matches them under kemu, kontra-dsp-filter tests/cascade.rs).

use kontra_ir as ir;
use kontra_law::kontakt::VERSATILE_NOTE_HZ;

use crate::script_knobs::SlotKnob;

/// The record's knobs after its kind, as `set_engine_par` sets them:
/// cutoff, resonance, SHIFTB, SHIFTC, RESB, RESC, TYPEA..C, BYPA..C (each
/// stage's wet amount), FILTER_GAIN.
pub(crate) type Knobs = [f32; 13];

/// A newly loaded unit's knobs, read back in `on init` (Kontakt 8.13 oracle,
/// ANALOG STRINGS insert slot 6): all 0, FILTER_GAIN 0.5.
pub(crate) const DEFAULT: Knobs = [
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5,
];

/// The setter's `6.283` (`0x401921cac083126f`) where 2π belongs.
const NATIVE_TAU: f64 = f64::from_bits(0x4019_21ca_c083_126f);

/// Stage 0's corner for cutoff `x`, as Hz on an exact 2π.
fn hz(x: f32) -> f32 {
    let note = x.clamp(0.0, 1.0) * 111.0 + 20.0;
    VERSATILE_NOTE_HZ.eval_unclamped(note) * (NATIVE_TAU / std::f64::consts::TAU) as f32
}

/// Octaves a shift knob `x` raises the next stage by.
const SHIFT_OCTAVES: f32 = 80.0 / 12.0;

fn stage(r: f32, morph: f32, wet: f32) -> ir::CascadeStage {
    let q = if r > 0.5 {
        (r - 0.5) * 0.66 + 0.65
    } else {
        r * 1.3
    };
    ir::CascadeStage {
        q,
        limit: (q + 1.3075) * q * 0.293 + 0.7352,
        peak: 1.0 / ((1.0 - r) * 1.4 + 0.05),
        morph,
        wet,
    }
}

pub(crate) fn cascade(k: Knobs) -> ir::Cascade {
    let [cut, res, sb, sc, rb, rc, ta, tb, tc, wa, wb, wc, gain] = k.map(|v| v.clamp(0.0, 1.0));
    ir::Cascade {
        hz: hz(cut),
        shifts: [sb * SHIFT_OCTAVES, sc * SHIFT_OCTAVES],
        stages: [stage(res, ta, wa), stage(rb, tb, wb), stage(rc, tc, wc)],
        gain: kontra_law::kontakt::VERSATILE_GAIN.eval(gain),
    }
}

/// Every knob's law. Each knob moves only its own fields, so the laws hold
/// whatever the others are set to.
pub(crate) fn knobs() -> Vec<SlotKnob> {
    use ir::EffectParam as E;
    use ir::SlotKnob as K;
    let unit = ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let shift = ir::Law::Linear {
        lo: 0.0,
        hi: SHIFT_OCTAVES,
    };
    let mut out = vec![
        (
            K::Cutoff,
            E::CascadeHz,
            ir::Law::Exp {
                lo: hz(0.0),
                hi: hz(1.0),
            },
        ),
        (K::StageShift(1), E::CascadeShift(0), shift.clone()),
        (K::StageShift(2), E::CascadeShift(1), shift),
        (
            K::OutputGain,
            E::CascadeGain,
            ir::Law::Exp {
                lo: 10f32.powf(-1.5),
                hi: 10f32.powf(1.5),
            },
        ),
    ];
    let table = |f: fn(ir::CascadeStage) -> f32| {
        let at = |n: u8| f(stage(f32::from(n) / 64.0, 0.0, 1.0));
        ir::Law::Table((0..=64).map(at).collect())
    };
    for n in 0..3 {
        let resonance = if n == 0 {
            K::Resonance
        } else {
            K::StageResonance(n)
        };
        out.extend([
            (resonance, E::CascadeQ(n), table(|s| s.q)),
            (resonance, E::CascadeLimit(n), table(|s| s.limit)),
            (resonance, E::CascadePeak(n), table(|s| s.peak)),
            (K::StageMorph(n), E::CascadeMorph(n), unit.clone()),
            (K::StageWet(n), E::CascadeWet(n), unit.clone()),
        ]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{NATIVE_TAU, hz};

    #[test]
    fn cutoff_preserves_the_original_note_to_hz_calculation() {
        for n in 0..=10_000 {
            let x = n as f32 / 10_000.0;
            let note = x * 111.0 + 20.0;
            let expected =
                440.0 * ((note - 69.0) / 12.0).exp2() * (NATIVE_TAU / std::f64::consts::TAU) as f32;
            assert_eq!(hz(x), expected, "cutoff {x}");
        }
    }
}
