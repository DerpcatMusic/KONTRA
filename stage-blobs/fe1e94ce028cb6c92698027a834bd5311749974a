//! Plain mixer settings and meters shared by the shell and every core.

use super::tree::NodeMix;
use super::{BUSES, RACK_SLOTS};

/// [`PartControls::aux`] when the part sends nowhere.
pub const NO_AUX: u8 = u8::MAX;


#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartControls {
    /// Host MIDI/note input port.
    pub port: u8,
    /// The DAW pair (output bus) the instrument plays to.
    pub output: u8,
    /// MIDI channel, or −1 for omni.
    pub channel: i16,
    pub gain: f32,
    pub pan: f32,
    /// Semitones, cents as the fraction, within ±[`TUNE_RANGE`].
    pub tune: f32,
    pub mute: bool,
    pub solo: bool,
    /// Bus the part also sends to, post-fader, or [`NO_AUX`].
    pub aux: u8,
    /// Linear gain of that send.
    pub aux_gain: f32,
    /// MPE lower zone: manager channel 1, every other channel a member whose
    /// bend, pressure and timbre reach only its note. Takes every channel.
    pub mpe: bool,
    /// Pitch-bend range in semitones (members' in MPE); 0 keeps the default.
    pub bend_range: u8,
    /// [`crate::plugin::Part::switching`].
    pub switching: u8,
}

/// An output bus's fader: what it does to everything routed to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BusControls {
    /// Linear.
    pub gain: f32,
    /// Balance, −1..=1.
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// Host stereo output port (0..[`BUSES`]) the bus plays through.
    pub port: u8,
}

impl BusControls {
    /// Unity, centred, on host port `port`.
    pub fn on(port: u8) -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            port,
        }
    }
}

/// Everything the mixer sets, handed to the audio thread in one piece.
#[derive(Clone, Debug, PartialEq)]
pub struct Mix {
    pub parts: Vec<PartControls>,
    pub buses: [BusControls; BUSES],
    /// Per part, its tree's nodes after the root ([`super::tree`]).
    pub nodes: Vec<Vec<NodeMix>>,
}

impl Default for Mix {
    fn default() -> Self {
        Self {
            parts: vec![PartControls::default(); RACK_SLOTS],
            buses: std::array::from_fn(|n| BusControls::on(n as u8)),
            nodes: vec![Vec::new(); RACK_SLOTS],
        }
    }
}

/// Absolute sample peaks `[left, right]` since last taken: parts post-fader,
/// buses post-fader.
#[derive(Clone, Debug, PartialEq)]
pub struct Peaks {
    pub parts: Vec<[f32; 2]>,
    pub buses: [[f32; 2]; BUSES],
}

impl Default for Peaks {
    fn default() -> Self {
        Self { parts: vec![[0.0; 2]; RACK_SLOTS], buses: [[0.0; 2]; BUSES] }
    }
}

impl Default for PartControls {
    fn default() -> Self {
        Self {
            port: 0,
            output: 0,
            channel: -1,
            gain: 1.0,
            pan: 0.0,
            tune: 0.0,
            mute: false,
            solo: false,
            aux: NO_AUX,
            aux_gain: 0.0,
            mpe: false,
            bend_range: 0,
            switching: 0,
        }
    }
}


/// Left/right gains of a linear `gain` at balance `pan` (−1..=1); silence
/// for non-finite input.
pub fn balance(gain: f32, pan: f32) -> [f32; 2] {
    if !gain.is_finite() || !pan.is_finite() {
        return [0.0; 2];
    }
    let pan = pan.clamp(-1.0, 1.0);
    [gain * (1.0 - pan.max(0.0)), gain * (1.0 + pan.min(0.0))]
}

/// Linear gain of a fader at `db`, within −60..=+6 dB; unity if not finite.
pub fn db_gain(db: f32) -> f32 {
    if db.is_finite() { 10f32.powf(db.clamp(-60.0, 6.0) / 20.0) } else { 1.0 }
}
