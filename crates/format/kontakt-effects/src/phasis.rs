//! Phasis (`$EFFECT_TYPE_PHASIS`, 41) onto [`ir::Notches`], from Kontakt
//! 8.13.1's own structure (file 0141970000.c; notes with addresses in
//! scratch/lane-scripts/phasis-choral-STRUCTURE.md):
//! - it runs at `fs·2^round(log₂(90000/fs))` (FUN_14197c780);
//! - per sample (FUN_14197cfe0): the LFO's two lanes sit `±(stereo/2 −
//!   ¼)` cycles either side of its phase; each is a triangle rounded by
//!   `ρ = 1.995·clamp(f − 1, 0, 1)³` (FUN_14197eaf0), warped by
//!   `expm1(0.1·u)` about its peaks, times `2^(−2.6575·clamp(log₂₀ f, 0,
//!   1))`;
//! - the corner (FUN_14197e6e0): note `lerp(30, 120 | −30, 190 Ultra;
//!   centre) + (1 − |mm|)·amount²·c·56 (121 Ultra)·lfo`; the stages'
//!   `log₂ R = 3·clamp(0.9·(2·spread − 1) + 0.9·amount²·c·mm·lfo, ±0.9)`,
//!   `c = 1 + Ultra·((2/K)^0.7 − 1)` (FUN_14197ef70);
//! - the loop (FUN_14197dad0, FUN_14197bc10): `u = x + fb·band(L·AP(u))`
//!   solved without a delay, `fb = 0.95·Feedback`, the band from the
//!   corner `÷a` to `min(·a, tan⁻¹ 0.1)`, `a = r + √(1 + r²)`,
//!   `r = R/tan(0.25/K)` (FUN_14197ed80); out `lerp(x, ½(u + L·AP(u)),
//!   mix)/√(mix/(1 − fb) + 1 − mix)`, then a 5 Hz high-pass;
//! - the rate: note `lerp(−120, 20 | 111 Ultra; rate)`, `f = 440·2^((note −
//!   69)/12)`.
//!
//! Unread: the resampler's kernel (FUN_141ee2160; ours is the halfband);
//! which way the Invert Phase and Invert Mod Mix switches' ramps run (taken
//! as `1 − 2·switch`); the loop band's top edge 0.1 has one writer found
//! (the constructor). The tempo-synced rate is not carried (Hz as without).

use kontra_ir as ir;

/// A new Phasis's knobs as Kontakt reads them back, in [`model`]'s order:
/// rate, rate unit, Ultra, amount, centre, stereo, spread, feedback, mod
/// mix, notches, invert phase, invert mod mix, mix.
pub const INIT: [f32; 13] = [
    0.391_294, 0.0, 0.0, 1.0, 0.499_922, 0.5, 0.5, 0.210_526, 0.0, 2.0, 0.0, 1.0, 1.0,
];

/// `a` to `b` by `t`.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The notch phaser at knob positions `p` (see [`INIT`]).
pub fn phasis(p: [f32; 13]) -> ir::EffectKind {
    let [
        rate,
        _unit,
        ultra,
        amount,
        centre,
        stereo,
        spread,
        feedback,
        mod_mix,
        notches,
        invert,
        invert_mod,
        mix,
    ] = p;
    let [rate, amount, centre, stereo, spread, feedback, mod_mix, mix] =
        [rate, amount, centre, stereo, spread, feedback, mod_mix, mix].map(|x| x.clamp(0.0, 1.0));
    let ultra = if ultra > 0.0 { 1.0 } else { 0.0 };
    let stages = notches.round().clamp(2.0, 12.0);
    // The ramps of FUN_14197ef70 (0x14a1821c8..f4), Ultra off to on.
    // Invert Mod Mix polarity (FUN_14197df70 row 14, 0x14a64ef98..efa8):
    // base = -1.0, mult = +2.0 => 2*switch - 1 (so INIT switch=1 gives +mod_mix).
    let reach = amount * amount * (1.0 + ultra * ((2.0 / stages).powf(0.7) - 1.0));
    let mm = (mod_mix * (2.0 * invert_mod.clamp(0.0, 1.0) - 1.0)).clamp(-1.0, 1.0);
    let note = lerp(lerp(30.0, -30.0, ultra), lerp(120.0, 190.0, ultra), centre);
    let rate_note = lerp(-120.0, lerp(20.0, 111.0, ultra), rate);
    let lfo_hz = 440.0 * ((rate_note - 69.0) / 12.0).exp2();
    ir::EffectKind::Notches(ir::Notches {
        stages: stages as u8,
        hz: 440.0 * ((note - 69.0) / 12.0).exp2(),
        octaves: (1.0 - mm.abs()) * reach * lerp(56.0, 121.0, ultra) / 12.0,
        spread: 3.0 * 0.9 * (2.0 * spread - 1.0),
        spread_swing: 3.0 * 0.9 * reach * mm,
        spread_limit: 3.0 * 0.9,
        feedback: 0.95 * feedback,
        loop_width: 1.0 / (0.25 / stages).tan(),
        loop_top: 0.1,
        rate: ir::Rate::Hz(lfo_hz),
        rounding: 1.995 * (lfo_hz - 1.0).clamp(0.0, 1.0).powi(3),
        // log₂₀ f as notes: 1 Hz is note −36.376, 20 Hz 51.86 notes on.
        depth: (-2.657_542_5 * ((rate_note + 36.376_3) * 0.019_281_5).clamp(0.0, 1.0)).exp2(),
        warp: 0.1,
        phase: 0.5 * stereo - 0.25,
        polarity: match invert > 0.0 {
            true => ir::Polarity::Inverted,
            false => ir::Polarity::Normal,
        },
        mix,
        low_cut_hz: 5.0,
        internal_hz: 90_000.0,
    })
}

/// Decode a saved `BParFXPhasis` (`0x5b`, v0x51, 52 bytes) into [`INIT`]'s
/// knob order (`Kontakt 8.exe` `0x140d020a0` & `fx_raum_delight_psyche_vibrato_phasis_replika.tsv`):
/// 9 floats (`rate`, `center`, `spread`, `feedback`, `mix`, `notches_norm`,
/// `amount`, `stereo`, `mod_mix`), 3 bools (`ultra`, `invert_phase`,
/// `invert_mod_mix`), then 3 sync floats + 1 sync bool.
#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<[f32; 13]> {
    if version != 0x51 || public.len() < 52 {
        return None;
    }
    let f = |offset: usize| {
        let bytes = public.get(offset..offset + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    let flag = |offset: usize| public.get(offset).map(|&b| if b != 0 { 1.0 } else { 0.0 });
    let rate = f(0)?;
    let centre = f(4)?;
    let spread = f(8)?;
    let feedback = f(12)?;
    let mix = f(16)?;
    let notches_norm = f(20)?.clamp(0.0, 1.0);
    // Native 0x140d020a0 (0x144e79ad8 = -11.0f): `clamp(trunc(2 + 11 * notches_norm), 2, 12)`.
    let notches = (2.0 + 11.0 * notches_norm).trunc().clamp(2.0, 12.0);
    let amount = f(24)?;
    let stereo = f(28)?;
    let mod_mix = f(32)?;
    let ultra = flag(36)?;
    let invert_phase = flag(37)?;
    let invert_mod_mix = flag(38)?;
    let rate_unit_beats = f(39)?;
    let unit = if rate_unit_beats > 1e-9 { 1.0 } else { 0.0 };
    Some([
        rate,
        unit,
        ultra,
        amount,
        centre,
        stereo,
        spread,
        feedback,
        mod_mix,
        notches,
        invert_phase,
        invert_mod_mix,
        mix,
    ])
}

/// Single-encoding module descriptor for Phasis (`0x5b`).
pub static MODULE: crate::module::Module = crate::module::Module {
    id: crate::module::ids::PHASIS,
    module_id: 0x5b,
    versions: (0, u32::MAX),
    name: "Phasis",
    script_type: 41,
    mix: crate::module::MixMode::OwnMix,
    knobs: crate::module::KNOBS.split_at(13).0,
    build: |p| Some(phasis(p.try_into().ok()?)),
};

/// A Phasis's knobs from saved positions `p` as a script moves them.
pub fn model_from(saved: [f32; 13]) -> ir::KnobModel {
    MODULE.knob_model(&saved)
}

/// A Phasis's knobs as a script moves them: every move rebuilds it from all.
pub fn model() -> ir::KnobModel {
    model_from(INIT)
}

/// What `$EFFECT_TYPE_PHASIS` (41) loads, as Morphology Evolved, Conflux
/// and Shreddage load it: [`INIT`], every knob through [`model`].
pub fn slot_type() -> ir::SlotType {
    let effect = ir::Effect {
        kind: phasis(INIT),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    ir::SlotType {
        id: 41,
        subtype: false,
        reads: 0,
        kind: 41,
        effect: Some(effect),
        laws: [].into(),
        model: Some(model()),
    }
}

/// The KSP names of [`model`]'s knobs: a straight 0..1 for the continuous
/// ones (their texts are 0..100 % straight lines, or the laws above); the
/// stepped ones (notches 2..12, the switches, the unit) are set as raw
/// values.
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 13] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let raw = || ir::Law::Linear { lo: 0.0, hi: 1e6 };
    [
        ("$ENGINE_PAR_PHASIS_RATE", Slot(Knob(0)), unit()),
        ("$ENGINE_PAR_PHASIS_RATE_UNIT", Slot(Knob(1)), raw()),
        ("$ENGINE_PAR_PHASIS_ULTRA", Slot(Knob(2)), raw()),
        ("$ENGINE_PAR_PHASIS_AMOUNT", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_PHASIS_CENTER", Slot(Knob(4)), unit()),
        ("$ENGINE_PAR_PHASIS_STEREO", Slot(Knob(5)), unit()),
        ("$ENGINE_PAR_PHASIS_SPREAD", Slot(Knob(6)), unit()),
        ("$ENGINE_PAR_PHASIS_FEEDBACK", Slot(Knob(7)), unit()),
        ("$ENGINE_PAR_PHASIS_MOD_MIX", Slot(Knob(8)), unit()),
        ("$ENGINE_PAR_PHASIS_NOTCHES", Slot(Knob(9)), raw()),
        ("$ENGINE_PAR_PHASIS_INVERT_PHASE", Slot(Knob(10)), raw()),
        ("$ENGINE_PAR_PHASIS_INVERT_MOD_MIX", Slot(Knob(11)), raw()),
        ("$ENGINE_PAR_PHASIS_MIX", Slot(Knob(12)), unit()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_phasis_is_kontakts_init() {
        // Kontakt 8.13.1 reads back two notches, centre 622 Hz, spread 50 %
        // (Q ½), rate 0.19 Hz, feedback 20 %.
        let ir::EffectKind::Notches(n) = phasis(INIT) else {
            panic!("a notch phaser");
        };
        assert_eq!(n.stages, 2);
        assert!((n.hz - 622.0).abs() < 4.0, "{}", n.hz);
        assert!(n.spread.abs() < 1e-3);
        assert!(matches!(n.rate, ir::Rate::Hz(hz) if (hz - 0.19).abs() < 0.01));
        assert!((n.feedback - 0.95 * 0.210_526).abs() < 1e-4);
    }

    #[test]
    fn invert_mod_mix_follows_binary_polarity_and_saved_decodes_v51() {
        let mut p = INIT;
        p[8] = 0.5; // mod_mix
        p[11] = 1.0; // invert_mod_mix = 1 (INIT) -> positive spread_swing
        let ir::EffectKind::Notches(pos) = phasis(p) else {
            panic!("notches");
        };
        assert!(pos.spread_swing > 0.0, "{}", pos.spread_swing);
        p[11] = 0.0; // invert_mod_mix = 0 -> negative spread_swing
        let ir::EffectKind::Notches(neg) = phasis(p) else {
            panic!("notches");
        };
        assert!((neg.spread_swing + pos.spread_swing).abs() < 1e-6);

        let mut public = Vec::new();
        // rate, center, spread, feedback, mix, notches_norm (0.4 -> 6), amount, stereo, mod_mix
        for v in [0.5f32, 0.4, 0.6, 0.3, 0.8, 0.4, 0.9, 0.25, 0.5] {
            public.extend(v.to_le_bytes());
        }
        public.extend([1u8, 1, 0]); // ultra, invert_phase, invert_mod_mix
        for v in [-1.0f32, 0.0, 0.0] {
            public.extend(v.to_le_bytes());
        }
        public.push(0);
        let decoded = saved(0x51, &public).unwrap();
        assert_eq!(decoded[9], 6.0);
        assert_eq!(decoded[2], 1.0);
        assert_eq!(decoded[10], 1.0);
        assert_eq!(decoded[11], 0.0);
        assert_eq!(decoded[12], 0.8);
    }
}
