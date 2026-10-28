//! A UVI program as the instrument model. Layers and keygroups become groups,
//! each enabled oscillator a zone. A bundle (a `SamplePath` with `*` names, one
//! mono member each) becomes a zone per member, placed by its keygroup's
//! GainMatrix. Scripts run on the engine; the samples they load as the
//! program loads are put on their oscillators here (see [`load`]).

use std::collections::HashMap;
use std::path::PathBuf;

use kontra_format::Notes;
use kontra_ir as ir;
use roxmltree::Node;

mod load;
pub use load::Swap;

/// The samples a program names, looked up in the bank it came from.
pub(crate) trait Library {
    /// The sample `reference` names, relative to the program's folder `program_dir`.
    fn sample(&self, program_dir: &str, reference: &str) -> Result<ir::Sample, String>;
    /// Every Lua member of the bank, by its path: what `require` may load.
    fn modules(&self) -> Vec<(String, Vec<u8>)>;
}

/// Kontakt's streamed head per channel and its grain, the law the other
/// formats use for streamed zones. ponytail: UVI's own preload is not decoded
/// yet; this is the head a streamed UVI group keeps resident.
const PRELOAD_BYTES: u32 = 61_440;
const GRAIN_BYTES: u32 = 6144;

/// Insert effects this crate cannot carry. Each reports as `insert <tag>`.
const EFFECTS: [&str; 7] = [
    "OnePole",
    "DigitalEq",
    "Convolver",
    "EffectRack",
    "ThreeBandShelves",
    "TrackDelay",
    "SampledReverb",
];

pub(crate) fn translate(
    program: Node,
    program_dir: &str,
    library: &impl Library,
    swaps: &dyn Fn(&ir::Script) -> Vec<Swap>,
) -> Result<ir::Instrument, String> {
    let mut t = Translator {
        program,
        program_dir,
        library,
        out: ir::Instrument {
            name: program
                .attribute("DisplayName")
                .or_else(|| program.attribute("Name"))
                .unwrap_or_default()
                .into(),
            voices: num(program, "Polyphony", 64.0)?.clamp(1.0, 1024.0) as u16,
            ..ir::Instrument::DEFAULT
        },
        notes: Notes::default(),
        samples: HashMap::new(),
        lookups: HashMap::new(),
        made: HashMap::new(),
        sent: HashMap::new(),
        script_dirs: Vec::new(),
        base: Base {
            volume_db: db(num(program, "Gain", 1.0)?),
            pan: num(program, "Pan", 0.0)?.clamp(-1.0, 1.0) as f32,
            tune: num(program, "TransposeOctaves", 0.0)? * 12.0
                + num(program, "TransposeSemiTones", 0.0)?,
            streaming: flag(program, "Streaming"),
        },
    };
    t.scripts();
    t.effects();
    t.connections();
    for layer in program.descendants().filter(|n| n.has_tag_name("Layer")) {
        t.layer(layer)?;
    }
    if !t.out.scripts.is_empty() {
        let tree = t.tree();
        for script in &mut t.out.scripts {
            script
                .data
                .push(("__program.lua".into(), tree.clone().into_bytes()));
        }
        t.swap_samples(swaps);
    }
    t.out.diagnostics = t.notes.finish();
    Ok(t.out)
}

/// What a program sets for every group.
struct Base {
    volume_db: f32,
    pan: f32,
    tune: f64,
    streaming: bool,
}

struct Translator<'a, 'input, L> {
    program: Node<'a, 'input>,
    program_dir: &'a str,
    library: &'a L,
    out: ir::Instrument,
    notes: Notes,
    /// Each sample file and offset to its index in `out.samples`.
    samples: HashMap<(PathBuf, u64), usize>,
    /// Each sample reference to what the bank made of it: one header read
    /// per file, however many zones share it.
    lookups: HashMap<String, Result<ir::Sample, String>>,
    base: Base,
    /// The engine group each keygroup made, for scripts.
    made: HashMap<roxmltree::NodeId, usize>,
    /// Each layer's aux send levels (before, after its fader), for scripts.
    sent: HashMap<roxmltree::NodeId, (f64, f64)>,
    /// Each script's folder in the bank, relative to the program's.
    script_dirs: Vec<String>,
}

impl<L: Library> Translator<'_, '_, L> {
    /// A layer's keygroups, by its gain (linear) and pan. Its aux sends add
    /// their level to the layer's: VWinds layers are silent on the main
    /// output (gain 0) and sound through a pre-fader send. ponytail: the aux's
    /// effects are reported, not run.
    fn layer(&mut self, layer: Node) -> Result<(), String> {
        if flag(layer, "Mute") || flag(layer, "Bypass") {
            return Ok(());
        }
        let (pre, post) = self.sends(layer)?;
        self.sent.insert(layer.id(), (pre, post));
        let fader = num(layer, "Gain", 1.0)?;
        let gain = fader + pre + fader * post;
        let pan = num(layer, "Pan", 0.0)?;
        // A zero-gain layer sounds only when a script raises it.
        if gain == 0.0 && self.out.scripts.is_empty() {
            let keygroups = layer.descendants().filter(|n| n.has_tag_name("Keygroup"));
            for _ in keygroups {
                self.notes.add(
                    ir::Impact::Broken,
                    "silent layer (gain 0, played by script)",
                    layer.attribute("DisplayName").unwrap_or("layer"),
                );
            }
            return Ok(());
        }
        let keys = (midi_or(layer, "LowKey", 0), midi_or(layer, "HighKey", 127));
        for keygroup in layer.descendants().filter(|n| n.has_tag_name("Keygroup")) {
            let before = self.out.groups.len();
            self.keygroup(keygroup, (gain, pan), keys)?;
            if self.out.groups.len() > before {
                self.made.insert(keygroup.id(), before);
            }
        }
        Ok(())
    }

    fn keygroup(
        &mut self,
        kg: Node,
        layer: (f64, f64),
        layer_keys: (u8, u8),
    ) -> Result<(), String> {
        if flag(kg, "Bypass") {
            return Ok(());
        }
        let keys = (
            midi_or(kg, "LowKey", 0).max(layer_keys.0),
            midi_or(kg, "HighKey", 127).min(layer_keys.1),
        );
        let velocities = (
            midi_or(kg, "LowVelocity", 1).max(1),
            midi_or(kg, "HighVelocity", 127).max(1),
        );
        if keys.0 > keys.1 || velocities.0 > velocities.1 {
            return Ok(()); // Outside its layer's keys: never plays.
        }
        self.keygroup_effects_and_modes(kg);
        let mut group = ir::Group {
            name: kg.attribute("DisplayName").unwrap_or_default().into(),
            volume_db: db(num(kg, "Gain", 1.0)?) + db(layer.0) + self.base.volume_db,
            pan: (num(kg, "Pan", 0.0)? + layer.1 + f64::from(self.base.pan)).clamp(-1.0, 1.0)
                as f32,
            tune: self.base.tune,
            envelope: self.envelope(kg)?.unwrap_or(ir::Envelope::GATE),
            stream: self.base.streaming.then_some(ir::Stream {
                bytes_per_channel: PRELOAD_BYTES,
                grain: GRAIN_BYTES,
            }),
            ..ir::Group::DEFAULT
        };
        let placement = placement(kg);
        let span = Span {
            keys,
            velocities,
            key_fade: (midi_or(kg, "LowKeyFade", 0), midi_or(kg, "HighKeyFade", 0)),
            velocity_fade: (
                midi_or(kg, "LowVelocityFade", 0),
                midi_or(kg, "HighVelocityFade", 0),
            ),
        };
        let mut first = true;
        // A player's slot is its place among the keygroup's oscillators.
        for (slot, player) in kg
            .descendants()
            .filter(|n| n.has_tag_name("SamplePlayer"))
            .enumerate()
            .filter(|(_, n)| !flag(*n, "Bypass"))
        {
            let slot = u16::try_from(slot).unwrap_or(u16::MAX);
            if first {
                // Note tracking of the first enabled player; mixed tracking is reported.
                group.keytrack = if num(player, "NoteTracking", 1.0)? == 0.0 {
                    0.0
                } else {
                    1.0
                };
                first = false;
            } else if (num(player, "NoteTracking", 1.0)? == 0.0) != (group.keytrack == 0.0) {
                self.notes.add(
                    ir::Impact::Audible,
                    "note tracking differs within a keygroup",
                    "",
                );
            }
            let zones = self.player(player, placement, &span)?;
            group
                .zones
                .extend(zones.into_iter().map(|z| ir::Zone { slot, ..z }));
        }
        self.out.groups.push(group);
        Ok(())
    }

    /// Keygroup modes this crate does not carry, each reported with its value.
    fn keygroup_effects_and_modes(&mut self, kg: Node) {
        for (name, feature) in [
            ("ExclusiveGroup", "exclusive group"),
            ("TriggerRule", "trigger rule"),
            ("TriggerMode", "trigger mode"),
            ("LatchTrigger", "latch trigger"),
        ] {
            let value = kg.attribute(name).unwrap_or("0");
            if value != "0" {
                self.notes.add(ir::Impact::Audible, feature, value);
            }
        }
    }

    /// The zones one oscillator plays: one for a single sample, one per placed
    /// member for a bundle.
    fn player(
        &mut self,
        player: Node,
        placement: Option<Node>,
        span: &Span,
    ) -> Result<Vec<ir::Zone>, String> {
        let reference = player.attribute("SamplePath").unwrap_or_default();
        let parts: Vec<&str> = reference.split('*').collect();
        let members: Vec<(String, (f64, f64))> = match parts.as_slice() {
            [single] => vec![((*single).into(), (1.0, 1.0))],
            [folder, name] => vec![(format!("{folder}{name}"), (1.0, 1.0))],
            [_, _, _] => {
                self.notes
                    .add(ir::Impact::Broken, "bundle of two samples", reference);
                return Ok(Vec::new());
            }
            [folder, names @ ..] => {
                let width = names.len();
                names
                    .iter()
                    .enumerate()
                    .filter_map(|(k, name)| {
                        let (left, right) = match placement {
                            Some(m) => (
                                num(m, &format!("Gain_{}_1", k + 1), 0.0).unwrap_or(0.0),
                                num(m, &format!("Gain_{}_2", k + 1), 0.0).unwrap_or(0.0),
                            ),
                            // ponytail: no placement: an equal spread, not the program's intent.
                            None => (1.0, 1.0),
                        };
                        (left != 0.0 || right != 0.0).then(|| {
                            let spread = if placement.is_none() {
                                1.0 / (width as f64).sqrt()
                            } else {
                                1.0
                            };
                            (format!("{folder}{name}"), (left * spread, right * spread))
                        })
                    })
                    .collect()
            }
            [] => Vec::new(),
        };
        if let Some(start) = player.attribute("SampleStart").filter(|s| s.trim() != "0") {
            self.notes.add(ir::Impact::Audible, "sample start", start);
        }
        if player.children().any(|n| n.has_tag_name("PlaybackOptions")) {
            // ponytail: loop and start points of PlaybackOptions are not carried yet.
            self.notes
                .add(ir::Impact::Audible, "playback options (loops, start)", "");
        }
        let mut zones = Vec::new();
        for (path, (left, right)) in members {
            let sample = match self.sample(&path) {
                Ok(sample) => sample,
                Err(e) => {
                    self.notes.add(ir::Impact::Broken, "missing sample", e);
                    continue;
                }
            };
            let frames = sample.frames;
            let index = self.sample_index(sample);
            let (gain, pan) = placed(left, right);
            zones.push(ir::Zone {
                sample: index,
                keys: span.keys.0..=span.keys.1,
                velocities: span.velocities.0..=span.velocities.1,
                velocity_fade: span.velocity_fade,
                key_fade: span.key_fade,
                root: midi_or(player, "BaseNote", 60),
                tune: num(player, "CoarseTune", 0.0)?
                    + num(player, "FineTune", 0.0)? / 100.0
                    + num(player, "Pitch", 0.0)?,
                volume_db: db(num(player, "Gain", 1.0)? * gain),
                pan: pan as f32,
                start: 0,
                end: frames,
                start_mod: 0,
                loops: Vec::new(),
                reverse: flag(player, "Reverse"),
                slot: 0,
            });
        }
        Ok(zones)
    }

    /// The sample at `path`, read from the bank, and its index in the
    /// instrument. One file and offset is one entry, however many zones use it.
    fn sample(&mut self, path: &str) -> Result<ir::Sample, String> {
        if let Some(found) = self.lookups.get(path) {
            return found.clone();
        }
        let found = self.library.sample(self.program_dir, path);
        self.lookups.insert(path.into(), found.clone());
        found
    }

    fn sample_index(&mut self, sample: ir::Sample) -> usize {
        let key = (sample.file.clone(), sample.offset);
        if let Some(&index) = self.samples.get(&key) {
            return index;
        }
        self.out.samples.push(sample);
        let index = self.out.samples.len() - 1;
        self.samples.insert(key, index);
        index
    }

    /// The amplitude envelope: the DAHDSR a keygroup connects to its gain.
    fn envelope(&mut self, kg: Node) -> Result<Option<ir::Envelope>, String> {
        let Some(connection) = kg.descendants().find(|c| {
            c.has_tag_name("SignalConnection")
                && c.attribute("Destination") == Some("Gain")
                && num(*c, "Ratio", 0.0).is_ok_and(|r| r == 1.0)
                && !flag(*c, "Bypass")
                && source_node(self.program, *c).is_some_and(|n| n.has_tag_name("DAHDSR"))
        }) else {
            return Ok(None);
        };
        let Some(env) = source_node(self.program, connection) else {
            return Ok(None);
        };
        let source = connection.attribute("Source").unwrap_or_default();
        let seconds = |name| num(env, name, 0.0).map(|t| t.clamp(0.0, 30.0) as f32);
        if num(env, "DelayTime", 0.0)? != 0.0 {
            self.notes.add(ir::Impact::Audible, "envelope delay", "");
        }
        if ["AttackCurve", "DecayCurve", "ReleaseCurve"]
            .iter()
            .any(|c| num(env, c, 0.0).is_ok_and(|v| v != 0.0))
        {
            // ponytail: curves are not carried; the stages are linear.
            self.notes
                .add(ir::Impact::Audible, "envelope curve", source);
        }
        Ok(Some(ir::Envelope {
            attack: seconds("AttackTime")?,
            hold: seconds("HoldTime")?,
            decay: seconds("DecayTime")?,
            sustain: num(env, "SustainLevel", 1.0)?.clamp(0.0, 1.0) as f32,
            release: seconds("ReleaseTime")?,
            attack_shape: 0.0,
            fall_shape: 0.0,
        }))
    }

    /// Each active ScriptProcessor, with the bank's Lua members as its
    /// modules. Its other attributes are its widgets' saved values, by name.
    fn scripts(&mut self) {
        let processors: Vec<Node> = self
            .program
            .descendants()
            .filter(|n| n.has_tag_name("ScriptProcessor") && !flag(*n, "Bypass"))
            .collect();
        if processors.is_empty() {
            return;
        }
        let modules = self.library.modules();
        for processor in processors {
            let source = processor
                .children()
                .find(|c| c.has_tag_name("script"))
                .and_then(|c| c.text())
                .unwrap_or_default()
                .trim()
                .to_string();
            let child = |tag: &str| processor.children().find(|c| c.has_tag_name(tag));
            // Widget values: on the processor, tables in its ScriptData.
            let saved = processor
                .attributes()
                .chain(child("ScriptData").into_iter().flat_map(|d| d.attributes()))
                .filter(|a| !matches!(a.name(), "Name" | "Bypass" | "API_version"))
                .map(|a| format!("{}={}", a.name(), a.value()))
                .collect();
            let mut data = modules.clone();
            if let Some(state) = child("state").and_then(|s| s.text()) {
                data.push(("__state.lua".into(), load::lua_literal(state).into_bytes()));
            }
            let script_path = child("Properties").and_then(|p| p.attribute("ScriptPath"));
            self.script_dirs.push(
                script_path
                    .and_then(|p| p.rsplit_once('/'))
                    .map_or(".", |(dir, _)| dir)
                    .into(),
            );
            self.out.scripts.push(ir::Script {
                name: processor.attribute("Name").unwrap_or_default().into(),
                dialect: ir::Dialect::Lua,
                source,
                saved,
                data,
            });
        }
    }

    /// A layer's own aux sends, each at its gain times its aux's: the sum
    /// before the layer's fader, and the sum after it.
    fn sends(&mut self, layer: Node) -> Result<(f64, f64), String> {
        let (mut pre, mut post) = (0.0, 0.0);
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
            let aux = self
                .program
                .children()
                .filter(|n| n.has_tag_name("Auxs"))
                .flat_map(|n| n.children())
                .find(|n| n.attribute("Name") == Some(target) && !flag(*n, "Bypass"));
            let Some(aux) = aux else { continue };
            let level = num(send, "Gain", 1.0)? * num(aux, "Gain", 1.0)?;
            if flag(send, "PreFader") {
                pre += level;
            } else {
                post += level;
            }
        }
        Ok((pre, post))
    }

    /// Active insert effects and aux sends this crate does not carry.
    fn effects(&mut self) {
        for node in self.program.descendants().filter(|n| n.is_element()) {
            if flag(node, "Bypass") {
                continue;
            }
            let tag = node.tag_name().name();
            if EFFECTS.contains(&tag) {
                self.notes.add(
                    ir::Impact::Audible,
                    &format!("insert {tag}"),
                    node.attribute("Name").unwrap_or_default(),
                );
            } else if tag == "Gain" && node.parent().is_some_and(|p| p.has_tag_name("Inserts")) {
                self.notes.add(
                    ir::Impact::Audible,
                    "insert Gain",
                    node.attribute("Name").unwrap_or_default(),
                );
            } else if tag == "BusRouter" && num(node, "Gain", 1.0).is_ok_and(|g| g > 0.0) {
                self.notes.add(
                    ir::Impact::Audible,
                    "aux send",
                    node.attribute("Destination").unwrap_or_default(),
                );
            }
        }
    }

    /// Live modulation (LFOs, scripts, controllers) of gain, pan, pitch and the
    /// matrix. The amplitude envelope is carried by [`Self::envelope`] and is skipped.
    fn connections(&mut self) {
        for connection in self
            .program
            .descendants()
            .filter(|n| n.has_tag_name("SignalConnection") && !flag(*n, "Bypass"))
        {
            let dest = connection.attribute("Destination").unwrap_or_default();
            let source = connection.attribute("Source").unwrap_or_default();
            let envelope =
                source_node(self.program, connection).is_some_and(|n| n.has_tag_name("DAHDSR"));
            if dest == "Gain" && envelope && num(connection, "Ratio", 0.0).is_ok_and(|r| r == 1.0) {
                continue;
            }
            let feature = if dest.starts_with("Gain_") {
                "matrix cell modulation"
            } else {
                "modulation"
            };
            self.notes
                .add(ir::Impact::Audible, &format!("{feature} {dest}"), source);
        }
    }
}

impl<L: Library> Translator<'_, '_, L> {
    /// The program tree as a Lua module for the script runtime: each element's
    /// attributes as `params`, its children by container (`layers`,
    /// `keygroups`, `inserts`, ...), each keygroup's engine group (0-based),
    /// and the program-wide dB and pan every group adds to its layer's and
    /// keygroup's.
    fn tree(&self) -> String {
        let mut out = String::from("return ");
        self.lua_node(self.program, &mut out);
        // Close the program's table with what only it carries.
        out.pop();
        out.push_str(&format!(
            ",db={},pan={}}}",
            self.base.volume_db, self.base.pan
        ));
        out
    }

    fn lua_node(&self, node: Node, out: &mut String) {
        out.push_str("{params={");
        for a in node.attributes() {
            out.push_str(&format!(
                "[{}]={},",
                lua_string(a.name()),
                lua_string(a.value())
            ));
        }
        out.push('}');
        if let Some(group) = self.made.get(&node.id()) {
            out.push_str(&format!(",group={group}"));
        }
        if let Some((pre, post)) = self.sent.get(&node.id()) {
            out.push_str(&format!(",sendPre={pre},sendPost={post}"));
        }
        for container in node.children().filter(Node::is_element) {
            let Some(key) = CONTAINERS
                .iter()
                .find(|(tag, _)| container.has_tag_name(*tag))
                .map(|(_, key)| key)
            else {
                continue;
            };
            out.push_str(&format!(",{key}={{"));
            for child in container.children().filter(Node::is_element) {
                self.lua_node(child, out);
                out.push(',');
            }
            out.push('}');
        }
        out.push('}');
    }
}

/// XML containers as the script API names their lists.
const CONTAINERS: [(&str, &str); 9] = [
    ("Layers", "layers"),
    ("Keygroups", "keygroups"),
    ("Inserts", "inserts"),
    ("Auxs", "auxs"),
    // An effect rack's chains are its auxs to scripts.
    ("Chains", "auxs"),
    ("Oscillators", "oscillators"),
    ("ControlSignalSources", "modulations"),
    ("EventProcessors", "eventProcessors"),
    ("BusRouters", "busRouters"),
];

/// `text` as a Lua string literal.
fn lua_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The key and velocity range a keygroup's oscillators play, and the fades
/// its keygroup gives them.
struct Span {
    keys: (u8, u8),
    velocities: (u8, u8),
    key_fade: (u8, u8),
    velocity_fade: (u8, u8),
}

/// The node a connection's `$Program/` or `$Layer/` source names.
fn source_node<'a, 'input>(
    program: Node<'a, 'input>,
    connection: Node<'a, 'input>,
) -> Option<Node<'a, 'input>> {
    let source = connection.attribute("Source")?;
    let (scope, name) = match source.strip_prefix("$Layer/") {
        Some(name) => (
            connection.ancestors().find(|n| n.has_tag_name("Layer"))?,
            name,
        ),
        None => (program, source.strip_prefix("$Program/")?),
    };
    scope
        .descendants()
        .find(|n| n.attribute("Name") == Some(name))
}

/// A keygroup's GainMatrix: row `k` places member `k` as `[Gain_k_1, Gain_k_2]`.
fn placement<'a, 'input>(kg: Node<'a, 'input>) -> Option<Node<'a, 'input>> {
    kg.children()
        .filter(|n| n.has_tag_name("Inserts"))
        .flat_map(|n| n.children())
        .find(|n| n.has_tag_name("GainMatrix") && !flag(*n, "Bypass"))
}

/// A bundle member's gain (the louder of its two placements) and balance.
fn placed(left: f64, right: f64) -> (f64, f64) {
    let gain = left.max(right);
    let pan = if left + right > 0.0 {
        (right - left) / (right + left)
    } else {
        0.0
    };
    (gain, pan)
}

/// UVI stores linear gains (a program gain of 1.9952623 is +6 dB).
fn db(linear: impl Into<f64>) -> f32 {
    (20.0 * linear.into().max(1e-6).log10()) as f32
}

fn num(node: Node, name: &str, default: f64) -> Result<f64, String> {
    match node.attribute(name) {
        None => Ok(default),
        Some(text) => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("{name}={text:?} is not a number")),
    }
}

fn flag(node: Node, name: &str) -> bool {
    node.attribute(name)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .is_some_and(|v| v != 0.0)
}

/// A MIDI note or velocity attribute, clamped to 0..=127.
fn midi_or(node: Node, name: &str, default: u8) -> u8 {
    node.attribute(name)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .map_or(default, |v| v.round().clamp(0.0, 127.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every name is a sample of its own, 1000 frames long.
    struct Stub;

    impl Library for Stub {
        fn sample(&self, _: &str, reference: &str) -> Result<ir::Sample, String> {
            if reference.contains("missing") {
                return Err(format!("{reference}: not in this bank"));
            }
            Ok(ir::Sample {
                name: reference.into(),
                file: "bank.ufs".into(),
                offset: reference
                    .bytes()
                    .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b))),
                len: None,
                channels: 1,
                frames: 1000,
                rate: 48_000,
                keystream: None,
            })
        }

        fn modules(&self) -> Vec<(String, Vec<u8>)> {
            Vec::new()
        }
    }

    #[test]
    fn keygroups_become_groups_bundles_place_members_and_silent_layers_are_reported() {
        let xml = r#"<Program DisplayName="Synthetic" Gain="1" Polyphony="4">
          <Layers>
            <Layer Gain="1" Pan="0">
              <Keygroups>
                <Keygroup Gain="0.5" Pan="0" LowKey="60" HighKey="62" LowVelocity="1" HighVelocity="127">
                  <Oscillators>
                    <SamplePlayer Bypass="0" BaseNote="61" CoarseTune="1" SamplePath="./a.wav"/>
                    <SamplePlayer Bypass="0" BaseNote="61" SamplePath="./missing.wav"/>
                  </Oscillators>
                </Keygroup>
                <Keygroup Gain="1" Pan="0" LowKey="70" HighKey="70" LowVelocity="1" HighVelocity="127">
                  <Inserts>
                    <GainMatrix Bypass="0" Gain_1_1="1" Gain_1_2="0" Gain_2_1="0" Gain_2_2="1" Gain_3_1="0" Gain_3_2="0"/>
                  </Inserts>
                  <Oscillators>
                    <SamplePlayer Bypass="0" BaseNote="70" SamplePath="./bundle/*l.wav*r.wav*c.wav"/>
                  </Oscillators>
                </Keygroup>
              </Keygroups>
            </Layer>
            <Layer Gain="0" Pan="0">
              <Keygroups>
                <Keygroup LowKey="0" HighKey="127">
                  <Oscillators>
                    <SamplePlayer Bypass="0" BaseNote="60" SamplePath="./silent.wav"/>
                  </Oscillators>
                </Keygroup>
              </Keygroups>
            </Layer>
          </Layers>
        </Program>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
        assert_eq!(out.name, "Synthetic");
        assert_eq!(out.voices, 4);
        // One group per enabled keygroup; the silent layer's keygroup makes none.
        assert_eq!(out.groups.len(), 2);
        let first = &out.groups[0];
        assert!((first.volume_db - (-6.0206)).abs() < 1e-3);
        assert_eq!(first.zones.len(), 1);
        let zone = &first.zones[0];
        assert_eq!(
            (zone.keys.clone(), zone.root, zone.tune),
            (60..=62, 61, 1.0)
        );
        // Bundle: the matrix places L → left, R → right, the third member nowhere.
        let bundle = &out.groups[1];
        assert_eq!(bundle.zones.len(), 2);
        assert!(bundle.zones[0].pan < -0.99 && bundle.zones[1].pan > 0.99);
        // a.wav, then the two placed bundle members; the unplaced and missing ones add none.
        assert_eq!(out.samples.len(), 3);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.feature.starts_with("silent layer"))
        );
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.feature == "missing sample")
        );
    }
}
