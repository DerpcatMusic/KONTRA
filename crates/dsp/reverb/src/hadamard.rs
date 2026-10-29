//! A [`HadamardNetwork`], in 64-frame blocks: the input scaled to the
//! loop's loss and predelayed, through four allpasses per channel; `lines`
//! prime-length lines read at a sine-swung delay, scaled by their mid decay
//! over `√lines`, mixed by an unnormalised Hadamard transform and tapped
//! out; then each line's two-band decay filter, a 1 Hz DC blocker, its
//! input channel added, written back. Settings read physically: `seconds`
//! the mid band's 60 dB time, `size` the room's crossing time, s,
//! `diffusion` the allpass gain, `high_cut_hz` a one-pole on the output
//! below 19 999 Hz, `stereo` the taps' width; `modulation_ms` and
//! `modulation_hz` the swing of a line `size` long, a line `k` times longer
//! swinging `k` times as far at `1/k` the rate. `damping` and
//! `low_shelf_db` are not used.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use kontra_law::HadamardNetwork;

use crate::Settings;
use crate::combs::Line;
use crate::fade::flush;

const BLOCK: usize = 64;
const MAX_LINES: usize = 16;
/// The prime table's last entry: no line is longer.
const MAX_PRIME: usize = 191_999;
/// The loop filters' highest corner, of the rate.
const MAX_CORNER: f32 = 0.499;
/// Frames added to each channel's allpasses, in order.
const SKEW: [[usize; 4]; 2] = [[0, 23, 0, 23], [23, 0, 23, 0]];

/// A first-order section `(b0 + b1 z⁻¹)/(1 + a1 z⁻¹)`.
#[derive(Clone, Copy, Default)]
struct Section {
    b0: f32,
    b1: f32,
    a1: f32,
    x1: f32,
    y1: f32,
}

impl Section {
    fn new(b0: f32, b1: f32, a1: f32) -> Self {
        Self {
            b0,
            b1,
            a1,
            ..Self::default()
        }
    }

    /// DC at `dc`, Nyquist at `nyquist`, the turn at `f` of the rate.
    fn shelf(f: f32, dc: f32, nyquist: f32) -> Self {
        let t = (PI * f).tan();
        let s = (dc / nyquist).sqrt();
        let d = t / s + 1.0;
        Self::new(
            nyquist * (t * s + 1.0) / d,
            nyquist * (t * s - 1.0) / d,
            (t / s - 1.0) / d,
        )
    }

    /// Unity at DC; at Nyquist the analog one-pole's gain there.
    fn low_pass(f: f32) -> Self {
        let t = (PI * f).tan();
        let c = 1.0 / (1.0 + (0.5 / f).powi(2)).sqrt();
        Self::new(
            (t + c) / (t + 1.0),
            (t - c) / (t + 1.0),
            (t - 1.0) / (t + 1.0),
        )
    }

    fn high_pass(f: f32) -> Self {
        let t = (PI * f).tan();
        let u = 0.5 / f;
        let c = u / (1.0 + u * u).sqrt();
        Self::new(c / (t + 1.0), -c / (t + 1.0), (t - 1.0) / (t + 1.0))
    }

    /// Takes `to`'s coefficients, keeping the state.
    fn retune(&mut self, to: Self) {
        *self = Self {
            x1: self.x1,
            y1: self.y1,
            ..to
        };
    }

    fn tick(&mut self, x: f32) -> f32 {
        let y = flush(self.b0 * x + self.b1 * self.x1 - self.a1 * self.y1);
        self.x1 = x;
        self.y1 = y;
        y
    }
}

struct Tap {
    line: Line,
    len: f32,
    depth: f32,
    /// LFO cycles per frame, and where it is.
    inc: f32,
    phase: f32,
    /// The read delay at the end of the last block.
    delay: f32,
    gain: f32,
    low: Section,
    high: Section,
    dc: f32,
}

pub struct Hadamard {
    law: HadamardNetwork,
    rate: f32,
    taps: Box<[Tap]>,
    pre: [Line; 2],
    diffusers: [[Line; 4]; 2],
    diffusion: f32,
    input: f32,
    /// Even lines' and odd lines' weights into each channel.
    out: [[f32; 2]; 2],
    dc_pole: f32,
    rolloff: Option<f32>,
    rolled: [f32; 2],
    last: Option<Settings>,
    w: usize,
}

impl Hadamard {
    /// Not real-time: allocates the lines for `s`'s size, swing and
    /// predelay.
    #[must_use]
    pub fn new(law: HadamardNetwork, s: Settings, rate: u32) -> Self {
        let rate = rate.max(1) as f32;
        let lens = lengths(&law, s.size, rate);
        let n = lines(&law);
        // ponytail: sized for the patch's room, swing and predelay; a script
        // that grows them later is held at what was allocated.
        let taps = lens
            .iter()
            .take(n)
            .map(|&len| Tap {
                line: Line::new(len + swing(s, len as f32).ceil() as usize + 2),
                len: len as f32,
                depth: 0.0,
                inc: 0.0,
                phase: 0.0,
                delay: len as f32,
                gain: 0.0,
                low: Section::default(),
                high: Section::default(),
                dc: 0.0,
            })
            .collect();
        let pre = (s.predelay_ms.max(0.0) * 0.001 * rate) as usize;
        let diffuser = |c: usize| {
            std::array::from_fn(|k| {
                let skew = SKEW.get(c).and_then(|s| s.get(k)).copied().unwrap_or(0);
                let ms = law.diffuser_ms * FRAC_PI_2.powi(k as i32);
                Line::new((rate * 0.001 * ms).round().max(1.0) as usize + skew)
            })
        };
        let mut h = Self {
            law,
            rate,
            taps,
            pre: [Line::new(pre), Line::new(pre)],
            diffusers: [diffuser(0), diffuser(1)],
            diffusion: 0.0,
            input: 0.0,
            out: [[0.0; 2]; 2],
            dc_pole: (-TAU / rate).exp(),
            rolloff: None,
            rolled: [0.0; 2],
            last: None,
            w: 0,
        };
        h.set(s);
        for t in h.taps.iter_mut() {
            t.delay = t.len;
        }
        h
    }

    /// Never allocates.
    pub fn set(&mut self, s: Settings) {
        if self.last == Some(s) {
            return;
        }
        self.last = Some(s);
        let (law, rate) = (self.law, self.rate);
        let lens = lengths(&law, s.size, rate);
        let n = self.taps.len() as f32;
        let fl = (law.low_hz / rate).clamp(1e-6, MAX_CORNER);
        let fh = (law.high_hz / rate).clamp(1e-6, MAX_CORNER);
        let per_s = if law.freeze {
            -0.006
        } else {
            -60.0 / s.seconds.max(1e-3)
        };
        let mut power = 0.0;
        for (t, &len) in self.taps.iter_mut().zip(&lens) {
            let depth = swing(s, len as f32);
            let reach = t.line.reach() as f32;
            t.len = (len as f32).min(reach - depth - 2.0).max(1.0);
            t.depth = depth.min(t.len - 1.0).max(0.0);
            t.inc = s.modulation_hz.max(0.0) * s.size.max(1e-4) / t.len;
            let loss = t.len / rate * per_s;
            let mid = 10f32.powf(loss / 20.0);
            let band = |decay: f32| 10f32.powf(loss / (20.0 * decay.max(1e-3)));
            t.gain = mid / n.sqrt();
            t.low.retune(if law.low_cut {
                Section::high_pass(fl)
            } else {
                Section::shelf(fl, band(law.decay_low) / mid, 1.0)
            });
            t.high.retune(if law.high_cut {
                Section::low_pass(fh)
            } else {
                Section::shelf(fh, 1.0, band(law.decay_high) / mid)
            });
            power += mid * mid;
        }
        let m = power / n;
        self.input = ((1.0 - m) / (1.0 + m)).max(0.0).sqrt();
        let (sin, cos) = ((1.0 - s.stereo.clamp(0.0, 1.0)) * FRAC_PI_4).sin_cos();
        let g = (2.0 / n).sqrt();
        self.out = [[cos * g, sin * g], [sin * g, cos * g]];
        self.diffusion = s.diffusion.clamp(0.0, 0.99);
        let pre = (s.predelay_ms.max(0.0) * 0.001 * rate) as usize;
        for p in &mut self.pre {
            p.delay = pre.min(p.reach());
        }
        if self.rolloff.is_none() {
            self.rolled = [0.0; 2];
        }
        self.rolloff = (s.high_cut_hz < 19_999.0)
            .then(|| (-TAU * s.high_cut_hz.clamp(1.0, 0.49 * rate) / rate).exp());
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        for block in io.chunks_mut(BLOCK) {
            self.block(block);
        }
    }

    fn block(&mut self, io: &mut [[f32; 2]]) {
        let frames = io.len() as f32;
        // Each line's delay glides to where its LFO is at the block's end,
        // moving at most a frame per frame.
        let mut from = [0.0; MAX_LINES];
        let mut step = [0.0; MAX_LINES];
        for ((t, from), step) in self.taps.iter_mut().zip(&mut from).zip(&mut step) {
            t.phase = (t.phase + t.inc * frames).fract();
            let to = t.len + t.depth * (TAU * t.phase).sin();
            let change = (to - t.delay).clamp(-(frames - 1e-3), frames - 1e-3);
            *from = t.delay;
            *step = change / frames;
            t.delay += change;
        }
        let (g, input, keep) = (self.diffusion, self.input, 1.0 - self.dc_pole);
        for (k, frame) in io.iter_mut().enumerate() {
            let w = self.w;
            let mut fed = [0.0; 2];
            for (c, (fed, x)) in fed.iter_mut().zip(frame.iter()).enumerate() {
                let (Some(pre), Some(aps)) = (self.pre.get_mut(c), self.diffusers.get_mut(c))
                else {
                    continue;
                };
                pre.write(w, x * input);
                let mut v = pre.read(w);
                if self.law.diffusers {
                    for ap in aps {
                        let held = ap.read(w);
                        let u = v - g * held;
                        ap.write(w, flush(u));
                        v = held + g * u;
                    }
                }
                *fed = v;
            }
            let mut y = [0.0; MAX_LINES];
            let at = (k + 1) as f32;
            for (((y, t), from), step) in y.iter_mut().zip(&*self.taps).zip(&from).zip(&step) {
                *y = t.line.at(w, (from + step * at).max(1.0)) * t.gain;
            }
            let n = self.taps.len();
            let lines = y.get_mut(..n).unwrap_or_default();
            fwht(lines);
            let mut wet = [0.0; 2];
            for (i, y) in lines.iter().enumerate() {
                let weights = self.out.get(i % 2).copied().unwrap_or_default();
                for (wet, weight) in wet.iter_mut().zip(weights) {
                    *wet += y * weight;
                }
            }
            for (i, (y, t)) in lines.iter().zip(self.taps.iter_mut()).enumerate() {
                let v = t.high.tick(t.low.tick(*y));
                t.dc = flush(t.dc + keep * (v - t.dc));
                let back = v - t.dc + fed.get(i % 2).copied().unwrap_or(0.0);
                t.line.write(w, flush(back));
            }
            if let Some(p) = self.rolloff {
                for (r, wet) in self.rolled.iter_mut().zip(&mut wet) {
                    *r = flush(*wet + p * (*r - *wet));
                    *wet = *r;
                }
            }
            *frame = wet;
            self.w = w.wrapping_add(1);
        }
    }
}

fn lines(law: &HadamardNetwork) -> usize {
    usize::from(law.lines)
        .clamp(2, MAX_LINES)
        .next_power_of_two()
}

/// How far, in frames, a line `len` frames long swings.
fn swing(s: Settings, len: f32) -> f32 {
    (s.modulation_ms.max(0.0) * 0.001 * len / s.size.max(1e-4)).max(0.0)
}

/// The lines' lengths, frames, shortest first: geometric from the room's
/// crossing at `size` s, each the next prime.
fn lengths(law: &HadamardNetwork, size: f32, rate: f32) -> [usize; MAX_LINES] {
    let n = lines(law) as f32;
    let a = (n / 2.0).powf(law.shape);
    let r = (2.0 * a).powf(-1.0 / (n - 1.0));
    let crossing = size.max(1e-4) * rate;
    let base = n * crossing * (1.0 - r) / (1.0 - r.powf(n + 1.0));
    let mut lens = [0; MAX_LINES];
    for (k, len) in lens.iter_mut().take(lines(law)).enumerate() {
        *len = next_prime((base * r.powi(k as i32)) as usize);
    }
    lens.get_mut(..lines(law))
        .unwrap_or_default()
        .sort_unstable();
    lens
}

/// The first prime at or above `n`, at most [`MAX_PRIME`].
fn next_prime(n: usize) -> usize {
    let prime = |p: usize| {
        p >= 2
            && (2..)
                .take_while(|d| d * d <= p)
                .all(|d| !p.is_multiple_of(d))
    };
    (n.max(2)..MAX_PRIME)
        .find(|&p| prime(p))
        .unwrap_or(MAX_PRIME)
}

/// The unnormalised fast Walsh–Hadamard transform of `y`, a power of two
/// long.
fn fwht(y: &mut [f32]) {
    let mut h = 1;
    while h < y.len() {
        for pair in y.chunks_exact_mut(2 * h) {
            let (a, b) = pair.split_at_mut(h);
            for (a, b) in a.iter_mut().zip(b) {
                (*a, *b) = (*a + *b, *a - *b);
            }
        }
        h *= 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_twenty_metre_room_has_eight_primes_an_octave_apart() {
        let law = HadamardNetwork {
            lines: 8,
            shape: 0.0,
            diffusers: true,
            diffuser_ms: 5.0,
            decay_low: 1.0,
            decay_high: 1.0,
            low_hz: 250.0,
            high_hz: 12_000.0,
            low_cut: false,
            high_cut: false,
            freeze: false,
        };
        let lens = lengths(&law, 20.0 / 340.0, 48_000.0);
        let lens = &lens[..8];
        assert!(lens.iter().all(|&l| next_prime(l) == l), "{lens:?}");
        assert!(lens.is_sorted(), "{lens:?}");
        let spread = lens[7] as f32 / lens[0] as f32;
        assert!((spread - 2.0).abs() < 0.01, "{lens:?}");
    }

    #[test]
    fn the_transform_squared_is_n_times_identity() {
        let mut y = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        fwht(&mut y);
        fwht(&mut y);
        assert_eq!(y, [8.0, 16.0, 24.0, 32.0, 40.0, 48.0, 56.0, 64.0]);
    }
}
