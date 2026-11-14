//! Music information retrieval for `detect_*`: what a script may ask of a
//! zone's sample, measured once at load on the control thread so the audio
//! thread only reads numbers.
//!
//! Level, RMS, peak and loudness follow the standards the KSP manual cites
//! (ITU-R BS.1770-4: K-weighting, 400 ms blocks every 100 ms, absolute gate
//! -70 LKFS, relative gate -10 LU, 4x true peak). Pitch is YIN, tempo the
//! autocorrelation of a spectral-flux onset curve, key the correlation of
//! a chroma vector with the Krumhansl-Schmuckler profiles. Native
//! Instruments' sample, drum and instrument type detectors are trained
//! classifiers; the rules here are stand-ins (short, noisy and bright is a
//! drum; sustained and pitched is an instrument), and answer "invalid" when
//! the rules cannot tell.

use std::f64::consts::PI;
use std::io;

use kontra_engine::Analysis;
use kontra_stream::{Encoding, SampleFmt, Source};

/// Longest stretch analysed; one-shots are shorter, loops and phrases
/// are judged by their first half minute.
const MAX_SECONDS: u64 = 30;

/// `x[i]` as a number, 0 past the end.
fn at(x: &[f64], i: usize) -> f64 {
    x.get(i).copied().unwrap_or(0.0)
}

/// `x[from..to]`, empty when out of range.
fn span<T>(x: &[T], from: usize, to: usize) -> &[T] {
    x.get(from..to).unwrap_or_default()
}

/// Decodes up to [`MAX_SECONDS`] of `source` into one `Vec<f64>` per channel.
fn decode(source: &dyn Source, fmt: SampleFmt, frames: u64, rate: u32) -> io::Result<Vec<Vec<f64>>> {
    let channels = usize::from(fmt.channels.max(1));
    let width = fmt.encoding.bytes();
    let frames = frames.min(MAX_SECONDS * u64::from(rate));
    let mut out = vec![Vec::with_capacity(usize::try_from(frames).unwrap_or(0)); channels];
    let chunk = 16_384usize;
    let mut bytes = vec![0u8; chunk * channels * width];
    let mut from = 0u64;
    while from < frames {
        let n = usize::try_from(frames - from).unwrap_or(chunk).min(chunk);
        let bytes = bytes.get_mut(..n * channels * width).unwrap_or_default();
        source.read(from, bytes)?;
        for frame in bytes.chunks_exact(channels * width) {
            for (ch, v) in out.iter_mut().zip(frame.chunks_exact(width)) {
                ch.push(match (fmt.encoding, v) {
                    (Encoding::I16, &[a, b]) => f64::from(i16::from_le_bytes([a, b])) / 32_768.0,
                    (Encoding::I24, &[a, b, c]) => {
                        f64::from(i32::from_le_bytes([0, a, b, c]) >> 8) / 8_388_608.0
                    }
                    (_, &[a, b, c, d]) => f64::from(f32::from_le_bytes([a, b, c, d])),
                    _ => 0.0,
                });
            }
        }
        from += n as u64;
    }
    Ok(out)
}

/// Analyses a sample, `None` when it cannot be read or is empty.
pub fn analyse(source: &dyn Source, fmt: SampleFmt, frames: u64, rate: u32) -> Option<Analysis> {
    let channels = decode(source, fmt, frames, rate).ok()?;
    let rate = f64::from(rate.max(1));
    let len = channels.iter().map(Vec::len).min().filter(|&n| n > 0)?;
    let mono: Vec<f64> = (0..len)
        .map(|i| channels.iter().map(|c| at(c, i)).sum::<f64>() / channels.len() as f64)
        .collect();
    let db = |x: f64| if x > 1e-12 { 20.0 * x.log10() } else { -200.0 };
    let power: f64 = channels.iter().flatten().map(|x| x * x).sum();
    let rms = (power / (len * channels.len()) as f64).sqrt();
    let peak = channels.iter().map(|c| true_peak(c)).fold(0.0, f64::max);
    let (pitch, clarity) = pitch(&mono, rate);
    let shape = Shape::of(&mono, rate);
    let drum = shape.is_drum(clarity);
    let judged = shape.valid();
    Some(Analysis {
        peak_db: db(peak),
        rms_db: db(rms),
        loudness_lkfs: loudness(&channels, rate),
        pitch,
        tempo: tempo(&mono, rate),
        key: key(&mono, rate),
        is_drum: judged.then_some(drum),
        drum_type: (judged && drum).then(|| shape.drum_type(pitch.is_some())),
        instrument_type: if judged && !drum { shape.instrument_type(pitch) } else { None },
    })
}

/// A biquad in direct form I.
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// The two K-weighting stages of BS.1770-4 for any rate (libebur128's
/// derivation from the analogue prototypes).
fn k_weighting(rate: f64) -> [Biquad; 2] {
    let stage = |f0: f64, q: f64, b: &dyn Fn(f64, f64) -> [f64; 3]| {
        let k = (PI * f0 / rate).tan();
        let a0 = 1.0 + k / q + k * k;
        Biquad {
            b: b(k, a0),
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    };
    let (g, q) = (3.999_843_853_973_347, 0.707_175_236_955_419_6);
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    [
        stage(1_681.974_450_955_533, q, &|k, a0| {
            [
                (vh + vb * k / q + k * k) / a0,
                2.0 * (k * k - vh) / a0,
                (vh - vb * k / q + k * k) / a0,
            ]
        }),
        stage(38.135_470_876_024_44, 0.500_327_037_323_877_3, &|_, _| [1.0, -2.0, 1.0]),
    ]
}

/// Integrated loudness in LKFS (BS.1770-4); an ungated whole-signal block
/// when the sample is shorter than one 400 ms block.
fn loudness(channels: &[Vec<f64>], rate: f64) -> f64 {
    let weighted: Vec<Vec<f64>> = channels
        .iter()
        .map(|c| {
            let mut stages = k_weighting(rate);
            c.iter().map(|&x| stages.iter_mut().fold(x, |v, s| s.run(v))).collect()
        })
        .collect();
    let len = weighted.first().map_or(0, Vec::len);
    let (block, hop) = ((0.4 * rate) as usize, ((0.1 * rate) as usize).max(1));
    let energy = |from: usize, to: usize| -> f64 {
        let n = to.saturating_sub(from).max(1) as f64;
        weighted.iter().map(|c| span(c, from, to).iter().map(|v| v * v).sum::<f64>() / n).sum()
    };
    let lkfs = |e: f64| -0.691 + 10.0 * e.max(1e-12).log10();
    if len < block {
        return lkfs(energy(0, len));
    }
    let blocks = (0..=(len - block) / hop).map(|i| energy(i * hop, i * hop + block));
    let loud: Vec<f64> = blocks.filter(|&e| lkfs(e) > -70.0).collect();
    if loud.is_empty() {
        return lkfs(0.0);
    }
    let relative = lkfs(loud.iter().sum::<f64>() / loud.len() as f64) - 10.0;
    let kept: Vec<f64> = loud.into_iter().filter(|&e| lkfs(e) > relative).collect();
    lkfs(kept.iter().sum::<f64>() / kept.len().max(1) as f64)
}

/// The largest magnitude after 4x oversampling with a 16-tap windowed sinc
/// (BS.1770-4 annex 2's true-peak meter).
fn true_peak(x: &[f64]) -> f64 {
    const TAPS: i64 = 8;
    let kernels: Vec<Vec<f64>> = (1..4)
        .map(|phase| {
            let frac = f64::from(phase) / 4.0;
            (0..2 * TAPS)
                .map(|i| {
                    let t = (i - TAPS + 1) as f64 - frac;
                    let sinc = if t == 0.0 { 1.0 } else { (PI * t).sin() / (PI * t) };
                    sinc * (0.5 + 0.5 * (PI * t / TAPS as f64).cos())
                })
                .collect()
        })
        .collect();
    let mut peak = x.iter().fold(0.0f64, |p, v| p.max(v.abs()));
    for i in 0..x.len() as i64 {
        for k in &kernels {
            let sum: f64 = k
                .iter()
                .enumerate()
                .map(|(j, c)| {
                    let at_ = usize::try_from(i + j as i64 - TAPS + 1).unwrap_or(usize::MAX);
                    at(x, at_) * c
                })
                .sum();
            peak = peak.max(sum.abs());
        }
    }
    peak
}

/// The fundamental in MIDI notes (fractional) and how clean the periodicity
/// is (YIN's smallest normalised difference, 0 perfect, 1 noise). `None`
/// when no dip falls under 0.15.
fn pitch(x: &[f64], rate: f64) -> (Option<f64>, f64) {
    let tau_min = ((rate / 4_200.0) as usize).max(2);
    let tau_max = (rate / 27.5) as usize + 1;
    let window = 2048.max(tau_max);
    if x.len() < window + tau_max + 2 {
        return (None, 1.0);
    }
    // The loudest window in the first two seconds.
    let limit = x.len().min((2.0 * rate) as usize).max(window + tau_max + 1) - window - tau_max;
    let energy = |from: usize| span(x, from, from + window).iter().map(|v| v * v).sum::<f64>();
    let from = (0..limit).step_by(1024).max_by(|&a, &b| energy(a).total_cmp(&energy(b))).unwrap_or(0);
    let frame = span(x, from, from + window + tau_max);
    let diff = |tau: usize| -> f64 {
        frame.iter().zip(frame.iter().skip(tau)).take(window).map(|(a, b)| (a - b) * (a - b)).sum()
    };
    let mut running = 0.0;
    let mut cmnd = vec![1.0f64; tau_max + 2];
    for tau in 1..=tau_max {
        let d = diff(tau);
        running += d;
        if let Some(slot) = cmnd.get_mut(tau) {
            *slot = if running > 0.0 { d * tau as f64 / running } else { 1.0 };
        }
    }
    let floor = cmnd.iter().skip(tau_min).copied().fold(1.0, f64::min);
    let mut tau = tau_min;
    while tau < tau_max {
        if at(&cmnd, tau) < 0.15 {
            while tau + 1 < tau_max && at(&cmnd, tau + 1) < at(&cmnd, tau) {
                tau += 1;
            }
            let (a, b, c) = (at(&cmnd, tau - 1), at(&cmnd, tau), at(&cmnd, tau + 1));
            let curve = a - 2.0 * b + c;
            let shift = if curve.abs() > 1e-12 { 0.5 * (a - c) / curve } else { 0.0 };
            let hz = rate / (tau as f64 + shift);
            return (Some(69.0 + 12.0 * (hz / 440.0).log2()), floor);
        }
        tau += 1;
    }
    (None, floor)
}

/// In-place radix-2 FFT of `re`/`im`, a power-of-two length.
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
        let ang = -2.0 * PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (br, bi) = (at(re, b), at(im, b));
                let (tr, ti) = (br * cr - bi * ci, br * ci + bi * cr);
                let (ar, ai) = (at(re, a), at(im, a));
                for (slot, v) in [(&mut *re, (a, ar + tr, b, ar - tr)), (&mut *im, (a, ai + ti, b, ai - ti))] {
                    if let Some(s) = slot.get_mut(v.0) {
                        *s = v.1;
                    }
                    if let Some(s) = slot.get_mut(v.2) {
                        *s = v.3;
                    }
                }
                (cr, ci) = (cr * wr - ci * wi, cr * wi + ci * wr);
            }
        }
        len <<= 1;
    }
}

/// Magnitude spectra (below Nyquist) of Hann-windowed frames of `size` every `hop`.
fn spectra(x: &[f64], size: usize, hop: usize) -> Vec<Vec<f64>> {
    let window: Vec<f64> = (0..size).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / size as f64).cos()).collect();
    (0..)
        .map(|i| i * hop)
        .take_while(|&from| from + size <= x.len())
        .map(|from| {
            let mut re: Vec<f64> = span(x, from, from + size).iter().zip(&window).map(|(a, w)| a * w).collect();
            let mut im = vec![0.0; size];
            fft(&mut re, &mut im);
            re.iter().zip(&im).take(size / 2).map(|(a, b)| a.hypot(*b)).collect()
        })
        .collect()
}

/// Tempo in BPM (60..200) from the autocorrelation of the spectral flux,
/// weighted towards 120; `None` for under two seconds or no clear beat.
fn tempo(x: &[f64], rate: f64) -> Option<f64> {
    if (x.len() as f64) < 2.0 * rate {
        return None;
    }
    let hop = 512;
    let frames = spectra(x, 1024, hop);
    let flux: Vec<f64> = frames
        .windows(2)
        .map(|w| match w {
            [a, b] => b.iter().zip(a).map(|(b, a)| (b - a).max(0.0)).sum(),
            _ => 0.0,
        })
        .collect();
    let mean = flux.iter().sum::<f64>() / flux.len().max(1) as f64;
    let flux: Vec<f64> = flux.iter().map(|v| v - mean).collect();
    let zero: f64 = flux.iter().map(|v| v * v).sum();
    if zero <= 0.0 {
        return None;
    }
    let per_second = rate / hop as f64;
    let lag = |bpm: f64| (per_second * 60.0 / bpm).round() as usize;
    let mut best = (0.0, 0.0);
    for l in lag(200.0)..=lag(60.0).min(flux.len().saturating_sub(1)) {
        let r = flux.iter().zip(flux.iter().skip(l)).map(|(a, b)| a * b).sum::<f64>() / zero;
        let bpm = per_second * 60.0 / l as f64;
        let prior = (-(0.5 * ((bpm / 120.0).log2() / 0.6).powi(2))).exp();
        if r * prior > best.1 {
            best = (bpm, r * prior);
        }
    }
    (best.1 > 0.1).then_some(best.0)
}

/// Krumhansl-Schmuckler key profiles (major, minor), tonic first.
const MAJOR: [f64; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR: [f64; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

/// The key as 0..12 (C..B major) or 12..24 (C..B minor); `None` under a
/// second or when no profile correlates above 0.5.
fn key(x: &[f64], rate: f64) -> Option<u8> {
    if (x.len() as f64) < rate {
        return None;
    }
    let size = 8192;
    let mut chroma = [0.0f64; 12];
    for frame in spectra(x, size, size / 2) {
        for (k, m) in frame.iter().enumerate().skip(1) {
            let hz = k as f64 * rate / size as f64;
            if (65.0..2_100.0).contains(&hz) {
                let note = (12.0 * (hz / 440.0).log2() + 69.0).round() as i64;
                if let Some(c) = chroma.get_mut(note.rem_euclid(12) as usize) {
                    *c += m;
                }
            }
        }
    }
    let corr = |profile: &[f64; 12], tonic: usize| -> f64 {
        let rotated: Vec<f64> = (0..12).map(|i| at(&chroma, (i + tonic) % 12)).collect();
        let (mc, mp) = (rotated.iter().sum::<f64>() / 12.0, profile.iter().sum::<f64>() / 12.0);
        let num: f64 = rotated.iter().zip(profile).map(|(a, b)| (a - mc) * (b - mp)).sum();
        let sa: f64 = rotated.iter().map(|a| (a - mc).powi(2)).sum();
        let sb: f64 = profile.iter().map(|b| (b - mp).powi(2)).sum();
        if sa * sb > 0.0 { num / (sa * sb).sqrt() } else { 0.0 }
    };
    let (best, r) = (0..24u8)
        .map(|k| (k, corr(if k < 12 { &MAJOR } else { &MINOR }, usize::from(k % 12))))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    (r > 0.5).then_some(best)
}

/// The envelope and colour numbers the type rules read.
struct Shape {
    seconds: f64,
    /// Seconds from the start to 90 % of the peak.
    attack: f64,
    /// Seconds from the peak until the envelope is 40 dB down.
    decay: f64,
    /// Mean spectral centroid in Hz.
    centroid: f64,
    peak_db: f64,
}

impl Shape {
    fn of(x: &[f64], rate: f64) -> Self {
        let win = ((0.005 * rate) as usize).max(1);
        let env: Vec<f64> = x
            .chunks(win)
            .map(|c| (c.iter().map(|v| v * v).sum::<f64>() / c.len() as f64).sqrt())
            .collect();
        let peak = env.iter().copied().fold(0.0, f64::max);
        let attack = env.iter().position(|&e| e >= 0.9 * peak).unwrap_or(0);
        let top = env.iter().position(|&e| e >= peak).unwrap_or(0);
        let down = env.iter().skip(top).position(|&e| e < peak * 0.01).unwrap_or(env.len().saturating_sub(top));
        let (mut num, mut den) = (0.0, 0.0);
        for frame in spectra(x, 2048, 1024) {
            for (k, m) in frame.iter().enumerate() {
                num += k as f64 * rate / 2048.0 * m;
                den += m;
            }
        }
        let secs = |n: usize| n as f64 * win as f64 / rate;
        Self {
            seconds: x.len() as f64 / rate,
            attack: secs(attack),
            decay: secs(down),
            centroid: if den > 0.0 { num / den } else { 0.0 },
            peak_db: 20.0 * peak.max(1e-9).log10(),
        }
    }

    /// Long enough and loud enough to judge.
    fn valid(&self) -> bool {
        self.seconds >= 0.05 && self.peak_db > -60.0
    }

    /// Short, quick and not clearly periodic reads as a drum.
    fn is_drum(&self, clarity: f64) -> bool {
        (self.seconds < 2.5 && clarity > 0.2) || (self.attack < 0.01 && self.decay < 0.6 && clarity > 0.1)
    }

    /// Index into the manual's drum list: 0 kick, 1 snare, 2 closed hat, 3 open hat,
    /// 4 tom, 5 cymbal, 6 clap, 7 shaker, 8 drum percussion, 9 other percussion.
    fn drum_type(&self, pitched: bool) -> u8 {
        let (c, d) = (self.centroid, self.decay);
        match () {
            () if c < 300.0 && !pitched || c < 220.0 => 0,
            () if c > 6_000.0 && d < 0.15 => 2,
            () if c > 6_000.0 && d < 1.2 => 3,
            () if c > 4_000.0 => 5,
            () if c > 5_000.0 && self.attack > 0.02 => 7,
            () if pitched && c < 900.0 => 4,
            () if (1_000.0..3_500.0).contains(&c) && d < 0.3 => 6,
            () if (800.0..6_000.0).contains(&c) => 1,
            () if pitched => 8,
            () => 9,
        }
    }

    /// Index into the manual's instrument list: 0 bass, 1 bowed string, 2 brass, 3 flute,
    /// 4 guitar, 5 keyboard, 6 mallet, 7 organ, 8 plucked string, 9 reed, 10 synth, 11 vocal.
    /// `None` when there is no pitch to go by.
    fn instrument_type(&self, pitch: Option<f64>) -> Option<u8> {
        let p = pitch?;
        let (c, a, d) = (self.centroid, self.attack, self.decay);
        Some(match () {
            () if p < 45.0 => 0,
            () if a < 0.015 && c > 3_000.0 && d < 1.0 => 6,
            () if a < 0.015 && d < 0.8 => 8,
            () if a < 0.015 && d < 3.0 => 5,
            () if a < 0.015 => 4,
            () if a > 0.15 && c < 1_500.0 => 1,
            () if a > 0.08 && c > 2_500.0 => 3,
            () if c < 1_200.0 && d > 3.0 => 7,
            () if a > 0.05 => 2,
            () if c < 2_000.0 => 9,
            () => 10,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontra_stream::MemSource;

    fn pcm(samples: impl Iterator<Item = f64>) -> (MemSource, SampleFmt, u64) {
        let bytes: Vec<u8> = samples.flat_map(|v| ((v * 32_767.0) as i16).to_le_bytes()).collect();
        let fmt = SampleFmt { encoding: Encoding::I16, channels: 1 };
        let n = bytes.len() as u64 / 2;
        (MemSource::new(fmt, bytes), fmt, n)
    }

    /// 440 Hz at -20 dBFS peak: peak -20, RMS -23, pitch A4, and a K-weighted
    /// loudness of -23.7 LKFS (-23.01 dB RMS - 0.691 + 0.0 K-weight at 440 Hz).
    #[test]
    fn a_sine_reads_its_level_pitch_and_loudness() {
        let (s, fmt, n) = pcm((0..96_000).map(|i| 0.1 * (2.0 * PI * 440.0 * f64::from(i) / 48_000.0).sin()));
        let a = analyse(&s, fmt, n, 48_000).unwrap();
        assert!((a.peak_db + 20.0).abs() < 0.2, "{}", a.peak_db);
        assert!((a.rms_db + 23.0).abs() < 0.1, "{}", a.rms_db);
        assert!((a.pitch.unwrap() - 69.0).abs() < 0.05, "{:?}", a.pitch);
        assert!((a.loudness_lkfs + 23.7).abs() < 0.2, "{}", a.loudness_lkfs);
    }

    #[test]
    fn noise_has_no_pitch() {
        let mut s = 12_345u32;
        let (src, fmt, n) = pcm((0..48_000).map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f64::from((s >> 16) as i16) / 65_536.0
        }));
        assert!(analyse(&src, fmt, n, 48_000).unwrap().pitch.is_none());
    }
}
