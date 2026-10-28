//! MIDI 1.0 channel messages from the host's Universal MIDI Packets.

use std::sync::Arc;

use crate::{Engine, Kind, Running};

impl Engine {
    /// MIDI 1.0 channel messages: controllers feed behaviour (ranges, live
    /// crossfades) and the sustain pedal. Notes arrive as [`Event::NoteOn`].
    pub(crate) fn ump(&mut self, [word, _]: [u32; 2]) {
        if word >> 28 != 0x2 {
            return;
        }
        let status = (word >> 16) as u8;
        let channel = status & 0x0F;
        if matches!(status & 0xF0, 0xB0 | 0xE0) {
            self.behaviour.channel = channel;
        }
        if status & 0xF0 == 0xE0 {
            // 14 bits: LSB in data 1, MSB in data 2; 8192 is centre.
            let raw = (word & 0x7F) << 7 | (word >> 8 & 0x7F);
            let bend = (raw as f32 - 8192.0) / 8192.0;
            self.behaviour.set_bend(channel, bend.max(-1.0));
            let mut hits = std::mem::take(&mut self.hits);
            self.notes.select(&mut hits, |n| n.host.channel == channel);
            for &note in &hits {
                self.refresh_note(note, None);
            }
            self.hits = hits;
            return;
        }
        if status & 0xF0 != 0xB0 {
            return;
        }
        let (controller, value) = ((word >> 8) as u8 & 0x7F, word as u8 & 0x7F);
        if let Some(v) = self.cc_values.get_mut(usize::from(controller)) {
            *v = value;
        }
        let plan = Arc::clone(&self.plan);
        for (slot, script) in (0u8..).zip(&plan.scripts) {
            let Some(entry) = script.callbacks.controller else {
                continue;
            };
            let running = Running {
                controller,
                ..Running::new(Kind::Controller, None)
            };
            if self.run_callback(slot, entry, running).ignore_controller {
                return;
            }
        }
        self.behaviour.set_cc(channel, controller, value);
        let mut hits = std::mem::take(&mut self.hits);
        if controller == 64 {
            let down = value >= 64;
            if let Some(p) = self.pedal.get_mut(usize::from(channel)) {
                *p = down;
            }
            if !down {
                self.notes
                    .select(&mut hits, |n| n.pedal_held && n.host.channel == channel);
                for &note in &hits {
                    self.release(note);
                }
            }
        }
        self.notes.select(&mut hits, |n| n.host.channel == channel);
        for &note in &hits {
            self.refresh_note(note, Some(controller));
        }
        self.hits = hits;
    }
}
