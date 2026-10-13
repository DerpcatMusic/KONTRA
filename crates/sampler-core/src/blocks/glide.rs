//! Portamento. The block yields a pitch offset in semitones that decays to
//! zero; the router adds it to the voice pitch. Control-rate, no allocation.
//! Kontakt's stored glide object (0x0B) has no decoded reader or proven time
//! law yet; formats supply `Glide` once they do.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GlideTime {
    /// Every glide takes this many frames, whatever the interval.
    Constant(u32),
    /// Frames per semitone: wider intervals take longer.
    PerSemitone(f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glide {
    pub time: GlideTime,
    /// Glide only when the previous note is still held.
    pub legato_only: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GlideState {
    from: f64,
    age: u32,
    length: u32,
    /// Pitch (semitones) of the last note started: the next glide's origin.
    last: Option<f64>,
}

impl GlideState {
    /// Begin a note at `pitch` semitones. `legato`: another note is held.
    pub fn start(&mut self, glide: &Glide, pitch: f64, legato: bool) {
        let from = self.last.filter(|_| legato || !glide.legato_only);
        self.last = Some(pitch);
        let Some(from) = from else {
            self.length = 0;
            self.from = 0.0;
            return;
        };
        self.from = from - pitch;
        self.age = 0;
        self.length = match glide.time {
            GlideTime::Constant(frames) => frames,
            GlideTime::PerSemitone(per) => {
                (self.from.abs() * per).round().min(f64::from(u32::MAX)) as u32
            }
        };
    }

    /// Offset in semitones after `frames` more frames.
    pub fn advance(&mut self, frames: u32) -> f64 {
        self.age = self.age.saturating_add(frames).min(self.length);
        self.offset()
    }

    pub fn offset(&self) -> f64 {
        if self.age >= self.length {
            0.0
        } else {
            self.from * (1.0 - f64::from(self.age) / f64::from(self.length))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glides_linearly_from_the_previous_pitch() {
        let g = Glide { time: GlideTime::Constant(100), legato_only: false };
        let mut s = GlideState::default();
        s.start(&g, 60.0, false);
        assert_eq!(s.offset(), 0.0, "the first note has nothing to glide from");
        s.start(&g, 67.0, false);
        assert_eq!(s.offset(), -7.0);
        assert!((s.advance(25) + 5.25).abs() < 1e-12);
        assert_eq!(s.advance(75), 0.0);
        assert_eq!(s.advance(1000), 0.0);
    }

    #[test]
    fn legato_only_and_constant_rate() {
        let g = Glide { time: GlideTime::PerSemitone(10.0), legato_only: true };
        let mut s = GlideState::default();
        s.start(&g, 60.0, false);
        s.start(&g, 64.0, false);
        assert_eq!(s.offset(), 0.0, "detached note does not glide");
        s.start(&g, 52.0, true);
        assert_eq!(s.offset(), 12.0);
        assert!((s.advance(60) - 6.0).abs() < 1e-12, "120 frames for 12 semitones");
    }
}
