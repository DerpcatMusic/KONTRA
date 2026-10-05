//! Control-thread compilation of immutable resident assets and native mappings.
use super::{
    Envelope, Error, Expression, Frame, Input, NoteId, NoteOrigin, NotePitch, Playback, Runtime,
};

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
    candidates: Box<[Candidate]>,
    pub(super) programs: Box<[super::Program]>,
    note_program: Option<usize>,
    pub(super) modulation: super::Modulation,
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
            offsets,
            candidates: candidates.into_boxed_slice(),
            programs: Box::new([]),
            note_program: None,
            modulation: super::Modulation::default(),
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
        self.programs = programs.into_boxed_slice();
        self.note_program = note_program;
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

    fn matches(
        &self,
        pitch: NotePitch,
        velocity: f64,
    ) -> impl Iterator<Item = (&PreparedRegion, f64)> {
        let key = pitch.key();
        self.candidates[self.offsets[key as usize]..self.offsets[key as usize + 1]]
            .iter()
            .map(move |candidate| {
                (
                    &self.regions[candidate.region],
                    self.step(*candidate, pitch),
                )
            })
            .filter(move |(r, _)| r.velocity_low <= velocity && velocity <= r.velocity_high)
    }
}

impl Runtime {
    /// Select all matching native layers and admit them as one family. Preflight
    /// reserves the entire selection conceptually before publishing the note; no
    /// partial layer set sounds when capacity is exhausted. No match is a logical
    /// no-source note, still paired with its key-up and terminal acceptance.
    pub fn trigger(&mut self, input: Input, key: u8, velocity: f64) -> Result<NoteId, Error> {
        self.trigger_with_expression(input, key, velocity, Expression::default())
    }

    /// Admit a complete initial expression before selection or a bound note program.
    /// Failed native source preflight publishes neither an input nor partial layers.
    pub fn trigger_with_expression(
        &mut self,
        input: Input,
        key: u8,
        velocity: f64,
        expression: Expression,
    ) -> Result<NoteId, Error> {
        self.trigger_pitched(input, NotePitch::Key(key), velocity, expression)
    }

    /// Select regions using inherent pitch, preserving the independent input address.
    /// Absolute pitch bypasses per-key tuning; expression remains a relative offset.
    pub fn trigger_pitched(
        &mut self,
        input: Input,
        pitch: NotePitch,
        velocity: f64,
        expression: Expression,
    ) -> Result<NoteId, Error> {
        self.apply_due();
        if !expression.valid() {
            return Err(Error::InvalidInput);
        }
        if let Some(program) = self
            .plans
            .get(self.active_plan.0)
            .unwrap()
            .prepared
            .note_program
        {
            if self.behaviors.available() == 0 {
                return Err(Error::Capacity);
            }
            let note = self.note_on_pitched(input, pitch, velocity, expression)?;
            self.start_behavior(note, program)
                .expect("preflighted native behavior admission");
            Ok(note)
        } else {
            self.select(NoteOrigin::Input(input, expression), pitch, velocity)
        }
    }

    pub(super) fn trigger_child(
        &mut self,
        parent: NoteId,
        pitch: NotePitch,
        velocity: f64,
        linked: bool,
        inheritance: super::Inheritance,
    ) -> Result<NoteId, Error> {
        self.select(
            NoteOrigin::Child(parent, linked, inheritance),
            pitch,
            velocity,
        )
    }

    fn select(
        &mut self,
        origin: NoteOrigin,
        note_pitch: NotePitch,
        velocity: f64,
    ) -> Result<NoteId, Error> {
        self.apply_due();
        if !note_pitch.valid() || !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) {
            return Err(Error::InvalidInput);
        }
        let plan = match origin {
            NoteOrigin::Input(..) => self.active_plan,
            NoteOrigin::Child(parent, ..) => {
                self.notes.get(parent.0).ok_or(Error::StaleHandle)?.plan
            }
        };
        let pitch = match origin {
            NoteOrigin::Input(_, expression) => super::pitch::PitchRange::constant(
                self.project_expression(self.modulation_plan(plan), expression, None)?
                    .ratio,
            ),
            NoteOrigin::Child(_, _, super::Inheritance::Independent) => {
                super::pitch::PitchRange::constant(
                    self.project_expression(
                        self.modulation_plan(plan),
                        Expression::default(),
                        None,
                    )?
                    .ratio,
                )
            }
            NoteOrigin::Child(parent, _, inheritance) => {
                let owner = self.notes.get(parent.0).unwrap().expression;
                self.pitch_range(owner, inheritance == super::Inheritance::Linked)?
            }
        };
        let mut count = 0;
        for (_, step) in self
            .plans
            .get(plan.0)
            .unwrap()
            .prepared
            .matches(note_pitch, velocity)
        {
            pitch.apply(step)?;
            count += 1;
        }
        if count > self.voices.available() || (count != 0 && self.families.available() == 0) {
            return Err(Error::Capacity);
        }
        let note = match origin {
            NoteOrigin::Input(input, expression) => {
                self.note_on_pitched(input, note_pitch, velocity, expression)?
            }
            NoteOrigin::Child(parent, linked, inheritance) => {
                self.child_pitched(parent, note_pitch, velocity, linked, inheritance)?
            }
        };
        if count == 0 {
            return Ok(note);
        }
        // Everything below was validated by Prepared and preflight. No callbacks,
        // concurrent writers or newly due work can consume the reserved resources.
        let family = self.create_family(note).expect("preflight family capacity");
        let key = note_pitch.key();
        let prepared = &self.plans.get(plan.0).unwrap().prepared;
        let begin = prepared.offsets[key as usize];
        let end = prepared.offsets[key as usize + 1];
        for i in begin..end {
            let prepared = &self.plans.get(plan.0).unwrap().prepared;
            let candidate = prepared.candidates[i];
            let r = prepared.regions[candidate.region];
            if r.velocity_low <= velocity && velocity <= r.velocity_high {
                let step = prepared.step(candidate, note_pitch);
                self.admit_voice(
                    family,
                    r.sample,
                    self.now,
                    r.gain * velocity as f32,
                    r.envelope,
                    r.cursor.with_step(step),
                )
                .expect("prepared and preflighted source admission");
            }
        }
        self.finish_family(family).expect("admitted family");
        Ok(note)
    }
}
