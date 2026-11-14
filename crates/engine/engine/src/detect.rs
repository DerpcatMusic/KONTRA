//! Music information retrieval for `detect_*` (KSP manual: Music Information
//! Retrieval): measurements of a zone's whole sample, run off the audio
//! thread. Levels follow ITU-R BS.1770-4 (K-weighted, gated loudness; the
//! peak is the sample peak, not the oversampled true peak); the pitch is
//! YIN (de Cheveigné and Kawahara 2002); the key correlates a chromagram with
//! the Krumhansl-Schmuckler profiles; the tempo is the strongest period of an
//! energy-onset envelope. Kontakt classifies sample, drum and instrument
//! types with a trained model KONTRA does not have: those always fail.

use std::f64::consts::PI;

/// A sample's stereo frames and rate.
pub struct Audio<'a> {
    pub frames: &'a [[f32; 2]],
    pub rate: u32,
}

/// What a `detect_*` asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Key,
    Pitch,
    Loudness,
    Peak,
    Rms,
    Tempo,
    /// Sample, drum and instrument type: no model, so always a failed detection.
    Class,
}

fn db(power: f64) -> Option<f64> {
    (power > 1e-20).then(|| 10.0 * power.log10())
}

/// Highest absolute sample, dB full scale.
pub fn peak(a: &Audio) -> Option<f64> {
    let top = a.frames.iter().flatten().fold(0f32, |m, v| m.max(v.abs()));
    db(f64::from(top).powi(2))
}

/// Root mean square over both channels, dB full scale.
pub fn rms(a: &Audio) -> Option<f64> {
    let sum: f64 = a.frames.iter().flatten().map(|&v| f64::from(v).powi(2)).sum();
    db(sum / (a.frames.len() * 2).max(1) as f64)
}

struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
}

impl Biquad {
    fn run(&self, x: &[f64]) -> Vec<f64> {
        let (mut z1, mut z2) = (0.0, 0.0);
        x.iter()
            .map(|&v| {
                let y = self.b[0] * v + z1;
                z1 = self.b[1] * v - self.a[0] * y + z2;
                z2 = self.b[2] * v - self.a[1] * y;
                y
            })
            .collect()
    }
}

/// BS.1770's two K-weighting stages at `rate` (the standard's 48 kHz
/// coefficients are this derivation at 48 kHz).
fn k_weighting(rate: f64) -> [Biquad; 2] {
    let (f0, gain, q) = (1_681.974_450_955_533, 3.999_843_853_973_347, 0.707_175_236_955_419_6);
    let k = (PI * f0 / rate).tan();
    let vh = 10f64.powf(gain / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b: [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
    };
    let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
    let k = (PI * f0 / rate).tan();
    let a0 = 1.0 + k / q + k * k;
    let high_pass = Biquad { b: [1.0, -2.0, 1.0], a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0] };
    [shelf, high_pass]
}

/// Integrated loudness, LUFS: 400 ms blocks every 100 ms, gated at -70 LUFS
/// and then 10 LU under the mean of what passed.
pub fn loudness(a: &Audio) -> Option<f64> {
    let rate = f64::from(a.rate);
    let channels: Vec<Vec<f64>> = (0..2)
        .map(|c| {
            let x: Vec<f64> = a.frames.iter().map(|f| f64::from(f[c])).collect();
            let [shelf, high_pass] = k_weighting(rate);
            high_pass.run(&shelf.run(&x))
        })
        .collect();
    let block = (rate * 0.4) as usize;
    let step = ((rate * 0.1) as usize).max(1);
    let n = a.frames.len();
    let starts: Vec<usize> = if n < block { vec![0] } else { (0..=n - block).step_by(step).collect() };
    let power: Vec<f64> = starts
        .iter()
        .map(|&s| {
            let end = (s + block).min(n);
            let len = (end - s).max(1) as f64;
            channels.iter().map(|c| c[s..end].iter().map(|v| v * v).sum::<f64>() / len).sum()
        })
        .collect();
    let lufs = |p: f64| -0.691 + 10.0 * p.log10();
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let passed: Vec<f64> = power.iter().copied().filter(|&p| p > 0.0 && lufs(p) > -70.0).collect();
    if passed.is_empty() {
        return None;
    }
    let gate = lufs(mean(&passed)) - 10.0;
    let kept: Vec<f64> = passed.into_iter().filter(|&p| lufs(p) > gate).collect();
    (!kept.is_empty()).then(|| lufs(mean(&kept)))
}

fn mono(a: &Audio) -> Vec<f32> {
    a.frames.iter().map(|f| (f[0] + f[1]) * 0.5).collect()
}

/// Fundamental as a (fractional) MIDI note: YIN on up to five of the loudest
/// frames, the median of those that find a period.
pub fn pitch(a: &Audio) -> Option<f64> {
    const WINDOW: usize = 2048;
    let x = mono(a);
    let rate = f64::from(a.rate);
    let max_tau = ((rate / 25.0) as usize).min(WINDOW * 2 - 1);
    let min_tau = ((rate / 4_200.0) as usize).max(2);
    if x.len() < WINDOW + max_tau + 1 {
        return None;
    }
    let last = x.len() - WINDOW - max_tau - 1;
    let energy = |s: usize| x[s..s + WINDOW].iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
    let hops: Vec<usize> = (0..=last).step_by(WINDOW).collect();
    let mut loud: Vec<(usize, f64)> = hops.iter().map(|&s| (s, energy(s))).collect();
    loud.sort_by(|p, q| q.1.total_cmp(&p.1));
    let top = loud.first()?.1;
    let mut notes: Vec<f64> = loud
        .iter()
        .take(5)
        .filter(|p| p.1 > top * 0.1)
        .filter_map(|&(s, _)| yin(&x[s..s + WINDOW + max_tau + 1], WINDOW, min_tau, max_tau, rate))
        .collect();
    if notes.is_empty() {
        return None;
    }
    notes.sort_by(f64::total_cmp);
    Some(notes[notes.len() / 2])
}

fn yin(x: &[f32], window: usize, min_tau: usize, max_tau: usize, rate: f64) -> Option<f64> {
    let mut d = vec![0.0f64; max_tau + 1];
    for (tau, slot) in d.iter_mut().enumerate().skip(1) {
        *slot = (0..window).map(|j| (f64::from(x[j]) - f64::from(x[j + tau])).powi(2)).sum();
    }
    // Cumulative mean normalised difference.
    let mut sum = 0.0;
    let mut n = vec![1.0f64; max_tau + 1];
    for tau in 1..=max_tau {
        sum += d[tau];
        n[tau] = if sum > 0.0 { d[tau] * tau as f64 / sum } else { 1.0 };
    }
    let mut tau = min_tau;
    while tau < max_tau {
        if n[tau] < 0.15 {
            while tau + 1 < max_tau && n[tau + 1] < n[tau] {
                tau += 1;
            }
            // Parabolic refinement around the minimum.
            let (a, b, c) = (n[tau - 1], n[tau], n[tau + 1]);
            let shift = if (a - 2.0 * b + c).abs() > 1e-12 { 0.5 * (a - c) / (a - 2.0 * b + c) } else { 0.0 };
            let f = rate / (tau as f64 + shift);
            return Some(69.0 + 12.0 * (f / 440.0).log2());
        }
        tau += 1;
    }
    None
}

/// Krumhansl-Schmuckler major and minor key profiles.
const MAJOR: [f64; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR: [f64; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

/// The key as `$NI_DETECT_KEY_*` (0..12 C major upward, 12..24 C minor upward).
pub fn key(a: &Audio) -> Option<i32> {
    const FRAME: usize = 8192;
    let x = mono(a);
    let rate = f64::from(a.rate);
    let frames = (x.len() / FRAME).min(24);
    if frames == 0 {
        return None;
    }
    let mut chroma = [0.0f64; 12];
    for f in 0..frames {
        // Spread the frames over the whole sample.
        let start = f * (x.len() - FRAME) / frames.max(2).saturating_sub(1).max(1);
        let seg = &x[start.min(x.len() - FRAME)..][..FRAME];
        for note in 36..96 {
            let freq = 440.0 * 2f64.powf((f64::from(note) - 69.0) / 12.0);
            if freq > rate / 2.2 {
                break;
            }
            // Goertzel with a Hann window.
            let w = 2.0 * PI * freq / rate;
            let coeff = 2.0 * w.cos();
            let (mut s1, mut s2) = (0.0, 0.0);
            for (i, &v) in seg.iter().enumerate() {
                let hann = 0.5 - 0.5 * (2.0 * PI * i as f64 / FRAME as f64).cos();
                let s = f64::from(v) * hann + coeff * s1 - s2;
                s2 = s1;
                s1 = s;
            }
            chroma[(note % 12) as usize] += s1 * s1 + s2 * s2 - coeff * s1 * s2;
        }
    }
    if chroma.iter().sum::<f64>() <= 0.0 {
        return None;
    }
    let corr = |profile: &[f64; 12], shift: usize| {
        let rotated: Vec<f64> = (0..12).map(|i| profile[(i + 12 - shift) % 12]).collect();
        let (mc, mp) = (chroma.iter().sum::<f64>() / 12.0, rotated.iter().sum::<f64>() / 12.0);
        let num: f64 = (0..12).map(|i| (chroma[i] - mc) * (rotated[i] - mp)).sum();
        let den = ((0..12).map(|i| (chroma[i] - mc).powi(2)).sum::<f64>()
            * (0..12).map(|i| (rotated[i] - mp).powi(2)).sum::<f64>())
        .sqrt();
        if den > 0.0 { num / den } else { -1.0 }
    };
    (0..24)
        .map(|k| (k, corr(if k < 12 { &MAJOR } else { &MINOR }, k % 12)))
        .max_by(|p, q| p.1.total_cmp(&q.1))
        .map(|(k, _)| k as i32)
}

/// Tempo in beats per minute (60 to 200): the lag where the onset envelope
/// (the rise of 512-frame energies) repeats most strongly.
pub fn tempo(a: &Audio) -> Option<f64> {
    const HOP: usize = 512;
    let x = mono(a);
    let rate = f64::from(a.rate);
    if (x.len() as f64) < rate * 2.0 {
        return None;
    }
    let energy: Vec<f64> = x.chunks_exact(HOP).map(|c| c.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>()).collect();
    let rise: Vec<f64> = energy.windows(2).map(|w| (w[1].sqrt() - w[0].sqrt()).max(0.0)).collect();
    let per_second = rate / HOP as f64;
    let (lo, hi) = ((per_second * 60.0 / 200.0) as usize, (per_second * 60.0 / 60.0) as usize);
    let auto = |lag: usize| rise.iter().zip(&rise[lag..]).map(|(p, q)| p * q).sum::<f64>();
    let zero = auto(0);
    if zero <= 0.0 || hi + 1 >= rise.len() {
        return None;
    }
    let (lag, best) = (lo.max(1)..=hi).map(|l| (l, auto(l))).max_by(|p, q| p.1.total_cmp(&q.1))?;
    (best > 0.05 * zero).then(|| 60.0 * per_second / lag as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freqs: &[f64], amp: f64, seconds: f64, rate: u32) -> Vec<[f32; 2]> {
        (0..(seconds * f64::from(rate)) as usize)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                let v = freqs.iter().map(|f| (2.0 * PI * f * t).sin()).sum::<f64>() * amp / freqs.len() as f64;
                [v as f32; 2]
            })
            .collect()
    }

    #[test]
    fn levels_pitch_and_key_of_known_signals() {
        let rate = 44_100;
        let tone = sine(&[997.0], 0.1, 3.0, rate);
        let a = Audio { frames: &tone, rate };
        // BS.1770: a 997 Hz sine at -20 dBFS in both channels is -20.0 LUFS (the
        // two channels' powers add; the K shelf lifts that tone 0.69 dB against the
        // -0.691 offset).
        assert!((loudness(&a).unwrap() + 20.0).abs() < 0.1, "{:?}", loudness(&a));
        assert!((peak(&a).unwrap() + 20.0).abs() < 0.05);
        assert!((rms(&a).unwrap() + 23.0).abs() < 0.05);
        let a4 = sine(&[440.0], 0.5, 1.0, rate);
        assert!((pitch(&Audio { frames: &a4, rate }).unwrap() - 69.0).abs() < 0.1);
        let triad = sine(&[261.63, 329.63, 392.0], 0.8, 4.0, rate);
        assert_eq!(key(&Audio { frames: &triad, rate }), Some(0));
        let a_minor = sine(&[220.0, 261.63, 329.63], 0.8, 4.0, rate);
        assert_eq!(key(&Audio { frames: &a_minor, rate }), Some(12 + 9));
    }

    #[test]
    fn clicks_at_120_bpm_read_120() {
        let rate = 44_100;
        let mut clicks = vec![[0.0f32; 2]; rate as usize * 10];
        for beat in 0..20 {
            let at = beat * rate as usize / 2;
            for i in 0..200 {
                clicks[at + i] = [(1.0 - i as f32 / 200.0) * 0.8; 2];
            }
        }
        let bpm = tempo(&Audio { frames: &clicks, rate }).unwrap();
        assert!((bpm - 120.0).abs() < 3.0, "{bpm}");
        assert!(peak(&Audio { frames: &[[0.0; 2]; 100], rate }).is_none());
    }
}
