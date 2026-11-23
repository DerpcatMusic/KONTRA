//! The program's mixer as buses. Bus 0 is the main output; each layer and
//! each aux is a strip of buses: its voices or sends enter the first, its
//! inserts run in order, and its fader and pan sit on the last, which feeds
//! the auxes a layer sends to (before or after the fader). An active
//! EffectRack ends a bus: it feeds one bus per chain at the chain's gain,
//! and the chains feed a bus where the rest of the inserts run. Every strip
//! ends on the main output ahead of the program's own inserts, as an aux set
//! `PreInsert` does. Of the effects, Gain, GainMatrix, TrackDelay (at 0),
//! DigitalEq, Convolver, SampledReverb, ThreeBandShelves, DualDelay,
//! Maximizer, SparkVerb, WaveShaper, DiodeClipper, Flanger and Phasor are
//! carried; the rest pass their input and are reported (see `effects`).

use std::f32::consts::FRAC_PI_2;
use std::f64::consts::{FRAC_PI_4, LOG10_2};

use kontra_format::Notes;
use kontra_ir as ir;
use kontra_law::common::{DB_GAIN, SQUARE_ROOT};
use kontra_law::uvi::WAVESHAPER_DRIVE;
use roxmltree::Node;

use super::delay::dual_delay;
use super::digital_eq::{digital_eq, gain_scale};
use super::reverb::spark_verb;
mod inserts;
mod moved;
mod shaper;
mod sweep;

use super::{Library, Translator, db, flag, index_of, matrix, num, source_node};
use shaper::{diode_clipper, wave_shaper};
use sweep::{flanger, phasor};

/// An effect's saved parameters, some set to other values (to tabulate
/// what a connection moving them does: `moved`).
#[derive(Clone, Copy)]
pub(super) struct Params<'a, 'input> {
    pub(super) node: Node<'a, 'input>,
    set: &'a [(&'static str, f64)],
}

impl<'a, 'input> Params<'a, 'input> {
    pub(super) fn of(node: Node<'a, 'input>) -> Self {
        Self { node, set: &[] }
    }

    /// `name`'s value, or `default` when the effect has none.
    pub(super) fn num(&self, name: &str, default: f64) -> Result<f64, String> {
        match self.set.iter().find(|(n, _)| *n == name) {
            Some(&(_, v)) => Ok(v),
            None => num(self.node, name, default),
        }
    }
}

/// A number a script sets on an insert, and each control it moves (its
/// [`ir::EffectParam::code`]) with that control's value by the number.
#[derive(PartialEq)]
pub(super) struct Knob {
    pub name: String,
    pub controls: Vec<(u16, Vec<(f64, f32)>)>,
}

pub(super) type KnobKey = (String, Vec<(String, String)>, usize);
pub(super) type RoutedKnobs = (usize, Vec<(usize, usize)>);

/// Where an insert's effect plays, for scripts that bypass it: a keygroup's
/// voices (its slot), a bus's rack or the program's.
#[derive(Clone, Copy)]
pub(super) enum Fx {
    Voice(roxmltree::NodeId, usize),
    Bus(usize, usize),
    Instrument(usize),
}

const PER_VOICE: [&str; 2] = ["Gain", "ThreeBandShelves"];

/// Whether `node` is an effect this module carries: in the program's
/// inserts, a layer's, an aux's or a rack chain's, or a per-voice one in a
/// keygroup's.
pub(super) fn carried(node: Node) -> bool {
    let Some(owner) = node
        .parent()
        .filter(|p| p.has_tag_name("Inserts"))
        .and_then(|p| p.parent())
    else {
        return false;
    };
    let laid = owner.has_tag_name("Layer") || owner.has_tag_name("AuxEffect");
    let kind = [
        "Gain",
        "GainMatrix",
        "TrackDelay",
        "DigitalEq",
        "Convolver",
        "SampledReverb",
        "ThreeBandShelves",
        "DualDelay",
        "Maximizer",
        "SparkVerb",
        "WaveShaper",
        "DiodeClipper",
        "Flanger",
        "Phasor",
        "Drive",
        "CombFilter",
        "FeedbackMachine",
    ]
    .iter()
    .any(|t| node.has_tag_name(*t))
        || super::filters::on_bus(node);
    if owner.has_tag_name("Keygroup") {
        return PER_VOICE.iter().any(|t| node.has_tag_name(*t));
    }
    let in_keygroup = node.ancestors().any(|a| a.has_tag_name("Keygroup"));
    !in_keygroup
        && ((laid && (kind || node.has_tag_name("EffectRack")))
            || (owner.has_tag_name("Program") && kind))
}

/// A keygroup's TriggerRule as a start rule: every oscillator (0), one per
/// note in turn (1, "Cycle") or at random (2, "Random Cycle"). The names
/// follow the keygroup's trigger parameters in UVIWorkstation's string table
/// ([ref], before TriggerSync's "Immediate"/"Next Beat"/"Next Bar").
pub(super) fn cycle(kg: Node) -> Option<(ir::Join, ir::StartRule)> {
    let random = match kg.attribute("TriggerRule")? {
        "1" => false,
        "2" => true,
        _ => return None,
    };
    Some((ir::Join::And, ir::StartRule::Cycle { random }))
}

/// Latent for `node` on a program aux no open send reaches at load (by the
/// name [`Translator::sends_of`] resolves: AO's Feedback aux, its sends off
/// until a script turns one on), or for a connection from a bypassed
/// modulator (it moves nothing until a script turns it on); else Audible.
pub(super) fn impact(program: Node, node: Node) -> ir::Impact {
    if super::source_node(program, node).is_some_and(|n| flag(n, "Bypass")) {
        return ir::Impact::Latent;
    }
    let aux = node
        .ancestors()
        .filter(|a| a.has_tag_name("AuxEffect"))
        .last();
    let open = |aux: Node| {
        program.descendants().any(|s| {
            s.has_tag_name("BusRouter")
                && !flag(s, "Bypass")
                && s.attribute("Destination")
                    .and_then(|d| d.rsplit('/').next())
                    == aux.attribute("Name")
                && num(s, "Gain", 1.0).is_ok_and(|g| g > 0.0)
        })
    };
    match aux {
        Some(aux) if aux.parent().and_then(|p| p.parent()) == Some(program) && !open(aux) => {
            ir::Impact::Latent
        }
        _ => ir::Impact::Audible,
    }
}

fn children<'a, 'input>(
    owner: Node<'a, 'input>,
    container: &'static str,
    tag: &'static str,
) -> impl Iterator<Item = Node<'a, 'input>> {
    owner
        .children()
        .filter(move |n| n.has_tag_name(container))
        .flat_map(|n| n.children())
        .filter(move |n| n.is_element() && (tag.is_empty() || n.has_tag_name(tag)))
}

impl<L: Library> Translator<'_, '_, L> {
    /// Every bus, the program's inserts, and each layer's entry bus and each
    /// layer's and aux's fader bus. Runs before the layers.
    pub(super) fn buses(&mut self) -> Result<(), String> {
        let program = self.program;
        self.out.buses.push(ir::Bus::new("Main"));
        let layers: Vec<Node> = program
            .descendants()
            .filter(|n| n.has_tag_name("Layer"))
            .collect();
        for &layer in &layers {
            let entry = self.push(layer);
            self.entry.insert(layer.id(), entry);
            let end = if flag(layer, "BypassInsertFX") {
                entry
            } else {
                self.lay(layer, entry)?
            };
            self.strip(layer, end)?;
        }
        let mut entries = Vec::new();
        for aux in children(program, "Auxs", "AuxEffect") {
            let entry = self.push(aux);
            entries.push((aux, entry));
            // A bypassed aux takes no sends.
            if flag(aux, "Bypass") {
                continue;
            }
            if !flag(aux, "PreInsert") {
                self.notes.add(
                    ir::Impact::Audible,
                    "aux after the program's inserts",
                    aux.attribute("DisplayName").unwrap_or_default(),
                );
            }
            let end = self.lay(aux, entry)?;
            self.strip(aux, end)?;
        }
        for &layer in &layers {
            self.sends_of(layer, &entries)?;
        }
        self.out.inserts = Vec::new();
        for (n, node) in children(program, "Inserts", "").enumerate() {
            let slot = self.slot(node)?;
            if slot.is_some() {
                self.fx_of.insert(node.id(), Fx::Instrument(n));
            }
            self.out.inserts.push(slot);
        }
        Ok(())
    }

    /// A keygroup's inserts its voices run, by slot; the rest are empty.
    pub(super) fn voice_slots(&mut self, kg: Node) -> Result<Vec<Option<ir::Effect>>, String> {
        let mut slots = Vec::new();
        for (n, node) in children(kg, "Inserts", "").enumerate() {
            let per_voice = PER_VOICE.iter().any(|t| node.has_tag_name(*t));
            let slot = if per_voice { self.slot(node)? } else { None };
            if slot.is_some() {
                self.fx_of.insert(node.id(), Fx::Voice(kg.id(), n));
            }
            slots.push(slot);
        }
        if slots.iter().all(Option::is_none) {
            slots.clear();
        }
        Ok(slots)
    }

    /// A new bus named for `owner`, off the main output.
    fn push(&mut self, owner: Node) -> usize {
        let name = owner
            .attribute("DisplayName")
            .or_else(|| owner.attribute("Name"))
            .unwrap_or_default();
        self.out.buses.push(bus(name));
        self.out.buses.len() - 1
    }

    /// `owner`'s gain and pan on its last bus `end`, which scripts move.
    fn strip(&mut self, owner: Node, end: usize) -> Result<(), String> {
        let (gain, pan) = (num(owner, "Gain", 1.0)?, num(owner, "Pan", 0.0)?);
        self.bus_of.insert(owner.id(), end);
        if let Some(b) = self.out.buses.get_mut(end) {
            b.volume_db = db(gain);
            b.to_main = true;
            // A layer's pan is on its groups.
            if !owner.has_tag_name("Layer") {
                b.pan = pan.clamp(-1.0, 1.0) as f32;
            }
        }
        Ok(())
    }

    /// `owner`'s inserts from bus `at` on; returns the bus they end on.
    fn lay(&mut self, owner: Node, mut at: usize) -> Result<usize, String> {
        for node in children(owner, "Inserts", "") {
            if node.has_tag_name("DigitalEq") && !flag(node, "Bypass") {
                self.routed_eq(node, at)?;
                continue;
            }
            let racked = node.has_tag_name("EffectRack") || node.has_tag_name("FeedbackMachine");
            if !racked {
                let slot = self.slot(node)?;
                if let Some(b) = self.out.buses.get_mut(at) {
                    let insert = u8::try_from(b.inserts.len()).map_err(|e| e.to_string())?;
                    let present = slot.is_some();
                    b.inserts.push(slot);
                    if present {
                        self.fx_of
                            .insert(node.id(), Fx::Bus(at, usize::from(insert)));
                        self.moves(node, at, insert)?;
                    }
                }
                continue;
            }
            // A FeedbackMachine's `Mix` is its wet share; its input is the rest.
            let wet = if node.has_tag_name("FeedbackMachine") {
                self.feedback_machine(node)?
            } else {
                1.0
            };
            let mut ends = Vec::new();
            // A bypassed chain is built silent: a script may enable it.
            for chain in children(node, "Chains", "AuxEffect") {
                let entry = self.push(chain);
                self.feed(at, entry, 1.0, false);
                let end = self.lay(chain, entry)?;
                self.bus_of.insert(chain.id(), end);
                let gain = if flag(chain, "Bypass") {
                    0.0
                } else {
                    num(chain, "Gain", 1.0)?
                };
                if let Some(b) = self.out.buses.get_mut(end) {
                    b.volume_db = db(gain);
                }
                let pan = num(chain, "Pan", 0.0)?.clamp(-1.0, 1.0) as f32;
                if pan != 0.0
                    && let Some(b) = self.out.buses.get_mut(end)
                {
                    let kind = ir::EffectKind::Stereo { width: 0.5, pan };
                    b.inserts.push(Some(effect(kind, 1.0, 0.0)));
                }
                ends.push(end);
            }
            // The chains' gains are their faders: scripts move them. The
            // chains sum on a wet bus and the input passes on a dry one; the
            // rack's Bypass (saved, or a script's) opens one and shuts the
            // other, so a rack saved bypassed is built and can be switched on.
            let input = at;
            let wet_bus =
                if node.has_tag_name("FeedbackMachine") && self.script_words.contains("Mix") {
                    // Keep both shares present even at the endpoints. These gains
                    // are separate from the wet/dry faders controlled by Bypass.
                    let chains = self.rack_share(node, wet);
                    let direct = self.rack_share(node, 1.0 - wet);
                    let wet_bus = self.push(node);
                    for end in ends {
                        self.feed(end, chains, 1.0, true);
                    }
                    self.feed(input, direct, 1.0, false);
                    self.feed(chains, wet_bus, 1.0, true);
                    self.feed(direct, wet_bus, 1.0, true);
                    self.rack_mix_of.insert(node.id(), (chains, direct));
                    wet_bus
                } else {
                    let wet_bus = self.push(node);
                    for end in ends {
                        self.feed(end, wet_bus, wet, true);
                    }
                    if wet < 1.0 {
                        self.feed(input, wet_bus, 1.0 - wet, false);
                    }
                    wet_bus
                };
            let dry_bus = self.push(node);
            self.feed(input, dry_bus, 1.0, false);
            at = self.push(node);
            self.feed(wet_bus, at, 1.0, true);
            self.feed(dry_bus, at, 1.0, true);
            let bypassed = flag(node, "Bypass");
            for (bus, open) in [(wet_bus, !bypassed), (dry_bus, bypassed)] {
                if let Some(b) = self.out.buses.get_mut(bus) {
                    b.volume_db = if open { 0.0 } else { SILENT };
                }
            }
            self.bus_of.insert(node.id(), wet_bus);
            self.dry_of.insert(node.id(), dry_bus);
        }
        Ok(at)
    }

    /// A DigitalEq on bus `at` (one slot per four bands), its GainScale
    /// connections as the bus's routes.
    fn routed_eq(&mut self, node: Node, at: usize) -> Result<(), String> {
        let bands = digital_eq(node, &mut self.notes)?;
        let program = self.program;
        if let Some(b) = self.out.buses.get_mut(at) {
            let insert = u8::try_from(b.inserts.len()).map_err(|e| e.to_string())?;
            // GainScale routes carry u8 insert addresses.
            u8::try_from(b.inserts.len() + bands.effects.len() - 1).map_err(|e| e.to_string())?;
            b.inserts.extend(bands.effects.iter().cloned().map(Some));
            for c in gain_scale(program, node, &bands, insert, b) {
                self.carried.insert(c.id());
            }
            let slots: Vec<_> = (0..bands.effects.len())
                .filter_map(|chunk| Some((usize::from(insert) + chunk, self.knobs(node, chunk)?)))
                .collect();
            if !slots.is_empty() {
                self.routed_knobs.insert(node.id(), (at, slots));
            }
        }
        Ok(())
    }

    fn rack_share(&mut self, node: Node, gain: f64) -> usize {
        let share = self.push(node);
        if let Some(b) = self.out.buses.get_mut(share) {
            b.inserts
                .push(Some(effect(ir::EffectKind::Gain(gain as f32), 1.0, 0.0)));
        }
        share
    }

    fn feed(&mut self, from: usize, to: usize, level: f64, post_fader: bool) {
        if let Some(b) = self.out.buses.get_mut(from) {
            b.sends.push(ir::BusSend {
                bus: to,
                level: level as f32,
                post_fader,
            });
        }
    }

    /// A layer's active sends to active auxes, from its fader bus.
    fn sends_of(&mut self, layer: Node, auxes: &[(Node, usize)]) -> Result<(), String> {
        let Some(&from) = self.bus_of.get(&layer.id()) else {
            return Ok(());
        };
        for send in children(layer, "BusRouters", "BusRouter").filter(|n| !flag(*n, "Bypass")) {
            let target = send
                .attribute("Destination")
                .and_then(|d| d.rsplit('/').next())
                .unwrap_or_default();
            let found = auxes
                .iter()
                .find(|(n, _)| n.attribute("Name") == Some(target) && !flag(*n, "Bypass"));
            if let Some(&(_, to)) = found {
                let (level, factors) = self.send_level(send, from)?;
                self.feed(from, to, level, !flag(send, "PreFader"));
                self.move_send(from, factors);
            }
        }
        Ok(())
    }

    /// A send's saved level and the live factors its `Gain` connections
    /// scale it by: the parameter is multiplicative (BusRouter's ctor
    /// [ref] gives it a MultiplicativeParameterModulation, vtable
    /// [ref]: the law at [ref]). A macro folds in at its value;
    /// a source no bus reads (an LFO, an envelope) stays reported.
    fn send_level(&mut self, send: Node, from: usize) -> Result<(f64, Vec<ir::Factor>), String> {
        let program = self.program;
        let mut level = num(send, "Gain", 1.0)?;
        let mut factors = Vec::new();
        // The sending bus's oscillators; a bus has no voice, so each runs free.
        let mut lfos = self
            .out
            .buses
            .get_mut(from)
            .map(|b| std::mem::take(&mut b.lfos))
            .unwrap_or_default();
        let gains = matrix::children(send).filter(|c| c.attribute("Destination") == Some("Gain"));
        for c in gains {
            let Some(f) = matrix::factor(program, c, Some(&mut lfos)) else {
                continue;
            };
            match f.input.source {
                ir::Source::Constant => {
                    let held = source_node(program, c)
                        .is_some_and(|n| n.has_tag_name("ConstantModulation"));
                    if !held {
                        continue;
                    }
                    level *= f64::from(f.at(f.input.apply(1.0)));
                }
                _ => factors.push(f),
            }
            self.carried.insert(c.id());
        }
        for l in &mut lfos {
            l.clock = ir::Clock::Free;
        }
        if let Some(b) = self.out.buses.get_mut(from) {
            b.lfos = lfos;
        }
        Ok((level, factors))
    }

    /// Moves the last send of bus `from` by `factors`, as a blend of them.
    fn move_send(&mut self, from: usize, factors: Vec<ir::Factor>) {
        let Some(b) = self.out.buses.get_mut(from).filter(|_| !factors.is_empty()) else {
            return;
        };
        let Ok(send) = u8::try_from(b.sends.len().saturating_sub(1)) else {
            return;
        };
        let term = ir::Term {
            input: ir::Transfer {
                source: ir::Source::Constant,
                table: Vec::new(),
                ..Default::default()
            },
            amount: 1.0,
            factors,
        };
        let blend = ir::Blend {
            base: 0.0,
            terms: vec![term],
        };
        let source = ir::Source::Blend(index_of(&mut b.blends, blend));
        b.mods.push(ir::ModRoute::new(
            source,
            ir::ParamAddr::BusSend { send },
            1.0,
        ));
    }

    /// One insert as a slot; one not carried is empty (its input passes).
    /// `node`'s effect, built bypassed when it is saved so: a script may
    /// switch it on.
    fn slot(&mut self, node: Node) -> Result<Option<ir::Effect>, String> {
        let bypass = flag(node, "Bypass");
        let built = self.built(node)?.map(|e| ir::Effect { bypass, ..e });
        if built.is_some()
            && let Some(set) = self.knobs(node, 0)
        {
            self.knobs_of.insert(node.id(), set);
        }
        Ok(built)
    }

    /// What a script's `setParameter` on each of `node`'s numeric attributes
    /// moves: resolved directly from the declared `(effect, attribute) ->
    /// (EffectParam, law)` table with **zero** effect rebuilds at load time.
    /// Inserts of one type saved alike share the answer via `knob_cache`.
    fn knobs(&mut self, node: Node, eq_chunk: usize) -> Option<usize> {
        if self.script_words.is_empty() || !has_declared_knobs(node) {
            return None;
        }
        let key = (
            node.tag_name().name().to_owned(),
            node.attributes()
                .map(|a| (a.name().to_owned(), a.value().to_owned()))
                .collect(),
            eq_chunk,
        );
        if let Some(&known) = self.knob_cache.get(&key) {
            return known;
        }
        let mut out = Vec::new();
        for attr in node.attributes() {
            let name = attr.name();
            if !self.script_words.contains(name)
                || name == "Bypass"
                || attr.value().trim().parse::<f64>().is_err()
            {
                continue;
            }
            let controls = controls_for_chunk(node, name, eq_chunk);
            if !controls.is_empty() {
                out.push(Knob {
                    name: name.to_owned(),
                    controls,
                });
            }
        }
        let set = (!out.is_empty()).then(|| {
            if let Some(existing) = self.knob_sets.iter().position(|s| s == &out) {
                existing
            } else {
                self.knob_sets.push(out);
                self.knob_sets.len() - 1
            }
        });
        self.knob_cache.insert(key, set);
        set
    }

    fn built(&mut self, node: Node) -> Result<Option<ir::Effect>, String> {
        Ok(if node.has_tag_name("Gain") {
            let kind = ir::EffectKind::Gain(num(node, "Volume", 1.0)? as f32);
            Some(effect(kind, 1.0, 0.0))
        } else if node.has_tag_name("DigitalEq") {
            // Off a bus a slot holds one EQ: its first four bands.
            let bands = digital_eq(node, &mut self.notes)?;
            if bands.effects.len() > 1 {
                self.notes
                    .add(ir::Impact::Audible, "DigitalEq band past the fourth", "");
            }
            bands.effects.into_iter().next()
        } else if node.has_tag_name("TrackDelay") {
            // A track delay at 0 (every one in VWinds) passes its input.
            if num(node, "DelayTime", 0.0)? != 0.0 {
                self.notes.add(ir::Impact::Audible, "TrackDelay time", "");
            }
            None
        } else if node.has_tag_name("GainMatrix") {
            Some(matrix(node, &mut self.notes)?)
        } else if node.has_tag_name("ThreeBandShelves") {
            Some(shelves(node)?)
        } else if node.has_tag_name("SparkVerb") {
            Some(spark_verb(node, &mut self.notes)?)
        } else if node.has_tag_name("Maximizer") {
            Some(maximizer(node, &mut self.notes)?)
        } else if node.has_tag_name("WaveShaper") {
            wave_shaper(Params::of(node), &mut self.notes)?
        } else if node.has_tag_name("Flanger") {
            Some(flanger(Params::of(node))?)
        } else if node.has_tag_name("Phasor") {
            Some(phasor(Params::of(node), &mut self.notes)?)
        } else if node.has_tag_name("DiodeClipper") {
            Some(diode_clipper(node)?)
        } else if let Some(e) = inserts::bus_effect(node, &mut self.notes)? {
            Some(e)
        } else if node.has_tag_name("DualDelay") {
            Some(dual_delay(node, &mut self.notes)?)
        } else if node.has_tag_name("Convolver") || node.has_tag_name("SampledReverb") {
            let raw = node.has_tag_name("SampledReverb") && !flag(node, "NormalizePower");
            let wet = num(node, "Wet", 1.0)? as f32 * if raw { RAW_WET } else { 1.0 };
            let dry = num(node, "Dry", 0.0)? as f32;
            Some(match self.impulse(node)? {
                Some(impulse) => effect(ir::EffectKind::Convolution { impulse }, wet, dry),
                None => effect(ir::EffectKind::OtherWet, 1.0, dry),
            })
        } else {
            // ponytail: an effect with a wet path not carried yet plays only
            // its dry level; passing its input instead would be louder.
            wet_only(node)?.map(|dry| effect(ir::EffectKind::OtherWet, 1.0, dry))
        })
    }

    /// A Convolver's or SampledReverb's response, `None` (reported) when its
    /// file is missing.
    fn impulse(&mut self, node: Node) -> Result<Option<u16>, String> {
        let path = node.attribute("SamplePath").unwrap_or_default();
        let sample = match self.sample(path) {
            Ok(s) => s,
            Err(e) => {
                self.notes.add(
                    ir::Impact::Audible,
                    "missing impulse response",
                    format!("{path}: {e}"),
                );
                return Ok(None);
            }
        };
        let reverb = node.has_tag_name("SampledReverb");
        for (name, unset) in [("DampingLow", 0.0), ("DampingHigh", 0.0)] {
            if reverb && num(node, name, unset)? != unset {
                let feature = format!("sampled reverb {name}");
                self.notes.add(ir::Impact::Audible, &feature, path);
            }
        }
        let time = if reverb {
            num(node, "Time", 1.0)?.clamp(0.0, 1.0) as f32
        } else {
            1.0
        };
        let width = if reverb {
            num(node, "Width", 0.0)?.clamp(-1.0, 1.0) as f32
        } else {
            0.0
        };
        let stereo = sample.channels >= 2;
        let sample = self.sample_index(sample);
        self.out.impulses.push(ir::Impulse {
            sample,
            reverse: false,
            normalize: flag(node, "NormalizePower").then_some(POWER),
            predelay_ms: if reverb {
                num(node, "PreDelay", 0.0)?.max(0.0) as f32
            } else {
                0.0
            },
            cuts: None,
            envelope: None,
            mix: (width != 0.0 && stereo).then(|| widen(width)),
            size: [1.0, 1.0],
            // UVI keeps `Time` of the response and fades it by a half
            // cosine, `(cos(πn/(N+1)) + 1)/2` over the kept `N` frames
            // ([ref], π at [ref]), skipped when `N` is the
            // whole response unless UseWindowAtOriginalSize.
            // Both stay in the law: a script's `Time` moves the length at
            // run time, and the engine picks the one window from it.
            length: time,
            window_whole: flag(node, "UseWindowAtOriginalSize"),
        });
        Ok(u16::try_from(self.out.impulses.len() - 1).ok())
    }
}

impl<L: Library> Translator<'_, '_, L> {
    /// The responses a script's `loadImpulse` may name, as
    /// [`ir::Instrument::responses`]: every audio file in the response
    /// folder of one the program's convolvers play. That folder is the
    /// highest one above the played response that holds none of the zones'
    /// samples: VWinds' `Samples/IRs` (69 seats × 4 microphone pairs in
    /// `VirtSpace3`, rooms in `FalconIRs`, mutes beside them), Augmented
    /// Orchestra's `IRs` (its menu's nested rooms). Located, not read: the
    /// host decodes one when a script loads it. Only when a script calls
    /// `loadImpulse`.
    pub(super) fn responses(&mut self) {
        let loads = self.out.scripts.iter().any(|s| {
            s.source.contains("loadImpulse")
                || s.data.iter().any(|(name, bytes)| {
                    name.ends_with(".lua") && bytes.windows(11).any(|w| w == b"loadImpulse")
                })
        });
        if !loads {
            return;
        }
        let played: Vec<String> = self
            .program
            .descendants()
            .filter(|n| n.has_tag_name("Convolver") || n.has_tag_name("SampledReverb"))
            .filter_map(|n| {
                self.library
                    .path_of(self.program_dir, n.attribute("SamplePath")?)
            })
            .collect();
        let responses: std::collections::HashSet<usize> =
            self.out.impulses.iter().map(|i| i.sample).collect();
        let zones: std::collections::HashSet<String> = (self.out.samples.iter().enumerate())
            .filter(|(i, _)| !responses.contains(i))
            .map(|(_, s)| s.name.to_lowercase())
            .collect();
        let mut folders = std::collections::BTreeSet::new();
        for path in &played {
            let mut folder = parent(path);
            while let Some(up) = folder.and_then(parent) {
                let holds_zones = (self.library.audio_under(up).iter())
                    .any(|p| zones.contains(&p.to_lowercase()));
                if holds_zones {
                    break;
                }
                folder = Some(up);
            }
            folders.extend(folder.map(str::to_owned));
        }
        let mut seen = std::collections::HashSet::new();
        for folder in folders {
            for path in self.library.audio_under(&folder) {
                if !seen.insert(path.to_lowercase()) {
                    continue;
                }
                let Ok(sample) = self.sample(&path) else {
                    continue;
                };
                // The engine matches a load by its file name without the
                // extension (`load_ir_sample`'s law).
                let file = path.rsplit('/').next().unwrap_or(&path);
                let stem = file
                    .rsplit_once('.')
                    .map_or(file, |(s, _)| s)
                    .to_lowercase();
                let index = self.sample_index(sample);
                self.out.responses.push((stem, index));
            }
        }
    }
}

/// Effects with at least one script-addressable [`ir::EffectParam`] control.
fn has_declared_knobs(node: Node) -> bool {
    matches!(
        node.tag_name().name(),
        "ThreeBandShelves"
            | "DigitalEq"
            | "SparkVerb"
            | "Maximizer"
            | "WaveShaper"
            | "Flanger"
            | "Phasor"
            | "DiodeClipper"
            | "XpanderFilter"
            | "Drive"
            | "DualDelay"
            | "Convolver"
            | "SampledReverb"
    )
}

/// Knots over `[0, 1]` for curved unit-domain parameter laws.
const UNIT_KNOTS: [f64; 17] = [
    0.0, 0.0625, 0.125, 0.1875, 0.25, 0.3125, 0.375, 0.4375, 0.5, 0.5625, 0.625, 0.6875, 0.75,
    0.8125, 0.875, 0.9375, 1.0,
];
/// Extra resolution at both ends for the paired square-root wet and dry laws.
const EQUAL_POWER_KNOTS: [f64; 27] = [
    0.0, 0.0001, 0.001, 0.005, 0.01, 0.025, 0.0625, 0.125, 0.1875, 0.25, 0.3125, 0.375, 0.4375,
    0.5, 0.5625, 0.625, 0.6875, 0.75, 0.8125, 0.875, 0.9375, 0.975, 0.99, 0.995, 0.999, 0.9999,
    1.0,
];
/// Knots over `[-48, 24]` dB for exponential dB-to-linear gain laws.
const DB_KNOTS: [f64; 13] = [
    -48.0, -36.0, -24.0, -18.0, -12.0, -6.0, -3.0, 0.0, 3.0, 6.0, 12.0, 18.0, 24.0,
];
/// WaveShaper gain uses a wider range than the other dB controls.
const SHAPER_DB_KNOTS: [f64; 17] = [
    -48.0, -36.0, -24.0, -18.0, -12.0, -6.0, -3.0, 0.0, 3.0, 6.0, 12.0, 18.0, 24.0, 28.0, 32.0,
    36.0, 40.0,
];
/// Knots over `[-20, 20]` dB for `XpanderFilter` `Drive`.
const DRIVE_DB_KNOTS: [f64; 11] = [
    -20.0, -15.0, -10.0, -6.0, -3.0, 0.0, 3.0, 6.0, 10.0, 15.0, 20.0,
];
/// Log-spaced frequency knots for `Phasor` `MinFreq`/`MaxFreq`.
const FREQ_KNOTS: [f64; 14] = [
    20.0, 35.0, 60.0, 100.0, 170.0, 300.0, 500.0, 850.0, 1400.0, 2400.0, 4000.0, 7000.0, 12_000.0,
    20_000.0,
];
/// Bandwidth (octaves) knots for `DigitalEq` peak `Bandwidth{n}`.
const BW_KNOTS: [f64; 14] = [
    0.01, 0.05, 0.1, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0, 10.0,
];
/// Wide linear range matching the previous probing span.
const WIDE: (f64, f64) = (-48.0, 20_000.0);

/// Pushes `(param.code(), points)` when `param` has a script code and all points are finite.
fn push_law(
    out: &mut Vec<(u16, Vec<(f64, f32)>)>,
    param: ir::EffectParam,
    knots: &[f64],
    f: impl Fn(f64) -> f32,
) {
    let Some(code) = param.code() else {
        return;
    };
    let points: Vec<(f64, f32)> = knots
        .iter()
        .map(|&x| (x, f(x)))
        .filter(|(_, y)| y.is_finite())
        .collect();
    if points.len() >= 2
        && points
            .windows(2)
            .any(|w| matches!(w, [(_, a), (_, b)] if a != b))
    {
        out.push((code, points));
    }
}

/// Pushes an exact 2-point linear law over `[lo, hi]`.
fn push_linear(
    out: &mut Vec<(u16, Vec<(f64, f32)>)>,
    param: ir::EffectParam,
    lo: f64,
    hi: f64,
    f: impl Fn(f64) -> f32,
) {
    push_law(out, param, &[lo, hi], f);
}

/// Declared mapping from a UVI insert XML attribute `name` on `node` to the
/// [`ir::EffectParam`] controls it moves and their analytic laws — zero effect rebuilds.
fn controls_for_chunk(node: Node, name: &str, eq_chunk: usize) -> Vec<(u16, Vec<(f64, f32)>)> {
    let mut out = Vec::new();
    let val = |attr: &str, default: f64| num(node, attr, default).unwrap_or(default);
    match node.tag_name().name() {
        "ThreeBandShelves" => match name {
            "FreqLowMid" => {
                push_linear(&mut out, ir::EffectParam::BellHz(0), WIDE.0, WIDE.1, |x| {
                    x as f32
                })
            }
            "FreqMidHigh" => {
                push_linear(&mut out, ir::EffectParam::BellHz(1), WIDE.0, WIDE.1, |x| {
                    x as f32
                });
            }
            "GainLow" => {
                let mid = val("GainMid", 0.0);
                push_linear(
                    &mut out,
                    ir::EffectParam::BellGain(0),
                    WIDE.0,
                    WIDE.1,
                    |x| (x - mid) as f32,
                );
            }
            "GainHigh" => {
                let mid = val("GainMid", 0.0);
                push_linear(
                    &mut out,
                    ir::EffectParam::BellGain(1),
                    WIDE.0,
                    WIDE.1,
                    |x| (x - mid) as f32,
                );
            }
            "GainMid" => {
                let low = val("GainLow", 0.0);
                let high = val("GainHigh", 0.0);
                push_linear(
                    &mut out,
                    ir::EffectParam::BellGain(0),
                    WIDE.0,
                    WIDE.1,
                    |x| (low - x) as f32,
                );
                push_linear(
                    &mut out,
                    ir::EffectParam::BellGain(1),
                    WIDE.0,
                    WIDE.1,
                    |x| (high - x) as f32,
                );
                push_law(&mut out, ir::EffectParam::Output, &DB_KNOTS, |x| {
                    DB_GAIN.eval_unclamped(x as f32)
                });
            }
            _ => {}
        },
        "DigitalEq" => {
            // Match the groups of four enabled bands built by `digital_eq`.
            let scale = val("GainScale", 1.0);
            let mut carried = Vec::new();
            let mut band = 0;
            for n in 1..=16u8 {
                if !flag(node, &format!("Enabled{n}")) {
                    continue;
                }
                let kind = val(&format!("Type{n}"), 6.0) as i32;
                if !matches!(kind, 4..=6) {
                    continue;
                }
                if band / 4 == eq_chunk {
                    carried.push((
                        (band % 4) as u8,
                        n,
                        kind == 6,
                        val(&format!("Gain{n}"), 0.0),
                    ));
                }
                band += 1;
            }
            if name == "GainScale" {
                for &(b, _, _, gain) in &carried {
                    push_linear(
                        &mut out,
                        ir::EffectParam::BellGain(b),
                        WIDE.0,
                        WIDE.1,
                        |x| (gain * x) as f32,
                    );
                }
            } else {
                for &(b, n, is_peak, _) in &carried {
                    if let Some(rest) = name.strip_prefix("Freq")
                        && rest.parse::<u8>().ok() == Some(n)
                    {
                        push_linear(&mut out, ir::EffectParam::BellHz(b), 20.0, 20_000.0, |x| {
                            x as f32
                        });
                    } else if let Some(rest) = name.strip_prefix("Gain")
                        && rest.parse::<u8>().ok() == Some(n)
                    {
                        push_linear(
                            &mut out,
                            ir::EffectParam::BellGain(b),
                            WIDE.0,
                            WIDE.1,
                            |x| (x * scale) as f32,
                        );
                    } else if is_peak
                        && let Some(rest) = name.strip_prefix("Bandwidth")
                        && rest.parse::<u8>().ok() == Some(n)
                    {
                        push_law(&mut out, ir::EffectParam::BellQ(b), &BW_KNOTS, |x| {
                            let w = x.max(1e-3) as f32;
                            1.0 / ((w / 2.0).exp2() - (-w / 2.0).exp2())
                        });
                    }
                }
            }
        }
        "SparkVerb" => match name {
            "RoomSize" => {
                let decay = val("DecayTime", 1.0).max(0.01);
                push_linear(&mut out, ir::EffectParam::ReverbSeconds, 1.0, WIDE.1, |x| {
                    (decay * x / 10.0) as f32
                });
            }
            "DecayTime" => {
                let room = val("RoomSize", 20.0).max(1.0);
                push_linear(
                    &mut out,
                    ir::EffectParam::ReverbSeconds,
                    0.01,
                    WIDE.1,
                    |x| (x * room / 10.0) as f32,
                );
            }
            "PreDelay" => {
                push_linear(
                    &mut out,
                    ir::EffectParam::ReverbPredelay,
                    0.0,
                    WIDE.1,
                    |x| x as f32,
                );
            }
            "Rolloff" => {
                push_linear(
                    &mut out,
                    ir::EffectParam::ReverbHighCut,
                    WIDE.0,
                    WIDE.1,
                    |x| x as f32,
                );
            }
            "Width" => {
                push_linear(&mut out, ir::EffectParam::ReverbStereo, 0.0, 1.0, |x| {
                    x as f32
                });
            }
            "Mix" => {
                if val("MixMode", 0.0) == 1.0 {
                    push_law(&mut out, ir::EffectParam::Output, &EQUAL_POWER_KNOTS, |x| {
                        SQUARE_ROOT.eval_f64(x) as f32
                    });
                    push_law(&mut out, ir::EffectParam::Dry, &EQUAL_POWER_KNOTS, |x| {
                        SQUARE_ROOT.eval_f64(1.0 - x) as f32
                    });
                } else {
                    push_linear(&mut out, ir::EffectParam::Output, 0.0, 1.0, |x| x as f32);
                    push_linear(&mut out, ir::EffectParam::Dry, 0.0, 1.0, |x| {
                        (1.0 - x) as f32
                    });
                }
            }
            "MixMode" => {
                let mix = val("Mix", 1.0).clamp(0.0, 1.0);
                push_law(&mut out, ir::EffectParam::Output, &[0.0, 1.0], |x| {
                    if x == 1.0 {
                        SQUARE_ROOT.eval_f64(mix) as f32
                    } else {
                        mix as f32
                    }
                });
                push_law(&mut out, ir::EffectParam::Dry, &[0.0, 1.0], |x| {
                    if x == 1.0 {
                        SQUARE_ROOT.eval_f64(1.0 - mix) as f32
                    } else {
                        (1.0 - mix) as f32
                    }
                });
            }
            _ => {}
        },
        "Maximizer" => match name {
            "Threshold" => {
                let ceiling = val("Ceiling", -0.1);
                push_linear(
                    &mut out,
                    ir::EffectParam::LimiterInput,
                    WIDE.0,
                    WIDE.1,
                    |x| (ceiling - x) as f32,
                );
            }
            "Ceiling" => {
                let threshold = val("Threshold", 0.0);
                push_linear(
                    &mut out,
                    ir::EffectParam::LimiterInput,
                    WIDE.0,
                    WIDE.1,
                    |x| (x - threshold) as f32,
                );
            }
            "SlewRate" => {
                push_linear(
                    &mut out,
                    ir::EffectParam::LimiterRelease,
                    0.0,
                    WIDE.1,
                    |x| (x * 20.0 * LOG10_2 / 10.0) as f32,
                );
            }
            _ => {}
        },
        "WaveShaper" => {
            let mode = val("Mode", 0.0);
            if mode.fract() != 0.0 || !(0.0..=3.0).contains(&mode) {
                return out;
            }
            let mode = mode as u8;
            let input = DB_GAIN.eval_unclamped(val("InputGain", 0.0) as f32);
            let out_gain = DB_GAIN.eval_unclamped(val("OutputGain", 0.0) as f32);
            let d = WAVESHAPER_DRIVE.eval(val("Amount", 0.0) as f32);
            let mix = val("Mix", 1.0).clamp(0.0, 1.0) as f32;
            let shaper_scale = |drv: f32| match mode {
                0 => 1.0,
                1 => 1.0 / FRAC_PI_2,
                2 => 1.0 / (2.0 * drv).tanh(),
                _ => 1.0 / (2.0 * drv).atan(),
            };
            let curve_amt = |drv: f32, inp: f32| match mode {
                0 | 1 => drv * FRAC_PI_2 * inp,
                _ => drv * inp,
            };
            let scale = shaper_scale(d);
            match name {
                "InputGain" => {
                    push_law(
                        &mut out,
                        ir::EffectParam::ShaperAmount,
                        &SHAPER_DB_KNOTS,
                        |x| curve_amt(d, DB_GAIN.eval_unclamped(x as f32)),
                    );
                }
                "Amount" => {
                    push_law(&mut out, ir::EffectParam::ShaperAmount, &UNIT_KNOTS, |x| {
                        curve_amt(WAVESHAPER_DRIVE.eval(x as f32), input)
                    });
                    if matches!(mode, 2 | 3) {
                        push_law(&mut out, ir::EffectParam::Output, &UNIT_KNOTS, |x| {
                            mix * out_gain * shaper_scale(WAVESHAPER_DRIVE.eval(x as f32))
                        });
                    }
                }
                "Mix" => {
                    push_linear(&mut out, ir::EffectParam::Output, 0.0, 1.0, |x| {
                        x as f32 * out_gain * scale
                    });
                    push_linear(&mut out, ir::EffectParam::Dry, 0.0, 1.0, |x| {
                        (1.0 - x) as f32
                    });
                }
                "OutputGain" => {
                    push_law(&mut out, ir::EffectParam::Output, &SHAPER_DB_KNOTS, |x| {
                        mix * DB_GAIN.eval_unclamped(x as f32) * scale
                    });
                }
                _ => {}
            }
        }
        "Flanger" => match name {
            "DelayTime" => {
                let depth = val("Depth", 0.5).clamp(0.0, 1.0);
                push_linear(&mut out, ir::EffectParam::DelayMs, 0.0, 1.0, |x| {
                    (20.0 * (x + depth / 2.0)) as f32
                });
            }
            "Depth" => {
                let time = val("DelayTime", 0.0).clamp(0.0, 1.0);
                push_linear(&mut out, ir::EffectParam::DelayMs, 0.0, 1.0, |x| {
                    (20.0 * (time + x / 2.0)) as f32
                });
            }
            "Feedback" => {
                push_linear(
                    &mut out,
                    ir::EffectParam::DelayLoop(0),
                    WIDE.0,
                    WIDE.1,
                    |x| x as f32,
                );
                push_linear(
                    &mut out,
                    ir::EffectParam::DelayLoop(3),
                    WIDE.0,
                    WIDE.1,
                    |x| x as f32,
                );
            }
            "Speed" if !flag(node, "SyncToHost") => {
                push_linear(&mut out, ir::EffectParam::ModRate, 1e-3, WIDE.1, |x| {
                    x as f32
                });
            }
            "Mix" => {
                push_linear(&mut out, ir::EffectParam::Output, 0.0, 1.0, |x| x as f32);
            }
            _ => {}
        },
        "Phasor" => match name {
            "MinFreq" => {
                let high = val("MaxFreq", 3000.0).max(1.0);
                push_law(
                    &mut out,
                    ir::EffectParam::PhaserExponent,
                    &FREQ_KNOTS,
                    |x| (0.5 * (high / x.max(1.0)).ln()) as f32,
                );
            }
            "MaxFreq" => {
                let low = val("MinFreq", 200.0).max(1.0);
                push_law(
                    &mut out,
                    ir::EffectParam::PhaserExponent,
                    &FREQ_KNOTS,
                    |x| (0.5 * (x.max(1.0) / low).ln()) as f32,
                );
            }
            "Feedback" => {
                push_linear(
                    &mut out,
                    ir::EffectParam::PhaserFeedback,
                    WIDE.0,
                    WIDE.1,
                    |x| x as f32,
                );
            }
            "Speed" if !flag(node, "SyncToHost") => {
                push_linear(&mut out, ir::EffectParam::ModRate, 1e-3, 10.0, |x| x as f32);
            }
            "Spread" => {
                push_linear(&mut out, ir::EffectParam::ModPhase, 0.0, 1.0, |x| {
                    (x / 4.0) as f32
                });
            }
            "Depth" => {
                push_law(&mut out, ir::EffectParam::Output, &UNIT_KNOTS, |x| {
                    let d = x as f32;
                    d / (1.0 + d * d).sqrt()
                });
                push_law(&mut out, ir::EffectParam::Dry, &UNIT_KNOTS, |x| {
                    let d = x as f32;
                    1.0 / (1.0 + d * d).sqrt()
                });
            }
            _ => {}
        },
        "DiodeClipper" => {
            if name == "OutputGain" {
                push_law(&mut out, ir::EffectParam::Output, &DB_KNOTS, |x| {
                    DB_GAIN.eval_unclamped(x as f32)
                });
            }
        }
        "XpanderFilter" if super::filters::on_bus(node) => match name {
            "Freq" => {
                push_linear(&mut out, ir::EffectParam::FilterHz, 20.0, 20_000.0, |x| {
                    x as f32
                });
            }
            "Q" => {
                for stage in 0..3u8 {
                    push_linear(
                        &mut out,
                        ir::EffectParam::FilterDamping(stage),
                        0.0,
                        1.0,
                        |x| (2.0 * (1.0 - x)) as f32,
                    );
                }
            }
            "Drive" => {
                push_law(
                    &mut out,
                    ir::EffectParam::FilterGain,
                    &DRIVE_DB_KNOTS,
                    |x| DB_GAIN.eval_unclamped(x as f32),
                );
            }
            _ => {}
        },
        "Drive" => {
            if name == "DriveAmount" && val("Mode", 0.0) == 0.0 {
                push_law(&mut out, ir::EffectParam::ShaperAmount, &UNIT_KNOTS, |x| {
                    WAVESHAPER_DRIVE.eval(x as f32)
                });
                push_law(&mut out, ir::EffectParam::Output, &UNIT_KNOTS, |x| {
                    1.0 / WAVESHAPER_DRIVE.eval(x as f32).tanh().max(f32::EPSILON)
                });
            }
        }
        "DualDelay" => {
            let rot = |rad: f64| -> [[f32; 2]; 2] {
                let (s, c) = (rad as f32).sin_cos();
                [[c, -s], [s, c]]
            };
            let wid = |w: f64| -> [[f32; 2]; 2] {
                let (same, other) = ((1.0 + w as f32) / 2.0, (1.0 - w as f32) / 2.0);
                [[same, other], [other, same]]
            };
            let mm = |a: [[f32; 2]; 2], b: [[f32; 2]; 2]| -> [[f32; 2]; 2] {
                let [[b00, b01], [b10, b11]] = b;
                a.map(|[x, y]| [x * b00 + y * b10, x * b01 + y * b11])
            };
            let fb_pair = |fb: f64, ratio: f64| -> [f32; 2] {
                let (fb, r) = (fb as f32, ratio.clamp(-1.0, 1.0) as f32);
                if r <= 0.0 {
                    [(1.0 + r) * fb, fb]
                } else {
                    [fb, (1.0 - r) * fb]
                }
            };
            let cell_at = |m: [[f32; 2]; 2], cell: u8| -> f32 {
                let [[a, b], [c, d]] = m;
                match cell {
                    0 => a,
                    1 => b,
                    2 => c,
                    _ => d,
                }
            };
            match name {
                "DelayTime" if !flag(node, "SyncToHost") => {
                    push_linear(&mut out, ir::EffectParam::DelayMs, 0.0, 5.0, |x| {
                        (x * 1000.0) as f32
                    });
                }
                "HighCut" => {
                    push_linear(
                        &mut out,
                        ir::EffectParam::DelayDampingHz,
                        WIDE.0,
                        WIDE.1,
                        |x| x as f32,
                    );
                }
                "LowCut" => {
                    push_linear(&mut out, ir::EffectParam::DelayLowCutHz, 0.0, WIDE.1, |x| {
                        x as f32
                    });
                }
                "ModRate" => {
                    push_linear(&mut out, ir::EffectParam::ModRate, WIDE.0, WIDE.1, |x| {
                        x as f32
                    });
                }
                "Mix" => {
                    push_law(&mut out, ir::EffectParam::Output, &EQUAL_POWER_KNOTS, |x| {
                        SQUARE_ROOT.eval(x as f32)
                    });
                    push_law(&mut out, ir::EffectParam::Dry, &EQUAL_POWER_KNOTS, |x| {
                        SQUARE_ROOT.eval(1.0 - x as f32)
                    });
                }
                "Feedback" => {
                    let ratio = val("FeedbackRatio", 0.0);
                    let in_m = mm(
                        rot(val("InputRotation", 0.0) * FRAC_PI_4),
                        wid(val("InputWidth", 1.0)),
                    );
                    let fb_m = rot(val("Rotation", 0.0).to_radians());
                    for cell in 0..4u8 {
                        let top = cell < 2;
                        let k_in = cell_at(in_m, cell);
                        let k_fb = cell_at(fb_m, cell);
                        push_linear(
                            &mut out,
                            ir::EffectParam::DelayInput(cell),
                            WIDE.0,
                            WIDE.1,
                            |x| {
                                let [f0, f1] = fb_pair(x, ratio);
                                k_in * if top { f0 } else { f1 }
                            },
                        );
                        push_linear(
                            &mut out,
                            ir::EffectParam::DelayLoop(cell),
                            WIDE.0,
                            WIDE.1,
                            |x| {
                                let [f0, f1] = fb_pair(x, ratio);
                                k_fb * if top { f0 } else { f1 }
                            },
                        );
                    }
                }
                "FeedbackRatio" => {
                    let fb = val("Feedback", 0.3);
                    let in_m = mm(
                        rot(val("InputRotation", 0.0) * FRAC_PI_4),
                        wid(val("InputWidth", 1.0)),
                    );
                    let fb_m = rot(val("Rotation", 0.0).to_radians());
                    for cell in 0..4u8 {
                        let top = cell < 2;
                        let k_in = cell_at(in_m, cell);
                        let k_fb = cell_at(fb_m, cell);
                        push_law(
                            &mut out,
                            ir::EffectParam::DelayInput(cell),
                            &[-1.0, 0.0, 1.0],
                            |x| {
                                let [f0, f1] = fb_pair(fb, x);
                                k_in * if top { f0 } else { f1 }
                            },
                        );
                        push_law(
                            &mut out,
                            ir::EffectParam::DelayLoop(cell),
                            &[-1.0, 0.0, 1.0],
                            |x| {
                                let [f0, f1] = fb_pair(fb, x);
                                k_fb * if top { f0 } else { f1 }
                            },
                        );
                    }
                }
                "InputWidth" => {
                    let [fb0, fb1] = fb_pair(val("Feedback", 0.3), val("FeedbackRatio", 0.0));
                    let r = rot(val("InputRotation", 0.0) * FRAC_PI_4);
                    for cell in 0..4u8 {
                        let s = if cell < 2 { fb0 } else { fb1 };
                        push_linear(
                            &mut out,
                            ir::EffectParam::DelayInput(cell),
                            WIDE.0,
                            WIDE.1,
                            |x| cell_at(mm(r, wid(x)), cell) * s,
                        );
                    }
                }
                "InputRotation" => {
                    let [fb0, fb1] = fb_pair(val("Feedback", 0.3), val("FeedbackRatio", 0.0));
                    let w = wid(val("InputWidth", 1.0));
                    let knots: [f64; 17] = std::array::from_fn(|i| -4.0 + 0.5 * i as f64);
                    for cell in 0..4u8 {
                        let s = if cell < 2 { fb0 } else { fb1 };
                        push_law(&mut out, ir::EffectParam::DelayInput(cell), &knots, |x| {
                            cell_at(mm(rot(x * FRAC_PI_4), w), cell) * s
                        });
                    }
                }
                "Rotation" => {
                    let [fb0, fb1] = fb_pair(val("Feedback", 0.3), val("FeedbackRatio", 0.0));
                    let knots: [f64; 25] = std::array::from_fn(|i| -180.0 + 15.0 * i as f64);
                    for cell in 0..4u8 {
                        let s = if cell < 2 { fb0 } else { fb1 };
                        push_law(&mut out, ir::EffectParam::DelayLoop(cell), &knots, |x| {
                            cell_at(rot(x.to_radians()), cell) * s
                        });
                    }
                }
                _ => {}
            }
        }
        "Convolver" | "SampledReverb" => match name {
            "Wet" => {
                let scale = if node.has_tag_name("SampledReverb") && !flag(node, "NormalizePower") {
                    f64::from(RAW_WET)
                } else {
                    1.0
                };
                push_linear(&mut out, ir::EffectParam::Output, WIDE.0, WIDE.1, |x| {
                    (x * scale) as f32
                });
            }
            "Dry" => {
                push_linear(&mut out, ir::EffectParam::Dry, WIDE.0, WIDE.1, |x| x as f32);
            }
            _ => {}
        },
        _ => {}
    }
    out
}

/// The folder of a member path; `None` at the bank's root.
fn parent(path: &str) -> Option<&str> {
    path.rsplit_once('/')
        .map(|(f, _)| f)
        .filter(|f| !f.is_empty())
}

/// A sampled reverb's wet level when its response is not normalized: UVI's
/// SampledReverbState audio path scales the convolved signal by 0.1 (−20 dB)
/// unless NormalizePower is set ([ref], 0.1 at [ref]), before
/// Wet. The Convolver's paths apply only Wet.
const RAW_WET: f32 = 0.1;

/// `NormalizePower`: the response scaled to unit energy (sum of squares).
/// UVI (SampledReverb/Convolver IR build [ref]) divides by the root
/// of the channels' mean energy, floored at 1e-10, after
/// resampling to the host rate.
const POWER: ir::Normalize = ir::Normalize {
    energy: 1.0,
    max_gain: 1e5,
    floor: 0.0,
    channels: ir::EnergyChannels::Mean,
    after_resample: true,
};

/// A sampled reverb's `Width` (-1..1) on a stereo response: UVI (IR build
/// [ref], from setter [ref]) takes `c = -w`, `d = 2 + w` (2.0
/// at [ref]) scaled to unit length, then `L = dL + cR` and
/// `R = dR + cL` with the new `L`, before NormalizePower. A response with
/// one channel has no pair and is left alone.
fn widen(w: f32) -> [[f32; 2]; 2] {
    let (c, d) = (-w, 2.0 + w);
    let n = c.hypot(d).recip();
    let (c, d) = (c * n, d * n);
    [[d, c], [c * d, d + c * c]]
}

/// Silence for an envelope knot, dB.
const SILENT: f32 = -200.0;

/// ThreeBandShelves: UVI's one biquad ([ref] → [ref]) is the
/// mid gain times a first-order low shelf at FreqLowMid and a first-order
/// high shelf at FreqMidHigh, each by its band's gain over the mid's
/// (bilinear, tan-prewarped, [ref]). A cookbook shelf at slope 0.5 is
/// that first-order shelf exactly.
fn shelves(node: Node) -> Result<ir::Effect, String> {
    let mid = num(node, "GainMid", 0.0)? as f32;
    let shelf = |shape, hz: &str, default, gain: &str| -> Result<ir::Bell, String> {
        Ok(ir::Bell {
            shape,
            hz: num(node, hz, default)? as f32,
            q: 0.5,
            gain_db: num(node, gain, 0.0)? as f32 - mid,
        })
    };
    let bands = [
        shelf(ir::Shape::LowShelf, "FreqLowMid", 200.0, "GainLow")?,
        shelf(ir::Shape::HighShelf, "FreqMidHigh", 4000.0, "GainHigh")?,
        ir::Bell::FLAT,
        ir::Bell::FLAT,
    ];
    Ok(effect(
        ir::EffectKind::Eq(bands),
        DB_GAIN.eval_unclamped(mid),
        0.0,
    ))
}

/// A bus GainMatrix: `Gain_i_j` is input channel `i` into output `j`. The
/// buses carry two channels (a keygroup's matrix sums its sample channels
/// into the first two), so the top-left block is the whole matrix; a row
/// past the second feeding them is reported.
fn matrix(node: Node, notes: &mut Notes) -> Result<ir::Effect, String> {
    let cell = |i: u8, j: u8| num(node, &format!("Gain_{i}_{j}"), f64::from(u8::from(i == j)));
    for i in 3..=12 {
        if cell(i, 1)? != 0.0 || cell(i, 2)? != 0.0 {
            notes.add(
                ir::Impact::Audible,
                "GainMatrix channel past the second",
                "",
            );
            break;
        }
    }
    let [ll, lr, rl, rr] = [(1, 1), (2, 1), (1, 2), (2, 2)].map(|(i, j)| cell(i, j));
    let m = [[ll? as f32, lr? as f32], [rl? as f32, rr? as f32]];
    Ok(effect(ir::EffectKind::Matrix(m), 1.0, 0.0))
}

/// UVI's Maximizer (process [ref], prepare [ref], parameter
/// ids [ref]..2e4). Per block it takes the louder channel in dB,
/// floored at Threshold (or through a quadratic knee `(x−T+k)²/4k` within
/// ±Knee), and turns it into a gain of `Ceiling − level` dB after:
///
/// - a slew limit: the level falls at most `10000/SlewRate` dB/s (prepare
///   `10000/(rate·SlewRate)` per frame, no limit at 0); rises are free;
/// - two one-pole falls blended by ReleaseBlend: one to a
///   third in Release ms (`e^(ln(1/3)/frames)`, ln(1/3) at [ref]),
///   one 100× slower (`e^(−0.010986/frames)`); a rise uses Attack, which
///   CeilingMode 0 (Brickwall, the default) forces to 0;
/// - the largest level over the last `2·⌊Lookahead·rate/2000⌋` frames
///, applied to the audio delayed by as many frames.
///
/// CeilingMode 2 (Hard) also clamps the output at Ceiling (`10^(C/20)`).
/// Threshold-to-Ceiling is input gain into a limiter at Ceiling.
///
/// ponytail: TruePeak (Off in every corpus program) adds a 6-frame
/// oversampled detector; it is reported, not modelled.
fn maximizer(node: Node, notes: &mut Notes) -> Result<ir::Effect, String> {
    let threshold = num(node, "Threshold", 0.0)? as f32;
    let ceiling = num(node, "Ceiling", -0.1)? as f32;
    let mode = num(node, "CeilingMode", 0.0)?;
    if num(node, "TruePeak", 0.0)? != 0.0 {
        notes.add(ir::Impact::Audible, "maximizer TruePeak", "");
    }
    let slew = num(node, "SlewRate", 0.0)?;
    let kind = ir::EffectKind::Limiter(ir::Limiter {
        input_db: ceiling - threshold,
        ceiling_db: ceiling,
        // 6.02 dB at 10000/SlewRate dB/s.
        release_ms: if slew > 0.0 {
            (slew * 20.0 * std::f64::consts::LOG10_2 / 10.0) as f32
        } else {
            0.0
        },
        knee_db: num(node, "Knee", 0.0)?.max(0.0) as f32,
        attack_ms: if mode == 0.0 {
            0.0
        } else {
            num(node, "Attack", 0.0)?.max(0.0) as f32
        },
        smooth_ms: num(node, "Release", 10.0)?.max(0.0) as f32,
        slow: num(node, "ReleaseBlend", 0.0)?.clamp(0.0, 1.0) as f32,
        lookahead_ms: num(node, "Lookahead", 0.0)?.max(0.0) as f32,
        clip: mode == 2.0,
    });
    Ok(effect(kind, 1.0, 0.0))
}

/// The dry level of an effect whose output is mostly a wet path (a reverb,
/// a delay); `None` for one that processes in place.
fn wet_only(node: Node) -> Result<Option<f32>, String> {
    let tag = node.tag_name().name();
    Ok(match tag {
        "FeedbackMachine" => Some(1.0 - num(node, "Mix", 1.0)?.clamp(0.0, 1.0) as f32),
        _ => None,
    })
}

/// A bus that only feeds its sends until [`Translator::strip`] gives it a
/// fader on the main output.
fn bus(name: &str) -> ir::Bus {
    ir::Bus {
        to_main: false,
        ..ir::Bus::new(name)
    }
}

fn effect(kind: ir::EffectKind, output: f32, dry: f32) -> ir::Effect {
    ir::Effect {
        kind,
        bypass: false,
        output,
        dry,
    }
}

/// The part a program plays in: UVIWorkstation builds each part
/// ([ref], always with defaults on) with three connections
/// ([ref]..[ref]): `@MIDI CC 7` and `@MIDI CC 10` in Edition mode
/// (ConnectionMode 1: the controller sets the knob) on the part's Gain
/// (linear 0..10^(6/20), [ref]) and Pan (−1..1, [ref]), and
/// `@MIDI CC 11` in Modulation mode at ratio 1 on Gain, `(1 − r) + r·v/127`.
/// The script MIDI state starts CC 11 at 127. Gain and pan reach the mixer
/// as signals (per-sample buffers or per-block constants) from the
/// parameter; no smoother on them is found yet, so the output's measured
/// glide stands in.
pub(super) const PART_CONTROLLERS: ir::OutputControllers = ir::OutputControllers {
    volume: 7,
    pan: 10,
    top_db: 6.0,
    power: 1,
    expression: Some(11),
};

/// A stereo signal's pan in UVIWorkstation's mixer ([ref], the
/// stereo-to-stereo case of the layout switch at [ref], whatever the
/// element's PanLaw) turns the far channel to `(1 + sin(π/2 + π·|pan|))/2`
/// via sinf [ref]. PanLaw (0 by default, never saved by the programs
/// read) picks only the mono-to-stereo law: 0 is `(1 ∓ sin(π/2·pan))/2`
///, 1 the equal-power `sin(π/4·(1 ∓ pan))`.
pub(super) const PART_BALANCE: ir::Balance = ir::Balance::Cosine;

/// The voices a program sounds: `Polyphony` times the most zones a keygroup
/// makes, one per oscillator or bundle member. A UVI voice is a keygroup's,
/// all of them (VWinds' Alto Flute 2: 8 voices, a note's 10 sustain zones
/// beside its chiff, air and key click); here each zone is a voice.
pub(super) fn voices(program: Node) -> Result<u16, String> {
    let widest = f64::from(zones_per_note(program));
    Ok((num(program, "Polyphony", 64.0)? * widest).clamp(1.0, 1024.0) as u16)
}

/// The most zones one keygroup under `node` sounds for a note.
fn zones_per_note(node: Node) -> u16 {
    let zones = |p: Node| {
        p.attribute("SamplePath")
            .map_or(1, |s| s.split('*').count().max(2) - 1)
    };
    let players = |o: Node| {
        o.children()
            .filter(Node::is_element)
            .map(zones)
            .sum::<usize>()
    };
    let oscillators = node.descendants().filter(|n| n.has_tag_name("Oscillators"));
    let widest = oscillators.map(players).max().unwrap_or(1);
    u16::try_from(widest.clamp(1, 1024)).unwrap_or(1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_maximizer_lifts_by_its_threshold_into_a_limiter_at_its_ceiling() {
        // Augmented Orchestra's most common setting (90 programs).
        let xml = r#"<Maximizer Threshold="-6" Attack="0" Lookahead="2" Release="10" Ceiling="-0.1" Knee="0" ReleaseBlend="0.1" SlewRate="20"/>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let e = maximizer(doc.root_element(), &mut Notes::default()).unwrap();
        let ir::EffectKind::Limiter(l) = e.kind else {
            panic!("{e:?}")
        };
        assert_eq!((l.input_db, l.ceiling_db), (5.9, -0.1));
        // 500 dB/s: 6.02 dB in 12.04 ms.
        assert!((l.release_ms - 12.04).abs() < 0.01, "{}", l.release_ms);
        assert_eq!(
            (l.smooth_ms, l.slow, l.lookahead_ms, l.attack_ms, l.clip),
            (10.0, 0.1, 2.0, 0.0, false)
        );
        assert_eq!((e.output, e.dry), (1.0, 0.0));
    }

    #[test]
    fn three_band_shelves_are_two_half_slope_shelves_around_the_mid_gain() {
        let xml = r#"<ThreeBandShelves FreqLowMid="250" FreqMidHigh="5000" GainLow="3" GainMid="-1" GainHigh="4"/>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let e = shelves(doc.root_element()).unwrap();
        let ir::EffectKind::Eq([low, high, ..]) = e.kind else {
            panic!("{e:?}")
        };
        assert_eq!(
            (low.shape, low.hz, low.q, low.gain_db),
            (ir::Shape::LowShelf, 250.0, 0.5, 4.0)
        );
        assert_eq!(
            (high.shape, high.hz, high.gain_db),
            (ir::Shape::HighShelf, 5000.0, 5.0)
        );
        assert!((e.output - 0.891).abs() < 1e-3, "{}", e.output);
    }

    #[test]
    fn width_is_the_native_sequential_pair_mix() {
        assert_eq!(widen(0.0), [[1.0, 0.0], [0.0, 1.0]]);
        // [ref]'s loop on (L, R) = (0.3, -0.7), in its own order.
        for w in [1.0f32, 0.5, -0.4, -1.0] {
            let n = (-w).hypot(2.0 + w).recip();
            let (c, d) = (-w * n, (2.0 + w) * n);
            let l = c * -0.7 + d * 0.3;
            let r = l * c + d * -0.7;
            let [a, b] = widen(w);
            let (x, y) = (a[0] * 0.3 + a[1] * -0.7, b[0] * 0.3 + b[1] * -0.7);
            assert!(
                (x - l).abs() < 1e-6 && (y - r).abs() < 1e-6,
                "{w}: {x} {y} vs {l} {r}"
            );
        }
        // Full width: the sides lean apart, a quarter of each into the other.
        let [a, _] = widen(1.0);
        assert!((a[1] / a[0] + 1.0 / 3.0).abs() < 1e-6);
    }

    fn declared_controls(node: Node, name: &str) -> Vec<(u16, Vec<(f64, f32)>)> {
        controls_for_chunk(node, name, 0)
    }

    fn eval_piecewise(points: &[(f64, f32)], x: f64) -> f32 {
        let Some(&(_, first)) = points.first() else {
            return 0.0;
        };
        if let Some(&(last_x, last_y)) = points.last()
            && x >= last_x
        {
            return last_y;
        }
        for w in points.windows(2) {
            if let [(x0, y0), (x1, y1)] = *w
                && x < x1
            {
                return if x <= x0 {
                    y0
                } else {
                    y0 + (y1 - y0) * ((x - x0) / (x1 - x0)) as f32
                };
            }
        }
        first
    }

    #[test]
    fn equal_power_mix_resolves_quiet_shares() {
        for xml in [r#"<SparkVerb MixMode="1"/>"#, "<DualDelay/>"] {
            let doc = roxmltree::Document::parse(xml).unwrap();
            let laws = declared_controls(doc.root_element(), "Mix");
            for (param, x, off, on) in [
                (ir::EffectParam::Output, 0.01, 0.0, 1.0),
                (ir::EffectParam::Dry, 0.99, 1.0, 0.0),
            ] {
                let (_, points) = laws
                    .iter()
                    .find(|(code, _)| Some(*code) == param.code())
                    .unwrap();
                assert!((eval_piecewise(points, x) - 0.1).abs() < 1e-6);
                assert_eq!(eval_piecewise(points, 0.0), off);
                assert_eq!(eval_piecewise(points, 1.0), on);
            }
        }
    }

    #[test]
    fn waveshaper_gain_laws_reach_the_saved_effect_at_40_db() {
        let doc = roxmltree::Document::parse(
            r#"<WaveShaper Mode="0" Mix="1" InputGain="40" OutputGain="40"/>"#,
        )
        .unwrap();
        let node = doc.root_element();
        let mut built = wave_shaper(Params::of(node), &mut Notes::default())
            .unwrap()
            .unwrap();
        for (name, param) in [
            ("InputGain", ir::EffectParam::ShaperAmount),
            ("OutputGain", ir::EffectParam::Output),
        ] {
            let laws = declared_controls(node, name);
            let (code, points) = laws.first().unwrap();
            assert_eq!(Some(*code), param.code());
            assert!((eval_piecewise(points, 40.0) - param.value(&mut built).unwrap()).abs() < 1e-5);
        }
    }

    #[test]
    fn declared_knob_laws_match_built_effects_with_zero_rebuilds() {
        // ThreeBandShelves
        let xml = r#"<ThreeBandShelves FreqLowMid="250" FreqMidHigh="5000" GainLow="3" GainMid="-1" GainHigh="4"/>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = doc.root_element();
        let mid_controls = declared_controls(node, "GainMid");
        assert_eq!(mid_controls.len(), 3);
        let gain0 = mid_controls
            .iter()
            .find(|(c, _)| *c == ir::EffectParam::BellGain(0).code().unwrap())
            .unwrap();
        assert!((eval_piecewise(&gain0.1, 2.0) - 1.0).abs() < 1e-5);
        let out_ctrl = mid_controls
            .iter()
            .find(|(c, _)| *c == ir::EffectParam::Output.code().unwrap())
            .unwrap();
        assert!((eval_piecewise(&out_ctrl.1, -6.0) - DB_GAIN.eval_unclamped(-6.0)).abs() < 1e-4);

        // SparkVerb equal-power Mix
        let sv_xml = r#"<SparkVerb RoomSize="20" DecayTime="2" Mix="0.5" MixMode="1" PreDelay="10" Rolloff="8000" Width="0.8"/>"#;
        let sv_doc = roxmltree::Document::parse(sv_xml).unwrap();
        let sv_node = sv_doc.root_element();
        let mix_ctrls = declared_controls(sv_node, "Mix");
        assert_eq!(mix_ctrls.len(), 2);
        let wet_at_half = eval_piecewise(&mix_ctrls.first().unwrap().1, 0.5);
        assert!((wet_at_half - 0.5f32.sqrt()).abs() < 1e-3, "{wet_at_half}");
        let decay_ctrls = declared_controls(sv_node, "DecayTime");
        assert_eq!(decay_ctrls.len(), 1);
        assert!((eval_piecewise(&decay_ctrls.first().unwrap().1, 3.0) - 6.0).abs() < 1e-5);

        // Maximizer Threshold / SlewRate
        let mx_xml = r#"<Maximizer Threshold="-6" Ceiling="-0.1" SlewRate="20"/>"#;
        let mx_doc = roxmltree::Document::parse(mx_xml).unwrap();
        let thr_ctrls = declared_controls(mx_doc.root_element(), "Threshold");
        assert_eq!(thr_ctrls.len(), 1);
        assert!((eval_piecewise(&thr_ctrls.first().unwrap().1, -10.0) - 9.9).abs() < 1e-4);

        // DualDelay Feedback + Mix
        let dd_xml = r#"<DualDelay SyncToHost="0" DelayTime="0.25" Feedback="0.4" FeedbackRatio="0.5" Mix="0.25" Rotation="90" InputRotation="1" InputWidth="1"/>"#;
        let dd_doc = roxmltree::Document::parse(dd_xml).unwrap();
        let dd_node = dd_doc.root_element();
        let dt_ctrls = declared_controls(dd_node, "DelayTime");
        assert_eq!(dt_ctrls.len(), 1);
        assert!((eval_piecewise(&dt_ctrls.first().unwrap().1, 0.75) - 750.0).abs() < 1e-3);
        let fb_ctrls = declared_controls(dd_node, "Feedback");
        assert!(!fb_ctrls.is_empty());

        // Flanger + Phasor + XpanderFilter
        let fl_doc = roxmltree::Document::parse(
            r#"<Flanger SyncToHost="0" Speed="0.4" Feedback="0.3" DelayTime="0.2" Depth="0.5" Mix="0.8"/>"#,
        )
        .unwrap();
        assert_eq!(
            declared_controls(fl_doc.root_element(), "Feedback").len(),
            2
        );
        let ph_doc = roxmltree::Document::parse(
            r#"<Phasor SyncToHost="0" Speed="0.5" MinFreq="200" MaxFreq="3000" Feedback="0.4" Depth="0.5" Spread="1"/>"#,
        )
        .unwrap();
        let ph_depth = declared_controls(ph_doc.root_element(), "Depth");
        assert_eq!(ph_depth.len(), 2);
        let want_out = 0.5f32 / (1.0 + 0.25f32).sqrt();
        assert!((eval_piecewise(&ph_depth.first().unwrap().1, 0.5) - want_out).abs() < 1e-3);

        let xp_doc = roxmltree::Document::parse(
            r#"<XpanderFilter DistortionType="0" Algorithm="0" Mode="3" Freq="1000" Q="0.5" Drive="6"/>"#,
        )
        .unwrap();
        assert_eq!(declared_controls(xp_doc.root_element(), "Q").len(), 3);
        assert_eq!(declared_controls(xp_doc.root_element(), "Freq").len(), 1);

        let sr_raw = roxmltree::Document::parse(
            r#"<SampledReverb NormalizePower="0" Wet="0.5" Dry="0.25"/>"#,
        )
        .unwrap();
        let sr_raw_wet = declared_controls(sr_raw.root_element(), "Wet");
        assert_eq!(sr_raw_wet.len(), 1);
        assert_eq!(
            Some(sr_raw_wet.first().unwrap().0),
            ir::EffectParam::Output.code()
        );
        assert!((eval_piecewise(&sr_raw_wet.first().unwrap().1, 0.5) - 0.5 * RAW_WET).abs() < 1e-6);
        let sr_raw_dry = declared_controls(sr_raw.root_element(), "Dry");
        assert_eq!(sr_raw_dry.len(), 1);
        assert_eq!(
            Some(sr_raw_dry.first().unwrap().0),
            ir::EffectParam::Dry.code()
        );
        assert!((eval_piecewise(&sr_raw_dry.first().unwrap().1, 0.25) - 0.25).abs() < 1e-6);

        let sr_norm = roxmltree::Document::parse(
            r#"<SampledReverb NormalizePower="1" Wet="0.5" Dry="0.25"/>"#,
        )
        .unwrap();
        let sr_norm_wet = declared_controls(sr_norm.root_element(), "Wet");
        assert!((eval_piecewise(&sr_norm_wet.first().unwrap().1, 0.5) - 0.5).abs() < 1e-6);

        let conv = roxmltree::Document::parse(r#"<Convolver Wet="0.8" Dry="0.2"/>"#).unwrap();
        let conv_wet = declared_controls(conv.root_element(), "Wet");
        assert!((eval_piecewise(&conv_wet.first().unwrap().1, 0.8) - 0.8).abs() < 1e-6);
    }
}
