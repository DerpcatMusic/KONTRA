//! Structural checks shared by every consumer, so lowering can index freely.
use crate::{
    Chain, Depth, Envelope, Gain, Instrument, Looping, ModulationSource, Output, Pan, Pitch,
    Processor, Scope, SourceZoneHome, Take, Target, Time,
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
    /// A feature the core supports but no slot can feed yet (a key bus).
    Unrouted {
        owner: String,
        what: &'static str,
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
    ZoneRouting { owner: String, reason: &'static str },
    SampleTarget {
        owner: String,
        reason: &'static str,
    },
    SourceBundle {
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
            Self::Unrouted { owner, what } => write!(f, "{owner}: {what} is not routed"),
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
            Self::ZoneRouting { owner, reason } => write!(f, "{owner}: {reason}"),
            Self::SampleTarget { owner, reason } => write!(f, "{owner}: {reason}"),
            Self::SourceBundle { owner, reason } => write!(f, "{owner}: {reason}"),
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
            Curve::Tubescreamer(_) | Curve::Stereorizer(_) => {}
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
                Processor::ExpressionPan(pan) => {self.pan(pan.pan,"pan")?;self.within(pan.offset,-f64::from(f32::MAX)..=f64::from(f32::MAX),"Pan constant offset")?;},
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
                Processor::LivePhaser { knobs, .. } => {
                    for knob in knobs { self.within(f64::from(knob), 0.0..=1.0, "phaser control")?; }
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
                    // Legacy BFilter setters retain the raw cutoff but saturate its
                    // coefficient input. Preserve that base for subsequent modulation;
                    // clipping here would change a later negative modulation delta.
                    use crate::fx::filter::{Response, Topology, Variant};
                    let raw_legacy = c.topology == Topology::Cascade
                        && c.variant == Variant::Default
                        && matches!((c.response, c.poles),
                            (Response::LowPass, 1 | 2 | 4 | 6)
                            | (Response::HighPass, 1 | 2 | 4)
                            | (Response::BandPass, 2 | 4)
                            | (Response::Notch, 4));
                    let cutoff_bounds = if raw_legacy {
                        f64::from(f32::MIN)..=f64::from(f32::MAX)
                    } else { 0.0..=1.0 };
                    self.within(c.cutoff, cutoff_bounds, "filter cutoff")?;
                    for (v, field) in [(c.resonance, "filter resonance"), (c.extra, "filter extra")] {
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
                Processor::StereoTune(config) => {
                    config.check().map_err(|field| ValidationError::OutOfRange {
                        owner: self.owner.clone(),
                        field,
                        value: f64::NAN,
                    })?;
                }
                Processor::Kolor(record) => self.within(f64::from(record.mode), 0.0..=9.0, "kolor mode")?,
                Processor::RingMod(config) => {
                    config.check().map_err(|field| ValidationError::OutOfRange {
                        owner: self.owner.clone(),
                        field,
                        value: f64::NAN,
                    })?;
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
                        Law::KontaktHilbert(n, key) => {
                            self.within(n.threshold_db, 0.0..=1.0, "dynamics drive")?;
                            self.within(n.ratio, 0.0..=1.0, "dynamics ceiling")?;
                            // The core runs a key buffer; no slot routes one yet.
                            if key == crate::fx::dynamics::Key::External {
                                return Err(ValidationError::Unrouted { owner: self.owner.clone(), what: "dynamics side-chain key" });
                            }
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
        // The shared source hierarchy is document identity, not a guess from
        // processing names. Validate canonical direct membership before either
        // the event adapter or audio producer can resolve an owner through it.
        if !self.source_indices.owners.is_empty() {
            use crate::{SourceEventProcessorId as EventId, SourceInputPredicate as Predicate,
                SourceOwnerId as Id, SourceOwnerKind as Kind};
            let owners = &self.source_indices.owners;
            let invalid = |reason| ValidationError::ZoneRouting {
                owner: "source owner hierarchy".into(), reason,
            };
            let mut identities = std::collections::BTreeMap::new();
            for (index, owner) in owners.iter().enumerate() {
                if identities.insert(owner.id, index).is_some() {
                    return Err(invalid("duplicate native source owner identity"));
                }
                let kind_matches = match (owner.id, owner.kind, &self.source) {
                    (Id::UviNode(_), Kind::Instrument | Kind::Layer | Kind::Keygroup
                        | Kind::Player | Kind::Aux, crate::SourceFormat::Uvi) => true,
                    (Id::KontaktInstrument, Kind::Instrument, crate::SourceFormat::Kontakt { .. })
                    | (Id::KontaktGroup(_), Kind::KontaktGroup, crate::SourceFormat::Kontakt { .. })
                    | (Id::KontaktZone(_), Kind::KontaktZone, crate::SourceFormat::Kontakt { .. }) => true,
                    _ => false,
                };
                if !kind_matches {
                    return Err(invalid("native source identity does not match its format and kind"));
                }
            }
            let mut appearances = vec![0usize; owners.len()];
            let mut roots = Vec::new();
            let mut event_ids = std::collections::BTreeSet::new();
            for (index, owner) in owners.iter().enumerate() {
                if let Some(parent) = owner.parent {
                    if !identities.contains_key(&parent) || parent == owner.id {
                        return Err(invalid("dangling or self-parent native source owner"));
                    }
                } else {
                    if owner.kind != Kind::Instrument {
                        return Err(invalid("only an instrument is a source hierarchy root"));
                    }
                    roots.push(index);
                }
                for child in &owner.children {
                    let child = *identities.get(child)
                        .ok_or_else(|| invalid("dangling source child identity"))?;
                    if owners[child].parent != Some(owner.id) {
                        return Err(invalid("source child has a different canonical parent"));
                    }
                    appearances[child] = appearances[child].checked_add(1)
                        .ok_or_else(|| invalid("source child membership overflow"))?;
                    if appearances[child] != 1 {
                        return Err(invalid("duplicate source child membership"));
                    }
                }
                for (refs, count, reason) in [
                    (owner.groups.iter().map(|g| g.0).collect::<Vec<_>>(), self.groups.len(),
                        "dangling or duplicate source owner group binding"),
                    (owner.zones.iter().map(|z| z.0).collect::<Vec<_>>(), self.zones.len(),
                        "dangling or duplicate source owner zone binding"),
                ] {
                    let mut seen = std::collections::BTreeSet::new();
                    for reference in refs {
                        if reference >= count || !seen.insert(reference) {
                            return Err(invalid(reason));
                        }
                    }
                }
                for predicate in &owner.predicates {
                    let valid = match predicate {
                        Predicate::Keys(range) => range.low <= range.high && range.high < 128,
                        Predicate::Velocities(range) => range.low <= range.high && range.high < 128,
                        Predicate::Channel(channel) => *channel < 16,
                        Predicate::Unsupported { field } => !field.is_empty(),
                    };
                    if !valid { return Err(invalid("invalid retained source input predicate")); }
                }
                let mut switches = Vec::with_capacity(owner.event_processors.len() + 2);
                switches.extend(owner.bypass);
                switches.extend(owner.mute);
                for (ordinal, processor) in owner.event_processors.iter().enumerate() {
                    if processor.owner != owner.id || processor.ordinal as usize != ordinal
                        || processor.kind.is_empty() || !event_ids.insert(processor.id) {
                        return Err(invalid("event processor lost original owner, full-list order or identity"));
                    }
                    let address_matches = match (owner.id, processor.id) {
                        (Id::UviNode(_), EventId::UviNode(node)) => !identities.contains_key(&Id::UviNode(node)),
                        (Id::KontaktInstrument | Id::KontaktGroup(_) | Id::KontaktZone(_),
                            EventId::KontaktScriptSlot(_)) => true,
                        _ => false,
                    };
                    if !address_matches {
                        return Err(invalid("event processor address is not in its source namespace"));
                    }
                    switches.push(processor.bypass);
                }
                for switch in switches {
                    if let Some(control) = switch.control {
                        if !matches!(self.controls.get(control.0).map(|c| &c.value),
                            Some(crate::ControlValue::Toggle { .. })) {
                            return Err(invalid("source switch requires an actual typed boolean control"));
                        }
                    }
                }
            }
            if roots.len() != 1 || owners.iter().enumerate().any(|(index, owner)|
                appearances[index] != usize::from(owner.parent.is_some())) {
                return Err(invalid("source hierarchy requires one root and exact direct child membership"));
            }
            let mut reached = vec![false; owners.len()];
            let mut pending = roots;
            let mut visited = 0usize;
            while let Some(index) = pending.pop() {
                if reached[index] { return Err(invalid("cyclic native source hierarchy")); }
                reached[index] = true;
                visited += 1;
                pending.extend(owners[index].children.iter().map(|id| identities[id]));
            }
            if visited != owners.len() {
                return Err(invalid("unreachable or cyclic native source hierarchy"));
            }
        }
        let mut constant_nodes=std::collections::BTreeSet::new();let mut constant_fields=std::collections::BTreeSet::new();
        for property in &self.source_indices.constant_properties{
            let invalid=||ValidationError::ZoneRouting{owner:"Constant property definition".into(),reason:"missing/duplicate actual owner or incompatible Value/Style controls"};
            if !constant_nodes.insert(property.node)||!constant_fields.insert(property.value.0)||!constant_fields.insert(property.style.0)
                ||!self.source_indices.owners.iter().any(|owner|owner.id==property.owner)
                ||!matches!(self.controls.get(property.value.0).map(|c|&c.value),Some(crate::ControlValue::Continuous{min,max,..})if *min==f64::MIN&&*max==f64::MAX)
                ||!matches!(self.controls.get(property.style.0).map(|c|&c.value),Some(crate::ControlValue::Toggle{..})){return Err(invalid());}
        }
        let mut global_owners=std::collections::BTreeSet::new();
        for recipe in &self.source_indices.global_modulation {
            let invalid=|reason|ValidationError::ZoneRouting{owner:"global source modulation".into(),reason};
            let owner=self.source_indices.owners.iter().find(|row|row.id==recipe.owner)
                .ok_or_else(||invalid("global output owner is absent"))?;
            let input=self.source_indices.owners.iter().find(|row|row.id==recipe.input_owner)
                .ok_or_else(||invalid("resolved global MIDI source owner is absent"))?;
            if !global_owners.insert(recipe.owner)
                || !matches!(owner.kind,crate::SourceOwnerKind::Instrument|crate::SourceOwnerKind::Layer)
                || input.kind!=crate::SourceOwnerKind::Instrument
                || !matches!(input.id,crate::SourceOwnerId::UviNode(_))
                || (owner.id!=input.id && owner.parent!=Some(input.id))
                || recipe.routes.is_empty()
                || !self.processing_buses.iter().any(|bus|bus.bus==recipe.outlet&&bus.owner==Some(recipe.owner)) {
                return Err(invalid("global input/output identity or executable bus binding is invalid"));
            }
            let mut sources=std::collections::BTreeSet::new();
            for &source in &recipe.sources {
                if !sources.insert(source.0)
                    || (!matches!(self.modulators.get(source.0).map(|row|&row.source),Some(crate::ModulationSource::Controller(number))if *number<128)
                        &&!matches!(self.modulators.get(source.0).map(|row|&row.source),Some(crate::ModulationSource::Constant|crate::ModulationSource::ConstantProperty{..}|crate::ModulationSource::Script(_)|crate::ModulationSource::ScriptBipolar(_)))) {
                    return Err(invalid("global source roster requires distinct actual default-state controller sources"));
                }
            }
            let mut referenced=std::collections::BTreeSet::new();
            let mut routes=std::collections::BTreeSet::new();
            for &route in &recipe.routes {
                let row=self.routes.get(route.0).ok_or_else(||invalid("global route is absent"))?;
                let field=match row.target{crate::Target::Amplitude=>crate::SourceOwnerOutputField::Gain,crate::Target::Pan=>crate::SourceOwnerOutputField::Pan,_=>return Err(invalid("global route requires an actual output scalar target"))};
                if !routes.insert(route.0)||!self.source_indices.output_controls.iter().any(|home|
                    home.owner==recipe.owner&&home.field==field&&matches!(home.target,crate::SourceOwnerControlTarget::Bus{bus,..}if bus==recipe.outlet)) {
                    return Err(invalid("global route has no unique actual output-field home"));
                }
                referenced.insert(row.source.0);
                if let Some(scale)=row.scale {referenced.insert(scale.source.0);}
            }
            if referenced!=sources {return Err(invalid("global source roster differs from its exact route dependencies"));}
        }
        let mut recipe_members = std::collections::BTreeSet::new();
        let mut recipe_owners = std::collections::BTreeSet::new();
        for row in &self.source_indices.processing_parameters {
            if self.source_indices.processing.iter().filter(|recipe|recipe.owner==row.owner&&recipe.chain==Some(row.chain)).count()!=1 {
                return Err(ValidationError::ZoneRouting {
                    owner: "source processing recipe".into(),
                    reason: "shared processor binding has no unique actual source recipe",
                });
            }
        }
        for recipe in &self.source_indices.processing {
            let invalid = |reason| ValidationError::ZoneRouting { owner: "source processing recipe".into(), reason };
            if !recipe_owners.insert(recipe.owner)
                || !self.source_indices.owners.iter().any(|owner| owner.id == recipe.owner) {
                return Err(invalid("processing recipe has absent or duplicate source owner"));
            }
            let owner = self.source_indices.owners.iter().find(|owner| owner.id == recipe.owner)
                .ok_or_else(|| invalid("processing recipe source owner disappeared"))?;
            match recipe.lifetime {
                crate::SourceProcessingLifetime::UviKeygroupActivation => {
                    if !matches!(recipe.owner, crate::SourceOwnerId::UviNode(_))
                        || owner.kind != crate::SourceOwnerKind::Keygroup
                        || owner.zones != recipe.members {
                        return Err(invalid("UVI processing recipe differs from its actual keygroup members"));
                    }
                }
            }
            if recipe.members.is_empty() || recipe.members.len() > crate::MAX_PROCESSING_MEMBERS {
                return Err(invalid("processing recipe member capacity is invalid"));
            }
            for member in &recipe.members {
                check.exists(Reference::Zone(member.0))?;
                if !recipe_members.insert(member.0) {
                    return Err(invalid("source zone has multiple source processing recipes"));
                }
            }
            let mut roles=std::collections::BTreeSet::new();
            for role in &recipe.parameter_roles {
                check.exists(Reference::Control(role.control.0))?;
                if !roles.insert(role.field){return Err(invalid("duplicate scoped scalar field role"));}
                let chain_id=recipe.chain.ok_or_else(||invalid("scope scalar has no source chain"))?;
                check.exists(Reference::Chain(chain_id.0))?;let chain=&self.chains[chain_id.0];
                let stage=chain.pre_amplitude.iter().chain(&chain.post_amplitude).nth(role.chain_index).ok_or_else(||invalid("scope scalar original coordinate absent"))?;
                let parameter=match(role.field,stage){
                    (crate::SourceOwnerOutputField::Gain,crate::Processor::Gain(_))=>crate::ProcessorParameter::Gain,
                    (crate::SourceOwnerOutputField::Pan,crate::Processor::ExpressionPan(_))=>crate::ProcessorParameter::Pan,_=>return Err(invalid("scope scalar field differs from actual source operand")),
                };
                if self.processor_controls.iter().filter(|b|b.chain==chain_id&&b.index==role.chain_index&&b.parameter==parameter&&b.control==role.control).count()!=1{return Err(invalid("scope scalar control is not bound to its exact source operand"));}
                let domain=&self.controls[role.control.0].value;
                if !match(role.field,domain){
                    (crate::SourceOwnerOutputField::Gain,crate::ControlValue::Continuous{min,max,..})=>*min>=0.&&*max<=f64::from(f32::MAX),
                    (crate::SourceOwnerOutputField::Pan,crate::ControlValue::Continuous{min,max,..})=>*min>=-1.&&*max<=1.,_=>false,
                }{return Err(invalid("scope scalar role domain differs from public field"));}
            }
            let mut processor_parameters=std::collections::BTreeSet::new();
            let mut processor_sources=std::collections::BTreeSet::new();
            for binding in self.source_indices.processing_parameters.iter().filter(|row|row.owner==recipe.owner){
                check.exists(Reference::Chain(binding.chain.0))?;check.exists(Reference::Control(binding.control.0))?;
                if recipe.chain!=Some(binding.chain)||!processor_parameters.insert((binding.index,binding.parameter as u32))||!processor_sources.insert(binding.source_node) {
                    return Err(invalid("shared processor parameter has a duplicate or foreign source coordinate"));
                }
                let chain=&self.chains[binding.chain.0];
                let stage=chain.pre_amplitude.iter().chain(&chain.post_amplitude).nth(binding.index).ok_or_else(||invalid("shared processor original operand absent"))?;
                if binding.parameter!=crate::ProcessorParameter::Cutoff || !matches!(stage,crate::Processor::Filter(crate::Filter{kind:crate::FilterKind::LowPass{poles:1}|crate::FilterKind::HighPass{poles:1},..})) {
                    return Err(invalid("shared processor role differs from its actual frequency operand"));
                }
                if self.processor_controls.iter().filter(|row|row.chain==binding.chain&&row.index==binding.index&&row.parameter==binding.parameter&&row.control==binding.control).count()!=1 {
                    return Err(invalid("shared processor control is not its exact original operand"));
                }
                if !matches!(self.controls[binding.control.0].value,crate::ControlValue::Continuous{min,max,unit:crate::ControlUnit::Hertz,..}if min>0.&&max<=20000.)
                    ||binding.fixed_factors.iter().any(|value|matches!(value,crate::SourceFrequencyFold::MultiplyClamp(value)if !value.is_finite()||*value<=0.)) {
                    return Err(invalid("shared frequency binding has an invalid domain or fixed-source factor"));
                }
                if !binding.fixed_factors.is_empty()&&self.processor_controls.iter().any(|row|row.chain==binding.chain&&row.index==binding.index&&row.parameter==binding.parameter&&row.ramp.seconds()!=0.){
                    return Err(invalid("historical fixed Frequency clamp requires its actual zero-ramp base"));
                }
                if matches!(binding.law,crate::SourceProcessingParameterLaw::LegacyCutoffSemitoneMidpoint)&&!binding.fixed_factors.is_empty(){
                    return Err(invalid("legacy cutoff law cannot claim historical UVI fixed-source folding"));
                }
                if let Some(compiled)=binding.compiled {
                    if recipe.compiled!=Some(compiled.scope){return Err(invalid("shared processor compiled scope differs from actual owner"));}
                    let scope=self.processing_scopes.get(compiled.scope).ok_or_else(||invalid("shared processor compiled scope absent"))?;
                    match compiled.site {
                        crate::ScopeControlSite::SignalNode(node)=>{
                            let node=scope.signal.as_ref().and_then(|signal|signal.nodes.get(node)).ok_or_else(||invalid("shared processor emitted signal operand absent"))?;
                            let parameter=match node {crate::ProcessingSignalNode::OnePole(filter)=>&filter.cutoff_hz,crate::ProcessingSignalNode::UviOnePole(filter)=>&filter.frequency,_=>return Err(invalid("shared processor emitted operand has wrong kind"))};
                            if parameter.control.as_ref()!=Some(&self.controls[binding.control.0].key){return Err(invalid("shared processor emitted operand has a different control"));}
                        },
                        crate::ScopeControlSite::ChainPre(index)=>if scope.chain!=Some(binding.chain)||index!=binding.index{return Err(invalid("shared processor pre-chain coordinate differs"));},
                        crate::ScopeControlSite::ChainPost(index)=>if scope.chain!=Some(binding.chain)||chain.pre_amplitude.len().checked_add(index)!=Some(binding.index){return Err(invalid("shared processor post-chain coordinate differs"));},
                    }
                }
            }
            let mut owned_sources=std::collections::BTreeSet::new();
            for source in &recipe.sources {
                check.exists(Reference::Modulator(source.0))?;
                if !owned_sources.insert(source.0){return Err(invalid("duplicate actual processing source"));}
            }
            for route in &recipe.routes {
                check.exists(Reference::Route(route.0))?;
                let field=match self.routes[route.0].target{
                    crate::Target::Amplitude=>crate::SourceOwnerOutputField::Gain,
                    crate::Target::Pan=>crate::SourceOwnerOutputField::Pan,
                    crate::Target::SourceRate(source)|crate::Target::SourcePhase(source)=>{
                        check.exists(Reference::Modulator(source.0))?;
                        if !matches!(self.modulators[source.0].source,crate::ModulationSource::Lfo(_))
                            ||!(recipe.sources.contains(&source)||recipe.routes.iter().any(|route|self.routes[route.0].source==source||self.routes[route.0].scale.is_some_and(|scale|scale.source==source))){
                            return Err(invalid("scoped internal target has no actual owned LFO consumer"));
                        }continue;
                    },crate::Target::Processor{chain,index,parameter}=>{
                        if self.zones.iter().any(|zone|zone.routes.contains(route)){return Err(invalid("shared processor route escapes into physical source program"));}
                        if !self.source_indices.processing_parameters.iter().any(|row|row.owner==recipe.owner&&row.chain==chain&&row.index==index&&row.parameter==parameter)
                            ||!matches!(self.routes[route.0].depth,crate::Depth::Pitch(_)) {
                            return Err(invalid("scoped processor route has no exact owned parameter/law"));
                        }continue;
                    },_=>return Err(invalid("scoped program contains a non-owned output target")),
                };
                if !roles.contains(&field){return Err(invalid("scoped route has no declared scalar home"));}
            }
            if let Some(gate)=recipe.source_gate {
                check.exists(Reference::Modulator(gate.source.0))?;check.exists(Reference::Route(gate.route.0))?;
                let route=&self.routes[gate.route.0];
                if !recipe.routes.contains(&gate.route)||route.source!=gate.source||route.target!=crate::Target::Amplitude
                    ||route.depth!=crate::Depth::Normalized(1.)||route.invert||route.shape.is_some()||route.scale.is_some()||route.smoothing!=crate::Time::ZERO
                    ||!matches!(self.modulators[gate.source.0].source,crate::ModulationSource::Envelope(_))
                    ||self.modulators[gate.source.0].scope!=crate::Scope::Voice
                    ||self.source_indices.route_controls.iter().any(|row|row.0==gate.route)
                    ||matches!(gate.velocity,crate::VelocityResponse::Power(p)if !p.is_finite()||p<=0.){return Err(invalid("source gate requires exact actual plain amplitude source/route"));}
            }
            if let Some(chain) = recipe.chain { check.exists(Reference::Chain(chain.0))?; }
            if let Some(amplitude) = recipe.amplitude { check.exists(Reference::Modulator(amplitude.0))?; }
            if let Some(bus) = recipe.output { check.exists(Reference::Bus(bus.0))?; }
            if let Some(index) = recipe.compiled {
                let Some(scope) = self.processing_scopes.get(index) else {
                    return Err(invalid("source recipe compiled scope is absent"));
                };
                if scope.members != recipe.members || (scope.chain != recipe.chain && !(scope.chain.is_none() && scope.signal.is_some()))
                    || scope.amplitude != recipe.amplitude || scope.output != recipe.output {
                    return Err(invalid("source recipe differs from actual compiled scope"));
                }
            }
        }
        let mut native_owners=std::collections::BTreeSet::new();
        for profile in &self.source_indices.uvi_native_processing {
            let invalid=|reason|ValidationError::ZoneRouting{owner:"native UVI processing profile".into(),reason};
            let recipe=self.source_indices.processing.iter().find(|recipe|recipe.owner==profile.owner).ok_or_else(||invalid("native profile has no actual recipe"))?;
            if !native_owners.insert(profile.owner)||profile.members.iter().map(|row|row.0).collect::<Vec<_>>()!=recipe.members{return Err(invalid("native profile member identity/order differs from actual recipe"));}
            let mut sites=std::collections::BTreeSet::new();
            for site in &profile.one_poles {
                if !sites.insert((site.chain.0,site.index)){return Err(invalid("duplicate native OnePole source site"));}
                check.exists(Reference::Chain(site.chain.0))?;
                if Some(site.chain)!=recipe.chain || !site.frequency.is_finite()||site.frequency<=0.||site.frequency>f64::from(f32::MAX)||!site.tracking.is_finite()||!(0. ..=1.).contains(&site.tracking)
                    || !matches!(self.chains[site.chain.0].pre_amplitude.get(site.index),Some(crate::Processor::Filter(crate::Filter{kind:crate::FilterKind::LowPass{poles:1}|crate::FilterKind::HighPass{poles:1},..}))){return Err(invalid("native OnePole site has no exact retained processor or valid fields"));}
            }
        }
        let mut source_matrices=std::collections::BTreeSet::new();
        for matrix in &self.source_indices.processing_matrices {
            let invalid=|reason|ValidationError::ZoneRouting{owner:format!("source matrix {}",matrix.node),reason};
            check.exists(Reference::Chain(matrix.chain.0))?;
            if !source_matrices.insert((matrix.owner,matrix.node)) || matrix.matrix.input.channels!=12 || matrix.matrix.output.channels!=12
                || matrix.matrix.cells.len()!=144 || matrix.matrix.cells.iter().any(|value|!value.is_finite() || value.abs()>f64::from(f32::MAX)) {
                return Err(invalid("source matrix identity, geometry or coefficient is invalid"));
            }
            let chain=&self.chains[matrix.chain.0];
            if !matches!(chain.pre_amplitude.get(matrix.index),Some(crate::Processor::StereoMatrix(_)))
                || !self.source_indices.owners.iter().any(|owner|owner.id==matrix.owner) {
                return Err(invalid("source matrix has no actual owner or chain coordinate"));
            }
            let mut definitions=std::collections::BTreeSet::new();
            let mut definition_keys=std::collections::BTreeSet::new();
            for cell in &matrix.control_definitions {
                if cell.input>=12 || cell.output>=12 || !definitions.insert((cell.input,cell.output))
                    || !definition_keys.insert(&cell.control.key) || cell.control.key.is_empty() {
                    return Err(invalid("source matrix control schema is duplicated or outside its layout"));
                }
                let coefficient=matrix.matrix.cells[cell.output*12+cell.input];
                if !matches!(cell.control.value,crate::ControlValue::Continuous{min,max,default,..}
                    if min.is_finite() && max.is_finite() && min<=max
                    && min.abs()<=f64::from(f32::MAX) && max.abs()<=f64::from(f32::MAX)
                    && default==coefficient && (min..=max).contains(&default)) {
                    return Err(invalid("source matrix control schema differs from its exact authored coefficient"));
                }
            }
            let mut raw_cells=std::collections::BTreeSet::new();
            for cell in &matrix.controls {
                check.exists(Reference::Control(cell.control.0))?;
                if cell.input>=12 || cell.output>=12 || !raw_cells.insert((cell.input,cell.output)) {
                    return Err(invalid("raw source matrix control is duplicated or outside its layout"));
                }
                if !matches!(self.controls[cell.control.0].value,crate::ControlValue::Continuous{min,max,..}
                    if min.is_finite() && max.is_finite() && min.abs()<=f64::from(f32::MAX) && max.abs()<=f64::from(f32::MAX)) {
                    return Err(invalid("raw source matrix control requires a bounded real domain"));
                }
            }
            if let Some(binding)=&matrix.compiled {
                let Some(recipe)=self.source_indices.processing.iter().find(|recipe|recipe.owner==matrix.owner && recipe.compiled==Some(binding.scope)) else {
                    return Err(invalid("source matrix capability has no compiled owner"));
                };
                if recipe.chain!=Some(matrix.chain) || !(1..=12).contains(&binding.layout.channels) {return Err(invalid("source matrix capability differs from its owner"));}
                let scope=self.processing_scopes.get(binding.scope).ok_or_else(||invalid("source matrix capability has no scope"))?;
                if let Some(node)=binding.signal_node {
                    let Some(crate::ProcessingSignalNode::Matrix(operation))=scope.signal.as_ref().and_then(|signal|signal.nodes.get(node)) else {
                        return Err(invalid("source matrix capability has no executable matrix"));
                    };
                    if operation.input!=binding.layout || operation.output!=binding.layout {return Err(invalid("source matrix capability layout differs from executable node"));}
                } else if scope.signal.is_some() || binding.layout.channels!=2 || scope.chain!=Some(matrix.chain) {
                    return Err(invalid("source matrix legacy capability has no actual stereo chain"));
                }
                let mut cells=std::collections::BTreeSet::new();
                for cell in &binding.cells {
                    check.exists(Reference::Control(cell.control.0))?;
                    if !matrix.controls.contains(cell) {return Err(invalid("compiled matrix cell differs from its actual source control"));}
                    if let Some(node)=binding.signal_node {
                        if !scope.signal.as_ref().is_some_and(|signal|signal.controls.iter().any(|candidate|
                            candidate.node==node && candidate.input==cell.input && candidate.output==cell.output
                            && candidate.control==self.controls[cell.control.0].key)) {
                            return Err(invalid("compiled matrix cell differs from actual signal control binding"));
                        }
                    }
                    if cell.input>=binding.layout.channels || cell.output>=binding.layout.channels || !cells.insert((cell.input,cell.output)) {
                        return Err(invalid("source matrix capability cell is out of bounds or duplicated"));
                    }
                }
            }
        }
        let mut source_ports=std::collections::BTreeSet::new();
        for port in &self.source_indices.processing_ports {
            let invalid=|reason|ValidationError::ZoneRouting{owner:"source processing port".into(),reason};
            let matrix=&port.conversion;
            if !source_ports.insert(port.owner) || !self.source_indices.owners.iter().any(|owner|owner.id==port.owner)
                || matrix.input.channels==0 || matrix.output.channels!=2
                || matrix.input.channels.checked_mul(matrix.output.channels)!=Some(matrix.cells.len())
                || matrix.cells.iter().any(|value|!value.is_finite() || value.abs()>f64::from(f32::MAX)) {
                return Err(invalid("source output port identity or explicit matrix is invalid"));
            }
        }
        if self.processing_scopes.len() > crate::MAX_PROCESSING_SCOPES {
            return Err(ValidationError::ZoneRouting { owner: "processing scopes".into(),
                reason: "processing scope definition capacity exceeded" });
        }
        let mut processing_members = std::collections::BTreeSet::new();
        for (index, scope) in self.processing_scopes.iter().enumerate() {
            check.owner = format!("processing scope {index}");
            if scope.members.is_empty() || scope.members.len() > crate::MAX_PROCESSING_MEMBERS {
                return Err(ValidationError::ZoneRouting { owner: check.owner.clone(),
                    reason: "processing scope requires 1..=MAX_PROCESSING_MEMBERS authored members" });
            }
            if let Some(crate::ProcessingSignal{source_law:crate::ProcessingSourceLaw::UviC997Layout{members},..})=&scope.signal {
                if members.iter().filter(|input|input.is_some()).count()!=scope.members.len(){return Err(ValidationError::ZoneRouting{owner:check.owner.clone(),reason:"native source ordinal roster differs from actual scope members"});}
            }
            for member in &scope.members {
                check.exists(Reference::Zone(member.0))?;
                if !processing_members.insert(member.0) {
                    return Err(ValidationError::ZoneRouting { owner: check.owner.clone(),
                        reason: "source zone belongs to multiple processing memberships" });
                }
            }
            if let Some(signal) = &scope.signal {
                let output=scope.output.and_then(|bus|self.processing_buses.iter().find(|row|row.bus==bus)).map_or(2,|row|row.signal.input.channels);
                self.validate_processing_signal(signal,output,true,scope.chain,&mut check)?;
            }
            if let Some(chain) = scope.chain { check.exists(Reference::Chain(chain.0))?; }
            if let Some(amplitude) = scope.amplitude { check.exists(Reference::Modulator(amplitude.0))?; }
            if let Some(bus) = scope.output { check.exists(Reference::Bus(bus.0))?; }
        }
        for (source, zone) in self.source_indices.zones.iter().enumerate() {
            check.owner = format!("source zone {source}");
            if let Some(zone) = zone {
                check.exists(Reference::Zone(zone.0))?;
            }
        }
        let invalid_routing = |owner: String, reason| ValidationError::ZoneRouting { owner, reason };
        let catalog = &self.source_indices.zone_catalog;
        if !catalog.is_empty() && catalog.len() != self.source_indices.zones.len() {
            return Err(invalid_routing("source zone catalog".into(), "catalog length differs from physical zone order"));
        }
        let mut identities = std::collections::HashMap::new();
        for (ordinal, row) in catalog.iter().enumerate() {
            check.owner = format!("source zone catalog {ordinal}");
            check.range(row.home.low_key, row.home.high_key, "keys")?;
            check.within(f64::from(row.home.high_key), 0.0..=127.0, "high key")?;
            check.within(f64::from(row.home.group), 0.0..=f64::from(i32::MAX), "physical group")?;
            // A shared identity addresses its first zone, as Kontakt's zone lookup does.
            if let Some(id) = row.id {
                identities.entry(id).or_insert(ordinal);
            }
        }
        if !self.zone_routing.is_empty() && !matches!(self.zone_routing_mode, Some(2 | 3)) {
            return Err(invalid_routing("zone routing".into(), "ordinary zones require mode 2 or 3"));
        }
        let mut routed = std::collections::HashSet::new();
        let mut templates = std::collections::HashSet::new();
        for route in &self.zone_routing {
            check.owner = format!("zone routing {}", route.source_zone);
            let ordinal = *identities.get(&route.source_zone).ok_or_else(|| invalid_routing(check.owner.clone(), "unknown native source identity"))?;
            let home = SourceZoneHome { group: route.group, low_key: route.low_key, high_key: route.high_key };
            if !routed.insert(route.source_zone) || home != catalog[ordinal].home {
                return Err(invalid_routing(check.owner.clone(), "duplicate route or initial home differs from source catalog"));
            }
            let mut groups = std::collections::HashSet::new();
            let mut initial = false;
            for variant in &route.variants {
                if variant.group > i32::MAX as u32 || !groups.insert(variant.group) {
                    return Err(invalid_routing(check.owner.clone(), "invalid or duplicate destination group"));
                }
                if variant.group == route.group {
                    initial = true;
                    if variant.zone != self.source_indices.zones[ordinal] {
                        return Err(invalid_routing(check.owner.clone(), "initial recipe differs from admitted physical primary"));
                    }
                }
                if let Some(zone) = variant.zone {
                    check.exists(Reference::Zone(zone.0))?;
                    if !templates.insert(zone) {
                        return Err(invalid_routing(check.owner.clone(), "recipe belongs to several destinations"));
                    }
                }
            }
            if !initial {
                return Err(invalid_routing(check.owner.clone(), "missing original destination"));
            }
        }
        let mut file_playback_zones=std::collections::HashSet::new();
        for zone in &self.source_indices.file_playback_zones {
            check.owner=format!("file playback zone {}",zone.0);
            check.exists(Reference::Zone(zone.0))?;
            if !file_playback_zones.insert(zone.0) {
                return Err(ValidationError::SampleTarget {owner:check.owner.clone(),reason:"duplicate file playback zone"});
            }
        }
        let mut sample_nodes = std::collections::HashSet::new();
        let mut sample_zones = std::collections::HashSet::new();
        for target in &self.source_indices.sample_targets {
            check.owner = format!("sample target {}", target.node);
            if !sample_nodes.insert(target.node) || target.zones.is_empty() {
                return Err(ValidationError::SampleTarget { owner: check.owner.clone(), reason: "duplicate or empty sample target" });
            }
            for zone in &target.zones {
                check.exists(Reference::Zone(zone.0))?;
                if !sample_zones.insert(zone.0) {
                    return Err(ValidationError::SampleTarget { owner: check.owner.clone(), reason: "zone belongs to several sample targets" });
                }
            }
        }
        let mut bundled_zones = std::collections::HashSet::new();
        for (ordinal, bundle) in self.source_indices.source_bundles.iter().enumerate() {
            check.owner = format!("source bundle {ordinal}");
            let invalid = |reason| ValidationError::SourceBundle {
                owner: check.owner.clone(), reason,
            };
            check.exists(Reference::Zone(bundle.zone.0))?;
            check.exists(Reference::Chain(bundle.boundary.chain.0))?;
            if !bundled_zones.insert(bundle.zone.0) {
                return Err(invalid("several bundles own one zone"));
            }
            for pack in bundle.packs { check.exists(Reference::Asset(pack.0))?; }
            if bundle.packs[0] == bundle.packs[1] || bundle.packs[0] == bundle.packs[2]
                || bundle.packs[1] == bundle.packs[2] {
                return Err(invalid("physical pack references must be distinct"));
            }
            let zone = &self.zones[bundle.zone.0];
            if zone.asset != Some(bundle.packs[0]) || zone.chain != Some(bundle.boundary.chain) {
                return Err(invalid("bundle primary/chain differs from its owning zone"));
            }
            if bundle.geometry.rate == 0 || bundle.geometry.frames == 0 {
                return Err(invalid("empty source geometry"));
            }
            let chain = &self.chains[bundle.boundary.chain.0];
            let section = match bundle.boundary.section {
                crate::SourceBoundarySection::BeforeAmplitude => &chain.pre_amplitude,
                crate::SourceBoundarySection::AfterAmplitude => &chain.post_amplitude,
            };
            let Some(processor) = section.get(bundle.boundary.index) else {
                return Err(invalid("boundary index is outside its amplitude section"));
            };
            let Processor::StereoMatrix(matrix) = processor else {
                return Err(invalid("boundary must identify the authored matrix"));
            };
            let mut cells = [[false; 6]; 2];
            let mut controls = std::collections::HashSet::new();
            for cell in bundle.boundary.cells {
                let (input, output) = (usize::from(cell.input), usize::from(cell.output));
                if input >= 6 || output >= 2 { return Err(invalid("cell coordinates exceed 2x6")); }
                if std::mem::replace(&mut cells[output][input], true) {
                    return Err(invalid("duplicate matrix cell"));
                }
                check.within(cell.coefficient, -1.0..=1.0, "matrix coefficient")?;
                check.within(cell.ramp.seconds(), 0.0..=f64::MAX, "matrix ramp")?;
                if input < 2 && matrix[output][input] != cell.coefficient {
                    return Err(invalid("matrix first-pair coefficients differ from boundary"));
                }
                if let Some(control) = cell.control {
                    check.exists(Reference::Control(control.0))?;
                    if !controls.insert(control.0) { return Err(invalid("cells must have independent control identities")); }
                    let crate::ControlValue::Continuous { min, max, default, .. } = self.controls[control.0].value else {
                        return Err(invalid("matrix cell needs a continuous numeric control"));
                    };
                    if !min.is_finite() || !max.is_finite() || min > max || min < -1.0 || max > 1.0
                        || default != cell.coefficient || default < min || default > max {
                        return Err(invalid("matrix control range/default differs from signed cell"));
                    }
                }
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
            if let Some(crate::Machine::S1200 { filter_mode: Some(mode) }) = group.machine {
                check.within(f64::from(mode), 0.0..=f64::from(crate::S1200_FILTER_MODE_MAX), "S1200 filter mode")?;
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
            if let Some(asset) = zone.asset {
                check.exists(Reference::Asset(asset.0))?;
            } else if !sample_zones.contains(&i) {
                return Err(ValidationError::SampleTarget { owner: check.owner.clone(), reason: "dormant zone has no sample target" });
            }
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
                ModulationSource::ConstantProperty{node,value,style,bipolar}=>{
                    check.exists(Reference::Control(value.0))?;check.exists(Reference::Control(style.0))?;
                    if !self.source_indices.constant_properties.iter().any(|row|row.node==*node&&row.value==*value&&row.style==*style&&row.bipolar==*bipolar){return Err(ValidationError::ZoneRouting{owner:"Constant source".into(),reason:"source differs from actual definition property role"});}
                },
                ModulationSource::Envelope(envelope) => check.envelope(envelope)?,
                ModulationSource::Glide(g) => {
                    if !matches!(g.mode, 1 | 2) || !g.time_ms.is_finite() || g.time_ms < 0.0 {
                        return Err(ValidationError::ZoneRouting { owner: "Glide source".into(), reason: "glide mode must be 1 or 2 and time finite and non-negative" });
                    }
                }
                ModulationSource::Follower(f) => {
                    let finite = [f.attack, f.decay, f.gain, f.adaption].iter().all(|v| v.is_finite());
                    if !finite || f.adaption <= 0.0 {
                        return Err(ValidationError::ZoneRouting { owner: "Follower source".into(), reason: "stored follower fields must be finite and adaption speed positive" });
                    }
                }
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
                Target::EnvelopeStage { source, .. } => {
                    check.exists(Reference::Modulator(source.0))?;
                    if !matches!(self.modulators[source.0].source, ModulationSource::Envelope(_)) {
                        check.within(
                            f64::NAN,
                            0.0..=0.0,
                            "an envelope stage destination must be an AHDSR envelope",
                        )?;
                    }
                }
                Target::Amplitude
                | Target::Pitch
                | Target::Pan
                | Target::SampleStart
                | Target::LoopStart
                | Target::LoopLength
                | Target::Wavetable(_) => {}
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
            .chain(self.source_indices.source_bundles.iter()
                .flat_map(|b| b.boundary.cells.iter().filter_map(|cell| cell.control)))
            .collect();
        let mut native_aliases = std::collections::HashSet::new();
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
            if !native_aliases.insert((
                alias.parameter.trim_start_matches('$'),
                alias.address.group,
                alias.address.slot,
                alias.address.generic,
            )) {
                return Err(ValidationError::OutOfRange {
                    owner: check.owner.clone(),
                    field: "duplicate native control alias",
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
            let homes=chain.pre_amplitude.iter().chain(&chain.post_amplitude)
                .filter(|stage|matches!(stage,crate::Processor::ExpressionPan(_))).count();
            let hidden_home=[&chain.pre_amplitude[..],&chain.post_amplitude[..]].into_iter().any(|section|section.iter().enumerate().any(|(index,stage)|matches!(stage,crate::Processor::ExpressionPan(_)) && section[..index].iter().enumerate().any(|(start,previous)|matches!(previous,crate::Processor::Mix{count,..}|crate::Processor::Branch{count,..} if index-start<=usize::from(*count)))));
            if homes>1 || hidden_home || homes!=0 && (self.zones.iter().any(|zone|zone.chain==Some(crate::ChainRef(i)))
                || chain.scope!=crate::Scope::Voice
                || !self.processing_scopes.iter().any(|scope|scope.chain==Some(crate::ChainRef(i)))
                    && !self.source_indices.processing.iter().any(|recipe|recipe.chain==Some(crate::ChainRef(i)))) {
                return Err(ValidationError::ZoneRouting{owner:check.owner.clone(),reason:"expression Pan requires one actual shared source home"});
            }
        }
        let mut channel_buses=std::collections::BTreeSet::new();
        for row in &self.processing_buses {
            check.owner=format!("processing bus {}",row.bus.0);check.exists(Reference::Bus(row.bus.0))?;
            let invalid=|reason|ValidationError::ZoneRouting{owner:check.owner.clone(),reason};
            if !channel_buses.insert(row.bus) || row.output.channels==0{return Err(invalid("channel program must bind one existing global bus"));}
            let marker_count=row.signal.nodes.iter().filter(|node|matches!(node,crate::ProcessingSignalNode::BusChain)).count();
            let chain=if let Some(chain)=self.buses[row.bus.0].chain {
                check.exists(Reference::Chain(chain.0))?;
                let chain=&self.chains[chain.0];
                if chain.scope!=crate::Scope::Bus(row.bus){return Err(invalid("channel marker must own the same actual bus chain"));}
                Some(chain)
            }else{None};
            let active_chain=chain.is_some_and(|chain|!chain.pre_amplitude.is_empty()||!chain.post_amplitude.is_empty());
            if marker_count!=usize::from(active_chain){return Err(invalid("channel program requires one exact nonempty bus chain marker"));}
            if row.owner.is_some_and(|owner|!self.source_indices.owners.iter().any(|row|row.id==owner)){return Err(invalid("global bus source owner is absent"));}
            self.validate_processing_signal(&row.signal,row.output.channels,false,None,&mut check)?;
        }
        let mut output_homes=std::collections::BTreeSet::new();
        for home in &self.source_indices.output_controls {
            check.owner=format!("source output {:?} {:?}",home.owner,home.field);
            check.exists(Reference::Control(home.control.0))?;
            let invalid=||ValidationError::ZoneRouting{owner:check.owner.clone(),reason:"source output requires exact declared owner operand binding"};
            if !output_homes.insert((home.owner,home.field)){return Err(invalid());}
            let scalar_key=match home.target {
                crate::SourceOwnerControlTarget::Bus{bus,node}=>{
                    let _=self.buses.get(bus.0).ok_or_else(invalid)?;
                    let row=self.processing_buses.iter().find(|row|row.bus==bus&&row.owner==Some(home.owner)).ok_or_else(invalid)?;
                    let chain=self.buses[bus.0].chain.map(|chain|&self.chains[chain.0]);
                    if chain.is_some_and(|chain|!chain.pre_amplitude.is_empty()||!chain.post_amplitude.is_empty())||row.signal.input!=row.output{return Err(invalid());}
                    let mut phase=0;
                    for operation in &row.signal.nodes {
                        let next=match operation{crate::ProcessingSignalNode::Gain(_)=>0,crate::ProcessingSignalNode::Pan(_)=>1,crate::ProcessingSignalNode::Mute(_)=>2,_=>return Err(invalid())};
                        if next<phase{return Err(invalid());}phase=next;
                    }
                    match(home.field,row.signal.nodes.get(node)){
                        (crate::SourceOwnerOutputField::Gain,Some(crate::ProcessingSignalNode::Gain(stage)))=>stage.gain.control.as_ref(),
                        (crate::SourceOwnerOutputField::Pan,Some(crate::ProcessingSignalNode::Pan(stage)))=>stage.position.control.as_ref(),
                        (crate::SourceOwnerOutputField::Mute,Some(crate::ProcessingSignalNode::Mute(stage)))=>stage.control.as_ref(),_=>return Err(invalid()),
                    }
                },
                crate::SourceOwnerControlTarget::ProcessingScope{scope,site}=>{
                    let definition=self.processing_scopes.get(scope).ok_or_else(invalid)?;
                    if !self.source_indices.processing.iter().any(|row|row.owner==home.owner&&row.compiled==Some(scope)){return Err(invalid());}
                    match site {
                        crate::ScopeControlSite::SignalNode(node)=>{
                            let signal=definition.signal.as_ref().ok_or_else(invalid)?;
                            if signal.nodes.iter().enumerate().take(node).any(|(at,p)|matches!(p,crate::ProcessingSignalNode::Mix(m)if node-at<=usize::from(m.count))){return Err(invalid());}
                            match(home.field,signal.nodes.get(node)){
                                (crate::SourceOwnerOutputField::Gain,Some(crate::ProcessingSignalNode::Gain(stage)))=>stage.gain.control.as_ref(),
                                (crate::SourceOwnerOutputField::Pan,Some(crate::ProcessingSignalNode::Pan(stage)))if stage.expression=>stage.position.control.as_ref(),
                                (crate::SourceOwnerOutputField::Pan,Some(crate::ProcessingSignalNode::UviLayout(stage)))=>stage.pan.control.as_ref(),_=>return Err(invalid()),
                            }
                        },
                        crate::ScopeControlSite::ChainPre(node)|crate::ScopeControlSite::ChainPost(node)=>{
                            let chain_id=definition.chain.ok_or_else(invalid)?;let chain=&self.chains[chain_id.0];
                            let (list,index)=if matches!(site,crate::ScopeControlSite::ChainPre(_)){(&chain.pre_amplitude,node)}else{(&chain.post_amplitude,chain.pre_amplitude.len()+node)};
                            if list.iter().enumerate().take(node).any(|(at,p)|matches!(p,crate::Processor::Mix{count,..}|crate::Processor::Branch{count,..}if node-at<=usize::from(*count))){return Err(invalid());}
                            let role=match(home.field,list.get(node)){
                                (crate::SourceOwnerOutputField::Gain,Some(crate::Processor::Gain(_)))=>crate::ProcessorParameter::Gain,
                                (crate::SourceOwnerOutputField::Pan,Some(crate::Processor::ExpressionPan(_)))=>crate::ProcessorParameter::Pan,_=>return Err(invalid()),
                            };
                            let mut bindings=self.processor_controls.iter().filter(|binding|binding.chain==chain_id&&binding.index==index&&binding.parameter==role);
                            let binding=bindings.next().ok_or_else(invalid)?;if bindings.next().is_some()||binding.control!=home.control{return Err(invalid());}
                            Some(&self.controls[binding.control.0].key)
                        },
                    }
                },
            };
            let control=&self.controls[home.control.0];
            // Output roles are narrower than generic signed signal Gain. Check
            // the declared public field domain before behavior/resource binding.
            let valid_domain=match (home.field,&control.value) {
                (crate::SourceOwnerOutputField::Gain,crate::ControlValue::Continuous{min,max,..})=>*min>=0. && *max<=f64::from(f32::MAX),
                (crate::SourceOwnerOutputField::Pan,crate::ControlValue::Continuous{min,max,..})=>*min>=-1. && *max<=1.,
                (crate::SourceOwnerOutputField::Mute,crate::ControlValue::Toggle{..})=>true,
                _=>false,
            };
            if !valid_domain{return Err(invalid());}
            if scalar_key!=Some(&control.key){return Err(invalid());}
        }
        for(index,bus)in self.buses.iter().enumerate(){
            check.owner=format!("bus {index}");
            let output=self.processing_buses.iter().find(|row|row.bus.0==index).map_or(2,|row|row.output.channels);
            for(target,position)in std::iter::once((bus.output,crate::SendPosition::PostChain)).chain(bus.sends.iter().map(|send|(send.to,send.position))){
                let input=match target{crate::Output::Master=>2,crate::Output::Bus(bus)=>self.processing_buses.iter().find(|row|row.bus==bus).map_or(2,|row|row.signal.input.channels)};
                let source=if position==crate::SendPosition::PreChain{self.processing_buses.iter().find(|row|row.bus.0==index).map_or(2,|row|row.signal.input.channels)}else{output};
                if input!=source{return Err(ValidationError::ZoneRouting{owner:check.owner.clone(),reason:"bus edge requires an explicit layout conversion"});}
            }
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

    #[test]
    fn legacy_filter_raw_cutoff_retains_its_native_saturating_domain() {
        use fx::filter::{Config, Response, Topology, Variant};
        for (response, poles) in [(Response::LowPass, 1), (Response::HighPass, 1),
            (Response::BandPass, 2), (Response::LowPass, 2), (Response::HighPass, 2),
            (Response::LowPass, 4), (Response::HighPass, 4), (Response::BandPass, 4),
            (Response::Notch, 4), (Response::LowPass, 6)] {
            let mut config = Config::new(Topology::Cascade, response, poles);
            let check = |config| {
                let mut ir = one_zone();
                ir.chains.push(Chain { scope: Scope::Master,
                    pre_amplitude: vec![Processor::FilterBank(config)], post_amplitude: vec![] });
                ir.validate()
            };
            for cutoff in [-8.5, 8.5, 9.7, 10.0] {
                config.cutoff = cutoff;
                assert_eq!(check(config), Ok(()));
            }
            for cutoff in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
                config.cutoff = cutoff;
                assert!(check(config).is_err());
            }
            config.cutoff = 8.5;
            config.variant = Variant::Pro52;
            assert!(check(config).is_err());
            config.variant = Variant::Default;
            config.cutoff = 0.5;
            config.resonance = 8.5;
            assert!(check(config).is_err());
        }
    }

    #[test]
    fn s1200_filter_mode_is_bounded_by_the_stored_selector() {
        for (filter_mode, valid) in [(None, true), (Some(0), true), (Some(5), true), (Some(6), false)] {
            let mut ir = one_zone();
            ir.groups.push(Group { machine: Some(Machine::S1200 { filter_mode }), ..Default::default() });
            assert_eq!(ir.validate().is_ok(), valid, "{filter_mode:?}");
        }
    }

    fn one_zone() -> Instrument {
        Instrument {
            assets: vec![Asset {
                location: AssetLocation::Path("a.wav".into()),
                residency: Default::default(),
                encoding: Encoding::Wav,
                root_key: None,
                loops: Vec::new(),
            }],
            zones: vec![Zone::new(AssetRef(0))],
            ..Instrument::default()
        }
    }

    #[test]
    fn dormant_sample_targets_preserve_mapping_and_renumber_without_fabricated_assets() {
        let mut ir = one_zone();
        let dormant = Zone { keys: KeyRange { low: 48, high: 72 }, ..Zone::dormant() };
        ir.zones.push(dormant.clone());
        assert!(matches!(ir.validate(), Err(ValidationError::SampleTarget { .. })));
        ir.source_indices.sample_targets = vec![
            SourceSampleTarget { node: 100, zones: vec![ZoneRef(0)] },
            SourceSampleTarget { node: 200, zones: vec![ZoneRef(1)] },
        ];
        assert_eq!(ir.validate(), Ok(()));
        assert_eq!(ir.retain_zones(|z| z.asset.is_none()), Vec::<usize>::new());
        assert!(ir.assets.is_empty());
        assert_eq!(ir.zones, vec![dormant]);
        assert_eq!(ir.source_indices.sample_targets, vec![SourceSampleTarget { node: 200, zones: vec![ZoneRef(0)] }]);
        assert_eq!(ir.validate(), Ok(()));
        ir.source_indices.sample_targets.push(SourceSampleTarget { node: 300, zones: vec![ZoneRef(0)] });
        assert!(matches!(ir.validate(), Err(ValidationError::SampleTarget { .. })));
        ir.source_indices.sample_targets[1].zones = vec![ZoneRef(99)];
        assert!(matches!(ir.validate(), Err(ValidationError::Dangling { .. })));
    }

    #[test]
    fn retaining_zones_drops_and_renumbers_unused_assets() {
        let mut ir = one_zone();
        ir.assets
            .extend([ir.assets[0].clone(), ir.assets[0].clone()]);
        ir.zones = (0..3).map(|i| Zone::new(AssetRef(i))).collect();
        ir.zones[2].keys = KeyRange { low: 10, high: 10 };
        assert_eq!(
            ir.retain_zones(|z| z.keys.high != 127 || z.asset == Some(AssetRef(1))),
            [1, 2]
        );
        assert_eq!(
            ir.zones.iter().map(|z| z.asset.unwrap().0).collect::<Vec<_>>(),
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
        ir.zones[0].asset = Some(AssetRef(3));
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
    #[test]
    fn file_playback_marker_validates_and_tracks_zone_retention() {
        let mut ir=one_zone();let mut second=ir.zones[0].clone();second.keys=KeyRange {low:61,high:61};ir.zones.push(second);
        ir.source_indices.file_playback_zones=vec![ZoneRef(0),ZoneRef(1)];
        assert_eq!(ir.validate(),Ok(()));ir.retain_zones(|z|z.keys.low==61);
        assert_eq!(ir.source_indices.file_playback_zones,vec![ZoneRef(0)]);assert_eq!(ir.validate(),Ok(()));
        ir.source_indices.file_playback_zones.push(ZoneRef(0));assert!(matches!(ir.validate(),Err(ValidationError::SampleTarget {..})));
        ir.source_indices.file_playback_zones=vec![ZoneRef(99)];assert!(matches!(ir.validate(),Err(ValidationError::Dangling {..})));
    }

}

impl Instrument {
    fn validate_processing_signal(&self,signal:&crate::ProcessingSignal,output:usize,
        allow_expression:bool,chain:Option<crate::ChainRef>,check:&mut Check)->Result<(),ValidationError>{
        let invalid = |reason| ValidationError::ZoneRouting { owner: check.owner.clone(), reason };
        match signal.source_law {
            crate::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet=>{},
            crate::ProcessingSourceLaw::UviC997Layout{members}=>{
                let count=members.iter().take_while(|member|member.is_some()).count();
                if !allow_expression || count==0 || members[count..].iter().any(Option::is_some)
                    || members.iter().flatten().any(|member|match member.provenance {
                        crate::UviNativeSourceProvenance::PlainHeader=>member.storage_channels!=member.layout.channels()&&!(member.layout==crate::UviNativeLayout::Code1&&member.storage_channels==2),
                        crate::UviNativeSourceProvenance::UnloadedDefault=>member.layout!=crate::UviNativeLayout::Code1||member.storage_channels!=0,
                    })
                    || members.iter().flatten().map(|member|member.layout.channels()).max()!=Some(signal.input.channels){return Err(invalid("native member/header layout differs from its retained widest input"));}
            }
        }
        if signal.input.channels == 0 || signal.nodes.is_empty() {
            return Err(invalid("signal graph requires a nonempty input and executable nodes"));
        }
        let mut channels = signal.input.channels;
        let mut spans: Vec<(usize, usize)> = Vec::new();
        let mut expression_pan = false;
        for (node_index, node) in signal.nodes.iter().enumerate() {
            if !allow_expression && match node {
                crate::ProcessingSignalNode::Gain(gain)=>gain.gain.expression.is_some(),
                crate::ProcessingSignalNode::OnePole(filter)=>filter.cutoff_hz.expression.is_some(),
                crate::ProcessingSignalNode::UviOnePole(filter)=>filter.frequency.expression.is_some() || filter.tracking!=0.,
                crate::ProcessingSignalNode::Pan(pan)=>pan.expression || pan.position.expression.is_some(),
                _=>false,
            } {return Err(invalid("global processing bus has no per-note expression owner"));}
            while spans.last().is_some_and(|&(end, _)| end == node_index) {
                let (_, width) = spans.pop().ok_or_else(|| invalid("missing channel mix span"))?;
                if channels != width { return Err(invalid("channel mix must preserve its input width")); }
            }
            if let crate::ProcessingSignalNode::Mix(mix) = node {
                let end = node_index.checked_add(1).and_then(|index| index.checked_add(usize::from(mix.count)))
                    .ok_or_else(|| invalid("channel mix span overflow"))?;
                if end > signal.nodes.len() || spans.last().is_some_and(|&(parent,_)| end > parent)
                    || spans.len() == 16 || !mix.dry.is_finite() || !mix.wet.is_finite()
                    || !(0. ..=16.).contains(&mix.dry) || !(0. ..=16.).contains(&mix.wet) {
                    return Err(invalid("channel mix geometry, depth or default is invalid"));
                }
                spans.push((end, channels));
            }
            match node {
                crate::ProcessingSignalNode::BusChain => {
                    if allow_expression || channels!=2 || !spans.is_empty() {return Err(invalid("bus chain requires an unsuppressed stereo global boundary"));}
                }
                crate::ProcessingSignalNode::Matrix(matrix) => {
                    match matrix.arithmetic { crate::MatrixArithmetic::SeparateMulAddF32 => {} }
                    if matrix.input.channels != channels || matrix.output.channels == 0
                        || matrix.input.channels.checked_mul(matrix.output.channels) != Some(matrix.cells.len())
                        || matrix.cells.iter().any(|v| !v.is_finite() || v.abs() > f64::from(f32::MAX)) {
                        return Err(invalid("matrix layout, cell geometry or coefficient domain is invalid"));
                    }
                    channels = matrix.output.channels;
                }
                crate::ProcessingSignalNode::UviLayout(layout)=>{
                    if channels!=layout.source.channels() || layout.source.weight(layout.destination,0,0).is_none()
                        || !layout.pan_offset.is_finite() || layout.pan.expression.is_some(){return Err(invalid("native descriptor edge or Pan operand is invalid"));}
                    channels=layout.destination.channels();
                }
                crate::ProcessingSignalNode::UviOnePole(filter)=>{
                    if !filter.tracking.is_finite() || !(0. ..=1.).contains(&filter.tracking){return Err(invalid("native OnePole tracking is invalid"));}
                }
                crate::ProcessingSignalNode::Mute(mute) => {
                    if let Some(key)=&mute.control {
                        let control=self.controls.iter().find(|control|&control.key==key)
                            .ok_or_else(||invalid("outlet Mute Toggle control is missing"))?;
                        if !matches!(control.value,crate::ControlValue::Toggle{..}) {
                            return Err(invalid("outlet Mute requires a canonical Toggle control"));
                        }
                    }
                }
                crate::ProcessingSignalNode::Mix(_) | crate::ProcessingSignalNode::Rectifier(_) => {},
                crate::ProcessingSignalNode::Pan(pan) => {
                    if channels!=2 || !pan.offset.is_finite() || pan.offset.abs()>f64::from(f32::MAX) || pan.position.expression.is_some() {return Err(invalid("Pan requires a stereo boundary and one scalar position"));}
                    if pan.expression {
                        if expression_pan || !spans.is_empty() || chain.is_some_and(|chain|self.chains.get(chain.0).is_some_and(|chain|chain.pre_amplitude.iter().chain(&chain.post_amplitude).any(|stage|matches!(stage,crate::Processor::ExpressionPan(_))))) {
                            return Err(invalid("expression Pan requires one unsuppressed shared home"));
                        }
                        expression_pan=true;
                    }
                }
                crate::ProcessingSignalNode::Gain(gain) => {
                    match gain.arithmetic { crate::ChannelGainArithmetic::CoreGainF64ThenF32 => {} }
                }
                crate::ProcessingSignalNode::OnePole(filter) => {
                    match filter.arithmetic { crate::ChannelOnePoleArithmetic::CoreOnePoleF64 => {} }
                }
            }
        }
        while let Some((end, width)) = spans.pop() {
            if end != signal.nodes.len() || width != channels { return Err(invalid("channel mix must preserve its input width")); }
        }
        if channels != output { return Err(invalid("signal graph output differs from its declared destination layout")); }
        let mut tail = 0u32;
        for node in &signal.nodes {
            let parameter = match node {
                crate::ProcessingSignalNode::Matrix(_) | crate::ProcessingSignalNode::Mix(_) | crate::ProcessingSignalNode::Rectifier(_) | crate::ProcessingSignalNode::BusChain | crate::ProcessingSignalNode::Mute(_) => continue,
                crate::ProcessingSignalNode::Gain(gain) => &gain.gain,
                crate::ProcessingSignalNode::Pan(pan) => &pan.position,
                crate::ProcessingSignalNode::UviLayout(layout)=>&layout.pan,
                crate::ProcessingSignalNode::UviOnePole(filter)=>{tail=tail.checked_add(filter.tail_frames).ok_or_else(||invalid("native OnePole tail budget overflow"))?;&filter.frequency},
                crate::ProcessingSignalNode::OnePole(filter) => {
                    tail = tail.checked_add(filter.tail_frames)
                        .ok_or_else(|| invalid("signal stage tail budget overflow"))?;
                    &filter.cutoff_hz
                }
            };
            if !parameter.constant.is_finite() || parameter.constant.abs() > f64::from(f32::MAX) {
                return Err(invalid("signal stage constant is outside its finite domain"));
            }
            let bounds = if let Some(key) = &parameter.control {
                let control = self.controls.iter().find(|c| &c.key == key)
                    .ok_or_else(|| invalid("signal stage control is missing"))?;
                match &control.value {
                    crate::ControlValue::Continuous { min, max, .. }
                        if min.is_finite() && max.is_finite() && min.abs() <= f64::from(f32::MAX)
                            && max.abs() <= f64::from(f32::MAX) => [*min, *max],
                    _ => return Err(invalid("signal stages require a bounded real control")),
                }
            } else { [parameter.constant; 2] };
            if let Some(expression) = parameter.expression {
                let (zero, one, octaves) = match (node, expression.law) {
                    (crate::ProcessingSignalNode::Gain(_), crate::SignalExpressionLaw::LinearFactor { zero, one })
                        if zero >= 0. && one >= 0. => (zero, one, false),
                    (crate::ProcessingSignalNode::OnePole(_)|crate::ProcessingSignalNode::UviOnePole(_), crate::SignalExpressionLaw::Octaves { zero, one }) => (zero, one, true),
                    _ => return Err(invalid("expression law does not match its stage destination")),
                };
                if !zero.is_finite() || !one.is_finite() || !(one - zero).is_finite() {
                    return Err(invalid("expression endpoint domain is not finite"));
                }
                for endpoint in [zero, one] {
                    let factor = if octaves { endpoint.exp2() } else { endpoint };
                    if !factor.is_finite() || (octaves && factor <= 0.) {
                        return Err(invalid("expression factor is outside the finite destination domain"));
                    }
                    for base in [parameter.constant, bounds[0], bounds[1]] {
                        let value = base * factor;
                        if !value.is_finite() || value.abs() > f64::from(f32::MAX) || (octaves && value <= 0.) {
                            return Err(invalid("base and expression endpoint product is outside its destination domain"));
                        }
                    }
                }
            }
            if matches!(node,crate::ProcessingSignalNode::Pan(_)|crate::ProcessingSignalNode::UviLayout(_)) && bounds.iter().any(|v|!(-1. ..=1.).contains(v)) {
                return Err(invalid("Pan scalar domain is outside minus-one to one"));
            }
            if matches!(node, crate::ProcessingSignalNode::OnePole(_)|crate::ProcessingSignalNode::UviOnePole(_)) && bounds.iter().any(|v| *v <= 0.) {
                return Err(invalid("OnePole cutoff must be positive"));
            }
        }
        let mut cells = std::collections::BTreeSet::new();
        for binding in &signal.controls {
            let Some(crate::ProcessingSignalNode::Matrix(matrix)) = signal.nodes.get(binding.node) else {
                return Err(invalid("matrix binding has no executable node"));
            };
            if binding.input >= matrix.input.channels || binding.output >= matrix.output.channels
                || !cells.insert((binding.node, binding.input, binding.output)) {
                return Err(invalid("matrix binding cell is outside the layout or duplicated"));
            }
            let Some(control) = self.controls.iter().find(|c| c.key == binding.control) else {
                return Err(invalid("matrix binding control is missing"));
            };
            match &control.value {
                crate::ControlValue::Continuous { min, max, .. }
                    if min.is_finite() && max.is_finite() && min.abs() <= f64::from(f32::MAX)
                        && max.abs() <= f64::from(f32::MAX) => {}
                _ => return Err(invalid("matrix cells require a bounded real coefficient domain")),
            }
        }
        Ok(())
    }
}
