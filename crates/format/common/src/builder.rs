//! Ergonomic builders and sample deduplication for translating multisampler
//! formats into [`kontra_ir::Instrument`].

use std::collections::HashMap;
use std::io;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use kontra_ir as ir;

use crate::Notes;
use crate::header::{AudioHeader, probe_region};

/// Probing function used by [`SamplePool`] to read audio headers.
pub type ProbeFn<'a> = Box<dyn Fn(&Path) -> io::Result<AudioHeader> + 'a>;

type RegionProbe<'a> = dyn Fn(&Path, u64, Option<u64>) -> io::Result<AudioHeader> + 'a;

/// Deduplicates sample references by `(path, offset, length)` and probes each
/// unique region once, caching both success and failure.
pub struct SamplePool<'a> {
    samples: Vec<ir::Sample>,
    headers: Vec<Option<AudioHeader>>,
    index: HashMap<(PathBuf, u64, Option<u64>), usize>,
    probe: Box<RegionProbe<'a>>,
}

impl Default for SamplePool<'static> {
    fn default() -> Self {
        Self::new()
    }
}

impl SamplePool<'static> {
    /// Creates a pool that probes audio headers directly from disk including bounded regions.
    #[must_use]
    pub fn new() -> Self {
        Self::with_region_prober(probe_region)
    }
}

impl<'a> SamplePool<'a> {
    /// Creates a pool using a custom whole-file prober. Slices are probed from disk.
    #[must_use]
    pub fn with_prober<F>(probe: F) -> Self
    where
        F: Fn(&Path) -> io::Result<AudioHeader> + 'a,
    {
        Self::with_region_prober(move |path, offset, len| {
            if offset == 0 && len.is_none() {
                probe(path)
            } else {
                probe_region(path, offset, len)
            }
        })
    }

    /// Creates a pool using a custom prober for whole files and bounded regions.
    #[must_use]
    pub fn with_region_prober<F>(probe: F) -> Self
    where
        F: Fn(&Path, u64, Option<u64>) -> io::Result<AudioHeader> + 'a,
    {
        Self {
            samples: Vec::new(),
            headers: Vec::new(),
            index: HashMap::new(),
            probe: Box::new(probe),
        }
    }

    /// Registers a whole sample file, deduplicating by resolved path and probing
    /// its header once. Missing or unreadable files are reported into `notes`
    /// and kept with zero frames so referencing zones remain visible in the IR.
    pub fn add(
        &mut self,
        name: impl Into<String>,
        path: &Path,
        notes: &mut Notes,
    ) -> (usize, Option<&AudioHeader>) {
        self.add_slice(name, path, 0, None, notes)
    }

    /// Registers a sample slice `(path, offset, len)` with automatic deduplication.
    pub fn add_slice(
        &mut self,
        name: impl Into<String>,
        path: &Path,
        offset: u64,
        len: Option<u64>,
        notes: &mut Notes,
    ) -> (usize, Option<&AudioHeader>) {
        let file = crate::moved(path);
        let key = (file.clone(), offset, len);
        if let Some(&idx) = self.index.get(&key) {
            return (idx, self.headers.get(idx).and_then(Option::as_ref));
        }

        let (channels, rate, frames, header) = match (self.probe)(&file, offset, len) {
            Ok(hdr) => {
                let channels = u8::try_from(hdr.channels).unwrap_or(0);
                let rate = hdr.sample_rate;
                let frames = hdr.frames;
                (channels, rate, frames, Some(hdr))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                notes.add_missing(&file);
                (0, 0, 0, None)
            }
            Err(e) => {
                notes.add(
                    ir::Impact::Broken,
                    "sample file",
                    format!("{}: {e}", file.display()),
                );
                (0, 0, 0, None)
            }
        };

        let idx = self.samples.len();
        self.samples.push(ir::Sample {
            name: name.into(),
            file,
            offset,
            len,
            channels,
            frames,
            rate,
            keystream: None,
        });
        self.headers.push(header);
        self.index.insert(key, idx);
        (idx, self.headers.get(idx).and_then(Option::as_ref))
    }

    /// Returns the declared frame count for sample `index`, or 0 if out of range.
    #[must_use]
    pub fn frames(&self, index: usize) -> u64 {
        self.samples.get(index).map_or(0, |s| s.frames)
    }

    /// Returns the probed header for sample `index`, if probing succeeded.
    #[must_use]
    pub fn header(&self, index: usize) -> Option<&AudioHeader> {
        self.headers.get(index).and_then(Option::as_ref)
    }

    /// Returns all registered samples so far.
    #[must_use]
    pub fn samples(&self) -> &[ir::Sample] {
        &self.samples
    }

    /// Consumes the pool and returns the deduplicated sample table.
    #[must_use]
    pub fn into_samples(self) -> Vec<ir::Sample> {
        self.samples
    }

    /// Number of unique samples in the pool.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether no samples have been registered yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// Fluent builder for an [`ir::Zone`] with sensible sampler defaults.
#[derive(Clone, Debug)]
pub struct ZoneBuilder {
    zone: ir::Zone,
}

impl ZoneBuilder {
    /// Starts a zone playing `sample` over `0..frames` across all keys (`0..=127`),
    /// velocities (`1..=127`), root `60` (C4), unity gain, centered pan, and no detune.
    #[must_use]
    pub fn new(sample: usize, frames: u64) -> Self {
        Self {
            zone: ir::Zone {
                id: 0,
                sample,
                keys: 0..=127,
                velocities: 1..=127,
                velocity_fade: (0, 0),
                key_fade: (0, 0),
                root: 60,
                tune: 0.0,
                volume_db: 0.0,
                pan: 0.0,
                start: 0,
                end: frames,
                start_mod: 0,
                loops: Vec::new(),
                reverse: false,
                mix: Vec::new(),
                slot: 0,
                envelopes: Vec::new(),
            },
        }
    }

    #[must_use]
    pub fn keys(mut self, keys: RangeInclusive<u8>) -> Self {
        self.zone.keys = keys;
        self
    }

    #[must_use]
    pub fn key(mut self, key: u8) -> Self {
        self.zone.keys = key..=key;
        self.zone.root = key;
        self
    }

    #[must_use]
    pub fn velocities(mut self, velocities: RangeInclusive<u8>) -> Self {
        self.zone.velocities = velocities;
        self
    }

    #[must_use]
    pub fn key_fade(mut self, fade: (u8, u8)) -> Self {
        self.zone.key_fade = fade;
        self
    }

    #[must_use]
    pub fn velocity_fade(mut self, fade: (u8, u8)) -> Self {
        self.zone.velocity_fade = fade;
        self
    }

    #[must_use]
    pub fn root(mut self, root: u8) -> Self {
        self.zone.root = root;
        self
    }

    #[must_use]
    pub fn gain_db(mut self, db: f32) -> Self {
        self.zone.volume_db = db;
        self
    }

    #[must_use]
    pub fn pan(mut self, pan: f32) -> Self {
        self.zone.pan = pan.clamp(-1.0, 1.0);
        self
    }

    #[must_use]
    pub fn tune(mut self, semitones: f64) -> Self {
        self.zone.tune = semitones;
        self
    }

    #[must_use]
    pub fn range(mut self, start: u64, end: u64) -> Self {
        self.zone.start = start.min(end);
        self.zone.end = end;
        self
    }

    #[must_use]
    pub fn add_loop(mut self, loop_def: ir::Loop) -> Self {
        self.zone.loops.push(loop_def);
        self
    }

    #[must_use]
    pub fn loops(mut self, loops: Vec<ir::Loop>) -> Self {
        self.zone.loops = loops;
        self
    }

    #[must_use]
    pub fn reverse(mut self, reverse: bool) -> Self {
        self.zone.reverse = reverse;
        self
    }

    #[must_use]
    pub fn slot(mut self, slot: u16) -> Self {
        self.zone.slot = slot;
        self
    }

    #[must_use]
    pub fn id(mut self, id: u32) -> Self {
        self.zone.id = id;
        self
    }

    #[must_use]
    pub fn build(self) -> ir::Zone {
        self.zone
    }
}

impl From<ZoneBuilder> for ir::Zone {
    fn from(builder: ZoneBuilder) -> Self {
        builder.build()
    }
}

/// Fluent builder for an [`ir::Group`] with sensible sampler defaults.
#[derive(Clone, Debug)]
pub struct GroupBuilder {
    group: ir::Group,
}

impl GroupBuilder {
    /// Starts a group with unity gain, centered pan, chromatic keytracking (`1.0`),
    /// and [`ir::Envelope::GATE`].
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            group: ir::Group {
                name: name.into(),
                ..ir::Group::DEFAULT
            },
        }
    }

    #[must_use]
    pub fn gain_db(mut self, db: f32) -> Self {
        self.group.volume_db = db;
        self
    }

    #[must_use]
    pub fn pan(mut self, pan: f32) -> Self {
        self.group.pan = pan.clamp(-1.0, 1.0);
        self
    }

    #[must_use]
    pub fn tune(mut self, semitones: f64) -> Self {
        self.group.tune = semitones;
        self
    }

    #[must_use]
    pub fn keytrack(mut self, ratio: f64) -> Self {
        self.group.keytrack = ratio;
        self
    }

    #[must_use]
    pub fn envelope(mut self, envelope: ir::Envelope) -> Self {
        self.group.envelope = envelope;
        self
    }

    #[must_use]
    pub fn adsr(mut self, attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        self.group.envelope.attack = attack.max(0.0);
        self.group.envelope.decay = decay.max(0.0);
        self.group.envelope.sustain = sustain.clamp(0.0, 1.0);
        self.group.envelope.release = release.max(0.0);
        self
    }

    /// Adds a standard linear velocity-to-volume modulation route (`amount` in `-1.0..=1.0`).
    /// Does nothing when `amount == 0.0`.
    #[must_use]
    pub fn with_velocity_tracking(self, amount: f32) -> Self {
        self.with_velocity_tracking_curve(amount, ir::Curve::Depth { power: 1 })
    }

    /// Adds a velocity-to-volume modulation route using a specific [`ir::Curve`].
    /// Does nothing when `amount == 0.0`.
    #[must_use]
    pub fn with_velocity_tracking_curve(mut self, amount: f32, curve: ir::Curve) -> Self {
        let intensity = amount.clamp(-1.0, 1.0);
        if intensity != 0.0 {
            self.group.mods.push(ir::Mod {
                name: "velocity".into(),
                slot: 0,
                target: 0,
                bypass: false,
                kind: ir::ModKind::Volume {
                    source: ir::Source::Velocity,
                    intensity,
                    curve,
                    tau_ms: 0.0,
                },
            });
        }
        self
    }

    #[must_use]
    pub fn trigger(mut self, trigger: ir::Trigger) -> Self {
        self.group.trigger = trigger;
        self
    }

    #[must_use]
    pub fn one_shot(mut self, one_shot: bool) -> Self {
        self.group.one_shot = one_shot;
        self
    }

    #[must_use]
    pub fn polyphony(mut self, voices: Option<u16>) -> Self {
        self.group.polyphony = voices;
        self
    }

    #[must_use]
    pub fn voice_group(mut self, voice_group: Option<usize>) -> Self {
        self.group.voice_group = voice_group;
        self
    }

    #[must_use]
    pub fn off_by(mut self, off_by: Option<usize>) -> Self {
        self.group.off_by = off_by;
        self
    }

    #[must_use]
    pub fn add_start_rule(mut self, join: ir::Join, rule: ir::StartRule) -> Self {
        self.group.start.push((join, rule));
        self
    }

    #[must_use]
    pub fn add_insert(mut self, effect: ir::Effect) -> Self {
        self.group.inserts.push(Some(effect));
        self
    }

    #[must_use]
    pub fn add_mod(mut self, m: ir::Mod) -> Self {
        self.group.mods.push(m);
        self
    }

    #[must_use]
    pub fn add_lfo(mut self, lfo: ir::Lfo) -> Self {
        self.group.lfos.push(lfo);
        self
    }

    #[must_use]
    pub fn add_envelope(mut self, envelope: ir::Breakpoints) -> Self {
        self.group.envelopes.push(envelope);
        self
    }

    #[must_use]
    pub fn add_zone(mut self, zone: impl Into<ir::Zone>) -> Self {
        self.group.zones.push(zone.into());
        self
    }

    #[must_use]
    pub fn build(self) -> ir::Group {
        self.group
    }
}

impl From<GroupBuilder> for ir::Group {
    fn from(builder: GroupBuilder) -> Self {
        builder.build()
    }
}

/// Fluent builder for an [`ir::Instrument`] without vendor-specific boilerplate.
#[derive(Clone, Debug)]
pub struct InstrumentBuilder {
    instrument: ir::Instrument,
}

impl InstrumentBuilder {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            instrument: ir::Instrument {
                name: name.into(),
                ..ir::Instrument::DEFAULT
            },
        }
    }

    #[must_use]
    pub fn source_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.instrument.source_path = Some(path.into());
        self
    }

    #[must_use]
    pub fn voices(mut self, voices: u16) -> Self {
        self.instrument.voices = voices;
        self
    }

    #[must_use]
    pub fn add_sample(mut self, sample: ir::Sample) -> Self {
        self.instrument.samples.push(sample);
        self
    }

    pub fn push_sample(&mut self, sample: ir::Sample) -> usize {
        let idx = self.instrument.samples.len();
        self.instrument.samples.push(sample);
        idx
    }

    #[must_use]
    pub fn samples(mut self, samples: Vec<ir::Sample>) -> Self {
        self.instrument.samples = samples;
        self
    }

    #[must_use]
    pub fn add_group(mut self, group: impl Into<ir::Group>) -> Self {
        self.instrument.groups.push(group.into());
        self
    }

    pub fn push_group(&mut self, group: impl Into<ir::Group>) -> usize {
        let idx = self.instrument.groups.len();
        self.instrument.groups.push(group.into());
        idx
    }

    /// Appends a zone to the last group, creating a default group if none exist yet.
    #[must_use]
    pub fn add_zone(mut self, zone: impl Into<ir::Zone>) -> Self {
        if self.instrument.groups.is_empty() {
            self.instrument.groups.push(ir::Group::DEFAULT);
        }
        if let Some(group) = self.instrument.groups.last_mut() {
            group.zones.push(zone.into());
        }
        self
    }

    #[must_use]
    pub fn add_voice_group(mut self, vg: ir::VoiceGroup) -> Self {
        self.instrument.voice_groups.push(vg);
        self
    }

    pub fn push_voice_group(&mut self, vg: ir::VoiceGroup) -> usize {
        let idx = self.instrument.voice_groups.len();
        self.instrument.voice_groups.push(vg);
        idx
    }

    #[must_use]
    pub fn add_insert(mut self, effect: ir::Effect) -> Self {
        self.instrument.inserts.push(Some(effect));
        self
    }

    #[must_use]
    pub fn add_main(mut self, effect: ir::Effect) -> Self {
        self.instrument.main.push(Some(effect));
        self
    }

    #[must_use]
    pub fn output(mut self, output: ir::Output) -> Self {
        self.instrument.output = output;
        self
    }

    /// Returns a mutable slice of the groups added so far (e.g. for attaching modulators).
    pub fn groups_mut(&mut self) -> &mut [ir::Group] {
        &mut self.instrument.groups
    }

    /// Returns a mutable reference to the instrument-level insert slots.
    pub fn inserts_mut(&mut self) -> &mut Vec<Option<ir::Effect>> {
        &mut self.instrument.inserts
    }

    #[must_use]
    pub fn build(mut self, notes: Notes) -> ir::Instrument {
        self.instrument.diagnostics = notes.finish();
        self.instrument
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::header::AudioLoop;

    #[test]
    fn sample_pool_deduplicates_and_probes_once() {
        let calls = Cell::new(0usize);
        let mut pool = SamplePool::with_prober(|path| {
            calls.set(calls.get() + 1);
            if path.ends_with("missing.wav") {
                return Err(io::Error::new(io::ErrorKind::NotFound, "gone"));
            }
            Ok(AudioHeader {
                sample_rate: 48_000,
                channels: 2,
                frames: 4096,
                loops: vec![AudioLoop {
                    start: 100,
                    end: 500,
                    count: 0,
                    ping_pong: false,
                }],
            })
        });
        let mut notes = Notes::default();
        let (idx0, hdr0) = pool.add("a.wav", Path::new("/lib/a.wav"), &mut notes);
        let frames0 = hdr0.map(|h| h.frames);
        let (idx1, _) = pool.add("a.wav", Path::new("/lib/a.wav"), &mut notes);
        let (idx2, hdr2) = pool.add("missing.wav", Path::new("/lib/missing.wav"), &mut notes);

        assert_eq!(idx0, 0);
        assert_eq!(idx1, 0);
        assert_eq!(idx2, 1);
        assert_eq!(frames0, Some(4096));
        assert!(hdr2.is_none());
        assert_eq!(calls.get(), 2, "a.wav should be probed only once");
        assert_eq!(pool.len(), 2);
        let diags = notes.finish();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].impact, ir::Impact::Broken);
    }

    #[test]
    fn instrument_builder_constructs_clean_ir() {
        let mut pool = SamplePool::with_prober(|_| {
            Ok(AudioHeader {
                sample_rate: 44_100,
                channels: 1,
                frames: 10_000,
                loops: Vec::new(),
            })
        });
        let mut notes = Notes::default();
        let (sample_idx, hdr) = pool.add("piano_c4.wav", Path::new("piano_c4.wav"), &mut notes);
        let frames = hdr.map_or(0, |h| h.frames);

        let inst = InstrumentBuilder::new("Warm Piano")
            .source_path("warm_piano.dspreset")
            .voices(32)
            .samples(pool.into_samples())
            .add_group(
                GroupBuilder::new("Main")
                    .gain_db(-3.0)
                    .pan(0.1)
                    .adsr(0.01, 0.2, 0.8, 0.5)
                    .with_velocity_tracking(0.75)
                    .add_zone(ZoneBuilder::new(sample_idx, frames).keys(48..=72).root(60)),
            )
            .build(notes);

        assert_eq!(inst.name, "Warm Piano");
        assert_eq!(inst.source_path, Some(PathBuf::from("warm_piano.dspreset")));
        assert_eq!(inst.voices, 32);
        assert_eq!(inst.samples.len(), 1);
        assert_eq!(inst.groups.len(), 1);
        assert_eq!(inst.groups[0].name, "Main");
        assert_eq!(inst.groups[0].mods.len(), 1);
        assert_eq!(inst.groups[0].zones.len(), 1);
        assert_eq!(inst.groups[0].zones[0].keys, 48..=72);
        assert_eq!(inst.groups[0].zones[0].velocities, 1..=127);
        assert_eq!(inst.groups[0].zones[0].end, 10_000);
        assert!(inst.diagnostics.is_empty());
    }

    #[test]
    fn instrument_builder_add_zone_auto_creates_default_group() {
        let inst = InstrumentBuilder::new("OneShot")
            .add_zone(ZoneBuilder::new(0, 500).key(36))
            .build(Notes::default());

        assert_eq!(inst.groups.len(), 1);
        assert_eq!(inst.groups[0].zones.len(), 1);
        assert_eq!(inst.groups[0].zones[0].keys, 36..=36);
        assert_eq!(inst.groups[0].zones[0].root, 36);
    }
}
