//! Vibrato/Chorus (module 0x69, saved v0x10) onto the engine's swept delay
//! ([`ir::Delay`]). The saved record: DEPTH and COLOR as i32 at 0 and 4,
//! BLEND, RATE, WIDTH and MIX as floats at 8..24, then a byte (1 in every
//! factory preset).
//!
//! The core, read from Kontakt 8.13.1 (HammondVibratoChorus, ctor
//! [ref], process [ref]; docs/re/engine/fx-measured/
//! psyche-vibrato.md) as [`ir::Scanner`]:
//! - both channels' sum is split at 7.1 kHz (a Butterworth state-variable
//!   pair); the band below runs down a wave-digital LC ladder, half the band
//!   above passes around it;
//! - the ladder: 0.5 H and 4 nF sections (1 nF the last), 18 for DEPTH I..III
//!   and 36 for IV..VI, resistive dividers to ground at its first nodes, and
//!   COLOR's load at its end, `15000·10^(4.5u − 2.25)` Ω: `u` ½ at COLOR 0
//!   (15 kΩ), 0.3667 at 1 (3.8 kΩ), 0.6333 at 2 (60 kΩ);
//! - a pickup turns at `6·2^(5·RATE − 3)` Hz through 16 stations, out across
//!   DEPTH's nine taps and back, crossfading neighbours; the right one
//!   `WIDTH/2` turns ahead;
//! - MIX blends the input against it.
//!
//! BLEND is not in the core's process: taken (as measured, a chorus at
//! BLEND 1) as the line's input mixed into the pickup's signal, half each
//! at 1. The core's first continuous parameter (the source's resistance,
//! `11000·p²` Ω, and COLOR 1 and 2's load) stays at its fresh 1e-6.

use kontra_ir as ir;

/// The saved fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vibrato {
    pub depth: i32,
    pub color: i32,
    pub blend: f32,
    pub rate: f32,
    pub width: f32,
    pub mix: f32,
}

/// A fresh Vibrato/Chorus (its INIT preset; Kontakt 8.13.1 reads the same
/// back from a script-loaded one).
pub const INIT: Vibrato = Vibrato {
    depth: 0,
    color: 0,
    blend: 0.0,
    rate: 0.6,
    width: 0.0,
    mix: 1.0,
};

/// The nodes DEPTH I..VI's pickup crosses (I..III on the short line).
const TAPS: [[u8; 9]; 6] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8],
    [0, 1, 2, 4, 6, 8, 10, 11, 12],
    [0, 1, 3, 6, 9, 12, 15, 17, 18],
    [0, 1, 3, 7, 12, 17, 21, 23, 24],
    [0, 1, 4, 9, 15, 21, 26, 29, 30],
    [0, 2, 6, 12, 20, 28, 30, 34, 36],
];
/// The dividers to ground at nodes 1..5 (27k+68k at the input, unheard).
const SHUNT_OHM: [f32; 6] = [206e3, 189e3, 213e3, 198e3, 192e3, f32::INFINITY];

/// COLOR's load at the line's end, Ω.
fn load_ohm(color: i32) -> f32 {
    const P: f32 = 1e-6;
    let u = match color {
        1 => 0.366_666 * (1.0 - P),
        2 => 0.366_666 * P + 0.633_333,
        _ => 0.5,
    };
    15_000.0 * 10f32.powf(4.5 * u - 2.25)
}

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<Vibrato> {
    if version != 0x10 {
        return None;
    }
    let f = |n: usize| {
        let bytes = public.get(n..n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    let i = |n: usize| Some(i32::from_le_bytes(public.get(n..n + 4)?.try_into().ok()?));
    Some(Vibrato {
        depth: i(0)?,
        color: i(4)?,
        blend: f(8)?,
        rate: f(12)?,
        width: f(16)?,
        mix: f(20)?,
    })
}

#[must_use]
pub fn scanner(v: Vibrato) -> ir::Scanner {
    let d = usize::try_from(v.depth.clamp(0, 5)).unwrap_or(0);
    ir::Scanner {
        sections: if d < 3 { 18 } else { 36 },
        henry: 0.5,
        farad: 4e-9,
        last_farad: 1e-9,
        load_ohm: load_ohm(v.color),
        shunt_ohm: SHUNT_OHM,
        taps: TAPS.get(d).copied().unwrap_or(TAPS[0]),
        hz: 6.0 * 2f32.powf(5.0 * v.rate.clamp(0.0, 1.0) - 3.0),
        lead: 0.5 * v.width.clamp(0.0, 1.0),
        split_hz: 7100.0,
        high: 0.5,
        blend: v.blend.clamp(0.0, 1.0),
        mix: v.mix.clamp(0.0, 1.0),
    }
}

/// The record as a script moves it: every knob move rebuilds the scanner
/// through [`scanner`].
#[must_use]
pub fn model(v: Vibrato) -> ir::KnobModel {
    use ir::SlotKnob::Knob;
    ir::KnobModel {
        knobs: (0..6).map(Knob).collect(),
        saved: [
            v.depth as f32,
            v.color as f32,
            v.blend,
            v.rate,
            v.width,
            v.mix,
        ]
        .into(),
        build: |p| {
            let [depth, color, blend, rate, width, mix] = *p else {
                return None;
            };
            Some(ir::EffectKind::Scanner(scanner(Vibrato {
                depth: depth.round() as i32,
                color: color.round() as i32,
                blend,
                rate,
                width,
                mix,
            })))
        },
    }
}

/// The KSP names of [`model`]'s knobs: DEPTH and COLOR are indices (the
/// raw value), the rest saved as set.
#[must_use]
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 6] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let index = || ir::Law::Linear {
        lo: 0.0,
        hi: 1_000_000.0,
    };
    [
        ("$ENGINE_PAR_VC_DEPTH", Slot(Knob(0)), index()),
        ("$ENGINE_PAR_VC_COLOR", Slot(Knob(1)), index()),
        ("$ENGINE_PAR_VC_BLEND", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_VC_RATE", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_VC_WIDTH", Slot(Knob(4)), unit()),
        ("$ENGINE_PAR_VC_MIX", Slot(Knob(5)), unit()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The INIT preset's record (`- INIT Vibrato-Chorus -.nkp`).
    #[test]
    fn the_init_record_reads_as_the_fresh_unit() {
        let mut public = [0u8, 0, 0, 0, 0, 0, 0, 0].to_vec();
        for v in [0.0f32, 0.6, 0.0, 1.0] {
            public.extend(v.to_le_bytes());
        }
        public.push(1);
        assert_eq!(saved(0x10, &public), Some(INIT));
        let s = scanner(INIT);
        assert_eq!((s.hz, s.load_ohm.round(), s.sections), (6.0, 15_000.0, 18));
        // COLOR 1 loads the line below its 11 kΩ, COLOR 2 above.
        let load = |color| scanner(Vibrato { color, ..INIT }).load_ohm;
        assert!((load(1) - 3768.0).abs() < 2.0 && (load(2) - 59_716.0).abs() < 30.0);
    }
}
