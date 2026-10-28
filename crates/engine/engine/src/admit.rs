//! A full note table makes room instead of refusing a note: a burst of
//! short notes must never go silent while voices are free to play it.

use crate::Engine;
use crate::ledger::{Note, NoteRef, Origin};

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
}
