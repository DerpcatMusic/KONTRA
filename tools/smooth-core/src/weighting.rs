//! Eight-record weighting frontend from clean mathematics. Public tag layouts remain inputs.
use crate::numerics::{exp32, ln32, recip32, scalar_power32, vector_power32};

const EPS: f32 = 1.1920928955078125e-7;
pub const RECORDS: usize = 8;
pub const DEFAULT_CENTERS: [f32; RECORDS] =
    [120.0, 700.0, 3000.0, 9000.0, 60.0, 350.0, 1500.0, 15000.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Shape {
    LowCut,
    LowShelf,
    Bell,
    Bandpass,
    BandReject,
    Tilt,
    HighShelf,
    HighCut,
}
impl Shape {
    pub const NAMES: [&'static str; 8] = [
        "Low cut",
        "Low shelf",
        "Bell",
        "Bandpass",
        "Band reject",
        "Tilt",
        "High shelf",
        "High cut",
    ];
    pub fn from_index(index: usize) -> Self {
        [
            Self::LowCut,
            Self::LowShelf,
            Self::Bell,
            Self::Bandpass,
            Self::BandReject,
            Self::Tilt,
            Self::HighShelf,
            Self::HighCut,
        ][index.min(7)]
    }
}

pub fn center_from_normalized(u: f32) -> f32 {
    let a = 170.0_f64.ln();
    let span = 20150.0_f64.ln() - a;
    let scaled = u.clamp(0.0, 1.0) as f64 * span;
    ((a + scaled).exp() - 150.0) as f32
}
pub fn normalized_from_center(center: f32) -> f32 {
    (((center as f64 + 150.0).ln() - 170.0_f64.ln()) / (20150.0_f64.ln() - 170.0_f64.ln()))
        .clamp(0.0, 1.0) as f32
}
fn q_shape_exponent() -> f64 {
    let a = 0.1_f64.ln();
    let span = 10.0_f64.ln() - a;
    let position = (0.707_f64.ln() - a) / span;
    position.ln() * -1.4426950408889634_f64
}
pub fn q_from_normalized(u: f32) -> f32 {
    let a = 0.1_f64.ln();
    let span = 10.0_f64.ln() - a;
    let position = (u.clamp(0.0, 1.0) as f64)
        .powf(q_shape_exponent())
        .clamp(0.0, 1.0);
    (a + position * span).exp() as f32
}
pub fn normalized_from_q(q: f32) -> f32 {
    let position = ((q as f64).ln() - 0.1_f64.ln()) / (10.0_f64.ln() - 0.1_f64.ln());
    position.clamp(0.0, 1.0).powf(1.0 / q_shape_exponent()) as f32
}
pub fn width_from_q(q: f32) -> f32 {
    if q <= 0.0 {
        return 0.0;
    }
    let doubled = 2.0_f32 * q;
    ((1.0_f64 / doubled as f64).asinh() * 2.8853900817779268_f64) as f32
}
pub fn slope_from_normalized(u: f32) -> f32 {
    let span = 128.0_f32.ln() - 1.7917594909667969_f32;
    let scaled = u.clamp(0.0, 1.0) * span;
    (1.7917594909667969_f32 + scaled).exp()
}
pub fn normalized_from_slope(s: f32) -> f32 {
    ((s.ln() - 1.7917594909667969_f32) / (128.0_f32.ln() - 1.7917594909667969_f32)).clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalizedRecord {
    pub used: f32,
    pub enabled: f32,
    pub frequency: f32,
    pub depth: f32,
    pub q: f32,
    pub slope: f32,
    pub focus: f32,
    pub selector: f32,
    pub shape: f32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandRecord {
    pub used: bool,
    pub enabled: bool,
    pub frequency: f32,
    pub depth: f32,
    pub width: f32,
    pub slope: f32,
    pub focus: f32,
    pub selector: u8,
    pub shape: Shape,
}
impl BandRecord {
    pub fn from_normalized(n: NormalizedRecord) -> Result<Self, &'static str> {
        if [
            n.used,
            n.enabled,
            n.frequency,
            n.depth,
            n.q,
            n.slope,
            n.focus,
            n.selector,
            n.shape,
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return Err("nonfinite normalized band record");
        }
        let u = n.depth.clamp(0.0, 1.0);
        let scaled = 24.0 * u;
        Ok(Self {
            used: n.used.clamp(0.0, 1.0) >= 0.5,
            enabled: n.enabled.clamp(0.0, 1.0) >= 0.5,
            frequency: center_from_normalized(n.frequency),
            depth: scaled - 12.0,
            width: width_from_q(q_from_normalized(n.q)),
            slope: slope_from_normalized(n.slope),
            focus: n.focus.clamp(0.0, 1.0),
            selector: (n.selector.clamp(0.0, 1.0) * 19.0).round() as u8,
            shape: Shape::from_index((n.shape.clamp(0.0, 1.0) * 7.0).round() as usize),
        })
    }
    pub fn flat(frequency: f32) -> Self {
        Self {
            used: true,
            enabled: true,
            frequency,
            depth: 0.0,
            width: width_from_q(0.707),
            slope: 12.0,
            focus: 0.5,
            selector: 0,
            shape: Shape::Bell,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelGroup {
    pub tag: i32,
    pub first: usize,
    pub count: usize,
}

/// Declared experiment layout only: the source public channel-tag layout is not inferred.
pub const LAB_PAIR_GROUP: ChannelGroup = ChannelGroup {
    tag: 0,
    first: 0,
    count: 2,
};
pub fn selector_matches(selector: u8, tag: i32) -> bool {
    let p4 = ![8, 9, 10].contains(&tag);
    let p5 = [16, 17, 18].contains(&tag);
    match selector {
        0 => true,
        1 => tag != 10,
        2 => tag != 9,
        3 => ![9, 10].contains(&tag),
        4 => p4,
        5 => p5,
        6 => p4 && !p5,
        7 => [8, 9].contains(&tag),
        8..=19 => tag == selector as i32,
        _ => false,
    }
}
fn selected_weights(
    record: BandRecord,
    channels: usize,
    groups: &[ChannelGroup],
    stereo: bool,
) -> Result<[Option<f32>; 2], &'static str> {
    if channels == 1 {
        return Ok([Some(1.0), None]);
    }
    let first = (2.0 * (1.0 - record.focus)).clamp(0.0, 1.0);
    let second = (2.0 * record.focus).clamp(0.0, 1.0);
    let mut result = [None; 2];
    let mut occupied = [false; 2];
    for group in groups {
        if group.count == 0 || group.first + group.count > channels {
            return Err("invalid weighting group geometry");
        }
        for i in 0..group.count {
            let channel = group.first + i;
            if occupied[channel] {
                return Err("overlapping weighting groups");
            }
            occupied[channel] = true;
            if !selector_matches(record.selector, group.tag) {
                continue;
            }
            let variants = group.count == 2 || (stereo && ![9, 10].contains(&record.selector));
            if variants && i > 1 {
                return Err("unclosed larger-group focus variant");
            }
            result[channel] = Some(if variants { [first, second][i] } else { 1.0 });
        }
    }
    Ok(result)
}

pub fn gaussian32(f: f32, center: f32, width: f32, amount: f32) -> f32 {
    let width = width as f64 * 0.601591280067049_f64;
    let sigma = width * 0.7071067811865475_f64;
    let kappa = (-0.5_f64 / (sigma * sigma)) as f32;
    let inverse = (1.0_f64 / (center as f64).max(EPS as f64)) as f32;
    let coordinate = (f + EPS) * inverse;
    let logarithm = ln32(coordinate) * 1.4426950216293335_f32;
    let square = logarithm * logarithm;
    let exponent = square * kappa;
    let profile = exp32(exponent);
    let gain = 10.0_f32.powf(amount * 0.05000000074505806_f32).ln();
    exp32(profile * gain)
}
pub fn shelf32(f: f32, center: f32, slope: f32, amount: f32) -> f32 {
    let inverse = (1.0_f64 / (center as f64).max(EPS as f64)) as f32;
    let coordinate = (f + EPS) * inverse;
    let logarithm = ln32(coordinate) * 1.4426950216293335_f32;
    let scaled = -(slope / 6.0);
    let exponent = logarithm * scaled;
    let exponential = exp32(exponent);
    let inverse = recip32(exponential + 1.0);
    let centered = inverse - 0.5;
    let scaled = centered * amount;
    exp32(scaled * 0.115129254758358_f32)
}
pub fn cut32(f: f32, center: f32, slope: f32, high: bool) -> f32 {
    let tiny = 9.9999998245167e-14_f32;
    let coordinate = if high {
        (f + EPS) * (1.0_f32 / center.max(EPS))
    } else {
        f.max(tiny) * (1.0_f32 / center.max(tiny))
    };
    let logarithm = ln32(coordinate) * 1.4426950216293335_f32;
    let slope = slope * 0.16609640419483185_f32;
    let slope = slope
        * if high {
            0.6931471824645996_f32
        } else {
            -0.6931471824645996_f32
        };
    let exponent = logarithm * slope;
    let exponential = exp32(exponent);
    let reference = 10.0_f32.powf(-0.15000000596046448_f32);
    let b = 1.0_f32 / reference - 1.0;
    let scaled = exponential * b;
    let shifted = scaled + 1.0;
    let inverse = recip32(shifted);
    if high {
        inverse.max(0.0)
    } else {
        inverse + 1.1754943508222875e-37_f32
    }
}
pub fn bandpass32(f: f32, center: f32, width: f32, transition: f32, amount: f32) -> f32 {
    let half = width * 0.5;
    let scale = 2.0_f32.powf(half);
    let low = center / scale;
    let high = center * scale;
    let slope = 24.0 / transition;
    let lower = cut32(f, low, slope, false);
    let upper = cut32(f, high, slope, true);
    let reference = cut32(center, low, slope, false) * cut32(center, high, slope, true);
    let inverse = 1.0_f32 / reference;
    let gain = 10.0_f32.powf(amount * 0.05000000074505806_f32) * inverse;
    (lower * upper) * gain
}
fn amount(record: BandRecord, hard: bool, bandpass: bool) -> f32 {
    let mode_scale = if hard { 1.0 } else { 0.5 };
    let scale = mode_scale
        * if record.depth < 0.0 && !bandpass {
            2.0
        } else {
            1.0
        };
    let absolute = record.depth.abs().clamp(0.0, 10.0);
    let squared = 0.10000000149011612_f32 * absolute;
    let root = squared.sqrt(); // Supplied correctly rounded scalar SQRT, not half-power estimate.
    let scaled = root * scale;
    scaled * record.depth
}
pub fn bell_exponent(f: f32) -> f32 {
    let inverse = (1.0_f64 / 150.0) as f32;
    let coordinate = (f + EPS) * inverse;
    let cap = (2.0_f64.powf(1.0 / 4.0) * 150.0) as f32;
    let fourth = scalar_power32(coordinate.min(cap), 4.0);
    let inverse = recip32(fourth + 1.0).max(0.0);
    let half = scalar_power32(inverse, 0.5).min(1.0);
    let scaled = 4.0 * half;
    scaled + 1.0
}
fn ordinary_shape(f: f32, record: BandRecord, hard: bool, exponent: f32) -> f32 {
    let c = record.frequency;
    let d = record.depth;
    let w = record.width;
    let s = record.slope;
    let result = match record.shape {
        Shape::LowCut => cut32(f, c, s, false),
        Shape::HighCut => cut32(f, c, s, true),
        Shape::Bell => vector_power32(
            gaussian32(f, c, w, amount(record, hard, false)).max(0.0),
            exponent,
        ),
        Shape::LowShelf | Shape::HighShelf => {
            let half = d * 0.5;
            let gain = 10.0_f32.powf(half * 0.05000000074505806_f32);
            shelf32(
                f,
                c,
                s,
                if record.shape == Shape::LowShelf {
                    -d
                } else {
                    d
                },
            ) * gain
        }
        Shape::Tilt => shelf32(f, c, s, 2.0 * d),
        Shape::BandReject => {
            let transition = (w * 0.7071067690849304_f32).min(0.25);
            (1.0 - bandpass32(f, c, w, transition, 0.0)).max(0.0)
        }
        Shape::Bandpass => unreachable!(),
    };
    result.max(EPS)
}

pub struct WeightBanks {
    pub wb: [Vec<f32>; 2],
    pub e: [Vec<f32>; 2],
    exponent: Vec<f32>,
    sum: [Vec<f32>; 2],
}
impl WeightBanks {
    pub fn new(frequencies: &[f32]) -> Self {
        let n = frequencies.len();
        Self {
            wb: std::array::from_fn(|_| vec![1.0; n]),
            e: std::array::from_fn(|_| vec![1.0; n]),
            sum: std::array::from_fn(|_| vec![0.0; n]),
            exponent: frequencies.iter().map(|f| bell_exponent(*f)).collect(),
        }
    }
    pub fn build(
        &mut self,
        frequencies: &[f32],
        records: &[BandRecord; 8],
        hard: bool,
        channels: usize,
        groups: &[ChannelGroup],
        stereo: bool,
        curve_scalar: f32,
    ) -> Result<(), &'static str> {
        if !(1..=2).contains(&channels)
            || frequencies.len() != self.exponent.len()
            || frequencies.is_empty()
            || frequencies
                .iter()
                .any(|f| !f.is_finite() || !(0.0..=4294967296.0).contains(f))
            || !curve_scalar.is_finite()
            || curve_scalar < 0.0
        {
            return Err("unsupported weighting geometry");
        }
        for record in records {
            if !record.frequency.is_finite()
                || !(0.5..=1048576.0).contains(&record.frequency)
                || !record.depth.is_finite()
                || !(-12.0..=12.0).contains(&record.depth)
                || !record.width.is_finite()
                || record.width <= 0.0
                || !record.slope.is_finite()
                || record.slope <= 0.0
                || !record.focus.is_finite()
                || !(0.0..=1.0).contains(&record.focus)
                || record.selector > 19
            {
                return Err("unsupported stored weighting record");
            }
            if matches!(record.shape, Shape::Bandpass | Shape::BandReject) {
                let half = record.width * 0.5;
                let scale = 2.0_f32.powf(half);
                let low = record.frequency / scale;
                let high = record.frequency * scale;
                if !(0.5..=1048576.0).contains(&low) || !(0.5..=1048576.0).contains(&high) {
                    return Err("bandpass effective centers are outside the checked shape domain");
                }
            }
            selected_weights(*record, channels, groups, stereo)?;
        }
        for c in 0..2 {
            self.wb[c].fill(1.0);
            self.e[c].fill(1.0);
            self.sum[c].fill(0.0);
        }
        // E, the per-channel spatial curve. It begins at one, and only the
        // positive-depth Bell and Bandpass records take part: each one
        // multiplies a width-dependent Gaussian into the channels it selects.
        //
        //     t = max(powf(W, -0.7) - 1, 0)
        //     a = (t * s) * min(2 * D, 6)
        //     shape = max(Gaussian32(f, C, 2 * W, a), eps)
        //
        // and the focus variants weight it: below 0.01 the variant is one,
        // above 0.99 it is the shape itself, and between them it is the
        // shape raised to the weight.
        for record in records.iter().filter(|r| {
            r.used && r.enabled && matches!(r.shape, Shape::Bell | Shape::Bandpass) && r.depth > 0.0
        }) {
            let t = (record.width.powf(-0.699999988079071_f32) - 1.0).max(0.0);
            let a = t * curve_scalar * (2.0 * record.depth).min(6.0);
            let weights = selected_weights(*record, channels, groups, stereo)?;
            for (k, &frequency) in frequencies.iter().enumerate() {
                let shape = gaussian32(frequency, record.frequency, 2.0 * record.width, a).max(EPS);
                for c in 0..channels {
                    let Some(weight) = weights[c] else {
                        continue;
                    };
                    let value = if weight < 0.01 {
                        1.0
                    } else if weight > 0.99 {
                        shape
                    } else {
                        scalar_power32(shape, weight)
                    };
                    self.e[c][k] *= value;
                }
            }
        }
        let cap = 10.0_f32.powf(3.0);
        for record in records
            .iter()
            .filter(|r| r.used && r.enabled && r.shape != Shape::Bandpass)
        {
            let weights = selected_weights(*record, channels, groups, stereo)?;
            for (k, &frequency) in frequencies.iter().enumerate() {
                let full = ordinary_shape(frequency, *record, hard, self.exponent[k]);
                for c in 0..channels {
                    if let Some(weight) = weights[c] {
                        if (weight as f64) < 0.01 {
                            continue;
                        }
                        let variant = if (weight as f64) > 0.99 {
                            full
                        } else if record.shape == Shape::BandReject {
                            let scaled = full * weight;
                            let complement = 1.0 - weight;
                            scaled + complement
                        } else {
                            scalar_power32(full, weight)
                        };
                        self.wb[c][k] *= variant;
                    }
                }
            }
            for row in self.wb.iter_mut().take(channels) {
                for value in row {
                    *value = (*value).min(cap);
                }
            }
        }
        let mut selected = [false; 2];
        for record in records
            .iter()
            .filter(|r| r.used && r.enabled && r.shape == Shape::Bandpass)
        {
            let weights = selected_weights(*record, channels, groups, stereo)?;
            for c in 0..channels {
                selected[c] |= weights[c].is_some();
            }
            let a = amount(*record, hard, true);
            for (k, &frequency) in frequencies.iter().enumerate() {
                let full = bandpass32(
                    frequency,
                    record.frequency,
                    record.width,
                    record.width.max(0.25),
                    a,
                )
                .max(EPS);
                for c in 0..channels {
                    if let Some(weight) = weights[c] {
                        let contribution = full * weight;
                        self.sum[c][k] += contribution;
                    }
                }
            }
            for row in self.sum.iter_mut().take(channels) {
                for value in row {
                    *value = (*value).min(cap);
                }
            }
        }
        let lo = 10.0_f32.powf(-4.800000190734863_f32);
        let hi = 10.0_f32.powf(2.4000000953674316_f32);
        for c in 0..channels {
            if !selected[c] {
                self.sum[c].fill(1.0);
            }
            for k in 0..frequencies.len() {
                let product = self.wb[c][k] * self.sum[c][k];
                self.wb[c][k] = product.max(lo).min(hi);
            }
        }
        for record in records.iter().filter(|r| {
            r.used
                && r.enabled
                && [Shape::Bell, Shape::Bandpass].contains(&r.shape)
                && r.depth > 0.0
        }) {
            let weights = selected_weights(*record, channels, groups, stereo)?;
            let t = (record.width.powf(-0.699999988079071_f32) - 1.0).max(0.0);
            let depth = (2.0 * record.depth).min(6.0);
            let scalar = t * curve_scalar;
            let a = scalar * depth;
            for (k, &frequency) in frequencies.iter().enumerate() {
                let full = gaussian32(frequency, record.frequency, 2.0 * record.width, a).max(EPS);
                for c in 0..channels {
                    if let Some(weight) = weights[c] {
                        let variant = if (weight as f64) < 0.01 {
                            1.0
                        } else if (weight as f64) > 0.99 {
                            full
                        } else {
                            scalar_power32(full, weight)
                        };
                        self.e[c][k] *= variant;
                    }
                }
            }
        }
        if self
            .wb
            .iter()
            .chain(&self.e)
            .flatten()
            .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err("nonfinite weighting result");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalized_record_maps_and_explicit_group_predicates() {
        assert!((center_from_normalized(0.0) - 20.0).abs() < 2.0e-5);
        assert!((center_from_normalized(1.0) - 20000.0).abs() < 0.002);
        assert!((q_from_normalized(0.5) - 0.707).abs() < 2.0e-7);
        assert_eq!(width_from_q(0.0), 0.0);
        for center in DEFAULT_CENTERS {
            assert!(
                (center_from_normalized(normalized_from_center(center)) - center).abs() < 0.005
            );
        }
        let n = NormalizedRecord {
            used: 0.5,
            enabled: 0.49,
            frequency: 0.5,
            depth: 1.0,
            q: 0.5,
            slope: 0.0,
            focus: 0.25,
            selector: 1.0,
            shape: 1.0,
        };
        let r = BandRecord::from_normalized(n).unwrap();
        assert!(r.used && !r.enabled);
        assert_eq!(r.depth, 12.0);
        assert_eq!(r.selector, 19);
        assert_eq!(r.shape, Shape::HighCut);
        assert!((r.slope - 6.0).abs() < 1.0e-6);
        assert!(!selector_matches(1, 10));
        assert!(!selector_matches(2, 9));
        assert!(selector_matches(5, 17));
        assert!(!selector_matches(6, 17));
        assert!(selector_matches(6, 3));
        assert!(selector_matches(7, 8));
        assert!(!selector_matches(7, 10));
        assert!(selector_matches(19, 19));
    }
    #[test]
    fn eight_shapes_focus_bandpass_selection_and_multiplicative_e() {
        // Finite shape geometry can overflow Exp32 internally, then require the +Inf
        // reciprocal repair even though the final cut/shelf output is finite.
        assert_eq!(cut32(0.0, 1048576.0, 128.0, false).to_bits(), 0x02200000);
        assert_eq!(cut32(4294967296.0, 0.5, 128.0, true).to_bits(), 0);
        let extreme_shelf = shelf32(0.0, 1048576.0, 128.0, 12.0);
        let centered = -0.5_f32 * 12.0;
        assert_eq!(
            extreme_shelf.to_bits(),
            exp32(centered * 0.115129254758358_f32).to_bits()
        );
        let grid = [0.0, 20.0, 200.0, 1000.0, 3000.0, 10000.0, 20000.0];
        let group = [ChannelGroup {
            tag: 9,
            first: 0,
            count: 2,
        }];
        let mut records = [BandRecord::flat(1000.0); 8];
        for r in &mut records {
            r.enabled = false;
        }
        records[0].enabled = true;
        records[0].width = 0.5;
        records[0].depth = 4.0;
        let mut banks = WeightBanks::new(&grid);
        for index in 0..8 {
            records[0].shape = Shape::from_index(index);
            banks
                .build(&grid, &records, false, 2, &group, false, 1.0)
                .unwrap();
            assert!(banks.wb.iter().flatten().all(|v| v.is_finite() && *v > 0.0));
            assert!(banks.wb[0].iter().any(|v| (*v - 1.0).abs() > 0.0001));
        }
        records[0].shape = Shape::Bell;
        records[0].focus = 0.0;
        banks
            .build(&grid, &records, false, 2, &group, false, 1.0)
            .unwrap();
        assert!(banks.wb[1].iter().all(|v| *v == 1.0));
        assert!(banks.e[1].iter().all(|v| *v == 1.0));
        assert!(banks.e[0][3] > 1.0);
        records[0].shape = Shape::Bandpass;
        banks
            .build(&grid, &records, false, 2, &group, false, 1.0)
            .unwrap();
        let floor = 10.0_f32.powf(-4.800000190734863_f32);
        assert!(banks.wb[1].iter().all(|v| v.to_bits() == floor.to_bits())); // Selected zero-focus channel is not unselected.
        records[0].selector = 2;
        banks
            .build(&grid, &records, false, 2, &group, false, 1.0)
            .unwrap();
        assert!(banks.wb.iter().flatten().all(|v| *v == 1.0));
        banks
            .build(&grid, &records, false, 1, &[], false, 1.0)
            .unwrap(); // Mono bypasses selector/focus.
        assert!(banks.e[0][3] > 1.0);
        let invalid = [f32::from_bits(0x4f800001)];
        let mut unsupported = WeightBanks::new(&invalid);
        assert!(
            unsupported
                .build(&invalid, &records, false, 1, &[], false, 1.0)
                .is_err()
        );
    }
}
