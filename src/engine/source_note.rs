//! Source-facing physical ownership, key-up and pedal policy. A copied snapshot
//! carries the same decisions without mutating a live voice.

use super::{HostRef, pedals::Pedals, voice::Voice};

/// A source-facing voice snapshot, not an additional live note registry. The
/// physical owner key can differ from the played note after routing or KSP.
#[derive(Clone, Copy)]
pub(super) struct SourceNote {
    pub channel: u8,
    pub note: u8,
    pub owner: Option<(u8, u8)>,
    pub input_channel: Option<u8>,
    pub host_note: Option<HostRef>,
    pub held: bool,
    pub captured: bool,
    pub released: bool,
    pub release_trigger: bool,
}

impl SourceNote {
    pub(super) fn input(&self) -> u8 { self.input_channel.unwrap_or(self.channel) }

    /// An anonymous MIDI key-up cannot release an exact host root. Physical
    /// ownership wins over the played key, with the direct-event fallback kept.
    pub(super) fn matches_key_up(&self, channel: u8, note: u8, owner: Option<u8>) -> bool {
        self.host_note.is_none() && self.channel == channel
            && owner.map_or(self.note == note, |owner|
                self.owner.map_or(self.note == note, |input| input == (owner, note)))
            && self.held && !self.released && !self.release_trigger
    }

    /// Accept an event Off and return whether it releases the source now.
    pub(super) fn key_up(&mut self, pedals: &Pedals) -> bool {
        if self.released || self.release_trigger { return false; }
        self.held = false;
        // The existing direct event path masks its input channel here.
        if pedals.holds(self.input() & 15, false, self.captured) { return false; }
        self.released = true;
        true
    }

    /// Only the selected physical input's unheld, uncaptured attack releases.
    pub(super) fn pedal_up(&mut self, input: u8, pedals: &Pedals) -> bool {
        if self.input() != input || self.released || self.release_trigger
            || pedals.holds(input, self.held, self.captured) { return false; }
        self.released = true;
        true
    }

    /// Down captures only ordinary held attacks. Up clears every clone's
    /// capture before the same effective-Off policy is evaluated.
    pub(super) fn sostenuto(&mut self, input: u8, down: bool) {
        if self.input() != input { return; }
        if !down { self.captured = false; }
        else if self.held && !self.released && !self.release_trigger { self.captured = true; }
    }
}

impl Voice {
    pub(super) fn source_note(&self) -> SourceNote {
        SourceNote { channel: self.channel, note: self.note, owner: self.owner,
            input_channel: self.input_channel, host_note: self.host_note,
            held: self.held, captured: self.sostenuto, released: self.released,
            release_trigger: self.release_trigger }
    }

    pub(super) fn source_key_up(&mut self, pedals: &Pedals) -> bool {
        let mut note = self.source_note();
        let release = note.key_up(pedals);
        self.held = note.held;
        // Voice::release commits released alongside its other live source and
        // sample state; a projected clone keeps the returned state directly.
        release
    }

    pub(super) fn source_pedal_up(&self, input: u8, pedals: &Pedals) -> bool {
        self.source_note().pedal_up(input, pedals)
    }

    pub(super) fn source_sostenuto(&mut self, input: u8, down: bool) {
        let mut note = self.source_note();
        note.sostenuto(input, down);
        self.sostenuto = note.captured;
    }
}
