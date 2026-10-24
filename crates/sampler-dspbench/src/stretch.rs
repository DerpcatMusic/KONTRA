//! Time-stretch A/B: the shipping engine (WSOLA, sampler-core `source::time_machine`) against
//! Signalsmith Stretch (MIT, Copyright 2022 Geraint Luff; vendored under `candidates/signalsmith-stretch`).
//! Bench only, feature `candidates`. Run `dspbench --stretch`.
//!
//! Metrics, all on mono material:
//! * transient rise: 10%..90% rise time of the peak envelope at each drum hit (ms). A time stretch
//!   should keep this at the input value; a ratio above 1 is smear.
//! * harmonic clarity: a 4096-frame power spectrum of a sustained 220 Hz tone, mean partial peak over
//!   the median floor between partials (dB). Higher is cleaner; the output minus the input is the loss.
//! * CPU: min over repeats, ns per output frame.
#![allow(unsafe_code)]

use crate::fft::power_spectrum;
use crate::signal;

unsafe extern "C" {
    fn sst_new(rate: i32) -> *mut std::ffi::c_void;
    fn sst_free(p: *mut std::ffi::c_void);
    fn sst_reset(p: *mut std::ffi::c_void);
    fn sst_process(p: *mut std::ffi::c_void, input: *const f32, in_n: i32, out: *mut f32, out_n: i32);
}

/// Signalsmith Stretch, mono, one instance.
pub struct Signalsmith {
    raw: *mut std::ffi::c_void,
}

impl Signalsmith {
    pub fn new(rate: u32) -> Self {
        // SAFETY: `sst_new` returns an owned instance that `Drop` frees exactly once.
        Self { raw: unsafe { sst_new(rate as i32) } }
    }

    /// Consume `input` and fill `out` (lengths are the frame counts for this call).
    pub fn process(&mut self, input: &[f32], out: &mut [f32]) {
        // SAFETY: both slices are valid for the lengths passed; the instance is live.
        unsafe { sst_process(self.raw, input.as_ptr(), input.len() as i32, out.as_mut_ptr(), out.len() as i32) }
    }

    pub fn reset(&mut self) {
        // SAFETY: the instance is live.
        unsafe { sst_reset(self.raw) }
    }
}

impl Drop for Signalsmith {
    fn drop(&mut self) {
        // SAFETY: freed once, here.
        unsafe { sst_free(self.raw) }
    }
}

/// Run Signalsmith over `input` at time ratio `speed` (0.5 = twice as long), in 2048-frame blocks.
pub fn signalsmith(input: &[f32], rate: u32, speed: f64) -> Vec<f32> {
    let mut s = Signalsmith::new(rate);
    let mut out = Vec::new();
    let mut target = 0.0f64;
    for chunk in input.chunks(2048) {
        target += chunk.len() as f64 / speed;
        let want = target.round() as usize - out.len();
        let mut buf = vec![0.0f32; want];
        s.process(chunk, &mut buf);
        out.extend_from_slice(&buf);
    }
    out
}

/// Run the shipping engine over `input` at time ratio `speed`, mono out (left channel).
pub fn ours(input: &[f32], rate: u32, speed: f64) -> Vec<f32> {
    let frames: Vec<[f32; 2]> = input.iter().map(|&x| [x, x]).collect();
    let want = (input.len() as f64 / speed).round() as usize;
    let mut ts = sampler_core::dspbench::TimeStretch::new(frames, rate, speed);
    let mut out = vec![[0.0f32; 2]; want];
    let mut n = 0;
    while n < want {
        let end = (n + 512).min(want);
        let got = ts.render(&mut out[n..end]);
        if got == 0 {
            break;
        }
        n += got;
    }
    out.truncate(n);
    out.iter().map(|f| f[0]).collect()
}

/// Peak envelope with a 0.5 ms (min 1 frame) window, centred.
fn envelope(x: &[f32], rate: f64) -> Vec<f32> {
    let w = ((rate * 0.0005) as usize).max(1);
    let h = w / 2;
    (0..x.len())
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(h), (i + h + 1).min(x.len()));
            x[lo..hi].iter().fold(0.0f32, |m, v| m.max(v.abs()))
        })
        .collect()
}

/// 10%..90% rise time (ms) of the envelope peak found in `[lo, hi)` (frames).
pub fn rise_ms(x: &[f32], rate: f64, lo: usize, hi: usize) -> Option<f64> {
    let env = envelope(x, rate);
    let hi = hi.min(env.len());
    if lo >= hi {
        return None;
    }
    let p = (lo..hi).max_by(|&a, &b| env[a].total_cmp(&env[b]))?;
    let peak = env[p];
    if peak < 1e-4 {
        return None;
    }
    let i10 = (lo..=p).rev().find(|&i| env[i] < 0.1 * peak)? + 1;
    let i90 = (i10..=p).find(|&i| env[i] >= 0.9 * peak)?;
    Some((i90 - i10) as f64 * 1000.0 / rate)
}

/// Median of a non-empty list.
fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
}

/// Median transient rise (ms) for input and output, and the median output/input ratio, over the drum
/// hits. The output hit is searched from the expected onset (`onset / speed`) up to the next hit, so a
/// latency of the engine moves the window rather than dropping the hit.
fn transients(input: &[f32], output: &[f32], rate: f64, speed: f64) -> (f64, f64, f64, usize) {
    let beat = (rate * 0.5) as usize;
    let (mut rin, mut rout, mut ratio) = (Vec::new(), Vec::new(), Vec::new());
    for k in 1..input.len() / beat {
        let onset = k * beat;
        let a = rise_ms(input, rate, onset.saturating_sub((rate * 0.01) as usize), onset + (rate * 0.08) as usize);
        let c = (onset as f64 / speed) as usize;
        let span = (beat as f64 / speed * 0.8) as usize;
        let b = rise_ms(output, rate, c.saturating_sub((rate * 0.02) as usize), c + span);
        let (Some(a), Some(b)) = (a, b) else {
            continue;
        };
        if a > 0.0 && b > 0.0 {
            rin.push(a);
            rout.push(b);
            ratio.push(b / a);
        }
    }
    let n = rin.len();
    // Ratio of medians: a per-hit ratio is unstable when an input rise is a single frame.
    let (mi, mo) = (median(rin), median(rout));
    (mi, mo, mo / mi, n)
}

/// Harmonic clarity (dB): mean of the first eight partial peaks over the median floor between them.
pub fn clarity_db(x: &[f32], rate: f64, centre: usize, n: usize, hz: f64) -> f64 {
    let p = power_spectrum(&x[..centre.min(x.len())], n);
    let bin = |f: f64| (f * n as f64 / rate).round() as usize;
    let mut peaks = Vec::new();
    let mut floor = Vec::new();
    let mut near = vec![false; p.len()];
    for k in 1..=8 {
        let b = bin(k as f64 * hz);
        for j in b.saturating_sub(3)..=(b + 3).min(p.len() - 1) {
            near[j] = true;
        }
        let top = (b.saturating_sub(1)..=(b + 1).min(p.len() - 1)).map(|j| p[j]).fold(0.0f64, f64::max);
        peaks.push(top);
    }
    for (j, &v) in p.iter().enumerate().skip(1) {
        if !near[j] && j < bin(8.5 * hz) {
            floor.push(v);
        }
    }
    let mean_peak = peaks.iter().sum::<f64>() / peaks.len() as f64;
    10.0 * (mean_peak / median(floor).max(1e-30)).log10()
}

/// Minimum over `reps` of the wall time (ns) of `f`, divided by the output frames it produced.
fn ns_per_frame(reps: usize, mut f: impl FnMut() -> usize) -> f64 {
    let mut best = f64::INFINITY;
    for _ in 0..reps {
        let t = std::time::Instant::now();
        let frames = f();
        best = best.min(t.elapsed().as_nanos() as f64 / frames.max(1) as f64);
    }
    best
}

/// `dspbench --stretch`: prints the A/B table for drums and sustained strings at two speeds.
pub fn run() {
    let rate = 48_000.0;
    let r = rate as u32;
    let drums = signal::drum_loop(rate, 48_000 * 4, 11);
    let strings = signal::strings(rate, 220.0, 48_000 * 4);
    println!("time-stretch A/B, {rate} Hz mono. speed = time ratio (0.5 = twice as long).");
    println!("transient rise (ms, median over hits; ratio = output/input, 1.0 = no smear)");
    for speed in [0.5, 0.75, 1.5] {
        let s = ours(&drums, r, speed);
        let g = signalsmith(&drums, r, speed);
        let (ri, ro, rr, n) = transients(&drums, &s, rate, speed);
        let (_, go, gr, _) = transients(&drums, &g, rate, speed);
        println!("  drums speed {speed:>4}: input {ri:.2} ms | ours {ro:.2} ms (ratio {rr:.2}) | signalsmith {go:.2} ms (ratio {gr:.2}) | hits {n}");
    }
    println!("harmonic clarity (dB, 4096-frame window at 1.0 s source time; higher is cleaner)");
    for speed in [0.5, 0.75, 1.5] {
        let s = ours(&strings, r, speed);
        let g = signalsmith(&strings, r, speed);
        let at_in = clarity_db(&strings, rate, 48_000, 4096, 220.0);
        let at_ours = clarity_db(&s, rate, (48_000.0 / speed) as usize, 4096, 220.0);
        let at_sig = clarity_db(&g, rate, (48_000.0 / speed) as usize, 4096, 220.0);
        println!("  strings speed {speed:>4}: input {at_in:.1} dB | ours {at_ours:.1} dB | signalsmith {at_sig:.1} dB");
    }
    let long = signal::strings(rate, 220.0, 48_000 * 10);
    let ours_ns = ns_per_frame(3, || ours(&long, r, 0.75).len());
    let sig_ns = ns_per_frame(3, || signalsmith(&long, r, 0.75).len());
    println!("CPU (ns per output frame, min of 3, 10 s strings at speed 0.75): ours {ours_ns:.1} | signalsmith {sig_ns:.1}");
}

/// Identity check: at speed 1.0 the output should equal the input delayed. Prints the best lag (frames,
/// by cross-correlation over the first second) and the envelope around the first drum hit.
pub fn identity(rate: f64) {
    let drums = signal::drum_loop(rate, 48_000 * 2, 11);
    for (name, out) in [("ours", ours(&drums, rate as u32, 1.0)), ("signalsmith", signalsmith(&drums, rate as u32, 1.0))] {
        let n = 48_000usize;
        let (mut best, mut best_lag) = (f64::NEG_INFINITY, 0usize);
        for lag in 0..24_000usize {
            let c: f64 = (0..n).filter_map(|i| out.get(i + lag)).zip(&drums[..n]).map(|(a, b)| f64::from(*a) * f64::from(*b)).sum();
            if c > best {
                best = c;
                best_lag = lag;
            }
        }
        let env = envelope(&out, rate);
        let peak = env.iter().enumerate().skip(best_lag).take(4000).fold((0, 0.0f32), |m, (i, &v)| if v > m.1 { (i, v) } else { m });
        println!("identity {name}: best lag {best_lag} frames ({:.1} ms); peak env {:.3} at frame {}", best_lag as f64 * 1000.0 / rate, peak.1, peak.0);
        let around: Vec<String> = (best_lag..best_lag + 40).step_by(4).map(|i| format!("{:.3}", env.get(i).copied().unwrap_or(0.0))).collect();
        println!("  env from lag: {}", around.join(" "));
    }
}
