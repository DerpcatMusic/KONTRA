//! Uniform working-spectrum time banks and admitted refresh ordering from clean mathematics.
//! Transform inputs and the subsequent spectrum bridge are separate integration contracts.
use crate::numerics::{exp32, recip32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceMode {
    Soft = 0,
    Hard = 1,
}

pub fn precursor_times(mode: SourceMode, detail: f32) -> [f32; 2] {
    let u = detail.clamp(0.0, 1.0);
    let negative = if mode == SourceMode::Hard {
        let span = 50.0_f32.ln() - 2.3025851249694824_f32;
        let scaled = span * u;
        (scaled + 2.3025851249694824_f32).exp()
    } else {
        let scaled = 250.0_f32.ln() * u;
        scaled.exp()
    };
    [negative, negative * 2.0]
}

fn unsigned_float32(value: u64) -> f32 {
    if value < (1_u64 << 63) {
        value as f32
    } else {
        (((value >> 1) | (value & 1)) as f32) * 2.0
    }
}

fn coefficient(time: f32, rate_factor: f32) -> f32 {
    let bounded = time.max(1.1920928955078125e-7_f32);
    let scaled = bounded * rate_factor;
    let inverse = recip32(scaled);
    let logarithm = inverse * -2.3025851249694824_f32;
    let exponential = exp32(logarithm);
    let negative = exponential * -1.0;
    negative + 1.0
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RefreshFlags {
    pub mode_reset: bool,
    pub detail_family: bool,
    pub hop: bool,
    pub response_time_family: bool,
}
impl RefreshFlags {
    fn any(self) -> bool {
        self.mode_reset || self.detail_family || self.hop || self.response_time_family
    }
}

pub struct MPrecursor {
    state: Vec<Vec<f32>>,
    negative: Vec<f32>,
    positive: Vec<f32>,
    owner_rate: f32,
    hop: u64,
    position: i64,
    mode: SourceMode,
    detail: f32,
}
impl MPrecursor {
    pub fn new(
        length: usize,
        channels: usize,
        owner_rate: f32,
        hop: u64,
        mode: SourceMode,
        detail: f32,
    ) -> Result<Self, &'static str> {
        if length == 0
            || channels == 0
            || !owner_rate.is_finite()
            || owner_rate < 44100.0
            || hop == 0
            || hop > i64::MAX as u64
            || !detail.is_finite()
        {
            return Err("unsupported M precursor geometry");
        }
        let mut result = Self {
            state: vec![vec![0.0; length]; channels],
            negative: vec![0.0; length],
            positive: vec![0.0; length],
            owner_rate,
            hop,
            position: 0,
            mode,
            detail,
        };
        result.rebuild(); // Direct constructor fill; no dirty flag required.
        Ok(result)
    }
    fn rebuild(&mut self) {
        let frame_rate = self.owner_rate / unsigned_float32(self.hop);
        let factor = frame_rate * 0.0010000000474974513_f32;
        let [negative, positive] = precursor_times(self.mode, self.detail);
        // Retain per-element numerical routing, rather than computing an ideal pole once.
        for k in 0..self.negative.len() {
            self.negative[k] = coefficient(negative, factor);
            self.positive[k] = coefficient(positive, factor);
        }
    }
    pub fn reset(&mut self) {
        for row in &mut self.state {
            row.fill(0.0);
        }
    }
    pub fn negative_coefficients(&self) -> &[f32] {
        &self.negative
    }
    pub fn positive_coefficients(&self) -> &[f32] {
        &self.positive
    }
    pub fn state(&self, channel: usize) -> &[f32] {
        &self.state[channel]
    }
    pub fn position(&self) -> i64 {
        self.position
    }
    pub fn hop(&self) -> u64 {
        self.hop
    }

    /// Caller supplies observed/exchanged flags. This models arithmetic/order, not host atomics.
    pub fn advance_and_refresh(
        &mut self,
        segment: usize,
        flags: RefreshFlags,
        new_hop: u64,
        mode: SourceMode,
        detail: f32,
    ) -> Result<bool, &'static str> {
        if segment > i64::MAX as usize
            || new_hop == 0
            || new_hop > i64::MAX as u64
            || !detail.is_finite()
        {
            return Err("unsupported precursor refresh input");
        }
        self.position = self
            .position
            .checked_sub(segment as i64)
            .ok_or("precursor position overflow")?;
        if self.position >= 0 {
            return Ok(false);
        }
        // Catch up with OLD hop before reading/applying the provided refresh observation.
        while self.position < 0 {
            self.position = self
                .position
                .checked_add(self.hop as i64)
                .ok_or("precursor position overflow")?;
        }
        if flags.hop {
            self.hop = new_hop;
        }
        if flags.mode_reset {
            self.reset();
        }
        if flags.any() {
            self.mode = mode;
            self.detail = detail;
            self.rebuild();
        }
        Ok(true)
    }
    pub fn update_channel(&mut self, channel: usize, current: &mut [f32]) {
        assert_eq!(current.len(), self.negative.len());
        for k in 0..current.len() {
            let difference = current[k] - self.state[channel][k];
            let positive = difference.max(0.0);
            let negative = difference.min(0.0);
            let release = positive * self.positive[k];
            let first = self.state[channel][k] + release;
            let attack = negative * self.negative[k];
            let updated = first + attack;
            self.state[channel][k] = updated;
            current[k] = updated;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uniform_asymmetric_banks_and_mode_detail_refresh_order() {
        assert_eq!(precursor_times(SourceMode::Soft, 0.0), [1.0, 2.0]);
        assert!((precursor_times(SourceMode::Hard, 0.0)[0] - 10.0).abs() < 2.0e-6);
        let mut p = MPrecursor::new(33, 2, 44100.0, 1024, SourceMode::Soft, 0.5).unwrap();
        assert!(p.state(0).iter().all(|v| *v == 0.0));
        assert!(
            p.negative
                .iter()
                .all(|v| v.to_bits() == p.negative[0].to_bits())
        );
        assert!(p.positive[0] < p.negative[0]);
        let mut input = vec![2.0; 33];
        p.update_channel(0, &mut input);
        assert_eq!(input[0].to_bits(), (2.0 * p.positive[0]).to_bits());
        let preserved = p.state(0)[0];
        assert!(
            p.advance_and_refresh(
                1,
                RefreshFlags {
                    detail_family: true,
                    ..Default::default()
                },
                1024,
                SourceMode::Soft,
                0.8
            )
            .unwrap()
        );
        assert_eq!(p.state(0)[0], preserved);
        assert_eq!(p.position(), 1023);
        let pending = RefreshFlags {
            hop: true,
            mode_reset: true,
            ..Default::default()
        };
        assert!(
            !p.advance_and_refresh(1, pending, 256, SourceMode::Hard, 0.8)
                .unwrap()
        );
        assert_eq!(p.hop(), 1024);
        assert!(
            p.advance_and_refresh(1023, pending, 256, SourceMode::Hard, 0.8)
                .unwrap()
        );
        assert_eq!(p.position(), 1023); // Catch-up used old1024, then installs256.
        assert_eq!(p.hop(), 256);
        assert!(p.state(0).iter().all(|v| *v == 0.0));
    }
}
