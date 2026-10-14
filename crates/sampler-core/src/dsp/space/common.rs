//! Stages outside the network engine: early reflections, and the decay
//! measurement the tests share.

use super::network::Ring;

/// Tap times in ms at size 1 of the early reflections, per output channel.
const EARLY_MS: [[f32; 8]; 2] = [
    [7.1, 11.3, 17.9, 23.7, 29.3, 37.1, 43.7, 53.9],
    [8.9, 13.7, 19.1, 26.3, 33.1, 41.3, 47.9, 59.3],
];
/// Alternating-sign taps each 1.4 dB quieter than the last.
const EARLY_DECAY: f32 = 0.851;

/// Eight reflections per channel from the mono input.
pub(super) struct Early {
    line: Ring,
    taps: [[usize; 8]; 2],
    gains: [[f32; 8]; 2],
}

impl Early {
    pub(super) fn new(level: f64, size: f64, predelay: usize, rate: f32) -> Self {
        let scale = size as f32 * 0.001 * rate;
        let taps = EARLY_MS.map(|ch| ch.map(|ms| predelay + (ms * scale) as usize));
        let longest = taps.iter().flatten().copied().max().unwrap_or(0);
        let mut gains = [[0.0; 8]; 2];
        for ch in &mut gains {
            let mut g = level as f32;
            for (k, v) in ch.iter_mut().enumerate() {
                *v = if k % 2 == 0 { g } else { -g };
                g *= EARLY_DECAY;
            }
        }
        Self { line: Ring::new(longest + 1), taps, gains }
    }
    pub(super) fn clear(&mut self) {
        self.line.buf.fill(0.0);
    }
    #[inline(always)]
    pub(super) fn tick(&mut self, pos: usize, mono: f32) -> [f32; 2] {
        self.line.put(pos, mono);
        std::array::from_fn(|ch| {
            self.taps[ch]
                .iter()
                .zip(&self.gains[ch])
                .map(|(&t, &g)| self.line.at(pos, t) * g)
                .sum()
        })
    }
}

/// Time for the Schroeder-integrated energy decay to fall from -5 to -25 dB,
/// extrapolated to 60 dB.
#[cfg(test)]
pub(super) fn rt60(x: &[f32], rate: f32) -> f32 {
    let mut tail = 0.0f64;
    let mut curve: Vec<f64> = x
        .iter()
        .rev()
        .map(|v| {
            tail += f64::from(*v).powi(2);
            tail
        })
        .collect();
    curve.reverse();
    let at = |level: f64| {
        (0..curve.len()).find(|&i| 10.0 * (curve[i] / curve[0]).log10() < level).unwrap() as f32 / rate
    };
    3.0 * (at(-25.0) - at(-5.0))
}
