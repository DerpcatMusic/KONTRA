//! The immutable instrument the engine plays, built off the audio thread by a
//! format translator (02-ENGINE.md §6).

use std::ops::RangeInclusive;

use kontra_voice::{Ahdsr, Region};

/// One playable zone, resolved: no format knowledge is left here.
#[derive(Clone)]
pub struct Zone {
    pub keys: RangeInclusive<u8>,
    /// MIDI velocities 1..=127 the zone answers.
    pub velocities: RangeInclusive<u8>,
    /// The key at which the sample plays at its recorded pitch.
    pub root: u8,
    /// Fine tune in semitones.
    pub tune: f64,
    /// The sample's own rate, for the step against the engine rate.
    pub sample_rate: u32,
    pub region: Region,
    /// Linear gain at full velocity.
    pub gain: f32,
    /// −1 (left) ..= 1 (right).
    pub pan: f32,
    /// Attenuation in dB at velocity 0, falling linearly to 0 dB at full velocity.
    pub velocity_db: f32,
    pub envelope: Ahdsr,
}

/// Pool sizes and zones. Growing a pool is a new plan, not a resize.
#[derive(Clone)]
pub struct Plan {
    pub zones: Vec<Zone>,
    pub voices: usize,
    pub notes: usize,
    /// Pending timed work; also bounds one block's input.
    pub queue: usize,
}
