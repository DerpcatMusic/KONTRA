//! A zone read as a wavetable: whole single cycles of [`CYCLE`] frames, one
//! after another. A voice plays at the note's frequency whatever the
//! sample's rate, from band-limited copies of each cycle; its position
//! morphs between neighbouring cycles, and two forms bend where in the
//! cycle it reads and shape what it read.

/// Frames in one cycle.
pub const CYCLE: u64 = 2048;
/// Cursor frames a voice moves per cycle played: a table zone's rate is
/// `PERIOD` times the frequency at its root, so pitch steps stay small.
pub const PERIOD: f64 = 64.0;

/// Where a voice reads its table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Read {
    /// 0 the first cycle ..= 1 the last; between two, a linear morph.
    pub position: f32,
    /// Where in the cycle a note starts, 0..1.
    pub phase: f32,
    /// Applied in order.
    pub warps: [Warp; 2],
}

/// One form by `amount`, 0..1: a bend of the read phase, then a shaper of
/// the level read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Warp {
    pub shape: Shape,
    pub shaper: Shaper,
    pub amount: f32,
}

impl Warp {
    pub const OFF: Self = Self {
        shape: Shape::Off,
        shaper: Shaper::Off,
        amount: 0.0,
    };
}

/// The model's shapers (`kontra_ir::Shaper` documents each).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shaper {
    Off,
    Edges,
    Window,
    Saturate,
    Fold,
    Blinds(u8),
    Arch,
    Peak,
    Fall,
    Wrap,
}

/// Which way a warp leans as its amount rises.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Minus,
    Plus,
    Both,
}

/// The model's warp shapes (`kontra_ir::WarpShape` documents each).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Off,
    Sync,
    Cosine,
    Arcsine,
    CosineArcsine,
    Pulse,
    Skew { reach: f32, side: Side },
    Flip,
    Mirror,
    Quantize,
    Seesaw,
    Exp,
    Log,
    LogExp,
    Bend { side: Side },
}

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

/// Levels each cycle is kept at, each with fewer harmonics than the last.
pub const LEVELS: usize = 29;
/// The longest period in output frames each level plays (Kontakt 8.13's
/// table at `0x144fd01c0`); level `k` keeps the harmonics up to
/// `TOP[k] / 2`, so none passes the output's Nyquist.
pub const TOP: [f32; LEVELS] = [
    2048.0, 1625.0, 1290.0, 1024.0, 812.0, 645.0, 512.0, 406.0, 322.0, 256.0, 203.0, 161.0, 128.0,
    101.0, 80.0, 64.0, 50.0, 40.0, 32.0, 25.0, 20.0, 16.0, 12.0, 10.0, 8.0, 6.0, 4.0, 3.0, 2.0,
];
/// Frames each level is kept at (`0x144fcffc0`): about four a harmonic.
pub const SIZE: [u32; LEVELS] = [
    4096, 4096, 4096, 2048, 2048, 2048, 1024, 1024, 1024, 512, 512, 512, 256, 256, 256, 128, 128,
    128, 64, 64, 64, 32, 32, 32, 16, 16, 16, 8, 8,
];

/// A zone's cycles, band-limited, and where a voice reads them.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub cycles: usize,
    /// Every cycle's levels.
    pub data: std::sync::Arc<[f32]>,
    /// Where in `data` level `k` of cycle `c` lies, at `c * LEVELS + k`:
    /// its first frame and its length, a power of two.
    pub spans: std::sync::Arc<[(u32, u32)]>,
    pub read: Read,
}

impl Table {
    /// `None` without a cycle, or unless every cycle has its levels.
    #[must_use]
    pub fn new(
        data: std::sync::Arc<[f32]>,
        spans: std::sync::Arc<[(u32, u32)]>,
        read: Read,
    ) -> Option<Self> {
        let cycles = spans.len() / LEVELS;
        let fits = spans.iter().all(|&(first, len)| {
            len.is_power_of_two() && (first as usize).saturating_add(len as usize) <= data.len()
        });
        (cycles > 0 && spans.len() == cycles * LEVELS && fits).then_some(Self {
            cycles,
            data,
            spans,
            read,
        })
    }

    /// Fills `out` from cursor `x` ([`PERIOD`] a cycle since the note
    /// began), moving `step` a frame, as Kontakt reads (`0x140cd55c0`):
    /// Hermite within the two levels about the period, blended, then the
    /// two cycles about `position`; each form's shaper last.
    pub(crate) fn render(&self, (x, step): (f64, f64), out: &mut [[f32; 2]]) {
        let at = f64::from(self.read.position.clamp(0.0, 1.0)) * (self.cycles - 1) as f64;
        let c0 = at as usize;
        let c1 = (c0 + 1).min(self.cycles - 1);
        let morph = at.fract() as f32;
        let [w1, w2] = self.read.warps;
        let (l0, l1, blend) = level(PERIOD / (step * sync(w2)));
        let cycle = |c: usize, q: f32| {
            let a = self.hermite(c * LEVELS + l0, q);
            a + (self.hermite(c * LEVELS + l1, q) - a) * blend
        };
        for (i, frame) in out.iter_mut().enumerate() {
            let cursor = x + i as f64 * step;
            let p = ((cursor / PERIOD) + f64::from(self.read.phase)).fract() as f32;
            let q = warp(w2, warp(w1, p));
            let a = cycle(c0, q);
            let s = if morph == 0.0 {
                a
            } else {
                a + (cycle(c1, q) - a) * morph
            };
            let s = shape(w2, p, shape(w1, p, s));
            *frame = [s, s];
        }
    }

    /// The level at `span` read at phase `q`, wrapping within it.
    fn hermite(&self, span: usize, q: f32) -> f32 {
        let Some(&(first, len)) = self.spans.get(span) else {
            return 0.0;
        };
        let at = len as f32 * q;
        let i = at as u32;
        let tap = |k: u32| {
            let at = first + (i.wrapping_add(k).wrapping_sub(1) & (len - 1));
            self.data.get(at as usize).copied().unwrap_or(0.0)
        };
        crate::hermite(tap(0), tap(1), tap(2), tap(3), at.fract())
    }
}

/// How many cycles a form squeezes into one (`0x140ce8df0`), so its level
/// choice follows; Kontakt counts only the second form.
fn sync(w: Warp) -> f64 {
    match w.shape {
        Shape::Sync | Shape::Pulse => f64::from(w.amount) * 15.0 + 1.0,
        _ => 1.0,
    }
}

/// The two levels about a period of `x` output frames and the blend
/// towards the second (`0x140ce7a50`).
fn level(x: f64) -> (usize, usize, f32) {
    let last = LEVELS - 1;
    if x <= 2.0 {
        return (last, last, 0.0);
    }
    // An unmoving voice (an infinite period) plays the full cycle too.
    if !(x < 2047.0) {
        return (0, 0, 0.0);
    }
    let x = x as f32;
    let hi = TOP.partition_point(|&h| h > x).clamp(1, last);
    let top = |k: usize| TOP.get(k).copied().unwrap_or(2.0);
    (hi - 1, hi, (top(hi - 1) - x) / (top(hi - 1) - top(hi)))
}

/// The level `s` a form's shaper leaves at phase `p` (`0x140ce6eb0`).
#[must_use]
pub fn shape(w: Warp, p: f32, s: f32) -> f32 {
    let a = w.amount;
    let window = |x: f32| {
        let x = if x > 0.5 { 1.0 - x } else { x };
        if x > 0.25 {
            1.0 - (0.5 - x).powi(3) * 32.0
        } else {
            x.powi(3) * 32.0
        }
    };
    match w.shaper {
        Shaper::Off => s,
        Shaper::Edges if p < 0.25 => window(p + p) * s,
        Shaper::Edges if p > 0.75 => window(p + p - 1.0) * s,
        Shaper::Edges => s,
        Shaper::Window => window(p) * s,
        Shaper::Saturate => ((a * a * 8.0 + 1.0) * s).clamp(-1.0, 1.0),
        Shaper::Fold => {
            let t = (a * a * 3.75 + 0.25) * s + 0.25;
            ((t - t.round()).abs() - 0.25) * 4.0
        }
        Shaper::Blinds(n) => {
            let odd = (f32::from(n) * 2.0 * p) as i32 % 2 != 0;
            if odd { (1.0 - a) * s } else { s }
        }
        Shaper::Arch => (p * std::f32::consts::PI).sin() * s,
        Shaper::Peak if p < 0.5 => (p + p) * s,
        Shaper::Peak => (1.0 - (p - 0.5 + p - 0.5)) * s,
        Shaper::Fall => (1.0 - p) * s,
        Shaper::Wrap => {
            let t = if p >= 0.5 { s + a } else { s - a };
            if t < -1.0 {
                t + 2.0
            } else if t > 1.0 {
                t - 2.0
            } else {
                t
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kontakt 8.13's phase-form numbers (its routine `0x140567aa0`) as shapes.
    fn form(kind: u8) -> Option<Shape> {
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
                    shape: form(kind as u8).unwrap(),
                    shaper: Shaper::Off,
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
    fn a_period_picks_the_levels_about_it() {
        assert_eq!(level(f64::INFINITY), (0, 0, 0.0));
        assert_eq!(level(2047.5), (0, 0, 0.0));
        assert_eq!(level(1.5), (LEVELS - 1, LEVELS - 1, 0.0));
        assert_eq!(level(1024.0), (2, 3, 1.0));
        let (lo, hi, blend) = level(183.0);
        assert_eq!((lo, hi), (10, 11));
        assert!((blend - 20.0 / 42.0).abs() < 1e-6);
    }

    #[test]
    fn a_table_without_every_level_is_refused() {
        let read = Read {
            position: 0.0,
            phase: 0.0,
            warps: [Warp::OFF; 2],
        };
        let spans: std::sync::Arc<[_]> = vec![(0, 8); LEVELS].into();
        assert!(Table::new(vec![0.0; 8].into(), spans.clone(), read).is_some());
        assert!(Table::new(vec![0.0; 4].into(), spans, read).is_none());
        assert!(Table::new(vec![0.0; 8].into(), vec![(0, 8); 3].into(), read).is_none());
    }
}
