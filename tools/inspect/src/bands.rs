//! Octave-band RMS levels from the offline output.
use rustfft::{FftPlanner, num_complex::Complex};
use serde_json::{Value, json};

const FFT: usize = 4096;
const HOP: usize = FFT / 2;
const CENTERS: [f64; 10] = [
    31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// Stereo mean-square per octave, Hann-windowed FFT, including silent windows.
pub fn octave_levels(audio: &[[f32; 2]], rate: u32) -> Vec<Value> {
    if audio.len() < FFT {
        return Vec::new();
    }
    let window: Vec<f64> = (0..FFT)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / (FFT - 1) as f64).cos())
        .collect();
    let window_energy: f64 = window.iter().map(|x| x * x).sum();
    let mut planner = FftPlanner::<f64>::new();
    let transform = planner.plan_fft_forward(FFT);
    let mut input = vec![Complex::new(0.0, 0.0); FFT];
    let mut sums = [0.0f64; CENTERS.len()];
    let mut count = 0usize;
    for chunk in audio.windows(FFT).step_by(HOP) {
        count += 1;
        for channel in 0..2 {
            for ((bin, frame), &w) in input.iter_mut().zip(chunk).zip(&window) {
                bin.re = f64::from(frame.get(channel).copied().unwrap_or(0.0)) * w;
                bin.im = 0.0;
            }
            transform.process(&mut input);
            for (i, bin) in input.iter().take(FFT / 2).enumerate().skip(1) {
                let freq = i as f64 * f64::from(rate) / FFT as f64;
                for (&center, sum) in CENTERS.iter().zip(&mut sums) {
                    if freq >= center / std::f64::consts::SQRT_2
                        && freq < center * std::f64::consts::SQRT_2
                    {
                        *sum += bin.norm_sqr();
                        break;
                    }
                }
            }
        }
    }
    // Two channels and the two-sided FFT cancel the factor of two.
    let denominator = count as f64 * FFT as f64 * window_energy;
    CENTERS.iter().zip(sums).map(|(&center, power)| {
        let mean_square = power / denominator;
        json!({"center_hz":center,"rms_dbfs": if mean_square > 0.0 { Some(10.0 * mean_square.log10()) } else { None }})
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_one_kilohertz_sine_peaks_in_its_octave() {
        let audio: Vec<_> = (0..8192)
            .map(|i| {
                let x = (std::f64::consts::TAU * 1000.0 * i as f64 / 48000.0).sin() as f32;
                [x, x]
            })
            .collect();
        let bands = octave_levels(&audio, 48000);
        let peak = bands
            .iter()
            .filter_map(|b| Some((b["center_hz"].as_f64()?, b["rms_dbfs"].as_f64()?)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        assert_eq!(peak.0, 1000.0);
        assert!((peak.1 + 3.0103).abs() < 0.1, "{peak:?}");
    }
}
