//! Music information retrieval for `detect_*` (KSP manual: Music Information
//! Retrieval): measurements of a zone's whole sample, run off the audio
//! thread. Levels follow ITU-R BS.1770-4 (K-weighted, gated loudness; the
//! peak is the sample peak, not the oversampled true peak); the pitch is
//! YIN (de Cheveigné and Kawahara 2002); the key and the tempo follow
//! the reference 8.13.1's own code (docs/re/MIR.md). The reference classifies sample, drum and instrument
//! types with a trained model KONTRA does not have: those always fail.

use std::f64::consts::PI;

use moose_dsp::biquad::{Biquad, k_weighting};
use moose_dsp::fft::{Complex, Fft};

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
    let sum: f64 = a
        .frames
        .iter()
        .flatten()
        .map(|&v| f64::from(v).powi(2))
        .sum();
    db(sum / (a.frames.len() * 2).max(1) as f64)
}

/// Integrated loudness, LUFS: 400 ms blocks every 100 ms, gated at -70 LUFS
/// and then 10 LU under the mean of what passed. A mono sample (`channels`
/// 1) counts once: the reference 8.13 reads a 440 Hz mono sine at -6.23 dBFS peak
/// as -9.961 LUFS (the detect_tone probe), not 3 dB louder.
pub fn loudness(a: &Audio, channels: u8) -> Option<f64> {
    let rate = f64::from(a.rate);
    let channels: Vec<Vec<f64>> = (0..channels.clamp(1, 2))
        .map(|c| {
            let x: Vec<f64> = a
                .frames
                .iter()
                .map(|&[l, r]| f64::from(if c == 0 { l } else { r }))
                .collect();
            let [mut shelf, mut high_pass] = k_weighting(rate).map(Biquad::new);
            x.iter()
                .map(|&v| high_pass.process(shelf.process(v)))
                .collect()
        })
        .collect();
    let block = (rate * 0.4) as usize;
    let step = ((rate * 0.1) as usize).max(1);
    let n = a.frames.len();
    let starts: Vec<usize> = if n < block {
        vec![0]
    } else {
        (0..=n - block).step_by(step).collect()
    };
    let power: Vec<f64> = starts
        .iter()
        .map(|&s| {
            let end = (s + block).min(n);
            let len = (end - s).max(1) as f64;
            channels
                .iter()
                .map(|c| {
                    c.get(s..end)
                        .map_or(0.0, |w| w.iter().map(|v| v * v).sum::<f64>() / len)
                })
                .sum()
        })
        .collect();
    let lufs = |p: f64| -0.691 + 10.0 * p.log10();
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let passed: Vec<f64> = power
        .iter()
        .copied()
        .filter(|&p| p > 0.0 && lufs(p) > -70.0)
        .collect();
    if passed.is_empty() {
        return None;
    }
    let gate = lufs(mean(&passed)) - 10.0;
    let kept: Vec<f64> = passed.into_iter().filter(|&p| lufs(p) > gate).collect();
    (!kept.is_empty()).then(|| lufs(mean(&kept)))
}

fn mono(a: &Audio) -> Vec<f32> {
    a.frames.iter().map(|&[l, r]| (l + r) * 0.5).collect()
}

/// Fundamental as a (fractional) MIDI note: YIN on up to five of the loudest
/// frames, the median of those that find a period.
pub fn pitch(a: &Audio) -> Option<f64> {
    const WINDOW: usize = 2048;
    let x = mono(a);
    let rate = f64::from(a.rate);
    let max_tau = ((rate / 25.0) as usize).min(WINDOW * 2 - 1);
    let min_tau = ((rate / 4_200.0) as usize).max(2);
    let reach = WINDOW + max_tau + 1;
    if x.len() < reach + 1 {
        return None;
    }
    let mut loud: Vec<(&[f32], f64)> = (0..=x.len() - reach)
        .step_by(WINDOW)
        .filter_map(|s| x.get(s..s + reach))
        .map(|w| {
            (
                w,
                w.iter().take(WINDOW).map(|v| f64::from(*v).powi(2)).sum(),
            )
        })
        .collect();
    loud.sort_by(|p, q| q.1.total_cmp(&p.1));
    let top = loud.first()?.1;
    let mut notes: Vec<f64> = loud
        .iter()
        .take(5)
        .filter(|p| p.1 > top * 0.1)
        .filter_map(|&(w, _)| yin(w, WINDOW, min_tau, max_tau, rate))
        .collect();
    notes.sort_by(f64::total_cmp);
    notes.get(notes.len() / 2).copied()
}

fn yin(x: &[f32], window: usize, min_tau: usize, max_tau: usize, rate: f64) -> Option<f64> {
    let difference = |tau: usize| -> f64 {
        x.iter()
            .zip(x.iter().skip(tau))
            .take(window)
            .map(|(p, q)| (f64::from(*p) - f64::from(*q)).powi(2))
            .sum()
    };
    // Cumulative mean normalised difference.
    let n: Vec<f64> = (0..=max_tau)
        .scan(0.0, |sum, tau| {
            if tau == 0 {
                return Some(1.0);
            }
            let d = difference(tau);
            *sum += d;
            Some(if *sum > 0.0 {
                d * tau as f64 / *sum
            } else {
                1.0
            })
        })
        .collect();
    let at = |t: usize| n.get(t).copied();
    let mut tau = (min_tau..max_tau).find(|&t| at(t).is_some_and(|v| v < 0.15))?;
    while tau + 1 < max_tau
        && at(tau + 1)
            .zip(at(tau))
            .is_some_and(|(next, here)| next < here)
    {
        tau += 1;
    }
    // Parabolic refinement around the minimum.
    let (a, b, c) = (at(tau.checked_sub(1)?)?, at(tau)?, at(tau + 1)?);
    let bend = a - 2.0 * b + c;
    let shift = if bend.abs() > 1e-12 {
        0.5 * (a - c) / bend
    } else {
        0.0
    };
    let f = rate / (tau as f64 + shift);
    Some(69.0 + 12.0 * (f / 440.0).log2())
}

/// The reference 8.13.1's key profiles (the reference binary [ref]..[ref], read by
/// [ref]), each with its mode: 0 major, 1 minor.
const PROFILES: [([f64; 12], i32); 5] = [
    (
        [
            3.1319, 0.1491, 1.8413, 0.1816, 3.8298, 0.2926, 0.3723, 4.0026, 0.838, 0.4966, 0.4935,
            1.9564,
        ],
        0,
    ),
    (
        [
            14.4599, 1.7079, 9.006, 1.9282, 11.8119, 4.6, 2.9245, 15.3782, 2.1456, 7.6573, 1.932,
            8.515,
        ],
        0,
    ),
    (
        [
            3.2564, 0.3292, 1.5121, 2.9781, 1.0332, 0.6649, 0.1437, 4.6696, 0.0274, 0.4966, 1.7293,
            0.7452,
        ],
        1,
    ),
    (
        [
            15.91, 0.8688, 8.978, 8.5319, 4.4277, 6.7738, 1.4509, 16.5083, 4.0324, 3.9101, 7.7925,
            2.8821,
        ],
        1,
    ),
    (
        [
            15.5378, 1.0974, 8.311, 9.3425, 4.4277, 5.538, 2.662, 16.3838, 3.8523, 4.2393, 4.9959,
            5.6787,
        ],
        1,
    ),
];

/// `x` at `to` Hz: windowed-sinc interpolation, cut off below both Nyquists.
// ponytail: the reference resamples with its own polyphase filter; a key
// is an argmax over whole-sample chroma, so the filter's shape does not move it.
fn resample(x: &[f32], from: u32, to: u32) -> Vec<f64> {
    if from == to {
        return x.iter().map(|&v| f64::from(v)).collect();
    }
    const TAPS: i64 = 16;
    let ratio = f64::from(from) / f64::from(to);
    let cut = 0.95 / ratio.max(1.0);
    let len = (x.len() as f64 / ratio).ceil() as usize;
    (0..len)
        .map(|i| {
            let t = i as f64 * ratio;
            let c = t.floor() as i64;
            let span = (TAPS as f64 * ratio.max(1.0)).ceil() as i64;
            (c - span + 1..=c + span)
                .filter_map(|j| Some((j, f64::from(*x.get(usize::try_from(j).ok()?)?))))
                .map(|(j, v)| {
                    let d = t - j as f64;
                    let w = 0.5 + 0.5 * (PI * d / span as f64).cos();
                    let s = if d == 0.0 {
                        1.0
                    } else {
                        (PI * cut * d).sin() / (PI * cut * d)
                    };
                    v * cut * s * w
                })
                .sum()
        })
        .collect()
}

/// `v` less its mean, over its standard deviation (n − 1), as [ref].
fn standardised(v: &[f64; 12]) -> [f64; 12] {
    let mean = v.iter().sum::<f64>() / 12.0;
    let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / 11.0;
    let sd = var.sqrt().max(f64::from(f32::EPSILON));
    v.map(|x| (x - mean) / sd)
}

/// The key as an index (0..12 C major upward, 12..24 C minor upward), the way
/// the reference 8.13.1 finds it (ni::kreator::mir::detectKey, the reference binary
/// [ref], docs/re/MIR.md): the channel mean at 22050 Hz, cut in
/// Hann-windowed 5513-sample frames every 2756 samples into an 8192-point
/// spectrum; 96 Gaussian semitone bands from MIDI 27, in dB above -80 less a
/// hearing curve and less what each band's neighbours mask; the bands from
/// MIDI 33 to 116 folded to a chroma that is averaged, standardised and
/// matched to five profiles at every root, minor ones weighted 1.05. None when
/// the mean power is under -60 dB.
pub fn key(a: &Audio) -> Option<i32> {
    const N: usize = 5513;
    const HOP: usize = 2756;
    const FFT: usize = 8192;
    const BANDS: usize = 96;
    let x = resample(&mono(a), a.rate, 22050);
    let power = x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64;
    if x.is_empty() || 10.0 * power.log10() < -60.0 {
        return None;
    }
    let bin_hz = 22050.0 / FFT as f64;
    // Band n is MIDI note 27 + n; a Gaussian a quarter semitone wide, cut at 3σ
    // and scaled to unit sum.
    let bank: Vec<(usize, Vec<f64>)> = (0..BANDS)
        .map(|n| {
            let fc = 440.0 * 2f64.powf((27 + n) as f64 / 12.0 - 69.0 / 12.0);
            let sigma = (fc - fc * 2f64.powf(-1.0 / 12.0)) * 0.25;
            let lo = ((fc - 3.0 * sigma) / bin_hz).ceil().max(0.0) as usize;
            let hi = (((fc + 3.0 * sigma) / bin_hz).floor() as usize).min(FFT / 2 - 1);
            let w: Vec<f64> = (lo..=hi)
                .map(|k| (-0.5 * ((k as f64 * bin_hz - fc) / sigma).powi(2)).exp())
                .collect();
            let sum: f64 = w.iter().sum();
            (
                lo,
                w.into_iter()
                    .map(|v| if sum > 0.0 { v / sum } else { v })
                    .collect(),
            )
        })
        .collect();
    let window: Vec<f64> = (0..N)
        .map(|i| 0.5 + 0.5 * (PI * (2.0 * i as f64 / (N - 1) as f64 - 1.0)).cos())
        .collect();
    let mut fft = Fft::<f64>::new(FFT);
    let mut buf = vec![Complex::new(0.0, 0.0); FFT];
    let mut frame = |seg: &[f64]| {
        buf.fill(Complex::new(0.0, 0.0));
        for ((b, &s), &w) in buf.iter_mut().zip(seg).zip(&window) {
            b.re = s * w;
        }
        fft.forward(&mut buf);
        let mut db = [0.0f64; BANDS];
        for (n, (lo, w)) in bank.iter().enumerate() {
            let e: f64 = w
                .iter()
                .zip(&buf[*lo..])
                .map(|(w, c)| w * c.norm() / 4096.0)
                .sum();
            let curve = 63.706_027_98 - 30.534_534_45 * ((n + 27) as f64).log10();
            db[n] = (20.0 * e.max(1e-4).log10() + 80.0 - curve).max(0.0);
        }
        // Each band masks its two neighbours either side, 2/3 then 1/3 of its
        // level.
        let mut masked = [0.0f64; BANDS];
        for (c, m) in masked.iter_mut().enumerate() {
            let spread: f64 = (c.saturating_sub(2)..(c + 3).min(BANDS))
                .filter(|&r| r != c)
                .map(|r| 10f64.powf((1.0 - r.abs_diff(c) as f64 / 3.0) * db[r] * 0.05) - 1.0)
                .sum();
            *m = (db[c] - 20.0 * (spread + 1.0).log10()).max(0.0);
        }
        let mut chroma = [0.0f64; 12];
        for (i, v) in masked.iter().enumerate().take(90).skip(6) {
            chroma[(i + 3) % 12] += v;
        }
        chroma
    };
    let mut sum = [0.0f64; 12];
    let mut add = |c: [f64; 12]| sum.iter_mut().zip(c).for_each(|(s, c)| *s += c);
    if x.len() < N {
        let mut padded = x.clone();
        padded.resize(N, 0.0);
        add(frame(&padded));
    } else {
        for start in (0..=x.len() - N).step_by(HOP) {
            add(frame(&x[start..start + N]));
        }
    }
    let chroma = standardised(&sum);
    let mut best = (-1.0, None);
    for (profile, mode) in &PROFILES {
        let p = standardised(profile);
        let weight = if *mode == 1 { 1.05 } else { 1.0 };
        for root in 0..12 {
            let score = weight
                * (0..12)
                    .map(|k| chroma[k] * p[(k + 12 - root) % 12])
                    .sum::<f64>()
                / 12.0;
            if score > best.0 {
                best = (score, Some(root as i32 + 12 * mode));
            }
        }
    }
    best.1
}

/// Tempo in beats per minute the way the reference 8.13.1 reads it
/// (ni::kreator::mir::detectTempo, the reference binary [ref] → [ref]
/// with its flag 1 → [ref]): not from the beats it tracks but from the
/// sample's length, as the power-of-two multiple of one beat per sample that
/// lies nearest 120 (between 60 and 300; on a tie the slower); 0 for a sample
/// of 5 ms or less.
pub fn tempo(a: &Audio) -> Option<f64> {
    if a.frames.is_empty() {
        return None;
    }
    // The length at 22050 Hz, rounded up as the resampler does: 0.75 s at
    // 44.1 kHz is 16538 samples, so it reads 159.995 (the detect_laws probe).
    let seconds = (a.frames.len() as f64 * 22050.0 / f64::from(a.rate)).ceil() / 22050.0;
    if seconds <= 0.005 {
        return Some(0.0);
    }
    let mut bpm = 60.0 / seconds;
    while bpm >= 60.0 {
        bpm *= 0.5;
    }
    let mut best = f64::INFINITY;
    loop {
        bpm *= 2.0;
        if (120.0 - bpm).abs() < (120.0 - best).abs() {
            best = bpm;
        }
        if bpm > 300.0 {
            break Some(best);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freqs: &[f64], amp: f64, seconds: f64, rate: u32) -> Vec<[f32; 2]> {
        (0..(seconds * f64::from(rate)) as usize)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                let v = freqs.iter().map(|f| (2.0 * PI * f * t).sin()).sum::<f64>() * amp
                    / freqs.len() as f64;
                [v as f32; 2]
            })
            .collect()
    }

    #[test]
    fn levels_pitch_and_key_of_known_signals() {
        let rate = 44_100;
        let tone = sine(&[997.0], 0.1, 3.0, rate);
        let a = Audio {
            frames: &tone,
            rate,
        };
        // BS.1770: a 997 Hz sine at -20 dBFS in both channels is -20.0 LUFS (the
        // two channels' powers add; the K shelf lifts that tone 0.69 dB against the
        // -0.691 offset).
        assert!(
            (loudness(&a, 2).unwrap() + 20.0).abs() < 0.1,
            "{:?}",
            loudness(&a, 2)
        );
        assert!((peak(&a).unwrap() + 20.0).abs() < 0.05);
        assert!((rms(&a).unwrap() + 23.0).abs() < 0.05);
        let a4 = sine(&[440.0], 0.5, 1.0, rate);
        assert!((pitch(&Audio { frames: &a4, rate }).unwrap() - 69.0).abs() < 0.1);
        let triad = sine(&[261.63, 329.63, 392.0], 0.8, 4.0, rate);
        assert_eq!(
            key(&Audio {
                frames: &triad,
                rate
            }),
            Some(0)
        );
        let a_minor = sine(&[220.0, 261.63, 329.63], 0.8, 4.0, rate);
        assert_eq!(
            key(&Audio {
                frames: &a_minor,
                rate
            }),
            Some(12 + 9)
        );
    }

    #[test]
    fn tempo_is_the_length_law() {
        let rate = 44_100;
        let bpm = |seconds: f64| {
            let frames = vec![[0.0f32; 2]; (seconds * f64::from(rate)) as usize];
            tempo(&Audio {
                frames: &frames,
                rate,
            })
        };
        // 1 s: 60, 120, 240, 480 -> 120; 2.5 s: 48, 96, 192, 384 -> 96; 0.75 s
        // is 16538 samples at 22050 Hz: 79.9975 or 159.995 -> the faster;
        // 10 s: ... 96, 192 -> 96 (the reference 8.13, the detect_laws probe).
        assert_eq!(bpm(1.0), Some(120.0));
        assert_eq!(bpm(2.5), Some(96.0));
        assert!((bpm(0.75).unwrap() - 159.995).abs() < 0.001);
        assert_eq!(bpm(10.0), Some(96.0));
        assert_eq!(bpm(0.003), Some(0.0));
        assert_eq!(bpm(0.0), None);
        assert!(
            peak(&Audio {
                frames: &[[0.0; 2]; 100],
                rate
            })
            .is_none()
        );
    }
}
