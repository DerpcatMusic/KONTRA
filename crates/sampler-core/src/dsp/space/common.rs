//! Pieces every reverb topology shares: delay ring, one-pole coefficient,
//! wet tone stage (width and low shelf), quadrature LFO and early reflections.

use std::f32::consts::TAU;

/// Power-of-two ring; `mask` wraps indices without branches.
pub(super) struct Line {
    pub(super) buf: Vec<f32>,
    mask: usize,
}

impl Line {
    pub(super) fn new(min_len: usize) -> Self {
        let len = (min_len + 2).next_power_of_two();
        Self { buf: vec![0.0; len], mask: len - 1 }
    }
    #[inline(always)]
    pub(super) fn read(&self, pos: usize, delay: usize) -> f32 {
        self.buf[pos.wrapping_sub(delay) & self.mask]
    }
    /// Linear interpolation between `delay` and `delay + 1` frames back.
    #[inline(always)]
    pub(super) fn read_frac(&self, pos: usize, delay: f32) -> f32 {
        let whole = delay as usize;
        let frac = delay - whole as f32;
        let a = self.read(pos, whole);
        let b = self.read(pos, whole + 1);
        a + (b - a) * frac
    }
    #[inline(always)]
    pub(super) fn write(&mut self, pos: usize, value: f32) {
        self.buf[pos & self.mask] = value;
    }
    pub(super) fn clear(&mut self) {
        self.buf.fill(0.0);
    }
}

/// Coefficient of a one-pole lowpass at `hz`.
pub(super) fn one_pole(hz: f32, rate: f32) -> f32 {
    1.0 - (-TAU * hz.min(rate * 0.49) / rate).exp()
}

/// Wet stereo width (mid/side) then a low shelf that cuts below about 250 Hz.
pub(super) struct Tone {
    width: f32,
    shelf_coef: f32,
    shelf_gain: f32,
    shelf_state: [f32; 2],
}

impl Tone {
    pub(super) fn new(width: f64, low_shelf_db: f64, rate: f32) -> Self {
        Self {
            width: width as f32,
            shelf_coef: one_pole(250.0, rate),
            shelf_gain: 10f32.powf(low_shelf_db as f32 / 20.0) - 1.0,
            shelf_state: [0.0; 2],
        }
    }
    pub(super) fn clear(&mut self) {
        self.shelf_state = [0.0; 2];
    }
    #[inline(always)]
    pub(super) fn tick(&mut self, l: f32, r: f32) -> [f32; 2] {
        if self.width == 1.0 && self.shelf_gain == 0.0 {
            return [l, r];
        }
        let (mid, side) = (0.5 * (l + r), 0.5 * (l - r) * self.width);
        let mut out = [mid + side, mid - side];
        for (ch, v) in out.iter_mut().enumerate() {
            let low = &mut self.shelf_state[ch];
            *low = crate::dsp::kernels::one_pole32(*low, *v, self.shelf_coef);
            *v += *low * self.shelf_gain;
        }
        out
    }
}

/// A rotating phasor: sine and cosine without per-sample trigonometry.
/// Renormalised once per block, so the amplitude stays within f32 rounding.
pub(super) struct Lfo {
    sin: f32,
    cos: f32,
    step: [f32; 2],
}

impl Lfo {
    pub(super) fn new(hz: f64, rate: f32) -> Self {
        let w = TAU * hz as f32 / rate;
        Self { sin: 0.0, cos: 1.0, step: [w.sin(), w.cos()] }
    }
    #[inline(always)]
    pub(super) fn tick(&mut self) -> (f32, f32) {
        let [s, c] = self.step;
        let sin = self.sin * c + self.cos * s;
        let cos = self.cos * c - self.sin * s;
        (self.sin, self.cos) = (sin, cos);
        (sin, cos)
    }
    pub(super) fn renormalize(&mut self) {
        let n = (self.sin * self.sin + self.cos * self.cos).sqrt().recip();
        self.sin *= n;
        self.cos *= n;
    }
}

/// Tap times in ms at size 1 of the early reflections, per output channel.
const EARLY_MS: [[f32; 8]; 2] = [
    [7.1, 11.3, 17.9, 23.7, 29.3, 37.1, 43.7, 53.9],
    [8.9, 13.7, 19.1, 26.3, 33.1, 41.3, 47.9, 59.3],
];
/// Alternating-sign taps each 1.4 dB quieter than the last.
const EARLY_DECAY: f32 = 0.851;

/// Eight reflections per channel from the mono input.
pub(super) struct Early {
    line: Line,
    taps: [[usize; 8]; 2],
    gains: [[f32; 8]; 2],
}

impl Early {
    pub(super) fn new(level: f64, size: f64, predelay: usize, rate: f32) -> Self {
        let scale = size as f32 * 0.001 * rate;
        let taps = EARLY_MS.map(|ch| ch.map(|ms| predelay + (ms * scale) as usize));
        let longest = taps.iter().flatten().copied().max().unwrap_or(0);
        let mut gains = [[0.0; 8]; 2];
        for ch in &mut gains {
            let mut g = level as f32;
            for (k, v) in ch.iter_mut().enumerate() {
                *v = if k % 2 == 0 { g } else { -g };
                g *= EARLY_DECAY;
            }
        }
        Self { line: Line::new(longest + 1), taps, gains }
    }
    pub(super) fn clear(&mut self) {
        self.line.clear();
    }
    #[inline(always)]
    pub(super) fn tick(&mut self, pos: usize, mono: f32) -> [f32; 2] {
        self.line.write(pos, mono);
        std::array::from_fn(|ch| {
            self.taps[ch]
                .iter()
                .zip(&self.gains[ch])
                .map(|(&t, &g)| self.line.read(pos, t) * g)
                .sum()
        })
    }
}

/// Predelay with optional echo feedback, then an input lowpass.
pub(super) struct Front {
    line: Line,
    len: usize,
    echo: f32,
    coef: f32,
    state: f32,
}

impl Front {
    pub(super) fn new(predelay_seconds: f64, echo: f64, cutoff_hz: f64, rate: f32) -> Self {
        let len = (predelay_seconds as f32 * rate) as usize;
        Self {
            line: Line::new(len + 1),
            len,
            echo: if len > 0 { echo as f32 } else { 0.0 },
            coef: one_pole(cutoff_hz as f32, rate),
            state: 0.0,
        }
    }
    pub(super) fn clear(&mut self) {
        self.line.clear();
        self.state = 0.0;
    }
    #[inline(always)]
    pub(super) fn tick(&mut self, pos: usize, x: f32) -> f32 {
        let delayed = self.line.read(pos, self.len);
        self.line.write(pos, x + self.echo * delayed);
        let delayed = self.line.read(pos, self.len);
        self.state = crate::dsp::kernels::biased_one_pole32(self.state, (delayed - self.state) * self.coef);
        self.state
    }
}

/// Schroeder allpass: `out = delayed - g x`, `write = x + g out`.
pub(super) struct Allpass {
    line: Line,
    len: usize,
    g: f32,
}

impl Allpass {
    pub(super) fn new(len: usize, g: f32) -> Self {
        Self { line: Line::new(len + 1), len, g }
    }
    pub(super) fn clear(&mut self) {
        self.line.clear();
    }
    /// The stored value `delay` frames back.
    #[inline(always)]
    pub(super) fn peek(&self, pos: usize, delay: usize) -> f32 {
        self.line.read(pos, delay)
    }
    #[inline(always)]
    pub(super) fn tick(&mut self, pos: usize, x: f32) -> f32 {
        let delayed = self.line.read(pos, self.len);
        let out = delayed - self.g * x;
        self.line.write(pos, x + self.g * out);
        out
    }
}

/// Time for the Schroeder-integrated energy decay to fall from -5 to -25 dB,
/// extrapolated to 60 dB.
#[cfg(test)]
pub(super) fn rt60(x: &[f32], rate: f32) -> f32 {
    let mut tail = 0.0f64;
    let mut curve: Vec<f64> = x
        .iter()
        .rev()
        .map(|v| {
            tail += f64::from(*v).powi(2);
            tail
        })
        .collect();
    curve.reverse();
    let at = |level: f64| {
        (0..curve.len()).find(|&i| 10.0 * (curve[i] / curve[0]).log10() < level).unwrap() as f32 / rate
    };
    3.0 * (at(-25.0) - at(-5.0))
}
