//! Wow/Flutter (`$EFFECT_TYPE_WOWFLUTTER`, 56) onto a modulated
//! [`ir::Delay`]: a tape's pitch wobble, scrape flutter, saturation, noise
//! gate, and live wet/dry mix.
//!
//! Measured at INIT in Kontakt 8.13.1 (scratch/laneDSP/b9): a pitch wobble
//! of about 7.6 cents rms at 1.4..1.7 Hz. A sine swing of `D` s at `f` Hz
//! bends pitch by `2πfD`, so 0.7 ms at 1.5 Hz. Wow sets that swing, Speed
//! its rate; Flutter adds a fast shallow swing, so a Flutter-led setting
//! runs at ~10 Hz instead; Scrape adds fast tape-guide micro-flutter and
//! hiss; Age darkens the tape and raises tape noise; Saturation applies
//! odd soft-clipping in the delay line; Gate switches tape noise from
//! continuous floor to signal-gated; Stereo offsets the right LFO; Mix
//! crossfades wet and dry live.

use kontra_ir as ir;

/// A new Wow/Flutter's knobs, in [`model`]'s order: wow, flutter, scrape,
/// speed, stereo, age, saturation, gate, mix.
pub(crate) const INIT: [f32; 9] = [0.5, 0.3, 0.0, 0.5, 0.5, 0.3, 0.0, 0.0, 1.0];

/// Precomputed symmetric odd soft-saturation curves over `-1..=1` (33 points)
/// at five drive strengths (`0.2, 0.4, 0.6, 0.8, 1.0`), allocation-free on the
/// audio thread.
const fn tanh_approx(x: f32) -> f32 {
    // Accurate Padé / rational soft-clip for const table generation.
    let a = if x < 0.0 { -x } else { x };
    x / (1.0 + a + 0.28 * x * x)
}

#[allow(clippy::indexing_slicing)]
const fn sat_curve(drive: f32) -> [f32; 33] {
    let mut out = [0.0f32; 33];
    let norm = tanh_approx(drive);
    let mut i = 0;
    while i < 33 {
        let x = -1.0 + (i as f32) / 16.0;
        out[i] = if norm > 1e-6 {
            tanh_approx(drive * x) / norm
        } else {
            x
        };
        i += 1;
    }
    out
}

static SAT_TABLES: [[f32; 33]; 5] = [
    sat_curve(1.0),
    sat_curve(2.2),
    sat_curve(4.0),
    sat_curve(6.5),
    sat_curve(10.0),
];

fn saturation_shape(sat: f32) -> &'static [f32] {
    let s = sat.clamp(0.0, 1.0);
    if s < 0.05 {
        return &[];
    }
    let [t0, t1, t2, t3, t4] = &SAT_TABLES;
    match (s * 5.0 - 0.5).floor() as usize {
        0 => t0,
        1 => t1,
        2 => t2,
        3 => t3,
        _ => t4,
    }
}

/// The delay at knob positions `p` (see [`INIT`]).
pub(crate) fn wow_flutter(p: [f32; 9]) -> ir::Delay {
    let [
        wow,
        flutter,
        scrape,
        speed,
        stereo,
        age,
        saturation,
        gate,
        _mix,
    ] = p.map(|x| x.clamp(0.0, 1.0));
    let wow_ms = 1.4 * wow;
    let flutter_ms = 0.1 * flutter;
    let scrape_ms = 0.035 * scrape;
    // One LFO: the largest pitch bend (2πfD) sets the dominant modulation rate.
    let wow_hz = 1.5 * 4f32.powf(speed - 0.5);
    let flutter_hz = 10.0 * 2f32.powf(speed - 0.5);
    let scrape_hz = 28.0 * 1.5f32.powf(speed - 0.5);
    let wow_bend = wow_ms * wow_hz;
    let flutter_bend = flutter_ms * flutter_hz;
    let scrape_bend = scrape_ms * scrape_hz;
    let (depth_ms, hz) = if wow_bend >= flutter_bend && wow_bend >= scrape_bend {
        (wow_ms + flutter_ms + scrape_ms, wow_hz)
    } else if flutter_bend >= scrape_bend {
        (
            flutter_ms + wow_ms * wow_hz / flutter_hz + scrape_ms,
            flutter_hz,
        )
    } else {
        (scrape_ms + (wow_bend + flutter_bend) / scrape_hz, scrape_hz)
    };
    let noise_level = 0.0025 * scrape + 0.0008 * age * age;
    // Gate off -> constant tape hiss floor (`rms^0`); gate on -> signal-gated (`rms^1`).
    let noise = if noise_level > 1e-6 {
        [noise_level, if gate >= 0.5 { 1.0 } else { 0.0 }]
    } else {
        [0.0; 2]
    };
    ir::Delay {
        direct: 0.0,
        time: ir::DelayTime::Ms(5.0),
        depth_ms,
        second_head: scrape >= 0.35,
        input: ir::IDENTITY,
        feedback: [[0.0; 2]; 2],
        output: ir::IDENTITY,
        damping_hz: 18_000.0 * (3_000.0f32 / 18_000.0).powf(age),
        low_cut_hz: 0.0,
        steep_low_cut: false,
        body: std::borrow::Cow::Borrowed(&[]),
        triangle_s: None,
        shape: std::borrow::Cow::Borrowed(saturation_shape(saturation)),
        noise,
        high_shelf: None,
        spread_ms: 0.0,
        rate: ir::Rate::Hz(hz),
        phase: 0.25 * stereo,
    }
}

/// Decode a saved `BParFXWowFlutter` (`0x6a`, v0x10, 33 bytes) into [`INIT`]'s
/// knob order (`Kontakt 8.exe` [ref] and
/// `fx_bite_dirt_freak_wowflutter_twindelay.tsv:88-96`).
pub(crate) fn saved(version: u16, public: &[u8]) -> Option<([f32; 9], f32)> {
    if version != 0x10 || public.len() < 33 {
        return None;
    }
    let f = |offset: usize| {
        let bytes = public.get(offset..offset + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    let wow = f(0)?;
    let flutter = f(4)?;
    let scrape = f(8)?;
    let speed = f(12)?;
    let stereo = f(16)?;
    let age = f(20)?;
    let saturation = f(24)?;
    let gate = f32::from(u8::from(*public.get(28)? != 0));
    let mix = f(29)?;
    Some((
        [
            wow, flutter, scrape, speed, stereo, age, saturation, gate, mix,
        ],
        mix,
    ))
}

/// Custom [`ir::EffectModel`] so live `$ENGINE_PAR_WOWFLUTTER_MIX` moves update
/// the slot's wet/dry balance (`output` and `dry`) on the audio thread.
struct WowFlutterModel;

impl ir::EffectModel for WowFlutterModel {
    fn build(&self, params: &[f32]) -> Option<ir::EffectKind> {
        Some(ir::EffectKind::Delay(wow_flutter(params.try_into().ok()?)))
    }

    fn build_effect(&self, params: &[f32], current: &ir::Effect) -> Option<ir::Effect> {
        let arr: [f32; 9] = params.try_into().ok()?;
        let kind = ir::EffectKind::Delay(wow_flutter(arr));
        let mix = arr[8].clamp(0.0, 1.0);
        let total = current.output + current.dry;
        let gain = if total > 1e-6 { total } else { 1.0 };
        Some(ir::Effect {
            kind,
            bypass: current.bypass,
            output: gain * mix,
            dry: gain * (1.0 - mix),
        })
    }
}

static EFFECT_MODEL: WowFlutterModel = WowFlutterModel;

/// Single-encoding module descriptor for Wow/Flutter (`0x6a`).
pub(crate) static MODULE: kontra_kontakt_effects::module::Module =
    kontra_kontakt_effects::module::Module {
        id: kontra_kontakt_effects::module::ids::WOWFLUTTER,
        module_id: 0x6a,
        versions: (0x10, 0x10),
        name: "Wow/Flutter",
        script_type: 56,
        mix: kontra_kontakt_effects::module::MixMode::WetDry,
        knobs: &[
            ir::SlotKnob::Knob(0),
            ir::SlotKnob::Knob(1),
            ir::SlotKnob::Knob(2),
            ir::SlotKnob::Knob(3),
            ir::SlotKnob::Knob(4),
            ir::SlotKnob::Knob(5),
            ir::SlotKnob::Knob(6),
            ir::SlotKnob::Knob(7),
            ir::SlotKnob::Mix,
        ],
        build: |p| Some(ir::EffectKind::Delay(wow_flutter(p.try_into().ok()?))),
    };

/// A Wow/Flutter's knobs from saved positions as a script moves them.
pub(crate) fn model_from(saved: [f32; 9]) -> ir::KnobModel {
    ir::register_model(MODULE.id, &EFFECT_MODEL);
    ir::KnobModel {
        id: MODULE.id,
        knobs: MODULE.knobs.into(),
        saved: saved.into(),
    }
}

/// A Wow/Flutter's knobs as a script moves them: every move rebuilds it.
pub(crate) fn model() -> ir::KnobModel {
    model_from(INIT)
}

/// What `$EFFECT_TYPE_WOWFLUTTER` (56) loads: [`INIT`], fully wet.
pub(crate) fn slot_type() -> ir::SlotType {
    let effect = ir::Effect {
        kind: ir::EffectKind::Delay(wow_flutter(INIT)),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    ir::SlotType {
        id: 56,
        subtype: false,
        reads: 0,
        kind: 56,
        effect: Some(effect),
        laws: [].into(),
        model: Some(model()),
    }
}

/// The KSP names of [`model`]'s knobs, all 9 wired live.
pub(crate) fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 9] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let raw = || ir::Law::Linear { lo: 0.0, hi: 1e6 };
    [
        ("$ENGINE_PAR_WOWFLUTTER_WOW", Slot(Knob(0)), unit()),
        ("$ENGINE_PAR_WOWFLUTTER_FLUTTER", Slot(Knob(1)), unit()),
        ("$ENGINE_PAR_WOWFLUTTER_SCRAPE", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_WOWFLUTTER_SPEED", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_WOWFLUTTER_STEREO", Slot(Knob(4)), unit()),
        ("$ENGINE_PAR_WOWFLUTTER_AGE", Slot(Knob(5)), unit()),
        ("$ENGINE_PAR_WOWFLUTTER_SATURATION", Slot(Knob(6)), unit()),
        ("$ENGINE_PAR_WOWFLUTTER_GATE", Slot(Knob(7)), raw()),
        (
            "$ENGINE_PAR_WOWFLUTTER_MIX",
            Slot(ir::SlotKnob::Mix),
            unit(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_wobbles_about_eight_cents_at_one_and_a_half_hertz() {
        let d = wow_flutter(INIT);
        let ir::Rate::Hz(hz) = d.rate else {
            panic!("Hz")
        };
        // Peak ratio 2πfD; rms cents = 1200·log2(1 + peak)/√2.
        let peak = std::f32::consts::TAU * hz * d.depth_ms / 1000.0;
        let cents = 1200.0 * (1.0 + peak).log2() / std::f32::consts::SQRT_2;
        assert!(
            (1.4..1.8).contains(&hz) && (6.0..10.0).contains(&cents),
            "{hz} Hz {cents} c"
        );
    }

    #[test]
    fn kt24_scrape_saturation_gate_and_mix_are_live() {
        let mut p = INIT;
        let base = wow_flutter(p);
        assert!(base.shape.is_empty());

        p[6] = 0.8; // saturation
        let sat = wow_flutter(p);
        assert!(!sat.shape.is_empty());

        p[2] = 0.7; // scrape
        let scraped = wow_flutter(p);
        assert!(scraped.second_head);
        assert!(scraped.noise[0] > base.noise[0]);
        assert_eq!(scraped.noise[1], 0.0);

        p[7] = 1.0; // gate on
        let gated = wow_flutter(p);
        assert_eq!(gated.noise[1], 1.0);

        // Live mix updates output/dry via EffectModel::build_effect.
        let m = model();
        let mut live = m.saved.to_vec();
        live[8] = 0.25;
        let current = ir::Effect {
            kind: ir::EffectKind::Delay(base),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        };
        let built = m.build_effect(&live, &current).expect("built");
        assert!((built.output - 0.25).abs() < 1e-6);
        assert!((built.dry - 0.75).abs() < 1e-6);
    }
}
