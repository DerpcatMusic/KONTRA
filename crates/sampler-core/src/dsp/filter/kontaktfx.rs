//! Kontakt's KONTAKTFX filter cores: Simple LP/HP (saved type 120), Dual SKF (130) and Monark
//! (140..=143). Each is a configuration of the filter engine with its own recurrence, because
//! no section cascade expresses their feedback nonlinearities (the Ladder sibling here runs a
//! different native cutoff range, tap table and summation order from the v1-pinned Ladder
//! kernel, so it is not that kernel either).
//!
//! - Simple LP/HP (BFilterSimpleLPHP): FilterDJExtRange mode 2 high pass (vt 0x144fcf580; mode
//!   0x140b04660, setParam 0x140b058a0, prepare 0x140afd0f0, block 0x140b03490, process
//!   0x140afef00) into FilterLadderExtRange mode 0 (vt 0x144fcf7d8; setParam 0x140b05ce0,
//!   process 0x140b01440, prepare 0x140afd5f0/0x140b06380).
//! - Dual SKF (vt 0x144fce538): setParam 0x140b059c0, prepare 0x140afd350, block 0x140b03630,
//!   process 0x140b000c0, cutoff law 0x140b06580; x2 through the shared Kaiser stage below
//!   66 kHz.
//! - Monark (FilterBlueMonark vt 0x144f129a0): setParam 0x140b05740, prepare 0x140afcc40 (mode
//!   constants 0x140b06640), setType 0x140af0020, block 0x140b032f0, process 0x140afd9c0; x3
//!   below 66 kHz, x2 below 100 kHz. Tables 0x149e6c600.. are `monark.f32le`.
//!
//! Every operation is f32 in the native order (models sv/{dj,lad,skf,mon}_model.py, bit-exact
//! against the classes under kemu). Controls run on the native 32-frame block clock with its
//! 2 ms ramps (DJ: 1 ms per slot).
// ponytail: stereo runs the mono cores per channel with shared controls (DJ averages its
// resonance envelope over both channels as its channel loop does); the Monark noise LCG is per
// channel. The native channel interleave is unprobed; vectors are mono.
#![allow(clippy::excessive_precision, clippy::unreadable_literal)]
use super::super::kaiser::{Kaiser, State as KaiserState};
use super::super::{Planar, ProcessorState};
use super::ladder_kernel::{exponential_effektrig as table, prepare as prepare_table};
use sampler_ir::fx::filter::KontaktFx;
use std::sync::OnceLock;

/// Frames per native control block.
const QUANTUM: u64 = 32;
const EPS: f64 = 1e-15;
const PI: f32 = 3.14159274;
const C3: f32 = 3.28986835;
const C5: f32 = 12.9878807;
const TINY: f32 = KF385A4;

// Native constants (address low bits in the name; 0x144xxxxxx .rdata).
const KFCFEAC: f32 = f32::from_bits(0x4cbebc20); // 100000000
const KFC4974: f32 = f32::from_bits(0x42aa0000); // 85
const KF38CD8: f32 = f32::from_bits(0x43020000); // 130
const KF38C70: f32 = f32::from_bits(0x41c00000); // 24
const K7578C0: f32 = f32::from_bits(0x428c0000); // 70
const KFC4B50: f32 = f32::from_bits(0x44960000); // 1200
const KFC44FC: f32 = f32::from_bits(0x411f73da); // 9.96578407
const KF38E2C: f32 = f32::from_bits(0xc47a0000); // -1000
const K714A40: f32 = f32::from_bits(0x447a0000); // 1000
const K737710: f32 = f32::from_bits(0x3a03126f); // 0.000500000024
const KFCFD5C: f32 = f32::from_bits(0x3f7f1fc3); // 0.996578395
const KFCFCE8: f32 = f32::from_bits(0x3e19130f); // 0.149486765
const K7148D0: f32 = f32::from_bits(0x3dcccccd); // 0.100000001
const KFCFEBC: f32 = f32::from_bits(0xbbc409c3); // -0.005982609
const KFCFEB0: f32 = f32::from_bits(0xb7b58cf0); // -2.16425105e-05
const KFCFD48: f32 = f32::from_bits(0x3f5c28f6); // 0.860000014
const KFCFC90: f32 = f32::from_bits(0x3b4be3c7); // 0.00311111077
const KFCFE14: f32 = f32::from_bits(0x41af3a42); // 21.9034462
const KFCFE90: f32 = f32::from_bits(0x4502c4cd); // 2092.30005
const KFCFE54: f32 = f32::from_bits(0x431fd127); // 159.817001
const K71CB14: f32 = f32::from_bits(0x41a00000); // 20
const KFCFE44: f32 = f32::from_bits(0x430c0000); // 140
// DJ k34 immediates by group: 0x40d491d1 0xbf1a5e35 0xbf19999a
const LADDER_MIX: [f32; 5] = [f32::from_bits(0x00000000), f32::from_bits(0x3f800000), f32::from_bits(0x00000000), f32::from_bits(0x00000000), f32::from_bits(0x00000000)];
const LADDER_THR: [f32; 4] = [f32::from_bits(0x00000000), f32::from_bits(0x3e800000), f32::from_bits(0x3f400000), f32::from_bits(0x3f800000)];
const LADDER_GAIN: [f32; 4] = [f32::from_bits(0x3efae148), f32::from_bits(0x3f547ae1), f32::from_bits(0x3f547ae1), f32::from_bits(0x3ecccccd)];
const LADDER_SLOPE: [f32; 3] = [f32::from_bits(0x40800000), f32::from_bits(0x40000000), f32::from_bits(0x40800000)];
const KFCFE58: f32 = f32::from_bits(0x43669976); // 230.599457
const KFCFE10: f32 = f32::from_bits(0x41a5b934); // 20.7154312
const KFCFD9C: f32 = f32::from_bits(0x3fe74a0a); // 1.80694699
const KFCFCBC: f32 = f32::from_bits(0x3d467f85); // 0.0484614559
const KFCFE00: f32 = f32::from_bits(0x411b36e7); // 9.70090389
const KFCFD70: f32 = f32::from_bits(0x3fa648c8); // 1.29909611
const KF386A0: f32 = f32::from_bits(0x3d67b662); // 0.0565704182
const KF3873C: f32 = f32::from_bits(0x3e7631c8); // 0.240424275
const KF387FC: f32 = f32::from_bits(0x3f3167a3); // 0.692987621
const KF38654: f32 = f32::from_bits(0x3c1a7997); // 0.00942840334
const KFCFDF8: f32 = f32::from_bits(0x4102d013); // 8.17579937
const K7148B4: f32 = f32::from_bits(0x3c23d70a); // 0.00999999978
const KFCFD88: f32 = f32::from_bits(0x3fc114e4); // 1.50845003
const KFCFCB4: f32 = f32::from_bits(0x3d1f1c25); // 0.0388452001
const KFCFCC0: f32 = f32::from_bits(0x3db7a1a8); // 0.0896638036
const KFCFD60: f32 = f32::from_bits(0x3f8001a3); // 1.00004995
const KFCFCB8: f32 = f32::from_bits(0x3d259bd1); // 0.0404318012
const KFCFD0C: f32 = f32::from_bits(0x3edc9b20); // 0.43087101
const KF385A4: f32 = f32::from_bits(0x219392ef); // 1.00000005e-18
const KFCFE98: f32 = f32::from_bits(0x45480000); // 3200
const KFCFEA8: f32 = f32::from_bits(0x4780e800); // 66000
const SKF_CLIP: [f32; 4] = [f32::from_bits(0x40400000), f32::from_bits(0x3eaaaaab), f32::from_bits(0x3fc00000), f32::from_bits(0x3f2aaaab)];
const MF38804: f32 = f32::from_bits(0x3f333333); // 0.699999988
const MFCFD28: f32 = f32::from_bits(0x3f0ccccc); // 0.549999952
const MFCFEC4: f32 = f32::from_bits(0xbfa00000); // -1.25
const M714940: f32 = f32::from_bits(0x3fa00000); // 1.25
const MFCFE30: f32 = f32::from_bits(0x423c0000); // 47
const MFCFE4C: f32 = f32::from_bits(0x43114ccd); // 145.300003
const MF38820: f32 = f32::from_bits(0x3f4ccccd); // 0.800000012
const MFCFDE4: f32 = f32::from_bits(0x40c999a0); // 6.30000305
const MF38CC0: f32 = f32::from_bits(0x42a00000); // 80
const MFCFC94: f32 = f32::from_bits(0x3be18505); // 0.00688231224
const MFCFC88: f32 = f32::from_bits(0x37a7e153); // 2.00128761e-05
const M7149F4: f32 = f32::from_bits(0x40f00000); // 7.5
const M71CB14: f32 = f32::from_bits(0x41a00000); // 20
const MFCFD08: f32 = f32::from_bits(0x3ea06d3a); // 0.313333333
const M7149D0: f32 = f32::from_bits(0x40400000); // 3
const MF38CD0: f32 = f32::from_bits(0x42f00000); // 120
const M7148B4: f32 = f32::from_bits(0x3c23d70a); // 0.00999999978
const M70BAF8: f32 = f32::from_bits(0x41400000); // 12
const M7148D0: f32 = f32::from_bits(0x3dcccccd); // 0.100000001
const MFCFCFC: f32 = f32::from_bits(0x3e872b02); // 0.263999999
const MFCFDD8: f32 = f32::from_bits(0x40a00001); // 5.00000048
const MFCFDDC: f32 = f32::from_bits(0x40accccd); // 5.4000001
const M737724: f32 = f32::from_bits(0x3daaaaab); // 0.0833333358
const MFCFDF8: f32 = f32::from_bits(0x4102d013); // 8.17579937
const MFCFCE0: f32 = f32::from_bits(0x3e088889); // 0.13333334
const M760108: f32 = f32::from_bits(0x3eaaaaab); // 0.333333343
const MFCFD98: f32 = f32::from_bits(0x3fd8f2d0); // 1.69491005
const MFCFEC8: f32 = f32::from_bits(0xbfaf5c29); // -1.37
const MFCFD4C: f32 = f32::from_bits(0x3f5d70a4); // 0.86500001
const MFCFD8C: f32 = f32::from_bits(0x3fc28f5c); // 1.51999998
const MFCFDC4: f32 = f32::from_bits(0x406e76c9); // 3.72600007
const MFCFDBC: f32 = f32::from_bits(0x40555547); // 3.33332992
const MFCFECC: f32 = f32::from_bits(0xbfe04189); // -1.75199997
const MFCFD94: f32 = f32::from_bits(0x3fcf5c29); // 1.62
const MFCFE18: f32 = f32::from_bits(0x41d80000); // 27
const MF38E04: f32 = f32::from_bits(0xc0800000); // -4
const M7149FC: f32 = f32::from_bits(0x41100000); // 9
const M7149E4: f32 = f32::from_bits(0x40800000); // 4
const M714A00: f32 = f32::from_bits(0x41200000); // 10
const M7148C0: f32 = f32::from_bits(0x3d4ccccd); // 0.0500000007
const MF38764: f32 = f32::from_bits(0x3e99999a); // 0.300000012
const MFCFCA8: f32 = f32::from_bits(0x3c9a0275); // 0.0187999997
const MFCFD24: f32 = f32::from_bits(0x3f08d64d); // 0.53451997
const MFCFD90: f32 = f32::from_bits(0x3fcd4174); // 1.60355997
const MFCFD04: f32 = f32::from_bits(0x3e9fa509); // 0.311805993
const M7377CC: f32 = f32::from_bits(0x47c35000); // 100000
const MFCFEA8: f32 = f32::from_bits(0x4780e800); // 66000
const MF38D04: f32 = f32::from_bits(0x43fa0000); // 500
const MFCFE1C: f32 = f32::from_bits(0x41fbfbe7); // 31.4979992
const MF38778: f32 = f32::from_bits(0x3eb33333); // 0.349999994
const MFCFDD4: f32 = f32::from_bits(0x4093ad58); // 4.61491013
const MFCFE5C: f32 = f32::from_bits(0x436dcf9e); // 237.811005
const MFCFE0C: f32 = f32::from_bits(0x41973333); // 18.8999996
const MFCFDE0: f32 = f32::from_bits(0x40b3b646); // 5.61600018
const MFCFE9C: f32 = f32::from_bits(0x45d2f001); // 6750.00049
const MFCFD80: f32 = f32::from_bits(0x3fb504f3); // 1.41421354
const ME7BD94: f32 = f32::from_bits(0x42960000); // 75
const M714A0C: f32 = f32::from_bits(0x41700000); // 15
const M7149D4: f32 = f32::from_bits(0x40490fdb); // 3.14159274
const MF385C4: f32 = f32::from_bits(0x37be37c6); // 2.26757365e-05
const MFCFC84: f32 = f32::from_bits(0x2f80009d); // 2.32835001e-10
const MF386A0: f32 = f32::from_bits(0x3d67b662); // 0.0565704182
const MF3873C: f32 = f32::from_bits(0x3e7631c8); // 0.240424275
const MF387FC: f32 = f32::from_bits(0x3f3167a3); // 0.692987621
const MF38654: f32 = f32::from_bits(0x3c1a7997); // 0.00942840334
const T368: [f32; 4] = [f32::from_bits(0x41b9cd9f), f32::from_bits(0x41080000), f32::from_bits(0x40c00000), f32::from_bits(0x40c00000)];
const T378: [f32; 4] = [f32::from_bits(0x3e9b9475), f32::from_bits(0x3e9b9475), f32::from_bits(0x3e9b9475), f32::from_bits(0x3eb851ec)];
const T388: [f32; 4] = [f32::from_bits(0x40100000), f32::from_bits(0x40100000), f32::from_bits(0x40100000), f32::from_bits(0x3f000000)];
const T398: [f32; 4] = [f32::from_bits(0x3f800000), f32::from_bits(0x3f6b851f), f32::from_bits(0x3f400000), f32::from_bits(0x3fe66666)];
const T3A8: [f32; 4] = [f32::from_bits(0x3cf3dab6), f32::from_bits(0x3ce05894), f32::from_bits(0x3cb6e409), f32::from_bits(0x3cd049fb)];
const T3B8: [(usize, usize, f32); 4] = [(3, 0, f32::from_bits(0x00000000)), (1, 0, f32::from_bits(0x00000000)), (0, 0, f32::from_bits(0x00000000)), (1, 3, f32::from_bits(0x3f800000))];
// monark.f32le: T600 48, T6C0 21, T720 48, T7E0 4032, T6E0 4032 = 8181

/// Monark tables 0x149e6c600 (cutoff -> note, 48), 0x149e6c6c0 (res row -> feedback, 21),
/// 0x149e6c720 (mod depth, 48), 0x149e6c7e0 / 0x149e706e0 ([mode][21 res][48 note] each).
fn monark_tables() -> &'static [f32] {
    static T: OnceLock<Vec<f32>> = OnceLock::new();
    T.get_or_init(|| {
        include_bytes!("monark.f32le").chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
    })
}
const T600: usize = 0;
const T6C0: usize = 48;
const T720: usize = 69;
const T7E0: usize = 117;
const T6E0: usize = 117 + 4032;

/// 8-slot control ramp (cur, delta, target; per-sample delta spread over `n` blocks).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Ramp {
    cur: [f32; 5],
    d: [f32; 5],
    tgt: [f32; 5],
    dirty: bool,
    count: i32,
}

/// Ramp length in blocks and the per-sample step, for `rate` and `frames` per block.
#[derive(Clone, Copy, Debug)]
struct Timing {
    n: i32,
    inv: f32,
}

impl Timing {
    fn new(rate: f32, frames: u32, seconds: f32) -> Self {
        let n = ((rate * seconds / frames as f32 + 0.5) as i32).max(1);
        Self { n, inv: 1.0 / (n * frames as i32) as f32 }
    }
}

impl Ramp {
    fn snap(&mut self) {
        self.cur = self.tgt;
        self.dirty = true;
    }

    fn block(&mut self, t: Timing) {
        if self.dirty {
            self.dirty = false;
            let mut total = 0f32;
            for i in 0..5 {
                let mut d = (self.tgt[i] - self.cur[i]) * t.inv;
                if f64::from(d * d) < EPS {
                    d = 0.0;
                    self.cur[i] = self.tgt[i];
                }
                self.d[i] = d;
                total += d * d;
            }
            if total > 0.0 {
                self.count = t.n;
            }
            return;
        }
        self.count = self.count.wrapping_sub(1);
        if self.count == 0 {
            self.cur = self.tgt;
            self.d = [0.0; 5];
        }
    }

    /// The core's own advance by `k` samples' deltas.
    fn advance(&mut self, k: f32) {
        for i in 0..5 {
            self.cur[i] = k * self.d[i] + self.cur[i];
        }
    }
}

/// `tan(pi f T)` by the native 5th-order series (0x140b04f60).
fn tan_series(f: f32, t: f32) -> f32 {
    let a = (t * t) * C5;
    let g = (((a * (f * f)) + C3) * f) * ((t * f) * t) + 1.0;
    g * ((f * PI) * t)
}

// ---- FilterDJExtRange, mode 2 (group 0 high pass) -------------------------------------------

/// One DJ ramp slot (0x28 bytes): 1 ms in whole blocks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Slot {
    cur: f32,
    d: f32,
    tgt: f32,
    dirty: bool,
    count: i32,
}

impl Slot {
    fn block(&mut self, t: Timing) {
        if self.dirty {
            self.dirty = false;
            self.count = t.n;
            self.d = (self.tgt - self.cur) * t.inv;
            if f64::from(self.d) * f64::from(self.d) < EPS {
                self.cur = self.tgt;
                self.d = 0.0;
                self.count = 0;
            }
        } else if self.count != 0 {
            self.count -= 1;
            if self.count == 0 {
                self.cur = self.tgt;
                self.d = 0.0;
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Dj {
    t: f32,
    timing: Timing,
    k34: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct DjState {
    /// gain, cutoff Hz, resonance.
    slots: [Slot; 3],
    /// Per channel: hp, band, low.
    z: [[f32; 3]; 2],
    fb: f32,
    e48: f32,
    e4c: f32,
}

/// The DJ clip `(1 - |c|/2000) c`, `c` clamped to +-1000.
fn dj_sat(c: f32) -> f32 {
    let c = c.clamp(KF38E2C, K714A40);
    (1.0 - c.abs() * K737710) * c
}

impl Dj {
    const K38: f32 = 0.0;
    const K3C: f32 = 24.0;

    fn new(rate: f32) -> Self {
        Self {
            t: 1.0 / rate,
            timing: Timing::new(rate, QUANTUM as u32, 0.001),
            k34: table(f32::from_bits(0x40d491d1) * KFC44FC + KFC4B50),
        }
    }

    fn set(&self, s: &mut DjState, cutoff: f32, resonance: f32) {
        let n = (cutoff * KFCFE54 - K71CB14).min(KFCFE44);
        let hz = table(n * 5.0 + 1381.88159);
        let g = tan_series(hz.max(0.0), self.t);
        s.slots[1].tgt = hz;
        s.slots[0].tgt = g;
        s.slots[2].tgt = resonance;
        s.slots.iter_mut().for_each(|q| q.dirty = true);
    }

    fn snap(s: &mut DjState) {
        for q in &mut s.slots {
            (q.cur, q.d, q.dirty, q.count) = (q.tgt, 0.0, false, 0);
        }
    }

    #[inline(always)]
    fn tick(&self, s: &mut DjState, x: [f32; 2]) -> [f32; 2] {
        for q in &mut s.slots {
            q.cur = q.d + q.cur;
        }
        let (g, fc, r) = (s.slots[0].cur, s.slots[1].cur, s.slots[2].cur);
        let r24 = r * Self::K3C;
        let k = ((1.0 - s.fb) + (1.0 - s.fb)) + g;
        let (k54, k50) = (g * k, g + k);
        let k58 = 1.0 / (k54 + 1.0);
        // mix LP 0, BP 0, HP 1 (mode 2); group 0 gain law
        let gn = table((((KFCFD5C - r24 * KFCFCE8) * 0.0) - KFCFD5C) * r24 + KFC4B50);
        let mut acc = 0f32;
        let mut y = [0f32; 2];
        for c in 0..2 {
            let [a, b, l] = s.z[c];
            let hp = (x[c] - ((b * k50 + l) + a * k54)) * k58;
            let v = (a + hp) * g + b;
            let w = (v + b) * g + l;
            s.z[c] = [hp, dj_sat(v), dj_sat(w)];
            let bp = v + v;
            acc += bp;
            y[c] = (((w * 0.0) + (0.0 * bp)) + (hp * 1.0)) * gn;
        }
        // resonance envelope (after both channels)
        let t = self.t;
        let f = table(fc * 5.0 + 1381.88159);
        let a = f.max(KF38C70).min(K7578C0);
        let b = f.max(KFC4974).min(KF38CD8);
        let f21 = (a - K7578C0) * KFCFEBC;
        let f23 = ((b - KF38CD8) * (a - K7578C0)) * KFCFEB0;
        let f30 = KFCFD48 - (b - KF38CD8) * KFCFC90;
        let mut e = (acc * 0.5).abs();
        if e < s.e48 {
            e = ((e - s.e48) * t) * KFCFE14 + s.e48;
        }
        s.e48 = e;
        let ec = self.k34.max(e).min(KFCFEAC);
        let tt = table((r24 - Self::K38) * KFC44FC + KFC4B50);
        let h = 1.0 - 1.0 / ((tt - 1.0) * (self.k34 / ec) + 1.0);
        let h = if h > 0.0 { h } else { 0.0 };
        s.e4c = ((h - s.e4c) * t) * KFCFE90 + s.e4c;
        s.fb = s.e4c * (f30 - (f21 - f23));
        y
    }
}

// ---- FilterLadderExtRange, mode 0, rate factor 1 ---------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Ladder {
    t: f32,
    timing: Timing,
    c28c: f32,
    c290: f32,
    c294: f32,
    c298: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LadderState {
    ramp: Ramp,
    count: u32,
    gin: f32,
    g: f32,
    big_g: f32,
    r: f32,
    og: f32,
    /// Per channel: four stages and the feedback DC-block state.
    z: [[f32; 5]; 2],
}

/// Output gain against `r^2` (mode 0 rows of 0x144fcf070 / 0x144fcf130 / 0x144fcf280).
fn ladder_gain(r2: f32) -> f32 {
    let (g, s) = (LADDER_GAIN, LADDER_SLOPE);
    let (a, b) = (LADDER_THR[1], LADDER_THR[2]);
    if a <= r2 {
        let (lo, hi, t) = if b <= r2 { (g[2], g[3], (r2 - b) * s[2]) } else { (g[1], g[2], (r2 - a) * s[1]) };
        let q = (1.0 - t) * (1.0 - t);
        let q = (2.0 - q) * q;
        return ((lo.abs() * (((2.0 - t) * t) - (1.0 - q)) + (1.0 - q)) * ((hi - lo) + (hi - lo)) + lo) + lo;
    }
    let lo = g[0];
    let t = r2 * s[0];
    let q = (1.0 - t) * (1.0 - t);
    let q = 1.0 - (2.0 - q) * q;
    ((lo.abs() * (((2.0 - t) * t) - q) + q) * ((g[1] - lo) + (g[1] - lo)) + lo) + lo
}

impl Ladder {
    fn new(rate: f32) -> Self {
        let t = 1.0 / rate;
        let f7 = table(1196.0 + 0.681396484);
        let f6 = (80.0 / f7).max(0.0);
        let f8 = (t * t) * C5;
        let g = ((((((f8 * f6) * f6) + C3) * f6) * t) * f6) * t + 1.0;
        let g = ((g * f6) * PI) * t;
        Self { t, timing: Timing::new(rate, QUANTUM as u32, 0.002), c28c: g, c290: 1.0 / (g + 1.0), c294: g - 1.0, c298: f7 * f7 - 1.0 }
    }

    /// Targets for gain 0, `cutoff`, resonance 0.
    fn set(s: &mut LadderState, cutoff: f32) {
        s.ramp.tgt[0] = table(1200.0);
        s.ramp.tgt[1] = cutoff;
        s.ramp.tgt[2] = 1.0 - (1.0 - 0.0) * (1.0 - 0.0);
        s.ramp.dirty = true;
    }

    fn update(&self, s: &mut LadderState) {
        for i in 0..4 {
            s.ramp.cur[i] = 4.0 * s.ramp.d[i] + s.ramp.cur[i];
        }
        let c = s.ramp.cur;
        s.gin = c[0];
        let f = table(c[1] * 799.085022 + 1281.88159).max(0.0);
        let t = f * self.t;
        let t2 = t * t;
        s.g = ((((t2 * 40.8026314) + 10.3354263) * t2 + PI) * t).min(8.0);
        s.og = ladder_gain(c[2] * c[2]);
        s.r = c[2];
        s.big_g = 1.0 / (s.g + 1.0);
    }

    #[inline(always)]
    fn tick(&self, s: &mut LadderState, x: [f32; 2]) -> [f32; 2] {
        if s.count & 3 == 0 {
            s.count = 0;
            self.update(s);
        }
        s.count += 1;
        let (g, big_g) = (s.g, s.big_g);
        let gg = big_g * g;
        let k4 = s.r * 4.15999985;
        let mut out = [0f32; 2];
        for c in 0..2 {
            let z = &mut s.z[c];
            let xi = x[c] * s.gin;
            let (mut p, mut sum) = (1f32, 0f32);
            for &st in &z[..4] {
                p *= gg;
                sum = ((sum * g) + (st + st)) * big_g;
            }
            let y = ((p * xi + sum) / (p * k4 + 1.0)) * k4;
            let h0 = z[4];
            let h = (y - h0 * self.c294) * self.c290;
            z[4] = h;
            let u = xi - ((((h + h0) * self.c28c) * self.c298) + y);
            let u = u.max(-24.0).min(24.0);
            let u = (1.0 - u.abs() * 0.020833334) * u;
            let mut v = u;
            let mut vs = [0f32; 4];
            for (j, st) in z[..4].iter_mut().enumerate() {
                let s0 = *st;
                let n = ((v - (((s0 + s0) + (v * g)) * big_g)) * g) + s0;
                *st = n;
                v = n + s0;
                vs[j] = v;
            }
            let m = LADDER_MIX;
            let y = (((vs[3] * m[4]) + (vs[2] * m[3])) + (vs[1] * m[2])) + ((vs[0] * m[1]) + (u * m[0]));
            out[c] = y * s.og;
        }
        out
    }
}

// ---- Dual SKF ---------------------------------------------------------------------------------

/// Knob to hertz (0x140b06580).
fn skf_hz(x: f32) -> f32 {
    if x < K7148D0 {
        return ((x * KFCFE58) + KFCFE10) - (1.0 / ((x * KFCFD9C) + KFCFCBC));
    }
    note_exp(x * KFCFE00 + KFCFD70)
}

/// `8.1758 * 2^y`: the integer part (rounded to nearest) into the exponent, `2^frac` by a quartic.
fn note_exp(y: f32) -> f32 {
    let i = y.round_ties_even();
    let f = y - i;
    let mut p = f * KF386A0 + KF3873C;
    let f2 = f * f;
    let lin = f * KF387FC;
    p *= f2;
    let f4 = (f2 * f2) * KF38654;
    p += lin;
    p += 1.0;
    p += f4;
    (p * 2f32.powi(i as i32)) * KFCFDF8
}

const SKF_C238: [f32; 2] = [1.4, 1.5];
const SKF_C220: [f32; 4] = [0.45, 0.55, 0.3, 1.5];

#[derive(Clone, Copy, Debug)]
struct Skf {
    w: f32,
    timing: Timing,
    os: Option<Kaiser>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SkfState {
    ramp: Ramp,
    count: u32,
    h: [f32; 2],
    dv: f32,
    /// Per channel: two stages of (s1, s2).
    z: [[[f32; 2]; 2]; 2],
    rs: [KaiserState; 2],
}

impl Skf {
    fn new(rate: f32) -> Self {
        let os = if rate < KFCFEA8 { 2 } else { 1 };
        let fso = os as f32 * rate;
        Self {
            w: (std::f64::consts::PI / f64::from(fso)) as f32,
            timing: Timing::new(fso, QUANTUM as u32 * os, 0.002),
            os: (os == 2).then(|| Kaiser::new(2)),
        }
    }

    fn set(s: &mut SkfState, knobs: [f32; 5]) {
        for (i, v) in knobs.into_iter().enumerate() {
            let v = v.clamp(0.0, 1.0);
            s.ramp.tgt[i] = if i == 4 { (v + v) - 1.0 } else { v };
        }
        s.ramp.dirty = true;
    }

    fn update(s: &mut SkfState) {
        let c = s.ramp.cur;
        s.h = [skf_hz(c[0]), skf_hz(c[2])];
        s.dv = (c[4].abs() * c[4]) * KFCFE98;
    }

    #[inline(always)]
    fn core(&self, s: &mut SkfState, x: [f32; 2]) -> [f32; 2] {
        if s.count == 0 {
            s.ramp.advance(4.0);
            Self::update(s);
        }
        s.count = (s.count + 1) & 3;
        let cur = s.ramp.cur;
        let mut out = [0f32; 2];
        for c in 0..2 {
            let mut inp = x[c];
            let m = x[c] * s.dv;
            for j in 0..2 {
                let [s1, s2] = s.z[c][j];
                let (q, r) = (cur[2 * j + 1], cur[2 * j]);
                let f = ((m + s.h[j]).max(K7148B4) * self.w).min(KFCFD88);
                let num = ((KFCFCB4 - f * KFCFCC0) * (f * f)) + f * KFCFD60;
                let den = ((f * KFCFCB8) + 1.0) - ((f * f) * KFCFD0C);
                let g = num / den;
                let gg = g / (g + 1.0);
                let a = s1 - gg * s1;
                let k = (SKF_C238[j] - SKF_C220[j] * r) * (q + q);
                let y = ((((a - (((a - s2) * gg) + s2)) * k) + inp) / (1.0 - ((gg - gg * gg) * k))) * SKF_CLIP[2 * j + 1];
                let n = 1.0 / ((y * y) + 1.0).sqrt();
                let yy = (n * y) * SKF_CLIP[2 * j];
                let v = (yy - s1) * gg;
                let a1 = s1 + v;
                let hp = yy - a1;
                let w = (a1 - s2) * gg;
                let b1 = s2 + w;
                s.z[c][j] = [(a1 + v) + TINY, (b1 + w) + TINY];
                inp = (if j == 0 { hp } else { b1 }) * SKF_C220[2 + j];
            }
            out[c] = inp;
        }
        out
    }
}

// ---- Monark -----------------------------------------------------------------------------------

/// Linear table read: index `trunc(u)` clamped into the table, fraction from the unclamped index.
fn lerp(tab: &[f32], u: f32) -> f32 {
    let n = tab.len() as i32;
    let i = u as i32;
    let at = |k: i32| tab[k.clamp(0, n - 1) as usize];
    (at(i + 1) - at(i)) * (u - i as f32) + at(i)
}

/// `x (x^2 + 27) / (9 x^2 + 27)`.
fn sat3(v: f32) -> f32 {
    let v2 = v * v;
    ((v2 + MFCFE18) * v) / ((v2 * M7149FC) + MFCFE18)
}

/// Note to hertz, the SKF law's quartic summed in Monark's order.
fn monark_hz(n: f32) -> f32 {
    let y = n * M737724;
    let i = y.round_ties_even();
    let f = y - i;
    let f2 = f * f;
    let p = ((((f * MF386A0) + MF3873C) * f2) + (f * MF387FC)) + (((f2 * f2) * MF38654) + 1.0);
    (p * 2f32.powi(i as i32)) * MFCFDF8
}

fn clamp4(v: f32) -> f32 {
    if v > M7149E4 {
        4.0
    } else if v < MF38E04 {
        -4.0
    } else {
        v
    }
}

#[derive(Clone, Copy, Debug)]
struct Monark {
    mode: usize,
    variant: u8,
    factor: usize,
    os: Option<Kaiser>,
    timing: Timing,
    t: f32,
    k23: f32,
    ke0: f32,
    ke4: f32,
    ke8: f32,
    kec: f32,
    kf0: f32,
    kf8: f32,
    k5100: f32,
    k5104: f32,
    k5108: f32,
    k510c: f32,
    hg: f32,
    hk: f32,
}

/// Per-channel Monark history.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct MonarkChannel {
    st: [f32; 12],
    lcg: u32,
    h7: f32,
    h8: f32,
    /// Smoothed targets (k2b, k2e, k2f).
    sm: [f32; 3],
    k28: f32,
    k2c: f32,
    k2d: f32,
    k30: f32,
    k31: f32,
    rs: KaiserState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct MonarkState {
    ramp: Ramp,
    count: usize,
    k27: f32,
    k2a: f32,
    k29: f32,
    ch: [MonarkChannel; 2],
}

const MONARK_KF4: f32 = f32::from_bits(0x3ed7396d);
const MONARK_KFC: f32 = f32::from_bits(0xbf52bbdc);
const MONARK_HR: f32 = f32::from_bits(0x3fb504f3);

impl Monark {
    fn new(mode: u8, variant: u8, rate: f32) -> Self {
        let mode = usize::from(mode.min(3));
        let factor = if rate < M7377CC { 2 + usize::from(rate < MFCFEA8) } else { 1 };
        let fso = factor as f32 * rate;
        let t = (std::f64::consts::PI / f64::from(fso)) as f32;
        let k23 = (((std::f64::consts::PI / f64::from(rate)) as f32 * MF38D04) * 2.0).min(1.0);
        let f = (T368[mode].max(M7148B4) * t).max(0.0);
        let f2 = f * f;
        let f = ((f2 * f2) * (f * MFCFCE0)) + (((f * M760108) * f2) + f);
        let twice = |k: f32| ((t * k) + (t * k)).min(1.0);
        let f13 = (t * MF38778).max(0.0);
        let w = M7149D4 / fso;
        let (w15, w2) = (w * M714A0C, w * w);
        let g = ((((w2 * w2) * w15) * MFCFE9C) + ((w2 * w15) * ME7BD94)) + w15;
        Self {
            mode,
            variant,
            factor,
            os: (factor > 1).then(|| Kaiser::new(factor)),
            timing: Timing::new(fso, QUANTUM as u32 * factor as u32, 0.002),
            t,
            k23,
            ke0: f + f,
            ke4: 1.0 / (f + 1.0),
            ke8: f13 + f13,
            kec: 1.0 / (f13 + 1.0),
            kf0: twice(MFCFE1C),
            kf8: twice(MFCFDD4),
            k5100: twice(MFCFE5C),
            k5104: twice(MFCFE0C),
            k5108: twice(MFCFDE0),
            k510c: (fso * MF385C4).sqrt() * MFCFC84,
            hg: g,
            hk: 1.0 / (((g + MFCFD80) * g) + 1.0),
        }
    }

    fn set(s: &mut MonarkState, knobs: [f32; 5]) {
        for (i, v) in knobs.into_iter().enumerate() {
            let v = v.clamp(0.0, 1.0);
            s.ramp.tgt[i] = [v, v, v * M7149D0 + 1.0, (v + v) - 1.0, v][i];
        }
        s.ramp.dirty = true;
    }

    fn k27(s: &MonarkState) -> f32 {
        let c = s.ramp.cur;
        let t = (MF38804 - c[0] * MFCFD28) * c[3];
        let sign = if t < 0.0 { MFCFEC4 } else { M714940 };
        lerp(&monark_tables()[T720..T720 + 48], t.abs() * MFCFE30) * sign
    }

    fn note(c: [f32; 5], ch: &mut MonarkChannel) {
        let cut = lerp(&monark_tables()[T600..T600 + 48], c[0] * MFCFE30) + ch.k28;
        let lim5 = MFCFE4C - c[1] * MFCFDE4;
        let p2 = c[0] * c[0];
        let p4 = p2 * p2;
        let e = (cut - MF38CC0).max(0.0);
        let cc = if cut <= MFCFE4C { cut.max(0.0) } else { MFCFE4C };
        ch.k2d = cc * MFCFC94;
        let a4 = (cut - ((e * e) * (e * MFCFC88))) + ((((p4 * p4) * M7149F4) + 0.5) * (1.0 - c[1]));
        ch.k2c = if a4 <= lim5 { a4.max(0.0) } else { lim5 };
    }

    fn targets(&self, c: [f32; 5], ch: &mut MonarkChannel) -> [f32; 3] {
        let tabs = monark_tables();
        let u1 = c[1] * M71CB14;
        let i1 = u1 as i32;
        let f23 = u1 - i1 as f32;
        let (r0, r1) = (i1.clamp(0, 20) as usize, (i1 + 1).clamp(0, 20) as usize);
        let u4 = ch.k2c * MFCFD08;
        let i4 = u4 as i32;
        let f26 = u4 - i4 as f32;
        let (q0, q1) = (i4.clamp(0, 47) as usize, (i4 + 1).clamp(0, 47) as usize);
        let base = self.mode * 1008;
        let bil = |tab: usize| {
            let at = |r: usize, q: usize| tabs[tab + base + r * 48 + q];
            let lo = (at(r0, q1) - at(r0, q0)) * f26 + at(r0, q0);
            let hi = (at(r1, q1) - at(r1, q0)) * f26 + at(r1, q0);
            (hi - lo) * f23 + lo
        };
        let (k2b, k2e) = (bil(T7E0), bil(T6E0));
        let fb = &tabs[T6C0..T6C0 + 21];
        let g = (fb[r1] - fb[r0]) * f23 + fb[r0];
        let g = if g <= M70BAF8 { if g >= M7148B4 { g } else { 0.01 } } else { 12.0 };
        let k2f = g * g - 1.0;
        let cl = |v: f32| if v <= 1.0 { if v >= MF38820 { v } else { 0.8 } } else { 1.0 };
        ch.k30 = (cl(k2e) - MF38820) * M7148D0;
        ch.k31 = (c[4] * MFCFCFC) - ((((cl(ch.k2d) - MF38820) * MFCFDD8) * c[4]) * MFCFCFC);
        [k2b, k2e, k2f]
    }

    fn shared(s: &mut MonarkState) {
        let c4 = s.ramp.cur[4];
        s.k2a = c4 * M7149D0;
        s.k29 = ((c4 * c4) * c4) * MF38CD0;
    }

    fn snap(&self, s: &mut MonarkState) {
        s.k27 = Self::k27(s);
        Self::shared(s);
        let c = s.ramp.cur;
        for ch in &mut s.ch {
            ch.k28 = 0.0;
            Self::note(c, ch);
            ch.sm = self.targets(c, ch);
        }
    }

    #[inline(always)]
    fn core(&self, s: &mut MonarkState, x: [f32; 2]) -> [f32; 2] {
        let tick = s.count == 0;
        if tick {
            s.ramp.advance(self.factor as f32);
            s.k27 = Self::k27(s);
            Self::shared(s);
        }
        let c = s.ramp.cur;
        let mut out = [0f32; 2];
        for (k, ch) in s.ch.iter_mut().enumerate() {
            ch.k28 = x[k] * s.k27;
            Self::note(c, ch);
            if tick {
                let t = self.targets(c, ch);
                for i in 0..3 {
                    ch.sm[i] = ((t[i] - ch.sm[i]) * self.k23 + ch.sm[i]) + TINY;
                }
            }
            out[k] = self.voice(c, s.k2a, s.k29, ch, x[k]);
        }
        if tick {
            s.count = self.factor;
        }
        s.count -= 1;
        out
    }

    #[inline(always)]
    fn voice(&self, c: [f32; 5], k2a: f32, k29: f32, ch: &mut MonarkChannel, x: f32) -> f32 {
        let [s24, s25, s26] = ch.sm;
        let st = &mut ch.st;
        let f34 = s25 * MFCFDDC;
        let w = (monark_hz(ch.k2c).max(M7148B4) * self.t).max(0.0);
        ch.lcg = ch.lcg.wrapping_mul(0x41c64e6d).wrapping_add(0x3039);
        let nz = ch.lcg as i32 as f32;
        let w2 = w * w;
        let g = ((w2 * w2) * (w * MFCFCE0)) + (((w * M760108) * w2) + w);
        let gi = 1.0 / (g + 1.0);
        let gg = gi * g;
        let ke4 = self.ke4;
        let f26 = ((ke4 * gg) * gg) * (gg * gg);
        let f29 = 1.0 / ((f26 * f34) + 1.0);
        let f26 = f29 * f26;
        let f23 = T378[self.mode];
        let f31 = (x * MFCFD98) * c[2];
        let mut sum = ((gi * st[0]) * gg) + (gi * st[1]);
        sum = (sum * gg) + (gi * st[2]);
        sum = (sum * gg) + (gi * st[3]);
        let sum = ((sum * ke4) - (ke4 * st[4])) * f29;
        let kec = self.kec;
        let y = ((sum - ((((kec * st[8]) + st[9]) * kec) * f26)) * s24) + ((((kec * kec) * (f23 * f26)) * s24) * f31);
        let d = match self.variant {
            1 => {
                let old = st[6];
                st[6] = (((y - old) * self.kf0) + old) + TINY;
                let v = (((nz * self.k510c) * ch.k30) + (y - old)) * k29;
                let v = if v <= MFCFD4C { if v >= MFCFEC8 { v } else { MFCFEC8 } } else { MFCFD4C };
                let q = ((v - st[7]) * self.kf8) + st[7];
                st[7] = q + TINY;
                let r = (q * MONARK_KF4) + v;
                let r = if MFCFD8C <= r { MFCFD8C } else { r };
                (r - (y * ch.k31)) * MFCFDC4
            }
            2 => {
                let yy = (k2a * MFCFDBC) * y;
                let q = ((yy - st[7]) * self.k5100) + st[7];
                st[7] = q + TINY;
                let v = (q * MONARK_KFC) + yy;
                let v = if v <= MFCFD94 { if v >= MFCFECC { v } else { MFCFECC } } else { MFCFD94 };
                v * k2a
            }
            _ => 0.0,
        };
        let a = (((f31 + d) * f23) - st[8]) * kec;
        st[8] = ((a * self.ke8) + TINY) + st[8];
        let a7 = (a - st[9]) * kec;
        let mut f34 = ((a7 * f26) + sum) * f34;
        st[9] = ((a7 * self.ke8) + TINY) + st[9];
        if f34 < 0.0 {
            let q = (-f34).min(M714A00);
            let q = q * (1.0 - q * M7148C0);
            f34 = ((q + f34) * MF38764) - q;
        }
        let mut prev = sat3(clamp4(a7 - f34));
        let mut ys = [0f32; 4];
        for j in 0..4 {
            let yj = sat3(clamp4(((prev * g) + st[j]) * gi));
            let dd = (prev - yj) * g;
            st[j] = (dd + dd) + (st[j] + TINY);
            ys[j] = yj;
            prev = yj;
        }
        let (ia, ib, wb) = T3B8[self.mode];
        st[4] = ((((ys[3] - st[4]) * ke4) * self.ke0) + TINY) + st[4];
        let o = ((ys[ia] - (wb * ys[ib])) - st[5]) * ke4;
        st[5] = ((o * self.ke0) + TINY) + st[5];
        let q = ((o - st[10]) * self.k5104) + st[10];
        st[10] = q + TINY;
        let r = (((q * s26) + o) * s24) - st[11];
        let o0 = r + MFCFCA8;
        st[11] = ((r * self.k5108) + st[11]) + TINY;
        let gain = T388[self.mode];
        let o0 = clamp4(o0 * gain);
        let q2 = clamp4(gain);
        let z = (((((q2 * q2) * M7149FC) + MFCFE18) * T398[self.mode]) / (((q2 * q2) + MFCFE18) * q2)) * sat3(o0);
        let mut az = z.abs();
        if MFCFD24 < az {
            let t = az - MFCFD24;
            let t = if MFCFD90 <= t { MFCFD90 } else { t };
            az = ((1.0 - t * MFCFD04) * t) + MFCFD24;
        }
        let z = (if z < 0.0 { -az } else { az }) * MF38804;
        let h20 = ch.h8;
        let h = (((z - h20) * self.hg) + ch.h7) * self.hk;
        let n20 = (self.hg * (h + h)) + h20;
        ch.h7 = (h + h) - ch.h7;
        ch.h8 = n20;
        let lp = (h20 + n20) * 0.5;
        (z - h * MONARK_HR) - lp
    }
}

// ---- the engine core ----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Core {
    SimpleLpHp(Dj, Ladder),
    DualSkf(Skf),
    Monark(Monark),
}

/// The prepared core: its kind, the stored knobs (native parameter order) and which knob the
/// engine's cutoff and resonance modulation move.
#[derive(Clone, Copy, Debug)]
pub(crate) struct KontaktFilter {
    core: Core,
    knobs: [f32; 5],
    /// Knobs moved by cutoff and resonance modulation: the record's first two values.
    modulated: [usize; 2],
}

/// The core's history (in the processor's shared unit slot).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum State {
    #[default]
    None,
    SimpleLpHp {
        knobs: [f32; 5],
        dj: DjState,
        ladder: LadderState,
    },
    DualSkf {
        knobs: [f32; 5],
        s: SkfState,
    },
    Monark {
        knobs: [f32; 5],
        s: MonarkState,
    },
}

impl State {
    pub(crate) fn finite(&self) -> bool {
        // ponytail: the shared check covers the audio-path outputs; core histories are
        // f32 and flushed by their own saturators, a fault surfaces in the block output.
        true
    }
}

impl KontaktFilter {
    pub(crate) fn new(fx: KontaktFx, rate: u32) -> Self {
        prepare_table();
        monark_tables();
        let r = rate as f32;
        let (core, knobs, modulated) = match fx {
            KontaktFx::SimpleLpHp { lp, hp } => (Core::SimpleLpHp(Dj::new(r), Ladder::new(r)), [lp, hp, 0.0, 0.0, 0.0], [0, 1]),
            KontaktFx::DualSkf(k) => (Core::DualSkf(Skf::new(r)), k, [2, 0]),
            KontaktFx::Monark { mode, variant, knobs } => (Core::Monark(Monark::new(mode, variant, r)), knobs, [0, 1]),
        };
        Self { core, knobs, modulated }
    }

    fn knobs(&self, modulation: [f64; 4]) -> [f32; 5] {
        let mut k = self.knobs;
        k[self.modulated[0]] = (k[self.modulated[0]] + modulation[0] as f32).clamp(0.0, 1.0);
        k[self.modulated[1]] = (k[self.modulated[1]] + modulation[1] as f32).clamp(0.0, 1.0);
        k
    }

    /// A fresh state with the knobs set and snapped (the native prepare).
    fn start(&self, knobs: [f32; 5]) -> State {
        match &self.core {
            Core::SimpleLpHp(dj, _) => {
                let mut d = DjState::default();
                dj.set(&mut d, knobs[1], 0.0);
                Dj::snap(&mut d);
                let mut l = LadderState::default();
                Ladder::set(&mut l, knobs[0]);
                l.ramp.snap();
                State::SimpleLpHp { knobs, dj: d, ladder: l }
            }
            Core::DualSkf(_) => {
                let mut s = SkfState::default();
                Skf::set(&mut s, knobs);
                s.ramp.snap();
                State::DualSkf { knobs, s }
            }
            Core::Monark(m) => {
                let mut s = MonarkState::default();
                Monark::set(&mut s, knobs);
                s.ramp.snap();
                for ch in &mut s.ch {
                    ch.h8 = T3A8[m.mode];
                }
                m.snap(&mut s);
                State::Monark { knobs, s }
            }
        }
    }

    /// Filter `len` frames from absolute frame `at`. True on a fault.
    pub(crate) fn process(&self, state: &mut ProcessorState, block: &mut Planar, len: usize, at: u64, modulation: [f64; 4]) -> bool {
        let unit = state.color.kontakt_filter();
        let prepared = matches!(
            (&self.core, &*unit),
            (Core::SimpleLpHp(..), State::SimpleLpHp { .. }) | (Core::DualSkf(_), State::DualSkf { .. }) | (Core::Monark(_), State::Monark { .. })
        );
        if !prepared {
            *unit = self.start(self.knobs(modulation));
        }
        let mut i = 0;
        while i < len {
            let t = at + i as u64;
            let run = ((QUANTUM - t % QUANTUM) as usize).min(len - i);
            let quantum = t % QUANTUM == 0;
            let knobs = self.knobs(modulation);
            match (&self.core, &mut *unit) {
                (Core::SimpleLpHp(dj, ladder), State::SimpleLpHp { knobs: k, dj: d, ladder: l }) => {
                    if quantum {
                        if *k != knobs {
                            dj.set(d, knobs[1], 0.0);
                            Ladder::set(l, knobs[0]);
                            *k = knobs;
                        }
                        d.slots.iter_mut().for_each(|q| q.block(dj.timing));
                        l.ramp.block(ladder.timing);
                    }
                    for n in i..i + run {
                        let y = ladder.tick(l, dj.tick(d, [block[0][n] as f32, block[1][n] as f32]));
                        (block[0][n], block[1][n]) = (f64::from(y[0]), f64::from(y[1]));
                    }
                }
                (Core::DualSkf(skf), State::DualSkf { knobs: k, s }) => {
                    if quantum {
                        if *k != knobs {
                            Skf::set(s, knobs);
                            *k = knobs;
                        }
                        s.ramp.block(skf.timing);
                        Skf::update(s);
                    }
                    for n in i..i + run {
                        let x = [block[0][n] as f32, block[1][n] as f32];
                        let y = match &skf.os {
                            None => skf.core(s, x),
                            Some(k) => {
                                let mut up = [[0f32; 2]; 2];
                                for c in 0..2 {
                                    k.up(&mut s.rs[c], x[c], &mut up[c]);
                                }
                                let a = skf.core(s, [up[0][0], up[1][0]]);
                                let b = skf.core(s, [up[0][1], up[1][1]]);
                                [k.down(&mut s.rs[0], &[a[0], b[0]]), k.down(&mut s.rs[1], &[a[1], b[1]])]
                            }
                        };
                        (block[0][n], block[1][n]) = (f64::from(y[0]), f64::from(y[1]));
                    }
                }
                (Core::Monark(m), State::Monark { knobs: k, s }) => {
                    if quantum {
                        if *k != knobs {
                            Monark::set(s, knobs);
                            *k = knobs;
                        }
                        s.ramp.block(m.timing);
                    }
                    for n in i..i + run {
                        let x = [block[0][n] as f32, block[1][n] as f32];
                        let y = match &m.os {
                            None => m.core(s, x),
                            Some(k) => {
                                let f = m.factor;
                                let mut up = [[0f32; super::super::kaiser::MAX]; 2];
                                for c in 0..2 {
                                    let rs = &mut s.ch[c].rs;
                                    k.up(rs, x[c], &mut up[c]);
                                }
                                let mut down = [[0f32; super::super::kaiser::MAX]; 2];
                                for p in 0..f {
                                    let y = m.core(s, [up[0][p], up[1][p]]);
                                    (down[0][p], down[1][p]) = (y[0], y[1]);
                                }
                                [k.down(&mut s.ch[0].rs, &down[0][..f]), k.down(&mut s.ch[1].rs, &down[1][..f])]
                            }
                        };
                        (block[0][n], block[1][n]) = (f64::from(y[0]), f64::from(y[1]));
                    }
                }
                _ => unreachable!("state kind follows the core"),
            }
            i += run;
        }
        !block.iter().all(|c| c[..len].iter().all(|v| v.is_finite()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::BLOCK;

    /// kemu vectors of the native classes (sv/vectors.py, 48 kHz, 32-frame blocks, mono):
    /// an impulse and a +-4 square of period 40, 64 frames each. Bit-exact.
    #[test]
    fn matches_the_native_classes() {
        let file: serde_json::Value = serde_json::from_str(include_str!("kontaktfx_vectors.json")).unwrap();
        let f = |v: &serde_json::Value| v.as_f64().unwrap() as f32;
        for case in file["cases"].as_array().unwrap() {
            let k: Vec<f32> = case["knobs"].as_array().unwrap().iter().map(f).collect();
            let knobs = [k[0], k[1], k[2], k[3], k[4]];
            let fx = match case["class"].as_str().unwrap() {
                "SimpleLPHP" => KontaktFx::SimpleLpHp { lp: k[0], hp: k[1] },
                "DualSKF" => KontaktFx::DualSkf(knobs),
                _ => KontaktFx::Monark { mode: case["mode"].as_u64().unwrap() as u8, variant: case["variant"].as_u64().unwrap() as u8, knobs },
            };
            for input in ["impulse", "square"] {
                let native: Vec<f32> = case[input].as_array().unwrap().iter().map(f).collect();
                let x: Vec<f32> = match input {
                    "impulse" => (0..64).map(|n| if n == 0 { 1.0 } else { 0.0 }).collect(),
                    _ => (0..64).map(|n| if n % 40 < 20 { 4.0 } else { -4.0 }).collect(),
                };
                let unit = KontaktFilter::new(fx, 48_000);
                let mut state = ProcessorState::default();
                let mut out = Vec::new();
                // uneven fragments cross the 32-frame clock
                for (start, end) in [(0, 7), (7, 40), (40, 64)] {
                    let mut b: Planar = [[0.0; BLOCK]; 2];
                    for n in start..end {
                        b[0][n - start] = f64::from(x[n]);
                        b[1][n - start] = f64::from(x[n]);
                    }
                    assert!(!unit.process(&mut state, &mut b, end - start, start as u64, [0.0; 4]));
                    assert_eq!(b[0], b[1]);
                    out.extend(b[0][..end - start].iter().map(|&v| v as f32));
                }
                for (n, (a, b)) in out.iter().zip(&native).enumerate() {
                    assert!(a.to_bits() == b.to_bits() || (a - b).abs() <= 1e-6 * b.abs().max(1.0), "{} {knobs:?} {input} [{n}] {a} vs {b}", case["class"]);
                }
            }
        }
    }
}
