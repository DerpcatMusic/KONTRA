//! Voice limits shared across groups (Kontakt's voice groups): a full group
//! gives up a voice by its steal rule, and an exclusive set silences the
//! other voice groups in it.

use kontra_ir as ir;

use crate::{Engine, Slot};

impl Engine {
    /// Makes room for one more voice in voice group `vg`, before it starts.
    pub(crate) fn limit_voice_group(&mut self, vg: u32) {
        let plan = self.plan.clone();
        let Some(group) = plan.voice_groups.get(vg as usize) else {
            return;
        };
        let fade = (group.fade_ms * self.rate as f32 / 1000.0).round().max(1.0) as u32;
        let voice_group = |s: &Slot| {
            (s.voice.is_active() && !s.fading)
                .then(|| plan.zones.get(s.zone)?.voice_group)
                .flatten()
        };
        if let Some(set) = group.exclusive {
            for slot in self.voices.iter_mut() {
                let other = voice_group(slot)
                    .filter(|&o| o != vg)
                    .and_then(|o| plan.voice_groups.get(o as usize));
                if other.is_some_and(|o| o.exclusive == Some(set)) {
                    slot.voice.fade(fade);
                    slot.fading = true;
                }
            }
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
                (!(group.prefer_released && s.released), order)
            })
            .map(|(i, _)| i);
        if let Some(slot) = victim.and_then(|i| self.voices.get_mut(i)) {
            slot.voice.fade(fade);
            slot.fading = true;
        }
    }
}
