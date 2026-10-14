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
    Zone(usize),
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
            Reference::Zone(i) => i < ir.zones.len(),
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

    fn color(&self, c: &crate::fx::color::Config) -> Result<(), ValidationError> {
        use crate::fx::color::{Curve, Hold, Quantizer};
        let unit = |v: f32, field| self.within(f64::from(v), 0.0..=1.0, field);
        if let Some(g) = c.gainer {
            self.gain(g.gain, "gainer gain")?;
            self.finite(g.dry, "gainer dry")?;
        }
        match c.hold {
            Some(Hold::Native { frequency }) => unit(frequency, "color hold frequency")?,
            Some(Hold::Rate { hertz }) => self.within(hertz, 1.0..=f64::MAX, "color hold rate")?,
            None => {}
        }
        if let Some(d) = c.decimator {
            for hz in d.hertz {
                self.within(hz, 1.0..=f64::MAX, "decimator hertz")?;
            }
            self.within(f64::from(d.blend), 0.0..=1.0, "decimator blend")?;
        }
        if let Some(b) = c.filter {
            self.within(f64::from(b.mode), 0.0..=2.0, "color filter mode")?;
            self.within(b.hertz, 1.0..=f64::MAX, "color filter hertz")?;
            self.within(b.q, 1e-3..=f64::MAX, "color filter q")?;
        }
        match c.quantizer {
            Some(Quantizer::Native { bits }) => unit(bits, "color bits")?,
            Some(Quantizer::Floor { bits }) => self.within(bits, 1.0..=32.0, "color bits")?,
            None => {}
        }
        match c.curve {
            Curve::Identity | Curve::Rectifier(_) => {}
            Curve::Classic { shape } | Curve::Enhanced { shape } | Curve::Drums { shape } => self.within(f64::from(shape), -1.0..=1.0, "saturation shape")?,
            Curve::Tube { drive } | Curve::Transistor { drive } => unit(drive, "distortion drive")?,
            Curve::Destructor { drive } => self.within(drive, 0.0..=1.0, "bit crusher drive")?,
            Curve::Drive(d) => {
                for (v, range, field) in [
                    (d.input_gain, -30.0..=20.0, "drive input gain"),
                    (d.overdrive, 0.0..=10.0, "overdrive"),
                    (d.trash, 0.0..=1.0, "drive trash"),
                    (d.mix, 0.0..=100.0, "drive mix"),
                    (d.output_gain, -40.0..=10.0, "drive output gain"),
                ] {
                    self.within(f64::from(v), range, field)?;
                }
            }
            // Translators clamp to the native ranges; the runtime checks finiteness.
            Curve::Diode(_) => {}
            // Translators clamp to the native ranges; the runtime checks finiteness.
            Curve::ToneStack(_) => {}
            // Translators clamp to the native ranges; the runtime checks finiteness.
            Curve::Exciter(_) => {}
            // Translators clamp to the native ranges; the runtime checks finiteness.
            Curve::Tape(_) => {}
            // Translators clamp to the native ranges; the runtime checks finiteness.
            Curve::Bite(_) => {}
            // Translators clamp to the native ranges; the runtime checks finiteness.
            Curve::Dirt(_) | Curve::Freak(_) | Curve::Amp(_) => {}
            // Translators clamp to the native ranges; the runtime checks finiteness.
            Curve::Tubescreamer(_) => {}
            Curve::Skreamer(knobs) => {
                for knob in knobs {
                    unit(knob, "skreamer control")?;
                }
            }
        }
        if let Some(r) = c.redux {
            self.within(f64::from(r.frequency), 1.0..=f64::MAX, "redux frequency")?;
            self.within(f64::from(r.bits), 1.0..=32.0, "redux bits")?;
            self.within(f64::from(r.jitter), 0.0..=1.0, "redux jitter")?;
            self.within(f64::from(r.dither), 0.0..=2.0, "redux dither")?;
            self.within(f64::from(r.band_limit), 1.0..=f64::MAX, "redux band limit")?;
        }
        if let Some(w) = c.widener {
            self.within(f64::from(w.delay), -15.0..=15.0, "widener delay")?;
            self.within(f64::from(w.width), -1.0..=1.0, "widener width")?;
            self.within(f64::from(w.shelf_hertz), 1.0..=f64::MAX, "widener shelf hertz")?;
            self.within(f64::from(w.shelf_db), -15.0..=15.0, "widener shelf gain")?;
        }
        if let Some(tone) = c.tone {
            self.within(f64::from(tone), -1.0..=1.0, "bit crusher cutoff")?;
        }
        if let Some(damping) = c.damping {
            unit(damping, "distortion damping")?;
        }
        if let Some(n) = c.noise {
            unit(n.level, "lofi noise")?;
            unit(n.color, "lofi color")?;
        }
        self.within(c.mix, 0.0..=1.0, "color mix")?;
        if let Some(s) = c.stereo {
            self.within(s.width, 0.0..=1.0, "stereo width")?;
            self.within(s.pan, -1.0..=1.0, "stereo pan")?;
        }
        Ok(())
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
                Processor::Color(c) => self.color(&c)?,
                Processor::Pan(pan) => self.pan(pan, "pan")?,
                Processor::Eq(eq) => {
                    self.finite(f64::from(eq.output_gain_db), "EQ output gain")?;
                    self.within(f64::from(eq.count), 1.0..=crate::fx::eq::MAX_BANDS as f64, "EQ band count")?;
                    for band in eq.bands() {
                        self.finite(f64::from(band.gain_db), "EQ gain")?;
                        self.finite(f64::from(band.proportional_q), "EQ proportional Q")?;
                        match band.law {
                            crate::fx::eq::Law::Rbj | crate::fx::eq::Law::Cookbook | crate::fx::eq::Law::UviDigital => {
                                self.within(f64::from(band.frequency), 1e-3..=1e6, "EQ frequency")?;
                                self.within(f64::from(band.q), 1e-3..=1e3, "EQ Q")?;
                            }
                            crate::fx::eq::Law::KontaktLegacy => {
                                self.within(f64::from(band.frequency), 0.0..=1.0, "legacy EQ frequency")?;
                                self.within(f64::from(band.q), 0.0..=1.0, "legacy EQ bandwidth")?;
                                self.within(f64::from(band.gain_db), -18.0..=18.0, "legacy EQ gain")?;
                            }
                            crate::fx::eq::Law::Solid(_) => {
                                self.within(f64::from(band.frequency), 0.0..=1.0, "Solid EQ frequency")?;
                                self.within(f64::from(band.q), 0.0..=1.0, "Solid EQ Q")?;
                                self.within(f64::from(band.gain_db), 0.0..=1.0, "Solid EQ gain")?;
                            }
                        }
                    }
                }
                Processor::LiveFormant { knobs, .. } => {
                    for knob in knobs { self.within(f64::from(knob), 0.0..=1.0, "formant control")?; }
                }
                Processor::SendReturnGate { .. } => {}
                Processor::Branch { gain, .. } => self.gain(gain, "branch gain")?,
                Processor::Daft(d) => {
                    self.within(d.gain, 0.0..=1.0, "daft gain")?;
                    self.within(d.cutoff, 0.0..=1.0, "daft cutoff")?;
                    self.within(d.resonance, 0.0..=1.0, "daft resonance")?;
                }
                Processor::Formant(knobs) => {
                    for knob in knobs { self.within(f64::from(knob), 0.0..=1.0, "formant control")?; }
                }
                Processor::FilterBank(c) => {
                    for (v, field) in [(c.cutoff, "filter cutoff"), (c.resonance, "filter resonance"), (c.extra, "filter extra")] {
                        self.within(v, 0.0..=1.0, field)?;
                    }
                    if let crate::fx::filter::Variant::Versatile(v) = c.variant {
                        let knobs = v.shift.iter().chain(&v.resonance).chain(&v.types).chain(&v.wet);
                        for &knob in knobs.chain([&v.gain]) {
                            self.within(f64::from(knob), 0.0..=1.0, "versatile control")?;
                        }
                    }
                }
                Processor::TimeMod(config) => {
                    config.check().map_err(|field| ValidationError::OutOfRange {
                        owner: self.owner.clone(),
                        field,
                        value: f64::NAN,
                    })?;
                }
                Processor::Phaser(knobs) => {
                    for knob in knobs { self.within(f64::from(knob), 0.0..=1.0, "phaser control")?; }
                }
                Processor::Ar(d) => {
                    self.within(f64::from(d.mode), 0.0..=8.0, "ar mode")?;
                    self.within(d.cutoff, 0.0..=1.0, "ar cutoff")?;
                    self.within(d.resonance, 0.0..=1.0, "ar resonance")?;
                }
                Processor::LadderLP4(d) => {
                    self.within(d.gain, -1.0..=1.0, "ladder gain")?;
                    self.within(d.cutoff, 0.0..=1.0, "ladder cutoff")?;
                    self.within(d.resonance, 0.0..=1.0, "ladder resonance")?;
                    self.within(f64::from(d.taps), 1.0..=12.0, "ladder taps")?;
                }
                Processor::Space(c) => {
                    self.finite(c.dry, "space dry")?;
                    self.finite(c.wet, "space wet")?;
                    if !c.finite() {
                        self.finite(f64::NAN, "space parameter")?;
                    }
                    if let Some(impulse) = c.impulse {
                        self.exists(Reference::Impulse(impulse.0))?;
                    }
                }
                Processor::Dynamics(c) => {
                    use crate::fx::dynamics::{Detector, Law};
                    self.finite(c.threshold(), "dynamics threshold")?;
                    self.time(c.attack, "dynamics attack")?;
                    self.time(c.release, "dynamics release")?;
                    self.gain(c.makeup, "dynamics makeup")?;
                    self.within(c.mix, 0.0..=1.0, "dynamics mix")?;
                    match c.law {
                        Law::Generic(g) => {
                            self.within(g.ratio, 1.0..=f64::MAX, "dynamics ratio")?;
                            self.within(g.knee_db, 0.0..=f64::MAX, "dynamics knee")?;
                            self.time(g.lookahead, "dynamics lookahead")?;
                            self.within(g.sidechain_highpass_hz, 0.0..=f64::MAX, "dynamics side-chain")?;
                            self.within(g.range_db, 0.0..=f64::MAX, "dynamics range")?;
                            if let Detector::Rms { window } = g.detector {
                                self.time(window, "dynamics rms window")?;
                            }
                        }
                        Law::KontaktClassic(n) | Law::KontaktEnhanced(n) | Law::KontaktPro(n) => {
                            self.within(n.ratio, 0.0..=1.0, "dynamics ratio")?;
                        }
                        Law::KontaktLimiter(_) => {}
                        Law::KontaktFeedback { native: n, .. } => {
                            self.within(n.threshold_db, 0.0..=1000.0, "dynamics drive")?;
                            self.within(n.ratio, 1.0..=20.0, "dynamics ratio")?;
                        }
                        Law::KontaktTransient { attack, sustain, gain, .. } => {
                            for v in [attack, sustain, gain] {
                                self.within(v, 0.0..=1.0, "transient control")?;
                            }
                        }
                        Law::KontaktBusComp { threshold, makeup, attack, release, ratio, .. } => {
                            for v in [threshold, makeup, attack, release, ratio] {
                                self.within(v, 0.0..=1.0, "bus comp control")?;
                            }
                        }
                        Law::UviMaximizer(m) => {
                            self.within(m.threshold_db, -40.0..=0.0, "maximizer threshold")?;
                            self.within(m.attack_ms, 0.0..=20.0, "maximizer attack")?;
                            self.within(m.lookahead_ms, 1.0..=20.0, "maximizer lookahead")?;
                            self.within(m.release_ms, 0.1..=1000.0, "maximizer release")?;
                            self.within(m.ceiling_db, -20.0..=0.0, "maximizer ceiling")?;
                            self.within(m.knee_db, 0.0..=10.0, "maximizer knee")?;
                            self.within(m.release_blend, 0.0..=1.0, "maximizer release blend")?;
                            self.within(m.slew_ms, 0.0..=200.0, "maximizer slew")?;
                            self.within(f64::from(m.mode), 0.0..=2.0, "maximizer mode")?;
                        }
                        Law::KontaktHilbert(n) => {
                            self.within(n.threshold_db, 0.0..=1.0, "dynamics drive")?;
                            self.within(n.ratio, 0.0..=1.0, "dynamics ceiling")?;
                        }
                    }
                }
                Processor::Mix { dry, wet, .. } => {
                    self.within(dry, 0.0..=16.0, "mix dry")?;
                    self.within(wet, 0.0..=16.0, "mix wet")?;
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
        if let Some(bus) = self.input_bus {
            check.output(Output::Bus(bus))?;
        }
        for (source, zone) in self.source_indices.zones.iter().enumerate() {
            check.owner = format!("source zone {source}");
            if let Some(zone) = zone {
                check.exists(Reference::Zone(zone.0))?;
            }
        }
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
            if let Some(stretch) = group.time_stretch {
                check.within(f64::from(stretch.speed), 0.0..=64.0, "time stretch speed")?;
            }
            if let Some(limit) = group.source_voice_limit {
                check.exists(Reference::VoiceLimit(limit))?;
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
            let ranges = match zone.playback.looping {
                Looping::Continuous(range) | Looping::UntilRelease(range) => vec![range],
                Looping::Slots(slots) => {
                    for slot in slots.iter().flatten() {
                        check.within(slot.tuning, f64::MIN_POSITIVE..=f64::MAX, "loop tuning")?;
                    }
                    slots.iter().flatten().map(|slot| slot.range).collect()
                }
                _ => Vec::new(),
            };
            if let Some(crate::Span::Time(time)) = zone.playback.loop_control_crossfade {
                check.time(time, "loop control crossfade")?;
            }
            for range in ranges {
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
                    if let crate::LfoShape::DigitalSine { level, .. }
                    | crate::LfoShape::SineScaled(level) = lfo.shape
                    {
                        check.within(level, -1.0..=1.0, "sine level")?;
                    }
                    if let crate::LfoShape::NativeFaded { fade_ms, .. } = lfo.shape {
                        check.within(f64::from(fade_ms), 0.0..=5000.0, "native fade")?;
                    }
                    if let crate::LfoShape::Native(crate::NativeWave::Multi { weights, width, .. })
                    | crate::LfoShape::NativeFaded { wave: crate::NativeWave::Multi { weights, width, .. }, .. } =
                        lfo.shape
                    {
                        for weight in weights {
                            check.within(f64::from(weight), f64::MIN..=f64::MAX, "multi weight")?;
                        }
                        check.within(f64::from(width), 0.0..=1.0, "multi pulse width")?;
                    }
                    if let crate::LfoShape::DigitalSine { fade_ms, .. } = lfo.shape {
                        check.within(f64::from(fade_ms), 0.0..=5000.0, "digital fade")?;
                        if modulator.scope != Scope::Voice
                            || !lfo.retrigger
                            || matches!(lfo.rate, crate::Frequency::Beats(_))
                            || lfo.delay.seconds() != 0.
                            || lfo.fade_in.seconds() != 0.
                        {
                            check.within(f64::NAN, 0.0..=0.0, "digital fade clock")?;
                        }
                    }
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
                Target::SourcePhase(source) | Target::SourceRate(source) => {
                    check.exists(Reference::Modulator(source.0))?;
                    if !matches!(self.modulators[source.0].source, ModulationSource::Lfo(_)) {
                        check.within(f64::NAN, 0.0..=0.0, "phase or rate destination must be an LFO")?;
                    }
                }
                Target::Amplitude
                | Target::Pitch
                | Target::Pan
                | Target::SampleStart
                | Target::LoopStart
                | Target::LoopLength => {}
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
        // Sets, not rescans of earlier entries: these lists reach thousands
        // on large instruments and validation runs on every load.
        let mut routes = std::collections::HashSet::new();
        for (i, &(route, depth, bypass)) in self.source_indices.route_controls.iter().enumerate() {
            check.owner = format!("source route controls {i}");
            check.exists(Reference::Route(route.0))?;
            for control in [depth, bypass] {
                check.exists(Reference::Control(control.0))?;
            }
            if !routes.insert(route) {
                return Err(ValidationError::OutOfRange {
                    owner: check.owner.clone(),
                    field: "duplicate source route controls",
                    value: i as f64,
                });
            }
        }
        let mut processors = std::collections::HashSet::new();
        for (i, binding) in self.processor_controls.iter().enumerate() {
            check.owner = format!("processor control {i}");
            if !processors.insert((binding.chain, binding.index, binding.parameter)) {
                return Err(ValidationError::OutOfRange {
                    owner: check.owner.clone(),
                    field: "duplicate processor control",
                    value: i as f64,
                });
            }
            check.exists(Reference::Control(binding.control.0))?;
            check.exists(Reference::Processor {
                chain: binding.chain.0,
                index: binding.index,
            })?;
            check.time(binding.ramp, "ramp")?;
        }
        let bound: std::collections::HashSet<_> = (self.processor_controls.iter())
            .map(|binding| binding.control)
            .chain(self.source_indices.route_controls.iter().flat_map(|b| [b.1, b.2]))
            .collect();
        for (i, alias) in self.source_indices.control_aliases.iter().enumerate() {
            check.owner = format!("source control alias {i}");
            check.exists(Reference::Control(alias.control.0))?;
            check.within(
                f64::from(alias.address.slot),
                0.0..=f64::from(i32::MAX),
                "native module alias slot",
            )?;
            if !matches!(
                self.controls[alias.control.0].value,
                crate::ControlValue::Continuous { .. }
            ) || !bound.contains(&alias.control)
            {
                return Err(ValidationError::OutOfRange {
                    owner: check.owner.clone(),
                    field: "unbound native control alias",
                    value: i as f64,
                });
            }
        }
        for (i, tap) in self.voice_send_taps.iter().enumerate() {
            check.owner = format!("voice send tap {i}");
            check.exists(Reference::Chain(tap.chain.0))?;
            check.exists(Reference::Bus(tap.bus.0))?;
            let chain = &self.chains[tap.chain.0];
            let (n, len) = match tap.position {
                crate::VoiceSendPosition::BeforeAmplitude(n) => (n, chain.pre_amplitude.len()),
                crate::VoiceSendPosition::AfterAmplitude(n) => (n, chain.post_amplitude.len()),
            };
            check.within(n as f64, 0.0..=len as f64, "tap position")?;
            check.gain(tap.gain, "send gain")?;
            check.time(tap.ramp, "ramp")?;
            for control in [tap.gain_control, tap.bypass_control].into_iter().flatten() {
                check.exists(Reference::Control(control.0))?;
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
            if switching.owner == SwitchOwner::Behavior
                && a.switch_keys.is_empty()
                && a.control.is_none()
            {
                return fail(
                    &check.owner,
                    "behavior-owned articulation has no key or selection control",
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
