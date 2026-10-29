//! Regions into the instrument model: one zone per region, groups by their
//! group-level settings, samples by file. What the IR cannot carry is a
//! diagnostic (06-FORMATS.md).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use kontra_ir as ir;

use crate::laws;
use crate::parse::{Document, Opcodes};

/// Opcodes this translator maps. Any other is reported as unsupported.
const MAPPED: &[&str] = &[
    "sample",
    "key",
    "lokey",
    "hikey",
    "xfin_lokey",
    "xfout_hikey",
    "lovel",
    "hivel",
    "xfin_lovel",
    "xfout_hivel",
    "pitch_keycenter",
    "pitch_keytrack",
    "tune",
    "transpose",
    "volume",
    "amplitude",
    "pan",
    "offset",
    "end",
    "loop_mode",
    "loopmode",
    "loop_start",
    "loopstart",
    "loop_end",
    "loopend",
    "trigger",
    "seq_length",
    "seq_position",
    "group",
    "off_by",
    "amp_veltrack",
    "direction",
    "cutoff",
    "resonance",
    "fil_type",
    "default_path",
    "ampeg_attack",
    "ampeg_hold",
    "ampeg_decay",
    "ampeg_sustain",
    "ampeg_release",
];

fn mapped(name: &str) -> bool {
    MAPPED.contains(&name) || cc_number(name, "locc").is_some() || cc_number(name, "hicc").is_some()
}

/// The controller of `locc5` or `hicc5`.
fn cc_number(name: &str, prefix: &str) -> Option<u8> {
    name.strip_prefix(prefix)?
        .parse()
        .ok()
        .filter(|cc: &u8| *cc <= 127)
}

/// Reads the parsed document into an instrument. Sample paths resolve from
/// `base`, then `default_path`.
pub(crate) fn instrument(doc: &Document, base: &Path) -> ir::Instrument {
    let mut build = Build::default();
    for (feature, detail) in &doc.problems {
        build.notes.add(ir::Impact::Audible, feature, detail);
    }
    unmapped(&mut build.notes, &doc.control, "control");
    let default_path = normalize(doc.control.get("default_path").map_or("", String::as_str));
    let dir = base.join(default_path);
    for region in &doc.regions {
        build.region(region, &dir);
    }
    build.finish()
}

/// A group under construction: its SFZ number, its `off_by` number, the
/// settings its regions share, and its zones.
struct Entry {
    number: u32,
    off_by: Option<u32>,
    settings: ir::Group,
    zones: Vec<ir::Zone>,
}

#[derive(Default)]
struct Build {
    samples: Vec<ir::Sample>,
    sample_index: HashMap<PathBuf, usize>,
    entries: Vec<Entry>,
    notes: Notes,
}

impl Build {
    fn region(&mut self, ops: &Opcodes, dir: &Path) {
        let sample = raw(ops, &["sample"]).unwrap_or("*sine");
        if sample == "*silence" {
            self.notes.add(ir::Impact::Cosmetic, "silence region", "");
            return;
        }
        if sample.starts_with('*') {
            self.notes
                .add(ir::Impact::Audible, "built-in sample", sample);
            return;
        }
        let (sample, total) = self.sample(&normalize(sample), dir);

        let key = self.read(ops, &["key"], laws::midi_key);
        let lo = self
            .read(ops, &["lokey"], laws::midi_key)
            .or(key)
            .unwrap_or(0);
        let hi = self
            .read(ops, &["hikey"], laws::midi_key)
            .or(key)
            .unwrap_or(127);
        let xfin = self
            .read(ops, &["xfin_lokey"], laws::midi_key)
            .unwrap_or(lo);
        let xfout = self
            .read(ops, &["xfout_hikey"], laws::midi_key)
            .unwrap_or(hi);
        let (keys, key_fade) = laws::span(lo, hi, xfin, xfout);

        let vlo = self.number(ops, &["lovel"]).map_or(0, midi);
        let vhi = self.number(ops, &["hivel"]).map_or(127, midi);
        let vxin = self.number(ops, &["xfin_lovel"]).map_or(vlo, midi);
        let vxout = self.number(ops, &["xfout_hivel"]).map_or(vhi, midi);
        let (velocities, velocity_fade) = laws::span(vlo, vhi, vxin, vxout);

        let start = self.number(ops, &["offset"]).map_or(0, frames).min(total);
        let end = self
            .number(ops, &["end"])
            // -1 is a region that triggers but plays nothing (it can still
            // choke its off_by targets); end is otherwise inclusive.
            .map_or(total, |end| {
                if end == -1.0 {
                    0
                } else {
                    frames(end).saturating_add(1)
                }
            })
            .min(total);
        let zone = ir::Zone {
            mix: Vec::new(),
            sample,
            keys,
            velocities,
            velocity_fade,
            key_fade,
            root: self
                .read(ops, &["pitch_keycenter"], laws::midi_key)
                .or(key)
                .unwrap_or(60),
            tune: self.number(ops, &["transpose"]).unwrap_or(0.0)
                + self.number(ops, &["tune"]).unwrap_or(0.0) / 100.0,
            volume_db: self.number(ops, &["volume"]).unwrap_or(0.0) as f32
                + self
                    .number(ops, &["amplitude"])
                    .map_or(0.0, laws::amplitude_db),
            pan: self.number(ops, &["pan"]).map_or(0.0, laws::pan),
            start: start.min(end),
            end,
            start_mod: 0,
            loops: self.looped(ops, total).into_iter().collect(),
            reverse: raw(ops, &["direction"]) == Some("reverse"),
            slot: 0,
        };

        let settings = self.settings(ops);
        let number = self.number(ops, &["group"]).map_or(0, whole);
        let off_by = self.number(ops, &["off_by"]).map(whole);
        let index = self.entry(number, off_by, settings);
        if let Some(entry) = self.entries.get_mut(index) {
            entry.zones.push(zone);
        }
        unmapped(&mut self.notes, ops, "opcode");
    }

    /// The group-level settings of a region: envelope, keytrack, trigger,
    /// velocity tracking, round robin, controller conditions.
    fn settings(&mut self, ops: &Opcodes) -> ir::Group {
        let mut envelope = laws::ENVELOPE;
        let times = [
            ("ampeg_attack", &mut envelope.attack),
            ("ampeg_hold", &mut envelope.hold),
            ("ampeg_decay", &mut envelope.decay),
            ("ampeg_release", &mut envelope.release),
        ];
        for (name, field) in times {
            if let Some(v) = self.number(ops, &[name]) {
                *field = laws::seconds(v);
            }
        }
        if let Some(v) = self.number(ops, &["ampeg_sustain"]) {
            envelope.sustain = laws::sustain(v);
        }
        ir::Group {
            envelope,
            mods: laws::velocity_track(self.number(ops, &["amp_veltrack"]).unwrap_or(100.0))
                .into_iter()
                .collect(),
            keytrack: self.number(ops, &["pitch_keytrack"]).unwrap_or(100.0) / 100.0,
            trigger: self
                .read(ops, &["trigger"], laws::trigger)
                .unwrap_or_default(),
            one_shot: raw(ops, &["loop_mode", "loopmode"]) == Some("one_shot"),
            start: self.start_rules(ops),
            inserts: self.filter(ops).into_iter().map(Some).collect(),
            ..ir::Group::DEFAULT
        }
    }

    /// `seq_length` / `seq_position` and the `locc`/`hicc` pairs, all ANDed.
    fn start_rules(&mut self, ops: &Opcodes) -> Vec<(ir::Join, ir::StartRule)> {
        let mut rules = Vec::new();
        let length = self.number(ops, &["seq_length"]).map_or(1, whole);
        if length > 1 {
            let position = self
                .number(ops, &["seq_position"])
                .map_or(1, whole)
                .saturating_sub(1);
            rules.push((
                ir::Join::And,
                ir::StartRule::RoundRobin {
                    position: u8::try_from(position).unwrap_or(0),
                    length: u8::try_from(length).unwrap_or(u8::MAX),
                    random: false,
                },
            ));
        }
        let ccs: BTreeSet<u8> = ops
            .keys()
            .filter_map(|k| cc_number(k, "locc").or_else(|| cc_number(k, "hicc")))
            .collect();
        for cc in ccs {
            let (lo_name, hi_name) = (format!("locc{cc}"), format!("hicc{cc}"));
            let lo = self.number(ops, &[lo_name.as_str()]).map_or(0, midi);
            let hi = self.number(ops, &[hi_name.as_str()]).map_or(127, midi);
            if lo <= hi {
                let range = lo..=hi;
                rules.push((ir::Join::And, ir::StartRule::Controller { cc, range }));
            } else {
                self.notes
                    .add(ir::Impact::Audible, "cc range", format!("cc{cc}"));
            }
        }
        rules
    }

    /// `loop_start`/`loop_end` as the zone's loop, SFZ's inclusive end made exclusive.
    fn looped(&mut self, ops: &Opcodes, total: u64) -> Option<ir::Loop> {
        let mode = raw(ops, &["loop_mode", "loopmode"]).unwrap_or("no_loop");
        match laws::loops(mode) {
            None => {
                self.notes.add(ir::Impact::Audible, "loop_mode", mode);
                None
            }
            Some(false) => None,
            Some(true) => {
                let start = self
                    .number(ops, &["loop_start", "loopstart"])
                    .map_or(0, frames)
                    .min(total);
                let end = self
                    .number(ops, &["loop_end", "loopend"])
                    .map_or(total, |end| frames(end).saturating_add(1))
                    .min(total);
                if end > start {
                    Some(ir::Loop {
                        start,
                        end,
                        until_release: mode == "loop_sustain",
                        ..ir::Loop::default()
                    })
                } else {
                    if total > 0 {
                        self.notes.add(
                            ir::Impact::Audible,
                            "empty loop",
                            format!("{start}..{end}"),
                        );
                    }
                    None
                }
            }
        }
    }

    /// `fil_type` and `cutoff`/`resonance` as a group insert, which runs per voice.
    fn filter(&mut self, ops: &Opcodes) -> Option<ir::Effect> {
        let cutoff = self.number(ops, &["cutoff"])?;
        let shape = self
            .read(ops, &["fil_type"], laws::filter)
            .unwrap_or(laws::Filter::Lpf2p);
        let k = laws::damping(self.number(ops, &["resonance"]).unwrap_or(0.0));
        Some(ir::Effect {
            kind: ir::EffectKind::Filter(laws::voice_filter(shape, cutoff, k)),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        })
    }

    /// The index of the group with these settings, made when new.
    fn entry(&mut self, number: u32, off_by: Option<u32>, settings: ir::Group) -> usize {
        let found = self
            .entries
            .iter()
            .position(|e| e.number == number && e.off_by == off_by && e.settings == settings);
        found.unwrap_or_else(|| {
            self.entries.push(Entry {
                number,
                off_by,
                settings,
                zones: Vec::new(),
            });
            self.entries.len() - 1
        })
    }

    /// The sample file's index and frame count. A file that cannot be read is
    /// reported and kept with zero frames, so the zone stays visible in the IR.
    fn sample(&mut self, name: &str, dir: &Path) -> (usize, u64) {
        let file = dir.join(name);
        if let Some(&index) = self.sample_index.get(&file) {
            let frames = self.samples.get(index).map_or(0, |s| s.frames);
            return (index, frames);
        }
        let (channels, rate, frames) = match hound::WavReader::open(&file) {
            Ok(reader) => {
                let spec = reader.spec();
                let channels = u8::try_from(spec.channels).unwrap_or(0);
                (channels, spec.sample_rate, u64::from(reader.duration()))
            }
            Err(e) => {
                self.notes.add(
                    ir::Impact::Broken,
                    "sample file",
                    format!("{}: {e}", file.display()),
                );
                (0, 0, 0)
            }
        };
        self.samples.push(ir::Sample {
            name: name.into(),
            file: file.clone(),
            offset: 0,
            len: None,
            channels,
            frames,
            rate,
            keystream: None,
        });
        let index = self.samples.len() - 1;
        self.sample_index.insert(file, index);
        (index, frames)
    }

    /// The value of the first opcode in `names` that parses. A value that does
    /// not parse is reported and read as absent.
    fn read<T>(
        &mut self,
        ops: &Opcodes,
        names: &[&str],
        parse: impl Fn(&str) -> Option<T>,
    ) -> Option<T> {
        let text = raw(ops, names)?;
        let value = parse(text);
        if value.is_none() {
            let name = names.first().copied().unwrap_or_default();
            self.notes.add(
                ir::Impact::Audible,
                "invalid value",
                format!("{name}={text}"),
            );
        }
        value
    }

    fn number(&mut self, ops: &Opcodes, names: &[&str]) -> Option<f64> {
        self.read(ops, names, |text| {
            text.parse().ok().filter(|v: &f64| v.is_finite())
        })
    }

    fn finish(self) -> ir::Instrument {
        let mut notes = self.notes;
        let numbers: Vec<u32> = self.entries.iter().map(|e| e.number).collect();
        for entry in &self.entries {
            if let Some(n) = entry.off_by
                && numbers.iter().filter(|&&x| x == n).count() > 1
            {
                notes.add(
                    ir::Impact::Audible,
                    "off_by to a split group",
                    format!("group {n}"),
                );
            }
        }
        let groups = self
            .entries
            .into_iter()
            .map(|e| ir::Group {
                off_by: e.off_by.and_then(|n| numbers.iter().position(|&x| x == n)),
                zones: e.zones,
                ..e.settings
            })
            .collect();
        ir::Instrument {
            samples: self.samples,
            groups,
            diagnostics: notes.finish(),
            ..ir::Instrument::DEFAULT
        }
    }
}

/// The first of `names` that is present, as written.
fn raw<'a>(ops: &'a Opcodes, names: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .find_map(|name| ops.get(*name))
        .map(String::as_str)
}

/// Reports every opcode this does not map.
fn unmapped(notes: &mut Notes, ops: &Opcodes, scope: &str) {
    for (name, value) in ops {
        if !mapped(name) {
            notes.add(ir::Impact::Audible, &format!("{scope} {name}"), value);
        }
    }
}

/// Backslashes to slashes, without surrounding spaces.
fn normalize(path: &str) -> String {
    path.trim().replace('\\', "/")
}

fn midi(value: f64) -> u8 {
    value.clamp(0.0, 127.0).round() as u8
}

fn frames(value: f64) -> u64 {
    value.max(0.0) as u64
}

fn whole(value: f64) -> u32 {
    value.clamp(0.0, f64::from(u32::MAX)) as u32
}

/// Diagnostics folded by feature: one entry with a count and the first detail.
#[derive(Default)]
struct Notes(BTreeMap<(ir::Impact, String), (usize, String)>);

impl Notes {
    fn add(&mut self, impact: ir::Impact, feature: &str, detail: impl ToString) {
        let entry = self
            .0
            .entry((impact, feature.into()))
            .or_insert((0, detail.to_string()));
        entry.0 += 1;
    }

    fn finish(self) -> Vec<ir::Diagnostic> {
        self.0
            .into_iter()
            .rev()
            .map(|((impact, feature), (count, first))| ir::Diagnostic {
                impact,
                detail: format!("{count}× (first: {first})"),
                feature,
            })
            .collect()
    }
}
