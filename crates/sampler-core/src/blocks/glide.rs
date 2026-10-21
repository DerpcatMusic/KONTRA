//! Portamento. The block yields a pitch offset in semitones that decays to
//! zero; the router adds it to the voice pitch. Control-rate, no allocation.
//! Kontakt's runtime glide (`BGlide`, slot 6 0x140ceb090) is the different,
//! sample-stepped [`GlideRamp`] below. The stored glide object (0x0B) is decoded
//! (ni-file `Glide`), but how its fields and the note pitch become the ramp's
//! start, step and gain is not proved, so the Kontakt mapping reports it
//! unsupported instead of configuring either type.

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

/// Kontakt's `BGlide` ramp (slot 6, 0x140ceb090): `start` (`+0xb8`), `step`
/// (`+0xbc`) and `gain` (`+0xc0`), all binary32. Each output is `gain * start`
/// taken before `start += step`; once `start` is strictly on the side of zero
/// the step is heading to (positive with a positive step, negative with a
/// negative one) every remaining output of the call is exactly zero, and so
/// are all later calls. A zero step never stops. The crossing sample is written
/// with the pre-update value: it is neither clamped nor interpolated to the
/// endpoint. Control rate and units are unproved; the caller picks them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlideRamp {
    pub start: f32,
    pub step: f32,
    pub gain: f32,
}

impl GlideRamp {
    pub fn render(&mut self, out: &mut [f32]) {
        for i in 0..out.len() {
            if (self.step > 0.0 && self.start > 0.0) || (self.step < 0.0 && self.start < 0.0) {
                out[i..].fill(0.0);
                return;
            }
            out[i] = self.gain * self.start;
            self.start += self.step;
        }
    }
}

impl GlideRamp {
    /// Kontakt's `BGlide` setup (FUN_140cebba0) from the stored `BParGlide`:
    /// `mode` 1 multiplies the time by the interval (per-semitone), 2 keeps
    /// it constant; `time_ms` is the stored float at `+0xc`. The interval is
    /// clamped to ±12 semitones. Output is in octaves: `start = -1` rising to
    /// zero for an upward interval (`+1` falling for a downward one), with
    /// `gain = |interval| / 12` (binary32 `0x3daaaaab`, checked in the binary)
    /// and `step = 1 / (time_s * rate)`. `rate` is the object rate (`+0x14`).
    pub fn kontakt_glide(mode: u32, time_ms: f32, interval: i32, rate: f32) -> Self {
        const TWELFTH: f32 = 0.083_333_336; // 0x3daaaaab at 0x144737724
        const MILLISECOND: f32 = 0.001; // 0x3a83126f at 0x1447148a4
        let interval = interval.clamp(-12, 12);
        let semitones = interval.unsigned_abs() as f32;
        let mut time = time_ms;
        if mode == 1 {
            time *= semitones;
        }
        let step = 1.0 / (time * MILLISECOND * rate);
        let gain = semitones * TWELFTH;
        if interval < 0 {
            Self { start: 1.0, step: -step, gain }
        } else {
            Self { start: -1.0, step, gain }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kontakt_glide_maps_the_stored_mode_and_time() {
        // Mode 1, 100 ms per semitone, seven semitones up: 700 ms = 700 frames at 1 kHz.
        let mut up = GlideRamp::kontakt_glide(1, 100.0, 7, 1000.0);
        let mut out = [0.0; 701];
        up.render(&mut out);
        assert!((out[0] + 7.0 / 12.0).abs() < 1e-6, "starts at -interval/12 octaves");
        assert!((out[350] + 7.0 / 24.0).abs() < 1e-5, "linear halfway");
        assert!(out[700].abs() < 1e-5, "lands on zero within f32 accumulation");
        // A downward interval mirrors the ramp in sign.
        let mut down = GlideRamp::kontakt_glide(1, 100.0, -7, 1000.0);
        let mut mirror = [0.0; 2];
        down.render(&mut mirror);
        assert!((mirror[0] - 7.0 / 12.0).abs() < 1e-6);
        // Mode 2 keeps 100 ms whatever the interval: 100 frames.
        let mut fixed = GlideRamp::kontakt_glide(2, 100.0, 12, 1000.0);
        assert!((fixed.step - 0.01).abs() < 1e-7);
        let mut tail = [9.0; 101];
        fixed.render(&mut tail);
        assert!(tail[100].abs() < 1e-5);
        // Zero interval: no output.
        let mut none = GlideRamp::kontakt_glide(1, 100.0, 0, 1000.0);
        let mut silent = [9.0; 3];
        none.render(&mut silent);
        assert_eq!(silent, [0.0; 3]);
    }

    #[test]
    fn ramp_writes_the_pre_update_value_then_stays_exactly_zero() {
        let mut r = GlideRamp { start: -2.0, step: 1.0, gain: 0.5 };
        let mut out = [9.0; 6];
        r.render(&mut out);
        // -1, -0.5, 0, then start = 1 > 0 stops the ramp.
        assert_eq!(out, [-1.0, -0.5, 0.0, 0.0, 0.0, 0.0]);
        let mut later = [9.0; 3];
        r.render(&mut later);
        assert_eq!(later, [0.0; 3], "stopped for good");
        // The downward mirror.
        let mut r = GlideRamp { start: 2.0, step: -1.0, gain: 1.0 };
        let mut out = [9.0; 5];
        r.render(&mut out);
        assert_eq!(out, [2.0, 1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn ramp_already_past_zero_or_with_no_step_follows_the_branches() {
        let mut past = GlideRamp { start: 0.5, step: 0.25, gain: 1.0 };
        let mut out = [9.0; 2];
        past.render(&mut out);
        assert_eq!(out, [0.0, 0.0]);
        let mut flat = GlideRamp { start: 0.5, step: 0.0, gain: 2.0 };
        let mut out = [0.0; 3];
        flat.render(&mut out);
        assert_eq!(out, [1.0; 3], "a zero step never stops");
        // Split calls agree with one call.
        let mut a = GlideRamp { start: -3.0, step: 1.0, gain: 1.0 };
        let mut b = a;
        let (mut whole, mut x, mut y) = ([0.0; 6], [0.0; 2], [0.0; 4]);
        a.render(&mut whole);
        b.render(&mut x);
        b.render(&mut y);
        assert_eq!([x.as_slice(), y.as_slice()].concat(), whole);
    }

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
