//! Control-thread compilation of immutable resident assets and native mappings.
mod predicates;
mod selection;
use super::{Envelope, Error, Frame, NotePitch, Playback};
pub use predicates::ControllerCondition;
use predicates::Matching;

/// Validated immutable resident PCM. Construct, clone and drop handles on the
/// control side. Clones share the original sample buffer; rendering only borrows.
#[derive(Clone, Debug)]
pub struct Pcm(std::sync::Arc<PcmData>);

#[derive(Debug)]
struct PcmData {
    rate: u32,
    frames: Box<[Frame]>,
}
impl Pcm {
    /// Validate once without copying the owned frame buffer. All public access is
    /// immutable, so subsequent prepared plans need not rescan sample contents.
    pub fn new(rate: u32, frames: Box<[Frame]>) -> Result<Self, Error> {
        if rate == 0 || frames.is_empty() || frames.iter().flatten().any(|x| !x.is_finite()) {
            return Err(Error::InvalidInput);
        }
        Ok(Self(std::sync::Arc::new(PcmData { rate, frames })))
    }
    pub fn sample_rate(&self) -> u32 {
        self.0.rate
    }
    pub fn frames(&self) -> &[Frame] {
        &self.0.frames
    }
}

/// Native tuning offsets in semitones from the nominal 12-tone equal-tempered
/// pitch of each logical key. Zero preserves authored tuning. Compile off audio;
/// adoption affects new roots, while existing notes and children keep their plan.
#[derive(Clone, Debug)]
pub struct Tuning([f64; 128]);

impl Tuning {
    pub fn new(offsets_semitones: [f64; 128]) -> Result<Self, Error> {
        if offsets_semitones.iter().any(|offset| !offset.is_finite()) {
            return Err(Error::InvalidInput);
        }
        Ok(Self(offsets_semitones))
    }

    pub fn offsets_semitones(&self) -> &[f64; 128] {
        &self.0
    }
}

impl Default for Tuning {
    fn default() -> Self {
        Self([0.0; 128])
    }
}

/// Native resident region with optional tuned root-key tracking.
/// PCM can be shared by regions with independent ranges, directions and loops.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub sample: usize,
    pub key_low: u8,
    pub key_high: u8,
    /// Nominal recorded root key. Tracking includes the played key's tuning offset;
    /// None keeps the source at its authored pitch and bypasses the tuning table.
    pub root_key: Option<u8>,
    pub velocity_low: f64,
    pub velocity_high: f64,
    pub gain: f32,
    pub envelope: Envelope,
    pub playback: Playback,
}

/// Render-ready region: authoring bounds and playback metadata have been compiled.
#[derive(Clone, Copy)]
struct PreparedRegion {
    sample: usize,
    velocity_low: f64,
    velocity_high: f64,
    gain: f32,
    envelope: Envelope,
    cursor: super::source::Cursor,
    root_key: Option<u8>,
    transpose_semitones: f64,
    take: Option<super::Take>,
    trigger: super::Trigger,
    articulation: Option<u32>,
    conditions: Option<usize>,
}

#[derive(Clone, Copy)]
struct Candidate {
    region: usize,
    step: f64,
}

pub struct Prepared {
    pub(super) rate: u32,
    pub(super) pcm: Box<[Pcm]>,
    regions: Box<[PreparedRegion]>,
    offsets: [usize; 129],
    phase_offsets: [[usize; 2]; 128],
    pub(super) release_options: [super::ReleaseOptions; 2],
    pub(super) release_reserves: [[super::ReleaseReserve; 2]; 128],
    candidates: Box<[Candidate]>,
    pub(super) programs: Box<[super::Program]>,
    note_program: Option<usize>,
    pub(super) release_program: Option<usize>,
    pub(super) note_cells: usize,
    pub(super) controls: Box<[super::ControlDefinition]>,
    pub(super) control_programs: Box<[(super::ControlId, usize)]>,
    keyswitches: [Option<u32>; 128],
    articulated: bool,
    conditions: Box<[Box<[ControllerCondition]>]>,
    condition_ends: Box<[usize]>,
    pub(super) release_selection: [super::SelectionPolicy; 2],
    pub(super) modulation: super::Modulation,
    pub(super) sequences: Box<[super::variation::PreparedSequence]>,
    pub(super) sequence_cells: usize,
    pub(super) shuffle_entries: usize,
}

impl Prepared {
    /// Takes immutable PCM handles without copying/rescanning their frames. Region
    /// validation and candidate construction happen off audio. max_candidates
    /// bounds the expanded key index, not file IO.
    pub fn new(
        rate: u32,
        pcm: Vec<Pcm>,
        regions: Vec<Region>,
        max_candidates: usize,
    ) -> Result<Self, Error> {
        Self::new_tuned(rate, pcm, regions, max_candidates, &Tuning::default())
    }

    /// Compile a tuning table into key candidates, with no table lookup or pitch
    /// calculation added to rendering. The recorded root remains nominal: tuning
    /// a played root key changes its pitch too. Fixed-pitch regions are exempt.
    pub fn new_tuned(
        rate: u32,
        pcm: Vec<Pcm>,
        regions: Vec<Region>,
        max_candidates: usize,
        tuning: &Tuning,
    ) -> Result<Self, Error> {
        if rate == 0 {
            return Err(Error::InvalidInput);
        }
        let mut count = 0usize;
        let mut prepared_regions = Vec::new();
        for r in &regions {
            if r.sample >= pcm.len()
                || r.key_low > r.key_high
                || r.key_high >= 128
                || r.root_key.is_some_and(|key| key >= 128)
                || !r.velocity_low.is_finite()
                || !r.velocity_high.is_finite()
                || r.velocity_low < 0.0
                || r.velocity_high > 1.0
                || r.velocity_low > r.velocity_high
                || !r.gain.is_finite()
                || !(0.0..=1.0).contains(&r.gain)
            {
                return Err(Error::InvalidInput);
            }
            let mut playback = r.playback;
            if let Some(root) = r.root_key {
                playback.transpose_semitones +=
                    f64::from(r.key_low) - f64::from(root) + tuning.0[r.key_low as usize];
            }
            let cursor = playback.cursor(
                pcm[r.sample].frames().len(),
                pcm[r.sample].sample_rate(),
                rate,
            )?;
            count = count
                .checked_add(usize::from(r.key_high - r.key_low) + 1)
                .ok_or(Error::Capacity)?;
            if count > max_candidates {
                return Err(Error::Capacity);
            }
            prepared_regions.push(PreparedRegion {
                sample: r.sample,
                velocity_low: r.velocity_low,
                velocity_high: r.velocity_high,
                gain: r.gain,
                envelope: r.envelope,
                cursor,
                root_key: r.root_key,
                transpose_semitones: r.playback.transpose_semitones,
                take: None,
                trigger: super::Trigger::Attack,
                articulation: None,
                conditions: None,
            });
        }
        let mut offsets = [0; 129];
        let mut candidates = Vec::with_capacity(count);
        for key in 0..128 {
            offsets[key as usize] = candidates.len();
            for (region, r) in regions.iter().enumerate() {
                if r.key_low <= key && key <= r.key_high {
                    let step = if let Some(root) = r.root_key {
                        let mut playback = r.playback;
                        playback.transpose_semitones +=
                            f64::from(key) - f64::from(root) + tuning.0[key as usize];
                        playback.step(pcm[r.sample].sample_rate(), rate)
                    } else {
                        prepared_regions[region].cursor.step()
                    };
                    if !(super::resample::MIN_STEP..=super::resample::MAX_STEP).contains(&step) {
                        return Err(Error::InvalidInput);
                    }
                    candidates.push(Candidate { region, step });
                }
            }
        }
        offsets[128] = candidates.len();
        Ok(Self {
            rate,
            pcm: pcm.into_boxed_slice(),
            regions: prepared_regions.into_boxed_slice(),
            phase_offsets: std::array::from_fn(|key| [offsets[key + 1]; 2]),
            release_options: [super::ReleaseOptions::default(); 2],
            release_reserves: [[super::ReleaseReserve::default(); 2]; 128],
            offsets,
            candidates: candidates.into_boxed_slice(),
            programs: Box::new([]),
            note_program: None,
            release_program: None,
            note_cells: 0,
            controls: Box::new([]),
            control_programs: Box::new([]),
            keyswitches: [None; 128],
            articulated: false,
            conditions: Box::new([]),
            condition_ends: Box::new([]),
            release_selection: [super::SelectionPolicy::Onset; 2],
            modulation: super::Modulation::default(),
            sequences: Box::new([]),
            sequence_cells: 0,
            shuffle_entries: 0,
        })
    }

    /// Replace the complete program table and optional note binding atomically on
    /// the control thread. Generated children select regions without re-entry.
    pub fn with_programs(
        mut self,
        programs: Vec<super::Program>,
        note_program: Option<usize>,
    ) -> Result<Self, Error> {
        if note_program.is_some_and(|index| index >= programs.len()) {
            return Err(Error::InvalidInput);
        }
        self.validate_program_controls(&programs)?;
        self.note_cells = programs.iter().map(|p| p.note_cells).max().unwrap_or(0);
        self.programs = programs.into_boxed_slice();
        self.note_program = note_program;
        self.release_program = None;
        self.control_programs = Box::new([]);
        Ok(self)
    }

    /// Bind a physical-key release callback to triggered external inputs. It has
    /// an independently reserved continuation and may wait beyond gate closure.
    /// Replacing the complete program table clears this binding.
    pub fn with_release_program(mut self, program: usize) -> Result<Self, Error> {
        if !self
            .programs
            .get(program)
            .is_some_and(|p| p.wait_lifetime == super::WaitLifetime::Callback)
        {
            return Err(Error::InvalidInput);
        }
        self.release_program = Some(program);
        Ok(self)
    }

    /// Bind a validated note-scoped modulation program on the control thread.
    pub fn with_modulation(mut self, modulation: super::Modulation) -> Self {
        self.modulation = modulation;
        self
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }
    pub fn sample_count(&self) -> usize {
        self.pcm.len()
    }
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    fn step(&self, candidate: Candidate, pitch: NotePitch) -> f64 {
        let r = &self.regions[candidate.region];
        match (pitch, r.root_key) {
            (NotePitch::Absolute(pitch), Some(root)) => {
                let semitones = r.transpose_semitones + (pitch - f64::from(root));
                f64::from(self.pcm[r.sample].sample_rate()) / f64::from(self.rate)
                    * super::pitch::ratio(semitones)
            }
            _ => candidate.step,
        }
    }

    /// Attach one optional take to each region in its original authoring order.
    /// Candidates are grouped by sequence without expanding a key/channel/take
    /// Cartesian product. Every sequence advances once per accepted eligible note,
    /// including a deliberately unmapped take; completely unmatched notes do not.
    /// `max_states` bounds reserved scope cells; `max_shuffle_entries` bounds the
    /// total u32 bag entries (reserved owners times takes for shuffle sequences).
    pub fn with_variation(
        mut self,
        sequences: Vec<super::Sequence>,
        region_takes: Vec<Option<super::Take>>,
        max_states: usize,
        max_shuffle_entries: usize,
    ) -> Result<Self, Error> {
        if region_takes.len() != self.regions.len() {
            return Err(Error::InvalidInput);
        }
        let mut prepared = Vec::with_capacity(sequences.len());
        let mut cells = 0usize;
        let mut entries = 0usize;
        for mut spec in sequences {
            if spec.takes == 0
                || spec.capacity == 0
                || (matches!(spec.policy, super::TakePolicy::NoRepeat { .. }) && spec.takes < 2)
            {
                return Err(Error::InvalidInput);
            }
            spec.capacity = match spec.scope {
                super::SequenceScope::Global => 1,
                super::SequenceScope::Key => spec.capacity.min(128),
                _ => spec.capacity,
            };
            let offset = cells;
            cells = cells.checked_add(spec.capacity).ok_or(Error::Capacity)?;
            if cells > max_states {
                return Err(Error::Capacity);
            }
            let shuffle_offset = entries;
            if matches!(spec.policy, super::TakePolicy::Shuffle { .. }) {
                let count = spec
                    .capacity
                    .checked_mul(spec.takes as usize)
                    .ok_or(Error::Capacity)?;
                entries = entries.checked_add(count).ok_or(Error::Capacity)?;
                if entries > max_shuffle_entries {
                    return Err(Error::Capacity);
                }
            }
            prepared.push(super::variation::PreparedSequence {
                spec,
                offset,
                shuffle_offset,
            });
        }
        super::variation::SequenceState::check_size(cells, entries)?;
        for (r, take) in self.regions.iter_mut().zip(region_takes) {
            if let Some(take) = take
                && prepared
                    .get(take.sequence)
                    .is_none_or(|s| take.index >= s.spec.takes)
            {
                return Err(Error::InvalidInput);
            }
            r.take = take;
        }
        self.sequences = prepared.into_boxed_slice();
        self.sequence_cells = cells;
        self.shuffle_entries = entries;
        self.compile_selection();
        Ok(self)
    }

    /// Attach native attack/key-release/gate-release phases in authoring order.
    /// Release families choose their own takes and have independent source lifetimes.
    pub fn with_releases(
        mut self,
        triggers: Vec<super::Trigger>,
        key_release: super::ReleaseOptions,
        gate_release: super::ReleaseOptions,
    ) -> Result<Self, Error> {
        if triggers.len() != self.regions.len() {
            return Err(Error::InvalidInput);
        }
        for options in [key_release, gate_release] {
            if let super::ReleaseVelocity::KeyUp { fallback } = options.velocity {
                super::release::validate_velocity(Some(fallback))?;
            }
        }
        self.release_options = [key_release, gate_release];
        for (region, trigger) in self.regions.iter_mut().zip(triggers) {
            if let Some(index) = trigger.release_index()
                && self.release_options[index].duration.is_none()
                && region.cursor.looping()
            {
                return Err(Error::InvalidInput);
            }
            region.trigger = trigger;
        }
        self.compile_selection();
        Ok(self)
    }

    /// Tag regions in original authoring order. None is an unconditional layer.
    /// Policies independently choose onset/current articulation for key/gate releases.
    pub fn with_articulations(
        mut self,
        regions: Vec<Option<u32>>,
        switches: Vec<super::Keyswitch>,
        key_release: super::SelectionPolicy,
        gate_release: super::SelectionPolicy,
    ) -> Result<Self, Error> {
        if regions.len() != self.regions.len() {
            return Err(Error::InvalidInput);
        }
        let mut keyswitches = [None; 128];
        for switch in switches {
            let slot = keyswitches
                .get_mut(usize::from(switch.key))
                .ok_or(Error::InvalidInput)?;
            if slot.replace(switch.articulation).is_some() {
                return Err(Error::InvalidInput);
            }
        }
        self.articulated = regions.iter().any(Option::is_some);
        for (region, value) in self.regions.iter_mut().zip(regions) {
            region.articulation = value;
        }
        self.keyswitches = keyswitches;
        self.release_selection = [key_release, gate_release];
        self.compile_selection();
        Ok(self)
    }

    /// Current release state is unknown until its actual transition.
    pub(super) fn pending_selection<'a>(
        &self,
        trigger: super::Trigger,
        onset: &'a super::performance::State,
    ) -> Option<&'a super::performance::State> {
        match self.release_selection[trigger.release_index().unwrap()] {
            super::SelectionPolicy::Onset => Some(onset),
            super::SelectionPolicy::Current => None,
        }
    }

    // Two sparse ranges per sequence: unconditional regions plus the selected tag.
    // Do not expand keys x articulations x takes x microphones into another table.
    fn active_ranges(
        &self,
        range: std::ops::Range<usize>,
        articulation: u32,
    ) -> [std::ops::Range<usize>; 2] {
        if !self.articulated {
            return [range, 0..0];
        }
        let candidates = &self.candidates[range.clone()];
        let common = candidates.partition_point(|c| self.regions[c.region].articulation.is_none());
        let begin = candidates
            .partition_point(|c| self.regions[c.region].articulation < Some(articulation));
        let end = candidates
            .partition_point(|c| self.regions[c.region].articulation <= Some(articulation));
        [
            range.start..range.start + common,
            range.start + begin..range.start + end,
        ]
    }

    fn compile_selection(&mut self) {
        for key in 0..128 {
            let begin = self.offsets[key];
            let end = self.offsets[key + 1];
            let candidates = &mut self.candidates[begin..end];
            candidates.sort_by_key(|c| {
                let r = self.regions[c.region];
                (
                    r.trigger,
                    r.take.map(|t| t.sequence),
                    r.articulation,
                    r.conditions,
                    c.region,
                )
            });
            self.phase_offsets[key] = [
                begin
                    + candidates.partition_point(|c| {
                        self.regions[c.region].trigger == super::Trigger::Attack
                    }),
                begin
                    + candidates.partition_point(|c| {
                        self.regions[c.region].trigger != super::Trigger::GateRelease
                    }),
            ];
            for trigger in [super::Trigger::KeyRelease, super::Trigger::GateRelease] {
                self.release_reserves[key][trigger.release_index().unwrap()] =
                    self.release_bound(key as u8, trigger);
            }
        }
        if self.conditions.is_empty() {
            self.condition_ends = Box::new([]);
        } else {
            let mut ends = vec![0; self.candidates.len()];
            let mut end = ends.len();
            let mut previous = None;
            for i in (0..ends.len()).rev() {
                let condition = self.regions[self.candidates[i].region].conditions;
                if previous != Some(condition) {
                    end = i + 1;
                    previous = Some(condition);
                }
                ends[i] = end;
            }
            self.condition_ends = ends.into_boxed_slice();
        }
    }

    fn range(&self, key: u8, trigger: super::Trigger) -> std::ops::Range<usize> {
        let key = usize::from(key);
        match trigger {
            super::Trigger::Attack => self.offsets[key]..self.phase_offsets[key][0],
            super::Trigger::KeyRelease => self.phase_offsets[key][0]..self.phase_offsets[key][1],
            super::Trigger::GateRelease => self.phase_offsets[key][1]..self.offsets[key + 1],
        }
    }

    fn release_bound(&self, key: u8, trigger: super::Trigger) -> super::ReleaseReserve {
        use std::collections::BTreeMap;
        // Control-only sweep: closed velocity intervals overlap at shared endpoints.
        // Track each take's overlap and each group's maximum, without expanding
        // key x velocity x take x microphone products or scanning every velocity.
        #[derive(Default)]
        struct TakeOverlap {
            common: usize,
            counts: BTreeMap<u32, usize>,
            levels: BTreeMap<usize, usize>,
        }
        fn change(levels: &mut BTreeMap<usize, usize>, old: usize, new: usize) {
            if old > 0 {
                let frequency = levels.get_mut(&old).unwrap();
                *frequency -= 1;
                if *frequency == 0 {
                    levels.remove(&old);
                }
            }
            if new > 0 {
                *levels.entry(new).or_default() += 1;
            }
        }
        struct Group {
            counts: BTreeMap<u32, TakeOverlap>,
            levels: BTreeMap<usize, usize>,
            sequenced: bool,
        }
        let range = self.range(key, trigger);
        let mut events = Vec::with_capacity(range.len() * 2);
        let mut groups = Vec::new();
        let mut from = range.start;
        while from < range.end {
            let until = self.group_end(from, range.end);
            for candidate in &self.candidates[from..until] {
                let r = self.regions[candidate.region];
                events.push((
                    r.velocity_low,
                    true,
                    groups.len(),
                    r.take.map_or(0, |t| t.index),
                    r.articulation,
                ));
                events.push((
                    r.velocity_high,
                    false,
                    groups.len(),
                    r.take.map_or(0, |t| t.index),
                    r.articulation,
                ));
            }
            groups.push(Group {
                counts: BTreeMap::new(),
                levels: BTreeMap::new(),
                sequenced: self.regions[self.candidates[from].region].take.is_some(),
            });
            from = until;
        }
        events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then_with(|| b.1.cmp(&a.1)));
        let mut active = super::ReleaseReserve::default();
        let mut maximum = active;
        for (_, start, group, take, articulation) in events {
            let group = &mut groups[group];
            let old_maximum = group.levels.last_key_value().map_or(0, |(&level, _)| level);
            let take = group.counts.entry(take).or_default();
            let old = take.common + take.levels.last_key_value().map_or(0, |(&n, _)| n);
            if let Some(articulation) = articulation {
                let count = take.counts.entry(articulation).or_default();
                let before = *count;
                *count = if start { before + 1 } else { before - 1 };
                change(&mut take.levels, before, *count);
            } else {
                take.common = if start {
                    take.common + 1
                } else {
                    take.common - 1
                };
            }
            let new = take.common + take.levels.last_key_value().map_or(0, |(&n, _)| n);
            change(&mut group.levels, old, new);
            let new_maximum = group.levels.last_key_value().map_or(0, |(&level, _)| level);
            active.voices = active.voices - old_maximum + new_maximum;
            if old_maximum == 0 && new_maximum != 0 {
                active.families += 1;
                active.decisions += usize::from(group.sequenced);
            } else if new_maximum == 0 && old_maximum != 0 {
                active.families -= 1;
                active.decisions -= usize::from(group.sequenced);
            }
            active.commands = if self.release_options[trigger.release_index().unwrap()]
                .duration
                .is_some_and(|n| n > 0)
            {
                active.families
            } else {
                0
            };
            maximum = maximum.maximum(active);
        }
        if !self.conditions.is_empty() {
            maximum.voices = maximum
                .voices
                .min(self.controller_release_bound(key, trigger));
        }
        maximum
    }

    pub(super) fn validate_release_pitch(
        &self,
        pitch: NotePitch,
        trigger: super::Trigger,
        range: super::pitch::PitchRange,
        velocity: Option<f64>,
        state: Option<&super::performance::State>,
    ) -> Result<(), Error> {
        let candidates = self.range(pitch.key(), trigger);
        let mut from = candidates.start;
        while from < candidates.end {
            let until = self.group_end(from, candidates.end);
            let ranges = state.map_or([from..until, 0..0], |state| {
                self.active_ranges(from..until, state.articulation)
            });
            let mut matching = Matching::new(ranges);
            while let Some(candidate) = matching.next(self, state, velocity) {
                range.apply(self.step(candidate, pitch))?;
            }
            from = until;
        }
        Ok(())
    }

    // None means physical release velocity is not known yet.
    pub(super) fn release_velocity(
        &self,
        trigger: super::Trigger,
        onset: f64,
        key_released: bool,
        velocity: Option<f64>,
    ) -> Option<f64> {
        match self.release_options[trigger.release_index().unwrap()].velocity {
            super::ReleaseVelocity::Onset => Some(onset),
            super::ReleaseVelocity::KeyUp { fallback } => {
                key_released.then_some(velocity.unwrap_or(fallback))
            }
        }
    }

    fn sequence_groups<'a>(
        &'a self,
        key: u8,
        trigger: super::Trigger,
        state: Option<&'a super::performance::State>,
        velocity: Option<f64>,
    ) -> impl Iterator<Item = usize> + 'a {
        let mut range = self.range(key, trigger);
        std::iter::from_fn(move || {
            if range.is_empty() {
                return None;
            }
            let until = self.group_end(range.start, range.end);
            let ranges = state.map_or([range.start..until, 0..0], |state| {
                self.active_ranges(range.start..until, state.articulation)
            });
            let eligible = Matching::new(ranges).next(self, state, velocity).is_some();
            let sequence = self.regions[self.candidates[range.start].region]
                .take
                .map(|t| t.sequence)
                .filter(|_| eligible);
            range.start = until;
            Some(sequence)
        })
        .flatten()
    }

    fn group_end(&self, begin: usize, end: usize) -> usize {
        if self.sequences.is_empty() {
            return end;
        }
        let sequence = self.regions[self.candidates[begin].region]
            .take
            .map(|t| t.sequence);
        begin
            + self.candidates[begin..end]
                .partition_point(|c| self.regions[c.region].take.map(|t| t.sequence) == sequence)
    }

    fn choose(
        &self,
        first: Option<Candidate>,
        address: super::ChannelAddress,
        key: u8,
        state: &super::variation::SequenceState,
    ) -> Result<Option<super::variation::PendingTake>, Error> {
        let Some(take) = first.and_then(|c| self.regions[c.region].take) else {
            return Ok(None);
        };
        state
            .choose(take.sequence, &self.sequences[take.sequence], address, key)
            .map(Some)
    }
}
