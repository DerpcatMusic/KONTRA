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
    /// How far the lines' lengths swing (all but the first, half of them a
    /// quarter cycle apart), and how fast.
    pub modulation_ms: f32,
    pub modulation_hz: f32,
}

/// Mutually prime line lengths at size 1, ms.
const LINES_MS: [f32; 8] = [29.7, 37.1, 41.1, 43.7, 53.0, 59.9, 67.1, 79.3];
const DIFFUSERS_MS: [f32; 4] = [4.7, 3.6, 12.7, 9.3];
const SHELF_HZ: f32 = 200.0;
/// The largest size and predelay [`Fdn::set`] accepts; the lines are allocated for them.
const MAX_SIZE: f32 = 4.0;
const MAX_PREDELAY_MS: f32 = 1000.0;
/// The widest line swing [`Fdn::set`] accepts; the lines have room for it.
const MAX_MODULATION_MS: f32 = 5.0;
/// The wet output fades out and back in over this long around a change. Lines and
/// tail keep running, so the change lands in silence and never clicks.
const FADE_S: f32 = 0.01;

/// A delay line allocated for `max` samples; its read length moves within that.
struct Delay {
    buf: Box<[f32]>,
    at: usize,
    len: usize,
}

impl Delay {
    /// Not real-time: allocates. Two spare samples keep reads clear of the write.
    fn new(max: usize) -> Self {
        let max = max.max(1);
        Self {
            buf: vec![0.0; max + 2].into(),
            at: 0,
            len: max,
        }
    }

    /// Reads `len` samples back from now; never allocates.
    fn set_len(&mut self, len: usize) {
        self.len = len.clamp(1, self.buf.len().saturating_sub(2));
    }

    /// The sample written `len` samples ago.
    fn read(&self) -> f32 {
        let n = self.buf.len();
        self.buf
            .get((self.at + n - self.len) % n)
            .copied()
            .unwrap_or(0.0)
    }

    /// `len + offset` samples ago (`offset` ≥ 0), linearly interpolated.
    fn read_late(&self, offset: f32) -> f32 {
        let n = self.buf.len();
        let back = (self.len as f32 + offset).min((n - 2) as f32);
        let whole = back as usize;
        let t = back - whole as f32;
        let at = |k: usize| self.buf.get((self.at + n - k) % n).copied().unwrap_or(0.0);
        at(whole) + t * (at(whole + 1) - at(whole))
    }

    fn write(&mut self, x: f32) {
        if let Some(s) = self.buf.get_mut(self.at) {
            *s = x;
        }
        self.at = (self.at + 1) % self.buf.len();
    }
}

/// Delay samples for `ms` at `rate`, at least one.
fn samples(ms: f32, rate: f32) -> usize {
    (ms * 0.001 * rate).round().max(1.0) as usize
}

/// One-pole low-pass coefficient for `hz` at `rate`.
fn pole(hz: f32, rate: f32) -> f32 {
    (-std::f32::consts::TAU * hz.clamp(1.0, 0.49 * rate) / rate).exp()
}

pub struct Fdn {
    rate: f32,
    /// The last settings given (to [`Fdn::new`] or [`Fdn::set`]), and the one
    /// waiting for the wet output to fade out.
    requested: Settings,
    pending: Option<Settings>,
    /// Wet output gain, and its per-sample step: fade-out (−), fade-in (+), 0.
    gain: f32,
    step: f32,
    fade_step: f32,
    predelay: Delay,
    diffusers: Vec<Delay>,
    diffusion: f32,
    lines: Vec<Delay>,
    /// Per line: feedback gain for the decay, and its damping filter state.
    gains: [f32; 8],
    damp: f32,
    lows: [f32; 8],
    high_cut: f32,
    /// Line swing in samples (peak to peak), its phase in cycles and step.
    swing: f32,
    cycle: f64,
    cycle_step: f64,
    shelf: f32,
    shelf_gain: f32,
    /// Output filter states, per channel: high cut, shelf.
    out: [[f32; 2]; 2],
    stereo: f32,
    /// Where the line gains, shelf gain and width glide to: a change that
    /// keeps the line lengths moves them over about one fade, not at once.
    to: Glide,
    ease: f32,
}

#[derive(Clone, Copy)]
struct Glide {
    gains: [f32; 8],
    shelf_gain: f32,
    stereo: f32,
}

impl Fdn {
    /// Not real-time: allocates the delay lines for the largest size and predelay.
    #[must_use]
    pub fn new(s: Settings, rate: u32) -> Self {
        let rate = rate.max(1) as f32;
        let mut fdn = Self {
            rate,
            requested: s,
            pending: None,
            gain: 1.0,
            step: 0.0,
            fade_step: 1.0 / (FADE_S * rate),
            predelay: Delay::new(samples(MAX_PREDELAY_MS, rate)),
            diffusers: DIFFUSERS_MS
                .iter()
                .map(|&ms| Delay::new(samples(ms, rate)))
                .collect(),
            diffusion: 0.0,
            lines: LINES_MS
                .iter()
                .map(|&ms| Delay::new(samples(ms * MAX_SIZE + 2.0 * MAX_MODULATION_MS, rate)))
                .collect(),
            gains: [0.0; 8],
            damp: 0.0,
            lows: [0.0; 8],
            high_cut: 0.0,
            swing: 0.0,
            cycle: 0.0,
            cycle_step: 0.0,
            shelf: 0.0,
            shelf_gain: 0.0,
            out: [[0.0; 2]; 2],
            stereo: 0.0,
            to: Glide {
                gains: [0.0; 8],
                shelf_gain: 0.0,
                stereo: 0.0,
            },
            ease: 1.0 - (-1.0 / (FADE_S * rate)).exp(),
        };
        fdn.apply(s);
        (fdn.gains, fdn.shelf_gain, fdn.stereo) = (fdn.to.gains, fdn.to.shelf_gain, fdn.to.stereo);
        fdn
    }

    /// Changes every setting while it plays. Never allocates. A change that
    /// keeps the line lengths (size, pre-delay) applies at once. One that
    /// moves them fades the wet output out, applies in the silence and fades
    /// back in; changes during a fade wait for it, only the latest applies,
    /// and a fade-in finishes before the next fade-out. A repeat of the
    /// current settings does nothing.
    pub fn set(&mut self, s: Settings) {
        if s == self.requested {
            return;
        }
        let same_lengths =
            s.size == self.requested.size && s.predelay_ms == self.requested.predelay_ms;
        self.requested = s;
        if same_lengths && self.pending.is_none() {
            self.apply(s);
            return;
        }
        self.pending = Some(s);
        if self.step == 0.0 {
            self.step = -self.fade_step;
        }
    }

    /// Writes `s` into the lines and coefficients. Never allocates.
    fn apply(&mut self, s: Settings) {
        let rate = self.rate;
        let size = s.size.clamp(0.25, MAX_SIZE);
        let seconds = s.seconds.max(0.05);
        for ((line, &ms), g) in self.lines.iter_mut().zip(&LINES_MS).zip(&mut self.to.gains) {
            let len = samples(ms * size, rate);
            line.set_len(len);
            *g = 10f32.powf(-3.0 * len as f32 / (seconds * rate));
        }
        self.predelay
            .set_len(samples(s.predelay_ms.clamp(0.0, MAX_PREDELAY_MS), rate));
        self.diffusion = 0.7 * s.diffusion.clamp(0.0, 1.0);
        self.damp = 0.85 * s.damping.clamp(0.0, 1.0);
        self.high_cut = pole(s.high_cut_hz, rate);
        self.swing = 2.0 * s.modulation_ms.clamp(0.0, MAX_MODULATION_MS) * 0.001 * rate;
        self.cycle_step = f64::from(s.modulation_hz.max(0.0) / rate);
        self.shelf = pole(SHELF_HZ, rate);
        self.to.shelf_gain = 10f32.powf(s.low_shelf_db / 20.0) - 1.0;
        self.to.stereo = s.stereo.clamp(0.0, 1.0);
    }

    /// This frame's wet gain. At the bottom of a fade-out the pending settings
    /// apply, then the gain climbs back.
    fn duck(&mut self) -> f32 {
        let g = self.gain;
        self.gain = (self.gain + self.step).clamp(0.0, 1.0);
        if self.step < 0.0 && self.gain <= 0.0 {
            if let Some(s) = self.pending.take() {
                self.apply(s);
            }
            self.step = self.fade_step;
        } else if self.step > 0.0 && self.gain >= 1.0 {
            self.step = if self.pending.is_some() {
                -self.fade_step
            } else {
                0.0
            };
        }
        g
    }

    /// Replaces `io` with the reverb of its mono sum (the wet path only).
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        for frame in io {
            let wet = self.duck();
            let k = self.ease;
            for (g, to) in self.gains.iter_mut().zip(self.to.gains) {
                *g += (to - *g) * k;
            }
            self.shelf_gain += (self.to.shelf_gain - self.shelf_gain) * k;
            self.stereo += (self.to.stereo - self.stereo) * k;
            let [l, r] = *frame;
            let mut x = self.predelay.read();
            self.predelay.write(0.5 * (l + r));
            // Schroeder allpasses: y = −g·x + d, d' = x + g·y.
            for d in &mut self.diffusers {
                let y = d.read() - self.diffusion * x;
                d.write(flush(x + self.diffusion * y));
                x = y;
            }
            let mut y = [0.0f32; 8];
            // Two quadrature swings, 0..=1 of the peak-to-peak depth.
            let (sin, cos) = (std::f64::consts::TAU * self.cycle).sin_cos();
            let swings = [0.0, 0.5 + 0.5 * sin as f32, 0.5 + 0.5 * cos as f32];
            self.cycle = (self.cycle + self.cycle_step).fract();
            let lines = self.lines.iter().zip(&mut self.lows).zip(self.gains);
            for (i, (v, ((line, low), g))) in y.iter_mut().zip(lines).enumerate() {
                let out = if self.swing > 0.0 {
                    let phase = if i == 0 { 0 } else { 1 + i % 2 };
                    line.read_late(self.swing * swings.get(phase).copied().unwrap_or(0.0))
                } else {
                    line.read()
                };
                *low = out + self.damp * (*low - out);
                *v = *low * g;
            }
            let [a, b, c, d, e, f, g, h] = y;
            let (left, right) = (a - c + e - g, b - d + f - h);
            hadamard(&mut y);
            for ((line, v), sign) in self.lines.iter_mut().zip(y).zip([1.0, -1.0].iter().cycle()) {
                let w = v + sign * x;
                line.write(flush(w));
            }
            let (mid, side) = (0.5 * (left + right), 0.5 * (left - right) * self.stereo);
            for ((o, [cut, low]), v) in frame
                .iter_mut()
                .zip(&mut self.out)
                .zip([mid + side, mid - side])
            {
                *cut = v + self.high_cut * (*cut - v);
                *low = *cut + self.shelf * (*low - *cut);
                *o = wet * 0.5 * (*cut + self.shelf_gain * *low);
            }
        }
        for s in self.lows.iter_mut().chain(self.out.as_flattened_mut()) {
            if s.abs() < 1e-20 {
                *s = 0.0;
            }
        }
    }
}

/// `x`, or 0 where it would decay into denormals (the lines feed back).
fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
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
