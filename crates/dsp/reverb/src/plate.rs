//! Sixteen-line Walsh-Hadamard plate reverb: mono input sum through 20
//! first-order allpass diffusers, a 32-tap early-reflection ring feeding four
//! groups, 16 delay lines with per-line low-shelf and high-damping filters
//! mixed through a 16-point Walsh-Hadamard transform, four constant-power
//! stereo-panned output groups, and a wet stereo pre-delay.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use crate::Settings;
use crate::fade::flush;

const ALLPASSES: usize = 20;
const LINES: usize = 16;
const ER_TAPS: usize = 32;
const ALLPASS_COEFF: f32 = 0.9;
const MAX_PREDELAY_MS: f32 = 250.0;
const LOW_SHELF_HZ: f32 = 160.0;
const OUTPUT_LEVEL: f32 = 0.042_169_65;
const EARLY_LEVEL: f32 = 0.050_118_72;

/// The 16 plate delay line lengths in seconds.
const LINE_TIMES_S: [f32; LINES] = [
    0.021_145_834,
    0.021_395_834,
    0.023_604_166,
    0.025,
    0.026_375,
    0.029_062_5,
    0.030_895_833,
    0.032_854_166,
    0.034_479_167,
    0.037_854_165,
    0.040_041_666,
    0.042_437_5,
    0.045_437_5,
    0.048_354_167,
    0.050_979_167,
    0.055_062_5,
];

/// Polarity weights of the 16 lines into the 4 output groups.
const GROUP_SIGNS: [[f32; 4]; 4] = [
    [1.0, -1.0, 1.0, 1.0],
    [-1.0, 1.0, -1.0, -1.0],
    [-1.0, 1.0, -1.0, -1.0],
    [1.0, -1.0, 1.0, 1.0],
];

/// Early-reflection tap gains (32 taps, 8 per group).
const ER_GAINS: [f32; ER_TAPS] = [
    -1.0283, 1.0507, -1.0418, 0.9833, -0.8775, -1.3643, -1.2729, -1.2166, 1.3947, 1.2706, 1.2378,
    -1.4102, 1.3066, 1.4135, -1.3504, 1.0404, -1.0575, -0.8443, -1.1791, -1.0901, -1.0313, -1.1631,
    -1.2617, -1.2342, 0.9911, 0.9796, -0.7477, -1.2560, -1.1110, 0.9026, -1.3962, -1.0208,
];

/// Early-reflection tap delays in seconds (32 taps).
const ER_TIMES_S: [f32; ER_TAPS] = [
    0.031_166_667,
    0.029_062_5,
    0.029_895_833,
    0.035_541_665,
    0.046_687_5,
    0.003_5,
    0.010_291_667,
    0.014_708_334,
    0.001_333_333,
    0.010_458_333,
    0.013_020_833,
    0.000_25,
    0.007_729_167,
    0.000_020_833,
    0.004_5,
    0.030_020_833,
    0.028_416_667,
    0.050_458_334,
    0.017_770_832,
    0.025_458_334,
    0.030_875,
    0.019_104_166,
    0.011_145_833,
    0.013_312_5,
    0.034_770_835,
    0.035_916_667,
    0.062_354_166,
    0.011_583_333,
    0.023_583_334,
    0.043_916_665,
    0.001_229_167,
    0.031_875,
];

/// First-order bilinear shelf `(b0 + b1 z⁻¹) / (1 + a1 z⁻¹)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Shelf {
    b0: f32,
    b1: f32,
    a1: f32,
    x1: f32,
    y1: f32,
}

impl Shelf {
    /// DC gain `dc`, Nyquist gain `nyq`, transition at `hz / rate`.
    fn design(hz: f32, rate: f32, dc: f32, nyq: f32) -> Self {
        let f = (hz / rate).clamp(1e-5, 0.49);
        let t = (PI * f).tan();
        let s = (dc.max(1e-4) / nyq.max(1e-4)).sqrt();
        let d = t / s + 1.0;
        Self {
            b0: nyq * (t * s + 1.0) / d,
            b1: nyq * (t * s - 1.0) / d,
            a1: (t / s - 1.0) / d,
            x1: 0.0,
            y1: 0.0,
        }
    }

    fn retune(&mut self, to: Self) {
        self.b0 = to.b0;
        self.b1 = to.b1;
        self.a1 = to.a1;
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = flush(self.b0 * x + self.b1 * self.x1 - self.a1 * self.y1);
        self.x1 = x;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

struct TankLine {
    ring: kontra_dsp_core::FixedRing,
    gain: f32,
    low: Shelf,
    high: Shelf,
}

impl TankLine {
    fn new(len: usize) -> Self {
        Self {
            ring: kontra_dsp_core::FixedRing::new(len),
            gain: 0.0,
            low: Shelf::default(),
            high: Shelf::default(),
        }
    }

    #[inline]
    fn read(&self) -> f32 {
        self.ring.read()
    }

    #[inline]
    fn write_advance(&mut self, x: f32) {
        self.ring.write_advance(x);
    }

    fn reset(&mut self) {
        self.ring.reset();
        self.low.reset();
        self.high.reset();
    }
}

/// Sixteen-line Walsh-Hadamard plate reverb. Zero allocations in `set`/`process`.
pub struct Plate {
    rate: f32,
    allpass: [f32; ALLPASSES],
    er_ring: Box<[f32]>,
    er_pos: usize,
    er_offsets: [usize; ER_TAPS],
    lines: Box<[TankLine]>,
    predelay: Box<[[f32; 2]]>,
    pre_write: usize,
    pre_frames: f32,
    cur_pre_frames: f32,
    smooth: f32,
    /// Constant-power pan weights `[left, right]` for each of the 4 output groups.
    pan_l: [f32; 4],
    pan_r: [f32; 4],
    cur_pan_l: [f32; 4],
    cur_pan_r: [f32; 4],
    last: Option<Settings>,
}

impl Plate {
    /// Allocates all delay rings for `rate` (including max 250 ms pre-delay). Not real-time.
    #[must_use]
    pub fn new(s: Settings, rate: u32) -> Self {
        let rate_f = rate.max(1) as f32;
        let max_er = ER_TIMES_S.iter().fold(0.0f32, |a, &b| a.max(b));
        let er_len = ((max_er * rate_f).ceil() as usize + 8).max(64);
        let er_offsets = ER_TIMES_S.map(|t| ((t * rate_f) as usize).clamp(1, er_len - 1));
        let lines: Box<[TankLine]> = LINE_TIMES_S
            .iter()
            .map(|&t| TankLine::new((t * rate_f).round().max(1.0) as usize))
            .collect();
        let pre_cap = ((MAX_PREDELAY_MS * 0.001 * rate_f).ceil() as usize + 4).max(16);
        let smooth = 1.0 - (-1.0 / (0.020 * rate_f)).exp();
        let mut p = Self {
            rate: rate_f,
            allpass: [0.0; ALLPASSES],
            er_ring: vec![0.0; er_len].into_boxed_slice(),
            er_pos: 0,
            er_offsets,
            lines,
            predelay: vec![[0.0; 2]; pre_cap].into_boxed_slice(),
            pre_write: 0,
            pre_frames: 1.0,
            cur_pre_frames: 1.0,
            smooth,
            pan_l: [0.5; 4],
            pan_r: [0.5; 4],
            cur_pan_l: [0.5; 4],
            cur_pan_r: [0.5; 4],
            last: None,
        };
        p.set(s);
        p.cur_pre_frames = p.pre_frames;
        p.cur_pan_l = p.pan_l;
        p.cur_pan_r = p.pan_r;
        p
    }

    /// Updates reverb settings without allocating.
    pub fn set(&mut self, s: Settings) {
        if self.last == Some(s) {
            return;
        }
        self.last = Some(s);
        let rate = self.rate;
        let t60 = s.seconds.clamp(0.1, 30.0);
        // Low shelf gain below 160 Hz (clamped <= 0 dB per pass so feedback stays bounded).
        let low_db = s.low_shelf_db.clamp(-30.0, 6.0);
        // Per-pass shelf scaling derived from low shelf and high damping controls.
        let low_per_s = (low_db / 4.2).clamp(-6.5, 0.4);
        let hd = s.damping.clamp(0.0, 1.0);
        let damp_hz = s.high_cut_hz.clamp(1000.0, 0.48 * rate);
        let damp_gain = 10f32.powf((1.0 - hd) * 0.2 - 0.2);

        for (line, &sec) in self.lines.iter_mut().zip(&LINE_TIMES_S) {
            // Line decay gain per pass, normalized by 1/sqrt(16) = 0.25 for FWHT.
            let g_mid = 10f32.powf(-3.0 * sec / t60).min(0.995);
            line.gain = g_mid * 0.25;
            // Ensure combined per-pass shelf gain never exceeds 0.998/g_mid (guarantees stability).
            let max_boost = (0.996 / g_mid.max(1e-3)).min(1.15);
            let low_dc = 10f32.powf(low_per_s * sec).clamp(0.2, max_boost);
            line.low
                .retune(Shelf::design(LOW_SHELF_HZ, rate, low_dc, 1.0));
            line.high
                .retune(Shelf::design(damp_hz, rate, 1.0, damp_gain.clamp(0.2, 1.0)));
        }

        // Four-group constant-power stereo panning law.
        let st = s.stereo.clamp(0.0, 1.0);
        let angles = [
            ((1.0 - 2.0 * st) * FRAC_PI_4).max(0.0),
            (1.0 - st) * FRAC_PI_4,
            (1.0 + st) * FRAC_PI_4,
            ((1.0 + 2.0 * st) * FRAC_PI_4).min(FRAC_PI_2),
        ];
        for ((pl, pr), a) in self.pan_l.iter_mut().zip(&mut self.pan_r).zip(angles) {
            let (sin, cos) = a.sin_cos();
            *pl = cos;
            *pr = sin;
        }

        let max_pre = self.predelay.len().saturating_sub(2) as f32;
        self.pre_frames =
            (s.predelay_ms.clamp(0.0, MAX_PREDELAY_MS) * 0.001 * rate).clamp(0.0, max_pre.max(0.0));
    }

    /// Clears all delay lines and filter states.
    pub fn reset(&mut self) {
        self.allpass.fill(0.0);
        self.er_ring.fill(0.0);
        self.er_pos = 0;
        for l in &mut self.lines {
            l.reset();
        }
        self.predelay.fill([0.0; 2]);
        self.pre_write = 0;
        self.cur_pre_frames = self.pre_frames;
        self.cur_pan_l = self.pan_l;
        self.cur_pan_r = self.pan_r;
    }

    /// Processes stereo audio in place, replacing `io` with the wet plate signal.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let er_len = self.er_ring.len();
        let pre_len = self.predelay.len();
        if er_len < 2 || pre_len < 2 {
            return;
        }

        for frame in io {
            let mut x = 0.5 * (frame[0] + frame[1]);

            // 20 cascaded first-order allpass diffusers (a = 0.9).
            for z in &mut self.allpass {
                let v = flush(x + ALLPASS_COEFF * *z);
                let y = -ALLPASS_COEFF * v + *z;
                *z = v;
                x = y;
            }

            // Write diffused mono into early-reflection ring and read 32 taps into 4 groups.
            if let Some(slot) = self.er_ring.get_mut(self.er_pos) {
                *slot = x;
            }
            let mut er_groups = [0.0f32; 4];
            for ((grp, offs), gains) in er_groups
                .iter_mut()
                .zip(self.er_offsets.as_chunks::<8>().0)
                .zip(ER_GAINS.as_chunks::<8>().0)
            {
                let mut sum = 0.0f32;
                for (&off, &gain) in offs.iter().zip(gains) {
                    let r_idx = (self.er_pos + er_len - off) % er_len;
                    sum += self.er_ring.get(r_idx).copied().unwrap_or(0.0) * gain;
                }
                *grp = sum;
            }
            self.er_pos += 1;
            if self.er_pos >= er_len {
                self.er_pos = 0;
            }

            // Read all 16 delay lines, apply per-line shelves and gain, and mix via 16-pt FWHT.
            let mut y = [0.0f32; LINES];
            for (out, line) in y.iter_mut().zip(self.lines.iter_mut()) {
                let raw = line.read();
                let filt = line.high.tick(line.low.tick(raw));
                *out = filt * line.gain;
            }
            crate::hadamard::fwht(&mut y);

            // Feed diffused input + early reflections back into the 16 lines.
            for (((lines_g, mixed_g), signs), &er) in self
                .lines
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(y.as_chunks::<4>().0)
                .zip(&GROUP_SIGNS)
                .zip(&er_groups)
            {
                for ((line, &mixed), &sign) in lines_g.iter_mut().zip(mixed_g).zip(signs) {
                    line.write_advance(mixed + sign * er * 0.25);
                }
            }

            // Sum 16 lines into 4 polarity-weighted groups, pan across stereo with smooth glide.
            let smooth = self.smooth;
            self.cur_pre_frames += smooth * (self.pre_frames - self.cur_pre_frames);
            let mut late_l = 0.0f32;
            let mut late_r = 0.0f32;
            let mut early_l = 0.0f32;
            let mut early_r = 0.0f32;
            for ((((cur_l, cur_r), (&tgt_l, &tgt_r)), (y_g, signs)), &er) in self
                .cur_pan_l
                .iter_mut()
                .zip(&mut self.cur_pan_r)
                .zip(self.pan_l.iter().zip(&self.pan_r))
                .zip(y.as_chunks::<4>().0.iter().zip(&GROUP_SIGNS))
                .zip(&er_groups)
            {
                *cur_l += smooth * (tgt_l - *cur_l);
                *cur_r += smooth * (tgt_r - *cur_r);
                let late_g: f32 = y_g.iter().zip(signs).map(|(&v, &s)| v * s).sum();
                late_l += late_g * *cur_l;
                late_r += late_g * *cur_r;
                early_l += er * *cur_l;
                early_r += er * *cur_r;
            }

            // Combine early and late stereo reflections, then apply wet predelay to both
            // (predelay offsets the full wet onset of both early and late paths).
            let wet_l = OUTPUT_LEVEL * late_l + EARLY_LEVEL * early_l;
            let wet_r = OUTPUT_LEVEL * late_r + EARLY_LEVEL * early_r;
            if let Some(slot) = self.predelay.get_mut(self.pre_write) {
                *slot = [wet_l, wet_r];
            }
            let d = self.cur_pre_frames.clamp(0.0, (pre_len - 2) as f32);
            let i0 = d as usize;
            let frac = d - i0 as f32;
            let r0 = (self.pre_write + pre_len - i0) % pre_len;
            let r1 = (self.pre_write + pre_len - (i0 + 1)) % pre_len;
            let a = self.predelay.get(r0).copied().unwrap_or([0.0; 2]);
            let b = self.predelay.get(r1).copied().unwrap_or([0.0; 2]);
            let out_l = a[0] + frac * (b[0] - a[0]);
            let out_r = a[1] + frac * (b[1] - a[1]);
            self.pre_write += 1;
            if self.pre_write >= pre_len {
                self.pre_write = 0;
            }
            *frame = [
                if out_l.is_finite() { out_l } else { 0.0 },
                if out_r.is_finite() { out_r } else { 0.0 },
            ];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(
        seconds: f32,
        low_shelf_db: f32,
        damping: f32,
        predelay_ms: f32,
        stereo: f32,
    ) -> Settings {
        Settings {
            seconds,
            size: 1.0,
            damping,
            diffusion: 0.9,
            predelay_ms,
            high_cut_hz: (1.0 - damping) * 4851.0 + 6000.0,
            low_shelf_db,
            stereo,
            modulation_ms: 0.0,
            modulation_hz: 0.0,
        }
    }

    #[test]
    fn plate_produces_decaying_stereo_tail_and_respects_decay_and_stereo() {
        let rate = 48_000;
        let run = |s: Settings| -> Vec<[f32; 2]> {
            let mut p = Plate::new(s, rate);
            let mut io = vec![[0.0; 2]; rate as usize];
            io[0] = [1.0, 1.0];
            p.process(&mut io);
            assert!(io.iter().flatten().all(|v| v.is_finite()));
            io
        };

        let short = run(settings(0.6, 0.0, 0.5, 0.0, 1.0));
        let long = run(settings(6.5, 0.0, 0.5, 0.0, 1.0));
        let energy = |slice: &[[f32; 2]]| slice.iter().map(|[l, r]| l * l + r * r).sum::<f32>();
        assert!(energy(&long[24_000..]) > 5.0 * energy(&short[24_000..]));

        // Stereo=0 produces identical L/R; stereo=1 produces decorrelated L/R.
        let mono = run(settings(2.0, 0.0, 0.5, 0.0, 0.0));
        let mono_diff: f32 = mono.iter().map(|[l, r]| (l - r).abs()).fold(0.0, f32::max);
        assert!(
            mono_diff < 1e-6,
            "stereo=0 must be mono, got diff={mono_diff}"
        );

        let wide_diff: f32 = long[4096..16384]
            .iter()
            .map(|[l, r]| (l - r).abs())
            .fold(0.0, f32::max);
        assert!(
            wide_diff > 1e-3,
            "stereo=1 must decorrelate L/R, got {wide_diff}"
        );
    }

    #[test]
    fn extreme_settings_remain_stable_and_finite() {
        let mut p = Plate::new(settings(6.956, 5.625, 0.0, 250.0, 1.0), 48_000);
        let mut io = vec![[0.0; 2]; 48_000];
        io[0] = [1.0, 0.5];
        p.process(&mut io);
        assert!(io.iter().flatten().all(|v| v.is_finite() && v.abs() < 10.0));
        // Tail energy must decay, never diverge.
        let early: f32 = io[12_000..24_000].iter().map(|[l, r]| l * l + r * r).sum();
        let late: f32 = io[36_000..48_000].iter().map(|[l, r]| l * l + r * r).sum();
        assert!(
            late < early,
            "extreme positive low shelf must stay stable: early={early} late={late}"
        );
    }
}
