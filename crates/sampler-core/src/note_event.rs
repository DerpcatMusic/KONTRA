//! Script-visible note properties, independent of physical input and committed audio.
use super::{Error, NoteId, NotePitch, Runtime};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteProperties {
    pub pitch: NotePitch,
    pub velocity: f64,
}

#[derive(Clone, Copy)]
pub(super) struct NoteEvent {
    pub initial: NoteProperties,
    pub current: NoteProperties,
}

impl NoteEvent {
    pub(super) fn new(pitch: NotePitch, velocity: f64) -> Self {
        let initial = NoteProperties { pitch, velocity };
        Self {
            initial,
            current: initial,
        }
    }
}

impl Runtime {
    /// Admission properties, including full-resolution velocity and absolute pitch.
    /// Generated notes capture their own admission, not their parent's input.
    pub fn initial_note_properties(&self, note: NoteId) -> Result<NoteProperties, Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        Ok(self.note_events[note.0.index].initial)
    }

    /// Current script-visible properties; late edits do not change committed audio.
    pub fn note_event(&self, note: NoteId) -> Result<NoteProperties, Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        Ok(self.note_events[note.0.index].current)
    }

    /// Edit the event view atomically. A pending attack consumes this view when
    /// forwarded. Already committed voices/releases and physical pairing stay intact.
    pub fn edit_note_event(&mut self, note: NoteId, value: NoteProperties) -> Result<(), Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if !value.pitch.valid()
            || !value.velocity.is_finite()
            || !(0.0..=1.0).contains(&value.velocity)
        {
            return Err(Error::InvalidInput);
        }
        self.note_events[note.0.index].current = value;
        Ok(())
    }
}
