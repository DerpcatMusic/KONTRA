//! UVI Velvet Delay (`DiffuseDelayNested`; Workstation 4.0.9 process
//! `0x141234d20` → `0x14165abf0`, update `0x14165b730`, kernel `0x14165af50`,
//! crossfade `0x14165b160`, set rebuild `0x14165bd50`), per channel `ch`:
//! - `d = clamp(t, 0.001, 1)` s (synced `59.999996/tempo·t`), `D = (u64)(d·sr)`,
//!   `S = D·Spread`; 16 draws (MT19937 seeded `Seed + ch`, canonical double)
//!   `v = u·(S/8 - 32) + 32`, intervals `I = max(round(S·v/Σv), 1)` (half away),
//!   `I₀ += D - S`, `T = ΣI`.
//! - gains (MT seeded `ch + 1`): `p = (cum_i - (T - Spread·T))/(Spread·T)`,
//!   `g = shape(p)·(u < 0.5 ? 1 : -1)·(-1)^i·Diffusion` (shapes `0x141659ec0`).
//! - taps at `T - (I₀ + … + I_{k-1})`, in order: `v = x[w - pos]`, `x -= g·v`,
//!   `x[w - pos] = x·g + v`; out `x[w - T]`, then write `x` (ring `sr + 10`).
//! - prepare starts from a one-frame set; every block updates, so fades of
//!   `0.1·sr` frames chain forever (an unchanged set too): gains
//!   `t·g + (1-t)·g'` (f64), out `(1-t)·x[w-T'] + t·x[w-T]` (f32), `t = n/len`.
//! - out `Mix·wet + (1-Mix)·dry` (the unit's dry/wet).

use super::mt::Mt;
use super::{Planar, ProcessorState, TimeMod};
use sampler_ir::fx::timemod::UviVelvetDelay;

pub(super) fn ring(rate: f64) -> usize {
    rate as usize + 10
}

pub(super) fn capacity(rate: f64) -> usize {
    ring(rate)
}

fn round_away(x: f64) -> f64 {
    x.round()
}

/// One channel's taps: positions (descending from `T`) and gains.
pub(super) fn design(v: &UviVelvetDelay, ch: u32, rate: f64, tempo: f64) -> (Vec<(usize, f64)>, usize) {
    let t = if v.synced { f32::from_bits(0x426f_ffff) / tempo as f32 * v.time } else { v.time };
    let d = (t.clamp(0.001, 1.0) * rate as f32) as u64 as f64;
    let spread = f64::from(v.spread);
    let s = d * spread;
    let mut mt = Mt::new(v.seed.wrapping_add(ch));
    let draws: Vec<f64> = (0..16).map(|_| mt.unit_f64() * (s * 0.125 - 32.0) + 32.0).collect();
    let k = 1.0 / draws.iter().sum::<f64>();
    let mut iv: Vec<i64> = draws.iter().map(|&x| (round_away(s * x * k) as i64).max(1)).collect();
    iv[0] += (d - s) as i64;
    let total: i64 = iv.iter().sum();
    let tf = total as f64;
    let mut mt = Mt::new(ch + 1);
    let c = f64::from(v.curve);
    let mut cum = 0i64;
    let mut taps = Vec::with_capacity(16);
    for (i, &l) in iv.iter().enumerate() {
        let pos = total - cum;
        cum += l;
        let p = (cum as f64 - (tf - spread * tf)) * (1.0 / (spread * tf));
        let shape = match v.shape {
            0 => p,
            1 => 1.0,
            2 => 10f64.powf((1.0 - p) * -2.5),
            3 => 1.0 - p,
            4 => (1.0 - p) * 4.0 * p,
            5 => 1.0 - (1.0 - p) * 4.0 * p,
            _ if c == 0.0 => p,
            _ => (10.0 * c * p).exp_m1() / (10.0 * c).exp_m1(),
        };
        let sign = if mt.unit_f64() < 0.5 { 1.0 } else { -1.0 };
        let alt = if i % 2 == 0 { 1.0 } else { -1.0 };
        taps.push((pos as usize, shape * sign * alt * f64::from(v.diffusion)));
    }
    (taps, total as usize)
}

/// One frame of the nested-allpass network `0x14165af50` on `ring` (frame
/// `w`, column `ch`), crossfading at `t` from the previous set: `Some(T')`
/// the same taps at total `T'`, `None` the prepare set (no taps, `T' = 1`).
/// `t = 1` is the settled set.
#[allow(clippy::too_many_arguments)]
pub(super) fn nested(ring: &mut [[f64; 2]], ch: usize, w: usize, mut x: f32, taps: &[(usize, f64)], total: usize, t: f32, old: Option<usize>) -> f32 {
    let n = ring.len();
    let at = |pos: usize| (w + n - pos) % n;
    let (td, ud) = (f64::from(t), f64::from(1.0 - t));
    for &(pos, g) in taps {
        let g = td * g + ud * if old.is_some() { g } else { 0.0 };
        let i = at(pos);
        let z = ring[i][ch];
        let y = f64::from(x) - g * z;
        ring[i][ch] = f64::from((y * g + z) as f32);
        x = y as f32;
    }
    let y = (1.0 - t) * ring[at(old.unwrap_or(1))][ch] as f32 + t * ring[at(total)][ch] as f32;
    ring[w][ch] = f64::from(x);
    y
}

impl TimeMod {
    pub(super) fn uvi_velvet_delay(&self, state: &mut ProcessorState, slab: &mut [[f64; 2]], block: &mut Planar, len: usize, tempo: f64, v: UviVelvetDelay) -> bool {
        let n = ring(self.rate);
        let fade = (0.1 * self.rate) as usize;
        let sets = [design(&v, 0, self.rate, tempo), design(&v, 1, self.rate, tempo)];
        // aux: 0 write position, 1 crossfade frame, 3 the prepare set (one
        // frame, no taps) replaced. The update runs every block
        // (`0x14165abf0` forces it) and leaves a rebuild pending, so even an
        // unchanged set crossfades again the moment a fade ends: fades chain
        // every `fade` frames forever.
        let (mut w, mut k) = (state.aux[0] as usize, state.aux[1] as usize);
        let mut first = state.aux[3] == 0.0;
        let mut finite = true;
        for f in 0..len {
            let t = k as f32 / fade as f32;
            for (ch, (taps, total)) in sets.iter().enumerate() {
                let dry = block[ch][f];
                let y = nested(&mut slab[..n], ch, w, dry as f32, taps, *total, t, (!first).then_some(*total));
                block[ch][f] = f64::from(y) * self.cfg.wet + self.cfg.dry * dry;
                finite &= y.is_finite();
            }
            w = (w + 1) % n;
            k += 1;
            if k == fade {
                (k, first) = (0, false);
            }
        }
        (state.aux[0], state.aux[1], state.aux[3]) = (w as f64, k as f64, if first { 0.0 } else { 1.0 });
        !finite
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_ir::fx::timemod::{Config, DelayTime, Lfo, LfoShape, Line, Native, Order, Rate};

    fn velvet(time: f32, spread: f32, diffusion: f32, seed: u32, shape: u8, curve: f32) -> UviVelvetDelay {
        UviVelvetDelay { time, synced: false, spread, diffusion, seed, shape, curve }
    }

    /// Update `0x14165b730` at 48 kHz, channel 0.
    #[test]
    fn design_matches_native_vectors() {
        let (taps, total) = design(&velvet(0.05, 1.0, 0.1, 0, 0, 0.0), 0, 48_000.0, 120.0);
        let iv: Vec<usize> = taps.windows(2).map(|p| p[0].0 - p[1].0).chain([taps[15].0]).collect();
        assert_eq!(iv, [160, 217, 220, 217, 167, 113, 94, 40, 88, 134, 210, 135, 115, 215, 103, 173]);
        assert_eq!(total, 2401);
        assert_eq!((taps[0].1, taps[15].1), (-0.006663890145113944, 0.10000000149011612));
        let (taps, _) = design(&velvet(0.2, 0.5, 0.5, 7, 6, 0.5), 0, 48_000.0, 120.0);
        assert_eq!([taps[0].0 - taps[1].0, taps[1].0 - taps[2].0, taps[2].0 - taps[3].0], [4967, 221, 610]);
        assert_eq!(taps[15].1.abs(), 0.5);
        let (taps, _) = design(&velvet(0.01, 0.3, 0.9, 3, 2, 0.0), 0, 48_000.0, 120.0);
        let iv: Vec<usize> = taps.windows(2).map(|p| p[0].0 - p[1].0).chain([taps[15].0]).collect();
        assert_eq!(iv, [345, 7, 10, 8, 9, 11, 10, 10, 10, 7, 10, 9, 10, 9, 8, 6]);
    }

    /// Kernel `0x14165af50` after the prepare crossfade: an impulse on L.
    #[test]
    fn steady_impulse_matches_native_bits() {
        let mut line = Line::single(DelayTime::Ms(0.), 1000.);
        line.native = Native::UviVelvetDelay(velvet(0.05, 1.0, 0.1, 0, 0, 0.0));
        let cfg = Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: Lfo { shape: LfoShape::Sine, rate: Rate::Hz(0.), phase: 0. }, dry: 0., wet: 1. };
        let at = 6144; // 1344 frames into the second crossfade
        let mut x = vec![[0.0; 2]; at + 400];
        x[at][0] = 1.0;
        let (y, _) = super::super::tests::run(cfg, 48_000, 120., &x, 64);
        let bits = |k: usize| (y[at + k][0] as f32).to_bits();
        assert_eq!([bits(0), bits(160), bits(377)], [0xbbda5cc5, 0x3c809f95, 0x3ccba1c2]);
    }
}
