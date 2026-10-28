//! The program's mixer as buses. Bus 0 is the main output, then one bus per
//! layer at its fader, then one per aux with its effect chain, gain and pan.
//! A layer feeds the auxes it sends to, before or after its fader; every bus
//! ends on the main output ahead of the program's own inserts, as an aux set
//! `PreInsert` does. Of the chains' effects, Gain and Convolver are carried;
//! the rest pass their input and are reported (see `effects`).

use kontra_ir as ir;
use roxmltree::Node;

use super::{Library, Translator, db, flag, num};

/// Whether `node` is an effect in a chain this module carries: the program's
/// inserts or an aux's.
pub(super) fn carried(node: Node) -> bool {
    let chain = node
        .parent()
        .filter(|p| p.has_tag_name("Inserts"))
        .and_then(|p| p.parent());
    chain.is_some_and(|c| {
        c.has_tag_name("Program")
            || (c.has_tag_name("AuxEffect") && c.parent().is_some_and(|p| p.has_tag_name("Auxs")))
    }) && (node.has_tag_name("Gain") || node.has_tag_name("Convolver"))
}

fn auxes<'a, 'input>(program: Node<'a, 'input>) -> impl Iterator<Item = Node<'a, 'input>> {
    program
        .children()
        .filter(|n| n.has_tag_name("Auxs"))
        .flat_map(|n| n.children())
        .filter(|n| n.has_tag_name("AuxEffect"))
}

impl<L: Library> Translator<'_, '_, L> {
    /// Every bus, the program's inserts, and each layer's and aux's bus for
    /// [`Self::layer`] and scripts. Runs before the layers.
    pub(super) fn buses(&mut self) -> Result<(), String> {
        let program = self.program;
        let layers: Vec<Node> = program
            .descendants()
            .filter(|n| n.has_tag_name("Layer"))
            .collect();
        let first_aux = 1 + layers.len();
        self.out.buses.push(bus("Main", 0.0, 0.0));
        for layer in layers {
            self.bus_of.insert(layer.id(), self.out.buses.len());
            let mut b = bus(
                layer.attribute("DisplayName").unwrap_or_default(),
                db(num(layer, "Gain", 1.0)?),
                0.0,
            );
            b.sends = self.sends_of(layer, first_aux)?;
            self.out.buses.push(b);
        }
        for aux in auxes(program) {
            self.bus_of.insert(aux.id(), self.out.buses.len());
            let name = aux.attribute("DisplayName").unwrap_or_default();
            // A bypassed aux takes no sends.
            if flag(aux, "Bypass") {
                self.out.buses.push(bus(name, 0.0, 0.0));
                continue;
            }
            if !flag(aux, "PreInsert") {
                self.notes
                    .add(ir::Impact::Audible, "aux after the program's inserts", name);
            }
            let mut b = bus(
                name,
                db(num(aux, "Gain", 1.0)?),
                num(aux, "Pan", 0.0)?.clamp(-1.0, 1.0) as f32,
            );
            b.inserts = self.chain(aux)?;
            self.out.buses.push(b);
        }
        self.out.inserts = self.chain(program)?;
        Ok(())
    }

    /// A layer's active sends to active auxes.
    fn sends_of(&self, layer: Node, first_aux: usize) -> Result<Vec<ir::BusSend>, String> {
        let mut sends = Vec::new();
        for send in layer
            .children()
            .filter(|n| n.has_tag_name("BusRouters"))
            .flat_map(|n| n.children())
            .filter(|n| n.has_tag_name("BusRouter") && !flag(*n, "Bypass"))
        {
            let target = send
                .attribute("Destination")
                .and_then(|d| d.rsplit('/').next())
                .unwrap_or_default();
            let found = auxes(self.program)
                .enumerate()
                .find(|(_, n)| n.attribute("Name") == Some(target));
            let Some((at, _)) = found.filter(|(_, n)| !flag(*n, "Bypass")) else {
                continue;
            };
            sends.push(ir::BusSend {
                bus: first_aux + at,
                level: num(send, "Gain", 1.0)? as f32,
                post_fader: !flag(send, "PreFader"),
            });
        }
        Ok(sends)
    }

    /// The `Inserts` of `owner` by slot; an effect not carried is an empty
    /// slot (its input passes).
    fn chain(&mut self, owner: Node) -> Result<Vec<Option<ir::Effect>>, String> {
        let mut slots = Vec::new();
        for node in owner
            .children()
            .filter(|n| n.has_tag_name("Inserts"))
            .flat_map(|n| n.children())
            .filter(Node::is_element)
        {
            let effect = if flag(node, "Bypass") {
                None
            } else if node.has_tag_name("Gain") {
                Some(effect(
                    ir::EffectKind::Gain(num(node, "Volume", 1.0)? as f32),
                    1.0,
                    0.0,
                ))
            } else if node.has_tag_name("Convolver") {
                self.impulse(node)?.map(|impulse| {
                    let wet = num(node, "Wet", 1.0).unwrap_or(1.0) as f32;
                    let dry = num(node, "Dry", 0.0).unwrap_or(0.0) as f32;
                    effect(ir::EffectKind::Convolution { impulse }, wet, dry)
                })
            } else {
                None
            };
            slots.push(effect);
        }
        Ok(slots)
    }

    /// A Convolver's response, `None` (reported) when its file is missing.
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
        // ponytail: power normalization's law is not read yet; the response
        // plays at its stored level.
        if flag(node, "NormalizePower") {
            self.notes
                .add(ir::Impact::Audible, "convolver power normalization", path);
        }
        let sample = self.sample_index(sample);
        self.out.impulses.push(ir::Impulse {
            sample,
            reverse: false,
            normalize: None,
            predelay_ms: 0.0,
            cuts: None,
            envelope: None,
        });
        Ok(u16::try_from(self.out.impulses.len() - 1).ok())
    }
}

fn bus(name: &str, volume_db: f32, pan: f32) -> ir::Bus {
    ir::Bus {
        name: name.into(),
        volume_db,
        pan,
        inserts: Vec::new(),
        sends: Vec::new(),
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
