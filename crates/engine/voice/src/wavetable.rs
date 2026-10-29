//! A zone read as a wavetable: whole single cycles of [`CYCLE`] frames, one
//! after another. A voice plays at the note's frequency whatever the
//! sample's rate; its position morphs between neighbouring cycles, and two
//! phase warps bend where in the cycle it reads.

use kontra_ir::{Side, Warp, WarpShape as Shape, Wavetable};
use kontra_stream::{Mix, Sample, StreamClient};

/// Frames in one cycle.
pub const CYCLE: u64 = 2048;
/// Cursor frames a voice moves per cycle played: a table zone's rate is
/// `PERIOD` times the frequency at its root, so pitch steps stay small.
pub const PERIOD: f64 = 64.0;

/// The phase warp `w` reads at phase `p`, both in 0..1.
#[must_use]
pub fn warp(w: Warp, p: f32) -> f32 {
    let a = w.amount.clamp(0.0, 1.0);
    let lean = |side: Side| match side {
        Side::Minus => -a,
        Side::Plus => a,
        Side::Both => (a - 0.5) * 2.0,
    };
    let arcsine = |p: f32| (2.0 * p - 1.0).asin() * std::f32::consts::FRAC_1_PI + 0.5;
    let cosine = |p: f32| (1.0 - (p * std::f32::consts::PI).cos()) * 0.5;
    match w.shape {
        Shape::Off => p,
        Shape::Sync => ((a * a * a * 15.0 + 1.0) * p) % 1.0,
        Shape::Cosine => (cosine(p) - p) * a + p,
        Shape::Arcsine => (arcsine(p) - p) * a + p,
        Shape::CosineArcsine => {
            let to = if a >= 0.5 { arcsine(p) } else { cosine(p) };
            (2.0 * a - 1.0).abs() * (to - p) + p
        }
        Shape::Pulse => {
            let read = p / (1.0 - a * 0.9375);
            if read < 1.0 { read } else { 0.0 }
        }
        Shape::Skew { reach, side } => {
            let offset = lean(side) * reach;
            if f64::from(p) < 0.5 + f64::from(offset * 0.5) {
                p / (1.0 + offset)
            } else {
                (p - 1.0) / (1.0 - offset) + 1.0
            }
        }
        Shape::Flip => {
            let flip = if a < 0.5 {
                p < a * 2.0
            } else {
                f64::from(p) >= f64::from(a) * 2.0 - 1.0
            };
            if flip { 1.0 - p } else { p }
        }
        Shape::Mirror => {
            // Kontakt's own constants: its upper branch meets the lower
            // a little off the ideal seam.
            let folded = if p > 0.5 { 1.0 - p } else { p };
            let width = a * f32::from_bits(0x3f51_758e) + f32::from_bits(0x3da7_913f);
            if folded < width * 0.5 {
                folded / width
            } else {
                (folded - f32::from_bits(0x3f00_0011)) / (1.0 - width) + 1.0
            }
        }
        Shape::Quantize => {
            let steps = 2f64.powf(f64::from((1.0 - a) * 15.0 + 1.0)) as i64 as f32;
            ((steps * p).round() / steps - 1e-5).max(0.0)
        }
        Shape::Seesaw => match a {
            0.0 => p,
            1.0 => 1.0 - p,
            _ if p < 1.0 - a => p / (1.0 - a),
            _ => (a - (p - (1.0 - a))) / a,
        },
        Shape::Exp => p.powf(a * 9.0 + 1.0),
        Shape::Log => 1.0 - (1.0 - p).powf(a * 9.0 + 1.0),
        Shape::LogExp if a < 0.5 => 1.0 - (1.0 - p).powf((0.5 - a) * 18.0 + 1.0),
        Shape::LogExp => p.powf((a - 0.5) * 18.0 + 1.0),
        Shape::Bend { side } => {
            let bend = lean(side) * 0.9;
            let den = (p - 0.5).abs() * (bend * 4.0) - bend - 1.0;
            (0.5 / den) * ((p + p - 1.0) * (bend - 1.0)) + 0.5
        }
    }
}

/// A zone's cycles and where a voice reads them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Table {
    /// The first cycle's first frame.
    pub first: u64,
    /// Whole cycles from `first`; a trailing part cycle is not read.
    pub cycles: u64,
    pub read: Wavetable,
}

impl Table {
    /// The cycles of `first..end`; `None` under one cycle.
    #[must_use]
    pub fn new(first: u64, end: u64, read: Wavetable) -> Option<Self> {
        let cycles = end.checked_sub(first)? / CYCLE;
        (cycles > 0).then_some(Self {
            first,
            cycles,
            read,
        })
    }

    /// Fills `out` from cursor `x` ([`PERIOD`] a cycle since the note
    /// began), moving `step` a frame: Hermite within each cycle, the two
    /// cycles about `position` blended. Returns the frames not in memory.
    // ponytail: two short reads a frame; read whole cycles into the voice
    // when many wavetable voices cost too much.
    pub(crate) fn render(
        &self,
        sample: &Sample,
        client: &StreamClient,
        mix: Option<&Mix>,
        (x, step): (f64, f64),
        out: &mut [[f32; 2]],
    ) -> u32 {
        let at = f64::from(self.read.position.clamp(0.0, 1.0)) * (self.cycles - 1) as f64;
        let lo = at as u64;
        let hi = (lo + 1).min(self.cycles - 1);
        let blend = at.fract() as f32;
        let mut missing = 0;
        for (i, frame) in out.iter_mut().enumerate() {
            let cursor = x + i as f64 * step;
            let p = ((cursor / PERIOD) + f64::from(self.read.phase)).fract() as f32;
            let [w1, w2] = self.read.warps;
            let read = f64::from(warp(w2, warp(w1, p))) * CYCLE as f64;
            let mut tap = |cycle: u64| {
                let (taps, m) = self.taps(sample, client, mix, cycle, read);
                missing += m;
                taps
            };
            let a = tap(lo);
            *frame = if hi == lo || blend == 0.0 {
                a
            } else {
                let b = tap(hi);
                std::array::from_fn(|c| a[c] + (b[c] - a[c]) * blend)
            };
        }
        missing
    }

    /// Cycle `cycle` at fractional frame `read`, wrapping within the cycle.
    fn taps(
        &self,
        sample: &Sample,
        client: &StreamClient,
        mix: Option<&Mix>,
        cycle: u64,
        read: f64,
    ) -> ([f32; 2], u32) {
        let base = self.first + cycle * CYCLE;
        let start = (read as u64 + CYCLE - 1) % CYCLE;
        let mut q = [[0.0f32; 2]; 4];
        let mut missing = 0;
        if start + 4 <= CYCLE {
            missing += sample.read_mixed(client, base + start, &mut q, mix).missing;
        } else {
            for (k, tap) in q.iter_mut().enumerate() {
                let at = base + (start + k as u64) % CYCLE;
                missing += (sample.read_mixed(client, at, std::slice::from_mut(tap), mix)).missing;
            }
        }
        let t = read.fract() as f32;
        let [y0, y1, y2, y3] = q;
        let frame = std::array::from_fn(|c| {
            crate::hermite(
                y0.get(c).copied().unwrap_or(0.0),
                y1.get(c).copied().unwrap_or(0.0),
                y2.get(c).copied().unwrap_or(0.0),
                y3.get(c).copied().unwrap_or(0.0),
                t,
            )
        });
        (frame, missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kontakt 8.13's phase-form numbers (its routine `0x140567aa0`) as shapes.
    fn shape(kind: u8) -> Option<Shape> {
        let skew = |reach, side| Shape::Skew { reach, side };
        Some(match kind {
            0 => Shape::Off,
            1..=3 | 24..=26 => Shape::Sync,
            4 => Shape::Cosine,
            5 => Shape::Arcsine,
            6 => Shape::CosineArcsine,
            7 => Shape::Pulse,
            8 => skew(0.9, Side::Minus),
            9 => skew(0.9, Side::Plus),
            10 => skew(0.9, Side::Both),
            11 => Shape::Flip,
            12 => Shape::Mirror,
            13 => Shape::Quantize,
            14 => skew(0.98, Side::Minus),
            15 => skew(0.98, Side::Plus),
            16 => skew(0.98, Side::Both),
            17 => Shape::Seesaw,
            28 => Shape::Exp,
            29 => Shape::Log,
            30 => Shape::LogExp,
            31 => Shape::Bend { side: Side::Minus },
            32 => Shape::Bend { side: Side::Plus },
            33 => Shape::Bend { side: Side::Both },
            _ => return None,
        })
    }

    /// Each case (form, phase, amount, Kontakt's answer) was run through the
    /// routine's own instructions on this build (sha256 0fe6356e…); the
    /// second file substitutes libm for its helper calls.
    #[test]
    fn every_warp_reads_where_kontakt_reads() {
        for (file, tolerance) in [
            (include_str!("../tests/warp_vectors.txt"), 2e-7),
            (include_str!("../tests/warp_libm_vectors.txt"), 2e-6),
        ] {
            let mut n = 0;
            for line in file.lines().filter(|l| !l.starts_with('#')) {
                let v: Vec<f32> = line.split(' ').filter_map(|x| x.parse().ok()).collect();
                let [kind, p, a, native] = v[..] else {
                    continue;
                };
                let w = Warp {
                    shape: shape(kind as u8).unwrap(),
                    amount: a,
                };
                let got = warp(w, p);
                assert!(
                    (got - native).abs() <= tolerance,
                    "form {kind} p {p} a {a}: {got} vs {native}"
                );
                n += 1;
            }
            assert!(n > 100, "{n} cases");
        }
    }

    #[test]
    fn a_part_cycle_is_not_read() {
        let read = Wavetable {
            position: 0.0,
            phase: 0.0,
            warps: [Warp::OFF; 2],
        };
        assert_eq!(Table::new(0, 3 * CYCLE + 100, read).map(|t| t.cycles), Some(3));
        assert!(Table::new(0, 100, read).is_none());
    }
}
