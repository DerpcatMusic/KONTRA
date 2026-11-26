//! Speaker cabinet and room early-reflection simulator.
//!
//! Pre-allocated stereo early-reflection ring buffer + multi-band speaker
//! enclosure formant filter bank + bass/treble shelving EQ. Zero allocations
//! after [`Cabinet::new`].

use std::f64::consts::TAU;

use kontra_dsp_core::{Biquad, Side, high_pass, shelf};

/// Maximum early-reflection tap delay in milliseconds.
const MAX_AIR_MS: f32 = 35.0;

/// Physical settings for [`Cabinet`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CabinetSettings {
    /// Speaker/enclosure voicing index (`0..=11`).
    pub model: u8,
    /// Enclosure size scale (`0.0..=1.0`; larger shifts formants lower).
    pub size: f32,
    /// Room early-reflection mix (`0.0..=1.0`).
    pub air: f32,
    /// Low-shelf tone control in dB (`-12.0..=12.0`).
    pub bass_db: f32,
    /// High-shelf tone control in dB (`-12.0..=12.0`).
    pub treble_db: f32,
}

/// Speaker enclosure acoustic profile:
/// `[low_cut_hz, body_hz, body_db, body_q, cone_hz, cone_db, cone_q, high_cut_hz]`.
const PROFILES: [[f32; 8]; 12] = [
    // 0: 1x12 classic open combo
    [75.0, 145.0, 4.5, 1.4, 2100.0, 4.0, 1.8, 4800.0],
    // 1: 2x12 vintage British
    [70.0, 130.0, 5.0, 1.3, 2500.0, 5.0, 2.0, 5200.0],
    // 2: 4x12 closed classic rock
    [65.0, 115.0, 6.0, 1.5, 1900.0, 4.5, 1.7, 4500.0],
    // 3: 4x12 modern high-gain
    [60.0, 105.0, 5.5, 1.6, 2800.0, 5.5, 2.2, 5600.0],
    // 4: 4x10 bright combo
    [80.0, 160.0, 3.5, 1.2, 3100.0, 4.5, 1.6, 5800.0],
    // 5: 1x10 small studio amp
    [95.0, 195.0, 4.0, 1.5, 2300.0, 3.5, 1.9, 4400.0],
    // 6: 1x8 tiny tweed
    [120.0, 240.0, 5.0, 1.7, 1750.0, 4.0, 2.1, 3900.0],
    // 7: 2x12 jazz clean
    [65.0, 140.0, 3.0, 1.1, 3400.0, 2.5, 1.3, 6500.0],
    // 8: 4x12 scooped vintage
    [62.0, 110.0, 6.5, 1.4, 1500.0, -3.5, 1.3, 4700.0],
    // 9: 1x15 bass cabinet
    [40.0, 82.0, 6.0, 1.4, 950.0, 2.5, 1.4, 3400.0],
    // 10: 4x10 bass cabinet
    [45.0, 95.0, 5.0, 1.3, 1800.0, 3.5, 1.5, 4600.0],
    // 11: 8x10 stadium bass stack
    [38.0, 75.0, 7.0, 1.5, 1250.0, 3.0, 1.6, 4100.0],
];

/// Early-reflection stereo tap times (ms) and gains `[left_gain, right_gain]`.
const AIR_TAPS: [(f32, [f32; 2]); 6] = [
    (4.3, [0.32, 0.14]),
    (7.9, [0.16, 0.30]),
    (12.7, [-0.24, 0.18]),
    (18.1, [0.15, -0.22]),
    (24.3, [-0.16, -0.12]),
    (31.1, [0.11, -0.14]),
];

fn peak(hz: f32, q: f32, gain_db: f32, rate: u32) -> Biquad {
    let r = f64::from(rate.max(1));
    let f = f64::from(hz).clamp(20.0, 0.45 * r);
    let w = TAU * f / r;
    let a = 10.0f64.powf(f64::from(gain_db) / 40.0);
    let alpha = w.sin() / (2.0 * f64::from(q).max(0.1));
    let cos_w = w.cos();
    Biquad::new([
        1.0 + alpha * a,
        -2.0 * cos_w,
        1.0 - alpha * a,
        1.0 + alpha / a,
        -2.0 * cos_w,
        1.0 - alpha / a,
    ])
}

fn low_pass(hz: f32, q: f32, rate: u32) -> Biquad {
    let r = f64::from(rate.max(1));
    let f = f64::from(hz).clamp(20.0, 0.47 * r);
    let w = TAU * f / r;
    let cos_w = w.cos();
    let alpha = w.sin() / (2.0 * f64::from(q).max(0.1));
    let b0 = 0.5 * (1.0 - cos_w);
    Biquad::new([b0, 1.0 - cos_w, b0, 1.0 + alpha, -2.0 * cos_w, 1.0 - alpha])
}

#[inline]
fn flush32(x: f32) -> f32 {
    if x.abs() < 1e-20 || !x.is_finite() {
        0.0
    } else {
        x
    }
}

/// Speaker cabinet simulator with room early reflections.
#[derive(Debug)]
pub struct Cabinet {
    s: CabinetSettings,
    rate: u32,
    hp: Biquad,
    body: Biquad,
    cone: Biquad,
    lp1: Biquad,
    lp2: Biquad,
    bass: Biquad,
    treble: Biquad,
    ring: Box<[[f32; 2]]>,
    write: usize,
    tap_offsets: [usize; 6],
    air_lp: [f32; 2],
    air_cur: f32,
    smooth: f32,
}

impl Cabinet {
    #[must_use]
    pub fn new(s: CabinetSettings, rate: u32) -> Self {
        let rate = rate.max(8_000);
        let frames = ((MAX_AIR_MS * 0.001 * rate as f32) as usize).max(64) + 8;
        let tap_offsets =
            AIR_TAPS.map(|(ms, _)| ((ms * 0.001 * rate as f32) as usize).clamp(1, frames - 1));
        let smooth = 1.0 - (-1.0 / (0.010 * rate as f32)).exp();
        let mut cab = Self {
            s,
            rate,
            hp: high_pass(75.0, 0.707, rate),
            body: peak(145.0, 1.4, 4.5, rate),
            cone: peak(2100.0, 1.8, 4.0, rate),
            lp1: low_pass(4800.0, 0.8, rate),
            lp2: low_pass(5800.0, 0.707, rate),
            bass: shelf(130.0, 0.0, 0.7, Side::Low, rate),
            treble: shelf(3200.0, 0.0, 0.7, Side::High, rate),
            ring: vec![[0.0; 2]; frames].into(),
            write: 0,
            tap_offsets,
            air_lp: [0.0; 2],
            air_cur: s.air.clamp(0.0, 1.0),
            smooth,
        };
        cab.retune(s);
        cab
    }

    pub fn set(&mut self, s: CabinetSettings) {
        self.s = s;
        self.retune(s);
    }

    fn retune(&mut self, s: CabinetSettings) {
        let idx = usize::from(s.model) % PROFILES.len();
        let [
            hp_hz,
            body_hz,
            body_db,
            body_q,
            cone_hz,
            cone_db,
            cone_q,
            lp_hz,
        ] = PROFILES.get(idx).copied().unwrap_or(PROFILES[0]);
        // Larger size shifts resonances downward; smaller shifts upward.
        let scale = (0.65 * (0.5 - s.size.clamp(0.0, 1.0))).exp2();
        let r = self.rate;
        self.hp.retune(&high_pass(hp_hz * scale, 0.707, r));
        self.body.retune(&peak(body_hz * scale, body_q, body_db, r));
        self.cone.retune(&peak(cone_hz * scale, cone_q, cone_db, r));
        self.lp1.retune(&low_pass(lp_hz * scale, 0.85, r));
        self.lp2.retune(&low_pass(lp_hz * scale * 1.25, 0.707, r));
        self.bass.retune(&shelf(
            130.0,
            s.bass_db.clamp(-14.0, 14.0),
            0.7,
            Side::Low,
            r,
        ));
        self.treble.retune(&shelf(
            3200.0,
            s.treble_db.clamp(-14.0, 14.0),
            0.7,
            Side::High,
            r,
        ));
    }

    /// Processes stereo audio in place with zero allocations.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        if io.is_empty() {
            return;
        }
        self.hp.process(io);
        self.body.process(io);
        self.cone.process(io);
        self.lp1.process(io);
        self.lp2.process(io);
        self.bass.process(io);
        self.treble.process(io);

        let len = self.ring.len();
        let air_tgt = self.s.air.clamp(0.0, 1.0);
        let smooth = self.smooth;
        let damp = 0.35f32;

        for frame in io.iter_mut() {
            self.air_cur += (air_tgt - self.air_cur) * smooth;
            let [l, r] = *frame;
            if let Some(slot) = self.ring.get_mut(self.write) {
                *slot = [l, r];
            }
            if self.air_cur > 1e-5 {
                let mut er = [0.0f32; 2];
                for (&off, &(_, g)) in self.tap_offsets.iter().zip(AIR_TAPS.iter()) {
                    let idx = if self.write >= off {
                        self.write - off
                    } else {
                        self.write + len - off
                    };
                    let tap = self.ring.get(idx).copied().unwrap_or([0.0; 2]);
                    let mono_tap = 0.5 * (tap[0] + tap[1]);
                    er[0] += mono_tap * g[0];
                    er[1] += mono_tap * g[1];
                }
                self.air_lp[0] = flush32(self.air_lp[0] + (er[0] - self.air_lp[0]) * damp);
                self.air_lp[1] = flush32(self.air_lp[1] + (er[1] - self.air_lp[1]) * damp);
                let wet = 0.65 * self.air_cur;
                frame[0] = flush32(l + wet * self.air_lp[0]);
                frame[1] = flush32(r + wet * self.air_lp[1]);
            }
            self.write += 1;
            if self.write >= len {
                self.write = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cabinet_models_and_air_shape_spectrum_cleanly() {
        let mut cab = Cabinet::new(
            CabinetSettings {
                model: 2,
                size: 0.5,
                air: 0.5,
                bass_db: 0.0,
                treble_db: 0.0,
            },
            48_000,
        );
        let mut io = vec![[0.0f32; 2]; 2048];
        io[0] = [1.0, 1.0];
        cab.process(&mut io);
        assert!(io.iter().flatten().all(|v| v.is_finite()));
        // Early reflection tail should be non-zero past 10 ms (~480 frames).
        let tail: f32 = io[400..1600].iter().map(|[l, r]| l.abs() + r.abs()).sum();
        assert!(tail > 0.05, "expected cabinet air reflections, got {tail}");
    }
}
