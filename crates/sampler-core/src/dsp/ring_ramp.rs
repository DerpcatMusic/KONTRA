//! Four-lane ramp of the Kontakt Ring Modulator sub-object at +0x1f0 (native control 0x140acb6c0).
//!
//! The state is the native float layout, so each offset below is the byte offset in the sub-object:
//! lanes `cur` at 0x0..0x10, per-control `step` at 0x10..0x20, `target` at 0x20..0x30, the
//! state byte `busy` at 0x30, the active byte `live` at 0x31, the countdown at 0x34 (a float that
//! holds an integer), the reload at 0x38 and the rate at 0x3c. The state code at 0x40 is a float
//! whose bit pattern is 2 (ramping), 3 (settling) or 4 (idle), as in the native constants.
//!
//! Each call is one control step. A new target arms the ramp (`busy` set); the countdown then
//! runs to zero, at which point the lanes snap to the target. Lanes whose step squares below
//! 1e-15 are snapped at once.

/// Native state codes (0x140acb6c0 stores these bit patterns: 2.8026e-45, 4.2039e-45, 5.60519e-45).
const RAMPING: u32 = 2;
const SETTLING: u32 = 3;
const IDLE: u32 = 4;
/// 0x144fc3270: the snap threshold on a lane's squared step (a double in the binary).
const SNAP: f64 = 1e-15;

/// Ramp state of one sub-object, as the native byte image.
pub(crate) struct Ramp4 {
    m: [u8; 0x44],
}

impl Default for Ramp4 {
    fn default() -> Self {
        Self { m: [0; 0x44] }
    }
}

impl Ramp4 {
    fn f(&self, o: usize) -> f32 {
        f32::from_le_bytes([self.m[o], self.m[o + 1], self.m[o + 2], self.m[o + 3]])
    }

    fn wf(&mut self, o: usize, v: f32) {
        self.m[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// The four lanes of the current value.
    pub(crate) fn lanes(&self) -> [f32; 4] {
        std::array::from_fn(|l| self.f(4 * l))
    }

    /// Sets the lanes at once (a native apply writes current = target, with no ramp).
    pub(crate) fn snap(&mut self, lanes: [f32; 4]) {
        for (l, v) in lanes.iter().enumerate() {
            self.wf(4 * l, *v);
        }
    }

    /// Per-frame lane step: `cur += step` at the start of every frame of a block (Stereo Tune core).
    pub(crate) fn advance(&mut self) {
        for l in 0..4 {
            let v = self.f(4 * l) + self.f(0x10 + 4 * l);
            self.wf(4 * l, v);
        }
    }

    /// Sets the target lanes and the ramp rate, and arms the ramp (the caller owns the timing).
    pub(crate) fn set_target(&mut self, target: [f32; 4], rate: f32, reload: f32) {
        for (l, t) in target.iter().enumerate() {
            self.wf(0x20 + 4 * l, *t);
        }
        self.wf(0x3c, rate);
        self.wf(0x38, reload);
        self.m[0x30] = 1;
    }

    /// One control step (native 0x140acb6c0).
    pub(crate) fn control(&mut self) {
        if self.m[0x30] == 0 {
            // Countdown: an armed ramp finishes here.
            let c = self.f(0x34) as i32 - 1;
            self.wf(0x34, c as f32);
            if self.f(0x34) != 0.0 {
                let code = self.u_code();
                if code == IDLE {
                    self.wu_code(0);
                } else if code == RAMPING {
                    self.wu_code(SETTLING);
                }
                return;
            }
            // Land on the target: current <- target, steps cleared, ramp over.
            for l in 0..4 {
                let t = self.f(0x20 + 4 * l);
                self.wf(4 * l, t);
                self.wf(0x10 + 4 * l, 0.0);
            }
            self.m[0x31] = 0;
        } else {
            // A new target: compute the per-lane step and snap lanes that are already there.
            let rate = self.f(0x3c);
            self.m[0x30] = 0;
            self.wu_code(RAMPING);
            let mut step = [0f32; 4];
            for l in 0..4 {
                step[l] = (self.f(0x20 + 4 * l) - self.f(4 * l)) * rate;
                self.wf(0x10 + 4 * l, step[l]);
            }
            for l in 0..4 {
                let sq = step[l] * step[l];
                if f64::from(sq) < SNAP {
                    self.wf(0x10 + 4 * l, 0.0);
                    step[l] = 0.0;
                    let t = self.f(0x20 + 4 * l);
                    self.wf(4 * l, t);
                }
            }
            let sum = step[3] * step[3] + step[0] * step[0] + step[1] * step[1] + step[2] * step[2];
            if 0.0 < sum {
                let r = self.f(0x38);
                self.wf(0x34, r);
                self.m[0x31] = 1;
                return;
            }
        }
        self.wu_code(IDLE);
    }

    fn u_code(&self) -> u32 {
        u32::from_le_bytes([self.m[0x40], self.m[0x41], self.m[0x42], self.m[0x43]])
    }

    fn wu_code(&mut self, v: u32) {
        self.m[0x40..0x44].copy_from_slice(&v.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ramp lands exactly on its target, goes idle, and clears the live flag. Snapping a
    /// lane whose step is below the threshold is exact.
    #[test]
    fn ramp_lands_on_target_and_goes_idle() {
        let mut r = Ramp4::default();
        r.set_target([1.0, 2.0, 3.0, 4.0], 0.25, 4.0);
        r.control();
        assert_eq!(r.m[0x31], 1);
        let mut calls = 1;
        while r.m[0x31] != 0 && calls < 64 {
            r.control();
            calls += 1;
        }
        assert_eq!(r.lanes(), [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(r.m[0x31], 0);
        assert_eq!(r.u_code(), IDLE);
    }
}
