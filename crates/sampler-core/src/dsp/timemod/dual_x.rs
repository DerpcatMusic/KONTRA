//! UVI DualDelayX loop stages (Workstation 4.0.9; parameters copied by
//! `0x14123e1f0`), per channel in loop order: Crusher → Filtering →
//! Dispersion → Diffusion → tape emphasis + tanh; then a ±4 clamp
//! (`0x141686ce0`) and the feedback gain. Tape de-emphasis runs on the
//! delayed reads; tape drive scales the input by `10^(1.5·Drive)`.
//! - Crusher: rate table `0x141f8f908`, `ratio = min(1, f/sr)`,
//!   `step = max(1, 1/ratio)`; 4th-order Butterworth pre/post filters
//!   (`0x14129cf90`, `fc = min(ratio/2, 0.45)`, TDF-II f32 `0x141646fa0`)
//!   around a boxcar of `roundf(step)` frames (f64 sum) and the
//!   sample-and-hold quantizer `0x141663050` (`Q = 2^Bits`).
//! - Dispersion (`0x14164ccd0`): 32 equal allpass stages,
//!   `N = sr·3.125e-5·Spread + 2`, `R = e^(-2/(N+1))`,
//!   `a1 = cos(2πf/sr)·(-2R)`, `a2 = R²`.
//! - Diffusion (`0x141662830`): the nested-allpass network of
//!   [`super::velvet::nested`] with `D = trunc(Spread·sr/1000)`, 16 lengths
//!   from MT19937 seeded `ch + 1` (`l = 1 + u·D/8`), gains
//!   `(P/T + 1e-10)·Amount·0.618f`, ring `2^⌈log2(0.1·sr)⌉`; prepare fades
//!   in over 500 frames from the empty set.
//! - Tape (`0x141649120` shelves at 2500 Hz, gain `10^Warmth`).

use super::mt::Mt;
use super::tape::shelf;
use super::velvet::nested;
use sampler_ir::fx::timemod::DualX;

const RATES: [f32; 10] = [4000.0, 8000.0, 10_000.0, 12_000.0, 16_000.0, 20_000.0, 24_000.0, 32_000.0, 44_100.0, 48_000.0];
const STAGES: usize = 32;
const FADE: usize = 500;

/// Scalar slots per channel column.
const SC: usize = 24;
const DISP: usize = SC;
const BOX: usize = DISP + 4 * STAGES;

fn box_len(rate: f64) -> usize {
    (rate / 4000.0).ceil() as usize + 2
}

fn diff_ring(rate: f64) -> usize {
    ((rate * 0.1) as usize).next_power_of_two()
}

/// Extra slab frames after the delay ring.
pub(super) fn capacity(rate: f64) -> usize {
    BOX + box_len(rate) + diff_ring(rate)
}

/// Butterworth sections `[b0, b1, b2, a1, a2]` (f32) for cutoff `fc`·sr.
fn butterworth(fc: f32) -> [[f32; 5]; 2] {
    let k = 1.0 / (std::f64::consts::PI * f64::from(fc)).tan();
    std::array::from_fn(|s| {
        let c = -2.0 * ((2 * (s + 1) + 3) as f64 * std::f64::consts::FRAC_PI_2 / 4.0).cos();
        let n = 1.0 / (k * c + k * k + 1.0);
        [n, 2.0 * n, n, 2.0 * (1.0 - k * k) * n, (k * k - k * c + 1.0) * n].map(|v| v as f32)
    })
}

/// The stage designs for one block.
pub(super) struct Stages {
    crush: Option<([[f32; 5]; 2], usize, f32, u32)>,
    disp: Option<(f32, f32)>,
    diff: Option<[(Vec<(usize, f64)>, usize); 2]>,
    emph: Option<[f32; 3]>,
    pub(super) deemph: Option<[f32; 3]>,
    /// Input gain `10^(1.5·Drive)`.
    pub(super) drive: f32,
    diff_n: usize,
}

/// The diffusion taps for channel `ch` (`0x141662830`).
pub(super) fn diffusion(amount: f32, spread: f32, ch: u32, rate: f64) -> (Vec<(usize, f64)>, usize) {
    let d = (f64::from(spread) * 0.001 * rate) as i64 as f64;
    let mut mt = Mt::new(ch + 1);
    let l: Vec<f64> = (0..16).map(|_| 1.0 + mt.unit_f64() * d * 0.125).collect();
    let sum: f64 = l.iter().sum();
    let lens: Vec<usize> = l.iter().map(|&v| ((d * v / sum).round() as usize).max(1)).collect();
    let total: usize = lens.iter().sum();
    let g = f64::from(amount) * f64::from(0.618f32);
    let mut before = 0;
    let taps = lens
        .iter()
        .map(|&len| {
            let tap = (total - before, (before as f64 / total as f64 + 1e-10) * g);
            before += len;
            tap
        })
        .collect();
    (taps, total)
}

impl Stages {
    pub(super) fn new(x: &DualX, rate: f64) -> Self {
        let sr = rate as f32;
        let crush = x.crusher.map(|(i, bits)| {
            let ratio = (RATES[usize::from(i.min(9))] / sr).min(1.0);
            let step = (1.0 / ratio).max(1.0);
            (butterworth((0.5 * ratio).min(f32::from_bits(0x3ee6_6666))), step.round() as usize, step, 1u32 << bits)
        });
        let disp = x.dispersion.map(|(f, spread)| {
            let n = (rate * 3.125e-5 * f64::from(spread) + 2.0) as f32;
            let r = (-2.0 / (n + 1.0)).exp();
            (((std::f64::consts::TAU * f64::from(f) / rate).cos() * f64::from(-2.0 * r)) as f32, r * r)
        });
        let diff = x.diffusion.map(|(a, s)| [diffusion(a, s, 0, rate), diffusion(a, s, 1, rate)]);
        let tape = x.tape.map(|(drive, warmth)| {
            let g = f64::from(10f32.powf(warmth));
            (shelf(2500.0 / rate, 1.0, g), shelf(2500.0 / rate, 1.0, 1.0 / g), 10f32.powf(1.5 * drive))
        });
        Self { crush, disp, diff, emph: tape.map(|t| t.0), deemph: tape.map(|t| t.1), drive: tape.map_or(1.0, |t| t.2), diff_n: diff_ring(rate) }
    }

    /// De-emphasis of a delayed read (`s` the extras region).
    pub(super) fn deemphasis(&self, s: &mut [[f64; 2]], ch: usize, v: f32) -> f32 {
        let Some([b0, b1, a1]) = self.deemph else { return v };
        let y = b0 * v + s[5][ch] as f32;
        s[5][ch] = f64::from(b1 * v - a1 * y);
        y
    }

    /// Crusher (before Filtering).
    pub(super) fn crush(&self, s: &mut [[f64; 2]], ch: usize, x: f32) -> f32 {
        let Some((bq, l, step, q)) = self.crush else { return x };
        let tdf = |s: &mut [[f64; 2]], at: usize, mut x: f32| {
            for (k, [b0, b1, b2, a1, a2]) in bq.iter().enumerate() {
                let (s1, s2) = (s[at + 2 * k][ch] as f32, s[at + 2 * k + 1][ch] as f32);
                let y = b0 * x + s1;
                s[at + 2 * k][ch] = f64::from(b1 * x + s2 - a1 * y);
                s[at + 2 * k + 1][ch] = f64::from(b2 * x - a2 * y);
                x = y;
            }
            x
        };
        let x = tdf(s, 6, x);
        // Boxcar.
        let i = s[2][ch] as usize % l;
        let oldest = s[BOX + i][ch] as f32;
        s[BOX + i][ch] = f64::from(x);
        s[2][ch] = ((i + 1) % l) as f64;
        let sum = f64::from(x - oldest) + s[3][ch];
        s[3][ch] = sum;
        let x = (sum / l as f64) as f32;
        // Quantizer.
        let (mut counter, mut held) = (s[0][ch] as f32, s[1][ch] as f32);
        if counter <= 0.0 {
            let qf = q as f32;
            let d = x - held;
            let m = ((qf * d).abs() as u32) & (q - 1);
            held += (m as f32 / qf).copysign(d);
            counter += step;
        }
        counter -= 1.0;
        (s[0][ch], s[1][ch]) = (f64::from(counter), f64::from(held));
        tdf(s, 10, held)
    }

    /// Dispersion, Diffusion and tape (after Filtering).
    pub(super) fn tail(&self, s: &mut [[f64; 2]], ch: usize, mut x: f32) -> f32 {
        if let Some((a1, a2)) = self.disp {
            for k in 0..STAGES {
                let at = DISP + 4 * k;
                let [x1, x2, y1, y2] = [0, 1, 2, 3].map(|j| s[at + j][ch] as f32);
                let y = ((x1 * a1 + x * a2 + x2) - y1 * a1) - a2 * y2;
                for (j, v) in [x, x1, y, y1].into_iter().enumerate() {
                    s[at + j][ch] = f64::from(v);
                }
                x = y;
            }
        }
        if let Some(sets) = &self.diff {
            let (taps, total) = &sets[ch];
            let ring_at = s.len() - self.diff_n;
            let (w, k) = (s[14][ch] as usize, s[15][ch] as usize);
            let (t, old) = if k < FADE { (k as f32 / FADE as f32, None) } else { (1.0, Some(*total)) };
            x = nested(&mut s[ring_at..], ch, w, x, taps, *total, t, old);
            let n = s.len() - ring_at;
            s[14][ch] = ((w + 1) % n) as f64;
            s[15][ch] = (k + 1).min(FADE) as f64;
        }
        if let Some([b0, b1, a1]) = self.emph {
            let y = b0 * x + s[4][ch] as f32;
            s[4][ch] = f64::from(b1 * x - a1 * y);
            x = y.tanh();
        }
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `0x14129cf90` at 48 kHz, CrusherFreq 0.
    #[test]
    fn crusher_design_matches_native() {
        let s = butterworth((0.5f32 * (4000.0 / 48_000.0f32)).min(0.45));
        let want = [[0.015501709239158678, 0.031003418478317356, 0.015501709239158678, -1.7577536011953796, 0.8197604381520143], [0.013749370936628924, 0.027498741873257848, 0.013749370936628924, -1.559054289121878, 0.6140517728683936]];
        for (got, want) in s.iter().zip(want) {
            assert_eq!(got, &want.map(|v| v as f32));
        }
    }

    /// Quantizer `0x141663050`, Bits 8, step 1 (no filters, boxcar 1).
    #[test]
    fn quantizer_matches_native() {
        let st = Stages { crush: Some(([[1.0, 0.0, 0.0, 0.0, 0.0]; 2], 1, 1.0, 256)), disp: None, diff: None, emph: None, deemph: None, drive: 1.0, diff_n: diff_ring(48_000.0) };
        let mut s = vec![[0.0; 2]; capacity(48_000.0)];
        let got: Vec<f32> = (0..6).map(|i| st.crush(&mut s, 0, 0.9 * (0.37 * i as f32).sin() + 0.05 * i as f32 / 40.0)).collect();
        let want = [0.0, 0.324219, 0.605469, 0.808594, 0.898438, 0.875];
        assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 5e-7), "{got:?}");
    }

    /// `0x14164ccd0` at 48 kHz, 200 Hz, spread 20: impulse response.
    #[test]
    fn dispersion_matches_native() {
        let x = DualX { crusher: None, dispersion: Some((200.0, 20.0)), diffusion: None, tape: None, reflection: false };
        let st = Stages::new(&x, 48_000.0);
        assert_eq!(st.disp.map(|(a, b)| (a.to_bits(), b.to_bits())), Some((0xbff0dcf3, 0x3f62c6ce)));
        let mut s = vec![[0.0; 2]; capacity(48_000.0)];
        let got: Vec<f32> = (0..6).map(|i| st.tail(&mut s, 0, if i == 0 { 1.0 } else { 0.0 })).collect();
        assert_eq!(got, [0.020675858, -0.16043778, 0.46190459, -0.52489209, -0.00068068504, 0.31490204]);
    }

    /// `0x141662830` / `0x14165af50` at 48 kHz, spread 20, amount 0.3.
    #[test]
    fn diffusion_matches_native() {
        let (taps, total) = diffusion(0.3, 20.0, 0, 48_000.0);
        let lens: Vec<usize> = taps.windows(2).map(|p| p[0].0 - p[1].0).chain([taps[15].0]).collect();
        assert_eq!((total, lens), (960, vec![100, 94, 14, 101, 24, 40, 40, 68, 94, 85, 32, 53, 45, 24, 54, 92]));
        let x = DualX { crusher: None, dispersion: None, diffusion: Some((0.3, 20.0)), tape: None, reflection: false };
        let st = Stages::new(&x, 48_000.0);
        let mut s = vec![[0.0; 2]; capacity(48_000.0)];
        let got: Vec<f32> = (0..1800).map(|i| st.tail(&mut s, 0, [1.0, -0.5][usize::from(i == 700)] * f32::from(u8::from(i == 0 || i == 700)))).collect();
        assert_eq!([got[960], got[1052], got[1660], got[1752]], [0.9431032, -0.028262226, -0.41614252, 0.070180014]);
    }

    /// Tape emphasis at 48 kHz, Warmth 0.5.
    #[test]
    fn tape_emphasis_matches_native() {
        let x = DualX { crusher: None, dispersion: None, diffusion: None, tape: Some((0.0, 0.5)), reflection: false };
        let st = Stages::new(&x, 48_000.0);
        assert_eq!(st.emph.map(|c| c.map(f32::to_bits)), Some([0x402afa48, 0xc00ded3c, 0xbf0bcbd2]));
    }
}
