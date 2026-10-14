//! UVI DualDelay core (Workstation 4.0.9 kernel `0x14165c960`, setters
//! `0x141237c50..f60`):
//! - blocks of `min(64, ⌊rate·0.0005 - 2⌋)` frames; delay `d = seconds·rate`
//!   (synced: `59.9999962/tempo·beats`, at most 5 s), split by DelayRatio
//!   (`r ≤ 0`: left `(1+r)d`; else right `(1-r)d`), at least `n + 1` frames.
//! - modulation per block: the UVI sine table at `phase + n·⌊2^32·Hz/rate⌋`
//!   (right offset `⌊offset·2^31⌋`), target `d + lfo·cents·0.05·e`,
//!   `e = max(0, min(0.0018280526·rate/Hz, d/10, (d - n)/2))`, glided across
//!   the block by at most `n - 1` frames.
//! - loop: `g_c·(R(rotation)·delayed + Min·input)`, then the high-cut and
//!   low-cut one-poles `exp(-2π·f/rate)` (`0x141641310`/`0x141641420`), into
//!   the ring; output `Mout·delayed`. `W(w) = [[1+w, 1-w], [1-w, 1+w]]/2`,
//!   `Min = R(in_rot·π/4)·W(in_w)`, `Mout = W(out_w)·R(out_rot·π/4)`.
//! DualDelayX adds the loop stages of `dual_x.rs` (gain after the chain, ±4
//! clamp, reflected rotation `0x14165df70`, latency compensation).
//! The peaking filter and the delay smoother's glide after a time change are
//! not reproduced (the mapper reports a set PeakGain).

use super::dual_x::Stages;
use super::{GUARD, Planar, ProcessorState, TimeMod, flush, read};
use sampler_ir::fx::timemod::{DualDelay, Interpolation};
use std::f64::consts::PI;

/// Ring frames: `ceil(rate·5.0182805)` (`0x40a095c1`).
pub(super) fn ring(rate: f64) -> usize {
    (rate * f64::from(f32::from_bits(0x40a0_95c1))).ceil() as usize
}

fn rotation(theta: f64) -> [[f64; 2]; 2] {
    let (s, c) = theta.sin_cos();
    [[c, -s], [s, c]]
}

fn width(w: f64) -> [[f64; 2]; 2] {
    let (a, b) = ((1. + w) * 0.5, (1. - w) * 0.5);
    [[a, b], [b, a]]
}

fn mul(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
    std::array::from_fn(|i| std::array::from_fn(|j| a[i][0] * b[0][j] + a[i][1] * b[1][j]))
}

fn apply(m: [[f64; 2]; 2], v: [f64; 2]) -> [f64; 2] {
    [m[0][0] * v[0] + m[0][1] * v[1], m[1][0] * v[0] + m[1][1] * v[1]]
}

impl TimeMod {
    pub(super) fn dual_delay(&self, state: &mut ProcessorState, slab: &mut [[f64; 2]], block: &mut Planar, len: usize, tempo: f64, d: DualDelay) -> bool {
        let rate = self.rate;
        let cap = ring(rate) + GUARD;
        let kind = self.cfg.line.map_or(Interpolation::Linear, |l| l.interpolation);
        let nmax = ((rate * 0.0005 - 2.).floor() as usize).max(1);
        let n_block = nmax.min(64);
        let tempo = if tempo < 0.1 { 120. } else { tempo };
        // f32, as native.
        let seconds = if d.synced { f32::from_bits(0x426f_ffff) / tempo as f32 * d.time } else { d.time }.min(5.);
        let full = f64::from(seconds * rate as f32);
        let r = f64::from(d.delay_ratio);
        let mut base = if r <= 0. { [(1. + r) * full, full] } else { [full, (1. - r) * full] };
        if let Some(x) = d.x {
            // Latency compensation (on by default).
            if let Some((_, spread)) = x.diffusion {
                base = base.map(|v| (v - f64::from(spread) * 0.001 * rate).max(nmax as f64));
            }
            if x.dispersion.is_some() {
                base = base.map(|v| (v - 64.).max(nmax as f64));
            }
        }
        let base = base.map(|v| v.max(nmax as f64 + 1.).min((cap - 8) as f64));
        let stages = d.x.map(|x| Stages::new(&x, rate));
        let (ring_part, extra) = slab.split_at_mut(cap);
        let slab = ring_part;
        let (f, q) = (f64::from(d.feedback), f64::from(d.feedback_ratio));
        let gain = if q <= 0. { [(1. + q) * f, f] } else { [f, (1. - q) * f] };
        let mut loop_m = rotation(f64::from(d.rotation) * f64::from(f32::from_bits(0x3c8e_fa35)));
        if d.x.is_some_and(|x| x.reflection) {
            // 0x14165df70: the rotation's rows swapped.
            loop_m.swap(0, 1);
        }
        let input = mul(rotation(f64::from(d.input[1]) * PI / 4.), width(f64::from(d.input[0])));
        let output = mul(width(f64::from(d.output[0])), rotation(f64::from(d.output[1]) * PI / 4.));
        let hc = (-2. * PI * f64::from(d.high_cut_hz) / rate).exp();
        let lc = (-2. * PI * f64::from(d.low_cut_hz) / rate).exp();
        let hz = f64::from(d.mod_hz);
        let inc = (4_294_967_296. / rate * hz) as u32;
        let offset = [0u32, (f64::from(d.mod_offset) * 2_147_483_648.) as i64 as u32];
        let depth = f64::from(d.mod_cents) * 0.05;
        // aux: 0-1 modulated delay, 2-3 LFO phase, 4-5 high-cut and 8-9 low-cut states, 14 started.
        if state.aux[14] == 0. {
            (state.aux[0], state.aux[1]) = (base[0], base[1]);
            (state.aux[2], state.aux[3], state.aux[4], state.aux[5], state.aux[8], state.aux[9]) = (0., 0., 0., 0., 0., 0.);
            state.aux[14] = 1.;
        }
        let mut w = state.delay_position as usize % cap;
        let mut finite = true;
        let [left, right] = block;
        let mut start = 0;
        while start < len {
            let n = (len - start).min(n_block);
            let mut glide = [(0., 0.); 2];
            for c in 0..2 {
                let phase = (state.aux[2 + c] as u32).wrapping_add((n as u32).wrapping_mul(inc));
                state.aux[2 + c] = f64::from(phase);
                let lfo = f64::from(crate::blocks::lfo::uvi_sine(phase.wrapping_add(offset[c])));
                let e = (0.001_828_052_6 * rate / hz).min(0.1 * base[c]).min(0.5 * (base[c] - n as f64)).max(0.);
                let target = base[c] + lfo * depth * e;
                let cur = state.aux[c];
                let span = n as f64 - 1.;
                let next = cur + (target - cur).clamp(-span, span);
                glide[c] = (cur, (next - cur) / n as f64);
                state.aux[c] = next;
            }
            for k in start..start + n {
                let x = [left[k], right[k]];
                let i = (k - start) as f64;
                let mut delayed: [f64; 2] = std::array::from_fn(|c| read(slab, cap, w, usize::MAX, glide[c].0 + i * glide[c].1, kind, c));
                if let Some(st) = &stages {
                    for c in 0..2 {
                        delayed[c] = f64::from(st.deemphasis(extra, c, delayed[c] as f32));
                    }
                }
                let fb = apply(loop_m, delayed);
                let drive = stages.as_ref().map_or(1., |st| f64::from(st.drive));
                let inp = apply(input, x.map(|v| v * drive));
                let mut v = if stages.is_some() { [fb[0] + inp[0], fb[1] + inp[1]] } else { [(fb[0] + inp[0]) * gain[0], (fb[1] + inp[1]) * gain[1]] };
                if let Some(st) = &stages {
                    for c in 0..2 {
                        v[c] = f64::from(st.crush(extra, c, v[c] as f32));
                    }
                }
                if d.filters {
                    for c in 0..2 {
                        state.aux[4 + c] += (v[c] - state.aux[4 + c]) * (1. - hc);
                        v[c] = state.aux[4 + c];
                        state.aux[8 + c] += (v[c] - state.aux[8 + c]) * (1. - lc);
                        v[c] -= state.aux[8 + c];
                    }
                }
                if let Some(st) = &stages {
                    for c in 0..2 {
                        v[c] = f64::from(st.tail(extra, c, v[c] as f32).clamp(-4., 4.)) * gain[c];
                    }
                }
                slab[w] = v.map(flush);
                let out = apply(output, delayed);
                left[k] = self.cfg.dry * x[0] + self.cfg.wet * out[0];
                right[k] = self.cfg.dry * x[1] + self.cfg.wet * out[1];
                finite &= out[0].is_finite() && out[1].is_finite();
                w = if w + 1 == cap { 0 } else { w + 1 };
            }
            start += n;
        }
        state.delay_position = w as u32;
        !finite
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{impulse, run};
    use super::*;
    use sampler_ir::fx::timemod::{Config, DelayTime, Lfo, LfoShape, Line, Native, Order, Rate};

    fn unit(d: DualDelay, mix: f64) -> Config {
        let mut line = Line::single(DelayTime::Ms(0.), 5000.);
        line.native = Native::UviDualDelay(d);
        Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: Lfo { shape: LfoShape::Sine, rate: Rate::Hz(0.), phase: 0. }, dry: (1. - mix).sqrt(), wet: mix.sqrt() }
    }

    const PLAIN: DualDelay = DualDelay {
        time: 0.01,
        synced: false,
        delay_ratio: 0.,
        feedback: 0.3,
        feedback_ratio: 0.,
        filters: false,
        low_cut_hz: 20.,
        high_cut_hz: 20_000.,
        rotation: 0.,
        input: [1., 0.],
        output: [1., 0.],
        mod_cents: 0.,
        mod_hz: 1.,
        mod_offset: 1.,
        x: None,
    };

    /// Feedback scales the input too: the first echo is `f·x` and at
    /// Feedback 0 the wet path is silent (`0x14165d550..d74c`).
    #[test]
    fn feedback_scales_the_input() {
        let (out, _) = run(unit(PLAIN, 1.), 48_000, 120., &impulse(1500, [1., 0.]), 64);
        assert!((out[480][0] - 0.3).abs() < 1e-6 && (out[960][0] - 0.09).abs() < 1e-6, "{:?} {:?}", out[480], out[960]);
        let (silent, _) = run(unit(DualDelay { feedback: 0., ..PLAIN }, 1.), 48_000, 120., &impulse(1500, [1., 0.]), 64);
        assert!(silent.iter().flatten().all(|v| *v == 0.));
    }

    /// DelayRatio -0.5 at 0.2 s and 48 kHz: left 4800, right 9600 frames;
    /// a 90° loop rotation sends the left echo to the right (`fbR = dL`).
    #[test]
    fn ratio_and_rotation_route_the_echoes() {
        let d = DualDelay { time: 0.2, delay_ratio: -0.5, feedback: 1., ..PLAIN };
        let (out, _) = run(unit(d, 1.), 48_000, 120., &impulse(20_000, [1., 1.]), 64);
        assert!((out[4800][0] - 1.).abs() < 1e-9 && (out[9600][1] - 1.).abs() < 1e-9);
        let d = DualDelay { rotation: 90., ..d };
        let (out, _) = run(unit(d, 1.), 48_000, 120., &impulse(20_000, [1., 0.]), 64);
        // Left echo at 4800, rotated into the right line: out at 4800 + 9600.
        assert!((out[14_400][1] - 1.).abs() < 1e-6, "{:?}", out[14_400]);
    }

    #[test]
    fn matrices_follow_the_width_law() {
        assert_eq!(width(0.), [[0.5, 0.5], [0.5, 0.5]]);
        assert_eq!(ring(48_000.), 240_878);
    }
}
