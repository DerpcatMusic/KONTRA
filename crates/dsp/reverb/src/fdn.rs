//! The algorithmic reverb: one 8-line feedback delay network (Hadamard
//! feedback, a one-pole damping filter per line) behind a predelay and four
//! input allpasses. Formats load their own laws into [`Settings`].

/// What a reverb sounds like, in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Time for the tail to fall 60 dB.
    pub seconds: f32,
    /// Room size: scales the delay lines, 1 is a mid-size room.
    pub size: f32,
    /// 0..=1: how much faster the highs die than the lows.
    pub damping: f32,
    /// 0..=1: input allpass gain over 0.7, the tail's early density.
    pub diffusion: f32,
    pub predelay_ms: f32,
    /// Output low-pass corner.
    pub high_cut_hz: f32,
    /// Output gain below 200 Hz.
    pub low_shelf_db: f32,
    /// 0 mono, 1 full width.
    pub stereo: f32,
}

/// Mutually prime line lengths at size 1, ms.
const LINES_MS: [f32; 8] = [29.7, 37.1, 41.1, 43.7, 53.0, 59.9, 67.1, 79.3];
const DIFFUSERS_MS: [f32; 4] = [4.7, 3.6, 12.7, 9.3];
const SHELF_HZ: f32 = 200.0;

struct Delay {
    buf: Box<[f32]>,
    at: usize,
}

impl Delay {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)].into(),
            at: 0,
        }
    }

    /// The sample written `len` samples ago.
    fn read(&self) -> f32 {
        self.buf.get(self.at).copied().unwrap_or(0.0)
    }

    fn write(&mut self, x: f32) {
        if let Some(s) = self.buf.get_mut(self.at) {
            *s = x;
        }
        self.at = (self.at + 1) % self.buf.len();
    }
}

/// One-pole low-pass coefficient for `hz` at `rate`.
fn pole(hz: f32, rate: f32) -> f32 {
    (-std::f32::consts::TAU * hz.clamp(1.0, 0.49 * rate) / rate).exp()
}

pub struct Fdn {
    predelay: Delay,
    diffusers: Vec<Delay>,
    diffusion: f32,
    lines: Vec<Delay>,
    /// Per line: feedback gain for the decay, and its damping filter state.
    gains: [f32; 8],
    damp: f32,
    lows: [f32; 8],
    high_cut: f32,
    shelf: f32,
    shelf_gain: f32,
    /// Output filter states, per channel: high cut, shelf.
    out: [[f32; 2]; 2],
    stereo: f32,
}

impl Fdn {
    /// Not real-time: allocates the delay lines.
    #[must_use]
    pub fn new(s: Settings, rate: u32) -> Self {
        let rate = rate.max(1) as f32;
        let samples = |ms: f32| (ms * 0.001 * rate).round().max(1.0) as usize;
        let size = s.size.clamp(0.25, 4.0);
        let seconds = s.seconds.max(0.05);
        let lines: Vec<Delay> = LINES_MS
            .iter()
            .map(|&ms| Delay::new(samples(ms * size)))
            .collect();
        let gains = std::array::from_fn(|i| {
            let len = lines.get(i).map_or(1, |l| l.buf.len()) as f32;
            10f32.powf(-3.0 * len / (seconds * rate))
        });
        Self {
            predelay: Delay::new(samples(s.predelay_ms.clamp(0.0, 1000.0))),
            diffusers: DIFFUSERS_MS
                .iter()
                .map(|&ms| Delay::new(samples(ms)))
                .collect(),
            diffusion: 0.7 * s.diffusion.clamp(0.0, 1.0),
            lines,
            gains,
            damp: 0.85 * s.damping.clamp(0.0, 1.0),
            lows: [0.0; 8],
            high_cut: pole(s.high_cut_hz, rate),
            shelf: pole(SHELF_HZ, rate),
            shelf_gain: 10f32.powf(s.low_shelf_db / 20.0) - 1.0,
            out: [[0.0; 2]; 2],
            stereo: s.stereo.clamp(0.0, 1.0),
        }
    }

    /// Replaces `io` with the reverb of its mono sum (the wet path only).
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        for frame in io {
            let [l, r] = *frame;
            let mut x = self.predelay.read();
            self.predelay.write(0.5 * (l + r));
            // Schroeder allpasses: y = −g·x + d, d' = x + g·y.
            for d in &mut self.diffusers {
                let y = d.read() - self.diffusion * x;
                d.write(x + self.diffusion * y);
                x = y;
            }
            let mut y = [0.0f32; 8];
            let lines = self.lines.iter().zip(&mut self.lows).zip(self.gains);
            for (v, ((line, low), g)) in y.iter_mut().zip(lines) {
                let out = line.read();
                *low = out + self.damp * (*low - out);
                *v = *low * g;
            }
            let [a, b, c, d, e, f, g, h] = y;
            let (left, right) = (a - c + e - g, b - d + f - h);
            hadamard(&mut y);
            for ((line, v), sign) in self.lines.iter_mut().zip(y).zip([1.0, -1.0].iter().cycle()) {
                let w = v + sign * x;
                // The lines feed back: flush what would decay into denormals.
                line.write(if w.abs() < 1e-20 { 0.0 } else { w });
            }
            let (mid, side) = (0.5 * (left + right), 0.5 * (left - right) * self.stereo);
            for ((o, [cut, low]), v) in frame
                .iter_mut()
                .zip(&mut self.out)
                .zip([mid + side, mid - side])
            {
                *cut = v + self.high_cut * (*cut - v);
                *low = *cut + self.shelf * (*low - *cut);
                *o = 0.5 * (*cut + self.shelf_gain * *low);
            }
        }
        for s in self.lows.iter_mut().chain(self.out.as_flattened_mut()) {
            if s.abs() < 1e-20 {
                *s = 0.0;
            }
        }
    }
}

/// The orthonormal 8-point Walsh–Hadamard transform, in place.
fn hadamard(v: &mut [f32; 8]) {
    for h in [1, 2, 4] {
        for pair in v.chunks_exact_mut(2 * h) {
            let (lo, hi) = pair.split_at_mut(h);
            for (a, b) in lo.iter_mut().zip(hi) {
                (*a, *b) = (*a + *b, *a - *b);
            }
        }
    }
    for x in v {
        *x *= std::f32::consts::FRAC_1_SQRT_2 * 0.5;
    }
}
