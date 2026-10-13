//! Kontakt delay and modulation effects onto `sampler_ir::fx::timemod`.
//!
//! Native (Kontakt 8.13.1 binary): Legacy Phaser (0x14), Legacy Delay (0x10,
//! audio 0x140aa1130) and Legacy Flanger (0x12, audio 0x140aa2350). Chorus
//! (0x11, audio 0x140aa0510) stays generic: its setter (0x140963970) reads a
//! base delay from saved-struct +8, which neither the reader (0x140d00760) nor
//! the constructor writes, so the value is unknown. The modern effects stay
//! unsupported with the reasons in [`unproved`].
use super::{Notes, Reader};
use sampler_ir::{
    Reason,
    fx::timemod::{
        Allpass, Config, DelayTime, Interpolation, Lfo, LfoShape, Line, Native, Order, Rate, SweepLaw,
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

/// Why a delay or modulation module without a mapping stays unsupported.
pub(super) fn unproved(module: u16) -> Option<&'static str> {
    Some(match module {
        0x5a => "Replika: modern core behind wrapper 0x1408f98d0 (53 controls; core +0x1c0 setParam slot 0x60, update 0x40, process 0x38); the core class, its style kernels and the saved-field binding are not extracted",
        0x5b => "Phasis: modern wrapper core not located; no saved-field binding measured",
        0x5c => "Flair: field binding measured (fx_other.tsv) but the modern wrapper core and its audio laws are not extracted",
        0x5d => "Choral: field binding measured (fx_other.tsv) but the modern wrapper core and its audio laws are not extracted",
        0x63 => "Psyche Delay: modern wrapper core not located; no saved-field binding measured",
        0x69 => "Vibrato Chorus: modern wrapper core not located; no saved-field binding measured",
        _ => return None,
    })
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
            // Setter case 0x43 @0x1409645ae, audio 0x140aa1130: feedback * 0.98, damping * 0.7,
            // pan crossing, ring 2^18. The synced time goes through the time object's vtable
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
            generic(notes, "chorus base delay (setter 0x140963970 reads unwritten struct +8; audio 0x140aa0510 taps at base and base/2, depth * 75 frames)", format!("{depth}/{speed}/{phase}"));
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
            if !(0. ..=5.).contains(&color) {
                notes.push(("parameters".into(), format!("color {color}"), Reason::InvalidValue));
                return None;
            }
            // Setter case 0x12 @0x1409637f0, audio 0x140aa2350: color is the base delay in ms,
            // depth * 50 frames of excursion, feedback * 0.95 subtracted, phase * 0.25 cycles.
            if unit_set(unit).is_some() {
                generic(notes, "tempo-synced modulation speed", "Phaser Hz getter; the speed object's class was not confirmed".into());
            }
            let mut line = Line::single(DelayTime::Ms(f64::from(color)), 6.);
            line.native = Native::LegacyFlanger { color, depth, feedback };
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
    fn native_delay_and_generic_chorus_flanger() {
        // Time is stored in ms (measured table 5..2900), Speed in Hz.
        let (delay, notes) = config(0x10, &[250., 0.4, 0.5, 0.25, -1.]);
        let line = delay.line.unwrap();
        assert_eq!(line.native, sampler_ir::fx::timemod::Native::LegacyDelay { time: 250., unit: -1., feedback: 0.4, damping: 0.25, pan: 0.5 });
        assert_eq!(delay.dry, 0.5, "the slot's dry level mixes outside a Mix block");
        assert!(notes.is_empty(), "free-time Legacy Delay is fully native");

        let (chorus, notes) = config(0x11, &[0.5, 2., 1., -1.]);
        assert_eq!(chorus.lfo.rate, sampler_ir::fx::timemod::Rate::Hz(2.));
        assert!(!notes.is_empty());

        let (flanger, notes) = config(0x12, &[0.5, 0.5, 1., 0., 2., -1.]);
        assert_eq!(flanger.line.unwrap().native, sampler_ir::fx::timemod::Native::LegacyFlanger { color: 2., depth: 0.5, feedback: 0.5 });
        assert!(notes.is_empty());
    }

    #[test]
    fn replika_and_friends_stay_unsupported_with_a_reason() {
        for module in [0x5a, 0x5b, 0x5c, 0x5d, 0x22] {
            let built = chain(&[slot(module, 0x50, &[0.; 12])], Scope::Voice);
            assert!(built.processors.is_empty());
            assert!(built.notes.iter().any(|n| n.3 == sampler_ir::Reason::NotModeled), "{module:#x}");
        }
    }
}
