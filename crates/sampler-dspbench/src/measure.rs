//! Metrics on a `Block`: response, distortion, aliasing, noise, denormals, time constants,
//! static curve, stability, latency and CPU. Each function builds a fresh block so runs are independent.

use crate::block::{Block, Factory};
use crate::fft::{bin_hz, power_spectrum};
use crate::signal;
use std::f64::consts::TAU;
use std::time::Instant;

pub const FFT_LEN: usize = 1 << 15;
/// Frames of an impulse response for probe measurements: the Kontakt oracle's 16384-frame window.
pub const PROBE_FRAMES: usize = 16384;

/// Process `input` in chunks of `block`, returning the output.
pub fn run(b: &mut dyn Block, input: &[f32], block: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; input.len()];
    for (x, y) in input.chunks(block).zip(out.chunks_mut(block)) {
        b.process(x, y);
    }
    out
}

pub fn build(make: Factory, rate: f64, params: &[(&str, f64)]) -> Box<dyn Block> {
    make(rate, params)
}

/// Complex gain (dB, degrees) of the block at `hz`, from the steady state of a sine of `amp`.
pub fn tone_gain(make: Factory, rate: f64, params: &[(&str, f64)], hz: f64, block: usize, amp: f64) -> (f64, f64) {
    let mut b = build(make, rate, params);
    let cycles = (rate / hz).ceil() as usize;
    let settle = (rate * 0.25) as usize;
    let span = (cycles * 64).max(4096);
    let frames = settle + span;
    let x = signal::sine(rate, hz, amp, frames);
    let y = run(b.as_mut(), &x, block);
    let (mut yr, mut yi, mut xr, mut xi) = (0.0, 0.0, 0.0, 0.0);
    for n in settle..frames {
        let (s, c) = (TAU * hz * n as f64 / rate).sin_cos();
        yr += f64::from(y[n]) * c;
        yi += f64::from(y[n]) * s;
        xr += f64::from(x[n]) * c;
        xi += f64::from(x[n]) * s;
    }
    let (ym, xm) = ((yr * yr + yi * yi).sqrt(), (xr * xr + xi * xi).sqrt());
    let phase = (yi.atan2(yr) - xi.atan2(xr)).to_degrees();
    let phase = (phase + 540.0).rem_euclid(360.0) - 180.0;
    (20.0 * (ym / xm.max(1e-30)).max(1e-30).log10(), phase)
}

/// Impulse response of `frames` frames, the input to the Kontakt-oracle style probe.
pub fn impulse_response(make: Factory, rate: f64, params: &[(&str, f64)], block: usize, frames: usize) -> Vec<f32> {
    let mut b = build(make, rate, params);
    let x = signal::impulse(frames, 1.0);
    run(b.as_mut(), &x, block)
}

/// dB of a windowed DFT at each probe frequency, measured from `h` the way the Kontakt oracle does
/// (16384-frame window starting 256 frames before the loudest sample).
pub fn probe_db(h: &[f32], rate: f64, probes: &[f64]) -> Vec<f64> {
    let peak = (0..h.len()).max_by(|&a, &b| f32::abs(h[a]).total_cmp(&f32::abs(h[b]))).unwrap_or(0);
    let win = |n: usize| (peak + n).checked_sub(256).and_then(|i| h.get(i).copied()).map_or(0.0, f64::from);
    probes
        .iter()
        .map(|&f| {
            let (mut re, mut im) = (0.0, 0.0);
            for n in 0..16384 {
                let a = -TAU * f * n as f64 / rate;
                re += win(n) * a.cos();
                im += win(n) * a.sin();
            }
            20.0 * (re * re + im * im).sqrt().max(1e-12).log10()
        })
        .collect()
}

/// Aliasing of a driven tone at `f0`: energy that the tone's harmonics (k = 1..12) and their
/// +-400 Hz neighbourhoods (which hold modulation sidebands from gain or cutoff motion) do not
/// explain, relative to the fundamental's neighbourhood, in dB. A time-invariant linear block
/// reads at the window floor; folded components from fast modulation or nonlinearity raise it.
pub fn alias_db(out: &[f32], rate: f64, f0: f64) -> f64 {
    const TOL_HZ: f64 = 400.0;
    let p = power_spectrum(out, FFT_LEN);
    let mut explained = vec![false; p.len()];
    let mut fund = 0.0;
    for k in 1..=12 {
        let hk = k as f64 * f0;
        if hk >= rate * 0.5 {
            break;
        }
        for (i, slot) in explained.iter_mut().enumerate() {
            let hz = bin_hz(rate, FFT_LEN, i);
            if (hz - hk).abs() <= TOL_HZ {
                *slot = true;
                if k == 1 {
                    fund += p[i];
                }
            }
        }
    }
    let alias: f64 = p.iter().enumerate().filter(|(i, _)| !explained[*i] && bin_hz(rate, FFT_LEN, *i) > 20.0).map(|(_, v)| v).sum();
    10.0 * (alias.max(1e-30) / fund.max(1e-30)).log10()
}

/// THD+N (dB) of a tone at `f0`: everything except the fundamental's own bins, relative to it.
pub fn thd_n_db(out: &[f32], rate: f64, f0: f64) -> f64 {
    let p = power_spectrum(out, FFT_LEN);
    let c = (f0 * FFT_LEN as f64 / rate).round() as usize;
    let fund: f64 = (c - 4..=c + 4).map(|i| p[i]).sum();
    let total: f64 = p.iter().skip(2).sum();
    10.0 * (((total - fund).max(1e-30)) / fund.max(1e-30)).log10()
}

/// Output RMS (dBFS) of silence in, after settling: the block's own noise floor.
pub fn noise_floor_dbfs(make: Factory, rate: f64, params: &[(&str, f64)], block: usize) -> f64 {
    let mut b = build(make, rate, params);
    let x = signal::silence(rate as usize);
    let y = run(b.as_mut(), &x, block);
    let ms: f64 = y.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / y.len() as f64;
    10.0 * ms.max(1e-40).log10() / 2.0
}

/// Subnormal outputs during a decay to 1e-38, and the worst block time over the tail relative to the median.
pub fn denormal_tail(make: Factory, rate: f64, params: &[(&str, f64)], block: usize) -> (usize, f64) {
    let mut b = build(make, rate, params);
    let x = signal::denormal_tail(rate, rate as usize * 2);
    let y = run(b.as_mut(), &x, block);
    let subnormal = y.iter().filter(|v| **v != 0.0 && v.abs() < f32::MIN_POSITIVE).count();
    let mut times = Vec::new();
    let mut b2 = build(make, rate, params);
    let mut out = vec![0.0f32; block];
    for chunk in x.chunks(block) {
        let t = Instant::now();
        b2.process(chunk, &mut out[..chunk.len()]);
        times.push(t.elapsed().as_nanos() as f64);
    }
    times.sort_by(f64::total_cmp);
    let median = times[times.len() / 2].max(1.0);
    (subnormal, times[times.len() - 1] / median)
}

/// Time (ms) to reach 63.2 % of the final change after a step from `lo` to `hi` dB-independent
/// level. Used for attack (up-step) and release (down-step) of dynamics; also filter rise time.
pub fn time_constant_ms(make: Factory, rate: f64, params: &[(&str, f64)], block: usize, lo: f64, hi: f64) -> f64 {
    let mut b = build(make, rate, params);
    let frames = (rate * 0.5) as usize;
    let at = (rate * 0.05) as usize;
    let x = signal::step(lo, hi, at, frames);
    let y = run(b.as_mut(), &x, block);
    let target = y[frames - 1] as f64;
    let start = y[at] as f64;
    let thr = start + (target - start) * (1.0 - (-1.0f64).exp());
    let idx = (at..frames).find(|&n| {
        let v = f64::from(y[n]);
        if target >= start { v >= thr } else { v <= thr }
    });
    idx.map_or(f64::INFINITY, |n| (n - at) as f64 * 1000.0 / rate)
}

/// Steady-state output peak (dBFS) of a sine at each input level (dBFS): the static curve.
pub fn static_curve(make: Factory, rate: f64, params: &[(&str, f64)], block: usize, levels_db: &[f64]) -> Vec<f64> {
    levels_db
        .iter()
        .map(|&lv| {
            let mut b = build(make, rate, params);
            let amp = 10f64.powf(lv / 20.0);
            let frames = (rate * 0.4) as usize;
            let x = signal::sine(rate, 1000.0, amp, frames);
            let y = run(b.as_mut(), &x, block);
            let peak = y[frames * 3 / 4..].iter().fold(0f32, |m, v| m.max(v.abs()));
            20.0 * f64::from(peak).max(1e-12).log10()
        })
        .collect()
}

/// Output under fast modulation: `key` swept between `lo` and `hi` every block by a 37 Hz triangle,
/// driving a 2 kHz sine at -6 dBFS.
pub fn modulated_run(make: Factory, rate: f64, params: &[(&str, f64)], key: &str, lo: f64, hi: f64, block: usize) -> Vec<f32> {
    let mut b = build(make, rate, params);
    let frames = rate as usize;
    let x = signal::sine(rate, 2000.0, 0.5, frames);
    let mut out = vec![0.0f32; frames];
    for (i, (xs, ys)) in x.chunks(block).zip(out.chunks_mut(block)).enumerate() {
        let t = (i * block) as f64 / rate;
        let tri = 2.0 * (2.0 * (37.0 * t - (37.0 * t + 0.5).floor())).abs() - 1.0;
        b.set(key, lo + (hi - lo) * (0.5 + 0.5 * tri));
        b.process(xs, ys);
    }
    out
}

/// Stable if every output is finite and bounded: the modulated run stays under 10x full scale.
pub fn is_stable(out: &[f32]) -> (bool, f64) {
    let peak = out.iter().filter(|v| v.is_finite()).fold(0f64, |m, v| m.max(f64::from(v.abs())));
    let finite = out.iter().all(|v| v.is_finite());
    (finite && peak < 10.0, 20.0 * peak.max(1e-12).log10())
}

/// Peak of the 4x cubic-interpolated output (dBFS): the true-peak estimate used for overshoot.
pub fn true_peak_db(out: &[f32]) -> f64 {
    let at = |i: isize| f64::from(*out.get(i.clamp(0, out.len() as isize - 1) as usize).unwrap_or(&0.0));
    let mut peak = 0.0f64;
    for n in 0..out.len() as isize {
        let (p0, p1, p2, p3) = (at(n - 1), at(n), at(n + 1), at(n + 2));
        for k in 0..4 {
            let t = k as f64 / 4.0;
            let v = p1 + 0.5 * t * (p2 - p0 + t * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + t * (3.0 * (p1 - p2) + p3 - p0)));
            peak = peak.max(v.abs());
        }
    }
    20.0 * peak.max(1e-12).log10()
}

/// Measured latency: frame of the impulse response's peak.
pub fn latency_frames(make: Factory, rate: f64, params: &[(&str, f64)]) -> usize {
    let h = impulse_response(make, rate, params, 64, PROBE_FRAMES);
    (0..h.len()).max_by(|&a, &b| f32::abs(h[a]).total_cmp(&f32::abs(h[b]))).unwrap_or(0)
}

/// Ns per frame of processing a fixed 1-second white-noise signal, minimum of `reps` runs.
pub fn cpu_ns_per_frame(make: Factory, rate: f64, params: &[(&str, f64)], block: usize, reps: usize) -> f64 {
    let x = signal::white(rate as usize, 0.5, 7);
    let mut out = vec![0.0f32; block];
    let mut best = f64::INFINITY;
    for _ in 0..reps {
        let mut b = build(make, rate, params);
        let t = Instant::now();
        for chunk in x.chunks(block) {
            b.process(chunk, &mut out[..chunk.len()]);
        }
        let ns = t.elapsed().as_nanos() as f64 / x.len() as f64;
        best = best.min(ns);
        std::hint::black_box(&out);
    }
    best
}
