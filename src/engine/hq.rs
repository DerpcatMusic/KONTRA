//! Native ordinary High/Ultra convolution. Source geometry is supplied by its
//! owner; neither quality nor source support changes with available CPU work.

use super::stream::StreamErrorKind;
use crate::audio::Frame;

#[path = "hq_tables.rs"]
mod tables;

#[derive(Clone, Copy, Default, Debug)]
pub(super) struct Point {
    pub floor: u64,
    pub fraction: f32,
    pub step: f64,
    pub resample: bool,
    pub ended: bool,
    pub domain: Option<Domain>,
}

impl Point {
    pub fn validate(self) -> Result<(), StreamErrorKind> {
        if self.floor >= (1u64 << 48) || !self.fraction.is_finite()
            || !(0. ..1.).contains(&self.fraction) || !self.step.is_finite()
            || self.step < 0. || !(self.step as f32).is_finite() {
            return Err(StreamErrorKind::UnrepresentablePitch);
        }
        if self.domain.is_some_and(|domain| domain.start >= domain.end
            || domain.end > i32::MAX as u32) {
            return Err(StreamErrorKind::UnsupportedWindow);
        }
        Ok(())
    }
    pub fn raw(self) -> [u64; 4] {
        let domain = self.domain;
        let flags = u64::from(self.resample) << 32
            | u64::from(domain.is_some_and(|d| d.left)) << 33
            | u64::from(domain.is_some_and(|d| d.right)) << 34
            | u64::from(domain.is_some()) << 35
            | u64::from(self.ended) << 36;
        [self.floor, u64::from(self.fraction.to_bits()) | flags, self.step.to_bits(),
            domain.map_or(0, |d| u64::from(d.start) | u64::from(d.end) << 32)]
    }
    pub fn from_raw(raw: [u64; 4]) -> Self {
        Self { floor:raw[0], fraction:f32::from_bits(raw[1] as u32), step:f64::from_bits(raw[2]),
            resample:raw[1] & (1 << 32) != 0, ended:raw[1] & (1 << 36) != 0,
            domain:(raw[1] & (1 << 35) != 0).then_some(Domain {
                start:raw[3] as u32, end:(raw[3] >> 32) as u32,
                left:raw[1] & (1 << 33) != 0, right:raw[1] & (1 << 34) != 0 }) }
    }

    pub fn window(self, quality: i32) -> Result<(i64, usize), StreamErrorKind> {
        self.validate()?;
        if !self.resample {
            if !matches!(quality, 1 | 2) || !self.step.is_finite()
                || ((self.step as f32) - 1.).abs() > 1e-4 {
                return Err(StreamErrorKind::UnrepresentablePitch);
            }
            return Ok((i64::try_from(self.floor).map_err(|_| StreamErrorKind::UnrepresentablePitch)?, 1));
        }
        let kernel = Kernel::new(quality, self)?;
        let first = i64::try_from(self.floor).ok()
            .and_then(|floor| floor.checked_sub(i64::try_from(kernel.left).ok()?))
            .ok_or(StreamErrorKind::UnrepresentablePitch)?;
        let count = kernel.left.checked_add(kernel.right)
            .ok_or(StreamErrorKind::UnrepresentablePitch)?;
        first.checked_add(i64::try_from(count).map_err(|_| StreamErrorKind::UnrepresentablePitch)?)
            .ok_or(StreamErrorKind::UnrepresentablePitch)?;
        Ok((first, count))
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Domain {
    pub start: u32,
    pub end: u32,
    pub left: bool,
    pub right: bool,
}

impl Domain {
    fn run(self, first: i64, count: usize) -> (i64, usize) {
        let start = i64::from(self.start);
        let end = i64::from(self.end);
        if self.left && first < start || self.right && first >= end {
            let at = start + ((i128::from(first) - i128::from(start)).rem_euclid(i128::from(end - start))) as i64;
            let n = count.min((end - at) as usize);
            return (at, if first < start { n.min((i128::from(start) - i128::from(first)).min(usize::MAX as i128) as usize) } else { n });
        }
        let n = if self.right && first < end { count.min((i128::from(end) - i128::from(first)).min(usize::MAX as i128) as usize) } else { count };
        (first, n)
    }
}

pub(super) fn read(point: Point, first: i64, out: &mut [Frame],
    mut physical: impl FnMut(u64, &mut [Frame]) -> Result<(), StreamErrorKind>,
) -> Result<(), StreamErrorKind> {
    first.checked_add(i64::try_from(out.len()).map_err(|_| StreamErrorKind::UnrepresentablePitch)?)
        .ok_or(StreamErrorKind::UnrepresentablePitch)?;
    let mut done = 0;
    while done < out.len() {
        let from = first + done as i64;
        let (at, mut n) = point.domain.map_or((from, out.len() - done), |domain| domain.run(from, out.len() - done));
        if at < 0 {
            n = n.min(at.unsigned_abs() as usize);
            out[done..done + n].fill([0.; 2]);
        } else {
            physical(at as u64, &mut out[done..done + n])?;
        }
        done += n;
    }
    Ok(())
}

pub(super) fn covered(point: Point, first: i64, count: usize,
    mut physical: impl FnMut(i64, usize) -> bool,
) -> bool {
    let Ok(width) = i64::try_from(count) else { return false; };
    let Some(end) = first.checked_add(width) else { return false; };
    let mut from = first;
    while from < end {
        let remaining = (end - from) as usize;
        if let Some(domain) = point.domain {
            let start = i64::from(domain.start);
            let terminal = i64::from(domain.end);
            let wrapped = if domain.left && from < start { end.min(start) - from }
                else if domain.right && from >= terminal { end - from } else { 0 };
            if wrapped >= terminal - start {
                if !physical(start, (terminal - start) as usize) { return false; }
                from += wrapped;
                continue;
            }
        }
        let (at, n) = point.domain.map_or((from, remaining), |domain| domain.run(from, remaining));
        if !physical(at, n) { return false; }
        from += n as i64;
    }
    true
}

struct Kernel {
    coefficients: &'static [u32],
    scale: f32,
    gain: f32,
    left_origin: f32,
    right_origin: f32,
    left: usize,
    right: usize,
}

impl Kernel {
    fn new(quality: i32, point: Point) -> Result<Self, StreamErrorKind> {
        let step = point.step as f32;
        if !point.step.is_finite() || point.step < 0. || !step.is_finite()
            || !point.fraction.is_finite() || !(0. ..1.).contains(&point.fraction) {
            return Err(StreamErrorKind::UnrepresentablePitch);
        }
        let down = step > 1.;
        let coefficients: &'static [u32] = match (quality, down) {
            (1, false) => &tables::HIGH_UP,
            (1, true) => &tables::HIGH_DOWN,
            (2, false) => &tables::ULTRA_UP,
            (2, true) => &tables::ULTRA_DOWN,
            _ => return Err(StreamErrorKind::UnsupportedWindow),
        };
        let scale = if down { 128. / step } else { 128. };
        let gain = if down { 1. / step } else { 1. };
        let support = (coefficients.len() - 2) as f32;
        let left_origin = point.fraction * scale;
        let right_origin = (1. - point.fraction) * scale;
        let left = generated(left_origin, scale, support)?.checked_sub(1)
            .and_then(|n| n.checked_add(3)).map(|n| n & !3)
            .ok_or(StreamErrorKind::UnrepresentablePitch)?;
        let right = generated(right_origin, scale, support)?.checked_add(4)
            .map(|n| n & !3).ok_or(StreamErrorKind::UnrepresentablePitch)?;
        Ok(Self { coefficients, scale, gain, left_origin, right_origin, left, right })
    }

    fn coefficient(&self, offset: i64) -> Result<f32, StreamErrorKind> {
        let (origin, n) = if offset <= 0 { (self.left_origin, offset.unsigned_abs()) }
            else { (self.right_origin, (offset - 1) as u64) };
        let support = (self.coefficients.len() - 2) as f32;
        let u = distance(origin, self.scale, n, support)?;
        let index = u as usize;
        if index >= self.coefficients.len() - 2 { return Ok(0.); }
        let value = f32::from_bits(self.coefficients[index]);
        let delta = f32::from_bits(self.coefficients[index + 1]) - value;
        Ok((u - index as f32) * delta + value)
    }
}

/// Repeated positive float additions, retaining their rounding. Within a
/// binade the rounded mantissa stride is constant after a tie-even warmup.
/// Crossings use the actual addition. A stalled native table walk cannot
/// represent a finite convolution and is rejected rather than truncated.
fn distance(mut u: f32, scale: f32, mut n: u64, support: f32) -> Result<f32, StreamErrorKind> {
    while n != 0 && u < support {
        let next = u + scale;
        if next <= u { return Err(StreamErrorKind::UnrepresentablePitch); }
        u = next;
        n -= 1;
        if n == 0 || u >= support { break; }
        let next = u + scale;
        if next <= u { return Err(StreamErrorKind::UnrepresentablePitch); }
        if u.to_bits() >> 23 != next.to_bits() >> 23 { continue; }
        u = next;
        n -= 1;
        if n == 0 || u >= support { break; }
        let next = u + scale;
        if next <= u { return Err(StreamErrorKind::UnrepresentablePitch); }
        let bits = u.to_bits();
        if bits >> 23 != next.to_bits() >> 23 { continue; }
        let stride = next.to_bits() - bits;
        let boundary = ((bits >> 23) + 1) << 23;
        let take = n.min(u64::from((boundary - 1 - bits) / stride));
        if take != 0 {
            u = f32::from_bits(bits + (take as u32) * stride);
            n -= take;
        }
    }
    Ok(u)
}

fn generated(origin: f32, scale: f32, support: f32) -> Result<usize, StreamErrorKind> {
    if origin >= support { return Ok(0); }
    let mut hi = 1u64;
    while distance(origin, scale, hi, support)? < support {
        hi = hi.checked_mul(2).ok_or(StreamErrorKind::UnrepresentablePitch)?;
    }
    let mut lo = 0;
    while lo + 1 < hi {
        let mid = lo + (hi - lo) / 2;
        if distance(origin, scale, mid, support)? < support { lo = mid; }
        else { hi = mid; }
    }
    usize::try_from(hi).map_err(|_| StreamErrorKind::UnrepresentablePitch)
}

pub(super) fn sample(
    quality: i32,
    point: Point,
    mut read: impl FnMut(i64, &mut [Frame]) -> Result<(), StreamErrorKind>,
) -> Result<Frame, StreamErrorKind> {
    if point.ended { return Ok([0.; 2]); }
    let (first, count) = point.window(quality)?;
    if !point.resample {
        let mut frame = [[0.; 2]; 1];
        read(first, &mut frame)?;
        return Ok(frame[0]);
    }
    let kernel = Kernel::new(quality, point)?;
    let mut acc = [[0.; 2]; 4];
    let mut frames = [[0.; 2]; 16];
    let mut at = 0;
    while at < count {
        let n = (count - at).min(16);
        read(first + at as i64, &mut frames[..n])?;
        let mut products = [[0.; 2]; 16];
        for i in 0..n {
            let offset = at as i64 + i as i64 - kernel.left as i64;
            let coefficient = kernel.coefficient(offset)?;
            products[i] = frames[i].map(|v| v * coefficient);
        }
        for lane in 0..4 {
            for channel in 0..2 {
                acc[lane][channel] += products[lane][channel];
                if n >= 8 { acc[lane][channel] += products[lane + 4][channel]; }
                if n == 16 {
                    acc[lane][channel] += products[lane + 12][channel] + products[lane + 8][channel];
                } else if n >= 12 { acc[lane][channel] += products[lane + 8][channel]; }
            }
        }
        at += n;
    }
    Ok(std::array::from_fn(|channel|
        ((acc[0][channel] + acc[1][channel]) + (acc[2][channel] + acc[3][channel])) * kernel.gain))
}

#[cfg(all(test, feature = "plugin"))]
#[path = "hq_tests.rs"]
mod tests;
