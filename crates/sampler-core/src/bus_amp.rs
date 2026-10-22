//! Kontakt 8 instrument bus amplifier (BInsertBus audio, `FUN_140824910`,
//! Ghidra export `140820000.c:3436`): L/R swap, phase invert, the fader with a
//! one-pole ramp, then the balance with a per-sample pan slew.
//!
//! The constants are the binary's own: 1/180 fader ramp (`DAT_144f38638`),
//! 1e-12 bias and 0.999/1.001 ratio window (`DAT_144f385a8`, `DAT_144714934`,
//! `DAT_144f38868`), 10 units/s pan slew (`DAT_144714a00`, doubled into the
//! threshold) and -20 units/s for the falling pan (`DAT_144f38e14`), each divided
//! by the sample rate. Arithmetic is f32 as in the native code.

/// One-pole fader step per sample (`DAT_144f38638`), shared by the FX output stage.
pub(crate) const FADER_RAMP: f32 = 1.0 / 180.0;
const RATIO_BIAS: f32 = 1e-12;
const RATIO_LOW: f32 = 0.999;
const RATIO_HIGH: f32 = 1.001;
const PAN_SLEW_RATE: f32 = 10.0;
const PAN_FALL_RATE: f32 = -20.0;

/// Native gain test (`FUN_14097cc50`): a gain change is ramped when the current and target
/// magnitudes differ by more than 0.1% (ratio outside [0.999, 1.001]).
pub(crate) fn gain_ramps(current: f32, target: f32) -> bool {
    let magnitude = |x: f32| f32::from_bits(x.to_bits() & 0x7fff_ffff);
    let ratio = magnitude(current + RATIO_BIAS) / magnitude(target + RATIO_BIAS);
    ratio > RATIO_HIGH || ratio < RATIO_LOW
}

/// The state of one bus amplifier: the saved targets, and the current fader and
/// pan that ramp toward them. The current values start at the targets on the
/// first block (native first-call flag +0x2e).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BusAmp {
    pub invert: bool,
    pub swap: bool,
    volume: f32,
    pan: f32,
    current_volume: f32,
    current_pan: f32,
    primed: bool,
}

impl BusAmp {
    pub fn new(invert: bool, swap: bool, volume: f32, pan: f32) -> Self {
        Self { invert, swap, volume, pan, current_volume: volume, current_pan: pan, primed: false }
    }

    /// Set the saved fader (linear) target; the output ramps toward it.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
    }

    /// Set the saved pan target (-1..1); the balance slews toward it.
    pub fn set_pan(&mut self, pan: f32) {
        self.pan = pan;
    }

    /// Process one block in place. `channels` is the stereo pair when there
    /// are two channels; a mono bus passes `[x]` and gets the fader only.
    /// Allocation-free: the native pan buffer is evaluated per sample.
    pub fn process(&mut self, sample_rate: f32, channels: &mut [&mut [f32]]) {
        if !self.primed {
            self.current_volume = self.volume;
            self.current_pan = self.pan;
            self.primed = true;
        }
        let stereo = channels.len() == 2;
        if stereo && self.swap {
            let (left, right) = channels.split_at_mut(1);
            for (a, b) in left[0].iter_mut().zip(right[0].iter_mut()) {
                core::mem::swap(a, b);
            }
        }
        if self.invert {
            for channel in channels.iter_mut() {
                for x in channel.iter_mut() {
                    // Sign-bit XOR, as the native `^ 0x80000000` masks.
                    *x = f32::from_bits(x.to_bits() ^ 0x8000_0000);
                }
            }
        }
        self.fader(channels);
        if stereo {
            self.balance(sample_rate, channels);
        }
    }

    /// Native fader: a ramp when the current and target fader differ by more than
    /// 0.1% (ratio outside [0.999, 1.001]), otherwise a constant target gain. The
    /// ramp restarts from the stored value on every channel and stores the final
    /// value of the last channel.
    fn fader(&mut self, channels: &mut [&mut [f32]]) {
        if gain_ramps(self.current_volume, self.volume) {
            let mut gain = self.current_volume;
            for channel in channels.iter_mut() {
                gain = self.current_volume;
                for x in channel.iter_mut() {
                    *x *= gain;
                    gain += (self.volume - gain) * FADER_RAMP;
                }
            }
            self.current_volume = gain;
        } else {
            for channel in channels.iter_mut() {
                for x in channel.iter_mut() {
                    *x *= self.volume;
                }
            }
        }
    }

    /// Native balance with the slew: left `1 - clamp(p, 0, 1)`, right
    /// `clamp(p + 1, 0, 1)`. A pan further than the 2 * 10 / rate step from its
    /// target moves one step per sample; otherwise the target is used directly.
    fn balance(&mut self, sample_rate: f32, channels: &mut [&mut [f32]]) {
        let step_up = PAN_SLEW_RATE / sample_rate;
        let step_up = step_up + step_up;
        let frames = channels[0].len().min(channels[1].len());
        let slewing = step_up < (self.current_pan - self.pan).abs();
        if slewing {
            let mut cur = self.current_pan;
            let falling = self.pan <= self.current_pan;
            let step = if falling { PAN_FALL_RATE / sample_rate } else { step_up };
            let (left, right) = channels.split_at_mut(1);
            let (left, right) = (&mut *left[0], &mut *right[0]);
            for i in 0..frames {
                let p = cur;
                left[i] *= 1.0 - p.clamp(0.0, 1.0);
                right[i] *= (p + 1.0).clamp(0.0, 1.0);
                cur += step;
                if falling {
                    if cur <= self.pan {
                        cur = self.pan;
                    }
                } else if self.pan <= cur {
                    cur = self.pan;
                }
            }
            self.current_pan = cur;
        } else {
            let p = self.pan;
            let (left, right) = channels.split_at_mut(1);
            let (left, right) = (&mut *left[0], &mut *right[0]);
            let l = 1.0 - p.clamp(0.0, 1.0);
            let r = (p + 1.0).clamp(0.0, 1.0);
            for x in left.iter_mut() {
                *x *= l;
            }
            for x in right.iter_mut() {
                *x *= r;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BusAmp;

    fn run(amp: &mut BusAmp, rate: f32, l: &mut [f32], r: &mut [f32]) {
        amp.process(rate, &mut [l, r]);
    }

    /// Swap, then invert, then the balance: [0.25, -0.5] -> [0.5, -0.25] -> pan +0.5 halves left.
    #[test]
    fn swap_and_invert_run_before_the_balance() {
        let mut amp = BusAmp::new(true, true, 1.0, 0.5);
        let (mut l, mut r) = ([0.25f32], [-0.5f32]);
        run(&mut amp, 48_000.0, &mut l, &mut r);
        assert_eq!([l[0], r[0]], [0.25, -0.25]);
    }

    /// A 1 -> 0 fader change ramps by 1/180 per sample from the stored value.
    #[test]
    fn fader_ramp_is_one_pole_from_the_stored_value() {
        let mut amp = BusAmp::new(false, false, 1.0, 0.0);
        let (mut l, mut r) = ([1.0f32; 2], [1.0f32; 2]);
        run(&mut amp, 48_000.0, &mut l, &mut r);
        amp.set_volume(0.0);
        let (mut l, mut r) = ([1.0f32; 2], [1.0f32; 2]);
        run(&mut amp, 48_000.0, &mut l, &mut r);
        let g1 = 1.0 - 1.0 / 180.0;
        assert_eq!(l[0], 1.0);
        assert!((l[1] - g1).abs() < 1e-6, "{}", l[1]);
        assert_eq!(l, r);
    }

    /// Pan 0 -> 1 at 48 kHz moves 20/rate per sample; the stored pan reaches the target.
    #[test]
    fn pan_slews_by_twenty_units_per_second() {
        let rate = 1000.0;
        let mut amp = BusAmp::new(false, false, 1.0, 0.0);
        // The first block primes the current pan from the stored one (native +0x2e).
        let (mut l, mut r) = ([1.0f32; 1], [1.0f32; 1]);
        run(&mut amp, rate, &mut l, &mut r);
        amp.set_pan(1.0);
        let (mut l, mut r) = ([1.0f32; 4], [1.0f32; 4]);
        run(&mut amp, rate, &mut l, &mut r);
        assert_eq!(l[0], 1.0);
        assert!((l[1] - 0.98).abs() < 1e-6, "{l:?}");
        assert!((r[1] - 1.0).abs() < 1e-6, "{r:?}");
        assert!((amp.current_pan - 0.08).abs() < 1e-6);
    }
}
