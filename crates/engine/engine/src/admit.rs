//! The note table and the voice pool: a full one makes room instead of
//! refusing a note (a burst of short notes must never go silent), and notes
//! that went silent leave it.

use crate::ledger::{Note, NoteRef, Origin};
use crate::{Engine, Slot, Trigger};

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
                    .any(|s| s.note == Some(r) && s.voice.is_active() && !s.fading);
                let held = n.origin == Origin::Host && !n.is_up();
                (sounding || held, !n.released, n.at)
            })
            .map(|(r, _)| r)?;
        let fade = self.fade_frames();
        for (_index, slot) in self
            .voices
            .iter_mut()
            .enumerate()
            .filter(|(_, s)| s.note == Some(victim) && s.voice.is_active() && !s.fading)
        {
            #[cfg(feature = "trace")]
            if let Some(trace) = &mut self.trace {
                trace.voice(self.now.0, _index, slot, crate::VoiceEvent::Steal);
            }
            slot.voice.fade(fade);
            slot.fading = true;
            self.stats.voices_stolen += 1;
        }
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
            let playing = |s: &Slot| s.voice.is_active() && !s.fading;
            let victim = self
                .voices
                .iter()
                .enumerate()
                .filter(|(_, s)| playing(s))
                .min_by_key(|(_, s)| (!s.released, s.started))
                .map(|(i, s)| (i, s.note, group(s)));
            if let Some((v, note, of)) = victim {
                for (i, s) in self.voices.iter_mut().enumerate() {
                    let sibling = plan.steal_whole_groups
                        && note.is_some()
                        && s.note == note
                        && group(s) == of;
                    if playing(s) && (i == v || sibling) {
                        #[cfg(feature = "trace")]
                        if let Some(trace) = &mut self.trace {
                            trace.voice(self.now.0, i, s, crate::VoiceEvent::Steal);
                        }
                        s.voice.fade(fade);
                        s.fading = true;
                        self.stats.voices_stolen += 1;
                    }
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
                    .min_by_key(|(_, s)| (!s.fading, s.started))?;
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
}
