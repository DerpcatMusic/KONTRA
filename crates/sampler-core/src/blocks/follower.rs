//! Envelope follower: smooths an input level with separate attack and release
//! times. Control-rate; exact for an input held across the step.
//! Kontakt's runtime follower is the different [`AdaptiveFollower`] below
//! (`BEnvFollowA`, producer 0x140ae3de0). The stored follower (0x0A) is decoded
//! (ni-file `EnvelopeFollower`) but its four floats have no proved path into
//! the builder's controls, so the Kontakt mapping reports it unsupported.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Follower {
    pub attack_ms: f64,
    pub release_ms: f64,
    pub gain: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FollowerState {
    level: f32,
}

impl FollowerState {
    /// Advance `frames` at `rate` Hz towards `input` (magnitude taken).
    pub fn advance(&mut self, f: &Follower, input: f32, frames: u32, rate: f64) -> f32 {
        let target = input.abs() * f.gain;
        let ms = if target > self.level { f.attack_ms } else { f.release_ms };
        let tau = ms * 0.001 * rate;
        let coef = if tau <= 0.0 { 1.0 } else { 1.0 - (-f64::from(frames) / tau).exp() };
        self.level += (target - self.level) * coef as f32;
        self.level
    }
}

/// Kontakt's adaptive follower detector (`BEnvFollowA` producer 0x140ae3de0),
/// all binary32 in the native instruction order. Per input sample: the target
/// is `gain * x` clamped to 0..1 with the sign bit cleared (NaN counts as zero;
/// the native compare is not proved); the first state `a` is the larger of the
/// target and `a * first` (a falling target decays it by the factor `first`);
/// the second state `r` is the smaller of `a` and `1 - (1 - r) * second` (its
/// own recovery towards 1). The output cell is `r`. The native names for these
/// roles are not proved, so none are claimed.
/// Output is a 32-cell ring decimated 32:1: a call of `N <= 1024` samples
/// shifts the ring left by `(1024 - N) / 32` cells (when that is below 32)
/// and writes sample `i` into cell `(1024 - N + i) / 32`; slot 6 (0x140ae3d80)
/// hands out the first cells. Cells vacated by the shift keep their old
/// contents. How the caller derives `N` and the rate is not proved here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdaptiveFollower {
    /// `+0xb0`
    pub gain: f32,
    /// `+0xc0`
    pub first: f32,
    /// `+0xbc`
    pub second: f32,
    a: f32,
    r: f32,
    /// `+0xe0`: whether the last sample's second state was clamped to `a`.
    pub flag: bool,
    ring: [f32; Self::CELLS],
}

impl AdaptiveFollower {
    pub const CELLS: usize = 32;
    pub const FRAME: usize = 1024;

    pub fn new(gain: f32, first: f32, second: f32) -> Self {
        Self { gain, first, second, a: 0.0, r: 0.0, flag: false, ring: [0.0; Self::CELLS] }
    }

    /// Builder coefficients (`FUN_140973970` tail, 0x140977a95..0x140977b6a):
    /// from the source controls `t_first` (`+0xa54`, becomes `first`),
    /// `t_second` (`+0xa50`, `second`), `g` (`+0xa58`, gain exponent) and `b`
    /// (`+0xa5c`, decay base exponent) at the object `rate` (`+0x14`).
    /// `base = 2^(b/6)`, `gain = 2^(g/6)`, `n = trunc(rate * t * 0.032)` with
    /// a zero count replaced by one (not a general `max(1, n)`) and each
    /// coefficient is `base^(1/n)` rounded to binary32. Units of the controls
    /// are not proved. `None` for non-finite inputs.
    pub fn from_builder(rate: f32, t_first: f32, t_second: f32, g: f32, b: f32) -> Option<Self> {
        if ![rate, t_first, t_second, g, b].iter().all(|v| v.is_finite()) {
            return None;
        }
        const SIXTH: f32 = 0.166_666_67; // 0x144737728
        let base = 2f32.powf(b * SIXTH);
        let coefficient = |t: f32| {
            let n = (rate * t * 0.032_000_002) as i64 as i32; // 0x144f3868c
            let n = if n == 0 { 1 } else { n };
            f64::from(base).powf(1.0 / f64::from(n)) as f32
        };
        Some(Self::new(2f32.powf(g * SIXTH), coefficient(t_first), coefficient(t_second)))
    }

    /// Process one call; at most [`Self::FRAME`] samples (more is unproved and ignored).
    #[allow(clippy::manual_clamp)]
    pub fn process(&mut self, input: &[f32]) {
        let n = input.len();
        if n > Self::FRAME {
            return;
        }
        let shift = (Self::FRAME - n) / 32;
        if shift > 0 && shift < Self::CELLS {
            self.ring.copy_within(shift.., 0);
        }
        for (i, &x) in input.iter().enumerate() {
            let target = (self.gain * x).max(0.0).min(1.0).abs();
            let rise = if target - self.a < 0.0 { 0.0 } else { 1.0 };
            let a1 = self.a * self.first * (1.0 - rise) + target * rise;
            let k = if target - a1 < 0.0 { 0.0 } else { 1.0 };
            self.a = a1 + (target - a1) * k;
            let h = if self.r - self.a < 0.0 { 0.0 } else { 1.0 };
            let r1 = (1.0 - (1.0 - self.r) * self.second) * (1.0 - h) + self.a * h;
            let j = if r1 - self.a < 0.0 { 0.0 } else { 1.0 };
            self.flag = j != 0.0;
            self.r = r1 - (r1 - self.a) * j;
            self.ring[(Self::FRAME - n + i) >> 5] = self.r;
        }
    }

    /// Slot 6 (0x140ae3d80): the first `out.len()` ring cells.
    pub fn read(&self, out: &mut [f32]) {
        let n = out.len().min(Self::CELLS);
        out[..n].copy_from_slice(&self.ring[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_follower_holds_peaks_and_recovers_below_them() {
        let mut f = AdaptiveFollower::new(1.0, 0.5, 0.9);
        f.process(&[1.0; 32]);
        // The target is met at once; the second state climbs towards it.
        assert_eq!(f.a, 1.0);
        assert!(f.r > 0.0 && f.r < 1.0);
        let mut last = [0.0; 32];
        f.read(&mut last);
        assert_eq!(last[0], 0.0, "32 samples write only the last cell");
        // Input collapse: `a` decays by `first`; `r` never rises past it.
        f.process(&[0.0; 1]);
        assert_eq!(f.a, 0.5);
        assert!(f.r <= f.a);
        for _ in 0..64 {
            f.process(&[0.0; 1]);
        }
        assert!(f.a < 1e-6 && f.r <= f.a);
    }

    #[test]
    fn adaptive_follower_clamps_signs_and_scales_the_target() {
        let mut f = AdaptiveFollower::new(2.0, 0.0, 0.0);
        f.process(&[-0.25]);
        assert_eq!(f.a, 0.0, "a negative product clamps to zero first");
        f.process(&[0.25]);
        assert_eq!(f.a, 0.5);
        f.process(&[4.0]);
        assert_eq!(f.a, 1.0, "clamped to one");
        // A zero second coefficient makes r snap onto a.
        assert_eq!(f.r, 1.0);
        assert!(f.flag);
    }

    #[test]
    fn adaptive_follower_ring_shifts_by_the_unwritten_share_of_the_frame() {
        let mut f = AdaptiveFollower::new(1.0, 0.0, 0.0);
        f.process(&[0.25; 1024]);
        let mut all = [0.0; 32];
        f.read(&mut all);
        assert_eq!(all, [0.25; 32], "a full frame writes every cell");
        // 512 samples: shift by 16 cells, write cells 16..32.
        let mut f = AdaptiveFollower::new(1.0, 0.0, 0.0);
        f.ring = std::array::from_fn(|i| i as f32);
        f.process(&[1.0; 512]);
        let mut all = [0.0; 32];
        f.read(&mut all);
        let moved: Vec<f32> = (16..32).map(|i| i as f32).collect();
        assert_eq!(&all[..16], moved.as_slice());
        assert_eq!(&all[16..], [1.0; 16].as_slice());
        // Over a frame is unproved and ignored.
        let before = f;
        f.process(&[1.0; 2000]);
        assert_eq!(f, before);
    }

    #[test]
    fn builder_coefficients_follow_the_native_count_rule() {
        // base = 2^(-6/6) = 0.5; n = trunc(1000 * 1 * 0.032) = 32.
        let f = AdaptiveFollower::from_builder(1000.0, 1.0, 2.0, 6.0, -6.0).unwrap();
        assert_eq!(f.gain, 2.0);
        assert!((f64::from(f.first) - 0.5f64.powf(1.0 / 32.0)).abs() < 1e-7);
        assert!((f64::from(f.second) - 0.5f64.powf(1.0 / 64.0)).abs() < 1e-7);
        // A zero count becomes one: the coefficient is the base itself.
        let f = AdaptiveFollower::from_builder(1000.0, 0.0, 0.0, 0.0, -6.0).unwrap();
        assert_eq!((f.first, f.second, f.gain), (0.5, 0.5, 1.0));
        assert!(AdaptiveFollower::from_builder(f32::NAN, 1.0, 1.0, 0.0, 0.0).is_none());
    }

    #[test]
    fn rises_with_attack_and_falls_with_release_time_constants() {
        let f = Follower { attack_ms: 10.0, release_ms: 100.0, gain: 1.0 };
        let mut s = FollowerState::default();
        // One attack time constant reaches 1 - 1/e of the step.
        let up = s.advance(&f, -1.0, 480, 48000.0);
        assert!((up - (1.0 - (-1f32).exp())).abs() < 1e-6, "{up}");
        // Splitting the step does not change the result.
        let mut a = FollowerState::default();
        let mut b = FollowerState::default();
        a.advance(&f, 1.0, 480, 48000.0);
        b.advance(&f, 1.0, 240, 48000.0);
        b.advance(&f, 1.0, 240, 48000.0);
        assert!((a.level - b.level).abs() < 1e-6);
        let down = s.advance(&f, 0.0, 4800, 48000.0);
        assert!((down - up * (-1f32).exp()).abs() < 1e-6);
    }

    #[test]
    fn zero_time_follows_instantly_with_gain() {
        let f = Follower { attack_ms: 0.0, release_ms: 0.0, gain: 0.5 };
        let mut s = FollowerState::default();
        assert_eq!(s.advance(&f, 0.8, 32, 48000.0), 0.4);
        assert_eq!(s.advance(&f, 0.0, 32, 48000.0), 0.0);
    }
}
