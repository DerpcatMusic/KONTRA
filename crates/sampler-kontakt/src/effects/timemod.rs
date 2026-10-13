//! Kontakt delay and modulation effects onto `sampler_ir::fx::timemod`.
//!
//! Exact: Legacy Phaser (0x14). Value-exact but audio-generic: Legacy Delay
//! (0x10), Chorus (0x11), Flanger (0x12): the stored fields are measured
//! (docs/re/kontakt-bindings/fx_legacy_delay_reverb_mod.tsv: time in ms, speed
//! in Hz, the rest 0..1) but their audio laws were never extracted, so those
//! three report `UnknownLaw` and use clearly generic topologies. Replika,
//! Phasis, Flair, Choral, Rotator, Psyche Delay and Vibrato Chorus have no
//! proved field binding and stay unsupported.
use super::{Notes, Reader};
use sampler_ir::{
    Reason,
    fx::timemod::{
        Allpass, Config, DelayTime, Interpolation, Lfo, LfoShape, Line, Order, Rate, SweepLaw, Tap,
    },
};

/// Stored fields, in file order. `unit` > 0 selects the tempo-synced period.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Unit {
    Phaser { depth: f32, feedback: f32, speed: f32, phase: f32, unit: f32 },
    Delay { time: f32, feedback: f32, pan: f32, damping: f32, unit: f32 },
    Chorus { depth: f32, speed: f32, phase: f32, unit: f32 },
    Flanger { depth: f32, feedback: f32, speed: f32, phase: f32, color: f32, unit: f32 },
}

/// v0x50 stops after `unit`; v0x51 adds two floats and a flag the engine does not read.
pub(super) fn read(module: u16, version: u16, r: &mut Reader) -> Option<Unit> {
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

fn unit_set(unit: f32) -> Option<f32> {
    // Kontakt's sync threshold 0x144f38cd0 (speed getter 0x140a736a0).
    (unit > f32::from_bits(0x3089705f)).then_some(unit)
}

fn rate(speed: f32, unit: f32) -> Rate {
    unit_set(unit).map_or(Rate::Hz(speed), |unit| Rate::Sync { unit, speed })
}

fn unit_interval(values: &[f32]) -> bool {
    values.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v))
}

fn generic(notes: &mut Notes, what: &str, value: String) {
    notes.push((what.into(), value, Reason::UnknownLaw));
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
            generic(notes, "delay feedback, damping and pan laws", format!("{feedback}/{damping}/{pan}"));
            if unit_set(unit).is_some() {
                generic(notes, "tempo-synced delay time", format!("unit {unit}; free time used"));
            }
            let mut line = Line::single(DelayTime::Ms(f64::from(time)), 2900.);
            line.taps[0].pan = f64::from(pan) * 2. - 1.;
            let fb = f64::from(feedback).min(0.99);
            line.feedback.matrix = [[fb, 0.], [0., fb]];
            line.feedback.lowpass = (1. - f64::from(damping)).max(0.05);
            Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: lfo(0., 0., 0.), dry, wet: 1. }
        }
        Unit::Chorus { depth, speed, phase, unit } => {
            if !(unit_interval(&[depth, phase]) && speed.is_finite() && speed >= 0.) {
                notes.push(("parameters".into(), format!("{depth}/{speed}/{phase}/{unit}"), Reason::InvalidValue));
                return None;
            }
            generic(notes, "chorus delay, depth, voices and mix laws", format!("{depth}/{speed}/{phase}"));
            if unit_set(unit).is_some() {
                generic(notes, "tempo-synced modulation speed", "Phaser getter assumed".into());
            }
            let mut line = Line::single(DelayTime::Ms(12.), 20.);
            line.taps[0].depth_ms = f64::from(depth) * 6.;
            line.interpolation = Interpolation::Hermite;
            Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: lfo(speed, unit, phase * 0.25), dry, wet: 1. }
        }
        Unit::Flanger { depth, feedback, speed, phase, color, unit } => {
            if !(unit_interval(&[depth, feedback, phase]) && speed.is_finite() && speed >= 0. && color.is_finite()) {
                notes.push(("parameters".into(), format!("{depth}/{feedback}/{speed}/{phase}/{color}/{unit}"), Reason::InvalidValue));
                return None;
            }
            generic(notes, "flanger delay, depth, feedback sign and color laws", format!("{depth}/{feedback}/{color}"));
            if unit_set(unit).is_some() {
                generic(notes, "tempo-synced modulation speed", "Phaser getter assumed".into());
            }
            let mut line = Line::single(DelayTime::Ms(2.5), 6.);
            line.taps[0] = Tap { time: DelayTime::Ms(2.5), level: 1., pan: 0., depth_ms: f64::from(depth) * 2.4 };
            line.interpolation = Interpolation::Hermite;
            let fb = f64::from(feedback).min(0.99);
            line.feedback.matrix = [[fb, 0.], [0., fb]];
            Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: lfo(speed, unit, phase * 0.25), dry, wet: 1. }
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
