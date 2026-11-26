//! Ring Modulator (module `0x64`, saved v`0x50`, `$EFFECT_TYPE_RINGMOD` = 50)
//! onto [`ir::RingMod`] with [`ir::RingSource::Modulated`].
//!
//! Saved record (45 bytes, `BFx4IFx5B`):
//! - `+0` (`u8`): `fast_mode` (`$ENGINE_PAR_RINGMOD_FAST_MODE`, bool)
//! - `+1` (`f32`): `ring` (`$ENGINE_PAR_RINGMOD_RING`, `0..=1`)
//! - `+5` (`f32`): `fm` (`$ENGINE_PAR_RINGMOD_FM`, `0..=1`)
//! - `+9` (`f32`): `frequency` (`$ENGINE_PAR_RINGMOD_FREQUENCY`, `0..=1`)
//! - `+13` (`f32`): `edge` (`$ENGINE_PAR_RINGMOD_EDGE`, `0..=1`)
//! - `+17` (`i32`): `lfo_wave` (`$ENGINE_PAR_RINGMOD_LFO_WAVE`, 0 = sine, 1 = square)
//! - `+21` (`f32`): `lfo_rate` (`$ENGINE_PAR_RINGMOD_LFO_RATE`, `0..=1`)
//! - `+25` (`f32`): `lfo_amount` (`$ENGINE_PAR_RINGMOD_LFO_AMOUNT`, `0..=1`)
//! - `+29` (`f32`): `sync_beats` (quarter-note beats when tempo-synced, `<= 0` free)
//!
//! Laws:
//! - Carrier frequency: `0.625 * 128^frequency * (50 if fast_mode else 1)` Hz
//!   (`0.625..80` Hz slow, `31.25..4000` Hz fast);
//! - LFO amount (`a2 = amount^2`): `num = (0.1476 * a2 + 0.5342) * a2 + 1`,
//!   `den = (0.1288 * a2 - 0.5342) * a2 + 1`, carrier base scales by `den`,
//!   modulation depth `2 * (num / den - 1)`;
//! - LFO rate (free): `0.03125 * 1024^lfo_rate` Hz (`0.03125..32` Hz);
//! - Ring mix: `dry = 1 - 2.6 * ring`, `wet = 3.2 * ring` against `[0, 1]` carrier
//!   (equivalent to `(1 - ring) * x + 1.6 * ring * bipolar_carrier * x`);
//! - FM delay swing: `1.28 * fm^2` ms.

use kontra_ir as ir;

use crate::module::{RAW_LAW, UNIT_LAW, f32_le, i32_le, knob_row, sync_beats, sync_unit};

/// Saved fields in record order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingModulator {
    pub fast_mode: bool,
    pub ring: f32,
    pub fm: f32,
    pub frequency: f32,
    pub edge: f32,
    pub lfo_wave: i32,
    pub lfo_rate: f32,
    pub lfo_amount: f32,
    /// `$NI_SYNC_UNIT_*` index (0 = free Hz).
    pub lfo_unit: i32,
    /// Quarter-note beats per cycle when saved synced (`<= 0` = free).
    pub sync_beats: f32,
}

/// Fresh INIT defaults for a script-loaded Ring Modulator.
pub const INIT: RingModulator = RingModulator {
    fast_mode: false,
    ring: 0.5,
    fm: 0.0,
    frequency: 0.5,
    edge: 0.0,
    lfo_wave: 0,
    lfo_rate: 0.5,
    lfo_amount: 0.0,
    lfo_unit: 0,
    sync_beats: -1.0,
};

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<RingModulator> {
    if version != 0x50 {
        return None;
    }
    let sync = f32_le(public, 29).unwrap_or(-1.0);
    Some(RingModulator {
        fast_mode: *public.first()? != 0,
        ring: f32_le(public, 1)?,
        fm: f32_le(public, 5)?,
        frequency: f32_le(public, 9)?,
        edge: f32_le(public, 13)?,
        lfo_wave: i32_le(public, 17)?,
        lfo_rate: f32_le(public, 21)?,
        lfo_amount: f32_le(public, 25)?,
        lfo_unit: sync_unit(sync),
        sync_beats: if sync > 1e-9 { sync } else { -1.0 },
    })
}

/// Physical IR settings for `r`.
#[must_use]
pub fn ringmod(r: RingModulator) -> ir::RingMod {
    let ring = r.ring.clamp(0.0, 1.0);
    let fm = r.fm.clamp(0.0, 1.0);
    let freq = r.frequency.clamp(0.0, 1.0);
    let edge = r.edge.clamp(0.0, 1.0);
    let lfo_rate = r.lfo_rate.clamp(0.0, 1.0);
    let amt = r.lfo_amount.clamp(0.0, 1.0);

    let mult = if r.fast_mode { 50.0 } else { 1.0 };
    let raw_hz = 0.625 * (7.0 * freq).exp2() * mult;

    let a2 = amt * amt;
    let num = (0.1476 * a2 + 0.5342) * a2 + 1.0;
    let den = (0.1288 * a2 - 0.5342) * a2 + 1.0;
    let ratio = num / den.max(1e-4);
    let base_hz = raw_hz * den.max(1e-4);
    let lfo_depth = (2.0 * (ratio - 1.0)).max(0.0);

    let synced_beats =
        sync_beats(r.lfo_unit).or_else(|| (r.sync_beats > 1e-9).then_some(r.sync_beats));
    let rate = match synced_beats {
        Some(beats) => ir::Rate::Beats(beats),
        None => ir::Rate::Hz(0.031_25 * (10.0 * lfo_rate).exp2()),
    };

    ir::RingMod {
        hz: base_hz,
        spread: 0.0,
        bias: 0.0,
        quadrature: 0.0,
        drive: 1.0,
        highpass_hz: 0.0,
        gate: false,
        feedback: 0.0,
        source: ir::RingSource::Modulated {
            edge,
            fm_ms: 1.28 * fm * fm,
            lfo_rate: rate,
            lfo_depth,
            lfo_square: r.lfo_wave != 0,
        },
        wet: 3.2 * ring,
        dry: 1.0 - 2.6 * ring,
    }
}

/// Single-encoding module descriptor for Ring Modulator (`0x64`).
pub static MODULE: crate::module::Module = crate::module::Module {
    id: crate::module::ids::RINGMOD,
    module_id: 0x64,
    versions: (0x50, 0x50),
    name: "Ring Modulator",
    script_type: 50,
    mix: crate::module::MixMode::InPlace,
    knobs: crate::module::KNOBS.split_at(9).0,
    build: |v| {
        let [
            fast_mode,
            ring,
            fm,
            frequency,
            edge,
            lfo_wave,
            lfo_rate,
            lfo_amount,
            lfo_unit,
            sync_beats,
        ] = *v
        else {
            return None;
        };
        Some(ir::EffectKind::RingMod(ringmod(RingModulator {
            fast_mode: fast_mode > 0.0,
            ring,
            fm,
            frequency,
            edge,
            lfo_wave: lfo_wave.round() as i32,
            lfo_rate,
            lfo_amount,
            lfo_unit: lfo_unit.round() as i32,
            sync_beats,
        })))
    },
};

#[must_use]
pub fn model(r: RingModulator) -> ir::KnobModel {
    MODULE.knob_model(&[
        if r.fast_mode { 1.0 } else { 0.0 },
        r.ring,
        r.fm,
        r.frequency,
        r.edge,
        r.lfo_wave as f32,
        r.lfo_rate,
        r.lfo_amount,
        r.lfo_unit as f32,
        r.sync_beats,
    ])
}

/// KSP `$ENGINE_PAR_RINGMOD_*` knob rows.
#[must_use]
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 9] {
    [
        knob_row("$ENGINE_PAR_RINGMOD_FAST_MODE", 0, RAW_LAW),
        knob_row("$ENGINE_PAR_RINGMOD_RING", 1, UNIT_LAW),
        knob_row("$ENGINE_PAR_RINGMOD_FM", 2, UNIT_LAW),
        knob_row("$ENGINE_PAR_RINGMOD_FREQUENCY", 3, UNIT_LAW),
        knob_row("$ENGINE_PAR_RINGMOD_EDGE", 4, UNIT_LAW),
        knob_row("$ENGINE_PAR_RINGMOD_LFO_WAVE", 5, RAW_LAW),
        knob_row("$ENGINE_PAR_RINGMOD_LFO_RATE", 6, UNIT_LAW),
        knob_row("$ENGINE_PAR_RINGMOD_LFO_AMOUNT", 7, UNIT_LAW),
        knob_row("$ENGINE_PAR_RINGMOD_LFO_RATE_UNIT", 8, RAW_LAW),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_decodes_and_carrier_and_lfo_laws_match_endpoints() {
        let mut bytes = vec![1u8]; // fast_mode = true
        for v in [0.5f32, 0.5, 1.0, 0.25] {
            bytes.extend(v.to_le_bytes());
        }
        bytes.extend(1i32.to_le_bytes()); // square LFO
        for v in [0.0f32, 0.0, -1.0, 0.0, 0.0] {
            bytes.extend(v.to_le_bytes());
        }
        bytes.push(0);
        let r = saved(0x50, &bytes).unwrap();
        assert!(r.fast_mode);
        assert_eq!(r.lfo_wave, 1);

        let rm = ringmod(r);
        // Fast mode at freq=1.0 -> 4000 Hz.
        assert!((rm.hz - 4000.0).abs() < 1e-2, "fast hz {}", rm.hz);
        // Ring=0.5 -> dry=-0.3, wet=1.6 -> range [-0.3..1.3] = 0.5*(dry) +/- 0.8*(carrier)
        assert!((rm.dry - -0.3).abs() < 1e-5);
        assert!((rm.wet - 1.6).abs() < 1e-5);
        match rm.source {
            ir::RingSource::Modulated {
                edge,
                fm_ms,
                lfo_rate,
                lfo_depth,
                lfo_square,
            } => {
                assert!((edge - 0.25).abs() < 1e-6);
                assert!((fm_ms - 0.32).abs() < 1e-5);
                assert_eq!(lfo_rate, ir::Rate::Hz(0.031_25));
                assert!(lfo_depth.abs() < 1e-6);
                assert!(lfo_square);
            }
            _ => panic!("expected Modulated source"),
        }

        // Slow mode at freq=0.0 -> 0.625 Hz; freq=1.0 -> 80 Hz.
        let slow0 = ringmod(RingModulator {
            fast_mode: false,
            frequency: 0.0,
            ..INIT
        });
        assert!((slow0.hz - 0.625).abs() < 1e-4);
        let slow1 = ringmod(RingModulator {
            fast_mode: false,
            frequency: 1.0,
            ..INIT
        });
        assert!((slow1.hz - 80.0).abs() < 1e-3);
    }

    #[test]
    fn knob_model_roundtrips() {
        let m = model(INIT);
        assert_eq!(
            m.build(&m.saved),
            Some(ir::EffectKind::RingMod(ringmod(INIT)))
        );
    }
}
