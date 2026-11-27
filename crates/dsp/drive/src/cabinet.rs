//! Speaker cabinet and room early-reflection simulator.
//!
//! Pre-allocated stereo early-reflection ring buffer + multi-band speaker
//! enclosure formant filter bank + bass/treble shelving EQ. Zero allocations
//! after [`Cabinet::new`].

use kontra_dsp_core::{
    Biquad, DelayLine, Side, flush_f32 as flush32, high_pass, low_pass, peak, shelf,
};

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

/// Speaker enclosure acoustic profiles:
/// `[low_cut_hz, body_hz, body_db, body_q, cone_hz, cone_db, cone_q, high_cut_hz, trim_db]`.
const PROFILES: [[f32; 9]; 12] = [
    // 0: 1x12 classic open combo (-4.6 @100, -1.8 @250, -6.4 @1k, -5.3 @4k, -23.3 @8k, -36.3 @12k)
    [68.0, 235.0, 4.2, 1.1, 3600.0, 3.8, 1.3, 4650.0, -5.8],
    // 1: 2x12 vintage British
    [65.0, 190.0, 4.5, 1.2, 2700.0, 4.2, 1.5, 4900.0, -4.5],
    // 2: 4x12 closed classic rock (-2.2 @100, +2.7 @250, -2.5 @1k, -4.2 @4k, -28.3 @8k, -50.8 @12k)
    [60.0, 225.0, 5.2, 1.2, 3400.0, 3.2, 1.3, 3750.0, -2.5],
    // 3: 4x12 modern high-gain
    [58.0, 145.0, 4.5, 1.4, 2800.0, 4.5, 1.8, 5200.0, -3.5],
    // 4: 4x10 bright combo
    [78.0, 195.0, 3.5, 1.1, 3100.0, 4.0, 1.5, 5500.0, -4.0],
    // 5: 1x10 small studio amp
    [95.0, 240.0, 4.0, 1.3, 2400.0, 3.2, 1.6, 4400.0, -4.5],
    // 6: 1x8 tiny tweed
    [120.0, 260.0, 4.5, 1.5, 1850.0, 3.8, 1.8, 3900.0, -4.5],
    // 7: 2x12 jazz clean
    [62.0, 165.0, 2.8, 1.1, 3400.0, 2.5, 1.2, 6200.0, -3.5],
    // 8: 4x12 scooped vintage
    [60.0, 140.0, 5.5, 1.3, 1500.0, -3.5, 1.3, 4500.0, -3.5],
    // 9: Horn/tweeter bass cab (+0.5 @100, -2.9 @250, -2.9 @1k, -21.0 @2k notch, -0.2 @4k, +7.1 @8k, +8.6 @12k)
    [35.0, 100.0, 3.2, 1.0, 2000.0, -18.5, 2.6, 19_000.0, -2.6],
    // 10: 4x10 bass cabinet
    [45.0, 110.0, 4.5, 1.2, 1800.0, 3.0, 1.4, 4500.0, -3.5],
    // 11: 8x10 stadium bass stack
    [38.0, 90.0, 5.5, 1.3, 1250.0, 2.8, 1.5, 4100.0, -3.0],
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

/// Speaker cabinet simulator with room early reflections.
#[derive(Clone, Debug)]
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
    tweeter: Biquad,
    trim: f32,
    ring: DelayLine,
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
            bass: shelf(320.0, 0.0, 0.65, Side::Low, rate),
            treble: shelf(2600.0, 0.0, 0.65, Side::High, rate),
            tweeter: shelf(5200.0, 0.0, 0.7, Side::High, rate),
            trim: 1.0,
            ring: DelayLine::new(frames),
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

    pub fn reset(&mut self) {
        self.hp.reset();
        self.body.reset();
        self.cone.reset();
        self.lp1.reset();
        self.lp2.reset();
        self.bass.reset();
        self.treble.reset();
        self.tweeter.reset();
        self.ring.reset();
        self.air_lp = [0.0; 2];
        self.air_cur = self.s.air.clamp(0.0, 1.0);
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
            trim_db,
        ] = PROFILES.get(idx).copied().unwrap_or(PROFILES[0]);
        // Larger size shifts resonances downward; smaller shifts upward (~±0.85 oct).
        let scale = (0.85 * (0.5 - s.size.clamp(0.0, 1.0))).exp2();
        let r = self.rate;
        self.hp.retune(&high_pass(hp_hz * scale, 0.707, r));
        self.body.retune(&peak(body_hz * scale, body_q, body_db, r));
        self.cone.retune(&peak(cone_hz * scale, cone_q, cone_db, r));
        self.lp1.retune(&low_pass(lp_hz * scale, 0.85, r));
        self.lp2.retune(&low_pass(lp_hz * scale * 1.18, 0.707, r));
        self.bass.retune(&shelf(
            320.0,
            s.bass_db.clamp(-18.0, 18.0),
            0.65,
            Side::Low,
            r,
        ));
        self.treble.retune(&shelf(
            1450.0,
            s.treble_db.clamp(-18.0, 18.0),
            0.65,
            Side::High,
            r,
        ));
        let tweeter_db = if idx == 9 { 11.2 } else { 0.0 };
        self.tweeter
            .retune(&shelf(5200.0, tweeter_db, 0.7, Side::High, r));
        self.trim = 10.0f32.powf(trim_db / 20.0);
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
        self.tweeter.process(io);
        self.bass.process(io);
        self.treble.process(io);

        let trim = self.trim;
        let air_tgt = self.s.air.clamp(0.0, 1.0);
        let smooth = self.smooth;
        let damp = 0.35f32;

        for frame in io.iter_mut() {
            frame[0] *= trim;
            frame[1] *= trim;
            self.air_cur += (air_tgt - self.air_cur) * smooth;
            let [l, r] = *frame;
            self.ring.push(0.5 * (l + r));
            if self.air_cur > 1e-5 {
                let mut er = [0.0f32; 2];
                for (&off, &(_, g)) in self.tap_offsets.iter().zip(AIR_TAPS.iter()) {
                    let mono_tap = self.ring.read_int(off);
                    er[0] += mono_tap * g[0];
                    er[1] += mono_tap * g[1];
                }
                self.air_lp[0] = flush32(self.air_lp[0] + (er[0] - self.air_lp[0]) * damp);
                self.air_lp[1] = flush32(self.air_lp[1] + (er[1] - self.air_lp[1]) * damp);
                let wet = 0.65 * self.air_cur;
                frame[0] = flush32(l + wet * self.air_lp[0]);
                frame[1] = flush32(r + wet * self.air_lp[1]);
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
