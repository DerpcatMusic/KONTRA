//! Plain mixer settings and meters shared by the shell and every core.

use super::{BUSES, RACK_SLOTS};
use crate::fx::OUTS;

/// [`PartControls::aux`] when the part sends nowhere.
pub const NO_AUX: u8 = u8::MAX;


#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartControls {
    pub port: u8,
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
    /// Per output channel the instrument plays to past its own output (a
    /// mic mixer's "Out 2"), the bus it goes to; [`NO_AUX`] joins `output`.
    pub outs: [u8; OUTS],
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
}

impl Default for Mix {
    fn default() -> Self {
        Self {
            parts: vec![PartControls::default(); RACK_SLOTS],
            buses: std::array::from_fn(|n| BusControls::on(n as u8)),
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
            outs: [NO_AUX; OUTS],
        }
    }
}

