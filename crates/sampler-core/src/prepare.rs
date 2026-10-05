//! Control-thread compilation of immutable resident assets and native mappings.
use super::{Envelope, Error, Frame, Input, NoteId, Playback, Runtime};

#[derive(Clone, Debug)]
pub struct Pcm {
    pub rate: u32,
    pub frames: Box<[Frame]>,
}

/// Native resident region with optional equal-tempered root-key tracking.
/// PCM can be shared by regions with independent ranges, directions and loops.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub sample: usize,
    pub key_low: u8,
    pub key_high: u8,
    /// Equal-tempered key tracking; None keeps the source at its authored pitch.
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
}

impl Prepared {
    /// Takes ownership without copying PCM. Validation and candidate construction
    /// happen off audio. max_candidates bounds the expanded key index, not file IO.
    pub fn new(
        rate: u32,
        pcm: Vec<Pcm>,
        regions: Vec<Region>,
        max_candidates: usize,
    ) -> Result<Self, Error> {
        if rate == 0
            || pcm.iter().any(|p| {
                p.rate == 0
                    || p.frames.is_empty()
                    || p.frames.iter().flatten().any(|x| !x.is_finite())
            })
        {
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
                playback.transpose_semitones += f64::from(r.key_low) - f64::from(root);
            }
            let cursor = playback.cursor(pcm[r.sample].frames.len(), pcm[r.sample].rate, rate)?;
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
                        playback.transpose_semitones += f64::from(key) - f64::from(root);
                        playback.step(pcm[r.sample].rate, rate)
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

    fn matches(&self, key: u8, velocity: f64) -> impl Iterator<Item = &PreparedRegion> {
        self.candidates[self.offsets[key as usize]..self.offsets[key as usize + 1]]
            .iter()
            .map(|candidate| &self.regions[candidate.region])
            .filter(move |r| r.velocity_low <= velocity && velocity <= r.velocity_high)
    }
}

enum Origin {
    Input(Input),
    Child(NoteId, bool, super::Inheritance),
}

impl Runtime {
    /// Select all matching native layers and admit them as one family. Preflight
    /// reserves the entire selection conceptually before publishing the note; no
    /// partial layer set sounds when capacity is exhausted. No match is a logical
    /// no-source note, still paired with its key-up and terminal acceptance.
    pub fn trigger(&mut self, input: Input, key: u8, velocity: f64) -> Result<NoteId, Error> {
        self.apply_due();
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
            let note = self.note_on(input, key, velocity)?;
            self.start_behavior(note, program)
                .expect("preflighted native behavior admission");
            Ok(note)
        } else {
            self.select(Origin::Input(input), key, velocity)
        }
    }

    pub(super) fn trigger_child(
        &mut self,
        parent: NoteId,
        key: u8,
        velocity: f64,
        linked: bool,
        inheritance: super::Inheritance,
    ) -> Result<NoteId, Error> {
        self.select(Origin::Child(parent, linked, inheritance), key, velocity)
    }

    fn select(&mut self, origin: Origin, key: u8, velocity: f64) -> Result<NoteId, Error> {
        self.apply_due();
        if key >= 128 || !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) {
            return Err(Error::InvalidInput);
        }
        let plan = match origin {
            Origin::Input(_) => self.active_plan,
            Origin::Child(parent, ..) => self.notes.get(parent.0).ok_or(Error::StaleHandle)?.plan,
        };
        let count = self
            .plans
            .get(plan.0)
            .unwrap()
            .prepared
            .matches(key, velocity)
            .count();
        if count > self.voices.available() || (count != 0 && self.families.available() == 0) {
            return Err(Error::Capacity);
        }
        let note = match origin {
            Origin::Input(input) => self.note_on(input, key, velocity)?,
            Origin::Child(parent, linked, inheritance) => {
                self.child(parent, key, velocity, linked, inheritance)?
            }
        };
        if count == 0 {
            return Ok(note);
        }
        // Everything below was validated by Prepared and preflight. No callbacks,
        // concurrent writers or newly due work can consume the reserved resources.
        let family = self.create_family(note).expect("preflight family capacity");
        let prepared = &self.plans.get(plan.0).unwrap().prepared;
        let begin = prepared.offsets[key as usize];
        let end = prepared.offsets[key as usize + 1];
        for i in begin..end {
            let prepared = &self.plans.get(plan.0).unwrap().prepared;
            let candidate = prepared.candidates[i];
            let r = prepared.regions[candidate.region];
            if r.velocity_low <= velocity && velocity <= r.velocity_high {
                self.admit_voice(
                    family,
                    r.sample,
                    self.now,
                    r.gain * velocity as f32,
                    r.envelope,
                    r.cursor.with_step(candidate.step),
                )
                .expect("prepared and preflighted source admission");
            }
        }
        self.finish_family(family).expect("admitted family");
        Ok(note)
    }
}
