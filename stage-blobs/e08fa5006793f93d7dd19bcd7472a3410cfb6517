//! Structural checks shared by every consumer, so lowering can index freely.
use crate::{
    Chain, Depth, Envelope, Gain, Instrument, Looping, ModulationSource, Output, Pan, Pitch,
    Processor, Scope, Take, Target, Time,
};
use std::fmt;

/// What a dangling index pointed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reference {
    Asset(usize),
    Group(usize),
    Sequence(usize),
    Articulation(usize),
    Modulator(usize),
    Chain(usize),
    Bus(usize),
    Control(usize),
    Route(usize),
    Shape(usize),
    VoiceLimit(usize),
    Impulse(usize),
    Processor { chain: usize, index: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ValidationError {
    Dangling {
        owner: String,
        reference: Reference,
    },
    /// A low/high pair with low above high.
    InvertedRange {
        owner: String,
        field: &'static str,
    },
    /// Non-finite, negative where forbidden, or beyond the field's range.
    OutOfRange {
        owner: String,
        field: &'static str,
        value: f64,
    },
    /// A take index at or beyond its sequence's take count.
    TakeOutOfSequence {
        zone: usize,
        take: u32,
        takes: u32,
    },
    /// Articulation switching that cannot select unambiguously.
    Switching {
        owner: String,
        reason: &'static str,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dangling { owner, reference } => {
                write!(f, "{owner} refers to missing {reference:?}")
            }
            Self::InvertedRange { owner, field } => {
                write!(f, "{owner}: {field} low is above high")
            }
            Self::OutOfRange {
                owner,
                field,
                value,
            } => {
                write!(f, "{owner}: {field} = {value} is out of range")
            }
            Self::TakeOutOfSequence { zone, take, takes } => {
                write!(f, "zone {zone}: take {take} of a {takes}-take sequence")
            }
            Self::Switching { owner, reason } => write!(f, "{owner}: {reason}"),
        }
    }
}

impl std::error::Error for ValidationError {}

struct Check<'a> {
    ir: &'a Instrument,
    owner: String,
}

impl Check<'_> {
    fn exists(&self, reference: Reference) -> Result<(), ValidationError> {
        let ir = self.ir;
        let present = match reference {
            Reference::Asset(i) => i < ir.assets.len(),
            Reference::Group(i) => i < ir.groups.len(),
            Reference::Sequence(i) => i < ir.sequences.len(),
            Reference::Articulation(i) => i < ir.articulations.len(),
            Reference::Modulator(i) => i < ir.modulators.len(),
            Reference::Chain(i) => i < ir.chains.len(),
            Reference::Bus(i) => i < ir.buses.len(),
            Reference::Control(i) => i < ir.controls.len(),
            Reference::Route(i) => i < ir.routes.len(),
            Reference::Shape(i) => i < ir.shapes.len(),
            Reference::VoiceLimit(i) => i < ir.voice_limits.len(),
            Reference::Impulse(i) => i < ir.impulses.len(),
            Reference::Processor { chain, index } => ir
                .chains
                .get(chain)
                .is_some_and(|c| index < c.pre_amplitude.len() + c.post_amplitude.len()),
        };
        if present {
            Ok(())
        } else {
            Err(ValidationError::Dangling {
                owner: self.owner.clone(),
                reference,
            })
        }
    }

    fn range<T: PartialOrd>(
        &self,
        low: T,
        high: T,
        field: &'static str,
    ) -> Result<(), ValidationError> {
        if low > high {
            return Err(ValidationError::InvertedRange {
                owner: self.owner.clone(),
                field,
            });
        }
        Ok(())
    }

    fn within(
        &self,
        value: f64,
        bounds: std::ops::RangeInclusive<f64>,
        field: &'static str,
    ) -> Result<(), ValidationError> {
        if value.is_finite() && bounds.contains(&value) {
            Ok(())
        } else {
            Err(ValidationError::OutOfRange {
                owner: self.owner.clone(),
                field,
                value,
            })
        }
    }

    fn finite(&self, value: f64, field: &'static str) -> Result<(), ValidationError> {
        self.within(value, f64::MIN..=f64::MAX, field)
    }

    fn gain(&self, gain: Gain, field: &'static str) -> Result<(), ValidationError> {
        match gain {
            Gain::Decibels(db) => self.finite(db, field),
            Gain::Linear(factor) => self.finite(factor, field),
        }
    }

    fn pitch(&self, pitch: Pitch, field: &'static str) -> Result<(), ValidationError> {
        match pitch {
            Pitch::Ratio(ratio) => self.within(ratio, f64::MIN_POSITIVE..=f64::MAX, field),
            other => self.finite(other.semitones(), field),
        }
    }

    fn pan(&self, pan: Pan, field: &'static str) -> Result<(), ValidationError> {
        self.within(pan.position, -1.0..=1.0, field)
    }

    fn time(&self, time: Time, field: &'static str) -> Result<(), ValidationError> {
        self.within(time.seconds(), 0.0..=f64::MAX, field)
    }

    fn output(&self, output: Output) -> Result<(), ValidationError> {
        match output {
            Output::Master => Ok(()),
            Output::Bus(bus) => self.exists(Reference::Bus(bus.0)),
        }
    }

    fn scope(&self, scope: Scope) -> Result<(), ValidationError> {
        match scope {
            Scope::Voice | Scope::Master => Ok(()),
            Scope::Group(group) => self.exists(Reference::Group(group.0)),
            Scope::Bus(bus) => self.exists(Reference::Bus(bus.0)),
        }
    }

    fn envelope(&self, e: &Envelope) -> Result<(), ValidationError> {
        self.time(e.delay, "delay")?;
        self.time(e.attack, "attack")?;
        self.time(e.hold, "hold")?;
        self.time(e.decay, "decay")?;
        self.time(e.release, "release")?;
        self.within(e.sustain, 0.0..=1.0, "sustain")
    }

    fn chain(&self, chain: &Chain) -> Result<(), ValidationError> {
        self.scope(chain.scope)?;
        for processor in chain.pre_amplitude.iter().chain(&chain.post_amplitude) {
            match *processor {
                Processor::Gain(gain) => self.gain(gain, "gain")?,
                Processor::Pan(pan) => self.pan(pan, "pan")?,
                Processor::Reverb(r) => {
                    for (v, field) in [
                        (r.decay_seconds, "reverb decay"),
                        (r.size, "reverb size"),
                        (r.damping_hz, "reverb damping"),
                        (r.modulation_seconds, "reverb modulation"),
                        (r.diffusion, "reverb diffusion"),
                        (r.predelay_seconds, "reverb predelay"),
                        (r.input_cutoff_hz, "reverb input cutoff"),
                        (r.low_shelf_db, "reverb low shelf"),
                        (r.width, "reverb width"),
                    ] {
                        self.finite(v, field)?;
                    }
                }
                Processor::Convolution { impulse, dry, wet } => {
                    self.exists(Reference::Impulse(impulse.0))?;
                    self.finite(dry, "convolution dry")?;
                    self.finite(wet, "convolution wet")?;
                }
                Processor::StereoMatrix(matrix) => {
                    for x in matrix.as_flattened() {
                        self.finite(*x, "stereo matrix")?;
                    }
                }
                Processor::Filter(filter) => {
                    if let crate::Frequency::Hertz(hz) = filter.cutoff {
                        self.within(hz, f64::MIN_POSITIVE..=f64::MAX, "cutoff")?;
                    }
                }
                Processor::Delay {
                    time,
                    feedback,
                    mix,
                } => {
                    self.time(time, "delay")?;
                    self.within(feedback, -1.0..=1.0, "feedback")?;
                    self.within(mix, 0.0..=1.0, "mix")?;
                }
            }
        }
        Ok(())
    }
}

impl Instrument {
    /// Every reference resolves, every range is ordered and every quantity is
    /// finite and in range. Consumers may index without further checks.
    pub fn validate(&self) -> Result<(), ValidationError> {
        let mut check = Check {
            ir: self,
            owner: String::new(),
        };
        for (i, impulse) in self.impulses.iter().enumerate() {
            check.owner = format!("impulse {i}");
            check.within(f64::from(impulse.rate), 1.0..=f64::from(u32::MAX), "rate")?;
            check.within(
                impulse.left.len() as f64,
                1.0..=f64::from(1u32 << 24),
                "frames",
            )?;
            check.within(
                impulse.right.len() as f64,
                impulse.left.len() as f64..=impulse.left.len() as f64,
                "right frames",
            )?;
            for x in impulse.left.iter().chain(&impulse.right) {
                check.finite(f64::from(*x), "sample")?;
            }
        }
        for (i, group) in self.groups.iter().enumerate() {
            check.owner = format!("group {i}");
            check.gain(group.gain, "gain")?;
            check.pan(group.pan, "pan")?;
            check.pitch(group.tune, "tune")?;
            if let Some(chain) = group.chain {
                check.exists(Reference::Chain(chain.0))?;
            }
            check.output(group.output)?;
            for send in &group.sends {
                check.output(Output::Bus(send.to))?;
                check.gain(send.gain, "send")?;
            }
            if let Some(limit) = group.voice_limit {
                check.exists(Reference::VoiceLimit(limit))?;
            }
        }
        for (i, limit) in self
            .voice_limit
            .iter()
            .chain(&self.voice_limits)
            .enumerate()
        {
            check.owner = format!("voice limit {i}");
            check.within(f64::from(limit.voices), 1.0..=f64::from(u32::MAX), "voices")?;
            check.time(limit.fade, "fade")?;
        }
        for (i, zone) in self.zones.iter().enumerate() {
            check.owner = format!("zone {i}");
            check.exists(Reference::Asset(zone.asset.0))?;
            if let Some(group) = zone.group {
                check.exists(Reference::Group(group.0))?;
            }
            check.range(zone.keys.low, zone.keys.high, "keys")?;
            check.within(f64::from(zone.keys.high), 0.0..=127.0, "keys")?;
            check.range(zone.velocities.low, zone.velocities.high, "velocities")?;
            check.within(f64::from(zone.velocities.high), 0.0..=127.0, "velocities")?;
            for condition in &zone.conditions {
                check.range(condition.low, condition.high, "controller")?;
                check.within(f64::from(condition.controller), 0.0..=127.0, "controller")?;
            }
            if let Some(selection) = zone.selection {
                check.exists(Reference::Sequence(selection.sequence.0))?;
                match selection.take {
                    Take::Index(take) => {
                        let takes = self.sequences[selection.sequence.0].takes;
                        if take >= takes {
                            return Err(ValidationError::TakeOutOfSequence {
                                zone: i,
                                take,
                                takes,
                            });
                        }
                    }
                    Take::Probability { low, high } => {
                        check.within(low, 0.0..=1.0, "random")?;
                        check.within(high, 0.0..=1.0, "random")?;
                        check.range(low, high, "random")?;
                    }
                }
            }
            if let Some(articulation) = zone.articulation {
                check.exists(Reference::Articulation(articulation.0))?;
            }
            check.pitch(zone.tune, "tune")?;
            check.gain(zone.gain, "gain")?;
            check.pan(zone.pan, "pan")?;
            if let Some(end) = zone.playback.end {
                check.range(zone.playback.start, end, "playback")?;
            }
            if let Looping::Continuous(range) | Looping::UntilRelease(range) = zone.playback.looping
            {
                check.range(range.start, range.end, "loop")?;
                if let crate::Span::Time(time) = range.crossfade {
                    check.time(time, "loop crossfade")?;
                }
            }
            if let Some(chain) = zone.chain {
                check.exists(Reference::Chain(chain.0))?;
            }
            if let Some(modulator) = zone.amplitude {
                check.exists(Reference::Modulator(modulator.0))?;
            }
            for route in &zone.routes {
                check.exists(Reference::Route(route.0))?;
            }
        }
        self.validate_switching(&mut check)?;
        for (i, modulator) in self.modulators.iter().enumerate() {
            check.owner = format!("modulator {i}");
            check.scope(modulator.scope)?;
            match &modulator.source {
                ModulationSource::Envelope(envelope) => check.envelope(envelope)?,
                ModulationSource::Breakpoints(b) => {
                    for point in &b.points {
                        check.time(point.time, "breakpoint time")?;
                        check.within(point.level, 0.0..=1.0, "breakpoint level")?;
                    }
                    if b.sustain.is_some_and(|s| s >= b.points.len()) {
                        check.within(f64::NAN, 0.0..=0.0, "sustain point")?;
                    }
                }
                ModulationSource::Lfo(lfo) => {
                    let rate = match lfo.rate {
                        crate::Frequency::Hertz(hz) => hz,
                        crate::Frequency::Beats(beats) => beats,
                    };
                    check.within(rate, f64::MIN_POSITIVE..=f64::MAX, "rate")?;
                    check.time(lfo.delay, "delay")?;
                    check.time(lfo.fade_in, "fade in")?;
                    check.within(lfo.phase, 0.0..=1.0, "phase")?;
                }
                _ => {}
            }
        }
        for (i, shape) in self.shapes.iter().enumerate() {
            check.owner = format!("shape {i}");
            let mut previous = f64::NEG_INFINITY;
            for &(input, output) in &shape.points {
                check.within(input, 0.0..=1.0, "input")?;
                check.finite(output, "output")?;
                check.range(previous, input, "input")?;
                previous = input;
            }
        }
        for (i, route) in self.routes.iter().enumerate() {
            check.owner = format!("route {i}");
            check.exists(Reference::Modulator(route.source.0))?;
            match route.target {
                Target::Processor { chain, index, .. } => {
                    check.exists(Reference::Processor {
                        chain: chain.0,
                        index,
                    })?;
                }
                Target::Control(control) => check.exists(Reference::Control(control.0))?,
                Target::Amplitude | Target::Pitch | Target::Pan | Target::SampleStart => {}
            }
            if let Some(shape) = route.shape {
                check.exists(Reference::Shape(shape.0))?;
            }
            if let Some(scale) = route.scale {
                check.exists(Reference::Modulator(scale.source.0))?;
                if let Some(shape) = scale.shape {
                    check.exists(Reference::Shape(shape.0))?;
                }
            }
            check.time(route.smoothing, "smoothing")?;
            match route.depth {
                Depth::Gain(gain) => check.gain(gain, "depth")?,
                Depth::Pitch(pitch) => check.pitch(pitch, "depth")?,
                Depth::Normalized(value) => check.finite(value, "depth")?,
            }
        }
        for (i, chain) in self.chains.iter().enumerate() {
            check.owner = format!("chain {i}");
            check.chain(chain)?;
        }
        for (i, bus) in self.buses.iter().enumerate() {
            check.owner = format!("bus {i}");
            if let Some(chain) = bus.chain {
                check.exists(Reference::Chain(chain.0))?;
            }
            check.output(bus.output)?;
            for send in &bus.sends {
                check.output(send.to)?;
                check.gain(send.gain, "send")?;
            }
        }
        Ok(())
    }
}

impl Instrument {
    fn validate_switching(&self, check: &mut Check) -> Result<(), ValidationError> {
        use crate::{Driver, SwitchKeys, SwitchOwner};
        let switching = self.switching;
        let fail = |owner: &str, reason| {
            Err(ValidationError::Switching {
                owner: owner.into(),
                reason,
            })
        };
        if self.articulations.iter().filter(|a| a.default).count() > 1 {
            return fail("articulations", "more than one default");
        }
        if switching.owner == SwitchOwner::Behavior {
            if switching.keys == SwitchKeys::Play {
                return fail(
                    "switching",
                    "behavior-owned switch keys cannot be freed to play",
                );
            }
            if let Some(i) = self.zones.iter().position(|z| z.articulation.is_some()) {
                return fail(
                    &format!("zone {i}"),
                    "behavior-owned switching tags no zones",
                );
            }
        }
        let mut keys = [false; 128];
        for (i, a) in self.articulations.iter().enumerate() {
            check.owner = format!("articulation {i}");
            for &key in &a.switch_keys {
                check.within(f64::from(key), 0.0..=127.0, "switch key")?;
                if std::mem::replace(&mut keys[usize::from(key)], true) {
                    return fail(&check.owner, "switch key shared with another articulation");
                }
            }
            let alt = a.alternatives;
            if let Some(v) = alt.velocities {
                check.range(v.low, v.high, "velocities")?;
                check.within(f64::from(v.low), 1.0..=127.0, "velocities")?;
                check.within(f64::from(v.high), 1.0..=127.0, "velocities")?;
            }
            if let Some(channel) = alt.channel {
                check.within(f64::from(channel), 0.0..=15.0, "channel")?;
            }
            if let Some(c) = alt.controller {
                check.within(f64::from(c.controller), 0.0..=119.0, "controller")?;
                check.range(c.low, c.high, "controller")?;
                check.within(f64::from(c.high), 0.0..=127.0, "controller")?;
            }
            if let Some(program) = alt.program {
                check.within(f64::from(program), 0.0..=127.0, "program")?;
            }
            let needed = match switching.driver {
                Driver::Keys => true,
                Driver::Velocity => alt.velocities.is_some(),
                Driver::Channel => alt.channel.is_some(),
                Driver::Controller => alt.controller.is_some(),
                Driver::Program => alt.program.is_some(),
            };
            if !needed {
                return fail(&check.owner, "no value for the active driver");
            }
            if switching.owner == SwitchOwner::Behavior && a.switch_keys.is_empty() {
                return fail(
                    &check.owner,
                    "behavior-owned articulation has no key to tap",
                );
            }
            for b in &self.articulations[..i] {
                let (x, y) = (alt, b.alternatives);
                let overlap = |l1: u8, h1: u8, l2: u8, h2: u8| l1 <= h2 && l2 <= h1;
                let clash = match switching.driver {
                    Driver::Keys => false,
                    Driver::Velocity => matches!((x.velocities, y.velocities),
                        (Some(p), Some(q)) if overlap(p.low, p.high, q.low, q.high)),
                    Driver::Channel => x.channel == y.channel,
                    Driver::Controller => matches!((x.controller, y.controller),
                        (Some(p), Some(q)) if p.controller == q.controller
                            && overlap(p.low, p.high, q.low, q.high)),
                    Driver::Program => x.program == y.program,
                };
                if clash {
                    return fail(&check.owner, "driver value overlaps another articulation");
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    fn one_zone() -> Instrument {
        Instrument {
            assets: vec![Asset {
                location: AssetLocation::Path("a.wav".into()),
                encoding: Encoding::Wav,
                root_key: None,
                loops: Vec::new(),
            }],
            zones: vec![Zone::new(AssetRef(0))],
            ..Instrument::default()
        }
    }

    #[test]
    fn retaining_zones_drops_and_renumbers_unused_assets() {
        let mut ir = one_zone();
        ir.assets
            .extend([ir.assets[0].clone(), ir.assets[0].clone()]);
        ir.zones = (0..3).map(|i| Zone::new(AssetRef(i))).collect();
        ir.zones[2].keys = KeyRange { low: 10, high: 10 };
        assert_eq!(
            ir.retain_zones(|z| z.keys.high != 127 || z.asset.0 == 1),
            [1, 2]
        );
        assert_eq!(
            ir.zones.iter().map(|z| z.asset.0).collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!((ir.assets.len(), ir.validate()), (2, Ok(())));
    }

    #[test]
    fn accepts_a_minimal_instrument() {
        assert_eq!(one_zone().validate(), Ok(()));
    }

    #[test]
    fn names_the_dangling_reference_and_its_owner() {
        let mut ir = one_zone();
        ir.zones[0].asset = AssetRef(3);
        let error = ir.validate().unwrap_err();
        assert_eq!(
            error,
            ValidationError::Dangling {
                owner: "zone 0".into(),
                reference: Reference::Asset(3)
            }
        );
        assert_eq!(error.to_string(), "zone 0 refers to missing Asset(3)");
    }

    #[test]
    fn rejects_inverted_ranges_and_out_of_sequence_takes() {
        let mut ir = one_zone();
        ir.zones[0].keys = KeyRange { low: 70, high: 60 };
        assert!(matches!(
            ir.validate(),
            Err(ValidationError::InvertedRange { field: "keys", .. })
        ));
        let mut ir = one_zone();
        ir.sequences.push(Sequence {
            policy: SequencePolicy::RoundRobin,
            takes: 2,
            counter: CounterScope::Key,
        });
        ir.zones[0].selection = Some(Selection {
            sequence: SequenceRef(0),
            take: Take::Index(2),
        });
        assert_eq!(
            ir.validate(),
            Err(ValidationError::TakeOutOfSequence {
                zone: 0,
                take: 2,
                takes: 2
            })
        );
    }

    #[test]
    fn units_convert_from_source_values() {
        assert!((Gain::Decibels(-6.0).linear() - 0.501).abs() < 1e-3);
        assert_eq!(Pitch::Cents(-250.0).semitones(), -2.5);
        assert!((Pitch::Ratio(2.0).semitones() - 12.0).abs() < 1e-12);
        assert_eq!(Time::Milliseconds(5.0).seconds(), 0.005);
    }

    #[test]
    fn alternatives_follow_key_order_and_switching_is_checked() {
        let mut ir = one_zone();
        ir.articulations = [36u8, 24, 30]
            .map(|key| Articulation {
                name: format!("{key}"),
                switch_keys: vec![key],
                ..Default::default()
            })
            .to_vec();
        ir.assign_alternatives(32);
        let alt = |i: usize| ir.articulations[i].alternatives;
        assert_eq!(
            (alt(1).program, alt(2).program, alt(0).program),
            (Some(0), Some(1), Some(2))
        );
        assert_eq!(alt(2).channel, Some(1));
        assert_eq!(
            alt(0).controller,
            Some(ControllerRange {
                controller: 32,
                low: 2,
                high: 2
            })
        );
        let v = [1, 2, 0].map(|i| alt(i).velocities.unwrap());
        assert_eq!((v[0].low, v[0].high, v[1].low, v[2].high), (1, 42, 43, 127));
        for driver in [
            Driver::Keys,
            Driver::Velocity,
            Driver::Channel,
            Driver::Controller,
            Driver::Program,
        ] {
            ir.switching.driver = driver;
            assert_eq!(ir.validate(), Ok(()));
        }
        ir.articulations[0].alternatives.program = Some(0);
        assert!(matches!(
            ir.validate(),
            Err(ValidationError::Switching { .. })
        ));
        ir.assign_alternatives(32);
        ir.switching.owner = SwitchOwner::Behavior;
        ir.switching.keys = SwitchKeys::Play;
        assert!(matches!(
            ir.validate(),
            Err(ValidationError::Switching { .. })
        ));
        ir.switching.keys = SwitchKeys::Swallow;
        assert_eq!(ir.validate(), Ok(()));
        ir.zones[0].articulation = Some(ArticulationRef(0));
        assert!(matches!(
            ir.validate(),
            Err(ValidationError::Switching { .. })
        ));
    }
}
