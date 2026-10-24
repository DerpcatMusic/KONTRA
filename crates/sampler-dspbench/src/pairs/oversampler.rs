//! Pair 5: 2x oversampling around a tanh drive. Ours is the engine's half-band (`hiir`, 20 kHz
//! edge, 78 dB, as used by the drive and exciter); the candidate is Signalsmith `Oversampler2xFIR`
//! (feature `candidates`, MIT). Both get the same nonlinearity `tanh(drive x) / drive` at 2x.
//! Fidelity: no Kontakt oversampler capture, so the Kontakt axis is absent.

use super::{PairReport, Quality, side};
use crate::band::Band;
use crate::block::{Block, Factory, Impl, param};
use crate::measure;
use crate::signal;
use sampler_core::dspbench::HalfBand;

/// Our half-band path with the shared drive.
struct OursHalfBand {
    rate: f32,
    band: HalfBand,
    drive: f64,
    up: Vec<f32>,
    buf: Vec<f32>,
}

impl OursHalfBand {
    fn new(rate: f64, drive: f64) -> Self {
        Self { rate: rate as f32, band: HalfBand::new(rate as f32), drive, up: Vec::new(), buf: Vec::new() }
    }
}

impl Block for OursHalfBand {
    fn process(&mut self, input: &[f32], out: &mut [f32]) {
        let n = input.len();
        self.up.resize(2 * n, 0.0);
        self.buf.resize(2 * n, 0.0);
        self.band.up(input, &mut self.up);
        let d = self.drive as f32;
        for (dst, &v) in self.buf.iter_mut().zip(&self.up) {
            *dst = (d * v).tanh() / d;
        }
        self.band.down(&mut self.buf, out);
    }
    fn set(&mut self, key: &str, value: f64) -> bool {
        match key {
            "drive" => self.drive = value,
            _ => return false,
        }
        true
    }
    fn reset(&mut self) {
        *self = Self::new(f64::from(self.rate), self.drive);
    }
}

fn make_ours(rate: f64, params: &[(&str, f64)]) -> Box<dyn Block> {
    Box::new(OursHalfBand::new(rate, param(params, "drive", 4.0)))
}

#[cfg(feature = "candidates")]
fn make_signalsmith(_rate: f64, params: &[(&str, f64)]) -> Box<dyn Block> {
    Box::new(crate::signalsmith::SigOversampler::new(1 << 12, param(params, "drive", 4.0)))
}

pub fn ours_impl() -> Impl {
    Impl { label: "sampler-core hiir 2x (20 kHz, 78 dB)", make: make_ours }
}

#[cfg(feature = "candidates")]
pub fn cand_impl() -> Option<Impl> {
    Some(Impl { label: "Signalsmith Oversampler2xFIR", make: make_signalsmith })
}

#[cfg(not(feature = "candidates"))]
pub fn cand_impl() -> Option<Impl> {
    None
}

/// Round-trip gain ripple of the oversampler at a near-linear drive (passband, 100 Hz .. 18 kHz at 48 kHz).
fn passband_ripple(make: Factory, rate: f64, block: usize) -> f64 {
    let params = [("drive", 1e-3)];
    let freqs = [100.0, 500.0, 1000.0, 3000.0, 6000.0, 9000.0, 12000.0, 15000.0, 18000.0];
    let gains: Vec<f64> = freqs
        .iter()
        .filter(|&&f| f < rate * 0.4)
        .map(|&f| measure::tone_gain(make, rate, &params, f, block, 0.5).0)
        .collect();
    gains.iter().fold(f64::MIN, |m, &g| m.max(g)) - gains.iter().fold(f64::MAX, |m, &g| m.min(g))
}

/// Alias rejection: a 5 kHz tone through the hot nonlinearity (drive 4); folded harmonics counted.
fn quality(make: Factory, rate: f64, block: usize) -> (Quality, f64) {
    let params = [("drive", 4.0)];
    let x = signal::sine(rate, 5000.0, 0.5, rate as usize);
    let y = measure::run(measure::build(make, rate, &params).as_mut(), &x, block);
    let (subnormal, spike) = measure::denormal_tail(make, rate, &params, block);
    let ripple = passband_ripple(make, rate, block);
    (
        Quality {
            alias_db: measure::alias_db(&y, rate, 5000.0),
            thd_n_db: measure::thd_n_db(&y, rate, 5000.0),
            noise_dbfs: measure::noise_floor_dbfs(make, rate, &params, block),
            subnormal,
            denormal_spike: spike,
            stable: true,
            peak_dbfs: measure::true_peak_db(&y),
            latency: measure::latency_frames(make, rate, &[("drive", 1e-3)]),
            ns_per_frame: measure::cpu_ns_per_frame(make, rate, &params, block, 5),
            instr_per_frame: None,
        },
        ripple,
    )
}

pub fn run(band: &Band, ours: Impl, cand: Option<Impl>, rate: f64, block: usize) -> PairReport {
    let (qo, ro) = quality(ours.make, rate, block);
    let cand = cand.map(|c| (c, quality(c.make, rate, block)));
    let other_for_ours = cand.as_ref().map_or(qo, |(_, (q, _))| *q);
    let mut sides = vec![side(band, ours.label, qo, None, &other_for_ours, format!("passband ripple {ro:.3} dB"))];
    if let Some((c, (qc, rc))) = cand {
        sides.push(side(band, c.label, qc, None, &qo, format!("passband ripple {rc:.3} dB")));
    }
    PairReport { id: "oversampler", title: "2x oversampling: hiir half-band (ours) vs Signalsmith Oversampler2xFIR", reference: "none (no Kontakt oversampler capture)".into(), rate, block, sides }
}
