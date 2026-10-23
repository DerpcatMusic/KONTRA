//! Pair 3: sample-rate conversion. Ours is the engine's kernel (`resample::Kernel`, `High` quality
//! for the sinc path); candidate is rubato's async sinc (MIT / Apache-2.0, feature `candidates`).
//! Fidelity: no Kontakt interpolation capture exists yet, so the Kontakt axis is reported absent.

use super::{PairReport, Quality, side};
use crate::band::Band;
use crate::measure;
use crate::signal;
use sampler_core::dspbench::{ResampleQuality, Resampler};
use std::f64::consts::TAU;
use std::time::Instant;

/// A converter: input samples, input rate, output rate, output samples.
pub type Convert = fn(&[f32], f64, f64) -> Vec<f32>;

pub struct Src {
    pub label: &'static str,
    pub convert: Convert,
}

fn ours(x: &[f32], rin: f64, rout: f64) -> Vec<f32> {
    Resampler::new(ResampleQuality::High).resample(x, rin / rout)
}

#[cfg(feature = "candidates")]
fn rubato_convert(x: &[f32], rin: f64, rout: f64) -> Vec<f32> {
    use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
    use rubato::{Async, FixedAsync, Resampler as _, SincInterpolationParameters, SincInterpolationType, WindowFunction};
    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: None,
        oversampling_factor: 256,
        interpolation: SincInterpolationType::Cubic,
        window: WindowFunction::BlackmanHarris2,
    };
    let mut r = Async::<f64>::new_sinc(rout / rin, 1.1, &params, 1024, 1, FixedAsync::Input).expect("rubato");
    let data = vec![x.iter().map(|&v| f64::from(v)).collect::<Vec<f64>>()];
    let adapter = SequentialSliceOfVecs::new(&data, 1, x.len()).expect("adapter");
    let out = r.process_all(&adapter, x.len(), None).expect("process");
    out.take_data().into_iter().map(|v| v as f32).collect()
}

#[cfg(not(feature = "candidates"))]
fn rubato_convert(x: &[f32], _rin: f64, _rout: f64) -> Vec<f32> {
    x.to_vec()
}

/// Gain (dB) of a converter at `hz`: a sine through the converter, steady-state projection at the output rate.
fn tone_gain_db(convert: Convert, rin: f64, rout: f64, hz: f64) -> f64 {
    let frames = rin as usize;
    let x = signal::sine(rin, hz, 0.5, frames);
    let y = convert(&x, rin, rout);
    let skip = y.len() / 4;
    let (mut yr, mut yi, mut xr, mut xi, mut n) = (0.0, 0.0, 0.0, 0.0, 0usize);
    for (m, &v) in y.iter().enumerate().skip(skip) {
        let (s, c) = (TAU * hz * m as f64 / rout).sin_cos();
        yr += f64::from(v) * c;
        yi += f64::from(v) * s;
        n += 1;
    }
    for (m, &v) in x.iter().enumerate().skip(skip) {
        let (s, c) = (TAU * hz * m as f64 / rin).sin_cos();
        xr += f64::from(v) * c;
        xi += f64::from(v) * s;
    }
    let ym = (yr * yr + yi * yi).sqrt() / n.max(1) as f64;
    let xm = (xr * xr + xi * xi).sqrt() / (x.len() - skip).max(1) as f64;
    20.0 * (ym / xm.max(1e-30)).max(1e-30).log10()
}

/// Pass-band ripple (max - min gain, dB) over 20 Hz .. 0.9 of the output Nyquist.
fn passband_ripple(convert: Convert, rin: f64, rout: f64) -> f64 {
    let top = 0.9 * rout / 2.0;
    let gains: Vec<f64> = [20.0, 100.0, 1000.0, 3000.0, 6000.0, 10000.0, 15000.0, 19000.0]
        .iter()
        .filter(|&&f| f < top)
        .map(|&f| tone_gain_db(convert, rin, rout, f))
        .collect();
    gains.iter().fold(f64::MIN, |m, &g| m.max(g)) - gains.iter().fold(f64::MAX, |m, &g| m.min(g))
}

/// Stop-band attenuation (dB, worst case) for tones between the output Nyquist and the input Nyquist
/// (above that, the input itself aliases and the converter cannot be blamed).
fn stopband_db(convert: Convert, rin: f64, rout: f64) -> f64 {
    let nyq = rout / 2.0;
    [1.02, 1.05, 1.08]
        .iter()
        .map(|&k| {
            let hz = nyq * k;
            let x = signal::sine(rin, hz, 0.5, rin as usize);
            let y = convert(&x, rin, rout);
            let rms = |v: &[f32]| (v.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / v.len().max(1) as f64).sqrt();
            let skip = y.len() / 4;
            20.0 * (rms(&x[..]) / rms(&y[skip..]).max(1e-12)).log10()
        })
        .fold(f64::MAX, f64::min)
}

fn quality(convert: Convert, rin: f64, rout: f64) -> (Quality, String) {
    let y1k = convert(&signal::sine(rin, 1000.0, 0.5, rin as usize), rin, rout);
    let thd = measure::thd_n_db(&y1k, rout, 1000.0);
    let impulse = convert(&signal::impulse(rin as usize, 1.0), rin, rout);
    let peak = (0..impulse.len()).max_by(|&a, &b| f32::abs(impulse[a]).total_cmp(&f32::abs(impulse[b]))).unwrap_or(0);
    let silence = convert(&signal::silence(rin as usize), rin, rout);
    let noise = 10.0 * silence.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>().max(1e-40).log10() / 2.0;
    let ripple = passband_ripple(convert, rin, rout);
    let stop = stopband_db(convert, rin, rout);
    let x = signal::white(rin as usize, 0.5, 7);
    let mut best = f64::INFINITY;
    for _ in 0..5 {
        let t = Instant::now();
        std::hint::black_box(convert(&x, rin, rout));
        best = best.min(t.elapsed().as_nanos() as f64 / rout);
    }
    (
        Quality {
            // Leakage of out-of-band tones into the output, as a positive number (lower is cleaner).
            alias_db: -stop,
            thd_n_db: thd,
            noise_dbfs: noise,
            latency: peak,
            ns_per_frame: best,
            stable: true,
            peak_dbfs: measure::true_peak_db(&y1k),
            ..Quality::default()
        },
        format!("passband ripple {ripple:.2} dB; stopband {stop:.1} dB (worst of 1.02/1.05/1.08 x output Nyquist); impulse peak at frame {peak}"),
    )
}

/// Engine kernel vs candidate at 48 kHz -> 44.1 kHz. The Kontakt axis is absent: no interpolation
/// capture yet. Latency is the impulse-response peak index: a centred kernel peaks at 0, so a
/// non-zero value is the converter's look-ahead, not the engine's full group delay.
pub fn run(band: &Band, ours: Src, cand: Src) -> PairReport {
    let (rin, rout) = (48_000.0, 44_100.0);
    let (qo, no) = quality(ours.convert, rin, rout);
    let (qc, nc) = quality(cand.convert, rin, rout);
    let sides = vec![
        side(band, ours.label, qo, None, &qc, no),
        side(band, cand.label, qc, None, &qo, nc),
    ];
    PairReport { id: "resampler", title: "Sample-rate conversion: engine kernel (ours) vs rubato sinc", reference: "none (no Kontakt interpolation capture)".into(), rate: rin, block: 0, sides }
}

pub fn ours_src() -> Src {
    Src { label: "sampler-core resample (High)", convert: ours }
}

/// The candidate exists only with the `candidates` feature (rubato is not in the shipping build).
#[cfg(feature = "candidates")]
pub fn cand_src() -> Option<Src> {
    Some(Src { label: "rubato sinc (async)", convert: rubato_convert })
}

#[cfg(not(feature = "candidates"))]
pub fn cand_src() -> Option<Src> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tone above the output Nyquist must be strongly attenuated by any real anti-alias filter.
    #[test]
    fn out_of_band_tone_is_attenuated() {
        let (rin, rout) = (48_000.0, 44_100.0);
        let x = signal::sine(rin, 23_150.0, 0.5, rin as usize);
        let y = ours(&x, rin, rout);
        let rms = |v: &[f32]| (v.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
        eprintln!("ours out rms {:.5}, in rms {:.5}", rms(&y[y.len() / 4..]), rms(&x));
        assert!(rms(&y[y.len() / 4..]) < 0.01, "ours passes the tone");
    }
}
