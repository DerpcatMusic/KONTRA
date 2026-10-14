//! UVI Workstation 4.0.9 drive engines as the color chain's shaper: input gain, a curve
//! (GarageFuzz `tanh`, SmoothOverdrive `s / (1 + |s|)`, StraightFuzz a stereo-linked square),
//! wet/dry and output gain, in the native f32 order (model uvicolor/drive_model.py, checked
//! against the emulated kernels 0x14125f320, 0x1412663f0, 0x141268ee0).
use super::super::hiir::{self, Hiir, HiirState};
use super::super::Planar;
pub use sampler_ir::fx::color::{Drive, DriveKind};

fn p10(x: f32) -> f32 {
    10f32.powf(x)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Engine {
    kind: DriveKind,
    gin: f32,
    drive: f32,
    trash: f32,
    dry: f32,
    wet: f32,
    gout: f32,
    /// 2^k oversampling through the shared HIIR stage (designed at 20 kHz, 78 dB).
    oversampling: usize,
    hiir: Hiir,
}

/// GarageFuzz's smoothed drive (+0x48): prepare leaves it at 1, so the first call ramps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct State {
    smoothed: Option<f32>,
    hiir: [HiirState; 2],
}

impl State {
    pub(super) fn finite(&self) -> bool {
        self.hiir.iter().all(HiirState::finite)
    }
}

impl Engine {
    pub(super) fn new(d: Drive, rate: u32) -> Self {
        let od = d.overdrive * 0.1;
        let m = d.mix * 0.01;
        let (trash, wet) = match d.kind {
            DriveKind::Garage => ((d.trash * 9.0) + 1.0, m),
            DriveKind::Smooth => (p10(d.trash * 1.5), m * 0.501_187_2),
            // StraightFuzz keeps its threshold here and folds the makeup `10^(1.75 Trash)`
            // into `wet`.
            DriveKind::Straight => (p10(d.trash * -2.4), (m * 0.707_945_76) * p10(d.trash * 1.75)),
        };
        Self {
            kind: d.kind,
            gin: p10(d.input_gain * 0.05),
            drive: p10(od + od),
            trash,
            dry: 1.0 - m,
            wet,
            gout: p10(d.output_gain * 0.05),
            oversampling: usize::from(d.oversampling.min(hiir::STAGES as u8)),
            hiir: Hiir::new(rate as f32, 20_000.0, 78.0),
        }
    }

    pub(super) fn process(&self, state: &mut State, block: &mut Planar, len: usize) {
        const BLOCK: usize = super::super::BLOCK;
        let k = self.oversampling;
        let mut x = [[0f32; BLOCK << hiir::STAGES]; 2];
        let mut scratch = [0f32; BLOCK << hiir::STAGES];
        for ch in 0..2 {
            let input: [f32; BLOCK] = std::array::from_fn(|i| block[ch][i] as f32);
            self.hiir.up(&mut state.hiir[ch], &input[..len], k, &mut x[ch], &mut scratch);
        }
        let n = len << k;
        let [left, right] = &mut x;
        self.kernel(state, &mut left[..n], &mut right[..n]);
        for ch in 0..2 {
            let mut out = [0f32; BLOCK];
            self.hiir.down(&mut state.hiir[ch], &mut x[ch][..n], k, &mut out);
            for i in 0..len {
                block[ch][i] = f64::from(out[i]);
            }
        }
    }

    fn kernel(&self, state: &mut State, left: &mut [f32], right: &mut [f32]) {
        let len = left.len();
        let (g, t) = (self.gin, self.trash);
        match self.kind {
            DriveKind::Garage => {
                let from = state.smoothed.unwrap_or(1.0);
                state.smoothed = Some(self.drive);
                let step = (self.drive - from) / len.max(1) as f32;
                for ch in [&mut *left, &mut *right] {
                    for (i, v) in ch.iter_mut().enumerate() {
                        let x = *v;
                        let s = if from == self.drive {
                            ((((self.drive * 0.5) * t) * x) * g).tanh()
                        } else {
                            ((((g * x) * (from + i as f32 * step)) * t) * 0.5).tanh()
                        };
                        *v = ((s * self.wet) + ((g * x) * self.dry)) * self.gout;
                    }
                }
            }
            DriveKind::Smooth => {
                for ch in [&mut *left, &mut *right] {
                    for v in ch.iter_mut() {
                        let u = g * *v;
                        let s = if t == 1.0 {
                            u * self.drive
                        } else {
                            let s = (u * self.drive) * t;
                            if s <= 0.0 { (1.0 / (1.0 - s)) - 1.0 } else { 1.0 - (1.0 / (s + 1.0)) }
                        };
                        *v = ((s * self.wet) + (u * self.dry)) * self.gout;
                    }
                }
            }
            DriveKind::Straight => {
                for i in 0..len {
                    let x = [left[i], right[i]];
                    let peak = x.map(|x| ((self.drive * x) * g).abs());
                    let peak = peak[0].max(peak[1]);
                    let y = x.map(|x| {
                        let mut v = x * self.drive;
                        if t < peak {
                            v = if v <= 0.0 { -t } else { t };
                        }
                        ((self.wet * v) + (x * self.dry)) * self.gout
                    });
                    (left[i], right[i]) = (y[0], y[1]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Static ramps (33 points) of the emulated native kernels, 48 kHz (one case oversampled 2x).
    #[test]
    fn ramps_match_the_native_kernels() {
        let vectors: std::collections::BTreeMap<String, serde_json::Value> =
            serde_json::from_str(include_str!("tables/drive_vectors.json")).unwrap();
        for (name, cases) in &vectors {
            let kind = match name.as_str() {
                "GarageFuzz" => DriveKind::Garage,
                "SmoothOverdrive" => DriveKind::Smooth,
                _ => DriveKind::Straight,
            };
            for case in cases.as_array().unwrap() {
                let p = &case["params"];
                let num = |k: &str, d: f64| p[k].as_f64().unwrap_or(d) as f32;
                let drive = Drive {
                    kind,
                    input_gain: num("InputGain", 0.0),
                    overdrive: num("Overdrive", 0.0),
                    trash: num("Trash", 0.0),
                    mix: num("Mix", 100.0),
                    output_gain: num("OutputGain", 0.0),
                    oversampling: num("Oversampling", 0.0) as u8,
                };
                let engine = Engine::new(drive, 48_000);
                let input: Vec<f64> = case["ramp_in"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
                let native: Vec<f64> = case["ramp_out"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
                let mut block = [[0f64; super::super::super::BLOCK]; 2];
                block[0][..input.len()].copy_from_slice(&input);
                block[1][..input.len()].copy_from_slice(&input);
                // A fresh unit: GarageFuzz ramps its drive from 1 across this first call.
                let mut state = State::default();
                engine.process(&mut state, &mut block, input.len());
                for (i, (a, b)) in block[0].iter().zip(&native).enumerate() {
                    assert!((a - b).abs() <= 1e-6 * b.abs().max(1.0), "{name} {p} {i}: {a} vs {b}");
                }
            }
        }
    }
}
