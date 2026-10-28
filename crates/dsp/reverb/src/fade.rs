//! The generic reverb's live-change rule (a translator's network glides
//! instead, by its law): a change that
//! moves the delay lengths lands in silence.

use crate::Settings;

/// The wet output fades out and back in over this long around a change. Lines and
/// tail keep running, so the change lands in silence and never clicks.
const FADE_S: f32 = 0.01;

pub(crate) struct Fade {
    requested: Settings,
    pending: Option<Settings>,
    gain: f32,
    step: f32,
    fade_step: f32,
}

impl Fade {
    pub(crate) fn new(s: Settings, rate: f32) -> Self {
        Self {
            requested: s,
            pending: None,
            gain: 1.0,
            step: 0.0,
            fade_step: 1.0 / (FADE_S * rate),
        }
    }

    /// Takes a change: `Some` applies now. A change that keeps the line
    /// lengths (size, pre-delay) applies at once. One that moves them fades
    /// the wet output out and comes back from [`Self::tick`] in the silence;
    /// changes during a fade wait for it, only the latest applies, and a
    /// fade-in finishes before the next fade-out. A repeat does nothing.
    pub(crate) fn set(&mut self, s: Settings) -> Option<Settings> {
        if s == self.requested {
            return None;
        }
        let same_lengths =
            s.size == self.requested.size && s.predelay_ms == self.requested.predelay_ms;
        self.requested = s;
        if same_lengths && self.pending.is_none() {
            return Some(s);
        }
        self.pending = Some(s);
        if self.step == 0.0 {
            self.step = -self.fade_step;
        }
        None
    }

    /// This frame's wet gain, and the pending settings at the bottom of a
    /// fade-out.
    pub(crate) fn tick(&mut self) -> (f32, Option<Settings>) {
        let g = self.gain;
        self.gain = (self.gain + self.step).clamp(0.0, 1.0);
        let mut apply = None;
        if self.step < 0.0 && self.gain <= 0.0 {
            apply = self.pending.take();
            self.step = self.fade_step;
        } else if self.step > 0.0 && self.gain >= 1.0 {
            self.step = if self.pending.is_some() {
                -self.fade_step
            } else {
                0.0
            };
        }
        (g, apply)
    }
}

/// `x`, or 0 where it would decay into denormals (the lines feed back).
pub(crate) fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}
