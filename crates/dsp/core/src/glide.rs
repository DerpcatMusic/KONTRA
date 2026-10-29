//! Levels that move in a straight line to where they were last aimed, so a
//! change never steps (a stepped gain clicks).

/// A slot's 2×2 gain matrix and its dry and wet levels, gliding 10 ms to
/// where its settings put them, as the native engines ramp their gains.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Levels {
    glide: Glide<6>,
    frames: u32,
}

impl Levels {
    #[must_use]
    pub const fn new(rate: u32) -> Self {
        Self {
            glide: Glide::new(),
            frames: rate / 100,
        }
    }

    /// `io` at the `dry` level plus `wet` at the `level`.
    pub fn mix(&mut self, io: &mut [[f32; 2]], wet: &[[f32; 2]], dry: f32, level: f32) {
        self.glide
            .aim([1.0, 0.0, 0.0, 1.0, dry, level], self.frames);
        for (x, w) in io.iter_mut().zip(wet) {
            let [.., dry, level] = self.glide.next();
            *x = [x[0] * dry + w[0] * level, x[1] * dry + w[1] * level];
        }
    }

    /// `io` through the matrix `[[ll, lr], [rl, rr]]`.
    pub fn matrix(&mut self, io: &mut [[f32; 2]], [[ll, lr], [rl, rr]]: [[f32; 2]; 2]) {
        self.glide.aim([ll, lr, rl, rr, 1.0, 1.0], self.frames);
        if self.glide.resting() && [ll, lr, rl, rr] == [1.0, 0.0, 0.0, 1.0] {
            return;
        }
        for f in io {
            let [ll, lr, rl, rr, ..] = self.glide.next();
            *f = [ll * f[0] + lr * f[1], rl * f[0] + rr * f[1]];
        }
    }
}

/// An output fader and balance: the gain follows a one-pole, each channel's
/// balance gain moves at most 1 per `slew` frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fader {
    gain: f32,
    balance: [f32; 2],
    coef: f32,
    step: f32,
    aimed: bool,
}

impl Fader {
    #[must_use]
    pub fn new(rate: u32, tau_ms: f32, slew_ms: f32) -> Self {
        let frames = |ms: f32| ms * rate as f32 / 1000.0;
        Self {
            gain: 1.0,
            balance: [1.0; 2],
            coef: 1.0 - (-1.0 / frames(tau_ms).max(1.0)).exp(),
            step: 1.0 / frames(slew_ms).max(1.0),
            aimed: false,
        }
    }

    /// `io` at `gain`, its channels at `balance`. The first call lands at once.
    pub fn process(&mut self, io: &mut [[f32; 2]], gain: f32, balance: [f32; 2]) {
        if !self.aimed {
            (self.gain, self.balance, self.aimed) = (gain, balance, true);
        }
        if [self.gain, gain] == [1.0; 2] && [self.balance, balance] == [[1.0; 2]; 2] {
            return;
        }
        for f in io {
            self.gain += (gain - self.gain) * self.coef;
            for ((b, to), x) in self.balance.iter_mut().zip(balance).zip(f.iter_mut()) {
                *b += (to - *b).clamp(-self.step, self.step);
                *x *= *b * self.gain;
            }
        }
    }
}

/// `N` values gliding to their target over a set number of frames. The first
/// aim lands at once, so a slot starts where its settings are.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Glide<const N: usize> {
    now: [f32; N],
    to: [f32; N],
    step: [f32; N],
    left: u32,
    aimed: bool,
}

impl<const N: usize> Glide<N> {
    const fn new() -> Self {
        Self {
            now: [0.0; N],
            to: [0.0; N],
            step: [0.0; N],
            left: 0,
            aimed: false,
        }
    }

    /// Aims at `to`, arriving `frames` frames from now.
    fn aim(&mut self, to: [f32; N], frames: u32) {
        if !self.aimed {
            *self = Self {
                now: to,
                to,
                aimed: true,
                ..Self::new()
            };
        } else if to != self.to {
            self.to = to;
            self.left = frames.max(1);
            for ((s, t), n) in self.step.iter_mut().zip(to).zip(self.now) {
                *s = (t - n) / self.left as f32;
            }
        }
    }

    /// True when it has arrived.
    fn resting(&self) -> bool {
        self.left == 0
    }

    /// The values for the next frame.
    fn next(&mut self) -> [f32; N] {
        if self.left > 0 {
            self.left -= 1;
            for (n, s) in self.now.iter_mut().zip(self.step) {
                *n += s;
            }
            if self.left == 0 {
                self.now = self.to;
            }
        }
        self.now
    }
}

#[cfg(test)]
mod tests {
    use super::{Fader, Glide, Levels};

    #[test]
    fn a_fader_settles_as_a_one_pole_and_its_balance_slews() {
        let mut f = Fader::new(1000, 10.0, 50.0);
        f.process(&mut [[1.0; 2]], 1.0, [1.0; 2]);
        let mut io = [[1.0; 2]; 50];
        f.process(&mut io, 0.5, [1.0, 0.5]);
        // The gain is e^-1 of the way left after 10 ms; the right channel
        // falls 0.02 a frame and lands on 0.5 after 25.
        let left = (io[9][0] - 0.5) / 0.5;
        assert!((left - (-1.0f32).exp()).abs() < 0.02, "{left}");
        let right = |n: usize| io[n][1] / io[n][0];
        assert!((right(23) - 0.52).abs() < 1e-4 && (right(24) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn it_starts_where_first_aimed_then_glides_in_a_line() {
        let mut g = Glide::<1>::new();
        g.aim([1.0], 4);
        assert_eq!(g.next(), [1.0]);
        g.aim([0.0], 4);
        let path: Vec<f32> = (0..5).map(|_| g.next()[0]).collect();
        assert_eq!(path, [0.75, 0.5, 0.25, 0.0, 0.0]);
        assert!(g.resting());
    }

    #[test]
    fn a_wet_level_change_glides_over_ten_ms() {
        let mut levels = Levels::new(1000);
        let mut io = [[0.0; 2]; 12];
        levels.mix(&mut io, &[[1.0; 2]; 12], 0.0, 1.0);
        assert_eq!(io[0], [1.0; 2], "starts where its settings are");
        let mut io = [[0.0; 2]; 12];
        levels.mix(&mut io, &[[1.0; 2]; 12], 0.0, 0.0);
        assert_eq!(io[0][0], 0.9);
        assert!(io[8][0] > 0.0 && io[9][0] == 0.0, "{io:?}");
    }
}
