//! Kontakt delay and modulation effects onto `sampler_ir::fx::timemod`.
//!
//! Native (Kontakt 8.13.1 binary): Legacy Phaser (0x14), Legacy Delay (0x10,
//! audio 0x140aa1130) and Legacy Flanger (0x12, audio 0x140aa2350). Chorus
//! (0x11, audio 0x140aa0510) stays generic: its setter (0x140963970) reads a
//! base delay from saved-struct +8, which neither the reader (0x140d00760) nor
//! the constructor writes, so the value is unknown. Twin Delay (0x6B, kernel
//! 0x140ab07d0) and Psyche Delay (0x63, kernel 0x140aadb50) are native; the other modern effects stay unsupported with
//! the reasons in [`unproved`].
use super::{Notes, Reader};
use sampler_ir::{
    Reason,
    fx::timemod::{
        Allpass, Config, DelayTime, Interpolation, Lfo, LfoShape, Line, Native, Order, Rate, PsycheDelay, SweepLaw, TwinDelay,
    },
};

/// Stored fields, in file order. `unit` > 0 selects the tempo-synced period.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Unit {
    Phaser { depth: f32, feedback: f32, speed: f32, phase: f32, unit: f32 },
    Delay { time: f32, feedback: f32, pan: f32, damping: f32, unit: f32 },
    Chorus { depth: f32, speed: f32, phase: f32, unit: f32 },
    Flanger { depth: f32, feedback: f32, speed: f32, phase: f32, color: f32, unit: f32 },
    Twin(TwinDelay),
    Psyche(PsycheDelay),
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
        0x5b => "Phasis: modern wrapper core not located; no saved-field binding measured",
        0x5c => "Flair: field binding measured (fx_other.tsv) but the modern wrapper core and its audio laws are not extracted",
        0x5d => "Choral: field binding measured (fx_other.tsv) but the modern wrapper core and its audio laws are not extracted",
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
            generic(notes, "chorus base delay (setter 0x140963970 reads unwritten struct +8; audio 0x140aa0510 taps at base and base/2, depth * 75 frames)", format!("{depth}/{speed}/{phase}"));
            if unit_set(unit).is_some() {
                generic(notes, "tempo-synced modulation speed", "Phaser getter assumed".into());
            }
            let mut line = Line::single(DelayTime::Ms(12.), 20.);
            line.taps[0].depth_ms = f64::from(depth) * 6.;
            line.interpolation = Interpolation::Hermite;
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
