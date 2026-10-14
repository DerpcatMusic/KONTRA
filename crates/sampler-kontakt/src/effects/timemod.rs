//! Kontakt delay and modulation effects onto `sampler_ir::fx::timemod`.
//!
//! Native (Kontakt 8.13.1 binary): Legacy Phaser (0x14), Legacy Delay (0x10,
//! audio 0x140aa1130), Legacy Chorus (0x11, audio 0x140aa0510) and Legacy
//! Flanger (0x12, audio 0x140aa2350); the last two are one legacy modulated
//! delay kernel. Choral (0x5d) configures the line's voice bank
//! ([`choral`]). Twin Delay (0x6B, kernel 0x140ab07d0) and Psyche Delay
//! (0x63, kernel 0x140aadb50) are native. The other modern effects stay
//! unsupported with the reasons in [`unproved`].
use super::{Notes, Reader};
use sampler_ir::{
    Reason,
    fx::timemod::{
        Allpass, BANK_VOICES, BankVoice, Config, DelayTime, Lfo, LfoShape, Line, Native, Order, Rate, PsycheDelay,
        SweepLaw, TwinDelay, VoiceBank,
    },
};

/// Stored fields, in file order. `unit` > 0 selects the tempo-synced period.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Unit {
    Phaser { depth: f32, feedback: f32, speed: f32, phase: f32, unit: f32 },
    Delay { time: f32, feedback: f32, pan: f32, damping: f32, unit: f32 },
    Chorus { depth: f32, speed: f32, phase: f32, unit: f32 },
    Flanger { depth: f32, feedback: f32, speed: f32, phase: f32, color: f32, unit: f32 },
    /// Choral (0x5d, reader schema 7 f32, 2 flags, i32): amount, rate, delay,
    /// voices, width, feedback, mix, invert, scatter, mode.
    Choral { fields: [f32; 7], invert: bool, scatter: bool, mode: i32 },
    Twin(TwinDelay),
    Psyche(PsycheDelay),
}

/// Choral voice tables (0x14193cd70), f32 bits: G0 delay scale, G1 rate scale,
/// G2 LFO1 phase offset, G3 LFO2 rate ratio, G4 LFO2 depth, G5 filter Hz, G6
/// high-pass blend, G7 LFO1 shape, G8 modulation smoothing; seven voices each.
/// Modes 3 and 4 share a table; G8[6] is written after each branch.
const MODES: [[u32; 63]; 4] = [
    [
        0x3b449ba6, 0x3b449ba6, 0x3be56042, 0x3be56042, 0x3c343958, 0x3c343958, 0x3c54fdf4,
        0x3f800000, 0x3f800000, 0x3fb6db62, 0x3fb70a31, 0x3fdcb296, 0x3fdd2395, 0x3f3e075f,
        0x00000000, 0x3f000000, 0x00000000, 0x3f000000, 0x00000000, 0x3f000000, 0x00000000,
        0x40c33333, 0x40c33333, 0x40c33333, 0x40c33333, 0x3dcccccd, 0x3dcccccd, 0x3f800000,
        0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3c23d70a, 0x3c23d70a, 0x00000000,
        0x459c4000, 0x459c4000, 0x459c4000, 0x459c4000, 0x461c4000, 0x461c4000, 0x42480000,
        0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3f800000,
        0x3e4ccccd, 0x3e570a3d, 0x3e570a3d, 0x3e4ccccd, 0x3ecccccd, 0x3ecccccd, 0x00000000,
        0x3ecccccd, 0x3ecccccd, 0x3ecccccd, 0x3ecccccd, 0x3ecccccd, 0x3ecccccd, 0x3ecccccd,
    ],
    [
        0x3be56042, 0x3be56042, 0x3be56042, 0x3be56042, 0x3c54fdf4, 0x3c54fdf4, 0x3ced9168,
        0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f3e075f,
        0x00000000, 0x3f000000, 0x3eaaaa3b, 0x3f55551d, 0x3f2aaa3b, 0x3e2aa8eb, 0x00000000,
        0x40833333, 0x40833333, 0x40800000, 0x40800000, 0x4083d70a, 0x4083f8cb, 0x3f800000,
        0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x00000000,
        0x457a0000, 0x457a0000, 0x45bb8000, 0x45bb8000, 0x461c4000, 0x461c4000, 0x42480000,
        0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3f800000,
        0x3c23d70a, 0x3c23d70a, 0x3c23d70a, 0x3c54fdf4, 0x3c656042, 0x3c449ba6, 0x00000000,
        0x3e99999a, 0x3ea3d70a, 0x3ea3d70a, 0x3e99999a, 0x3ea8f5c3, 0x3e99999a, 0x3ecccccd,
    ],
    [
        0x3c343958, 0x3c343958, 0x3c343958, 0x3c343958, 0x3c343958, 0x3c343958, 0x3d591687,
        0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f3e075f,
        0x00000000, 0x3f000000, 0x3f000000, 0x00000000, 0x3e800000, 0x3f400000, 0x00000000,
        0x40c33333, 0x40c33333, 0x40c33333, 0x40c33333, 0x40c33333, 0x40c33333, 0x3f800000,
        0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000,
        0x463b8000, 0x463b8000, 0x43160000, 0x43160000, 0x469c4000, 0x469c4000, 0x42480000,
        0x00000000, 0x00000000, 0x3f800000, 0x3f800000, 0x00000000, 0x00000000, 0x3f800000,
        0x3dcccccd, 0x3dcccccd, 0x3dced917, 0x3dced917, 0x3ecccccd, 0x3ecccccd, 0x00000000,
        0x3e4ccccd, 0x3e6147ae, 0x3e6147ae, 0x3e4ccccd, 0x3e4ccccd, 0x3e4ccccd, 0x3f7d70a4,
    ],
    [
        0x3be56042, 0x3be56042, 0x3c343958, 0x3c343958, 0x3c54fdf4, 0x3c54fdf4, 0x3ced9168,
        0x3f800000, 0x3f800000, 0x3fb6db62, 0x3fb6db62, 0x3fdcb296, 0x3fdcb296, 0x3f3e075f,
        0x00000000, 0x00000000, 0x00000000, 0x3e800000, 0x00000000, 0x3e800000, 0x00000000,
        0x40c33333, 0x40c33333, 0x40c33333, 0x40c33333, 0x40c33333, 0x40c33333, 0x3f800000,
        0x00000000, 0x00000000, 0x3e99999a, 0x3e99999a, 0x3f19999a, 0x3f19999a, 0x00000000,
        0x469c4000, 0x469c4000, 0x469c4000, 0x469c4000, 0x469c4000, 0x469c4000, 0x42480000,
        0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3f800000,
        0x00000000, 0x3e4ccccd, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000,
        0x3f7d70a4, 0x3f7d70a4, 0x3f7d70a4, 0x3f7d70a4, 0x3f7d70a4, 0x3f7d70a4, 0x3f7d70a4,
    ],
];

/// Kontakt 8.13.1 Choral core setters, resolved to the voice bank: amount
/// 0x14193a2f0, rate 0x14193b140, delay 0x14193ac00, voices 0x14193a3e0,
/// width 0x14193a4a0, feedback 0x14193a610, scatter 0x14193a620, invert
/// 0x14193a6a0, mix 0x14193a6d0, mode 0x14193a230 / 0x14193e160.
fn choral([amount, rate, delay, voices, width, feedback, mix]: [f32; 7], invert: bool, scatter: bool, mode: usize) -> VoiceBank {
    let table = MODES[mode.min(3)].map(|bits| f64::from(f32::from_bits(bits)));
    let group = |g: usize, v: usize| table[g * 7 + v];
    // Amount: d = (a*a f32) * 0.99 + 0.01, pitch ratio R = 2^(d * 0.025).
    let d = f64::from(amount * amount) * 0.99 + 0.01;
    let excursion = 0.5 * (2f64.powf(d * 0.025) - 1.);
    // Rate: r^3 * 7.4 + 0.25 Hz.
    let r = f64::from(rate);
    let speed = r * r * r * 7.4 + 0.25;
    // Delay: seconds = G0 * (D * 5 + 0.1).
    let span = f64::from(delay) * 5. + 0.1;
    // Feedback: g = fb^(1/4) * 0.999; input gain (1 - min(g, .99)^4)^(1/4).
    let g = f64::from(feedback).sqrt().sqrt() * 0.999;
    let clamped = if 0.99 <= g { 0.99 } else { g };
    let c2 = clamped * clamped;
    let input = (1. - c2 * c2).sqrt().sqrt();
    // Voices: pairs 2/3 and 4/5 fade in over the two halves; 0/1 are 1 (ctor
    // 0x141939fa4), 6 is 0.
    let fade = |k: f64| (f64::from(voices) + f64::from(voices) - k).clamp(0., 1.).sqrt();
    let gains = [1., 1., fade(0.), fade(0.), fade(1.), fade(1.), 0.];
    let sum = 2. + fade(0.) * 2. + fade(1.) * 2.;
    let norm = (sum * 0.2 + 0.6).sqrt() * (1. / sum);
    // Width: pair k turns by -/+ w * pi/4 * (1 - 2k/7); voice 6 stays put.
    let (left_turn, right_turn) = (f64::from(width) * -std::f64::consts::FRAC_PI_4, f64::from(width) * std::f64::consts::FRAC_PI_4);
    let turn = |angle: f64| {
        let (s, c) = angle.sin_cos();
        [c, -s, s, c]
    };
    let voices: [BankVoice; BANK_VOICES] = std::array::from_fn(|v| {
        let reach = 1. - (v / 2 * 2) as f64 * 0.142_857_142_857_142_85;
        let lfo_hz = group(1, v) * speed;
        BankVoice {
            gain: gains[v],
            feedback: if v == 0 { g } else { g.powf(group(0, v) * (1. / group(0, 0))) },
            rotation: match v {
                6 => [1., -0., 0., 1.],
                _ if v % 2 == 0 => turn(left_turn * reach),
                _ => turn(right_turn * reach),
            },
            delay_s: group(0, v) * span,
            excursion,
            lfo_hz,
            lfo2_ratio: group(3, v),
            // LFO2 runs when G4 > 1e-8, scaled by G4 / G3.
            lfo2: if 1e-8 < group(4, v) { group(4, v) / group(3, v) } else { 0. },
            offset: group(2, v),
            shape: group(7, v),
            smoothing: group(8, v),
            filter_hz: group(5, v),
            highpass: group(6, v),
        }
    });
    // Shelf: S = max(1 - g, 0.2), corner min(0.25 / T0, 50) Hz warped by S^(1/4).
    let s = if 1. - g <= 0.2 { 0.2 } else { 1. - g };
    let fc = 0.25 / (group(0, 0) * span);
    let fc = if 50. <= fc { 50. } else { fc };
    let sc = if scatter { 1. } else { 0. };
    let (a, b) = (sc * -0.261_203_874_963_740_98, sc * 0.445_902_906_222_805_98);
    let mix = f64::from(mix);
    VoiceBank {
        voices,
        scatter: [1. - sc * 0.554_097_093_777_194_07, a, a, b, a, b, b],
        invert: 1. - f64::from(if invert { 2f32 } else { 0. }),
        input,
        // Output 0x14193be50: mix * input * norm * 0.707 (0x147697a98).
        wet: mix * input * norm * 0.707,
        dry: 1. - mix,
        shelf_hz: fc / s.sqrt().sqrt(),
        shelf_gain: s,
    }
}

/// Twin Delay v0x10 (reader `0x140ac6280`): four time sub-objects (predelay
/// L/R, time L/R; each a sync factor, two floats and a flag), then predelay,
/// time, feedback and level for L, the same for R, cross and width.
pub(super) fn read_twin(version: u16, r: &mut Reader) -> Option<Unit> {
    if version != 0x10 {
        return None;
    }
    let mut units = [0f32; 4];
    for u in &mut units {
        *u = r.f32()?;
        r.f32()?;
        r.f32()?;
        r.flag()?;
    }
    let mut v = [0f32; 10];
    for x in &mut v {
        *x = r.f32()?;
    }
    Some(Unit::Twin(TwinDelay {
        time: [v[1], v[5]],
        predelay: [v[0], v[4]],
        time_unit: [units[2], units[3]],
        predelay_unit: [units[0], units[1]],
        feedback: [v[2], v[6]],
        level: [v[3], v[7]],
        cross: v[8],
        width: v[9],
    }))
}

/// Psyche Delay v0x10 (reader `0x140ac5bc0`): time, feedback, detune, pitch,
/// offset, cross; reverse-stereo, detune-stereo and reverse bytes; the time
/// sub-object's unit factor and two unused floats, then a flag.
pub(super) fn read_psyche(version: u16, r: &mut Reader) -> Option<Unit> {
    if version != 0x10 {
        return None;
    }
    let mut v = [0f32; 6];
    for x in &mut v {
        *x = r.f32()?;
    }
    let [reverse_stereo, detune_stereo, reverse] = [r.flag()?, r.flag()?, r.flag()?];
    let unit = r.f32()?;
    r.f32()?;
    r.f32()?;
    r.flag()?;
    Some(Unit::Psyche(PsycheDelay { time: v[0], unit, feedback: v[1], detune: v[2], pitch: v[3], offset: v[4], cross: v[5], reverse_stereo, detune_stereo, reverse }))
}

/// v0x50 stops after `unit`; v0x51 adds two floats and a flag the engine does not read.
pub(super) fn read(module: u16, version: u16, r: &mut Reader) -> Option<Unit> {
    if module == 0x5d {
        let fields = [r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?];
        return Some(Unit::Choral { fields, invert: r.flag()?, scatter: r.flag()?, mode: r.i32()? });
    }
    let unit = match module {
        0x14 => Unit::Phaser { depth: r.f32()?, feedback: r.f32()?, speed: r.f32()?, phase: r.f32()?, unit: r.f32()? },
        0x10 => Unit::Delay { time: r.f32()?, feedback: r.f32()?, pan: r.f32()?, damping: r.f32()?, unit: r.f32()? },
        0x11 => Unit::Chorus { depth: r.f32()?, speed: r.f32()?, phase: r.f32()?, unit: r.f32()? },
        0x12 => Unit::Flanger { depth: r.f32()?, feedback: r.f32()?, speed: r.f32()?, phase: r.f32()?, color: r.f32()?, unit: r.f32()? },
        _ => return None,
    };
    match version {
        0x50 => {}
        0x51 => {
            r.f32()?;
            r.f32()?;
            r.flag()?;
        }
        _ => return None,
    }
    Some(unit)
}

/// Why a delay or modulation module without a mapping stays unsupported.
pub(super) fn unproved(module: u16) -> Option<&'static str> {
    Some(match module {
        0x5a => "Replika: core control laws known (setParam 0x141ac5980: time 8/9, type 28, gains 0/1, mix 4) but the saved-field to core-index binding (generic per-quantum streams at wrapper +0x14580/+0x145f0) and the line read/write recurrence (0x141ab7e00 / 0x141acd110, line filter, saturation 0x141aa44a0) are not extracted",
        0x5b => "Phasis: saved fields measured (fx_raum_delight_psyche_vibrato_phasis_replika.tsv) but their binding to the core parameter indices and the core's oversampling resamplers (about 96 kHz internal) are not extracted",
        0x5c => "Flair: saved fields and most of the core law are extracted, but the writer of the pitch note field (+0x40), the internal-rate resampler and the random start phase are not",
        0x69 => "Vibrato Chorus: saved fields measured (fx_raum_delight_psyche_vibrato_phasis_replika.tsv) but the core's wave-digital LC ladder (the delay line it models) is not extracted",
        0x22 => "Rotator: horn/drum law located (control 0x140a97d30, audio 0x140aae920, setter case 0xe 0x140962630) but the runtime-initialised lane vectors at 0x149e6c400..0x149e6c45c and the setter's parameter binding are not yet verified",
        _ => return None,
    })
}

fn unit_set(unit: f32) -> Option<f32> {
    // Readers set the sync flag when unit > 1e-9 (0x144737708, Chorus reader
    // 0x140d00760); the speed getter 0x140a736a0 tests that flag.
    (unit > f32::from_bits(0x3089705f)).then_some(unit)
}

fn rate(speed: f32, unit: f32) -> Rate {
    unit_set(unit).map_or(Rate::Hz(speed), |unit| Rate::Sync { unit, speed })
}

fn unit_interval(values: &[f32]) -> bool {
    values.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v))
}

/// `dry` is the slot's own dry level when the effect runs outside a Mix block.
pub(super) fn config(unit: &Unit, dry: f64, notes: &mut Notes) -> Option<Config> {
    let lfo = |speed, unit, phase: f32| Lfo { shape: LfoShape::Sine, rate: rate(speed, unit), phase };
    let cfg = match *unit {
        Unit::Phaser { depth, feedback, speed, phase, unit } => {
            if !(unit_interval(&[depth, feedback, phase]) && speed.is_finite() && speed >= 0. && unit.is_finite()) {
                notes.push(("parameters".into(), format!("{depth}/{feedback}/{speed}/{phase}/{unit}"), Reason::InvalidValue));
                return None;
            }
            // Reader FUN_140d01fa0; feedback .85 and phase .25 cycles at 0x140962c75.
            Config {
                line: None,
                allpass: Some(Allpass { stages: 4, law: SweepLaw::Legacy { depth }, feedback: (f64::from(feedback) * 0.85) as f32 }),
                order: Order::LineThenAllpass,
                lfo: lfo(speed, unit, phase * 0.25),
                dry: 0.,
                wet: 1.,
            }
        }
        Unit::Delay { time, feedback, pan, damping, unit } => {
            if !(unit_interval(&[feedback, pan, damping]) && time.is_finite() && (0. ..=2900.).contains(&time)) {
                notes.push(("parameters".into(), format!("{time}/{feedback}/{pan}/{damping}/{unit}"), Reason::InvalidValue));
                return None;
            }
            // Setter case 0x43 @0x1409645ae, audio 0x140aa1130: feedback * 0.98, damping * 0.7,
            // pan crossing, ring 2^18. Synced, field 0 holds an integer note count
            // round(1 + 11u) in 1..=12 and field 4 the unit factor (timemod-reads.tsv,
            // fx_legacy_delay_reverb_mod.tsv). The synced time goes through the time object's vtable
            // slot 5 (0x14072b380), which passes its own [0, 5800] ms bounds to getter
            // 0x140a74bf0 (counts below one read as one).
            let mut line = Line::single(DelayTime::Ms(f64::from(time)), 2900.);
            line.native = Native::LegacyDelay { time, unit, feedback, damping, pan };
            Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: lfo(0., 0., 0.), dry, wet: 1. }
        }
        Unit::Chorus { depth, speed, phase, unit } => {
            if !(unit_interval(&[depth, phase]) && speed.is_finite() && speed >= 0.) {
                notes.push(("parameters".into(), format!("{depth}/{speed}/{phase}/{unit}"), Reason::InvalidValue));
                return None;
            }
            // Setter case @0x140963970, audio 0x140aa0510: the base delay is
            // `BParFXChorus +8 * rate * 0.001` (container slot 1 = +0x58, 0x140570e20), not a
            // code constant. The reader 0x140d00760 fills +0xc/+0x10/+0x14/+0x30/+0x3c/+0x40
            // (CH_PHASE is +0x14), the writer 0x140d104e0 saves the same, and the only
            // vtable 0x144e74dc8 store is the container constructor (lea 0x140701ad7), which
            // leaves +8 alone (kemu: the ctor 0x140701a80 run on a 0xAA-filled object
            // writes none of +8..+0x17); the container is memset to zero on allocation (0x1408b01c9,
            // 0x1406e9458), so base = 0 frames, the excursion clamps to base - 1 = -1 and
            // Depth has no effect (the native setter + audio under kemu agree:
            // `legacy_chorus_and_flanger_render_native_kemu_vectors`). Two heads (base, base >> 1) on LFOs 0.015 cycles
            // apart, each one-pole 0.95, summed; Speed through the sync getter
            // 0x14072b330 -> 0x140a736a0 (sync class vtable 0x144e74208 slot 5).
            let mut line = Line::single(DelayTime::Frames(1), 0.);
            line.native = Native::LegacyModDelay {
                base_ms: 0.,
                excursion: f64::from(depth) * 75.,
                feedback: 0.,
                second_head: true,
                smoothing: f32::from_bits(0x3f733333),
            };
            Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: lfo(speed, unit, phase * 0.25), dry, wet: 1. }
        }
        Unit::Twin(t) => {
            // Kernel 0x140ab07d0 (sampler_core dsp/timemod/twin.rs): the
            // effect's output is wet only; the slot's dry level mixes.
            let mut line = Line::single(DelayTime::Ms(0.), 2000.);
            line.native = Native::TwinDelay(t);
            Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: lfo(0., 0., 0.), dry, wet: 1. }
        }
        Unit::Psyche(p) => {
            // Kernel 0x140aadb50 (sampler_core dsp/timemod/psyche.rs): wet only.
            let mut line = Line::single(DelayTime::Ms(0.), 2000.);
            line.native = Native::PsycheDelay(p);
            Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: lfo(0., 0., 0.), dry, wet: 1. }
        }
        Unit::Flanger { depth, feedback, speed, phase, color, unit } => {
            if !(unit_interval(&[depth, feedback, phase]) && speed.is_finite() && speed >= 0. && color.is_finite()) {
                notes.push(("parameters".into(), format!("{depth}/{feedback}/{speed}/{phase}/{color}/{unit}"), Reason::InvalidValue));
                return None;
            }
            if !(0. ..=5.).contains(&color) {
                notes.push(("parameters".into(), format!("color {color}"), Reason::InvalidValue));
                return None;
            }
            // Setter case @0x1409637f0, audio 0x140aa2350: color is the base delay in ms,
            // depth * 50 frames of excursion, feedback * 0.95 subtracted, phase * 0.25 cycles
            // (re-phase 0x140960040); Speed through the shared sync getter 0x140a736a0.
            let mut line = Line::single(DelayTime::Ms(f64::from(color)), 6.);
            line.native = Native::LegacyModDelay {
                base_ms: color,
                excursion: f64::from(depth) * 50.,
                feedback: feedback * f32::from_bits(0x3f733333),
                second_head: false,
                smoothing: 0.,
            };
            Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: lfo(speed, unit, phase * 0.25), dry, wet: 1. }
        }
        Unit::Choral { fields: [amount, rate, delay, voices, width, feedback, mix], invert, scatter, mode } => {
            // Core 0x14193be50; saved fields bind 1:1 to core params p0-p5, p8 and the
            // flags to p7 / p6 (fx_other.tsv); the mode index is (int)(p9 * 4) clamped
            // 0..=3 (0x14193a230). The kernel owns its dry/wet.
            if !(0..=3).contains(&mode) {
                notes.push(("mode".into(), mode.to_string(), Reason::InvalidValue));
                return None;
            }
            let mut line = Line::single(DelayTime::Frames(1), 0.);
            line.native = Native::VoiceBank(choral([amount, rate, delay, voices, width, feedback, mix], invert, scatter, mode as usize));
            Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: lfo(0., 0., 0.), dry: 0., wet: 1. }
        }
    };
    match cfg.check() {
        Ok(()) => Some(cfg),
        Err(field) => {
            notes.push(("parameters".into(), field.into(), Reason::InvalidValue));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Scope, Slot, chain};

    fn slot(module: u16, version: u16, floats: &[f32]) -> Slot {
        Slot {
            slot: 0,
            module,
            version,
            bypass: false,
            output_gain: 1.,
            dry_level: 0.5,
            output_set: true,
            public: floats.iter().flat_map(|f| f.to_le_bytes()).collect(),
        }
    }

    fn config(module: u16, floats: &[f32]) -> (sampler_ir::fx::timemod::Config, Vec<(usize, String, String, sampler_ir::Reason)>) {
        let built = chain(&[slot(module, 0x50, floats)], Scope::Voice);
        let sampler_ir::Processor::TimeMod(config) = built.processors[0] else { panic!("{:?}", built.processors) };
        (config, built.notes)
    }

    #[test]
    fn legacy_phaser_is_exact_and_reports_nothing() {
        let (cfg, notes) = config(0x14, &[0.5, 0.25, 1., 1., -1.]);
        assert!(notes.is_empty());
        assert_eq!(cfg.lfo.phase, 0.25);
        assert_eq!(cfg.allpass.unwrap().feedback, (0.25f64 * 0.85) as f32);
        assert_eq!((cfg.dry, cfg.wet), (0., 1.));
    }

    #[test]
    fn native_delay_chorus_flanger() {
        // Time is stored in ms (measured table 5..2900), Speed in Hz.
        let (delay, notes) = config(0x10, &[250., 0.4, 0.5, 0.25, -1.]);
        let line = delay.line.unwrap();
        assert_eq!(line.native, sampler_ir::fx::timemod::Native::LegacyDelay { time: 250., unit: -1., feedback: 0.4, damping: 0.25, pan: 0.5 });
        assert_eq!(delay.dry, 0.5, "the slot's dry level mixes outside a Mix block");
        assert!(notes.is_empty(), "free-time Legacy Delay is fully native");

        let (chorus, notes) = config(0x11, &[0.5, 2., 1., -1.]);
        assert_eq!(chorus.lfo.rate, sampler_ir::fx::timemod::Rate::Hz(2.));
        assert_eq!((chorus.lfo.phase, chorus.dry), (0.25, 0.5));
        // Native vector (setter 0x140963970): base +8 = 0, excursion depth * 75, two smoothed heads.
        assert_eq!(
            chorus.line.unwrap().native,
            sampler_ir::fx::timemod::Native::LegacyModDelay { base_ms: 0., excursion: 37.5, feedback: 0., second_head: true, smoothing: 0.95 }
        );
        assert!(notes.is_empty(), "{notes:?}");

        // Native vector (setter 0x1409637f0): color 2 ms, depth * 50, feedback * 0.95.
        let (flanger, notes) = config(0x12, &[0.5, 0.5, 1., 0., 2., -1.]);
        assert_eq!(
            flanger.line.unwrap().native,
            sampler_ir::fx::timemod::Native::LegacyModDelay { base_ms: 2., excursion: 25., feedback: 0.5 * 0.95, second_head: false, smoothing: 0. }
        );
        assert!(notes.is_empty());
    }

    #[test]
    fn choral_reads_seven_floats_two_flags_and_the_mode() {
        let mut public: Vec<u8> = [0.25f32, 0.5, 0.18, 1., 0.75, 0.3, 0.6].iter().flat_map(|f| f.to_le_bytes()).collect();
        public.extend([1, 0]);
        public.extend(2i32.to_le_bytes());
        let mut s = slot(0x5d, 0x51, &[]);
        s.public = public;
        let built = chain(&[s], Scope::Voice);
        let sampler_ir::Processor::TimeMod(cfg) = built.processors[0] else { panic!("{:?}", built.processors) };
        assert!(built.notes.is_empty(), "{:?}", built.notes);
        assert_eq!(cfg.line.unwrap().native, sampler_ir::fx::timemod::Native::VoiceBank(super::choral([0.25, 0.5, 0.18, 1., 0.75, 0.3, 0.6], true, false, 2)));
        assert_eq!((cfg.dry, cfg.wet), (0., 1.));
    }

    #[test]
    fn choral_laws_match_native_vectors() {
        use super::choral;
        // Delay 0.18, mode 0: B0 = 0.003f * (0.18f * 5 + 0.1) s (0x14193ac00) -> 144 frames at 48 kHz.
        let c = choral([1., 0.5, 0.18, 0., 0., 0.5, 1.], false, false, 0);
        let v0 = c.voices[0];
        assert!((v0.delay_s * 48_000. - 144.000_006_4).abs() < 1e-6, "{}", v0.delay_s);
        // Rate 0.5 -> 0.125 * 7.4 + 0.25 = 1.175 Hz (0x14193b140); amount 1 -> 2^0.025 (0x14193a2f0).
        assert!((v0.lfo_hz - 1.175).abs() < 1e-12);
        assert!(((48_000. / v0.lfo_hz) * v0.excursion - 357.032).abs() < 1e-3);
        // Feedback 0.5 (0x14193a610): g = 0.5^(1/4) * 0.999, input (1 - g^4)^(1/4).
        assert!((v0.feedback - 0.840_055_518_8).abs() < 1e-9);
        assert!((c.input - 0.841_734_796_5).abs() < 1e-9);
        // Voices (0x14193a3e0): 0 -> sqrt(1) / 2, 1 -> sqrt(1.8) / 6.
        assert_eq!(c.wet, 0.5 * c.input * 0.707);
        let full = choral([0., 0., 0., 1., 0., 0., 1.], false, false, 0);
        assert!((full.wet - 1.8f64.sqrt() / 6. * 0.707).abs() < 1e-15);
        assert_eq!(full.voices.map(|v| v.gain), [1., 1., 1., 1., 1., 1., 0.]);
        // Mode 1 voice 2: LFO2 depth G4 / G3 = 1 / 4, phase offset 0.33333f.
        let m1 = choral([0.; 7], false, false, 1);
        assert_eq!(m1.voices[2].lfo2, 0.25);
        assert_eq!(m1.voices[2].offset, f64::from(f32::from_bits(0x3eaaaa3b)));
    }

    /// Render `input` (48 kHz) through `processors` on a voice chain.
    fn render(processors: Vec<sampler_ir::Processor>, input: &[[f32; 2]]) -> Vec<[f32; 2]> {
        let mut instrument = sampler_ir::Instrument::default();
        instrument.assets.push(sampler_ir::Asset { location: sampler_ir::AssetLocation::Path("probe".into()), encoding: sampler_ir::Encoding::Wav, root_key: None, loops: vec![] });
        instrument.chains.push(sampler_ir::Chain { scope: sampler_ir::Scope::Voice, pre_amplitude: processors, post_amplitude: vec![] });
        let mut zone = sampler_ir::Zone::new(sampler_ir::AssetRef(0));
        zone.keys = sampler_ir::KeyRange { low: 60, high: 60 };
        zone.chain = Some(sampler_ir::ChainRef(0));
        zone.velocity = sampler_ir::VelocityResponse::None;
        instrument.zones.push(zone);
        let pcm = sampler_core::Pcm::new(48000, input.to_vec().into_boxed_slice()).unwrap();
        let plan = sampler_core::lower::lower(&instrument, 48000, vec![pcm], |_, p| Ok(p)).unwrap();
        let limits = sampler_core::Limits::for_plan(&plan, 4, 4);
        let mut rt = sampler_core::Runtime::new(plan, limits).unwrap();
        let key = sampler_core::Input { protocol: sampler_core::Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: None };
        rt.trigger(key, 60, 1.0).unwrap();
        let mut y = vec![[0f32; 2]; input.len()];
        rt.render(&mut y).unwrap();
        y
    }

    /// The kemu stimulus: `0.5 sin(0.05 n) + impulse` / `0.5 sin(0.031 n)` (f32).
    fn stimulus(n: u32) -> Vec<[f32; 2]> {
        (0..n)
            .map(|i| {
                let t = f64::from(i);
                [((t * 0.05).sin() * 0.5 + if i == 0 { 1. } else { 0. }) as f32, ((t * 0.031).sin() * 0.5) as f32]
            })
            .collect()
    }

    fn assert_ulps(y: &[[f32; 2]], want: &[(usize, u32, u32)], what: &str) {
        for &(i, l, r) in want {
            let ulps = |g: f32, w: u32| (i64::from(g.to_bits()) - i64::from(w)).abs();
            assert!(ulps(y[i][0], l) <= 1 && ulps(y[i][1], r) <= 1, "{what} frame {i}: {:?} vs {:?}", y[i], [f32::from_bits(l), f32::from_bits(r)]);
        }
    }

    /// Kontakt 8.13.1 under tools/re/kemu.py: object as built by 0x1407c7010
    /// (Chorus) / 0x1407c75a0 (Flanger), prepare 0x140960f00 at 48 kHz, the
    /// setter cases 0x140963981 / 0x140963801 on the saved fields, reset
    /// (vtable slot 5), audio 0x140aa0510 / 0x140aa2350 in 32-frame blocks
    /// (send helpers 0x140ac28f0 / 0x140ac21f0 stubbed). The harness sets the
    /// LFO objects' rate field (+0xc) to the sample rate, whose writer is not
    /// located. Wet output, f32 bits.
    #[test]
    fn legacy_chorus_and_flanger_render_native_kemu_vectors() {
        let input = stimulus(2000);
        let mut notes = Vec::new();
        let chorus = super::config(&super::Unit::Chorus { depth: 0.6, speed: 1.5, phase: 0.3, unit: -1. }, 0., &mut notes).unwrap();
        let y = render(vec![sampler_ir::Processor::TimeMod(chorus)], &input);
        assert_ulps(&y, &[(0, 0x3dbf2cd5, 0), (1, 0x3fed4542, 0x3c864249), (150, 0x3f6e9348, 0xbf87a26f), (777, 0x3f6c99aa, 0xbf6cf386), (1500, 0xbedffe9a, 0x3f1fce4e), (1999, 0xbf1aa44c, 0xbf4d2a8e)], "chorus");
        let flanger = super::config(&super::Unit::Flanger { depth: 0.7, feedback: 0.6, speed: 0.8, phase: 0.4, color: 2.5, unit: -1. }, 0., &mut notes).unwrap();
        let y = render(vec![sampler_ir::Processor::TimeMod(flanger)], &input);
        assert_ulps(&y, &[(1, 0, 0), (150, 0x3efd2f1d, 0x3dfad0db), (777, 0x3e93b433, 0x3dc530ac), (1500, 0xbe081447, 0xbe84e6f9), (1999, 0xbe4acd84, 0x3e0fa710)], "flanger");
        assert!(notes.is_empty(), "{notes:?}");
    }

    /// Kontakt 8.13.1 under tools/re/kemu.py: native ctor 0x141939b10,
    /// prepare 0x14193b840 at 48 kHz, setters, process 0x14193be50 in 32-frame
    /// blocks on `0.5 sin(0.05 n) + impulse` / `0.5 sin(0.031 n)` (f32); output
    /// f32 bits. Rendered here from the saved bytes through the engine.
    #[test]
    fn choral_renders_native_kemu_vectors() {
        let input = stimulus(3000);
        let cases: [([f32; 7], bool, bool, i32, [(usize, u32, u32); 6]); 2] = [
            ([0.6, 0.5, 0.18, 0.7, 0.4, 0.5, 0.8], true, true, 1, [(0, 0x3f32c0a0, 0), (1, 0x3c7145ad, 0x3c314b45), (150, 0x3ea49034, 0xbebce4dd), (777, 0x3e88e9e0, 0xbe871bd9), (1500, 0xbe883379, 0x3e1418bb), (2999, 0xbdc85a44, 0xbea4ea2d)]),
            ([0.3, 0.8, 0.5, 1.0, 1.0, 0.9, 1.0], false, true, 2, [(0, 0x3e820df2, 0), (1, 0x3bc4ddf0, 0x3b80fe4e), (150, 0x3defac3e, 0xbe071ad4), (777, 0x3ded953d, 0xbddf899f), (1500, 0xbd45a8a0, 0xbdf18584), (2999, 0xbe25b166, 0x3d3db892)]),
        ];
        for (fields, invert, scatter, mode, want) in cases {
            let mut public: Vec<u8> = fields.iter().flat_map(|f| f.to_le_bytes()).collect();
            public.extend([u8::from(invert), u8::from(scatter)]);
            public.extend(mode.to_le_bytes());
            let mut s = slot(0x5d, 0x51, &[]);
            s.public = public;
            let built = chain(&[s], Scope::Voice);
            assert!(built.notes.is_empty(), "{:?}", built.notes);
            let y = render(built.processors, &input);
            assert_ulps(&y, &want, &format!("choral mode {mode}"));
        }
    }

    #[test]
    fn replika_and_friends_stay_unsupported_with_a_reason() {
        for module in [0x5a, 0x5b, 0x5c, 0x22] {
            let built = chain(&[slot(module, 0x50, &[0.; 12])], Scope::Voice);
            assert!(built.processors.is_empty());
            assert!(built.notes.iter().any(|n| n.3 == sampler_ir::Reason::NotModeled), "{module:#x}");
        }
    }

    /// Reader 0x140ac5bc0: six floats, three bytes, three floats, a byte.
    #[test]
    fn psyche_delay_reads_its_stream_and_runs_native() {
        let mut public = Vec::new();
        for f in [20f32, 0.5, 0.7, 0.75, 0.6, 0.25] {
            public.extend(f.to_le_bytes());
        }
        public.extend([0, 1, 0]);
        for f in [-1f32, 500., 0.] {
            public.extend(f.to_le_bytes());
        }
        public.push(0);
        let built = chain(&[Slot { public, ..slot(0x63, 0x10, &[]) }], Scope::Voice);
        let sampler_ir::Processor::TimeMod(config) = built.processors[0] else { panic!("{:?}", built.notes) };
        let sampler_ir::fx::timemod::Native::PsycheDelay(p) = config.line.unwrap().native else { panic!() };
        assert_eq!((p.time, p.feedback, p.detune, p.pitch, p.offset, p.cross, p.unit), (20., 0.5, 0.7, 0.75, 0.6, 0.25, -1.));
        assert!(!p.reverse_stereo && p.detune_stereo && !p.reverse);
        assert!(built.notes.is_empty(), "{:?}", built.notes);
    }

    /// Reader 0x140ac6280: four sync groups, then ten floats.
    #[test]
    fn twin_delay_reads_its_stream_and_runs_native() {
        let mut public = Vec::new();
        for _ in 0..4 {
            for f in [-1f32, 0.5, 0.] {
                public.extend(f.to_le_bytes());
            }
            public.push(0);
        }
        for f in [0.001f32, 0.0001, 0.6, 0.9, 0.0013, 0.00021, 0.5, 0.7, 0.3, 0.8] {
            public.extend(f.to_le_bytes());
        }
        let built = chain(&[Slot { public, ..slot(0x6b, 0x10, &[]) }], Scope::Voice);
        let sampler_ir::Processor::TimeMod(config) = built.processors[0] else { panic!("{:?}", built.notes) };
        let sampler_ir::fx::timemod::Native::TwinDelay(t) = config.line.unwrap().native else { panic!() };
        assert_eq!((t.time, t.predelay, t.feedback, t.level, t.cross, t.width), ([0.0001, 0.00021], [0.001, 0.0013], [0.6, 0.5], [0.9, 0.7], 0.3, 0.8));
        assert!(built.notes.is_empty(), "{:?}", built.notes);
    }
}
