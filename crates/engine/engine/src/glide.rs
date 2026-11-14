//! Portamento ([`ir::Glide`]): a note's voices start where the previous
//! note of their scope had got to and slide to their own pitch. The slide is
//! read at each control step and the voice ramps its step frame by frame to
//! where the slide will be at the next, so the pitch moves every frame.

use kontra_ir as ir;

use crate::voice_gain::bent;
use crate::{Engine, MAX_SPAN, NoteRef};

/// The scopes the engine tracks; a higher scope shares the last.
pub(crate) const SCOPES: usize = 32;

/// A note's slide: `from` semitones off its pitch at engine frame `start`,
/// reaching it `frames` later along the law's curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Slide {
    from: f32,
    start: u64,
    frames: f32,
    law: ir::Glide,
}

impl Slide {
    /// Semitones off the note's own pitch at engine frame `t`.
    pub(crate) fn offset(&self, t: u64) -> f32 {
        let p = t.saturating_sub(self.start) as f32 / self.frames;
        if p >= 1.0 {
            0.0
        } else {
            self.from * (1.0 - self.law.covered(p))
        }
    }
}

/// A scope's latest note: its key and its slide.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Latest {
    note: Option<NoteRef>,
    key: f32,
    slide: Option<Slide>,
}

impl Engine {
    /// The slide a voice of `note` in `group` starts with: worked out at the
    /// note's first voice in the scope, shared by the rest of its voices.
    pub(crate) fn slide_for(&mut self, group: u32, note: NoteRef, key: u8) -> Option<Slide> {
        let law = self.groups.get(group as usize)?.glide?;
        let scope = usize::from(law.scope).min(SCOPES - 1);
        let latest = *self.glides.get(scope)?;
        if latest.note == Some(note) {
            return latest.slide;
        }
        let now = self.now.0;
        let key = f32::from(key);
        // Where the previous note sounds now, mid-slide or not.
        let sliding = latest.slide.filter(|_| law.resume);
        let from = latest
            .note
            .map(|_| latest.key + sliding.map_or(0.0, |s| s.offset(now)));
        let joined = !law.legato || self.holds_other(scope, note);
        let slide = from
            .filter(|_| joined)
            .map(|from| (from - key) * law.depth)
            .filter(|interval| interval.abs() > 1e-4)
            .map(|interval| Slide {
                from: interval,
                start: now,
                frames: (law.duration(interval) * self.rate as f32).max(1.0),
                law,
            });
        if let Some(l) = self.glides.get_mut(scope) {
            *l = Latest {
                note: Some(note),
                key,
                slide,
            };
        }
        slide
    }

    /// Whether a key other than `note`'s is down on a voice of `scope`.
    fn holds_other(&self, scope: usize, note: NoteRef) -> bool {
        self.voices.iter().any(|s| {
            s.note != Some(note)
                && s.voice.is_active()
                && !s.voice.is_released()
                && !s.voice.is_ending()
                && (self.plan.zones.get(s.zone))
                    .and_then(|z| self.groups.get(z.group as usize)?.glide)
                    .is_some_and(|g| usize::from(g.scope).min(SCOPES - 1) == scope)
        })
    }

    /// Sets voice `i`'s step: `step` raised by `semitones` and its slide,
    /// ramping frame by frame to where the slide is at the next control step.
    pub(crate) fn set_pitch(&mut self, i: usize, step: f64, semitones: f32) {
        let law = self.plan.route_pitch;
        let now = self.now.0;
        let next = now + (MAX_SPAN - now % MAX_SPAN);
        let Some(slot) = self.voices.get_mut(i) else {
            return;
        };
        match slot.slide {
            None => slot.voice.set_step(bent(law, step, semitones)),
            Some(s) => {
                slot.voice.set_step_ramp(
                    bent(law, step, semitones + s.offset(now)),
                    bent(law, step, semitones + s.offset(next)),
                    (next - now) as u32,
                );
                if s.offset(next) == 0.0 {
                    slot.slide = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAW: ir::Glide = ir::Glide {
        scope: 0,
        depth: 1.0,
        seconds: 1.0,
        per_semitone: false,
        resume: true,
        legato: false,
        curve: 0.0,
    };

    #[test]
    fn a_straight_slide_is_linear_in_semitones_and_ends_on_the_note() {
        let s = Slide {
            from: -12.0,
            start: 100,
            frames: 1000.0,
            law: LAW,
        };
        assert_eq!(s.offset(100), -12.0);
        assert_eq!(s.offset(600), -6.0);
        assert_eq!(s.offset(1100), 0.0);
        assert_eq!(s.offset(5000), 0.0);
    }

    #[test]
    fn a_bent_slide_keeps_its_ends_and_covers_its_half_way_share() {
        // Curve 0.5: a quarter of the interval at half time (slow, then fast).
        let law = ir::Glide { curve: 0.5, ..LAW };
        let s = Slide {
            from: 12.0,
            start: 0,
            frames: 100.0,
            law,
        };
        assert_eq!(s.offset(0), 12.0);
        assert!((s.offset(50) - 9.0).abs() < 1e-4, "{}", s.offset(50));
        assert_eq!(s.offset(100), 0.0);
        // A constant rate: 0.03 s a semitone, so an octave takes 0.36 s.
        let rate = ir::Glide {
            per_semitone: true,
            seconds: 0.03,
            ..LAW
        };
        assert!((rate.duration(-12.0) - 0.36).abs() < 1e-6);
    }
}
