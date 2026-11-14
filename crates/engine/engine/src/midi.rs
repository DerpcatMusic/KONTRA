//! MIDI 1.0 channel messages from the host's Universal MIDI Packets.

use std::sync::Arc;

use crate::{Engine, Kind, Running, SustainPedal};

/// The controller numbers scripts hear pitch bend and channel pressure as
/// (Kontakt's `$VCC_PITCH_BEND` and `$VCC_MONO_AT`).
pub(crate) const VCC_PITCH_BEND: u8 = 128;
pub(crate) const VCC_MONO_AT: u8 = 129;

impl Engine {
    /// MIDI 1.0 channel messages: controllers feed behaviour (ranges, live
    /// crossfades) and the sustain pedal. Notes arrive as [`Event::NoteOn`].
    pub(crate) fn ump(&mut self, [word, _]: [u32; 2]) {
        if word >> 28 != 0x2 {
            return;
        }
        let status = (word >> 16) as u8;
        let channel = status & 0x0F;
        if matches!(status & 0xF0, 0xB0 | 0xD0 | 0xE0) {
            self.behaviour.channel = channel;
        }
        if status & 0xF0 == 0xE0 {
            // 14 bits: LSB in data 1, MSB in data 2; 8192 is centre.
            let raw = (word & 0x7F) << 7 | (word >> 8 & 0x7F);
            let bend = (raw as f32 - 8192.0) / 8192.0;
            let before = self.behaviour.bend(channel);
            self.behaviour.set_bend(channel, bend.max(-1.0));
            // Scripts hear it first, as controller 128 (Kontakt's
            // `$VCC_PITCH_BEND`, UVI's `onPitchBend`); one may keep it.
            match self.run_controllers(VCC_PITCH_BEND) {
                Heard::Ignored => {
                    self.behaviour.set_bend(channel, before);
                    return;
                }
                // The script's own bend stands, set and heard already.
                Heard::Replaced => return,
                Heard::Passed => {}
            }
            let mut hits = std::mem::take(&mut self.hits);
            self.notes.select(&mut hits, |n| n.host.channel == channel);
            self.refresh_voices(None, |slot, _| slot.note.is_some_and(|n| hits.contains(&n)));
            self.hits = hits;
            return;
        }
        if status & 0xF0 == 0xA0 {
            self.poly_aftertouch(channel, (word >> 8) as u8 & 0x7F, word as u8 & 0x7F);
            return;
        }
        // Channel pressure: every note on the channel (MPE: its one note).
        if status & 0xF0 == 0xD0 {
            let value = (word >> 8) as u8 & 0x7F;
            let before = self.cc_values.get(usize::from(VCC_MONO_AT)).copied();
            if let Some(v) = self.cc_values.get_mut(usize::from(VCC_MONO_AT)) {
                *v = value;
            }
            // Controller 129 (`$VCC_MONO_AT`, UVI's `onAfterTouch`).
            match self.run_controllers(VCC_MONO_AT) {
                Heard::Ignored => {
                    if let (Some(v), Some(before)) =
                        (self.cc_values.get_mut(usize::from(VCC_MONO_AT)), before)
                    {
                        *v = before;
                    }
                    return;
                }
                Heard::Replaced => return,
                Heard::Passed => {}
            }
            self.behaviour.set_pressure(channel, value);
            let mut hits = std::mem::take(&mut self.hits);
            self.notes.select(&mut hits, |n| n.host.channel == channel);
            self.refresh_voices(None, |slot, _| slot.note.is_some_and(|n| hits.contains(&n)));
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
        for k in plan.knob_mods.iter().filter(|k| k.cc == controller) {
            self.set_control_mod(
                k.slot,
                k.id,
                f64::from(k.saved),
                f64::from(k.ratio),
                f64::from(value) / 127.0,
            );
        }
        if self.run_controllers(controller) != Heard::Passed {
            return;
        }
        self.behaviour.set_cc(channel, controller, value);
        self.output_controller(controller, value);
        let mut hits = std::mem::take(&mut self.hits);
        if controller == 64 {
            let down = value >= 64 && self.plan.sustain_pedal == SustainPedal::Holds;
            if let Some(p) = self.pedal.get_mut(usize::from(channel)) {
                *p = down;
            }
            if !down {
                self.notes
                    .select(&mut hits, |n| n.pedal_held && n.host.channel == channel);
                // Kontakt runs `on release` again at pedal-up for every note
                // the pedal held, after `on controller` (oracle, sine probe,
                // Kontakt 8, 2026-11-13); one that ignores it keeps playing.
                for &note in &hits {
                    if let Some(slot) = self.release_callbacks(note) {
                        if let Some(n) = self.notes.get_mut(note) {
                            n.pedal_held = false;
                        }
                        let script = self.plan.scripts.get(usize::from(slot));
                        if script.is_some_and(|s| s.held_release_ends_children) {
                            self.end_children(note);
                        }
                    } else {
                        self.release(note);
                    }
                }
            }
        }
        self.notes.select(&mut hits, |n| n.host.channel == channel);
        // One pass over the voices, not one per note.
        self.refresh_voices(Some(controller), |slot, _| {
            slot.note.is_some_and(|n| hits.contains(&n))
        });
        self.hits = hits;
    }

    /// Each script's `on controller` for `controller`, until one ignores
    /// it (or ignores it and sets its own value).
    fn run_controllers(&mut self, controller: u8) -> Heard {
        let plan = Arc::clone(&self.plan);
        for (slot, script) in (0u8..).zip(&plan.scripts) {
            let Some(entry) = script.callbacks.controller else {
                continue;
            };
            let running = Running {
                controller,
                ..Running::new(Kind::Controller, None)
            };
            let heard = Heard::of(&self.run_callback(slot, entry, running));
            if heard != Heard::Passed {
                return heard;
            }
        }
        Heard::Passed
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

    /// A key's polyphonic aftertouch as it arrives: kept for `%POLY_AT`,
    /// each script's `on poly_at` runs with the key as `$POLY_AT_NUM`, and
    /// the key's notes take it unless a script ignored it.
    pub(crate) fn poly_aftertouch(&mut self, channel: u8, key: u8, value: u8) {
        let at = usize::from(key);
        let before = self.poly_at.get(at).copied();
        if let Some(v) = self.poly_at.get_mut(at) {
            *v = value;
        }
        match self.run_poly_at(key) {
            Heard::Ignored => {
                if let (Some(v), Some(before)) = (self.poly_at.get_mut(at), before) {
                    *v = before;
                }
            }
            Heard::Replaced => {}
            Heard::Passed => self.set_poly_at(channel, key, value),
        }
    }

    /// Key `key`'s polyphonic aftertouch on `channel`, set without a
    /// script hearing it (a script's own, or one that passed them).
    pub(crate) fn set_poly_at(&mut self, channel: u8, key: u8, value: u8) {
        if let Some(v) = self.poly_at.get_mut(usize::from(key)) {
            *v = value;
        }
        if self.running.kind == Kind::PolyAt && self.running.controller == key {
            self.running.set_own = true;
        }
        // The key's notes on the channel take it as their own pressure.
        let mut hits = std::mem::take(&mut self.hits);
        self.notes
            .select(&mut hits, |n| n.host.channel == channel && n.key == key);
        for &note in &hits {
            if let Some(n) = self.notes.get_mut(note) {
                n.expression_pressure = Some(f32::from(value) / 127.0);
            }
        }
        self.refresh_voices(None, |slot, _| slot.note.is_some_and(|n| hits.contains(&n)));
        self.hits = hits;
    }

    /// Each script's `on poly_at` for `key`, its value already in
    /// `%POLY_AT`, until one ignores it.
    pub(crate) fn run_poly_at(&mut self, key: u8) -> Heard {
        let plan = Arc::clone(&self.plan);
        for (slot, script) in (0u8..).zip(&plan.scripts) {
            if let Some(entry) = script.callbacks.poly_at {
                let running = Running {
                    controller: key,
                    ..Running::new(Kind::PolyAt, None)
                };
                let heard = Heard::of(&self.run_callback(slot, entry, running));
                if heard != Heard::Passed {
                    return heard;
                }
            }
        }
        Heard::Passed
    }
}

/// What the scripts made of a controller (or aftertouch) they heard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Heard {
    /// It goes on to the instrument.
    Passed,
    /// A script kept it from the instrument (`ignore_controller`).
    Ignored,
    /// A script kept it and set its own value instead (Kontakt's
    /// `ignore_controller` with `set_controller`, UVI's changed event posted).
    Replaced,
}

impl Heard {
    pub(crate) fn of(ran: &Running) -> Self {
        match (ran.ignore_controller, ran.set_own) {
            (false, _) => Self::Passed,
            (true, false) => Self::Ignored,
            (true, true) => Self::Replaced,
        }
    }
}
