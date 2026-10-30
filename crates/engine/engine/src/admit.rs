//! The note table and the voice pool: a full one makes room instead of
//! refusing a note (a burst of short notes must never go silent), and notes
//! that went silent leave it.

use crate::ledger::{Note, NoteRef, Origin};
use crate::{Engine, Slot, Trigger};
use kontra_voice::FadeTarget;

impl Engine {
    /// Enters `note` in the table. When it is full, the note that matters
    /// least gives up its entry: one that no longer sounds, else the oldest
    /// (released first), its voices faded. No child waits on it.
    pub(crate) fn admit(&mut self, note: Note) -> Option<NoteRef> {
        if let Some(r) = self.notes.insert(note) {
            return Some(r);
        }
        let victim = self
            .notes
            .live()
            .filter(|&(r, _)| !self.notes.parents(r))
            .min_by_key(|&(r, n)| {
                let sounding = self
                    .voices
                    .iter()
                    .any(|s| s.note == Some(r) && s.voice.is_active() && !s.voice.is_ending());
                let held = n.origin == Origin::Host && !n.is_up();
                (sounding || held, !n.released, n.at)
            })
            .map(|(r, _)| r)?;
        let fade = self.fade_frames();
        self.steal_where(fade, |s| s.note == Some(victim));
        if let Some(n) = self.notes.remove(victim) {
            self.ended.push(n.host);
        }
        self.notes.insert(note)
    }

    /// An idle voice. At the plan's polyphony the oldest playing voice fades
    /// (released ones first), with its note's others in its group when the
    /// plan steals whole groups, and an idle reserve voice takes the new start.
    pub(crate) fn free_slot(&mut self) -> Option<usize> {
        if self.playing_voices() >= self.plan.voices {
            let fade = self.fade_frames();
            let plan = self.plan.clone();
            let group = |s: &Slot| plan.zones.get(s.zone).map(|z| z.group);
            let playing = |s: &Slot| s.voice.is_active() && !s.voice.is_ending();
            let victim = self
                .voices
                .iter()
                .enumerate()
                .filter(|(_, s)| playing(s))
                .min_by_key(|(_, s)| (!s.voice.is_released(), s.started))
                .map(|(i, s)| (i, s.note, group(s)));
            if let Some((v, note, of)) = victim {
                self.steal(v, fade);
                if plan.steal_whole_groups && note.is_some() {
                    self.steal_where(fade, |s| s.note == note && group(s) == of);
                }
            }
        }
        let index = match self.voices.iter().position(|s| !s.voice.is_active()) {
            Some(i) => i,
            None => {
                // Even the fade reserve is full: cut the oldest fading voice.
                let (i, _) = self
                    .voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, s)| (!s.voice.is_ending(), s.started))?;
                self.stats.voices_cut += 1;
                i
            }
        };
        let slot = self.voices.get_mut(index)?;
        #[cfg(feature = "trace")]
        if slot.voice.is_active()
            && let Some(trace) = &mut self.trace
        {
            trace.voice(self.now.0, index, slot, crate::VoiceEvent::Cut);
        }
        slot.voice.kill();
        Some(index)
    }

    /// Fades voice `index` out over `fade` frames (a running fade ends at the
    /// sooner of the two), after which it is free; `true` the first time only.
    fn fade_voice(&mut self, index: usize, fade: u32) -> bool {
        let Some(slot) = self.voices.get_mut(index).filter(|s| s.voice.is_active()) else {
            return false;
        };
        slot.voice.fade(FadeTarget::End, fade)
    }

    /// Calls `act` on each voice `pick` chooses.
    fn each_voice(&mut self, pick: impl Fn(&Slot) -> bool, mut act: impl FnMut(&mut Self, usize)) {
        for index in 0..self.voices.len() {
            if self.voices.get(index).is_some_and(&pick) {
                act(self, index);
            }
        }
    }

    /// Fades every playing voice `pick` chooses, ended on request, not stolen.
    pub(crate) fn fade_where(&mut self, fade: u32, pick: impl Fn(&Slot) -> bool) {
        self.each_voice(pick, |engine, index| _ = engine.fade_voice(index, fade));
    }

    /// Fades the voices of `notes` to `target`, and the attack voices they
    /// start later; an ending fade releases them.
    pub(crate) fn fade_notes(&mut self, notes: &[NoteRef], frames: u32, target: FadeTarget) {
        for &note in notes {
            if let Some(n) = self.notes.get_mut(note) {
                // A note yet to start starts with the fade and ends at its silence;
                // a stopping fade stays, as a voice's does, unless a shorter one comes.
                n.released |= target == FadeTarget::End && n.started;
                if !matches!(n.fade, Some((FadeTarget::End, left))
                    if target != FadeTarget::End || frames >= left)
                {
                    n.fade = Some((target, frames));
                }
            }
            for slot in self.voices.iter_mut().filter(|s| s.note == Some(note)) {
                slot.voice.fade(target, frames);
            }
        }
        if target == FadeTarget::End {
            // Ending a note ends the notes played for its length: one pass, into
            // a list of its own (`notes` may be the shared scratch).
            let mut children = std::mem::take(&mut self.children_scratch);
            let ledger = &self.notes;
            ledger.select(&mut children, |n| match n.origin {
                Origin::Child(p) => ledger
                    .get(p)
                    .is_some_and(|p| matches!(p.fade, Some((FadeTarget::End, _)))),
                _ => false,
            });
            for &child in &children {
                self.queue_end(child);
            }
            self.children_scratch = children;
        }
    }

    /// Steals every playing voice `pick` chooses.
    pub(crate) fn steal_where(&mut self, fade: u32, pick: impl Fn(&Slot) -> bool) {
        self.each_voice(pick, |engine, index| engine.steal(index, fade));
    }

    /// Steals voice `index` to make room: a counted, traced [`Self::fade_voice`].
    pub(crate) fn steal(&mut self, index: usize, fade: u32) {
        let stolen = self.fade_voice(index, fade);
        #[cfg(feature = "trace")]
        if let (true, Some(trace), Some(slot)) = (stolen, &mut self.trace, self.voices.get(index)) {
            trace.voice(self.now.0, index, slot, crate::VoiceEvent::Steal);
        }
        self.stats.voices_stolen += u64::from(stolen);
    }

    /// Ends notes with no sounding voice and reports them to the host. A key
    /// still down stays alive while the plan has release triggers to play.
    pub(crate) fn end_silent_notes(&mut self) {
        // A held key stays a live event while something may still act on its release.
        let release_triggers = self
            .plan
            .zones
            .iter()
            .any(|z| z.trigger == Trigger::Release)
            || self
                .plan
                .scripts
                .iter()
                .any(|s| s.callbacks.release.is_some());
        let mut hits = std::mem::take(&mut self.hits);
        self.notes.select(&mut hits, |_| true);
        for &note in &hits {
            let sounding = self
                .voices
                .iter()
                .any(|s| s.note == Some(note) && s.voice.is_active());
            let waiting = release_triggers
                && self
                    .notes
                    .get(note)
                    .is_some_and(|n| n.origin == Origin::Host && (!n.is_up() || n.pedal_held));
            if !sounding
                && !waiting
                // A silent parent stays addressable while a child waits on it.
                && !self.notes.parents(note)
                // and its event while a callback for it waits ($EVENT_NOTE
                // after a `wait` in `on release`).
                && !self.has_callback(note)
                && let Some(n) = self.notes.remove(note)
            {
                self.ended.push(n.host);
            }
        }
        self.hits = hits;
        for slot in self.voices.iter_mut().filter(|s| !s.voice.is_active()) {
            slot.note = None;
        }
    }

    /// Whether a script callback started for `note` is still running.
    fn has_callback(&self, note: NoteRef) -> bool {
        let id = note.event_id() as u32;
        self.vms
            .iter()
            .flatten()
            .any(|vm| vm.is_running(|context| context as u32 == id))
    }
}
