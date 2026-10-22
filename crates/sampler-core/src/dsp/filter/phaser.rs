//! Kontakt 8 Phaser filter (saved type 13) with live cutoff and resonance lanes.
//!
//! The native object (BFilterPhaser, vtable `0x144f0fa38`) runs the control clock shared with the
//! Vowel, Formant and BFilter LP wrappers (`0x140ae9f90`) over four first-order allpass stages
//! (kernel slot 33, `0x140909810`). The coefficient and feedback come from the stepped cutoff and
//! resonance (coefficient slot 36, `0x14095d770`). The clock rules are those of
//! `tools/re/phaser_kemu_vectors.py`, which drives the native object under kemu; the vectors are
//! `phaser_kemu_vectors.json`. The static saved-value path is `timemod::Config::phaser_filter`.

/// Frames per clock segment (the wrapper's countdown reload, `0x1bc` = `0x20`).
const QUANTUM: usize = 32;
/// Native "moving" test of the clock, `1e-4` on either lane.
const MOVING: f32 = 1e-4;
/// Native change threshold of the coefficient setter (`DAT_1447148f0`, 1e-6).
const CHANGE: f64 = 1e-6;
/// Taylor threshold of the coefficient law (`DAT_1447148a0`, `f32` of 1e-5).
const SERIES_MIN: f32 = f32::from_bits(0x3727_c5ac);
/// Native Taylor and ln2 constants at `obj+0x410..0x430` (constructor `0x1407c9a70`).
const HALF: f32 = f32::from_bits(0x3f00_0000);
const C6: f32 = f32::from_bits(0x3e2a_aaab);
const C24: f32 = f32::from_bits(0x3d2a_aaab);
const C120: f32 = f32::from_bits(0x3c08_8889);
const C720: f32 = f32::from_bits(0x3ab6_0b61);

/// Clock and allpass state of one mono Phaser object.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PhaserClock {
    /// Cutoff and resonance the clock has reached (`obj+0x1f0`, `obj+0x1ec`).
    current: [f32; 2],
    /// Values the coefficient law was last evaluated for (`obj+0x1e4`, `obj+0x1e8`); `-1` after reset.
    designed: [f32; 2],
    coefficient: f32,
    feedback: f32,
    started: bool,
    countdown: usize,
    /// Stage inputs and outputs of the previous sample (`obj+0x208`).
    x: [f32; 4],
    y: [f32; 4],
}

impl Default for PhaserClock {
    fn default() -> Self {
        Self::new()
    }
}

impl PhaserClock {
    pub(crate) fn new() -> Self {
        Self {
            current: [0.; 2],
            designed: [-1.; 2],
            coefficient: 0.,
            feedback: 0.,
            started: false,
            countdown: 0,
            x: [0.; 4],
            y: [0.; 4],
        }
    }

    /// Native coefficient setter `0x14095d770`: recompute only when a value moved by 1e-6.
    fn design(&mut self, [cutoff, resonance]: [f32; 2]) {
        let moved = |a: f32, b: f32| (f64::from(a) - f64::from(b)).abs() >= CHANGE;
        if !(moved(self.designed[0], cutoff) || moved(self.designed[1], resonance)) {
            return;
        }
        self.designed = [cutoff, resonance];
        let p = if cutoff >= 0. { cutoff.min(1.) } else { 0. };
        let v = p * 3.;
        let square = p * p * 9.;
        let cube = square * v;
        let fourth = cube * p * 3.;
        let fifth = fourth * v;
        let exponential = if v >= SERIES_MIN || v <= -SERIES_MIN {
            fifth * p * 3. * C720 + fifth * C120 + cube * C6 + fourth * C24 + square * HALF + v + 1.
        } else {
            1.
        };
        let frequency = (f64::from(exponential - 1.) * 0.054_310_930_074_677_53 * 0.32999999999999996 + 0.02) as f32;
        self.coefficient = (1. - frequency) / (frequency + 1.);
        let r = if resonance >= 0. { resonance.min(1.) } else { 0. };
        self.feedback = (f64::from(r) * 0.85) as f32;
    }

    /// Native clock step (`0x140ae9f90`): the cutoff moves by at most `step`, the resonance by 0.015
    /// of the distance while it is more than `1e-4` away.
    fn clock(&mut self, goal: [f32; 2], step: f32) {
        let delta = goal[0] - self.current[0];
        self.current[0] += delta.clamp(-step, step);
        if (goal[1] - self.current[1]).abs() > MOVING {
            self.current[1] += (goal[1] - self.current[1]) * 0.015;
        }
    }

    /// One host call: the goals `[cutoff, resonance]` and the frames of `input` into `output`.
    /// The countdown carries across calls; the first call snaps to its goals.
    pub(crate) fn process(&mut self, goal: [f32; 2], input: &[f64], output: &mut [f64]) {
        let len = input.len();
        assert_eq!(output.len(), len);
        if !self.started {
            self.current = goal;
            self.started = true;
            self.countdown = 0;
        }
        // The step is set by the resonance at the call's start.
        let step = (1. / (31. * self.current[1] + 4.)).clamp(1. / 35., 0.25);
        let mut position = 0;
        while position < len {
            self.design(self.current);
            let moving = (goal[0] - self.current[0]).abs() > MOVING || (goal[1] - self.current[1]).abs() > MOVING;
            let n = if moving { self.countdown.min(len - position) } else { len - position };
            for (k, out) in output[position..position + n].iter_mut().enumerate() {
                let x = input[position + k] as f32;
                let mut stage = self.feedback * self.y[3] + x;
                for s in 0..4 {
                    let y = (stage + self.y[s]) * self.coefficient - self.x[s];
                    self.x[s] = stage;
                    self.y[s] = y;
                    stage = y;
                }
                *out = f64::from((stage + x) * 0.5);
            }
            position += n;
            if moving {
                self.countdown -= n;
                // The countdown reloads only while frames remain.
                if self.countdown == 0 && position < len {
                    self.countdown = QUANTUM;
                }
                self.clock(goal, step);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PhaserClock;

    /// Native Phaser filter under kemu (`phaser_kemu_vectors.json`, written by
    /// `tools/re/phaser_kemu_vectors.py`): host-call schedules of cutoff and resonance goals with
    /// frame counts. The worst error is the largest difference over the peak of the native output.
    #[test]
    fn phaser_live_lanes_match_native_process_vectors() {
        let file: serde_json::Value = serde_json::from_str(include_str!("phaser_kemu_vectors.json")).unwrap();
        let mut failures = Vec::new();
        let mut worst_overall = 0f64;
        for case in file["cases"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let values = |v: &serde_json::Value| -> Vec<f64> { v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect() };
            let stim = values(&case["stim"]);
            let native = values(&case["out"]);
            let calls: Vec<(f32, f32, usize)> = case["calls"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| (c[0].as_f64().unwrap() as f32, c[1].as_f64().unwrap() as f32, c[2].as_u64().unwrap() as usize))
                .collect();
            let mut clock = PhaserClock::new();
            let mut out = vec![0f64; stim.len()];
            let mut at = 0;
            for (cut, res, frames) in calls {
                clock.process([cut, res], &stim[at..at + frames], &mut out[at..at + frames]);
                at += frames;
            }
            assert_eq!(at, stim.len(), "{name}: call sizes do not cover the stimulus");
            let peak = native.iter().fold(1e-6f64, |m, v| m.max(v.abs()));
            let worst = out.iter().zip(&native).fold(0f64, |m, (a, b)| m.max((a - b).abs())) / peak;
            println!("{name}: worst error {worst:e} of peak");
            worst_overall = worst_overall.max(worst);
            if worst >= 1e-5 {
                failures.push(format!("{name}: worst error {worst:e} of peak"));
            }
        }
        println!("phaser live lanes: worst case {worst_overall:e} of peak over all vectors");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
