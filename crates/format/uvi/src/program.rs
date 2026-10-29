//! A UVI program as the instrument model. Layers and keygroups become groups,
//! each enabled oscillator a zone. A bundle (a `SamplePath` with `*` names)
//! becomes a zone per member, its channels mixed by the keygroup's
//! GainMatrix. Scripts run on the engine; the samples they load as the
//! program loads are put on their oscillators here (see [`load`]).

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;

use kontra_format::Notes;
use kontra_ir as ir;
use roxmltree::Node;

mod blend;
mod delay;
mod digital_eq;
mod filters;
mod gains;
mod lfo_routes;
mod load;
mod matrix;
mod modulation;
mod reverb;
mod routing;
mod tree;
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

#[cfg(test)]
pub(crate) fn translate(
    program: Node,
    program_dir: &str,
    library: &impl Library,
    swaps: &dyn Fn(&ir::Script) -> Vec<Swap>,
) -> Result<ir::Instrument, String> {
    Ok(unfinished(program, program_dir, library)?.finish(swaps))
}

/// The program translated up to its scripts' load pass, which
/// [`Unfinished::finish`] runs once the caller has dropped the document.
pub(crate) fn unfinished<'a, L: Library>(
    program: Node,
    program_dir: &'a str,
    library: &'a L,
) -> Result<load::Unfinished<'a, L>, String> {
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
            voices: routing::voices(program)?,
            steal_whole_groups: true,
            output: ir::Output {
                controllers: Some(routing::PART_CONTROLLERS),
                balance: routing::PART_BALANCE,
                ..ir::Output::UNITY
            },
            ..ir::Instrument::DEFAULT
        },
        notes: Notes::default(),
        samples: HashMap::new(),
        lookups: HashMap::new(),
        made: HashMap::new(),
        bus_of: HashMap::new(),
        entry: HashMap::new(),
        script_dirs: Vec::new(),
        carried: HashSet::new(),
        routed: HashMap::new(),
        units: HashMap::new(),
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
    t.buses()?;
    for layer in program.descendants().filter(|n| n.has_tag_name("Layer")) {
        t.layer(layer)?;
    }
    t.script_routes();
    // After the layers: they mark the matrix connections they carry.
    t.connections();
    if !t.out.scripts.is_empty() {
        let tree = t.tree();
        for script in &mut t.out.scripts {
            script
                .data
                .push(("__program.lua".into(), tree.clone().into_bytes()));
        }
    }
    Ok(load::Unfinished {
        program_dir,
        library,
        out: t.out,
        notes: t.notes,
        samples: t.samples,
        lookups: t.lookups,
        script_dirs: t.script_dirs,
    })
}

/// The sample `path` names, one bank read per reference.
fn lookup(
    lookups: &mut HashMap<String, Result<ir::Sample, String>>,
    library: &impl Library,
    program_dir: &str,
    path: &str,
) -> Result<ir::Sample, String> {
    lookups
        .entry(path.into())
        .or_insert_with(|| library.sample(program_dir, path))
        .clone()
}

/// `sample`'s index in `out`, one entry per file and offset.
fn sample_entry(
    index: &mut HashMap<(PathBuf, u64), usize>,
    out: &mut Vec<ir::Sample>,
    sample: ir::Sample,
) -> usize {
    *index
        .entry((sample.file.clone(), sample.offset))
        .or_insert_with(|| {
            out.push(sample);
            out.len() - 1
        })
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
    /// Each layer's and aux's fader bus (routing.rs).
    bus_of: HashMap<roxmltree::NodeId, usize>,
    /// Each layer's first bus, where its voices play.
    entry: HashMap<roxmltree::NodeId, usize>,
    /// Each script's folder in the bank, relative to the program's.
    script_dirs: Vec<String>,
    /// Matrix cell connections carried, held or live (matrix.rs).
    carried: HashSet<roxmltree::NodeId>,
    /// Each connection a route carries, to the route's key, for scripts.
    routed: HashMap<roxmltree::NodeId, usize>,
    /// Each route key's amount per unit of ratio.
    units: HashMap<usize, f32>,
}

impl<L: Library> Translator<'_, '_, L> {
    /// A layer's keygroups, by its pan, on its bus (routing.rs: the fader
    /// and sends). VWinds layers are silent on the main output (gain 0) and
    /// sound through a pre-fader send.
    fn layer(&mut self, layer: Node) -> Result<(), String> {
        if flag(layer, "Mute") || flag(layer, "Bypass") {
            return Ok(());
        }
        let bus = self.entry.get(&layer.id()).copied().unwrap_or(0);
        let end = self.bus_of.get(&layer.id()).copied().unwrap_or(0);
        let fader = num(layer, "Gain", 1.0)?;
        let feeds = self.out.buses.get(end).is_some_and(|b| {
            b.sends
                .iter()
                .any(|s| s.level > 0.0 && (!s.post_fader || fader > 0.0))
        });
        let pan = num(layer, "Pan", 0.0)?;
        // A zero-gain layer sounds only when a script raises it.
        if fader == 0.0 && !feeds && self.out.scripts.is_empty() {
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
            self.keygroup(keygroup, (1.0, pan), keys)?;
            if let Some(group) = self.out.groups.get_mut(before) {
                group.bus = bus;
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
            inserts: self.voice_slots(kg)?,
            start: routing::cycle(kg).into_iter().collect(),
            ..ir::Group::DEFAULT
        };
        // Per-voice gain and shelves, else a one-pole filter, slot by slot.
        let filters = filters::keygroup(kg);
        group.inserts = (0..group.inserts.len().max(filters.len()))
            .map(|n| {
                let own = group.inserts.get(n).cloned().flatten();
                own.or_else(|| filters.get(n).cloned().flatten())
            })
            .collect();
        let program = self.program;
        self.cutoff_routes(&mut group, kg);
        let pans = kg
            .children()
            .filter(|c| c.has_tag_name("Connections"))
            .flat_map(|c| c.children())
            .filter_map(|c| Some((c, modulation::route(program, c, PAN)?)))
            .collect::<Vec<_>>();
        for (c, r) in pans {
            self.add_route(&mut group, c, r, |source, amount| ir::ModKind::Pan {
                source,
                amount,
            });
        }
        self.gain_routes(&mut group, kg);
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
        // A player's slot is its place among the keygroup's oscillators, of
        // any kind, as scripts index them.
        for (slot, player) in kg
            .children()
            .filter(|n| n.has_tag_name("Oscillators"))
            .flat_map(|o| o.children().filter(Node::is_element))
            .enumerate()
            .filter(|(_, n)| n.has_tag_name("SamplePlayer") && !flag(*n, "Bypass"))
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
            // ponytail: an oscillator's pitch routes move its whole
            // keygroup; split groups when a keygroup's players differ.
            let routes = player
                .children()
                .filter(|c| c.has_tag_name("Connections"))
                .flat_map(|c| c.children())
                .filter_map(|c| Some((c, modulation::route(program, c, PITCH)?)))
                .collect::<Vec<_>>();
            for (c, r) in routes {
                self.add_route(&mut group, c, r, |source, semitones| ir::ModKind::Pitch {
                    source,
                    semitones,
                    depth: 1.0,
                    tau_ms: 0.0,
                });
            }
            self.player(player, placement, &span, &mut group, slot)?;
        }
        self.out.groups.push(group);
        Ok(())
    }

    /// `r`, from `connection`, as a route of `kind` on `group`, its modulator
    /// copied onto the group. The route is named `#key` (scripts set its
    /// amount, [`tree`](Self::tree)); a connection alike in kind and unit
    /// joins it. A ratio of 0 makes one only in a scripted program.
    /// ponytail: joined connections share one amount; split them if a
    /// script sets two players of a keygroup apart.
    pub(super) fn add_route(
        &mut self,
        group: &mut ir::Group,
        connection: Node,
        r: modulation::Route,
        kind: impl Fn(ir::Source, f32) -> ir::ModKind,
    ) {
        if r.ratio == 0.0 && self.out.scripts.is_empty() {
            return;
        }
        self.carried.extend(blend::carries(connection, &r));
        let source = modulation::group_source(group, r.source, r.timed);
        if connection.attribute("Destination") != Some("Depth") {
            self.depth_scales(group, connection, source);
        }
        let kind = kind(source, r.unit * r.ratio);
        let alike = group.mods.iter().find_map(|m| {
            let key = m.name.strip_prefix('#')?.parse().ok()?;
            (m.kind == kind && self.units.get(&key) == Some(&r.unit)).then_some(key)
        });
        let key = alike.unwrap_or_else(|| {
            let key = connection.id().get_usize();
            self.units.insert(key, r.unit);
            group.mods.push(ir::Mod {
                name: format!("#{key}"),
                slot: 0,
                target: 0,
                bypass: false,
                kind,
            });
            key
        });
        self.routed.insert(connection.id(), key);
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
            if value != "0" && (name != "TriggerRule" || routing::cycle(kg).is_none()) {
                self.notes.add(ir::Impact::Audible, feature, value);
            }
        }
    }

    /// The zones one oscillator plays, one per sample. A bundle
    /// (`folder*a*b…`) concatenates its members' channels in order, and the
    /// keygroup's GainMatrix (`Gain_<channel>_<output>`) mixes them to the
    /// left and right outputs; without one, the first two channels are left
    /// and right.
    fn player(
        &mut self,
        player: Node,
        placement: Option<Node>,
        span: &Span,
        group: &mut ir::Group,
        slot: u16,
    ) -> Result<(), String> {
        let reference = player.attribute("SamplePath").unwrap_or_default();
        let members: Vec<String> = match reference.split('*').collect::<Vec<_>>().as_slice() {
            [single] => vec![(*single).into()],
            [folder, names @ ..] => names.iter().map(|n| format!("{folder}{n}")).collect(),
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
        let own = self.own_gains(player, group);
        // The bundle's channel the next member starts at.
        let mut first = 0;
        for path in members {
            let sample = match self.sample(&path) {
                Ok(sample) => sample,
                Err(e) => {
                    self.notes.add(ir::Impact::Broken, "missing sample", e);
                    continue;
                }
            };
            let (frames, channels) = (sample.frames, usize::from(sample.channels));
            let channels = first..first + channels;
            first = channels.end;
            let mut routes = Vec::new();
            let width = channels.len();
            let mut mix = match placement {
                Some(m) => self.mix(m, channels, group, &mut routes)?,
                // Unmixed outputs past the second are not heard.
                None if channels.start >= 2 => continue,
                None if channels.start == 1 => vec![[0.0, 1.0]],
                None => Vec::new(),
            };
            if !mix.is_empty() && mix.iter().flatten().all(|g| *g == 0.0) {
                continue;
            }
            let index = self.sample_index(sample);
            let member = u16::try_from(group.zones.len()).unwrap_or(u16::MAX);
            group
                .mods
                .extend(gains::own_mix(&own, &mut mix, width, member));
            for (source, row, side) in routes {
                group.mods.push(ir::Mod {
                    name: String::new(),
                    slot: 0,
                    target: 0,
                    bypass: false,
                    kind: ir::ModKind::Mix {
                        source,
                        zone: member,
                        row,
                        side,
                    },
                });
            }
            group.zones.push(ir::Zone {
                mix,
                sample: index,
                keys: span.keys.0..=span.keys.1,
                velocities: span.velocities.0..=span.velocities.1,
                velocity_fade: span.velocity_fade,
                key_fade: span.key_fade,
                root: midi_or(player, "BaseNote", 60),
                tune: num(player, "CoarseTune", 0.0)?
                    + num(player, "FineTune", 0.0)? / 100.0
                    + num(player, "Pitch", 0.0)?,
                volume_db: db(num(player, "Gain", 1.0)?),
                pan: 0.0,
                start: 0,
                end: frames,
                start_mod: 0,
                loops: Vec::new(),
                reverse: flag(player, "Reverse"),
                slot,
            });
        }
        // Rows past the bundle carry no signal: what moves them is moot.
        for connection in placement.into_iter().flat_map(|m| m.descendants()) {
            let row = connection.attribute("Destination").and_then(|d| {
                d.strip_prefix("Gain_")?
                    .split_once('_')?
                    .0
                    .parse::<usize>()
                    .ok()
            });
            if row.is_some_and(|r| r > first) {
                self.carried.insert(connection.id());
            }
        }
        Ok(())
    }

    /// The GainMatrix `m`'s left and right rows for bundle `channels`. A
    /// cell a macro moves live keeps its saved gain and gets a route
    /// (`routes`: source, the zone's channel and output side, [`matrix::live`]); one a macro
    /// holds is scaled to where it holds it ([`matrix::held`]). An output
    /// past the second is reported.
    fn mix(
        &mut self,
        m: Node,
        channels: Range<usize>,
        group: &mut ir::Group,
        routes: &mut Vec<(ir::Source, u8, u8)>,
    ) -> Result<Vec<[f32; 2]>, String> {
        let mut rows = Vec::new();
        let first = channels.start;
        for c in channels {
            let at = u8::try_from(c - first).unwrap_or(u8::MAX);
            let gain = |out: usize| num(m, &format!("Gain_{}_{out}", c + 1), 0.0);
            let mut row = [0.0; 2];
            for (side, cell) in (0u8..).zip(row.iter_mut()) {
                let name = format!("Gain_{}_{}", c + 1, side + 1);
                *cell = gain(usize::from(side) + 1)? as f32;
                let on_cell = m.descendants().filter(|n| {
                    n.has_tag_name("SignalConnection")
                        && n.attribute("Destination") == Some(name.as_str())
                });
                for connection in on_cell {
                    if let Some(f) = matrix::held(self.program, connection, false) {
                        *cell *= f;
                    } else if let Some(source) = matrix::live(self.program, connection, group) {
                        routes.push((source, at, side));
                        // The macro's inputs ride along in the route.
                        let inputs = source_node(self.program, connection)
                            .into_iter()
                            .flat_map(|n| n.descendants())
                            .filter(|n| n.has_tag_name("SignalConnection"));
                        self.carried.extend(inputs.map(|n| n.id()));
                    } else if let Some(f) = matrix::held(self.program, connection, true) {
                        // Its macro's inputs are not all carried.
                        *cell *= f;
                        let source = connection.attribute("Source").unwrap_or_default();
                        self.notes.add(
                            ir::Impact::Audible,
                            "matrix macro held at its value",
                            source,
                        );
                    } else {
                        continue;
                    }
                    self.carried.insert(connection.id());
                }
            }
            rows.push(row);
            for out in 3..=12 {
                if gain(out)? != 0.0 {
                    self.notes
                        .add(ir::Impact::Audible, "keygroup output past stereo", "");
                }
            }
        }
        Ok(rows)
    }

    /// The sample at `path`, read from the bank, and its index in the
    /// instrument. One file and offset is one entry, however many zones use it.
    fn sample(&mut self, path: &str) -> Result<ir::Sample, String> {
        lookup(&mut self.lookups, self.library, self.program_dir, path)
    }

    fn sample_index(&mut self, sample: ir::Sample) -> usize {
        sample_entry(&mut self.samples, &mut self.out.samples, sample)
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
                links: Vec::new(),
            });
        }
    }

    /// Active insert effects and aux sends this crate does not carry.
    fn effects(&mut self) {
        for node in self.program.descendants().filter(|n| n.is_element()) {
            if flag(node, "Bypass") {
                continue;
            }
            let tag = node.tag_name().name();
            let parent = node.parent();
            // A keygroup's GainMatrix places its bundle members (`placement`).
            let placing = tag == "GainMatrix"
                && parent
                    .and_then(|p| p.parent())
                    .is_some_and(|p| p.has_tag_name("Keygroup"));
            if parent.is_some_and(|p| p.has_tag_name("Inserts"))
                && !routing::carried(node)
                && !filters::carried(node)
                && !placing
            {
                self.notes.add(
                    routing::impact(self.program, node),
                    &format!("insert {tag}"),
                    node.attribute("Name").unwrap_or_default(),
                );
            } else if tag == "BusRouter"
                && !parent
                    .and_then(|p| p.parent())
                    .is_some_and(|p| p.has_tag_name("Layer"))
                && num(node, "Gain", 1.0).is_ok_and(|g| g > 0.0)
            {
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
            .filter(|n| n.has_tag_name("SignalConnection"))
        {
            // What moves a bypassed module (or through a bypassed connection) is moot.
            if connection.ancestors().any(|a| flag(a, "Bypass")) {
                continue;
            }
            // At ratio 0 a connection moves nothing: what modulates its
            // ratio scales it (matrix.rs), so 0 stays 0.
            if num(connection, "Ratio", 1.0).is_ok_and(|r| r == 0.0) {
                continue;
            }
            if modulation::script_route(self.program, connection).is_some()
                || self.routed.contains_key(&connection.id())
                || modulation::gain_route(self.program, connection).is_some()
                || [PITCH, PAN]
                    .into_iter()
                    .any(|to| modulation::route(self.program, connection, to).is_some())
            {
                continue;
            }
            let dest = connection.attribute("Destination").unwrap_or_default();
            let source = connection.attribute("Source").unwrap_or_default();
            let envelope =
                source_node(self.program, connection).is_some_and(|n| n.has_tag_name("DAHDSR"));
            if dest == "Gain" && envelope && num(connection, "Ratio", 0.0).is_ok_and(|r| r == 1.0) {
                continue;
            }
            if self.carried.contains(&connection.id()) {
                continue;
            }
            let feature = if dest.starts_with("Gain_") {
                "matrix cell modulation"
            } else {
                "modulation"
            };
            let impact = routing::impact(self.program, connection);
            self.notes.add(impact, &format!("{feature} {dest}"), source);
        }
    }
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

/// The owners and destinations [`modulation::route`] carries.
const PITCH: (&str, &str) = ("SamplePlayer", "Pitch");
const PAN: (&str, &str) = ("Keygroup", "Pan");

/// `item`'s index in `items`, pushed if absent.
fn index_of<T: PartialEq>(items: &mut Vec<T>, item: T) -> u8 {
    let n = items.iter().position(|i| *i == item).unwrap_or_else(|| {
        items.push(item);
        items.len() - 1
    });
    u8::try_from(n).unwrap_or(u8::MAX)
}

/// A keygroup's GainMatrix: row `k` places member `k` as `[Gain_k_1, Gain_k_2]`.
fn placement<'a, 'input>(kg: Node<'a, 'input>) -> Option<Node<'a, 'input>> {
    kg.children()
        .filter(|n| n.has_tag_name("Inserts"))
        .flat_map(|n| n.children())
        .find(|n| n.has_tag_name("GainMatrix") && !flag(*n, "Bypass"))
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
mod tests;
