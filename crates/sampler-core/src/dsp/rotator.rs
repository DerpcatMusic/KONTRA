//! Kontakt Rotator core, ported from the native code in its own object layout. Offsets are the
//! native ones, so each line can be checked against the decompile by address.
//!
//! - [`Rotator::process`]: the block driver (native 0x140aa4900). It splits the block at the
//!   32-frame countdown (0x1bc), runs the control law (0x140a97d30) when the countdown ends, then
//!   the summed-input core (0x140aae920) on each chunk.
//! - [`Rotator::control`]: the control law. It runs the gain ramp, the rotor velocities and the
//!   phase/wavetable lookups for the horn and drum branches.
//! - Constants are the binary's values; `*_LANE` tables are the runtime lane vectors at
//!   0x149e6c400..0x149e6c45c.
//!
//! The state image is [`Rotator::m`]. Parameter binding (the setters that fill 0x210..0x220,
//! 0x1dc..0x1e8, 0x1380..0x138c and 0x1300..0x131c) is a separate step and stays at zero here.
//! The post stage of 0x140aa4900 (0x140ac21f0) and the zeroing of the extra channels are not
//! ported yet.
#![allow(clippy::too_many_lines)]

/// Native object image covering every offset the core and control use (0x0..0x1400).
const MEM: usize = 0x1400;
/// Sign bit (0x144714ba0).
const SIGN: u32 = 0x8000_0000;
/// Lane sign pattern (0x149e6c400..40c).
const SIGN_LANE: [f32; 4] = [1.0, -1.0, 1.0, -1.0];
/// Lane offsets of the first crossover (0x149e6c410..41c).
const K_LANE: [f32; 4] = [0.56; 4];
/// Rotor rate per lane (0x149e6c430..43c).
const RATE_LANE: [f32; 4] = [0.44, 0.44, 0.40, 0.40];
/// Tap scale and offset pairs per branch (0x149e6c440..45c): lanes 0..3 for horn, then drum.
const TAP_LANE: [f32; 8] = [0.44, 1.54, 0.44, 1.54, 0.40, 1.62, 0.40, 1.62];
/// Ring index step per lane (0x149e6c420..42c, int 1).
const STEP: u32 = 1;
/// 0x144fc4234: horn rate gain.
const G_HORN: f32 = 3.467;
/// 0x144fc43a4: drum rate gain.
const G_DRUM: f32 = 6.08;
/// 0x144f385a8: denormal guard added in the lattice and control stages.
const DC: f32 = 9.999_999_960_041_972e-13;
/// 0x144fc3270: squared-ramp threshold, a double.
const RAMP_EPS: f64 = 1e-15;
/// Control constants: 0x144714a34 (wrap length), 0x144fc4d58 and 0x144fc4d54 (the fold edges),
/// 0x1447377bc, 0x144714a24 (window), 0x14470bad0 and 0x14470bad4 (0.5 and 1.0).
const WRAP: f32 = 128.0;
const WRAP_NEG: f32 = -128.0;
const LOW_EDGE: f32 = -64.0;
const HIGH_EDGE: f32 = 64.0;
const WINDOW: f32 = 32.0;
const HALF: f32 = 0.5;
const ONE: f32 = 1.0;
/// 0x144fc3e90: scale of the 0x210 parameter in the velocity block.
const PARAM_SCALE: f32 = f32::from_bits(0x3fa1_f601);
/// 0x144fc3ab4: scale of (1 - 0x210) in the velocity block.
const SPREAD_SCALE: f32 = f32::from_bits(0x3f15_bd51);
/// 0x1447148f8: the 0.4 rotor ratio of the velocity block.
const RATIO: f32 = f32::from_bits(0x3ecc_cccd);
/// 0x144fc3210: phase-to-position step.
const PHASE_STEP: f32 = f32::from_bits(0x3c8c_3059);
/// 0x144fc3e38: position step of the phase accumulator.
const POS_STEP: f32 = f32::from_bits(0x3f94_bfd7);
/// Period of the control countdown in frames (0x1bc reset value, FUN_140aa4900).
const PERIOD: u32 = 0x20;

/// Sine table (native 0x144f58910): one cycle of 128 entries, read cyclically with linear
/// interpolation. Entries 128..131 of the native table are not part of the cycle.
const SINE: [f32; 128] = [
    0.146632135, 0.0999835208, 0.0527137779, 0.00490486622,
    -0.0433561318, -0.0919766203, -0.140858054, -0.189895496,
    -0.238977104, -0.287983656, -0.336787969, -0.385254443,
    -0.433238417, -0.480585843, -0.52713275, -0.572704971,
    -0.617118061, -0.660177231, -0.701677918, -0.741406262,
    -0.779140651, -0.814653218, -0.847712636, -0.878086805,
    -0.905547202, -0.929873168, -0.950857103, -0.968310237,
    -0.982068062, -0.991995871, -0.997993767, -1.0,
    -0.997993767, -0.991995871, -0.982068062, -0.968310237,
    -0.950857103, -0.929873168, -0.905547202, -0.878086805,
    -0.847712636, -0.814653218, -0.779140651, -0.741406262,
    -0.701677918, -0.660177231, -0.617118061, -0.572704971,
    -0.52713275, -0.480585843, -0.433238417, -0.385254443,
    -0.336787969, -0.287983656, -0.238977104, -0.189895496,
    -0.140858054, -0.0919766203, -0.0433561318, 0.00490486622,
    0.0527137779, 0.0999835208, 0.146632135, 0.192582399,
    0.237761557, 0.282100946, 0.325535774, 0.368004799,
    0.409450263, 0.449817508, 0.489054918, 0.527113676,
    0.563947678, 0.599513471, 0.63376987, 0.66667825,
    0.698202074, 0.728307068, 0.756960988, 0.784133673,
    0.809796929, 0.833924472, 0.856491864, 0.877476573,
    0.896857798, 0.914616585, 0.930735528, 0.945199192,
    0.957993686, 0.969106734, 0.978527904, 0.986248195,
    0.992260396, 0.996558845, 0.999139488, 1.0,
    0.999139488, 0.996558845, 0.992260396, 0.986248195,
    0.978527904, 0.969106734, 0.957993686, 0.945199192,
    0.930735528, 0.914616585, 0.896857798, 0.877476573,
    0.856491864, 0.833924472, 0.809796929, 0.784133673,
    0.756960988, 0.728307068, 0.698202074, 0.66667825,
    0.63376987, 0.599513471, 0.563947678, 0.527113676,
    0.489054918, 0.449817508, 0.409450263, 0.368004799,
    0.325535774, 0.282100946, 0.237761557, 0.192582399,
];

/// Rotator state image in the native object layout.
pub(crate) struct Rotator {
    m: Box<[u8; MEM]>,
}

impl Default for Rotator {
    fn default() -> Self {
        let mut r = Self { m: Box::new([0; MEM]) };
        // The countdown must start at a full period, or the driver never advances.
        r.wu(0x1bc, PERIOD);
        r
    }
}

/// Sign flip by bit, as the native `^ 0x80000000`.
fn neg(x: f32) -> f32 {
    f32::from_bits(x.to_bits() ^ SIGN)
}

/// Table read at phase `x` (native 0x144f58910 with the `iVar + 1 < 0x80` wrap). The base index is
/// taken modulo 128, so a phase outside [0, 128) reads the same cycle.
fn sine_at(x: f32) -> f32 {
    let i = x as i32;
    let a = SINE[i.rem_euclid(128) as usize];
    let b = SINE[(i + 1).rem_euclid(128) as usize];
    (b - a) * (x - i as f32) + a
}

/// Two-sided fold of a phase into [-64, 64) by one wrap (the `0x1dc`/`0x1e4` branches).
fn fold(t: f32) -> f32 {
    if t < HIGH_EDGE {
        if t < LOW_EDGE { t + WRAP } else { t }
    } else {
        t + WRAP_NEG
    }
}

impl Rotator {
    fn f(&self, o: usize) -> f32 {
        f32::from_le_bytes([self.m[o], self.m[o + 1], self.m[o + 2], self.m[o + 3]])
    }

    fn wf(&mut self, o: usize, v: f32) {
        self.m[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn u(&self, o: usize) -> u32 {
        u32::from_le_bytes([self.m[o], self.m[o + 1], self.m[o + 2], self.m[o + 3]])
    }

    fn wu(&mut self, o: usize, v: u32) {
        self.m[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// Block driver (native 0x140aa4900). The 0x1bc countdown splits the block; the control law
    /// runs when the countdown ends, and the summed core runs on each chunk. `inputs[0]` is the
    /// dry reference and the sum of all inputs feeds the lattice; `inputs[1]`, when present with
    /// two outputs, drives the second channel. `scratch` holds the sum and must have `n` frames.
    pub(crate) fn process(&mut self, inputs: &[&[f32]], scratch: &mut [f32], out: &mut [&mut [f32]], n: usize) {
        let sum = &mut scratch[..n];
        sum.fill(0.0);
        for input in inputs {
            for (d, s) in sum.iter_mut().zip(&input[..n]) {
                *d += *s;
            }
        }
        let sum: &[f32] = sum;
        let stereo = inputs.len() > 1 && out.len() > 1;

        let (mut at, mut left) = (0, n);
        while left > 0 {
            let period = self.u(0x1bc) as usize;
            let count = period.min(left);
            self.wu(0x1bc, (period - count) as u32);
            if period == count {
                self.control();
                self.wu(0x1bc, PERIOD);
            }
            if count > 0 {
                self.core(sum, inputs, stereo, out, at, count);
            }
            at += count;
            left -= count;
        }
    }

    /// Control law (native 0x140a97d30), once per countdown: gain ramp, rotor velocities, and the
    /// phase and wavetable values of both branches.
    fn control(&mut self) {
        // Gain ramp to 0x220 over the counter 0x13d0 (the step is 0x13d8 per control).
        if self.m[0x1c0] == 0 {
            let c = self.u(0x13d0) as i32;
            if c > 0 {
                self.wu(0x13d0, (c - 1) as u32);
                if c - 1 == 0 {
                    self.m.copy_within(0x220..0x228, 0x270);
                    self.m.copy_within(0x228..0x230, 0x278);
                    self.m[0x280..0x290].fill(0);
                }
            }
        } else {
            self.m[0x1c0] = 0;
            let step = (self.f(0x220) - self.f(0x270)) * self.f(0x13d8);
            self.wf(0x280, step);
            if f64::from(step * step) < RAMP_EPS {
                self.wf(0x280, 0.0);
                let target = self.f(0x220);
                self.wf(0x270, target);
            }
            if 0.0 < step * step {
                let c = self.u(0x13d4);
                self.wu(0x13d0, c);
            }
        }

        // Rotor velocities, rebuilt when the 0x21c countdown ends.
        if (self.u(0x21c) as i32) < 1 {
            let g = self.f(0x13d8);
            let k = self.f(0x214);
            let s = self.f(0x210) * PARAM_SCALE;
            self.wu(0x21c, self.u(0x13d4));
            let one_minus_k = ONE - k;
            let a2 = self.f(0x12a0);
            let a4 = self.f(0x12a8);
            let spread = (ONE - self.f(0x210)) * SPREAD_SCALE;
            self.wf(0x12b0, (s * one_minus_k - a2) * g);
            self.wf(0x12b8, (s * k - a4) * g);
            self.wf(0x12b4, (a4 * RATIO - self.f(0x12a4)) * g);
            self.wf(0x12bc, (a2 * RATIO - self.f(0x12ac)) * g);
            let b0 = self.f(0x1320);
            self.wf(0x1330, (spread * k - b0) * g);
            let b4 = self.f(0x1328);
            self.wf(0x1338, (one_minus_k * spread - b4) * g);
            self.wf(0x1334, (b4 * RATIO - self.f(0x1324)) * g);
            self.wf(0x133c, (b0 * RATIO - self.f(0x132c)) * g);
            let r = self.f(0x218);
            self.wf(0x12d0, (r * a2 - self.f(0x12c0)) * g);
            self.wf(0x12d8, (r * a4 - self.f(0x12c8)) * g);
            self.wf(0x12d4, (neg(r * self.f(0x12a4)) - self.f(0x12c4)) * g);
            self.wf(0x12dc, (neg(r * self.f(0x12ac)) - self.f(0x12cc)) * g);
            self.wf(0x1350, (r * b0 - self.f(0x1340)) * g);
            self.wf(0x1358, (r * b4 - self.f(0x1348)) * g);
            self.wf(0x1354, (neg(r * self.f(0x1324)) - self.f(0x1344)) * g);
            self.wf(0x135c, (neg(r * self.f(0x132c)) - self.f(0x134c)) * g);
        } else {
            self.wu(0x21c, (self.u(0x21c) as i32 - 1) as u32);
        }

        // Phase of the rotor positions: advances by 0x208 each control and wraps at 0.
        let phase = self.f(0x20c) - self.f(0x208);
        self.wf(0x20c, phase);
        if phase < 0.0 {
            self.wf(0x20c, phase + ONE);
            let a = (self.f(0x1dc) - self.f(0x1f4)) * self.f(0x1e0) + DC + self.f(0x1f4);
            self.wf(0x1f4, a);
            let b = (self.f(0x1e4) - self.f(0x1fc)) * self.f(0x1e8) + DC + self.f(0x1fc);
            self.wf(0x1fc, b);
            if self.f(0x1dc) == 0.0 {
                let t = fold((self.f(0x1ec) * HALF + self.f(0x1f8)) - WINDOW);
                let v = self.f(0x1f8) - (t * PHASE_STEP + DC);
                self.wf(0x1f8, v);
            }
            if self.f(0x1e4) == 0.0 {
                let t = fold((self.f(0x200) - self.f(0x1f0) * HALF) - WINDOW);
                let v = self.f(0x200) - (t * PHASE_STEP + DC);
                self.wf(0x200, v);
            }
            let mut p = a * POS_STEP + self.f(0x1f8);
            self.wf(0x1f8, p);
            if WRAP <= p {
                p -= WRAP;
                self.wf(0x1f8, p);
            }
            let rate = self.f(0x204);
            self.wf(0x1370, (sine_at(p) - self.f(0x1360)) * rate);
            let mut q = self.f(0x1ec) + self.f(0x1f8);
            if WRAP <= q {
                q += WRAP_NEG;
            }
            self.wf(0x1374, (sine_at(q) - self.f(0x1364)) * rate);
            let mut r = b * POS_STEP + self.f(0x200);
            self.wf(0x200, r);
            if WRAP <= r {
                r -= WRAP;
                self.wf(0x200, r);
            }
            self.wf(0x1378, (sine_at(r) - self.f(0x1368)) * rate);
            let s = self.f(0x200) - self.f(0x1f0);
            let s = if s < 0.0 { s + WRAP } else { s };
            self.wf(0x137c, (sine_at(s) - self.f(0x136c)) * rate);
        }
    }

    /// Core over frames `at..at + count` (native 0x140aae920). `sum` is the summed input.
    fn core(&mut self, sum: &[f32], inputs: &[&[f32]], stereo: bool, out: &mut [&mut [f32]], at: usize, count: usize) {
        let dry0 = inputs[0];

        for i in at..at + count {
            let x = sum[i];
            // DC stage (0x13c0..0x13cc).
            let t1 = x - (self.f(0x13c4) * self.f(0x13cc) + self.f(0x13c0));
            let y1 = self.f(0x13c8) * t1 + self.f(0x13c4);
            self.wf(0x13c4, y1);
            let z1 = y1 * self.f(0x13c8) + DC + self.f(0x13c0);
            self.wf(0x13c0, z1);

            // Lattice stages (0x230..0x264): horn branch from t1, drum branch from z1.
            let y2 = ((t1 - self.f(0x240)) * self.f(0x250) - self.f(0x230)) * self.f(0x260) + self.f(0x240);
            self.wf(0x240, y2);
            let fa = y2 * G_HORN;
            self.wf(0x230, y2 * self.f(0x260) + DC + self.f(0x230));
            let y3 = ((z1 - self.f(0x244)) * self.f(0x254) - self.f(0x234)) * self.f(0x264) + self.f(0x244);
            self.wf(0x244, y3);
            let fb = y3 * G_DRUM;
            self.wf(0x234, y3 * self.f(0x264) + DC + self.f(0x234));
            let w0 = self.u(0x290) as usize;
            self.wf(0x2a0 + 4 * w0, fa + t1);
            let w1 = self.u(0x294) as usize;
            self.wf(0xaa0 + 4 * w1, fb + z1);

            // Gain ramp (0x270 += 0x280).
            let gain = self.f(0x270) + self.f(0x280);
            self.wf(0x270, gain);

            // Four-lane integrators: each position (0x12a0, 0x1320, 0x12c0, 0x1340, 0x1360) takes
            // its velocity (0x12b0, 0x1330, 0x12d0, 0x1350, 0x1370) lane by lane.
            for k in 0..4 {
                let o = 4 * k;
                let v = self.f(0x12a0 + o) + self.f(0x12b0 + o);
                self.wf(0x12a0 + o, v);
                let v = self.f(0x1320 + o) + self.f(0x1330 + o);
                self.wf(0x1320 + o, v);
                let v = self.f(0x12c0 + o) + self.f(0x12d0 + o);
                self.wf(0x12c0 + o, v);
                let v = self.f(0x1340 + o) + self.f(0x1350 + o);
                self.wf(0x1340 + o, v);
                let v = self.f(0x1360 + o) + self.f(0x1370 + o);
                self.wf(0x1360 + o, v);
            }

            // Rotor positions and rates (new values).
            let s1360 = self.f(0x1360);
            let s1364 = self.f(0x1364);
            let s1368 = self.f(0x1368);
            let s136c = self.f(0x136c);
            let v37 = s1360 * RATE_LANE[0];
            let v43 = s1364 * RATE_LANE[1];
            let v38 = s1368 * RATE_LANE[2];
            let v39 = s136c * RATE_LANE[3];

            // Ring indices (0x290..0x29c advance under the masks 0x13a0..0x13ac).
            let mask = [self.u(0x13a0), self.u(0x13a4), self.u(0x13a8), self.u(0x13ac)];
            let a_base = self.u(0x290).wrapping_add(mask[0]);
            let b_base = self.u(0x294).wrapping_add(mask[1]);
            let next = [
                self.u(0x290).wrapping_add(STEP) & mask[0],
                self.u(0x294).wrapping_add(STEP) & mask[1],
                self.u(0x298).wrapping_add(STEP) & mask[2],
                self.u(0x29c).wrapping_add(STEP) & mask[3],
            ];
            self.wu(0x290, next[0]);
            self.wu(0x294, next[1]);
            self.wu(0x298, next[2]);
            self.wu(0x29c, next[3]);

            // Horn taps: four fractional reads of the ring at 0x2a0.
            let fg = [
                (v37 * SIGN_LANE[0] + TAP_LANE[0]) * self.f(0x1380),
                (v37 * SIGN_LANE[1] + TAP_LANE[1]) * self.f(0x1384),
                (v43 * SIGN_LANE[2] + TAP_LANE[2]) * self.f(0x1388),
                (v43 * SIGN_LANE[3] + TAP_LANE[3]) * self.f(0x138c),
            ];
            let ih: [i32; 4] = std::array::from_fn(|l| fg[l] as i32);
            let dh: [u32; 4] = std::array::from_fn(|l| a_base.wrapping_sub(ih[l] as u32));
            let tap_h: [f32; 4] = std::array::from_fn(|l| self.f(0x2a0 + 4 * (mask[l] & dh[l]) as usize));
            let nxt_h: [f32; 4] = std::array::from_fn(|l| self.f(0x2a0 + 4 * (dh[l].wrapping_sub(STEP) & mask[l]) as usize));
            let fv = [
                self.f(0x1300) - self.f(0x1310) * s1360,
                self.f(0x1304) - self.f(0x1314) * s1360,
                self.f(0x1308) - self.f(0x1318) * s1364,
                self.f(0x130c) - self.f(0x131c) * s1364,
            ];
            // Lanes 0..1 use the rate 0x1360, lanes 2..3 the rate 0x1364.
            let rate = [s1360, s1360, s1364, s1364];
            let s12a: [f32; 4] = std::array::from_fn(|l| self.f(0x12a0 + 4 * l));
            let s12c: [f32; 4] = std::array::from_fn(|l| self.f(0x12c0 + 4 * l));
            let k_old: [f32; 4] = std::array::from_fn(|l| self.f(0x12e0 + 4 * l));
            let f_old: [f32; 4] = std::array::from_fn(|l| self.f(0x12f0 + 4 * l));
            let out_h: [f32; 4] = std::array::from_fn(|l| {
                let frac = fg[l] - ih[l] as f32;
                let interp = frac * (nxt_h[l] - tap_h[l]) + tap_h[l];
                (interp * (s12a[l] - s12c[l] * rate[l]) - (k_old[l] * K_LANE[l] + f_old[l])) * fv[l] + k_old[l]
            });
            for l in 0..4 {
                self.wf(0x12e0 + 4 * l, out_h[l]);
                self.wf(0x12f0 + 4 * l, out_h[l] * fv[l] + f_old[l]);
            }

            // Drum taps: the same reads on the ring at 0xaa0, indices from b_base.
            let fd = [
                (v38 * SIGN_LANE[0] + TAP_LANE[4]) * self.f(0x1380),
                (v38 * SIGN_LANE[1] + TAP_LANE[5]) * self.f(0x1384),
                (v39 * SIGN_LANE[2] + TAP_LANE[6]) * self.f(0x1388),
                (v39 * SIGN_LANE[3] + TAP_LANE[7]) * self.f(0x138c),
            ];
            let ib: [i32; 4] = std::array::from_fn(|l| fd[l] as i32);
            let db: [u32; 4] = std::array::from_fn(|l| b_base.wrapping_sub(ib[l] as u32));
            let tap_d: [f32; 4] = std::array::from_fn(|l| self.f(0xaa0 + 4 * (mask[l] & db[l]) as usize));
            let nxt_d: [f32; 4] = std::array::from_fn(|l| self.f(0xaa0 + 4 * (db[l].wrapping_sub(STEP) & mask[l]) as usize));
            let interp_d: [f32; 4] = std::array::from_fn(|l| {
                let frac = fd[l] - ib[l] as f32;
                frac * (nxt_d[l] - tap_d[l]) + tap_d[l]
            });
            let s1320: [f32; 4] = std::array::from_fn(|l| self.f(0x1320 + 4 * l));
            let s1340: [f32; 4] = std::array::from_fn(|l| self.f(0x1340 + 4 * l));
            // Summed drum branch (lanes 1 and 3 with the rate 0x1368 and 0x136c).
            let drum_sum = (self.f(0x12fc) + self.f(0x12f4))
                + interp_d[3] * (s1320[3] - s136c * s1340[3])
                + interp_d[1] * (s1320[1] - s1368 * s1340[1]);

            // Outputs: the horn-side gain-ramped mix with the dry reference.
            let gain_now = self.f(0x270);
            let dry0_i = dry0[i];
            let y0 = ((interp_d[0] * (s1320[0] - s1368 * s1340[0]) + self.f(0x12f0) + drum_sum) - dry0_i) * gain_now + dry0_i;
            out[0][i] = y0;
            if stereo {
                let dry1_i = inputs[1][i];
                let y1 = ((interp_d[2] * (s1320[2] - s136c * s1340[2]) + self.f(0x12f8) + drum_sum) - dry1_i) * gain_now + dry1_i;
                out[1][i] = y1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Driver smoke test: the countdown returns to its period after each block, the output stays
    /// finite, and the table read is linear between entries and cyclic at 128.
    #[test]
    fn driver_splits_blocks_and_stays_finite() {
        let mut r = Rotator::default();
        let input: Vec<f32> = (0..96).map(|i| (i as f32 * 0.1).sin() * 0.5).collect();
        let mut scratch = vec![0.0f32; 96];
        let mut left = vec![0.0f32; 96];
        let mut right = vec![0.0f32; 96];
        for _ in 0..200 {
            let mut out: [&mut [f32]; 2] = [&mut left[..], &mut right[..]];
            r.process(&[&input[..]], &mut scratch[..], &mut out[..], 96);
            assert!(left.iter().chain(&right).all(|v| v.is_finite()));
        }
        assert_eq!(r.u(0x1bc), PERIOD);
        assert_eq!(sine_at(3.0), SINE[3]);
        assert!((sine_at(127.5) - (SINE[127] + SINE[0]) * 0.5).abs() < 1e-6);
    }
}
