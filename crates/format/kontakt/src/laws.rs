//! Kontakt's measured laws, mapped onto the sampler's neutral shapes. Each
//! names the evidence it rests on; the IR and engine never see the vendor.

use kontra_ir as ir;
use ni_file::kontakt::objects::{EnvelopeAhdsr, EnvelopeFlex, Lfo};

/// Kontakt's decay and release: the kernel runs its state from 1.075 down to
/// 0.075, a ratio of 3/43, so the stage is silent exactly at its time
/// (docs/re/engine/k-mod-envelopes.md).
const FALL: f32 = -2.662_588;

/// A volume AHDSR. Stored times are milliseconds; the sustain knob is cubic.
pub(crate) fn envelope(e: &EnvelopeAhdsr) -> ir::Envelope {
    let seconds = |ms: f32| ms.max(0.0) / 1000.0;
    ir::Envelope {
        attack: seconds(e.attack_ms),
        hold: seconds(e.hold_ms),
        decay: seconds(e.decay_ms),
        sustain: e.sustain.clamp(0.0, 1.0).powi(3),
        release: seconds(e.release_ms),
        attack_shape: attack_shape(e.attack_curve),
        fall_shape: FALL,
    }
}

/// The attack curve knob (−1 concave ..= 1 convex) as a segment shape. The
/// engine rounds the base to f32 before taking its log
/// (docs/re/engine/k-mod-envelopes.md).
fn attack_shape(curve: f32) -> f32 {
    let c = f64::from(curve.clamp(-1.0, 1.0));
    let b = f64::from(((1.0 - c.abs()) * 500_000f64.ln() - 20_000f64.ln()).exp() as f32);
    let k = if c > 0.0 {
        (b / (1.0 + b)).ln()
    } else {
        ((1.0 + b) / b).ln()
    };
    k as f32
}

/// The volume law of a modulation source (Kontakt's "family H",
/// modulation compiler 0x140875ac8/0x140873120): cubed for note-time
/// sources (velocity, key, the release counter: external types 7..=13),
/// plain for controllers. The cube at 50/127 matches the measured velocity
/// table to 0.03 dB RMS.
pub(crate) fn volume(source: ir::Source) -> ir::Curve {
    ir::Curve::Depth {
        power: if matches!(source, ir::Source::Controller(_)) {
            1
        } else {
            3
        },
    }
}

/// The release-trigger counter: `max(T − held, 0)/T` in a release-trigger
/// group with a time T, else a constant 1 (binary 0x14084f380).
pub(crate) fn release_counter(release_trigger: bool, ms: i32) -> ir::Source {
    u32::try_from(ms)
        .ok()
        .filter(|&ms| release_trigger && ms > 0)
        .map_or(ir::Source::Constant, |span_ms| ir::Source::HeldTime {
            span_ms,
        })
}

/// Residency by source module (docs/re/engine/k-sampler-source.md): Sampler
/// (mode 0) keeps whole samples in memory; DFD (3) and the other modules keep
/// the program's preload per channel, plus twice the S.Mod range, in 6144-byte
/// grains. ponytail: a Sampler group whose resident flag is clear streams in
/// Kontakt too (0x140760000:19175); that flag is not decoded yet.
pub(crate) fn stream(mode: u32, preload: i32) -> Option<ir::Stream> {
    (mode != 0).then(|| ir::Stream {
        bytes_per_channel: u32::try_from(preload).unwrap_or(61_440),
        grain: 6144,
    })
}

/// Semitones at full intensity of a pitch route. The corpus's pitch-bend
/// routes store 2/12, 7/12 and 12/12 (bend ranges of 2, 7 and 12 semitones).
pub(crate) const PITCH_SEMITONES: f32 = 12.0;

/// One band of the 1/2/3-band EQ. The native coefficients (generator
/// 0x1409591c0) are the cookbook bell with A = 2^(dB/6) and
/// alpha = sin(w)·sinh(octaves·ln 2): twice the knob's dB at the centre and
/// twice its octaves of bandwidth, unwarped (docs/re/engine/eq-kontakt.md
/// §1.1, fitted +12 dB/1 oct → 24.08 dB, Q 0.667).
pub(crate) fn eq_band(hz: f32, octaves: f32, db: f32) -> Option<ir::Bell> {
    if !(hz > 0.0 && octaves > 0.0) {
        return None;
    }
    let peak = (2f32.powf(db.clamp(-18.0, 18.0) / 3.0)).log10() * 20.0;
    Some(ir::Bell {
        hz: hz.min(20_000.0),
        q: 1.0 / (2.0 * (octaves * std::f32::consts::LN_2).sinh()),
        gain_db: peak,
    })
}

/// Convolution Auto Gain (0x1419b2bc0): from the louder channel's energy E,
/// sqrt(0.5 / E) capped at 2; unity at E ≤ 0.001.
pub(crate) const AUTO_GAIN: ir::Normalize = ir::Normalize {
    energy: 0.5,
    max_gain: 2.0,
    floor: 0.001,
};

/// A flex envelope as breakpoints. Each point's curve knob `c` (0.5 linear)
/// sets the segment ratio `q` of the native recurrence (0x140ae5a10), whose
/// closed form is the shape `ln q`. The envelope starts from its last
/// point's level and holds at the sustain point.
pub(crate) fn flex(e: &EnvelopeFlex) -> ir::Breakpoints {
    let points: Vec<_> = e
        .points
        .iter()
        .map(|p| ir::Breakpoint {
            ms: p.time_ms.max(1.0),
            level: p.level.clamp(0.0, 1.0),
            curve: flex_ratio(p.curve).ln(),
        })
        .collect();
    ir::Breakpoints {
        start: points.last().map_or(0.0, |p| p.level),
        sustain: usize::try_from(e.sustain)
            .ok()
            .filter(|&s| s < points.len()),
        points,
    }
}

/// The level ratio across a flex segment for curve knob `c` (0x140ae5a10):
/// `c == 0` is linear; the ends clamp at 0.001 and 0.999.
fn flex_ratio(c: f32) -> f32 {
    let f = if c == 0.0 {
        0.5
    } else if c < 0.001 {
        0.001 * 0.002
    } else if c > 0.999 {
        (4.0 - 1.998) * 0.999 - 1.0
    } else if c <= 0.5 {
        2.0 * c * c
    } else {
        (4.0 - 2.0 * c) * c - 1.0
    };
    (1.0 / f - 1.0).powi(2)
}

/// The amplitude envelope of a group whose volume a flex envelope shapes and
/// no AHDSR does: full level, held through the flex release, then cut.
pub(crate) fn flex_gate(e: &ir::Breakpoints) -> ir::Envelope {
    let release: f32 = e
        .sustain
        .map_or(&[][..], |s| e.points.get(s + 1..).unwrap_or_default())
        .iter()
        .map(|p| p.ms / 1000.0)
        .sum();
    ir::Envelope {
        release: release.max(ir::Envelope::GATE.release),
        // Stays near full until the end of the release, then falls.
        fall_shape: 20.0,
        ..ir::Envelope::GATE
    }
}

/// A free-running (unsynced) LFO. Waveforms by id: sine 0, rectangle 1,
/// triangle 2, sawtooth 3, random 4; the Multi mixes (5, 6) and tempo sync are
/// not mapped. Values: fade-in ms, Hz, pulse width, start phase in cycles.
pub(crate) fn lfo(l: &Lfo, retrigger: bool) -> Option<ir::Lfo> {
    const FREE: f32 = -1.0;
    let [fade_ms, hz, width, phase] = l.initial_values;
    let wave = match l.waveform {
        0 => ir::Wave::Sine,
        1 => ir::Wave::Square { width },
        2 => ir::Wave::Triangle,
        3 => ir::Wave::Saw,
        4 => ir::Wave::Random,
        _ => return None,
    };
    let [rate, fade] = &l.records;
    (rate.values[0] == FREE && fade.values[0] == FREE).then_some(ir::Lfo {
        wave,
        hz,
        phase,
        fade_ms,
        retrigger,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flex_curve_knob_is_linear_at_half_and_bows_either_side() {
        assert!(flex_ratio(0.5).ln().abs() < 1e-6);
        assert!(flex_ratio(0.0).ln().abs() < 1e-6, "zero reads as linear");
        assert!(flex_ratio(0.25).ln() > 0.0);
        assert!(flex_ratio(0.75).ln() < 0.0);
    }
}
