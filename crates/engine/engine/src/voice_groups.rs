//! Voice limits: the instrument's and those shared across groups. A full
//! limit gives up a voice by its steal rule, an exclusive set silences the
//! other voice groups in it, and a choke stops the voice groups it names.
//! Each stops voices its own way: a fade, or their release.

use kontra_ir as ir;

use crate::{Engine, NoteRef, Slot};

impl Engine {
    /// The order `rule` gives up voices in, the least first.
    pub(crate) fn steal_order(&self, rule: &ir::VoiceGroup, s: &Slot) -> (bool, i64, u64) {
        let key = || {
            s.note
                .and_then(|n| self.notes.get(n))
                .map_or(0, |n| i64::from(n.key))
        };
        let level = || {
            let peak = s
                .voice
                .current_gain()
                .as_flattened()
                .iter()
                .fold(0.0f32, |a, &b| a.max(b.abs()));
            i64::from(if peak.is_finite() {
                peak.max(0.0).to_bits()
            } else {
                0
            })
        };
        let order = match rule.steal {
            ir::Steal::Oldest => s.started.0 as i64,
            ir::Steal::Newest => -(s.started.0 as i64),
            ir::Steal::Highest => -key(),
            ir::Steal::Lowest => key(),
            ir::Steal::Quietest => level(),
        };
        (
            !(rule.prefer_released && s.voice.is_released()),
            order,
            s.started.0,
        )
    }

    /// Stops voice `index` as `rule` stops voices: through its release, or a
    /// fade over the rule's time (at least 1 ms). Either way it stops counting.
    pub(crate) fn stop_by(&mut self, index: usize, rule: &ir::VoiceGroup) {
        if !rule.release {
            let fade = (rule.fade_ms.max(1.0) * self.rate as f32 / 1000.0).round() as u32;
            self.steal(index, fade);
            return;
        }
        let now = self.now.0;
        let Some(slot) = self.voices.get_mut(index).filter(|s| s.voice.is_active()) else {
            return;
        };
        let stopped = slot.voice.end_by_release(now);
        #[cfg(feature = "trace")]
        if let (true, Some(trace), Some(slot)) = (stopped, &mut self.trace, self.voices.get(index))
        {
            crate::trace::voice(trace, self.now.0, index, slot, crate::VoiceEvent::Steal);
        }
        self.stats.voices_stolen += u64::from(stopped);
    }

    /// The voice group a playing voice belongs to.
    fn voice_group_of(&self, s: &Slot) -> Option<u32> {
        (s.voice.is_active() && !s.voice.is_ending())
            .then(|| self.plan.zones.get(s.zone)?.voice_group)
            .flatten()
    }

    /// Stops, each by its own voice group's rule, the voices that a new
    /// voice of voice group `vg` chokes (`off_by`), except the note's own.
    pub(crate) fn choke_by(&mut self, vg: u32, note: NoteRef) {
        for i in 0..self.voices.len() {
            let Some(s) = self.voices.get(i) else {
                continue;
            };
            let Some(zone) = self.plan.zones.get(s.zone) else {
                continue;
            };
            if s.note == Some(note)
                || zone.off_by != Some(vg)
                || !s.voice.is_active()
                || s.voice.is_ending()
            {
                continue;
            }
            let rule = (zone.voice_group)
                .and_then(|g| self.voice_groups.get(g as usize).copied())
                .unwrap_or(self.plan.steal);
            self.stop_by(i, &rule);
        }
    }

    /// Makes room for one more voice of `note` in voice group `vg`, before
    /// it starts.
    pub(crate) fn limit_voice_group(&mut self, vg: u32, note: NoteRef) {
        let Some(&group) = self.voice_groups.get(vg as usize) else {
            return;
        };
        if let Some(set) = group.exclusive {
            for i in 0..self.voices.len() {
                let other = (self.voices.get(i))
                    .filter(|s| s.note != Some(note))
                    .and_then(|s| self.voice_group_of(s))
                    .filter(|&o| o != vg)
                    .and_then(|o| self.voice_groups.get(o as usize));
                if other.is_some_and(|o| o.exclusive == Some(set)) {
                    self.stop_by(i, &group);
                }
            }
        }
        if let Some(limit) = group.per_key.filter(|&l| l > 0) {
            self.limit_key(vg, &group, note, limit);
        }
        // Notes other than this one (its earlier zones are not rivals), or voices.
        let playing = if group.per_note {
            self.note_stamp = self.note_stamp.wrapping_add(1);
            if self.note_stamp == 0 {
                self.note_seen.fill(0);
                self.note_stamp = 1;
            }
            let mut notes = 0;
            for i in 0..self.voices.len() {
                let Some(n) = (self.voices.get(i))
                    .filter(|s| self.voice_group_of(s) == Some(vg))
                    .and_then(|s| s.note.filter(|&n| n != note))
                else {
                    continue;
                };
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
                .filter(|s| self.voice_group_of(s) == Some(vg))
                .count()
        };
        if group.voices == 0 || playing < usize::from(group.voices) {
            return;
        }
        // Down to one under the limit: a limit lowered while notes were
        // held (a script switching Poly to mono) gives up all the excess.
        for _ in usize::from(group.voices)..=playing {
            let victim = (self.voices.iter().enumerate())
                .filter(|(_, s)| self.voice_group_of(s) == Some(vg))
                .filter(|(_, s)| !group.per_note || s.note != Some(note))
                .min_by_key(|(_, s)| self.steal_order(&group, s))
                .map(|(i, s)| (i, s.note));
            let Some((index, owner)) = victim else {
                break;
            };
            // A counted note goes whole: every voice of it in the group.
            for i in 0..self.voices.len() {
                let whole = group.per_note
                    && (self.voices.get(i))
                        .is_some_and(|s| s.note == owner && self.voice_group_of(s) == Some(vg));
                if i == index || whole {
                    self.stop_by(i, &group);
                }
            }
        }
    }

    /// Keeps `note`'s key under `limit` voices in `vg`, counting other notes'
    /// voices: past it the oldest stop, or with `quieter_only` those no
    /// louder than `note`, quietest first.
    fn limit_key(&mut self, vg: u32, group: &ir::VoiceGroup, note: NoteRef, limit: u16) {
        let Some((key, velocity)) = self.notes.get(note).map(|n| (n.key, n.velocity)) else {
            return;
        };
        let rival = |e: &Self, s: &Slot| {
            s.note
                .filter(|&n| n != note)
                .and_then(|n| e.notes.get(n))
                .is_some_and(|n| n.key == key)
                && e.voice_group_of(s) == Some(vg)
        };
        let mut playing = self.voices.iter().filter(|s| rival(self, s)).count();
        while playing >= usize::from(limit) {
            let victim = (self.voices.iter().enumerate())
                .filter(|(_, s)| rival(self, s))
                .filter_map(|(i, s)| {
                    let v = s.note.and_then(|n| self.notes.get(n))?.velocity;
                    let order = if group.quieter_only {
                        (v <= velocity).then_some((v, s.started.0))?
                    } else {
                        (0.0, s.started.0)
                    };
                    Some((i, order))
                })
                .min_by(|a, b| a.1.0.total_cmp(&b.1.0).then(a.1.1.cmp(&b.1.1)))
                .map(|(i, _)| i);
            let Some(index) = victim else {
                return;
            };
            self.stop_by(index, group);
            playing -= 1;
        }
    }
}
