//! Recomputing sounding voices after something they read has changed.

#[cfg(feature = "trace")]
use crate::script::Kind;
use crate::{Engine, NoteRef, SampleTime, Slot, Zone, modulators, voice_gain};

impl Engine {
    /// Recomputes pitch and gain of a note's voices; with `cc`, only the gain
    /// of voices whose zone crossfades on that controller.
    pub(crate) fn refresh_note(&mut self, note: NoteRef, cc: Option<u8>) {
        self.refresh_voices(cc, |slot, _| slot.note == Some(note));
    }

    /// Recomputes every sounding voice of a held note that `pick` accepts,
    /// in one pass over the voices; `cc` as in [`Self::refresh_note`].
    /// Each voice's result reads only its own note, zone and group, so the
    /// order the voices are visited in does not matter.
    pub(crate) fn refresh_voices(&mut self, cc: Option<u8>, pick: impl Fn(&Slot, &Zone) -> bool) {
        let plan = self.plan.clone();
        for i in 0..self.voices.len() {
            let Some(slot) = self.voices.get(i) else {
                continue;
            };
            let Some(zone) = plan.zones.get(slot.zone) else {
                continue;
            };
            if !slot.voice.is_active() || !pick(slot, zone) {
                continue;
            }
            let Some(n) = slot.note.and_then(|note| self.notes.get(note).copied()) else {
                continue;
            };
            #[cfg(feature = "trace")]
            let trace_cc = cc.or_else(|| {
                (self.running.kind == Kind::Controller).then_some(self.running.controller)
            });
            #[cfg(feature = "trace")]
            if self.trace.is_some()
                && let Some(cc) = trace_cc
            {
                slot.pending_trace_cc
                    .set(slot.pending_trace_cc.get() | (1u128 << cc));
            }
            if cc.is_some_and(|cc| !self.follows(zone, cc)) {
                continue;
            }
            let age = modulators::Age {
                warp: slot.warp,
                chaos: slot.chaos.map(|c| c[3]),
                ..self.age(slot.started, slot.voice.released_at().map(SampleTime))
            };
            #[cfg(feature = "trace")]
            if let (Some(trace), Some(cc)) = (&self.trace, trace_cc) {
                trace.routes.begin(i, slot.zone, cc);
            }
            let pan = [slot.pan[0], self.group_pan(zone)];
            let (gain, lag) = self.voice_gain(zone, &n, slot.factor, age, pan[0]);
            let mix = self.voice_mix(zone, &n, age);
            let (step, bend) = self.step(zone, &n, age);
            let law = self.plan.route_pitch;
            let pitch = cc.is_none_or(|cc| self.pitch_follows(zone, cc));
            self.retune_rack(i, zone, &n, age);
            #[cfg(feature = "trace")]
            if let Some(trace) = &self.trace {
                trace.routes.end();
            }
            if let Some(slot) = self.voices.get_mut(i) {
                slot.pan = pan;
                slot.voice.set_gain(gain);
                slot.voice.set_lag(lag);
                if let Some(mix) = mix {
                    slot.voice.set_mix(mix);
                }
                if pitch {
                    slot.bend[1] = bend.semitones;
                    slot.voice
                        .set_step(voice_gain::bent(law, step, bend.instant + slot.bend[0]));
                }
            }
        }
    }
}
