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
/// modulation compiler 0x140875ac8/0x140873120): cubed (`fcubic`) for
/// internal source types 7..=13, plain otherwise. The loader FUN_140cf0420
/// maps saved codes to those types: key, velocity, release velocity, the
/// release counter, constant and the randoms land on 7..=13; pitch bend
/// (1), controllers (4) and scripts (5) stay plain. The cube at 50/127
/// matches the measured velocity table to 0.03 dB RMS.
pub(crate) fn volume(source: ir::Source) -> ir::Curve {
    ir::Curve::Depth {
        power: if matches!(
            source,
            ir::Source::Controller(_)
                | ir::Source::ControllerFrom { .. }
                | ir::Source::Script(_)
                | ir::Source::PitchBend
        ) {
            1
        } else {
            3
        },
    }
}

/// A route to an AHDSR attack multiplies its knob `u`, where the time is
/// `2·e^(8.9228·u) − 2` ms (fconv_u2n_env_a, 0x14071a980), by
/// `1 − (1 − x)·i` (0x144f1daa0): a power of `(t + 2)/2`. The law compiler
/// (FUN_140873040) gives release (0xbb) the same form through
/// fconv_u2n_env_d (0x14071a9e0, `2·e^(9.4336·u) − 2`), so the same floor.
pub(crate) const ATTACK_FLOOR_MS: f32 = 2.0;

/// The knob form and its `fcubic` apply only with the route's byte +0x53d
/// (the last saved source byte) set (0x1408766dc); clear, the compiler
/// emits `$in * (1.0 - (1.0 - x) * $intensity)`, a plain time scale (0).
pub(crate) fn time_floor_ms(source_data: &[u8]) -> f32 {
    if source_data.last().is_some_and(|&b| b != 0) {
        ATTACK_FLOOR_MS
    } else {
        0.0
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

/// Script index of a group's first external modulator: Kontakt numbers the
/// 16-slot internal array first, then each external slot after it, empty
/// slots kept. Real Kontakt 8 (`probes/find_mod_index.lua`, ANALOG STRINGS
/// group 0): `find_mod` answers 16 for external slot 0, 33 for slot 17.
pub(crate) const EXTERNAL_MOD_BASE: u16 = 16;

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
        shape: ir::Shape::Peak,
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
        stages: true,
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
        stages: false,
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

/// The free LFO's rate knob, measured on real Kontakt 8.13.1
/// (scratch probe lfo_freq.lua: ANALOG STRINGS LFO_P1 unsynced, 14 knob
/// values saved and read back): Hz = 0.010415 · 20465^x, within 0.4 %,
/// 0.0104 to 213.14 Hz.
pub(crate) const LFO_RATE_HZ: [f32; 2] = [0.010_415, 213.14];

/// A route on an LFO's rate knob, `intensity` in knob units.
pub(crate) fn lfo_rate(source: ir::Source, lfo: u8, intensity: f32) -> ir::ModKind {
    let [lo, hi] = LFO_RATE_HZ;
    ir::ModKind::LfoRate {
        source,
        lfo,
        depth: intensity,
        octaves: (hi / lo).log2(),
        range_hz: LFO_RATE_HZ,
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
        ir::Rate::Steps {
            beats: 4.0 * note,
            count: hz,
        }
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
/// Its `q_octaves` is the damping the sharpest stage gains over the knob's
/// whole travel, `log2(k(0) / k(1))` by this same law, so a resonance route
/// of depth 1 spans the knob's range.
pub(crate) fn filter(kind: i32, cutoff: f32, resonance: f32, gain: f32) -> Option<ir::Filter> {
    let mut f = filter_at(kind, cutoff, resonance, gain)?;
    let Some(stage) = usize::from(f.stages).checked_sub(1) else {
        return Some(f);
    };
    let at = |r: f32| {
        filter_at(kind, cutoff, r, gain)?
            .damping
            .get(stage)
            .copied()
    };
    if let (Some(lo), Some(hi)) = (at(0.0), at(1.0)) {
        let ratio = lo / hi;
        if ratio.is_finite() {
            f.q_octaves = ratio.log2().max(0.0);
        }
    }
    Some(f)
}

fn filter_at(kind: i32, cutoff: f32, resonance: f32, gain: f32) -> Option<ir::Filter> {
    use ir::Pass::{Band, High, Low, Notch};
    let (pass, stages) = match kind {
        // The Ladder's selector is `id − 29`; one damping holds its feedback.
        30..=41 => (Low, 1),
        2 | 50 => (Low, 0),
        3 | 51 => (High, 0),
        4 | 53 | 101 => (Band, 1),
        6 | 52 | 70 | 100 => (Low, 1),
        7 | 54 | 71 | 102 => (High, 1),
        8 | 16 | 17 | 55 | 103 | 106 => (Low, 2),
        9 | 57 | 105 | 108 => (High, 2),
        10 | 56 | 104 | 107 => (Band, 2),
        11 | 58 => (Notch, 2),
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
        ladder: u8::try_from(kind - 29)
            .ok()
            .filter(|_| (30..=41).contains(&kind)),
        damping: [2.0; 3],
        gain: 1.0,
        octaves: 0.0,
        q_octaves: 0.0,
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

/// A legacy EQ band's GAIN parameter is stored in dB, −18..=18 (eq-kontakt.md
/// §1.1), and the band's peak is twice the stored value. A route of depth 1
/// spans the parameter's whole range.
pub(crate) const EQ_GAIN_STORED_DB: f32 = 18.0;
pub(crate) const EQ_GAIN_LIMIT_DB: f32 = 2.0 * EQ_GAIN_STORED_DB;
pub(crate) const EQ_GAIN_SPAN_DB: f32 = 2.0 * EQ_GAIN_LIMIT_DB;

/// The band (0..=2) a legacy EQ's `eqGainN` target moves, N in 1..=3.
/// The route a modulator's `eqGainN` target makes on EQ `insert`.
pub(crate) fn eq_gain(
    source: ir::Source,
    insert: u8,
    param: &str,
    intensity: f32,
) -> Option<ir::ModKind> {
    eq_band_index(param).map(|band| ir::ModKind::EqGain {
        source,
        insert,
        band,
        depth_db: intensity * EQ_GAIN_SPAN_DB,
        limit_db: EQ_GAIN_LIMIT_DB,
        span_db: EQ_GAIN_SPAN_DB,
    })
}

pub(crate) fn eq_band_index(param: &str) -> Option<u8> {
    let n: u8 = param.strip_prefix("eqGain")?.parse().ok()?;
    (1..=3).contains(&n).then(|| n - 1)
}

/// A cutoff at MIDI note `n` that a knob moves `per_unit` notes; the last
/// stage resonates by `1 − (1 − r)²`, the others stay critically damped.
///
/// ponytail: these families' feedback loops, knees and passband loss
/// (AR, Daft, Pro-53) play as state-variable stages; measure
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

/// Solid G-EQ knob tables, 11 knots over 0..=1 (core `G_EQ`, constructor
/// 0x141a800c0; v2 sampler-core eq_family/solid.rs, proved against the
/// binary). `C_*` names are the curve's offset in the core.
mod solid {
    pub(super) const GAIN_HF: [f32; 11] = [-20.5, -18., -11., -6., -2., 0., 2., 6., 11., 18., 20.5];
    pub(super) const FREQ_HF: [f32; 11] = [
        1539., 1710., 2400., 3980., 5460., 6820., 9160., 13100., 17813., 21000., 21800.,
    ];
    pub(super) const GAIN_HMF: [f32; 11] =
        [-20., -17.5, -11., -6., -2., 0., 2., 6., 11., 17.5, 20.];
    pub(super) const FREQ_HMF: [f32; 11] = [
        692.48, 730.72, 1022.48, 1614.67, 2112.54, 2619.29, 3291.53, 4544.29, 5634.35, 6444.74,
        6892.65,
    ];
    pub(super) const Q_HMF: [f32; 11] = [
        0.65, 0.62, 0.58, 0.5, 0.45, 0.4, 0.37, 0.31, 0.26, 0.22, 0.22,
    ];
    pub(super) const GAIN_LMF: [f32; 11] =
        [-21.5, -18., -11., -6., -2., 0., 2., 6., 11., 18., 21.5];
    pub(super) const FREQ_LMF: [f32; 11] = [
        223., 239., 323., 494., 668., 809., 1050., 1479., 1841., 2083., 2200.,
    ];
    pub(super) const Q_LMF: [f32; 11] =
        [0.65, 0.62, 0.58, 0.5, 0.45, 0.4, 0.37, 0.3, 0.25, 0.21, 0.2];
    pub(super) const GAIN_LF: [f32; 11] =
        [-19.5, -16., -10., -5.3, -2., 0., 2., 5.3, 10., 16., 19.5];
    pub(super) const FREQ_LF: [f32; 11] = [
        40.43, 44.64, 63.44, 101.36, 140.21, 183.73, 245.15, 351.55, 513.3, 614.68, 697.34,
    ];
    /// Bandwidth by gain: wider for small boosts and cuts.
    pub(super) const C_4818: [f32; 11] = [
        1.0, 0.9167, 0.6667, 0.5833, 0.5417, 0.5417, 0.5417, 0.5833, 0.6667, 0.9167, 1.0,
    ];
    pub(super) const C_5078: [f32; 11] = [
        1.0, 1.0, 1.0, 1.0833, 1.0833, 1.0, 0.9167, 0.6666, 0.3333, 0.05, 0.0001,
    ];
    /// Q scale by gain for the LMF and HMF (C_71F8; LMF's C_82B8 is the same curve).
    pub(super) const C_71F8: [f32; 11] = [
        1.0, 0.8615, 0.6615, 0.5846, 0.5846, 0.5846, 0.5846, 0.5846, 0.6615, 0.8615, 1.0,
    ];
    pub(super) const C_9378: [f32; 11] = [
        1.0, 0.8796, 0.713, 0.6296, 0.6296, 0.6296, 0.6296, 0.6296, 0.713, 0.8796, 1.0,
    ];
    /// Shelves: HF corner (× 2 × the Hz knob) and gain by the gain knob; LF
    /// corner, its gain-knob scale, gain and its Hz-knob scale.
    pub(super) const HF_CORNER: [f32; 11] = [
        3800., 3295., 2790., 2510., 2455., 2400., 2474., 2547., 2815., 3655., 4509.,
    ];
    pub(super) const HF_SHELF_GAIN: [f32; 11] =
        [-20., -18., -11., -6., -2., 0., 2., 6., 11., 18., 20.];
    pub(super) const LF_CORNER: [f32; 11] = [
        90., 100., 125., 184., 240., 290., 380., 500., 650., 730., 780.,
    ];
    pub(super) const C_A868: [f32; 11] = [
        1.0, 1.1207, 1.2241, 1.2931, 1.2931, 1.2931, 1.2931, 1.2931, 1.2241, 1.1207, 1.0,
    ];
    pub(super) const LF_SHELF_GAIN: [f32; 11] =
        [-20., -15.8, -11., -6.5, -2.6, 0., 2.6, 6.5, 11., 15.8, 20.];
    pub(super) const C_AC98: [f32; 11] = [
        0.895, 0.91, 0.925, 0.965, 0.985, 1.0, 1.02, 1.03, 1.035, 1.035, 1.04,
    ];
}

/// The Solid G-EQ's four bands from its knobs (each 0..=1): LF gain and Hz,
/// LMF and HMF gain, Hz and Q, HF gain and Hz. The LF and HF bands are
/// shelves unless their shape is [`ir::Shape::Peak`] (the bell switch).
// ponytail: the tables' main curves on standard cookbook bands, linearly
// interpolated. The core's splines, its Orfanidis peaks, the LF shelf's two
// shaping peaks and the HF shelf's DC cut are left out (v2 has them all).
pub(crate) fn solid_eq(
    [lf_gain, lf_hz]: [f32; 2],
    lf_shape: ir::Shape,
    lmf: [f32; 3],
    hmf: [f32; 3],
    [hf_gain, hf_hz]: [f32; 2],
    hf_shape: ir::Shape,
) -> [ir::Bell; 4] {
    use solid::*;
    let t = |table: [f32; 11], x: f32| kontra_law::Law::Table(table.into()).eval(x);
    let peak = |hz, gain_db, q| ir::Bell {
        shape: ir::Shape::Peak,
        hz,
        q,
        gain_db,
    };
    let lf = if lf_shape == ir::Shape::Peak {
        peak(
            t(FREQ_LF, lf_hz),
            t(GAIN_LF, lf_gain),
            t(C_9378, lf_gain) * 1.08,
        )
    } else {
        ir::Bell {
            shape: ir::Shape::LowShelf,
            hz: t(C_A868, lf_gain) * t(LF_CORNER, lf_hz),
            q: 0.5,
            gain_db: t(LF_SHELF_GAIN, lf_gain) * t(C_AC98, lf_hz),
        }
    };
    let [g, hz, q] = lmf;
    let lmf = peak(t(FREQ_LMF, hz), t(GAIN_LMF, g), t(C_71F8, g) * t(Q_LMF, q));
    let [g, hz, q] = hmf;
    let hmf = peak(t(FREQ_HMF, hz), t(GAIN_HMF, g), t(C_71F8, g) * t(Q_HMF, q));
    let hf = if hf_shape == ir::Shape::Peak {
        let q = t(C_4818, hf_gain) * 1.2 * t(C_5078, hf_hz);
        peak(t(FREQ_HF, hf_hz), t(GAIN_HF, hf_gain), q)
    } else {
        ir::Bell {
            shape: ir::Shape::HighShelf,
            hz: t(HF_CORNER, hf_gain) * 2.0 * hf_hz,
            q: 0.55,
            gain_db: t(HF_SHELF_GAIN, hf_gain),
        }
    };
    [lf, lmf, hmf, hf]
}

/// The Compressor (0x19): saved mode, threshold dB, ratio knob, attack and
/// release ms, link byte. The Pro core's law (docs/re/engine/dyn-compressors.md,
/// setter 0x140ab9220): the knob `r` is the output slope over the threshold
/// (ratio `1/r`), a 12 dB knee, and Pro's own
/// make-up `−0.666·(1 − r)·threshold` dB (setter `+0x524`, constant at
/// 0x144fc4c90, added to the gain exponent in core 0x140ab35d0). Classic and
/// Enhanced have no make-up (their setters write no offset). The slot's Output
/// knob applies on top in every mode. Pro always links the channels. Every
/// threshold is `2^(dB/6)`. Classic is a [`ir::Detector::Trigger`] to the
/// fixed depth `thr^(1 − r)` (setter `+0x2e0`; core in dyn-compressors.md).
///
/// The caller (0x1409619b0) passes the saved attack and release ms unscaled;
/// each mode turns them into its own ballistics, given here as time
/// constants: Pro clamps attack to 0.1..=10 ms as a low-pass at `1000/ms` Hz
/// and release to 10..=1000 ms as a halving time; Enhanced (core 0x140ab2c80)
/// runs a two-pole detector at `2513.2/max(ms, 1)` rad/s with damping 1.8 and
/// a peak hold falling by `1778.3/max(ms, 50)` per second; Classic (core
/// 0x140ab2250) steps by `0.625^(1/n)` and `(1/3)^(1/n)` over the n frames.
// ponytail: Enhanced's piecewise curve runs as the soft knee; unmeasured.
pub(crate) fn compressor(
    mode: i32,
    threshold_db: f32,
    r: f32,
    attack_ms: f32,
    release_ms: f32,
    link: bool,
) -> ir::Compressor {
    let r = r.clamp(0.0, 1.0);
    let (attack_ms, release_ms) = match mode {
        0 => (
            attack_ms.max(0.0) / 1.6f32.ln(),
            release_ms.max(0.0) / 3f32.ln(),
        ),
        1 => (
            attack_ms.max(1.0) * 1800.0 / 2513.2,
            release_ms.max(50.0) * 1000.0 / 1778.3,
        ),
        _ => (
            attack_ms.clamp(0.1, 10.0) / std::f32::consts::TAU,
            release_ms.clamp(10.0, 1000.0) / std::f32::consts::LN_2,
        ),
    };
    ir::Compressor {
        threshold_db: threshold_db * 20.0 * 2f32.log10() / 6.0,
        ratio: 1.0 / r.max(1e-3),
        knee_db: 12.0,
        attack_ms,
        release_ms,
        makeup_db: if mode == 2 {
            -0.666 * (1.0 - r) * threshold_db
        } else {
            0.0
        },
        link: link || mode == 2,
        mix: 1.0,
        detector: if mode == 0 {
            ir::Detector::Trigger
        } else {
            ir::Detector::Input
        },
    }
}

/// The Limiter (0x1c): saved input gain (dB, as `2^(x/6)`) and release ms;
/// the output never passes full scale (docs/re/engine/dyn-limiters-transient.md).
pub(crate) fn limiter(input_db: f32, release_ms: f32) -> ir::Limiter {
    ir::Limiter {
        input_db,
        ceiling_db: 0.0,
        release_ms: release_ms.max(0.0),
    }
}

/// The Transparent Limiter (0x5f): saved drive, release and ceiling knobs
/// 0..=1. Drive `(1 − x)·40` dB, release `max(x, 0.001)` s as a one-pole time
/// constant, ceiling `−0.35 − (1 − x)·39.7` dB.
pub(crate) fn transparent_limiter(drive: f32, release: f32, ceiling: f32) -> ir::Limiter {
    let [drive, release, ceiling] = [drive, release, ceiling].map(|x| x.clamp(0.0, 1.0));
    ir::Limiter {
        input_db: (1.0 - drive) * 40.0,
        ceiling_db: -0.35 - (1.0 - ceiling) * 39.7,
        // A one-pole time constant τ halves in τ·ln 2.
        release_ms: 1000.0 * release.max(0.001) * std::f32::consts::LN_2,
    }
}

/// Solid Bus Comp (0x46): saved threshold, make-up, attack, release, ratio
/// and mix knobs 0..=1, and the link byte. Ratio, attack and release are
/// six-step switches (`floor(5x + 0.5)`); the threshold knob spans ±18.6 dB
/// around −20 dB less a per-ratio offset, make-up −5.25..=21 dB
/// (tables 0x1479a7e18..0x1479a7ea8, docs/re/engine/dyn-compressors.md).
/// Release step 5 is the auto release on a 75 ms base; the second value is
/// set then, for the caller to report.
// ponytail: the core's detector and curve are unverified; this is the
// standard compressor at the switch values, auto release as its 75 ms base.
pub(crate) fn bus_compressor(knobs: [f32; 6], link: bool) -> (ir::Compressor, bool) {
    const RATIO: [f32; 6] = [1.5, 2.0, 3.0, 4.0, 5.0, 10.0];
    const OFFSET_DB: [f32; 6] = [-12.0, -9.0, -5.0, -4.0, -3.3, -0.2];
    const ATTACK_MS: [f32; 6] = [0.1, 0.3, 1.0, 3.0, 10.0, 30.0];
    const RELEASE_MS: [f32; 6] = [12.5, 18.75, 37.5, 50.0, 100.0, 75.0];
    let [threshold, makeup, attack, release, ratio, mix] = knobs.map(|x| x.clamp(0.0, 1.0));
    let step = |x: f32| ((x * 5.0 + 0.5).floor() as usize).min(5);
    let at = |table: [f32; 6], x: f32| table.get(step(x)).copied().unwrap_or(1.0);
    let c = ir::Compressor {
        threshold_db: -18.6 + 37.2 * threshold - 20.0 + at(OFFSET_DB, ratio),
        ratio: at(RATIO, ratio),
        knee_db: 0.0,
        attack_ms: at(ATTACK_MS, attack),
        release_ms: at(RELEASE_MS, release),
        makeup_db: -5.25 + 26.25 * makeup,
        link,
        mix,
        detector: ir::Detector::Input,
    };
    (c, step(release) == 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compressor_knob_is_the_output_slope_with_threshold_makeup() {
        let c = compressor(2, -20.0, 0.25, std::f32::consts::TAU, 69.3, false);
        assert!((c.ratio - 4.0).abs() < 1e-4);
        assert!((c.makeup_db - 9.99).abs() < 1e-3);
        assert!((c.attack_ms - 1.0).abs() < 1e-3 && (c.release_ms - 100.0).abs() < 0.1);
        assert!(c.link);
        assert!(!compressor(1, -20.0, 0.25, 1.0, 100.0, false).link);
        // Only Pro adds its own make-up.
        assert_eq!(compressor(1, -20.0, 0.25, 1.0, 100.0, false).makeup_db, 0.0);
        // Classic and Enhanced keep long saved times that Pro clamps.
        let classic = compressor(0, -20.0, 0.25, 100.0, 300.0, false);
        assert!(
            (classic.attack_ms - 212.8).abs() < 0.1 && (classic.release_ms - 273.1).abs() < 0.1
        );
        let enhanced = compressor(1, -20.0, 0.25, 100.0, 20.0, false);
        assert!(
            (enhanced.attack_ms - 71.6).abs() < 0.1 && (enhanced.release_ms - 28.1).abs() < 0.1
        );
        // A knob of 1 leaves the signal alone.
        assert!((compressor(0, -20.0, 1.0, 1.0, 100.0, true).ratio - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_transparent_limiter_and_bus_comp_reach_their_table_ends() {
        let l = transparent_limiter(0.0, 1.0, 1.0);
        assert!((l.input_db - 40.0).abs() < 1e-4 && (l.ceiling_db + 0.35).abs() < 1e-4);
        let (b, auto) = bus_compressor([0.5, 0.2, 0.0, 1.0, 1.0, 1.0], true);
        assert!((b.ratio - 10.0).abs() < 1e-6 && (b.threshold_db + 20.2).abs() < 1e-4);
        assert!((b.attack_ms - 0.1).abs() < 1e-6 && (b.release_ms - 75.0).abs() < 1e-4);
        assert!(auto && !bus_compressor([0.5; 6], true).1);
        assert!(b.makeup_db.abs() < 1e-4);
    }

    #[test]
    fn a_centred_solid_eq_is_flat_and_its_knobs_reach_the_table_ends() {
        let mid = [0.5; 3];
        let flat = solid_eq(
            [0.5; 2],
            ir::Shape::LowShelf,
            mid,
            mid,
            [0.5; 2],
            ir::Shape::HighShelf,
        );
        assert!(flat.iter().all(|b| b.gain_db == 0.0));
        let [lf, lmf, _, hf] = solid_eq(
            [0.0, 0.5],
            ir::Shape::LowShelf,
            [1.0, 0.5, 0.5],
            mid,
            [0.5; 2],
            ir::Shape::Peak,
        );
        assert_eq!(lf.shape, ir::Shape::LowShelf);
        assert_eq!(lf.gain_db, -20.0);
        assert_eq!((lmf.gain_db, lmf.hz), (21.5, 809.0));
        assert_eq!((hf.shape, hf.hz), (ir::Shape::Peak, 6820.0));
    }

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
    fn the_resonance_span_is_the_damping_the_knob_moves_by() {
        // SV LP2 (52): k = 2(1 − R), R = 1 − (1 − r)³, clamped at 0.01 by the
        // law: the knob's travel is log2(2 / 0.01) octaves of Q.
        let f = filter(52, 0.5, 0.0, 0.0).unwrap();
        assert!(
            (f.q_octaves - 200f32.log2()).abs() < 0.01,
            "{}",
            f.q_octaves
        );
        // Legacy LP1 has no stage to resonate.
        assert_eq!(filter(2, 0.5, 0.0, 0.0).map(|f| f.q_octaves), Some(0.0));
    }

    #[test]
    fn the_legacy_eq_band_params_map_to_their_band() {
        assert_eq!(eq_band_index("eqGain1"), Some(0));
        assert_eq!(eq_band_index("eqGain3"), Some(2));
        assert_eq!(eq_band_index("eqGain4"), None);
        assert_eq!(eq_band_index("filterQ"), None);
    }

    #[test]
    fn the_flex_curve_knob_is_linear_at_half_and_bows_either_side() {
        assert!(flex_ratio(0.5).ln().abs() < 1e-6);
        assert!(flex_ratio(0.0).ln().abs() < 1e-6, "zero reads as linear");
        assert!(flex_ratio(0.25).ln() > 0.0);
        assert!(flex_ratio(0.75).ln() < 0.0);
    }
}
