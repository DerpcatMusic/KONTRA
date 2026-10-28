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
        // ponytail: a script value is taken as a controller (linear), not
        // measured; Conflux sends CC × 7874 to it in place of aftertouch.
        power: if matches!(source, ir::Source::Controller(_) | ir::Source::Script(_)) {
            1
        } else {
            3
        },
    }
}

/// A route to an AHDSR attack multiplies its knob `u`, where the time is
/// `2·e^(8.9228·u) − 2` ms (fconv_u2n_env_a, 0x14071a980), by
/// `1 − (1 − x)·i` (0x144f1daa0): a power of `(t + 2)/2`.
pub(crate) const ATTACK_FLOOR_MS: f32 = 2.0;

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

/// An AHDSR as breakpoints, for targets other than the amplitude: the same
/// times, shapes and cubed sustain as [`envelope`].
pub(crate) fn ahdsr(e: &EnvelopeAhdsr) -> ir::Breakpoints {
    let a = envelope(e);
    let point = |s: f32, level, curve| ir::Breakpoint {
        ms: s * 1000.0,
        level,
        curve,
    };
    ir::Breakpoints {
        start: 0.0,
        points: vec![
            point(a.attack, 1.0, a.attack_shape),
            point(a.hold, 1.0, 0.0),
            point(a.decay, a.sustain, a.fall_shape),
            point(a.release, 0.0, a.fall_shape),
        ],
        sustain: Some(2),
        loop_from: None,
    }
}

/// A flex envelope as breakpoints. Each point's curve knob `c` (0.5 linear)
/// sets the segment ratio `q` of the native recurrence (0x140ae5a10), whose
/// closed form is the shape `ln q`. The envelope starts from its last
/// point's level and holds at the sustain point, or with the saved loop flag
/// (the low byte of the v0x12 tail word) cycles from the point before it
/// (0x140ae57dd, docs/re/kontakt-flex-loop-jump.md).
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
    let sustain = usize::try_from(e.sustain)
        .ok()
        .filter(|&s| s < points.len());
    let looped = e.unknown_version_word().is_some_and(|w| w & 0xFF != 0);
    let loop_from = usize::try_from(e.unknown_index)
        .ok()
        .filter(|&from| looped && sustain.is_some_and(|s| from <= s));
    ir::Breakpoints {
        start: points.last().map_or(0.0, |p| p.level),
        sustain,
        loop_from,
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
/// triangle 2, sawtooth 3, random 4, and the Multi mixes 5 and 6, whose stored
/// weights (sine, rectangle, triangle, saw, random) reach the mixer as sine,
/// saw, triangle, pulse, random (0x140b07290, 0x140b078d0; the analog tables
/// play as their exact shapes). Values: fade-in ms, Hz (or a count of the
/// synced note value), pulse width, start phase in cycles. A synced note value
/// is in whole notes: the manual's "Whole, 1.0" cycle is one 4/4 bar, and
/// ANALOG STRINGS stores 1/24 × 24. A synced fade is not mapped.
pub(crate) fn lfo(l: &Lfo, clock: ir::Clock) -> Option<ir::Lfo> {
    const FREE: f32 = -1.0;
    let [fade_ms, hz, width, phase] = l.initial_values;
    if !l.initial_values.iter().all(|v| v.is_finite()) {
        return None;
    }
    let wave = match l.waveform {
        0 => ir::Wave::Sine,
        1 => ir::Wave::Square { width },
        2 => ir::Wave::Triangle,
        3 => ir::Wave::Saw,
        4 => ir::Wave::Random,
        5 | 6 => {
            let [sine, pulse, triangle, saw, random] = l
                .trailing_values
                .filter(|w| w.iter().all(|v| v.is_finite()))?;
            ir::Wave::Multi {
                weights: [sine, saw, triangle, pulse, random],
                width,
                normalize: l.records[0].flag,
            }
        }
        _ => return None,
    };
    let [rate, fade] = &l.records;
    let note = rate.values[0];
    let rate = if note == FREE {
        ir::Rate::Hz(hz)
    } else {
        ir::Rate::Beats(4.0 * note * hz)
    };
    (fade.values[0] == FREE).then_some(ir::Lfo {
        wave,
        rate,
        phase,
        fade_ms,
        clock,
    })
}

/// A legacy (types 2..=12) or SV (50..=62) filter; other kinds are not mapped.
pub(crate) fn filter(kind: i32, cutoff: f32, resonance: f32, gain: f32) -> Option<ir::Filter> {
    use ir::Pass::{Band, High, Low, Notch};
    let (pass, stages) = match kind {
        2 | 30 | 50 => (Low, 0),
        3 | 34 | 51 => (High, 0),
        4 | 38 | 53 | 101 => (Band, 1),
        6 | 31 | 52 | 70 | 100 => (Low, 1),
        7 | 35 | 54 | 71 | 102 => (High, 1),
        8 | 16 | 17 | 33 | 55 | 103 | 106 => (Low, 2),
        9 | 37 | 57 | 105 | 108 => (High, 2),
        10 | 39 | 56 | 104 | 107 => (Band, 2),
        11 | 58 => (Notch, 2),
        41 => (Notch, 1),
        12 | 59 => (Low, 3),
        60 => (Band, 3),
        61 => (High, 3),
        62 => (Notch, 3),
        _ => return None,
    };
    let (u, r) = (cutoff.clamp(0.0, 1.0), resonance.clamp(0.0, 1.0));
    let mut f = ir::Filter {
        pass,
        hz: 0.0,
        stages,
        damping: [2.0; 3],
        gain: 1.0,
        octaves: 0.0,
    };
    match kind {
        ..=17 if kind != 16 => legacy(&mut f, u, r),
        50..=62 => sv(&mut f, u, r),
        // The note each family's cutoff knob reaches (index `5n + 1381.88`
        // into the shared 2^(i/60 − 20) table): Ladder `115u + 20`
        // (filt-ladder-monark.md, its gain `2^(1.993g)`), Daft `125u + 20`
        // (filt-ar-daft.md), AR `min(145u, 140)`, Pro-53 `100u + 20`.
        30..=41 => {
            note(&mut f, 115.0 * u + 20.0, 115.0, r);
            f.gain = 2f32.powf(1.993 * gain.clamp(-1.0, 1.0));
        }
        70 | 71 => note(&mut f, 125.0 * u + 20.0, 125.0, r),
        100..=108 => note(&mut f, (145.0 * u).min(140.0), 145.0, r),
        _ => note(&mut f, 100.0 * u + 20.0, 100.0, r),
    }
    Some(f)
}

/// A cutoff at MIDI note `n` that a knob moves `per_unit` notes; the last
/// stage resonates by `1 − (1 − r)²`, the others stay critically damped.
///
/// ponytail: these families' feedback loops, knees and passband loss
/// (Ladder, AR, Daft, Pro-53) play as state-variable stages; measure
/// against Kontakt to add the loss where a library needs it.
fn note(f: &mut ir::Filter, n: f32, per_unit: f32, r: f32) {
    f.hz = 440.0 * 2f32.powf((n.min(160.0) - 69.0) / 12.0);
    f.octaves = per_unit / 12.0;
    if let Some(last) = f.damping.get_mut(usize::from(f.stages).saturating_sub(1)) {
        *last = (2.0 * (1.0 - r) * (1.0 - r)).max(MIN_DAMPING);
    }
}

/// The legacy cascade (docs/re/engine/filt-legacy.md): the knob through the
/// setters' exp series and warp, `(e^(6.15u) − 1) 0.0025728 + 0.002` of
/// 21759.5 Hz (Kontakt: −3 dB at 249 / 1189 Hz for .25 / .5; this law gives
/// 248 / 1200), Butterworth stages.
///
/// ponytail: resonance is fitted, not the native feedback: the sharpest
/// stage's Q grows by `e^(6.4 r)` and the passband falls 5.5 / 11 / 24 dB at
/// full resonance for 2 / 4 / 6 poles, which lands the measured LP4 peaks
/// (+20.9 / +37.3 dB at .5 / .9) within 2 dB. Measure LP2 and HP to refine.
fn legacy(f: &mut ir::Filter, u: f32, r: f32) {
    let v = ((6.15 * u).exp_m1() * 0.002_572_831_7).clamp(0.0, 1.0);
    f.hz = 21_759.5 * (v + 0.002);
    // log2(f(1) / f(0)); the law is close to exponential above the bottom.
    f.octaves = 8.97;
    let poles = 2.0 * f32::from(f.stages);
    for (i, k) in f.damping.iter_mut().take(usize::from(f.stages)).enumerate() {
        *k = 2.0 * ((2.0 * i as f32 + 1.0) * std::f32::consts::PI / (2.0 * poles)).cos();
    }
    if let Some(sharpest) = usize::from(f.stages).checked_sub(1) {
        if let Some(k) = f.damping.get_mut(sharpest) {
            *k = (*k / (6.4 * r).exp()).max(MIN_DAMPING);
        }
        let drop = [5.5, 11.0, 24.0].get(sharpest).copied().unwrap_or(0.0);
        f.gain = 10f32.powf(-drop * r / 20.0);
    }
}

/// The SV filters (docs/re/engine/filt-sv.md): cutoff is the note
/// `115u + 20` (at most 160), resonance `R = 1 − (1 − r)³` (six poles:
/// `1 − (1 − 0.9r)³`) damps each stage by `2(1 − R)` (four poles: the second
/// by `2(1 − 0.666R)`), and the tap's gain falls with `R`.
///
/// ponytail: the native state knees (`±2L`) only act on near self-oscillating
/// states; damping stops at 0.01 instead.
fn sv(f: &mut ir::Filter, u: f32, r: f32) {
    let note = (115.0 * u + 20.0).min(160.0);
    f.hz = 440.0 * 2f32.powf((note - 69.0) / 12.0);
    f.octaves = 115.0 / 12.0;
    if f.stages == 0 {
        return;
    }
    let big = if f.stages == 3 {
        1.0 - (1.0 - 0.9 * r).powi(3)
    } else {
        1.0 - (1.0 - r).powi(3)
    };
    f.damping = [2.0 * (1.0 - big); 3];
    if f.stages == 2 {
        f.damping[1] = 2.0 * (1.0 - 0.666 * big);
    }
    for k in &mut f.damping {
        *k = k.max(MIN_DAMPING);
    }
    let fall = match (f.stages, f.pass) {
        (1, _) => 0.5 * big.powi(5),
        (2, ir::Pass::Band) => 0.9334 * big,
        (2, ir::Pass::Notch) => 0.88 * big,
        (2, _) => 0.77 * big,
        (_, ir::Pass::Band) => (2.0 - big) * big * 0.991,
        (_, ir::Pass::Notch) => (1.482 - 0.494 * big) * big,
        _ => (1.2125 - 0.2425 * big) * big,
    };
    f.gain = 1.0 - fall;
}

const MIN_DAMPING: f32 = 0.01;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cutoff_laws_land_the_measured_corners() {
        let hz = |kind, u| filter(kind, u, 0.0, 0.0).map(|f| f.hz).unwrap_or_default();
        // Legacy LP1 at .25 / .5: Kontakt −3 dB at 249 / 1189 Hz.
        assert!((hz(2, 0.25) - 249.0).abs() < 5.0);
        assert!((hz(2, 0.5) - 1189.0).abs() < 20.0);
        // SV and Ladder at 0: 25.96 Hz; AR at 0 is note 0.
        assert!((hz(52, 0.0) - 25.96).abs() < 0.01);
        assert!((hz(33, 0.0) - 25.96).abs() < 0.01);
        assert!((hz(103, 0.0) - 8.176).abs() < 0.01);
    }

    #[test]
    fn the_flex_curve_knob_is_linear_at_half_and_bows_either_side() {
        assert!(flex_ratio(0.5).ln().abs() < 1e-6);
        assert!(flex_ratio(0.0).ln().abs() < 1e-6, "zero reads as linear");
        assert!(flex_ratio(0.25).ln() > 0.0);
        assert!(flex_ratio(0.75).ln() < 0.0);
    }
}
