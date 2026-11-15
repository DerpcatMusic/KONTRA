//! One note at a time in a voice group ([`ir::VoiceGroup::legato`],
//! [`ir::VoiceGroup::returns`]): a legato note takes over the sounding
//! note's voices, and releasing the newest note brings back an older key
//! still down. docs/engine/voices.md.

use kontra_ir as ir;

use crate::glide::Slide;
use crate::{Engine, NoteRef, Trigger};

/// Voice groups with a line; a higher one plays as an ordinary group.
pub(crate) const LINES: usize = 64;
/// Older keys a line remembers; past it the oldest is forgotten.
const HELD: usize = 8;

/// A mono voice group's state.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Line {
    /// The note sounding the group.
    note: Option<NoteRef>,
    /// That note took its voices over from the one before: its own zones in
    /// the group stay silent.
    took: bool,
    /// Older notes whose keys were down when a newer one came, newest last.
    held: [Option<NoteRef>; HELD],
}

impl Line {
    fn push(&mut self, note: NoteRef) {
        if self.held.iter().all(Option::is_some) {
            self.held.rotate_left(1);
            if let Some(last) = self.held.last_mut() {
                *last = None;
            }
        }
        if let Some(free) = self.held.iter_mut().find(|h| h.is_none()) {
            *free = Some(note);
        }
    }

    fn forget(&mut self, note: NoteRef) {
        for h in &mut self.held {
            if *h == Some(note) {
                *h = None;
            }
        }
        // Keep them packed, newest last.
        let mut packed = [None; HELD];
        for (to, from) in packed.iter_mut().zip(self.held.iter().flatten()) {
            *to = Some(*from);
        }
        self.held = packed;
    }

    fn pop(&mut self) -> Option<NoteRef> {
        let last = self.held.iter_mut().rev().find(|h| h.is_some())?;
        last.take()
    }
}

impl Engine {
    fn mono_group(&self, vg: usize) -> Option<ir::VoiceGroup> {
        (self.voice_groups.get(vg).copied()).filter(|g| g.legato || g.returns)
    }

    /// At a zone of `note` in voice group `vg`, before it starts: whether it
    /// stays silent because the note took over the group's sounding voices
    /// (`slide`: how they move to it).
    pub(crate) fn mono_line(&mut self, vg: u32, note: NoteRef, slide: Option<Slide>) -> bool {
        let vg = vg as usize;
        let (Some(group), Some(&line)) = (self.mono_group(vg), self.lines.get(vg)) else {
            return false;
        };
        if line.note == Some(note) {
            return line.took;
        }
        let owner = line.note.filter(|&o| self.holds(vg, o));
        let mut line = Line {
            note: Some(note),
            took: false,
            ..line
        };
        if let Some(owner) = owner {
            if group.returns {
                line.push(owner);
            }
            if group.legato {
                self.hand_over(vg, owner, note, slide);
                line.took = true;
            }
        }
        if let Some(l) = self.lines.get_mut(vg) {
            *l = line;
        }
        line.took
    }

    /// Before `note`'s voices release: in each line it sounds, the newest
    /// older key still down comes back; a line it only waited in forgets it.
    pub(crate) fn mono_release(&mut self, note: NoteRef) {
        for vg in 0..self.lines.len().min(self.voice_groups.len()) {
            let (Some(group), Some(&line)) = (self.mono_group(vg), self.lines.get(vg)) else {
                continue;
            };
            let mut line = line;
            line.forget(note);
            if line.note == Some(note) && group.returns {
                let back = std::iter::from_fn(|| line.pop())
                    .find(|&h| self.notes.get(h).is_some_and(|n| !n.is_up()));
                if let Some(back) = back {
                    line.note = Some(back);
                    line.took = group.legato && self.holds(vg, note);
                    if let Some(l) = self.lines.get_mut(vg) {
                        *l = line;
                    }
                    if line.took {
                        let slide = self.slide_back(vg, note, back);
                        self.hand_over(vg, note, back, slide);
                    } else {
                        // Started again in this group alone; the limit
                        // fades the released note's voices.
                        self.only_voice_group = Some(vg as u32);
                        self.start_zones(back, true, |t| t == Trigger::Attack);
                        self.only_voice_group = None;
                    }
                    continue;
                }
            }
            if let Some(l) = self.lines.get_mut(vg) {
                *l = line;
            }
        }
    }

    /// Whether a line still remembers `note` as a key to come back to.
    pub(crate) fn waits_in_line(&self, note: NoteRef) -> bool {
        self.lines.iter().any(|l| l.held.contains(&Some(note)))
    }

    /// The slide of `vg`'s voices of `from` going back to `to`.
    fn slide_back(&mut self, vg: usize, from: NoteRef, to: NoteRef) -> Option<Slide> {
        let group = (self.voices.iter())
            .filter(|s| s.note == Some(from))
            .filter_map(|s| self.plan.zones.get(s.zone))
            .find(|z| z.voice_group == Some(vg as u32))?
            .group;
        let key = self.notes.get(to)?.key;
        self.slide_for(group, to, key)
    }

    /// Whether `note` holds voices in `vg`: sounding, its key down.
    fn holds(&self, vg: usize, note: NoteRef) -> bool {
        self.voices.iter().any(|s| {
            s.note == Some(note)
                && s.voice.is_active()
                && !s.voice.is_released()
                && !s.voice.is_ending()
                && self
                    .plan
                    .zones
                    .get(s.zone)
                    .is_some_and(|z| z.voice_group == Some(vg as u32))
        })
    }

    /// `from`'s held voices in `vg` become `to`'s and move to its pitch.
    fn hand_over(&mut self, vg: usize, from: NoteRef, to: NoteRef, slide: Option<Slide>) {
        let plan = self.plan.clone();
        for slot in self.voices.iter_mut().filter(|s| {
            s.note == Some(from)
                && s.voice.is_active()
                && !s.voice.is_released()
                && plan
                    .zones
                    .get(s.zone)
                    .is_some_and(|z| z.voice_group == Some(vg as u32))
        }) {
            slot.note = Some(to);
            slot.slide = slide;
        }
        self.refresh_note(to, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(n: u32) -> NoteRef {
        // Generation 1 (the high half), slot n.
        NoteRef::from_event_id((1 << 16) | n as i32).unwrap()
    }

    #[test]
    fn a_line_remembers_keys_newest_last_and_forgets_released_ones() {
        let mut line = Line::default();
        for n in 0..10 {
            line.push(note(n));
        }
        // Eight kept: 2..=9.
        line.forget(note(9));
        assert_eq!(line.pop(), Some(note(8)));
        line.forget(note(2));
        assert_eq!(line.pop(), Some(note(7)));
        assert_eq!(line.held.iter().flatten().count(), 4);
    }
}
