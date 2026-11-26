//! Raum (`$EFFECT_TYPE_RAUM`, 51) onto the generic [`ir::Reverb`] network.
//!
//! Predelay, high-cut, low-shelf, dry/wet mix (`$ENGINE_PAR_RAUM_REVERB`),
//! predelay feedback (`$ENGINE_PAR_RAUM_FEEDBACK`), freeze
//! (`$ENGINE_PAR_RAUM_FREEZE`), sparse (`$ENGINE_PAR_RAUM_SPARSE`), and
//! tempo-synced predelay (`$ENGINE_PAR_RAUM_PREDELAY_UNIT` /
//! `$ENGINE_PAR_RAUM_TIME_UNIT`) follow the Kontakt 8 binary laws in
//! `docs/re/engine/k-raum-cosmic-laws.md`.

use kontra_ir as ir;

/// A new Raum's knobs, in [`model`]'s order: type, decay, size, damping,
/// diffusion, predelay, high cut, low shelf, modulation, rate, feedback,
/// freeze, sparse, reverb (mix), sync unit.
pub(crate) const INIT: [f32; 15] = [
    0.0, 0.5, 0.5, 0.5, 0.8, 0.0, 0.8, 0.0, 0.3, 0.3, 0.0, 0.0, 0.0, 0.5, 0.0,
];

/// Default Raum mix (`$ENGINE_PAR_RAUM_REVERB`) for a freshly loaded unit.
const INIT_MIX: f32 = 0.5;

/// Each type's base size: Grounded, Airy, Cosmic.
const BASE: [f32; 3] = [0.7, 1.2, 2.0];

/// Raum predelay law in ms ([`kontra_law::kontakt::RAUM_PREDELAY_MS`]).
pub(crate) fn predelay_ms(x: f32) -> f32 {
    kontra_law::kontakt::RAUM_PREDELAY_MS.eval(x)
}

/// Raum post-loop high-cut law in Hz ([`kontra_law::kontakt::RAUM_HIGH_CUT_HZ`]).
pub(crate) fn high_cut_hz(x: f32) -> f32 {
    kontra_law::kontakt::RAUM_HIGH_CUT_HZ.eval(x)
}

/// Raum post-loop low-shelf moving corner frequency (`Kontakt8.exe` [ref]):
/// `(200 + 200·max(0, x − 0.5)) / A^(1/4)` Hz, where `A = 10^(−24x / 20)`.
// Note: `ir::Reverb` currently fixes the shelf corner at 200 Hz; keep this
// binary-verified law ready to wire once `ir::Reverb` carries a shelf corner.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn low_shelf_hz(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let a = 10f32.powf(-24.0 * x / 20.0);
    (200.0 + 200.0 * (x - 0.5).max(0.0)) / a.sqrt().sqrt()
}

/// Raum dry/wet mix law (`Kontakt8.exe` [ref]) for `$ENGINE_PAR_RAUM_REVERB`:
/// `m <= 0.5` → `(dry = 1, wet = (2m)^(3/2))`;
/// `m > 0.5` → `(dry = 2 − 2m, wet = 1)`.
pub(crate) fn mix_gains(mix: f32) -> (f32, f32) {
    let m = mix.clamp(0.0, 1.0);
    if m <= 0.5 {
        let t = 2.0 * m;
        (1.0, t * t.sqrt())
    } else {
        (2.0 - 2.0 * m, 1.0)
    }
}

/// Raum predelay feedback loop gain (`Kontakt8.exe` [ref]):
/// `0.5·sqrt(clamp(F, 0, 1))` when `predelay_ms >= 0.1` (`1e-4` s gate), else `0`.
pub(crate) fn feedback_gain(feedback: f32, predelay_ms: f32) -> f32 {
    if predelay_ms < 0.1 {
        0.0
    } else {
        0.5 * feedback.clamp(0.0, 1.0).sqrt()
    }
}

fn effective_predelay_ms(predelay: f32, sync_unit: f32) -> f32 {
    if let Some(beats) = crate::psyche::sync_unit_beats(sync_unit.round() as i32) {
        // Synced predelay at 120 BPM reference (500 ms/beat); saved count or knob count.
        let count = if predelay > 1.0 {
            predelay.clamp(1.0, 16.0)
        } else {
            (1.0 + 7.0 * predelay.clamp(0.0, 1.0)).round().max(1.0)
        };
        (count * beats * 500.0).min(2000.0)
    } else {
        predelay_ms(predelay)
    }
}

/// The reverb at knob positions `p` (see [`INIT`]).
pub(crate) fn raum(p: [f32; 15]) -> ir::Reverb {
    let [
        kind,
        decay,
        size,
        damping,
        diffusion,
        predelay,
        high_cut,
        low_shelf,
        modulation,
        rate,
        feedback,
        freeze,
        sparse,
        _reverb,
        sync_unit,
    ] = p;
    let [
        decay,
        size,
        damping,
        diffusion,
        high_cut,
        low_shelf,
        modulation,
        rate,
        feedback,
    ] = [
        decay, size, damping, diffusion, high_cut, low_shelf, modulation, rate, feedback,
    ]
    .map(|x| x.clamp(0.0, 1.0));
    let frozen = freeze >= 0.5;
    let is_sparse = sparse >= 0.5;
    let base = BASE
        .get(kind.round().clamp(0.0, 2.0) as usize)
        .copied()
        .unwrap_or(1.0);
    let pd_ms = effective_predelay_ms(predelay, sync_unit);
    let fb = feedback_gain(feedback, pd_ms);
    let mut seconds = 0.2 * 100f32.powf(decay) * (1.0 + 2.5 * fb);
    let mut damp = damping;
    let mut diff = diffusion;
    let mut room_size = base * 4f32.powf(size - 0.5);
    let mut mod_ms = 2.0 * modulation;
    if is_sparse {
        diff *= 0.35;
        room_size = (room_size * 1.35).min(4.0);
    }
    if frozen {
        seconds = 1000.0;
        damp = 0.0;
        mod_ms *= 0.25;
    }
    ir::Reverb {
        seconds,
        size: room_size,
        network: None,
        damping: damp,
        diffusion: diff,
        predelay_ms: pd_ms,
        high_cut_hz: high_cut_hz(high_cut),
        low_shelf_db: -24.0 * low_shelf,
        stereo: 1.0,
        modulation_ms: mod_ms,
        modulation_hz: 0.1 * 50f32.powf(rate),
    }
}

/// Decode a saved `BParFXRaum` (`0x65`, v0x10, 67 bytes) into [`INIT`]'s knob
/// order and saved mix (`$ENGINE_PAR_RAUM_REVERB`)
/// (`Kontakt 8.exe` [ref] and
/// `fx_raum_delight_psyche_vibrato_phasis_replika.tsv:78-97`).
pub(crate) fn saved(version: u16, public: &[u8]) -> Option<([f32; 15], f32, Vec<&'static str>)> {
    if version != 0x10 || public.len() < 67 {
        return None;
    }
    let kind = i32::from_le_bytes(public.get(0..4)?.try_into().ok()?);
    if !(0..=2).contains(&kind) {
        return None;
    }
    let f = |offset: usize| {
        let bytes = public.get(offset..offset + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    let flag = |offset: usize| public.get(offset).map(|&b| b != 0);
    let predelay = f(4)?;
    let feedback = f(8)?;
    let low_shelf = f(12)?;
    let high_cut = f(16)?;
    let reverb = f(20)?;
    let diffusion = f(24)?;
    let decay = f(28)?;
    let modulation = f(32)?;
    let size = f(36)?;
    let damping = f(40)?;
    let rate = f(44)?;
    let freeze = flag(52)?;
    let sparse = flag(53)?;
    let sync_beats = f(54)?;
    let sync_unit =
        crate::psyche::beats_to_sync_unit(Some(sync_beats).filter(|&b| b > 1e-9)) as f32;
    Some((
        [
            kind as f32,
            decay,
            size,
            damping,
            diffusion,
            predelay,
            high_cut,
            low_shelf,
            modulation,
            rate,
            feedback,
            f32::from(u8::from(freeze)),
            f32::from(u8::from(sparse)),
            reverb,
            sync_unit,
        ],
        reverb,
        Vec::new(),
    ))
}

/// Custom [`ir::EffectModel`] so live `$ENGINE_PAR_RAUM_REVERB` updates the
/// slot's dry/wet mix using Kontakt's exact [`mix_gains`] law.
struct RaumModel;

impl ir::EffectModel for RaumModel {
    fn build(&self, params: &[f32]) -> Option<ir::EffectKind> {
        Some(ir::EffectKind::Reverb(raum(params.try_into().ok()?)))
    }

    fn build_effect(&self, params: &[f32], current: &ir::Effect) -> Option<ir::Effect> {
        let arr: [f32; 15] = params.try_into().ok()?;
        let kind = ir::EffectKind::Reverb(raum(arr));
        let (dry, wet) = mix_gains(arr[13]);
        // Preserve the slot's output-gain scaling factor relative to max(dry, wet).
        let scale = current.output.max(current.dry);
        let scale = if scale > 1e-6 { scale } else { 1.0 };
        Some(ir::Effect {
            kind,
            bypass: current.bypass,
            output: scale * wet,
            dry: scale * dry,
        })
    }
}

static EFFECT_MODEL: RaumModel = RaumModel;

/// Single-encoding module descriptor for Raum (`0x65`).
pub(crate) static MODULE: kontra_kontakt_effects::module::Module =
    kontra_kontakt_effects::module::Module {
        id: kontra_kontakt_effects::module::ids::RAUM,
        module_id: 0x65,
        versions: (0x10, 0x10),
        name: "Raum",
        script_type: 51,
        mix: kontra_kontakt_effects::module::MixMode::WetDry,
        knobs: kontra_kontakt_effects::module::KNOBS.split_at(15).0,
        build: |p| Some(ir::EffectKind::Reverb(raum(p.try_into().ok()?))),
    };

/// A Raum's knobs from saved positions as a script moves them.
pub(crate) fn model_from(saved: [f32; 15]) -> ir::KnobModel {
    ir::register_model(MODULE.id, &EFFECT_MODEL);
    ir::KnobModel {
        id: MODULE.id,
        knobs: MODULE.knobs.into(),
        saved: saved.into(),
    }
}

/// A Raum's knobs as a script moves them: every move rebuilds it from all.
pub(crate) fn model() -> ir::KnobModel {
    model_from(INIT)
}

/// What `$EFFECT_TYPE_RAUM` (51) loads, as Conflux loads it from its
/// script: [`INIT`], wet over the dry signal by [`mix_gains`], every knob
/// through [`model`].
pub(crate) fn slot_type() -> ir::SlotType {
    let (dry, output) = mix_gains(INIT_MIX);
    let effect = ir::Effect {
        kind: ir::EffectKind::Reverb(raum(INIT)),
        bypass: false,
        output,
        dry,
    };
    ir::SlotType {
        id: 51,
        subtype: false,
        reads: 0,
        kind: 51,
        effect: Some(effect),
        laws: [].into(),
        model: Some(model()),
    }
}

/// The KSP names of [`model`]'s knobs (all 16 Raum parameters, with
/// `PREDELAY_UNIT` and `TIME_UNIT` sharing `Knob(14)`), plus the live module
/// rows defined in `effects.rs`.
pub(crate) fn rows() -> Vec<(&'static str, ir::KnobTarget, ir::Law)> {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let raw = || ir::Law::Linear { lo: 0.0, hi: 1e6 };
    let mut out = vec![
        ("$ENGINE_PAR_RAUM_TYPE", Slot(Knob(0)), raw()),
        ("$ENGINE_PAR_RAUM_DECAY", Slot(Knob(1)), unit()),
        ("$ENGINE_PAR_RAUM_SIZE", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_RAUM_DAMPING", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_RAUM_DIFFUSION", Slot(Knob(4)), unit()),
        ("$ENGINE_PAR_RAUM_PREDELAY", Slot(Knob(5)), unit()),
        ("$ENGINE_PAR_RAUM_HIGHCUT", Slot(Knob(6)), unit()),
        ("$ENGINE_PAR_RAUM_LOWSHELF", Slot(Knob(7)), unit()),
        ("$ENGINE_PAR_RAUM_MOD", Slot(Knob(8)), unit()),
        ("$ENGINE_PAR_RAUM_RATE", Slot(Knob(9)), unit()),
        ("$ENGINE_PAR_RAUM_FEEDBACK", Slot(Knob(10)), unit()),
        ("$ENGINE_PAR_RAUM_FREEZE", Slot(Knob(11)), raw()),
        ("$ENGINE_PAR_RAUM_SPARSE", Slot(Knob(12)), raw()),
        ("$ENGINE_PAR_RAUM_REVERB", Slot(Knob(13)), unit()),
        ("$ENGINE_PAR_RAUM_PREDELAY_UNIT", Slot(Knob(14)), raw()),
        ("$ENGINE_PAR_RAUM_TIME_UNIT", Slot(Knob(14)), raw()),
    ];
    out.extend(crate::effects::live_rows());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kt03_raum_laws_match_binary_predelay_highcut_and_live_controls() {
        // Predelay: 0 -> 0, 0.25 -> 20, 0.5 -> 80, 0.75 -> 560, 1.0 -> 2000 ms.
        for (x, want) in [
            (0.0, 0.0),
            (0.25, 20.0),
            (0.5, 80.0),
            (0.75, 560.0),
            (1.0, 2000.0),
        ] {
            let mut p = INIT;
            p[5] = x;
            let got = raum(p).predelay_ms;
            assert!((got - want).abs() < 1e-2, "predelay({x}): {got} vs {want}");
        }

        // High cut: 0 -> 1000, 0.5 -> 6831.025, 0.95 -> 22050, 1.0 -> 110250 Hz.
        for (x, want) in [
            (0.0, 1000.0),
            (0.5, 6831.025),
            (0.95, 22_050.0),
            (1.0, 110_250.0),
        ] {
            let mut p = INIT;
            p[6] = x;
            let got = raum(p).high_cut_hz;
            assert!((got - want).abs() < 1.0, "high_cut({x}): {got} vs {want}");
        }

        // Low shelf: gain -24*x dB, moving corner (200 + 200*max(0, x-0.5)) / A^(1/4).
        for (x, want_hz) in [(0.0, 200.0), (0.5, 282.5075), (1.0, 598.5787)] {
            let got = low_shelf_hz(x);
            assert!(
                (got - want_hz).abs() < 0.05,
                "low_shelf_hz({x}): {got} vs {want_hz}"
            );
        }

        // Mix: m <= 0.5 => (1, (2m)^(3/2)); m > 0.5 => (2 - 2m, 1).
        for (m, want_dry, want_wet) in [
            (0.0, 1.0, 0.0),
            (0.25, 1.0, 0.5f32.powf(1.5)),
            (0.5, 1.0, 1.0),
            (0.75, 0.5, 1.0),
            (1.0, 0.0, 1.0),
        ] {
            let (dry, wet) = mix_gains(m);
            assert!((dry - want_dry).abs() < 1e-5, "dry({m}): {dry}");
            assert!((wet - want_wet).abs() < 1e-5, "wet({m}): {wet}");
        }

        // Feedback: gated off below 0.1 ms, else 0.5 * sqrt(F), and audibly lengthens tail.
        assert_eq!(feedback_gain(1.0, 0.05), 0.0);
        assert!((feedback_gain(0.25, 80.0) - 0.25).abs() < 1e-6);
        assert!((feedback_gain(1.0, 80.0) - 0.5).abs() < 1e-6);
        let mut p_fb = INIT;
        p_fb[5] = 0.5;
        let base_s = raum(p_fb).seconds;
        p_fb[10] = 0.8;
        assert!(raum(p_fb).seconds > base_s * 1.5);

        // Freeze, sparse, synced predelay, and live mix all act.
        let mut p_freeze = INIT;
        p_freeze[11] = 1.0;
        let frz = raum(p_freeze);
        assert!(frz.seconds >= 999.0 && frz.damping == 0.0);

        let mut p_sparse = INIT;
        p_sparse[12] = 1.0;
        assert!(raum(p_sparse).diffusion < raum(INIT).diffusion);

        let mut p_sync = INIT;
        p_sync[5] = 0.0;
        p_sync[14] = 12.0; // 1/8 note = 0.5 beat -> 250 ms at 120 BPM
        assert!((raum(p_sync).predelay_ms - 250.0).abs() < 1e-3);

        // Saved record decode: carries feedback, freeze, sparse, sync, mix with zero lost.
        let mut raw = [0u8; 67];
        raw[4..8].copy_from_slice(&0.5f32.to_le_bytes()); // predelay = 80 ms
        raw[8..12].copy_from_slice(&0.25f32.to_le_bytes()); // feedback = 0.25
        raw[20..24].copy_from_slice(&0.75f32.to_le_bytes()); // reverb mix = 0.75
        raw[52] = 1; // freeze
        raw[53] = 1; // sparse
        raw[54..58].copy_from_slice(&0.5f32.to_le_bytes()); // 1/8 beat sync
        let (knobs, mix, lost) = saved(0x10, &raw).expect("valid Raum record");
        assert!((knobs[5] - 0.5).abs() < 1e-6);
        assert!((knobs[10] - 0.25).abs() < 1e-6);
        assert_eq!(knobs[11], 1.0);
        assert_eq!(knobs[12], 1.0);
        assert!((mix - 0.75).abs() < 1e-6);
        assert_eq!(knobs[14], 12.0);
        assert!(lost.is_empty(), "all Raum controls wired: {lost:?}");
    }
}
