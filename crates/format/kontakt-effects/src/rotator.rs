//! Rotator (module `0x22`, saved v`0x50`) onto the engine's two-rotor rotary
//! speaker ([`ir::Rotary`]).
//!
//! Saved record (24 bytes, six LE floats, [ref]):
//! - `0`: `speed` (`0..=1`, `> 0.5` selects fast rotor speed)
//! - `4`: `balance` (`0..=1`, drum-to-horn balance)
//! - `8`: `accel_hi` (`0..=1`, horn acceleration responsiveness)
//! - `12`: `accel_lo` (`0..=1`, drum acceleration responsiveness)
//! - `16`: `distance` (`0..=1`, microphone distance / stereo spread)
//! - `20`: `mix` (`0..=1`, cubic wet law `x^3`, matching `RotatorMixDb` readout)
//!
//! Measured rotor rates (Kontakt 8.13.1, [ref]): slow horn ~0.81 Hz, fast horn
//! ~6.09 Hz; drum ~0.68 Hz slow / ~5.40 Hz fast; 800 Hz crossover.

use kontra_ir as ir;

/// Saved fields of a Kontakt Rotator slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotator {
    pub speed: f32,
    pub balance: f32,
    pub accel_hi: f32,
    pub accel_lo: f32,
    pub distance: f32,
    pub mix: f32,
}

/// Fresh Rotator defaults.
pub const INIT: Rotator = Rotator {
    speed: 0.0,
    balance: 0.5,
    accel_hi: 0.5,
    accel_lo: 0.5,
    distance: 0.3,
    mix: 1.0,
};

const HORN_SLOW_HZ: f32 = 0.806;
const HORN_FAST_HZ: f32 = 6.09;
const DRUM_SLOW_HZ: f32 = 0.68;
const DRUM_FAST_HZ: f32 = 5.40;
const CROSSOVER_HZ: f32 = 800.0;

#[inline]
fn f32_le(public: &[u8], offset: usize) -> Option<f32> {
    let bytes: [u8; 4] = public.get(offset..offset + 4)?.try_into().ok()?;
    Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
}

/// Decodes a saved Rotator record (`0x22`, version `0x50`).
#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<Rotator> {
    if version != 0x50 {
        return None;
    }
    Some(Rotator {
        speed: f32_le(public, 0)?,
        balance: f32_le(public, 4)?,
        accel_hi: f32_le(public, 8)?,
        accel_lo: f32_le(public, 12)?,
        distance: f32_le(public, 16)?,
        mix: f32_le(public, 20)?,
    })
}

/// Translates Kontakt Rotator fields into format-neutral [`ir::Rotary`].
#[must_use]
pub fn rotary(r: Rotator) -> ir::Rotary {
    let mix_u = r.mix.clamp(0.0, 1.0);
    ir::Rotary {
        fast: r.speed > 0.5,
        horn_slow_hz: HORN_SLOW_HZ,
        horn_fast_hz: HORN_FAST_HZ,
        drum_slow_hz: DRUM_SLOW_HZ,
        drum_fast_hz: DRUM_FAST_HZ,
        accel_hi: r.accel_hi.clamp(0.0, 1.0),
        accel_lo: r.accel_lo.clamp(0.0, 1.0),
        crossover_hz: CROSSOVER_HZ,
        balance: r.balance.clamp(0.0, 1.0),
        distance: r.distance.clamp(0.0, 1.0),
        mix: (mix_u * mix_u * mix_u).clamp(0.0, 1.0),
    }
}

/// Single-encoding module descriptor for Rotator (`0x22`).
pub static MODULE: crate::module::Module = crate::module::Module {
    id: crate::module::ids::ROTATOR,
    module_id: 0x22,
    versions: (0x50, 0x50),
    name: "Rotator",
    script_type: 22,
    mix: crate::module::MixMode::OwnMix,
    knobs: crate::module::KNOBS.split_at(6).0,
    build: |p| {
        let [speed, balance, accel_hi, accel_lo, distance, mix] = *p else {
            return None;
        };
        Some(ir::EffectKind::Rotary(rotary(Rotator {
            speed,
            balance,
            accel_hi,
            accel_lo,
            distance,
            mix,
        })))
    },
};

/// Builds a serializable [`ir::KnobModel`] for Rotator.
#[must_use]
pub fn model(r: Rotator) -> ir::KnobModel {
    MODULE.knob_model(&[
        r.speed, r.balance, r.accel_hi, r.accel_lo, r.distance, r.mix,
    ])
}

/// Fresh `$EFFECT_TYPE_ROTATOR` (22) slot descriptor for live KSP loading.
#[must_use]
pub fn slot_type() -> ir::SlotType {
    let effect = ir::Effect {
        kind: ir::EffectKind::Rotary(rotary(INIT)),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    ir::SlotType {
        id: 22,
        subtype: false,
        reads: 0,
        kind: 22,
        effect: Some(effect),
        laws: [].into(),
        model: Some(model(INIT)),
    }
}

/// KSP `$ENGINE_PAR_RT_*` bindings for Rotator.
#[must_use]
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 6] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    [
        ("$ENGINE_PAR_RT_SPEED", Slot(Knob(0)), unit()),
        ("$ENGINE_PAR_RT_BALANCE", Slot(Knob(1)), unit()),
        ("$ENGINE_PAR_RT_ACCEL_HI", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_RT_ACCEL_LO", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_RT_DISTANCE", Slot(Knob(4)), unit()),
        ("$ENGINE_PAR_RT_MIX", Slot(Knob(5)), unit()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotator_binary_round_trip_and_laws() {
        let mut public = Vec::new();
        for v in [1.0f32, 0.75, 0.6, 0.4, 0.2, 0.5] {
            public.extend(v.to_le_bytes());
        }
        assert!(saved(0x10, &public).is_none());
        let decoded = saved(0x50, &public).expect("valid v0x50 rotator");
        let ir = rotary(decoded);
        assert!(ir.fast);
        assert!((ir.balance - 0.75).abs() < 1e-6);
        assert!((ir.mix - 0.125).abs() < 1e-6, "cubic mix law 0.5^3 = 0.125");
        let m = model(decoded);
        assert_eq!(m.build(&m.saved), Some(ir::EffectKind::Rotary(ir)));
    }
}
