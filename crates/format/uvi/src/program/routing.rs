//! The program's mixer as buses. Bus 0 is the main output; each layer and
//! each aux is a strip of buses: its voices or sends enter the first, its
//! inserts run in order, and its fader and pan sit on the last, which feeds
//! the auxes a layer sends to (before or after the fader). An active
//! EffectRack ends a bus: it feeds one bus per chain at the chain's gain,
//! and the chains feed a bus where the rest of the inserts run. Every strip
//! ends on the main output ahead of the program's own inserts, as an aux set
//! `PreInsert` does. Of the effects, Gain and Convolver are carried; the
//! rest pass their input and are reported (see `effects`).

use kontra_ir as ir;
use roxmltree::Node;

use super::{Library, Translator, db, flag, num};

/// Whether `node` is an effect this module carries: in the program's
/// inserts, or in a layer's, an aux's or a rack chain's.
pub(super) fn carried(node: Node) -> bool {
    let Some(owner) = node
        .parent()
        .filter(|p| p.has_tag_name("Inserts"))
        .and_then(|p| p.parent())
    else {
        return false;
    };
    let laid = owner.has_tag_name("Layer") || owner.has_tag_name("AuxEffect");
    let kind = ["Gain", "Convolver", "SampledReverb"]
        .iter()
        .any(|t| node.has_tag_name(*t));
    let in_keygroup = node.ancestors().any(|a| a.has_tag_name("Keygroup"));
    !in_keygroup
        && ((laid && (kind || node.has_tag_name("EffectRack")))
            || (owner.has_tag_name("Program") && kind))
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
        self.out.buses.push(ir::Bus {
            to_main: true,
            ..bus("Main")
        });
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
        for node in children(program, "Inserts", "") {
            let slot = self.slot(node)?;
            self.out.inserts.push(slot);
        }
        Ok(())
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
            if !node.has_tag_name("EffectRack") || flag(node, "Bypass") {
                let slot = self.slot(node)?;
                if let Some(b) = self.out.buses.get_mut(at) {
                    b.inserts.push(slot);
                }
                continue;
            }
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
            // The chains' gains are their faders: scripts move them.
            at = self.push(node);
            for end in ends {
                self.feed(end, at, 1.0, true);
            }
        }
        Ok(at)
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
                let level = num(send, "Gain", 1.0)?;
                self.feed(from, to, level, !flag(send, "PreFader"));
            }
        }
        Ok(())
    }

    /// One insert as a slot; one not carried is empty (its input passes).
    fn slot(&mut self, node: Node) -> Result<Option<ir::Effect>, String> {
        Ok(if flag(node, "Bypass") {
            None
        } else if node.has_tag_name("Gain") {
            let kind = ir::EffectKind::Gain(num(node, "Volume", 1.0)? as f32);
            Some(effect(kind, 1.0, 0.0))
        } else if node.has_tag_name("Convolver") || node.has_tag_name("SampledReverb") {
            let wet = num(node, "Wet", 1.0)? as f32;
            let dry = num(node, "Dry", 0.0)? as f32;
            self.impulse(node)?
                .map(|impulse| effect(ir::EffectKind::Convolution { impulse }, wet, dry))
        } else if let Some(dry) = wet_only(node)? {
            // ponytail: an effect with a wet path not carried yet plays only
            // its dry level; passing its input instead would be louder.
            Some(effect(ir::EffectKind::OtherWet, 1.0, dry))
        } else {
            None
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
        // ponytail: power normalization's law and a sampled reverb's time
        // and damping are not read yet; the response plays as stored.
        if flag(node, "NormalizePower") {
            self.notes
                .add(ir::Impact::Audible, "convolver power normalization", path);
        }
        let reverb = node.has_tag_name("SampledReverb");
        for (name, unset) in [("Time", 1.0), ("DampingLow", 0.0), ("DampingHigh", 0.0)] {
            if reverb && num(node, name, unset)? != unset {
                let feature = format!("sampled reverb {name}");
                self.notes.add(ir::Impact::Audible, &feature, path);
            }
        }
        let sample = self.sample_index(sample);
        self.out.impulses.push(ir::Impulse {
            sample,
            reverse: false,
            normalize: None,
            predelay_ms: if reverb {
                num(node, "PreDelay", 0.0)?.max(0.0) as f32
            } else {
                0.0
            },
            cuts: None,
            envelope: None,
        });
        Ok(u16::try_from(self.out.impulses.len() - 1).ok())
    }
}

/// The dry level of an effect whose output is mostly a wet path (a reverb,
/// a delay); `None` for one that processes in place.
fn wet_only(node: Node) -> Result<Option<f32>, String> {
    let tag = node.tag_name().name();
    Ok(match tag {
        "SampledReverb" | "Convolver" => Some(num(node, "Dry", 0.0)? as f32),
        "SparkVerb" | "DualDelay" | "FeedbackMachine" | "Flanger" => {
            Some(1.0 - num(node, "Mix", 1.0)?.clamp(0.0, 1.0) as f32)
        }
        _ => None,
    })
}

/// A bus that only feeds its sends until [`Translator::strip`] gives it a
/// fader on the main output.
fn bus(name: &str) -> ir::Bus {
    ir::Bus {
        name: name.into(),
        volume_db: 0.0,
        pan: 0.0,
        inserts: Vec::new(),
        sends: Vec::new(),
        to_main: false,
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
