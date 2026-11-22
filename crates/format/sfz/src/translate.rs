//! Regions into the instrument model: one zone per region, groups by their
//! group-level settings, samples by file. What the IR cannot carry is a
//! diagnostic (06-FORMATS.md).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use kontra_format::Notes;
use kontra_format::builder::SamplePool;
use kontra_format::math::parse_midi_note;
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
    "xfin_hikey",
    "xfout_lokey",
    "xfout_hikey",
    "xf_keycurve",
    "lovel",
    "hivel",
    "xfin_lovel",
    "xfin_hivel",
    "xfout_lovel",
    "xfout_hivel",
    "xf_velcurve",
    "xf_cccurve",
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
    "lorand",
    "hirand",
    "group",
    "off_by",
    "off_mode",
    "off_time",
    "polyphony",
    "note_polyphony",
    "note_selfmask",
    "amp_veltrack",
    "direction",
    "cutoff",
    "resonance",
    "fil_type",
    "default_path",
    "ampeg_delay",
    "ampeg_attack",
    "ampeg_hold",
    "ampeg_decay",
    "ampeg_sustain",
    "ampeg_release",
];

fn mapped(name: &str) -> bool {
    MAPPED.contains(&name)
        || cc_number(name, "locc").is_some()
        || cc_number(name, "hicc").is_some()
        || cc_number(name, "xfin_locc").is_some()
        || cc_number(name, "xfin_hicc").is_some()
        || cc_number(name, "xfout_locc").is_some()
        || cc_number(name, "xfout_hicc").is_some()
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

/// Canonical bit representation of `f32`, normalizing `-0.0` to `+0.0`.
fn bits_f32(v: f32) -> u32 {
    if v == 0.0 {
        0.0_f32.to_bits()
    } else {
        v.to_bits()
    }
}

/// Canonical bit representation of `f64`, normalizing `-0.0` to `+0.0`.
fn bits_f64(v: f64) -> u64 {
    if v == 0.0 {
        0.0_f64.to_bits()
    } else {
        v.to_bits()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum StartRuleKey {
    RoundRobin {
        scope: ir::RrScope,
        position: u8,
        length: u8,
        random: bool,
    },
    Controller {
        cc: u8,
        lo: u8,
        hi: u8,
    },
    Random {
        lo: u16,
        hi: u16,
    },
}

/// Hashable exact key for deduplicating SFZ groups in `O(1)` amortized time.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct GroupKey {
    number: u32,
    off_by: Option<u32>,
    polyphony: Option<u16>,
    envelope: [u32; 6],
    veltrack: Option<u32>,
    keytrack: u64,
    trigger: u8,
    one_shot: bool,
    start: Vec<StartRuleKey>,
    filter: Option<(u8, u32, u8, [u32; 3], u32)>,
}

impl GroupKey {
    fn of(number: u32, off_by: Option<u32>, settings: &ir::Group) -> Self {
        let env = settings.envelope;
        let veltrack = settings.mods.first().and_then(|m| match m.kind {
            ir::ModKind::Volume { intensity, .. } => Some(bits_f32(intensity)),
            _ => None,
        });
        let trigger = match settings.trigger {
            ir::Trigger::Attack => 0,
            ir::Trigger::Release => 1,
            ir::Trigger::First => 2,
            ir::Trigger::Legato => 3,
        };
        let start = settings
            .start
            .iter()
            .filter_map(|(_, rule)| match rule {
                ir::StartRule::RoundRobin {
                    scope,
                    position,
                    length,
                    random,
                } => Some(StartRuleKey::RoundRobin {
                    scope: *scope,
                    position: *position,
                    length: *length,
                    random: *random,
                }),
                ir::StartRule::Controller { cc, range } => Some(StartRuleKey::Controller {
                    cc: *cc,
                    lo: *range.start(),
                    hi: *range.end(),
                }),
                ir::StartRule::Random { lo, hi } => Some(StartRuleKey::Random { lo: *lo, hi: *hi }),
                _ => None,
            })
            .collect();
        let filter = settings
            .inserts
            .first()
            .and_then(|slot| slot.as_ref())
            .and_then(|eff| match eff.kind {
                ir::EffectKind::Filter(f) => {
                    let pass = match f.pass {
                        ir::Pass::Low => 0,
                        ir::Pass::High => 1,
                        ir::Pass::Band => 2,
                        ir::Pass::Notch => 3,
                    };
                    Some((
                        pass,
                        bits_f32(f.hz),
                        f.stages,
                        [
                            bits_f32(f.damping[0]),
                            bits_f32(f.damping[1]),
                            bits_f32(f.damping[2]),
                        ],
                        bits_f32(f.gain),
                    ))
                }
                _ => None,
            });

        Self {
            number,
            off_by,
            polyphony: settings.polyphony,
            envelope: [
                bits_f32(env.delay),
                bits_f32(env.attack),
                bits_f32(env.hold),
                bits_f32(env.decay),
                bits_f32(env.sustain),
                bits_f32(env.release),
            ],
            veltrack,
            keytrack: bits_f64(settings.keytrack),
            trigger,
            one_shot: settings.one_shot,
            start,
            filter,
        }
    }
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
    samples: SamplePool<'static>,
    entries: Vec<Entry>,
    entry_index: HashMap<GroupKey, usize>,
    /// The voice rules of each group number that has one (its polyphony,
    /// key limit or off mode); with off_by targets, each becomes a voice group.
    limits: BTreeMap<u32, ir::VoiceGroup>,
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
        let sample_name = normalize(sample);
        let path = dir.join(&sample_name);
        let (sample_idx, hdr) = self.samples.add(sample_name, &path, &mut self.notes);
        let total = hdr.map_or(0, |h| h.frames);

        let key = self.read(ops, &["key"], parse_midi_note);
        let lo = self
            .read(ops, &["lokey"], parse_midi_note)
            .or(key)
            .unwrap_or(0);
        let hi = self
            .read(ops, &["hikey"], parse_midi_note)
            .or(key)
            .unwrap_or(127);
        let xfin_lokey = self.read(ops, &["xfin_lokey"], parse_midi_note);
        let xfin_hikey = self.read(ops, &["xfin_hikey"], parse_midi_note);
        let xfout_lokey = self.read(ops, &["xfout_lokey"], parse_midi_note);
        let xfout_hikey = self.read(ops, &["xfout_hikey"], parse_midi_note);
        let (keys, key_fade) =
            laws::span(lo, hi, xfin_lokey.unwrap_or(lo), xfout_hikey.unwrap_or(hi));

        let vlo = self.number(ops, &["lovel"]).map_or(0, midi);
        let vhi = self.number(ops, &["hivel"]).map_or(127, midi);
        let xfin_lovel = self.number(ops, &["xfin_lovel"]).map(midi);
        let xfin_hivel = self.number(ops, &["xfin_hivel"]).map(midi);
        let xfout_lovel = self.number(ops, &["xfout_lovel"]).map(midi);
        let xfout_hivel = self.number(ops, &["xfout_hivel"]).map(midi);
        let (velocities, velocity_fade) = laws::span(
            vlo,
            vhi,
            xfin_lovel.unwrap_or(vlo),
            xfout_hivel.unwrap_or(vhi),
        );

        let mut crossfades = Vec::new();
        let key_law = self.xf_law(ops, "xf_keycurve");
        if let Some(xf) = build_crossfade(
            ir::CrossfadeControl::Key,
            xfin_lokey,
            xfin_hikey,
            lo,
            xfout_lokey,
            xfout_hikey,
            hi,
            key_law,
        ) {
            crossfades.push(xf);
        }
        let vel_law = self.xf_law(ops, "xf_velcurve");
        if let Some(xf) = build_crossfade(
            ir::CrossfadeControl::Velocity,
            xfin_lovel,
            xfin_hivel,
            vlo,
            xfout_lovel,
            xfout_hivel,
            vhi,
            vel_law,
        ) {
            crossfades.push(xf);
        }
        self.cc_crossfades(ops, &mut crossfades);

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
        let loops = self.looped(ops, total, sample_idx);
        // No SFZ script addresses a zone by id.
        let zone = ir::Zone {
            id: 0,
            mix: Vec::new(),
            sample: sample_idx,
            keys,
            velocities,
            velocity_fade,
            key_fade,
            crossfades,
            root: self
                .read(ops, &["pitch_keycenter"], parse_midi_note)
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
            loops,
            reverse: raw(ops, &["direction"]) == Some("reverse"),
            slot: 0,
            envelopes: Vec::new(),
        };

        let mut settings = self.settings(ops);
        let grouped = self.number(ops, &["group"]);
        let number = grouped.map_or(0, whole);
        let off_by = self.number(ops, &["off_by"]).map(whole);
        let polyphony = self.number(ops, &["polyphony"]).map(voices);
        if grouped.is_none() {
            // Without a group number the limit is its own (sfizz: the
            // region's or its header's): kept per group of settings.
            settings.polyphony = polyphony;
        }
        self.voice_rules(ops, number, grouped.and(polyphony));
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
            ("ampeg_delay", &mut envelope.delay),
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
                    scope: ir::RrScope::Key,
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
        let lorand = self.number(ops, &["lorand"]);
        let hirand = self.number(ops, &["hirand"]);
        if lorand.is_some() || hirand.is_some() {
            let lo_f = lorand.unwrap_or(0.0).clamp(0.0, 1.0);
            let hi_f = hirand.unwrap_or(1.0).clamp(0.0, 1.0);
            let lo = (lo_f * 10_000.0).round() as u16;
            let hi = (hi_f * 10_000.0).round() as u16;
            if lo > hi {
                self.notes.add(
                    ir::Impact::Audible,
                    "random range",
                    format!("{lo_f}..{hi_f}"),
                );
            } else if !(lo == 0 && hi == 10_000) {
                rules.push((ir::Join::And, ir::StartRule::Random { lo, hi }));
            }
        }
        rules
    }

    /// SFZ crossfade law opcode (`xf_keycurve`, `xf_velcurve`, `xf_cccurve`):
    /// defaults to `"power"` (equal-power sine/cosine); `"gain"` is linear.
    fn xf_law(&mut self, ops: &Opcodes, name: &str) -> ir::CrossfadeLaw {
        match raw(ops, &[name]) {
            None | Some("power") => ir::CrossfadeLaw::EqualPower,
            Some("gain") => ir::CrossfadeLaw::Linear,
            Some(other) => {
                self.notes.add(
                    ir::Impact::Audible,
                    "invalid value",
                    format!("{name}={other}"),
                );
                ir::CrossfadeLaw::EqualPower
            }
        }
    }

    /// `xfin_loccN`/`xfin_hiccN`/`xfout_loccN`/`xfout_hiccN` crossfades.
    fn cc_crossfades(&mut self, ops: &Opcodes, out: &mut Vec<ir::Crossfade>) {
        let ccs: BTreeSet<u8> = ops
            .keys()
            .filter_map(|k| {
                cc_number(k, "xfin_locc")
                    .or_else(|| cc_number(k, "xfin_hicc"))
                    .or_else(|| cc_number(k, "xfout_locc"))
                    .or_else(|| cc_number(k, "xfout_hicc"))
            })
            .collect();
        if ccs.is_empty() {
            return;
        }
        let law = self.xf_law(ops, "xf_cccurve");
        for cc in ccs {
            let in_lo_k = format!("xfin_locc{cc}");
            let in_hi_k = format!("xfin_hicc{cc}");
            let out_lo_k = format!("xfout_locc{cc}");
            let out_hi_k = format!("xfout_hicc{cc}");
            let in_lo = self.number(ops, &[in_lo_k.as_str()]).map(midi);
            let in_hi = self.number(ops, &[in_hi_k.as_str()]).map(midi);
            let out_lo = self.number(ops, &[out_lo_k.as_str()]).map(midi);
            let out_hi = self.number(ops, &[out_hi_k.as_str()]).map(midi);
            let fade_in = (in_lo.is_some() || in_hi.is_some())
                .then(|| {
                    let lo = in_lo.unwrap_or(0);
                    let hi = in_hi.unwrap_or(0);
                    (lo, hi)
                })
                .filter(|(lo, hi)| hi > lo)
                .map(|(lo, hi)| (i16::from(lo), i16::from(hi)));
            let fade_out = (out_lo.is_some() || out_hi.is_some())
                .then(|| {
                    let lo = out_lo.unwrap_or(127);
                    let hi = out_hi.unwrap_or(127);
                    (lo, hi)
                })
                .filter(|(lo, hi)| hi > lo)
                .map(|(lo, hi)| (i16::from(lo), i16::from(hi)));
            if fade_in.is_some() || fade_out.is_some() {
                out.push(ir::Crossfade {
                    control: ir::CrossfadeControl::Cc(cc),
                    fade_in,
                    fade_out,
                    law,
                });
            }
        }
    }

    /// `loop_start`/`loop_end` as the zone's loops, SFZ's inclusive end made
    /// exclusive. When `loop_mode` enables looping without explicit points,
    /// embedded loops from the sample file header (`smpl` / `INST`) are used.
    fn looped(&mut self, ops: &Opcodes, total: u64, sample_idx: usize) -> Vec<ir::Loop> {
        let mode = raw(ops, &["loop_mode", "loopmode"]).unwrap_or("no_loop");
        match laws::loops(mode) {
            None => {
                self.notes.add(ir::Impact::Audible, "loop_mode", mode);
                Vec::new()
            }
            Some(false) => Vec::new(),
            Some(true) => {
                let until_release = mode == "loop_sustain";
                let explicit_start = self.number(ops, &["loop_start", "loopstart"]);
                let explicit_end = self.number(ops, &["loop_end", "loopend"]);
                if explicit_start.is_none()
                    && explicit_end.is_none()
                    && let Some(hdr) = self.samples.header(sample_idx)
                    && !hdr.loops.is_empty()
                {
                    return hdr
                        .loops
                        .iter()
                        .map(|l| l.to_ir_loop(until_release))
                        .collect();
                }

                let start = explicit_start.map_or(0, frames).min(total);
                let end = explicit_end
                    .map_or(total, |end| frames(end).saturating_add(1))
                    .min(total);
                if end > start {
                    vec![ir::Loop {
                        start,
                        end,
                        until_release,
                        ..ir::Loop::default()
                    }]
                } else {
                    if total > 0 {
                        self.notes.add(
                            ir::Impact::Audible,
                            "empty loop",
                            format!("{start}..{end}"),
                        );
                    }
                    Vec::new()
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

    /// The voice rules a region gives its group number (the last region's
    /// win, as sfizz's): `polyphony` with `group`, `note_polyphony` and
    /// `note_selfmask`, `off_mode` and `off_time`. sfizz counts voices not
    /// yet stopped, released ones too, the oldest first.
    fn voice_rules(&mut self, ops: &Opcodes, number: u32, polyphony: Option<u16>) {
        let per_key = self.number(ops, &["note_polyphony"]).map(voices);
        let mask = raw(ops, &["note_selfmask"]);
        let off = raw(ops, &["off_mode", "offmode"]);
        let off_time = self.number(ops, &["off_time"]);
        if polyphony.is_none() && per_key.is_none() && off.is_none() && off_time.is_none() {
            return;
        }
        let limit = self.limits.entry(number).or_insert(laws::VOICE_GROUP);
        if let Some(v) = polyphony {
            limit.voices = v;
        }
        if per_key.is_some() {
            limit.per_key = per_key;
            limit.quieter_only = !matches!(mask, Some("off" | "dontmask"));
        }
        // off_time alone is off_mode=time.
        match (off, off_time) {
            (Some("normal"), _) => limit.release = true,
            (Some("time") | None, Some(t)) => {
                limit.release = false;
                limit.fade_ms = (t.max(0.0) * 1000.0) as f32;
            }
            (Some("fast" | "time"), _) => {
                limit.release = false;
                limit.fade_ms = laws::VOICE_GROUP.fade_ms;
            }
            (Some(other), _) => self.notes.add(ir::Impact::Audible, "off_mode", other),
            (None, None) => {}
        }
    }

    /// The index of the group with these settings, made when new (`O(1)` lookup).
    fn entry(&mut self, number: u32, off_by: Option<u32>, settings: ir::Group) -> usize {
        let key = GroupKey::of(number, off_by, &settings);
        if let Some(&idx) = self.entry_index.get(&key) {
            return idx;
        }
        let idx = self.entries.len();
        self.entries.push(Entry {
            number,
            off_by,
            settings,
            zones: Vec::new(),
        });
        self.entry_index.insert(key, idx);
        idx
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
        // Group numbers with voice rules, and those an off_by names (a choke
        // stops its targets as their own number's rule does).
        let mut limits = self.limits;
        for e in &self.entries {
            if let Some(n) = e.off_by {
                limits.entry(n).or_insert(laws::VOICE_GROUP);
            }
        }
        let numbers: Vec<u32> = limits.keys().copied().collect();
        let index = |n: u32| numbers.iter().position(|&x| x == n);
        let groups = self
            .entries
            .into_iter()
            .map(|e| ir::Group {
                off_by: e.off_by.and_then(index),
                voice_group: index(e.number),
                zones: e.zones,
                ..e.settings
            })
            .collect();
        ir::Instrument {
            samples: self.samples.into_samples(),
            groups,
            voice_groups: limits.into_values().collect(),
            diagnostics: self.notes.finish(),
            ..ir::Instrument::DEFAULT
        }
    }
}

/// Builds an explicit key or velocity crossfade when any `xfin_*`/`xfout_*`
/// opcode is present on the region.
#[allow(clippy::too_many_arguments)]
fn build_crossfade(
    control: ir::CrossfadeControl,
    in_lo: Option<u8>,
    in_hi: Option<u8>,
    default_in_hi: u8,
    out_lo: Option<u8>,
    out_hi: Option<u8>,
    default_out_lo: u8,
    law: ir::CrossfadeLaw,
) -> Option<ir::Crossfade> {
    let fade_in = (in_lo.is_some() || in_hi.is_some())
        .then(|| (in_lo.unwrap_or(0), in_hi.unwrap_or(default_in_hi)))
        .filter(|(lo, hi)| hi > lo)
        .map(|(lo, hi)| (i16::from(lo), i16::from(hi)));
    let fade_out = (out_lo.is_some() || out_hi.is_some())
        .then(|| (out_lo.unwrap_or(default_out_lo), out_hi.unwrap_or(127)))
        .filter(|(lo, hi)| hi > lo)
        .map(|(lo, hi)| (i16::from(lo), i16::from(hi)));
    (fade_in.is_some() || fade_out.is_some()).then_some(ir::Crossfade {
        control,
        fade_in,
        fade_out,
        law,
    })
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

fn voices(value: f64) -> u16 {
    value.clamp(1.0, f64::from(u16::MAX)) as u16
}

fn whole(value: f64) -> u32 {
    value.clamp(0.0, f64::from(u32::MAX)) as u32
}
