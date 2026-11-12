//! Choral (`$EFFECT_TYPE_CHORAL`, 43) onto [`ir::Ensemble`], from Kontakt
//! 8.13.1's own structure (file 0141930000.c):
//! - seven voices; per mode, nine rows of per-voice data (FUN_14193cd70):
//!   delay scale, LFO rate and offset, a second LFO's rate and depth, the
//!   line filter's corner and high-pass share, the sweep's bend and its
//!   smoothing (per sample at the host rate);
//! - per sample (FUN_14193be50): line `i` takes `inv·g_i·Σₖ c[(k − i) mod
//!   7]·read_k + cin·x`; its tap `inv·read_i + input_i` turns to the bus by
//!   Width; the bus passes a low shelf; out
//!   `bus·mix·cin·norm·0.707 + (1 − mix)·x`;
//! - each line's input passes a Butterworth state-variable filter mixed
//!   `(1 − hp)·LP + hp·HP`, and is read 4-point cubic at
//!   `G0·span·fs + fs·(R − 1)/(2·f)·m`, `m` the bent triangle
//!   `u(1 − σ) + σu²` plus `D2·(1 + ½·l2)`, l2 a cosine from its low point,
//!   rounded by `m ← (1 − α)·m' + α·m`;
//! - knobs (FUN_14193b140, FUN_14193ae70, FUN_14193a3e0, FUN_14193a4a0,
//!   FUN_14193a620): rate `7.4·r³ + 0.25` Hz, `R = 2^(0.025·(0.99·a² +
//!   0.01))`, span `5·Delay + 0.1`, `F = min(0.999·fb^¼, 0.99)`,
//!   `cin = (1 − F⁴)^¼`, `g_v = F^(G0[v]/G0[0])`, the voices' levels and
//!   pans, the scatter row, the shelf `S = max(1 − F, 0.2)` at
//!   `min(0.25/(G0[0]·span), 50)/S^¼` Hz.
//!
//! Unread: the second LFO's start (taken as 0); modes 3 and 4 share a
//! table.

use std::f32::consts::{FRAC_PI_4, SQRT_2};
use std::sync::LazyLock;

use kontra_ir as ir;

/// A new Choral's knobs as Kontakt reads them back, in [`model`]'s order:
/// rate, mode, amount, voices, delay, width, feedback, scatter, invert, mix.
pub(crate) const INIT: [f32; 10] = [0.0, 0.0, 0.494_95, 0.0, 0.18, 0.5, 0.0, 0.0, 0.0, 0.995];

const V: usize = ir::ENSEMBLE_VOICES;
type Row = [f32; V];

/// Per mode, Kontakt's nine rows (0x14193cd70): delay scale (s), LFO rate
/// ratio, LFO offset (cycles), LFO 2 rate ratio, LFO 2 raw depth, filter
/// corner (Hz), high-pass share, bend, smoothing per sample.
const TABLES: [[Row; 9]; 4] = [
    [
        [0.003, 0.003, 0.007, 0.007, 0.011, 0.011, 0.013],
        [1.0, 1.0, 1.428_57, 1.429_999, 1.724_2, 1.727_648, 0.742_3],
        [0.0, 0.5, 0.0, 0.5, 0.0, 0.5, 0.0],
        [6.1, 6.1, 6.1, 6.1, 0.1, 0.1, 1.0],
        [0.0, 0.0, 0.0, 0.0, 0.01, 0.01, 0.0],
        [5000.0, 5000.0, 5000.0, 5000.0, 10_000.0, 10_000.0, 50.0],
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        [0.2, 0.21, 0.21, 0.2, 0.4, 0.4, 0.0],
        [0.4, 0.4, 0.4, 0.4, 0.4, 0.4, 0.0],
    ],
    [
        [0.007, 0.007, 0.007, 0.007, 0.013, 0.013, 0.029],
        [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.742_3],
        [0.0, 0.5, 0.333_33, 0.833_33, 0.666_66, 0.166_66, 0.0],
        [4.1, 4.1, 4.0, 4.0, 4.12, 4.124_12, 1.0],
        [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0],
        [4000.0, 4000.0, 6000.0, 6000.0, 10_000.0, 10_000.0, 50.0],
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        [0.01, 0.01, 0.01, 0.013, 0.014, 0.012, 0.0],
        [0.3, 0.32, 0.32, 0.3, 0.33, 0.3, 0.4],
    ],
    [
        [0.011, 0.011, 0.011, 0.011, 0.011, 0.011, 0.053],
        [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.742_3],
        [0.0, 0.5, 0.5, 0.0, 0.25, 0.75, 0.0],
        [6.1, 6.1, 6.1, 6.1, 6.1, 6.1, 1.0],
        [0.0; V],
        [12_000.0, 12_000.0, 150.0, 150.0, 20_000.0, 20_000.0, 50.0],
        [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0],
        [0.1, 0.1, 0.101, 0.101, 0.4, 0.4, 0.0],
        [0.2, 0.22, 0.22, 0.2, 0.2, 0.2, 0.0],
    ],
    [
        [0.007, 0.007, 0.011, 0.011, 0.013, 0.013, 0.029],
        [1.0, 1.0, 1.428_57, 1.428_57, 1.724_2, 1.724_2, 0.742_3],
        [0.0, 0.0, 0.0, 0.25, 0.0, 0.25, 0.0],
        [6.1, 6.1, 6.1, 6.1, 6.1, 6.1, 1.0],
        [0.0, 0.0, 0.3, 0.3, 0.6, 0.6, 0.0],
        [
            20_000.0, 20_000.0, 20_000.0, 20_000.0, 20_000.0, 20_000.0, 50.0,
        ],
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        [0.0, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.99; V],
    ],
];

/// The host rate the smoothing's per-sample coefficients were heard at.
const SMOOTH_RATE: f32 = 48_000.0;

/// Each mode's rows in physical units.
static MODES: LazyLock<[ir::EnsembleVoices; 4]> = LazyLock::new(|| {
    TABLES.map(
        |[delay, rate, start, rate2, depth2, corner, hp, bend, alpha]| {
            fn pair<T>(a: Row, b: Row, f: impl Fn(f32, f32) -> T) -> [T; V] {
                std::array::from_fn(|v| f(at(&a, v), at(&b, v)))
            }
            ir::EnsembleVoices {
                delay,
                rate,
                start,
                bend,
                wobble_rate: rate2,
                // LFO 2 runs only above 1e-8 (0x147697a60), at depth G4/G3.
                wobble: pair(
                    depth2,
                    rate2,
                    |d, r| if d > 1e-8 && r > 0.0 { d / r } else { 0.0 },
                ),
                smooth_hz: alpha.map(|a| match a > 0.0 {
                    true => -a.ln() * SMOOTH_RATE / std::f32::consts::TAU,
                    false => f32::INFINITY,
                }),
                filter: pair(corner, hp, |hz, hp| ir::Section {
                    hz,
                    num: [1.0 - hp, 0.0, hp],
                    den: [1.0, SQRT_2, 1.0],
                }),
            }
        },
    )
});

fn at(r: &Row, v: usize) -> f32 {
    r.get(v).copied().unwrap_or(0.0)
}

/// The circulant's first row at Scatter 1 (FUN_14193a620); Scatter 0 is
/// the identity.
const SCATTER: Row = [
    -0.554_097, -0.261_204, -0.261_204, 0.445_903, -0.261_204, 0.445_903, 0.445_903,
];

/// The chorus at knob positions `p` (see [`INIT`]).
pub(crate) fn choral(p: [f32; 10]) -> ir::EffectKind {
    let [
        rate,
        mode,
        amount,
        voices,
        delay,
        width,
        feedback,
        scatter,
        invert,
        mix,
    ] = p;
    let [rate, amount, voices, delay, width, feedback, scatter, mix] =
        [rate, amount, voices, delay, width, feedback, scatter, mix].map(|x| x.clamp(0.0, 1.0));
    let [m0, m1, m2, m3] = &*MODES;
    let table = match mode.round() as i32 {
        ..=0 => m0,
        1 => m1,
        2 => m2,
        _ => m3,
    };
    let span = 5.0 * delay + 0.1;
    let ratio = 2f32.powf(0.025 * (0.99 * amount * amount + 0.01));
    let f = (0.999 * feedback.sqrt().sqrt()).min(0.99);
    let input = (1.0 - f.powi(4)).sqrt().sqrt();
    // Pairs 2/3 and 4/5 fade in over Voices' two halves; voice 6 is heard
    // only through the scatter.
    let fade = |k: f32| (2.0 * voices - k).clamp(0.0, 1.0).sqrt();
    let levels = [1.0, 1.0, fade(0.0), fade(0.0), fade(1.0), fade(1.0), 0.0];
    let sum: f32 = levels.iter().sum();
    let norm = (0.2 * sum + 0.6).sqrt() / sum;
    // Pair k turns by ∓ Width·π/4·(1 − 2k/7), even voices left.
    let angles = std::array::from_fn(|v| match v {
        0..6 => {
            let reach = 1.0 - (v / 2 * 2) as f32 / 7.0;
            let side = if v % 2 == 0 { -1.0 } else { 1.0 };
            side * width * FRAC_PI_4 * reach
        }
        _ => 0.0,
    });
    let first = at(&table.delay, 0) * span;
    let shelf = (1.0 - f).max(0.2);
    ir::EffectKind::Ensemble(ir::Ensemble {
        voices: table,
        span,
        hz: 7.4 * rate.powi(3) + 0.25,
        sweep: 0.5 * (ratio - 1.0),
        levels,
        angles,
        scatter: std::array::from_fn(|k| {
            at(&SCATTER, k) * scatter + if k == 0 { 1.0 } else { 0.0 }
        }),
        feedback: f,
        polarity: match invert > 0.0 {
            true => ir::Polarity::Inverted,
            false => ir::Polarity::Normal,
        },
        input,
        wet: mix * input * norm * 0.707,
        direct: 1.0 - mix,
        shelf: ir::Section {
            hz: (0.25 / first).min(50.0) / shelf.sqrt().sqrt(),
            num: [shelf, SQRT_2 * shelf.sqrt(), 1.0],
            den: [1.0, SQRT_2, 1.0],
        },
    })
}

/// A Choral's knobs as a script moves them: every move rebuilds it from all.
pub(crate) fn model() -> ir::KnobModel {
    ir::KnobModel {
        knobs: (0..10).map(ir::SlotKnob::Knob).collect(),
        saved: INIT.into(),
        build: |p| Some(choral(p.try_into().ok()?)),
    }
}

/// What `$EFFECT_TYPE_CHORAL` (43) loads, as Morphology Evolved, Conflux
/// and Shreddage load it: [`INIT`], every knob through [`model`].
pub(crate) fn slot_type() -> ir::SlotType {
    let effect = ir::Effect {
        kind: choral(INIT),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    ir::SlotType {
        id: 43,
        subtype: false,
        reads: 0,
        kind: 43,
        effect: Some(effect),
        laws: [].into(),
        model: Some(model()),
    }
}

/// The KSP names of [`model`]'s knobs: a straight 0..1 for the continuous
/// ones; mode (0..3) and the switches are set as raw values.
pub(crate) fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 10] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let raw = || ir::Law::Linear { lo: 0.0, hi: 1e6 };
    [
        ("$ENGINE_PAR_CHORAL_RATE", Slot(Knob(0)), unit()),
        ("$ENGINE_PAR_CHORAL_MODE", Slot(Knob(1)), raw()),
        ("$ENGINE_PAR_CHORAL_AMOUNT", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_CHORAL_VOICES", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_CHORAL_DELAY", Slot(Knob(4)), unit()),
        ("$ENGINE_PAR_CHORAL_WIDTH", Slot(Knob(5)), unit()),
        ("$ENGINE_PAR_CHORAL_FEEDBACK", Slot(Knob(6)), unit()),
        ("$ENGINE_PAR_CHORAL_SCATTER", Slot(Knob(7)), raw()),
        ("$ENGINE_PAR_CHORAL_INVERT_PHASE", Slot(Knob(8)), raw()),
        ("$ENGINE_PAR_CHORAL_MIX", Slot(Knob(9)), unit()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_choral_is_kontakts_init() {
        // Kontakt 8.13.1 at INIT: one voice pair, first tap 3 ms·(5·0.18 +
        // 0.1), 0.25 Hz, half a cycle apart; the other voices silent.
        let ir::EffectKind::Ensemble(e) = choral(INIT) else {
            panic!("an ensemble");
        };
        assert!((e.voices.delay[0] * e.span - 0.003).abs() < 1e-6);
        assert!((e.hz - 0.25).abs() < 1e-6 && (e.voices.start[1] - 0.5).abs() < 1e-6);
        assert_eq!(e.levels[2..], [0.0; 5]);
        assert_eq!(e.scatter, [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }
}
