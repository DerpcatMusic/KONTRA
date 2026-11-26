//! A stereo pitch-detuning widener: a two-pole crossover splits the low band
//! (passed dry) from the high band, which is pitch-shifted down and up in a
//! Hann-windowed dual-tap cubic delay ring, staggered through stereo output
//! delay lines, spread across channels, and blended with the dry input.

use std::f32::consts::TAU;

/// Settings in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StereoDetuneSettings {
    /// Crossover frequency between dry low band and detuned high band, Hz.
    pub split_hz: f32,
    /// Pitch ratio (`>= 1.0`): left detunes by `1/ratio`, right by `ratio`.
    pub ratio: f32,
    /// Stereo cross-spread, 0..=1.
    pub spread: f32,
    /// Wet/dry blend, 0..=1.
    pub mix: f32,
}

/// Stereo pitch-detuning widener processor. Zero allocations in `set`/`process`.
#[derive(Clone, Debug, PartialEq)]
pub struct StereoDetune {
    rate: f32,
    smooth: f32,
    split_coeff: f32,
    cur_split: f32,
    rates: [f32; 2],
    spread: f32,
    cur_spread: f32,
    mix: f32,
    cur_mix: f32,
    /// Cascaded one-pole crossover state per channel: `[lp1, lp2]`.
    lp: [[f32; 2]; 2],
    /// Main high-band pitch-shift ring and precomputed Hann window.
    grain: Box<[f32]>,
    window: Box<[f32]>,
    grain_write: usize,
    read_pos: [f32; 2],
    /// Stereo output delay rings for spatial decorrelation.
    out_ring: [Box<[f32]>; 2],
    out_write: usize,
    out_lags: [usize; 2],
}

fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 || !x.is_finite() {
        0.0
    } else {
        x
    }
}

impl StereoDetune {
    /// Allocates pitch-shift and output delay rings for `rate`. Not real-time.
    #[must_use]
    pub fn new(s: StereoDetuneSettings, rate: u32) -> Self {
        let rate_f = rate.max(1) as f32;
        let grain_len = ((rate_f * 0.011_609_977).floor() as usize).max(16);
        let window: Box<[f32]> = (0..grain_len)
            .map(|i| 0.5 - 0.5 * (TAU * i as f32 / grain_len as f32).cos())
            .collect();
        let out_len = ((rate_f * 0.04).floor() as usize).max(16);
        let lag_long = ((out_len as f32) * 0.45).floor() as usize;
        let lag_short = lag_long / 2;
        let smooth = 1.0 - (-1.0 / (0.005 * rate_f)).exp();
        let mut unit = Self {
            rate: rate_f,
            smooth,
            split_coeff: 0.0,
            cur_split: 0.0,
            rates: [1.0, 1.0],
            spread: 0.5,
            cur_spread: 0.5,
            mix: 0.5,
            cur_mix: 0.5,
            lp: [[0.0; 2]; 2],
            grain: vec![0.0; grain_len].into_boxed_slice(),
            window,
            grain_write: 0,
            read_pos: [0.0, 0.0],
            out_ring: [
                vec![0.0; out_len].into_boxed_slice(),
                vec![0.0; out_len].into_boxed_slice(),
            ],
            out_write: 0,
            out_lags: [lag_short.max(1), lag_long.max(2)],
        };
        unit.set(s);
        unit.cur_split = unit.split_coeff;
        unit.cur_spread = unit.spread;
        unit.cur_mix = unit.mix;
        unit
    }

    /// Updates parameters without allocating.
    pub fn set(&mut self, s: StereoDetuneSettings) {
        let hz = s.split_hz.clamp(20.0, 0.45 * self.rate);
        self.split_coeff = (TAU * hz / self.rate).clamp(0.0, 0.99);
        let ratio = s.ratio.clamp(1.0, 2.0);
        self.rates = [1.0 / ratio, ratio];
        self.spread = s.spread.clamp(0.0, 1.0);
        self.mix = s.mix.clamp(0.0, 1.0);
    }

    /// Clears all filter and delay state and snaps smoothers.
    pub fn reset(&mut self) {
        self.lp = [[0.0; 2]; 2];
        self.grain.fill(0.0);
        self.grain_write = 0;
        self.read_pos = [0.0, 0.0];
        for r in &mut self.out_ring {
            r.fill(0.0);
        }
        self.out_write = 0;
        self.cur_split = self.split_coeff;
        self.cur_spread = self.spread;
        self.cur_mix = self.mix;
    }

    /// Reads `grain` with 4-point Catmull-Rom cubic interpolation.
    #[inline]
    fn cubic_read(grain: &[f32], pos: f32) -> f32 {
        let len = grain.len();
        if len < 4 {
            return 0.0;
        }
        let wrapped = pos.rem_euclid(len as f32);
        let i1 = wrapped as usize % len;
        let t = wrapped - i1 as f32;
        let i0 = (i1 + len - 1) % len;
        let i2 = (i1 + 1) % len;
        let i3 = (i1 + 2) % len;
        let y0 = grain.get(i0).copied().unwrap_or(0.0);
        let y1 = grain.get(i1).copied().unwrap_or(0.0);
        let y2 = grain.get(i2).copied().unwrap_or(0.0);
        let y3 = grain.get(i3).copied().unwrap_or(0.0);
        let a0 = -0.5 * y0 + 1.5 * y1 - 1.5 * y2 + 0.5 * y3;
        let a1 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let a2 = -0.5 * y0 + 0.5 * y2;
        ((a0 * t + a1) * t + a2) * t + y1
    }

    /// Processes stereo audio in place.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let g_len = self.grain.len();
        let half = g_len / 2;
        let out_len = self.out_ring[0].len();
        if g_len < 4 || out_len < 2 {
            return;
        }
        let smooth = self.smooth;
        let g_len_f = g_len as f32;

        for frame in io {
            self.cur_split += smooth * (self.split_coeff - self.cur_split);
            self.cur_spread += smooth * (self.spread - self.cur_spread);
            self.cur_mix += smooth * (self.mix - self.cur_mix);
            let c = self.cur_split;
            let mix = self.cur_mix;
            // Cross-spread angle: 0 = mono center, 1 = wide stereo.
            let s = self.cur_spread;
            let same = 0.5 * (1.0 + s);
            let cross = 0.5 * (1.0 - s);
            let [in_l, in_r] = *frame;

            // Two-stage one-pole crossover per channel (low stays dry, high is detuned).
            let mut low = [0.0f32; 2];
            let mut high = [0.0f32; 2];
            for ((low_c, high_c), (&x, st)) in low
                .iter_mut()
                .zip(&mut high)
                .zip([in_l, in_r].iter().zip(&mut self.lp))
            {
                let hp = x - st[0] - st[1];
                let lp1 = flush(st[0] + c * hp);
                let lp2 = flush(st[1] + c * lp1);
                st[0] = lp1;
                st[1] = lp2;
                *low_c = lp2;
                *high_c = x - lp2;
            }

            // Write summed high band into the pitch-shift ring.
            let mono_high = 0.5 * (high[0] + high[1]);
            if let Some(slot) = self.grain.get_mut(self.grain_write) {
                *slot = mono_high;
            }

            // Dual Hann-windowed pitch-shifted taps per stereo side.
            let mut shifted = [0.0f32; 2];
            let grain_write = self.grain_write;
            for ((sh, pos), &rate) in shifted.iter_mut().zip(&mut self.read_pos).zip(&self.rates) {
                let pos_a = *pos;
                let pos_b = (pos_a + half as f32).rem_euclid(g_len_f);
                let dist_a = ((grain_write as f32 - pos_a).rem_euclid(g_len_f)) as usize % g_len;
                let dist_b = (dist_a + half) % g_len;
                let wa = self.window.get(dist_a).copied().unwrap_or(0.5);
                let wb = self.window.get(dist_b).copied().unwrap_or(0.5);
                *sh = wa * Self::cubic_read(&self.grain, pos_a)
                    + wb * Self::cubic_read(&self.grain, pos_b);
                *pos = (pos_a + rate).rem_euclid(g_len_f);
            }
            self.grain_write = (grain_write + 1) % g_len;

            // Staggered stereo output delay rings (read before write).
            let mut wet = [0.0f32; 2];
            let out_write = self.out_write;
            for (((w, ring), &lag), &sh) in wet
                .iter_mut()
                .zip(&mut self.out_ring)
                .zip(&self.out_lags)
                .zip(&shifted)
            {
                let lag = lag % out_len;
                let r_idx = (out_write + out_len - lag) % out_len;
                *w = ring.get(r_idx).copied().unwrap_or(0.0);
                if let Some(slot) = ring.get_mut(out_write) {
                    *slot = sh;
                }
            }
            self.out_write = (out_write + 1) % out_len;

            // Recombine dry low band + spread detuned high band, then blend by mix.
            let spread_l = same * wet[0] + cross * wet[1];
            let spread_r = same * wet[1] + cross * wet[0];
            let full_l = low[0] + spread_l;
            let full_r = low[1] + spread_r;
            *frame = [in_l + mix * (full_l - in_l), in_r + mix * (full_r - in_r)];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(split_hz: f32, ratio: f32, spread: f32, mix: f32) -> StereoDetuneSettings {
        StereoDetuneSettings {
            split_hz,
            ratio,
            spread,
            mix,
        }
    }

    #[test]
    fn zero_mix_passes_dry_and_nonzero_mix_widens_mono_highs() {
        let mut unit = StereoDetune::new(settings(200.0, 1.03, 1.0, 0.0), 48_000);
        let mut io: Vec<[f32; 2]> = (0..4096)
            .map(|n| {
                let v = (TAU * 1200.0 * n as f32 / 48_000.0).sin() * 0.5;
                [v, v]
            })
            .collect();
        let dry = io.clone();
        unit.process(&mut io);
        assert_eq!(io, dry, "mix=0 must pass dry identically");

        unit.set(settings(200.0, 1.03, 1.0, 1.0));
        let mut wet = dry.clone();
        unit.process(&mut wet);
        assert!(wet.iter().flatten().all(|v| v.is_finite()));
        // Mono 1200 Hz input above 200 Hz split must produce genuine stereo difference.
        let side_energy: f32 = wet[2048..]
            .iter()
            .map(|[l, r]| (l - r).powi(2))
            .sum::<f32>();
        assert!(
            side_energy > 1.0,
            "expected stereo widening, got side_energy={side_energy}"
        );
    }

    #[test]
    fn silence_stays_silent_and_reset_is_deterministic() {
        let mut unit = StereoDetune::new(settings(300.0, 1.02, 0.8, 0.7), 48_000);
        let mut silence = vec![[0.0; 2]; 2048];
        unit.process(&mut silence);
        assert!(silence.iter().flatten().all(|&v| v == 0.0));

        let input: Vec<[f32; 2]> = (0..2048)
            .map(|n| [(TAU * 880.0 * n as f32 / 48_000.0).sin() * 0.3; 2])
            .collect();
        unit.reset();
        let mut a = input.clone();
        unit.process(&mut a);
        unit.reset();
        let mut b = input;
        for chunk in b.chunks_mut(17) {
            unit.process(chunk);
        }
        assert_eq!(a, b, "block-size invariance after reset");
    }
}
