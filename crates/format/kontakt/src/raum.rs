//! Raum (`$EFFECT_TYPE_RAUM`, 51) onto the generic [`ir::Reverb`] network.
//!
//! Good-enough laws, not yet measured against Kontakt: the three types
//! (Grounded, Airy, Cosmic) set the room's base size; decay runs 0.2..20 s
//! exponentially, predelay 0..500 ms on a square curve, high cut 1..90 kHz
//! (clamped by the engine), low shelf 0..−24 dB, modulation 0..2 ms at
//! 0.1..5 Hz. Feedback, Freeze, Sparse and the predelay's tempo sync are not
//! played.

use kontra_ir as ir;

/// A new Raum's knobs, in [`model`]'s order: type, decay, size, damping,
/// diffusion, predelay, high cut, low shelf, modulation, rate.
pub(crate) const INIT: [f32; 10] = [0.0, 0.5, 0.5, 0.5, 0.8, 0.0, 0.8, 0.0, 0.3, 0.3];

/// Each type's base size: Grounded, Airy, Cosmic.
const BASE: [f32; 3] = [0.7, 1.2, 2.0];

/// The reverb at knob positions `p` (see [`INIT`]).
pub(crate) fn raum(p: [f32; 10]) -> ir::Reverb {
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
    ] = p;
    let [
        decay,
        size,
        damping,
        diffusion,
        predelay,
        high_cut,
        low_shelf,
        modulation,
        rate,
    ] = [
        decay, size, damping, diffusion, predelay, high_cut, low_shelf, modulation, rate,
    ]
    .map(|x| x.clamp(0.0, 1.0));
    let base = BASE
        .get(kind.round().clamp(0.0, 2.0) as usize)
        .copied()
        .unwrap_or(1.0);
    ir::Reverb {
        seconds: 0.2 * 100f32.powf(decay),
        size: base * 4f32.powf(size - 0.5),
        network: None,
        damping,
        diffusion,
        predelay_ms: 500.0 * predelay * predelay,
        high_cut_hz: 1000.0 * 90f32.powf(high_cut),
        low_shelf_db: -24.0 * low_shelf,
        stereo: 1.0,
        modulation_ms: 2.0 * modulation,
        modulation_hz: 0.1 * 50f32.powf(rate),
    }
}

/// Decode a saved `BParFXRaum` (`0x65`, v0x10, 67 bytes) into [`INIT`]'s knob
/// order plus any unmodeled switches (`Kontakt 8.exe` `0x140ac5cc0` and
/// `fx_raum_delight_psyche_vibrato_phasis_replika.tsv:78-97`).
pub(crate) fn saved(version: u16, public: &[u8]) -> Option<([f32; 10], Vec<&'static str>)> {
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
    let _feedback = f(8)?;
    let low_shelf = f(12)?;
    let high_cut = f(16)?;
    let _reverb = f(20)?;
    let diffusion = f(24)?;
    let decay = f(28)?;
    let modulation = f(32)?;
    let size = f(36)?;
    let damping = f(40)?;
    let rate = f(44)?;
    let freeze = flag(52)?;
    let sparse = flag(53)?;
    let synced = f(54)? > 1e-9;
    let mut lost = Vec::new();
    if freeze {
        lost.push("Raum freeze");
    }
    if sparse {
        lost.push("Raum sparse");
    }
    if synced {
        lost.push("Raum tempo sync");
    }
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
        ],
        lost,
    ))
}

/// A Raum's knobs from saved positions as a script moves them.
pub(crate) fn model_from(saved: [f32; 10]) -> ir::KnobModel {
    ir::KnobModel {
        knobs: (0..10).map(ir::SlotKnob::Knob).collect(),
        saved: saved.into(),
        build: |p| Some(ir::EffectKind::Reverb(raum(p.try_into().ok()?))),
    }
}

/// A Raum's knobs as a script moves them: every move rebuilds it from all.
pub(crate) fn model() -> ir::KnobModel {
    model_from(INIT)
}

/// What `$EFFECT_TYPE_RAUM` (51) loads, as Conflux loads it from its
/// script: [`INIT`], wet over the dry signal, every knob through [`model`].
pub(crate) fn slot_type() -> ir::SlotType {
    let effect = ir::Effect {
        kind: ir::EffectKind::Reverb(raum(INIT)),
        bypass: false,
        output: 0.5,
        dry: 1.0,
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

/// The KSP names of [`model`]'s knobs; type (0..2) is set raw.
pub(crate) fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 10] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let raw = || ir::Law::Linear { lo: 0.0, hi: 1e6 };
    [
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
    ]
}
