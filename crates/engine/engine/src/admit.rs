//! The note table: a full one makes room instead of refusing a note (a burst of
//! short notes must never go silent), and notes that went silent leave it.

use crate::ledger::{Note, NoteRef, Origin};
use crate::{Engine, Trigger};

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
        for slot in self
            .voices
            .iter_mut()
            .filter(|s| s.note == Some(victim) && s.voice.is_active() && !s.fading)
        {
            slot.voice.fade(fade);
            slot.fading = true;
            self.stats.voices_stolen += 1;
        }
        if let Some(n) = self.notes.remove(victim) {
            self.ended.push(n.host);
        }
        self.notes.insert(note)
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
                    .is_some_and(|n| n.origin == Origin::Host && !n.is_up());
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
