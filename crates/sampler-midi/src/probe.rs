//! Cheap, non-realtime check that an instrument answers MPE per-note
//! expression: one note per run on a member channel, then a +2 semitone bend
//! or full channel pressure sent after it has started.
use crate::{Mpe, Packets, Zone};
use sampler_core::{Error, Runtime};

/// What the rendered second window showed against an untouched note.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MpeResponse {
    /// Frequency ratio the bend moved the spectrum by (1.12 for a clean +2 st),
    /// found by correlating the log band powers of the bent and plain renders.
    pub pitch_ratio: f64,
    /// RMS with pressure at full over RMS without, in dB.
    pub pressure_db: f64,
    /// Spectral centroid with timbre (CC74) at 0 over without; below 1 is darker.
    pub timbre_ratio: f64,
}
impl MpeResponse {
    /// Pitch moved up by about 1 to 3 semitones' worth of crossings.
    pub fn pitch_responds(&self) -> bool {
        (1.06..=1.19).contains(&self.pitch_ratio)
    }
    /// Timbre at 0 darkened the spectrum by at least 5 percent.
    pub fn timbre_responds(&self) -> bool {
        self.timbre_ratio < 0.95
    }
    /// Level moved by at least 1 dB.
    pub fn pressure_responds(&self) -> bool {
        self.pressure_db.abs() >= 1.0
    }
}

const WINDOW: usize = 8192;

/// `fresh` builds a new runtime for each of the three runs. The note plays on
/// lower-zone member channel 1 (member bend range 48, so +2 st is 8533).
pub fn mpe_response(mut fresh: impl FnMut() -> Runtime, key: u8) -> Result<MpeResponse, Error> {
    let mut run = |send: Option<u32>| -> Result<Vec<[f32; 2]>, Error> {
        let mut rt = fresh();
        let mut mpe = Mpe::new(&rt, 0, 0, Zone::Lower, 2, 8)?;
        let mut apply = |rt: &mut Runtime, word: u32| {
            let words = [word];
            let packet = Packets::new(&words).next().ok_or(Error::InvalidInput)?;
            let packet = packet.map_err(|_| Error::InvalidInput)?;
            mpe.apply(rt, packet).map_err(|_| Error::InvalidInput)
        };
        apply(&mut rt, 0x2091_0000 | u32::from(key) << 8 | 100)?;
        let mut out = vec![[0.0; 2]; 2 * WINDOW];
        rt.render(&mut out[..WINDOW])?;
        if let Some(word) = send {
            apply(&mut rt, word)?;
        }
        rt.render(&mut out[WINDOW..])?;
        out.drain(..WINDOW);
        Ok(out)
    };
    let plain = run(None)?;
    let bent = run(Some(0x20E1_0000 | (8533 & 127) << 8 | 8533 >> 7))?;
    let pressed = run(Some(0x20D1_0000 | 127 << 8))?;
    let dark = run(Some(0x20B1_0000 | 74 << 8))?;
    let rms = |x: &[[f32; 2]]| {
        let s: f64 = x
            .iter()
            .map(|f| f64::from(f[0]).powi(2) + f64::from(f[1]).powi(2))
            .sum();
        (s / (2 * x.len()) as f64).sqrt()
    };
    Ok(MpeResponse {
        pitch_ratio: spectral_ratio(&plain, &bent),
        pressure_db: 20.0 * ((rms(&pressed) + 1e-12) / (rms(&plain) + 1e-12)).log10(),
        timbre_ratio: centroid(&dark) / centroid(&plain).max(1e-9),
    })
}

// Bands start where one is wider than an 8192-point FFT bin (5.9 Hz).
const BANDS_PER_OCTAVE: usize = 24;
const LOW_HZ: f64 = 400.0;
const OCTAVES: usize = 5;

/// In-place radix-2 FFT of `re`/`im` (length a power of two).
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * std::f64::consts::PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (angle * k as f64).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                (re[b], im[b]) = (re[a] - tr, im[a] - ti);
                (re[a], im[a]) = (re[a] + tr, im[a] + ti);
            }
        }
        len <<= 1;
    }
}

/// Log power in log-spaced bands from `LOW_HZ` up: Hann-windowed mid signal.
/// Banding sums bins, so noise-like material gives a stable spectral shape.
fn log_bands(x: &[[f32; 2]], rate: f64) -> Vec<f64> {
    let n = x.len();
    let mut re: Vec<f64> = x
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos();
            0.5 * (f64::from(f[0]) + f64::from(f[1])) * w
        })
        .collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    let bands = BANDS_PER_OCTAVE * OCTAVES;
    let mut power = vec![0.0; bands];
    for k in 1..n / 2 {
        let hz = k as f64 * rate / n as f64;
        let band = (hz / LOW_HZ).log2() * BANDS_PER_OCTAVE as f64;
        if band >= 0.0 && (band as usize) < bands {
            power[band as usize] += re[k] * re[k] + im[k] * im[k];
        }
    }
    power.iter().map(|p| (p + 1e-12).ln()).collect()
}

/// The frequency ratio that best maps `plain`'s spectrum onto `bent`'s,
/// within +-6 semitones; 0 when either is silent. Rate is the runtime's 48 kHz.
pub fn spectral_ratio(plain: &[[f32; 2]], bent: &[[f32; 2]]) -> f64 {
    const RATE: f64 = 48_000.0;
    let (a, b) = (log_bands(plain, RATE), log_bands(bent, RATE));
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let (ma, mb) = (mean(&a), mean(&b));
    // Silent renders sit at the floor: nothing to compare.
    if a.iter().all(|v| *v < -20.0) || b.iter().all(|v| *v < -20.0) {
        return 0.0;
    }
    let reach = (BANDS_PER_OCTAVE / 2) as isize;
    let score = |shift: isize| -> f64 {
        let mut sum = 0.0;
        for (i, x) in a.iter().enumerate() {
            let j = i as isize + shift;
            if j >= 0 && (j as usize) < b.len() {
                sum += (x - ma) * (b[j as usize] - mb);
            }
        }
        sum
    };
    let best = (-reach..=reach)
        .max_by(|&p, &q| score(p).total_cmp(&score(q)))
        .unwrap_or(0);
    // A parabola through the peak and its neighbours gives sub-band shifts.
    let (l, c, r) = (score(best - 1), score(best), score(best + 1));
    let denom = l - 2.0 * c + r;
    let offset = if denom < 0.0 {
        0.5 * (l - r) / denom
    } else {
        0.0
    };
    ((best as f64 + offset) / BANDS_PER_OCTAVE as f64).exp2()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(f0: f64, harmonics: usize) -> Vec<[f32; 2]> {
        (0..WINDOW)
            .map(|i| {
                let t = i as f64 / 48_000.0;
                let v: f64 = (1..=harmonics)
                    .map(|h| (std::f64::consts::TAU * f0 * h as f64 * t).sin() / h as f64)
                    .sum();
                [v as f32; 2]
            })
            .collect()
    }

    #[test]
    fn the_spectral_ratio_reads_two_semitones_and_rest() {
        let up = 2f64.powf(2.0 / 12.0);
        let r = spectral_ratio(&tone(330.0, 20), &tone(330.0 * up, 20));
        assert!((r - up).abs() < 0.02, "{r}");
        let same = spectral_ratio(&tone(330.0, 20), &tone(330.0, 20));
        assert!((same - 1.0).abs() < 0.01, "{same}");
        assert_eq!(
            spectral_ratio(&tone(330.0, 20), &vec![[0.0; 2]; WINDOW]),
            0.0
        );
    }
}

/// Power-weighted mean frequency of the mid signal, Hz (0 when silent).
pub fn centroid(x: &[[f32; 2]]) -> f64 {
    let n = x.len();
    let mut re: Vec<f64> = x
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos();
            0.5 * (f64::from(f[0]) + f64::from(f[1])) * w
        })
        .collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    let (mut num, mut den) = (0.0, 0.0);
    for k in 1..n / 2 {
        let p = re[k] * re[k] + im[k] * im[k];
        num += p * k as f64;
        den += p;
    }
    if den < 1e-12 {
        0.0
    } else {
        num / den * 48_000.0 / n as f64
    }
}
