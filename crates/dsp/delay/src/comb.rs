//! A feedback comb on a stereo voice: `y[n] = x[n] + g·y[n − D]`, `D` the
//! period of its pitch, read with a linear interpolation. The delay line is
//! the caller's, sized once ([`line_frames`]) so a note never allocates.

/// Frames of delay line a comb needs at `rate` to reach `lowest_hz`.
#[must_use]
pub fn line_frames(rate: u32, lowest_hz: f32) -> usize {
    (rate as f32 / lowest_hz.max(1.0)).ceil() as usize + 2
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Comb {
    at: usize,
    /// The delay in frames.
    delay: f32,
    feedback: f32,
    /// A feed-forward comb: `y[n] = x[n] + g·x[n − D]`, the line holds input.
    forward: bool,
}

impl Comb {
    #[must_use]
    pub fn new(hz: f32, feedback: f32, forward: bool, rate: u32) -> Self {
        let mut c = Self {
            forward,
            ..Self::default()
        };
        c.set(hz, feedback, rate);
        c
    }

    pub fn set(&mut self, hz: f32, feedback: f32, rate: u32) {
        self.delay = rate as f32 / hz.max(1.0);
        self.feedback = feedback.clamp(0.0, 0.999);
    }

    /// Runs `io` through the comb on `line` (its history, cleared at a
    /// note's start).
    pub fn process(&mut self, io: &mut [[f32; 2]], line: &mut [[f32; 2]]) {
        let len = line.len();
        if len < 4 {
            return;
        }
        // At least one frame, within the line.
        let delay = self.delay.clamp(1.0, (len - 2) as f32);
        let whole = delay as usize;
        let frac = delay - whole as f32;
        for frame in io {
            let back = |n: usize| {
                line.get((self.at + len - n) % len)
                    .copied()
                    .unwrap_or_default()
            };
            let (a, b) = (back(whole), back(whole + 1));
            let mut out = [0.0; 2];
            for ((o, x), (p, q)) in out.iter_mut().zip(frame.iter()).zip(a.iter().zip(b.iter())) {
                *o = x + self.feedback * (p + (q - p) * frac);
            }
            if let Some(cell) = line.get_mut(self.at) {
                *cell = if self.forward { *frame } else { out };
            }
            self.at = (self.at + 1) % len;
            *frame = out;
        }
    }

    /// Starts a note: its position back at the start.
    pub fn reset(&mut self) {
        self.at = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feed_forward_comb_echoes_once() {
        let rate = 48_000;
        let mut line = vec![[0.0; 2]; line_frames(rate, 20.0)];
        let mut c = Comb::new(480.0, 0.5, true, rate);
        let mut io = vec![[0.0; 2]; 400];
        io[0] = [1.0, 1.0];
        c.process(&mut io, &mut line);
        assert!((io[100][0] - 0.5).abs() < 1e-5);
        assert!(io[200][0].abs() < 1e-6);
    }

    #[test]
    fn an_impulse_rings_at_its_period_and_decays_by_the_feedback() {
        let rate = 48_000;
        let mut line = vec![[0.0; 2]; line_frames(rate, 20.0)];
        let mut c = Comb::new(480.0, 0.5, false, rate);
        let mut io = vec![[0.0; 2]; 400];
        io[0] = [1.0, 1.0];
        c.process(&mut io, &mut line);
        // 480 Hz at 48 kHz is a period of 100 frames.
        assert_eq!(io[0][0], 1.0);
        assert!((io[100][0] - 0.5).abs() < 1e-5, "{}", io[100][0]);
        assert!((io[200][0] - 0.25).abs() < 1e-5, "{}", io[200][0]);
        assert!(io[50][0].abs() < 1e-6);
    }
}
