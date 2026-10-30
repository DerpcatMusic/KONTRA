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
        // A controller tied to a script control moves it before scripts hear it.
        for (slot, script) in (0u8..).zip(&plan.scripts) {
            for link in script.links.iter().filter(|l| l.cc == controller) {
                self.set_control(slot, link.id, link.value(value));
            }
        }
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
        self.output_controller(controller, value);
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

    /// The output's controllers set the instrument's fader and pan.
    fn output_controller(&mut self, cc: u8, value: u8) {
        let Some(c) = self.plan.output.controllers else {
            return;
        };
        let v = f32::from(value);
        if cc == c.volume {
            let (exponent, top) = (c.power, crate::ir::common::DB_GAIN.eval_unclamped(c.top_db));
            let taper = crate::ir::Law::IntegerPower {
                exponent,
                bipolar: false,
            };
            self.instrument.gain = top * taper.eval_unclamped(v / 127.0);
        }
        if Some(cc) == c.expression {
            self.expression = v / 127.0;
        }
        if cc == c.pan {
            self.instrument.pan = ((v - 63.5) / 63.5).clamp(-1.0, 1.0);
        }
    }
}
