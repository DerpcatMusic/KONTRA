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

    /// Makes room for one more voice in voice group `vg`, before it starts.
    pub(crate) fn limit_voice_group(&mut self, vg: u32) {
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
        let playing = self
            .voices
            .iter()
            .filter(|s| voice_group(s) == Some(vg))
            .count();
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
        if let Some(index) = victim {
            self.steal(index, fade);
        }
    }
}
