//! Control-thread compilation of immutable resident assets and native mappings.
use super::{Envelope, Error, Frame, Input, NoteId, Playback, Runtime};

#[derive(Clone, Debug)]
pub struct Pcm {
    pub rate: u32,
    pub frames: Box<[Frame]>,
}

/// Native fixed-pitch resident region. Playback metadata belongs to the region;
/// PCM can be shared by regions with independent ranges, directions and loops.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub sample: usize,
    pub key_low: u8,
    pub key_high: u8,
    pub velocity_low: f64,
    pub velocity_high: f64,
    pub gain: f32,
    pub envelope: Envelope,
    pub playback: Playback,
}

pub struct Prepared {
    pub(super) rate: u32,
    pub(super) pcm: Box<[Pcm]>,
    regions: Box<[Region]>,
    offsets: [usize; 129],
    candidates: Box<[usize]>,
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
                p.rate != rate
                    || p.frames.is_empty()
                    || p.frames.iter().flatten().any(|x| !x.is_finite())
            })
        {
            return Err(Error::InvalidInput);
        }
        let mut count = 0usize;
        for r in &regions {
            if r.sample >= pcm.len()
                || r.key_low > r.key_high
                || r.key_high >= 128
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
            r.playback.cursor(pcm[r.sample].frames.len())?;
            count = count
                .checked_add(usize::from(r.key_high - r.key_low) + 1)
                .ok_or(Error::Capacity)?;
            if count > max_candidates {
                return Err(Error::Capacity);
            }
        }
        let mut offsets = [0; 129];
        let mut candidates = Vec::with_capacity(count);
        for key in 0..128 {
            offsets[key as usize] = candidates.len();
            candidates.extend(
                regions
                    .iter()
                    .enumerate()
                    .filter_map(|(i, r)| (r.key_low <= key && key <= r.key_high).then_some(i)),
            );
        }
        offsets[128] = candidates.len();
        Ok(Self {
            rate,
            pcm: pcm.into_boxed_slice(),
            regions: regions.into_boxed_slice(),
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

    fn matches(&self, key: u8, velocity: f64) -> impl Iterator<Item = &Region> {
        self.candidates[self.offsets[key as usize]..self.offsets[key as usize + 1]]
            .iter()
            .map(|&i| &self.regions[i])
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
        if let Some(program) = self.plan.note_program {
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
        let count = self.plan.matches(key, velocity).count();
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
        let begin = self.plan.offsets[key as usize];
        let end = self.plan.offsets[key as usize + 1];
        for i in begin..end {
            let r = self.plan.regions[self.plan.candidates[i]];
            if r.velocity_low <= velocity && velocity <= r.velocity_high {
                self.start_family(
                    family,
                    r.sample,
                    self.now,
                    r.gain * velocity as f32,
                    r.envelope,
                    r.playback,
                )
                .expect("prepared and preflighted source admission");
            }
        }
        self.finish_family(family).expect("admitted family");
        Ok(note)
    }
}
