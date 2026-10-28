//! The host's transport as scripts see it: tempo, whether it runs, the song
//! position at 960 ticks per quarter and the time signature.

use kontra_vm::{Action, HostValue};

use crate::Engine;

/// Song-position ticks per quarter note.
const PPQ: f64 = 960.0;
// Measured, probes/callback_constants.lua.
const TRANSP_START: u8 = 1;
const TRANSP_STOP: u8 = 2;

pub(crate) struct Transport {
    playing: bool,
    ticks: f64,
    signature: (u8, u8),
    /// Per script slot, bit `signal`: its listener hears transport start or
    /// stop.
    listens: Box<[u8]>,
}

impl Transport {
    pub(crate) fn new(slots: usize) -> Self {
        Self {
            playing: false,
            ticks: 0.0,
            signature: (4, 4),
            listens: vec![0; slots].into(),
        }
    }
}

impl Engine {
    /// The host's tempo in quarter notes per minute; ignored unless finite
    /// and positive.
    pub fn set_tempo(&mut self, bpm: f32) {
        if bpm.is_finite() && bpm > 0.0 {
            self.tempo = bpm;
        }
    }

    /// The host's transport at the start of the next block: whether it runs,
    /// the song position in quarter notes and the time signature. Starting or
    /// stopping runs the listener callbacks that asked for it.
    pub fn set_transport(&mut self, playing: bool, quarters: f64, signature: (u8, u8)) {
        if quarters.is_finite() {
            self.transport.ticks = quarters.max(0.0) * PPQ;
        }
        if signature.0 > 0 && signature.1 > 0 {
            self.transport.signature = signature;
        }
        if playing == self.transport.playing {
            return;
        }
        self.transport.playing = playing;
        let signal = if playing { TRANSP_START } else { TRANSP_STOP };
        for slot in 0..self.transport.listens.len() {
            let hears = self
                .transport
                .listens
                .get(slot)
                .is_some_and(|bits| bits & 1 << signal != 0);
            if let (true, Ok(slot)) = (hears, u8::try_from(slot)) {
                self.run_listener(slot, signal);
            }
        }
    }

    /// `set_listener` for a transport signal; other signals do nothing.
    pub(crate) fn listen_transport(&mut self, slot: u8, signal: u8, on: bool) {
        if !matches!(signal, TRANSP_START | TRANSP_STOP) {
            return;
        }
        if let Some(bits) = self.transport.listens.get_mut(usize::from(slot)) {
            *bits = if on {
                *bits | 1 << signal
            } else {
                *bits & !(1 << signal)
            };
        }
    }

    /// Moves the song position on by `frames` while the transport runs.
    pub(crate) fn advance_transport(&mut self, frames: usize) {
        if self.transport.playing {
            self.transport.ticks +=
                frames as f64 * f64::from(self.tempo) / 60.0 * PPQ / f64::from(self.rate);
        }
    }

    /// The transport's answer to a script's `action`, or `None` if it is not
    /// a transport query. `arg` is the call's first integer.
    pub(crate) fn transport_value(&self, action: Action, arg: i32) -> Option<HostValue> {
        let t = &self.transport;
        // set_tempo keeps the tempo finite and positive.
        let quarter = 60e6 / f64::from(self.tempo);
        let int =
            |v: f64| HostValue::Int(v.round().clamp(-2_147_483_648.0, 2_147_483_647.0) as i32);
        let (num, denom) = t.signature;
        Some(match action {
            Action::TransportRunning => HostValue::Int(i32::from(t.playing)),
            Action::SongPosition => int(t.ticks.floor()),
            Action::SignatureNum => HostValue::Int(i32::from(num)),
            Action::SignatureDenom => HostValue::Int(i32::from(denom)),
            Action::Bpm | Action::Tempo => int(f64::from(self.tempo)),
            Action::MsToTicks => int(f64::from(arg) / quarter * PPQ),
            Action::TicksToMs => int(f64::from(arg) / PPQ * quarter),
            // The manual: a bar's duration is 0 while the clock stands.
            Action::DurationBar if !t.playing => HostValue::Int(0),
            Action::DurationBar => int(quarter * 4.0 * f64::from(num) / f64::from(denom)),
            Action::DurationQuarter => int(quarter),
            Action::DurationEighth => int(quarter / 2.0),
            Action::DurationSixteenth => int(quarter / 4.0),
            Action::DurationQuarterTriplet => int(quarter * 2.0 / 3.0),
            Action::DurationEighthTriplet => int(quarter / 3.0),
            Action::DurationSixteenthTriplet => int(quarter / 6.0),
            _ => return None,
        })
    }
}
