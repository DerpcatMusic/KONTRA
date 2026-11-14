//! Voice limits shared across groups: a full group gives up a voice by its
//! steal rule, and an exclusive set silences the other voice groups in it.

use kontra_ir as ir;

use crate::{Engine, NoteRef, Slot};

impl Engine {
    /// Fades voices that a new voice of `group` chokes (`off_by`), except the note's own.
    pub(crate) fn choke_by(&mut self, group: u32, note: NoteRef) {
        let fade = self.fade_frames();
        let plan = self.plan.clone();
        self.steal_where(fade, |s| {
            s.note != Some(note)
                && plan
                    .zones
                    .get(s.zone)
                    .is_some_and(|z| z.off_by == Some(group))
        });
    }

    /// Makes room for one more voice of `note` in voice group `vg`, before
    /// it starts.
    pub(crate) fn limit_voice_group(&mut self, vg: u32, note: NoteRef) {
        let plan = self.plan.clone();
        let Some(group) = plan.voice_groups.get(vg as usize) else {
            return;
        };
        // A stolen voice fades over its group's time, at least 1 ms.
        let fade = (group.fade_ms.max(1.0) * self.rate as f32 / 1000.0).round() as u32;
        let voice_group = |s: &Slot| {
            (s.voice.is_active() && !s.voice.is_ending())
                .then(|| plan.zones.get(s.zone)?.voice_group)
                .flatten()
        };
        if let Some(set) = group.exclusive {
            self.steal_where(fade, |s| {
                let other = voice_group(s)
                    .filter(|&o| o != vg)
                    .and_then(|o| plan.voice_groups.get(o as usize));
                other.is_some_and(|o| o.exclusive == Some(set))
            });
        }
        // Notes other than this one (its earlier zones are not rivals), or voices.
        let playing = if group.per_note {
            self.note_stamp = self.note_stamp.wrapping_add(1);
            if self.note_stamp == 0 {
                self.note_seen.fill(0);
                self.note_stamp = 1;
            }
            let mut notes = 0;
            for n in (self.voices.iter())
                .filter(|s| voice_group(s) == Some(vg))
                .filter_map(|s| s.note.filter(|&n| n != note))
            {
                if let Some(seen) = self.note_seen.get_mut(n.slot())
                    && *seen != self.note_stamp
                {
                    *seen = self.note_stamp;
                    notes += 1;
                }
            }
            notes
        } else {
            (self.voices.iter())
                .filter(|s| voice_group(s) == Some(vg))
                .count()
        };
        if playing < usize::from(group.voices) {
            return;
        }
        let key = |s: &Slot| {
            s.note
                .and_then(|n| self.notes.get(n))
                .map_or(0, |n| i64::from(n.key))
        };
        let victim = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, s)| voice_group(s) == Some(vg))
            .filter(|(_, s)| !group.per_note || s.note != Some(note))
            .min_by_key(|(_, s)| {
                let order = match group.steal {
                    ir::Steal::Oldest => s.started.0 as i64,
                    ir::Steal::Newest => -(s.started.0 as i64),
                    ir::Steal::Highest => -key(s),
                    ir::Steal::Lowest => key(s),
                };
                (!(group.prefer_released && s.voice.is_released()), order)
            })
            .map(|(i, _)| i);
        let Some(index) = victim else {
            return;
        };
        // A counted note goes whole: every voice of it in the group.
        let owner = self.voices.get(index).and_then(|s| s.note);
        for i in 0..self.voices.len() {
            let whole = group.per_note
                && self
                    .voices
                    .get(i)
                    .is_some_and(|s| s.note == owner && voice_group(s) == Some(vg));
            if i == index || whole {
                self.steal(i, fade);
            }
        }
    }
}
