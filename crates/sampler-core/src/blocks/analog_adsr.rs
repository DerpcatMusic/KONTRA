//! UVI AnalogADSR control-rate recurrence (native process 0x14113f0a0, setters
//! 0x14113eaf0..0x14113edc0, spec uvi-modulator-laws.md "Analog ADSR"; audit
//! uvi-core-analog-law). One value per at-most-32-frame step.
//!
//! Proved and implemented: the stage coefficients, attack/decay/release updates,
//! partial-block coefficient, sustain handoff, the 1e-5 floor (0x141e37c20) and
//! the dynamic-range scale. NOT proved, so not claimed: the attack's initial level
//! (here 0), the emit timing of decay/release samples, the sustain smoothing
//! segment, ManualTrigger, Bypass and event-edge sequencing (0x14113eea0/0x14113f010).
//! Not wired into the UVI importer for that reason; the importer reports it unsupported.

const FLOOR: f32 = 0.000_01;

#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub key_to_attack: f32,
    pub key_to_decay: f32,
    pub vel_to_attack: f32,
    pub vel_to_decay: f32,
    pub dynamic_range: f32,
    pub punch: f32,
    pub invert_velocity: bool,
    pub attack_decay_mode: bool,
}
impl Default for Params {
    /// Native per-voice setup 0x14113f760.
    fn default() -> Self {
        Self {
            attack: 0.001, decay: 0.05, sustain: 1.0, release: 0.01,
            key_to_attack: 0.0, key_to_decay: 0.0, vel_to_attack: 0.0, vel_to_decay: 0.0,
            dynamic_range: 0.0, punch: 0.0, invert_velocity: false, attack_decay_mode: false,
        }
    }
}

/// `1 - (2/3)^(1/(10000^(v*velAmt) * 2^((key-60)*keyAmt/12) * T * Fs/32))`, 1 for T <= 0.
fn coefficient(time: f32, velocity: f32, vel_amt: f32, key: f32, key_amt: f32, rate: f32) -> f32 {
    if time <= 0.0 {
        return 1.0;
    }
    let denom = 10_000f32.powf(velocity * vel_amt)
        * 2f32.powf((key - 60.0) * key_amt * (1.0 / 12.0))
        * time
        * (rate * 0.031_25);
    (1.0 - (2.0f64 / 3.0).powf(f64::from(1.0 / denom))) as f32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage { Idle, Attack, Decay, Sustain, Release }

#[derive(Clone, Debug)]
pub struct Voice {
    p: Params,
    stage: Stage,
    level: f32,
    scale: f32,
    attack: f32,
    decay: f32,
    release: f32,
}
impl Voice {
    pub fn start(p: Params, key: f32, velocity: u8, rate: f32) -> Self {
        let v = f32::from(velocity) * 0.007_874_015_72;
        let v = if p.invert_velocity { 1.0 - v } else { v };
        Self {
            p,
            stage: Stage::Attack,
            level: 0.0,
            scale: 10f32.powf((v - 1.0) * p.dynamic_range * 0.05),
            attack: coefficient(p.attack, v, p.vel_to_attack, key, p.key_to_attack, rate),
            decay: coefficient(p.decay, v, p.vel_to_decay, key, p.key_to_decay, rate),
            release: if p.release <= 0.0 {
                1.0
            } else {
                (1.0 - (2.0f64 / 3.0).powf(f64::from(1.0 / (p.release * rate * 0.031_25)))) as f32
            },
        }
    }
    pub fn note_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }
    pub fn finished(&self) -> bool {
        self.stage == Stage::Idle
    }
    /// One control value for a step of `frames` (1..=32) audio frames.
    pub fn step(&mut self, frames: u32) -> f32 {
        let part = |c: f32| if frames >= 32 { c } else { 1.0 - (1.0 - c).powf(frames as f32 * 0.031_25) };
        let out = self.level.min(1.0) * self.scale;
        match self.stage {
            Stage::Idle => return 0.0,
            Stage::Attack => {
                let target = self.p.punch + 1.0;
                self.level = (self.level + (target * 1.5 - self.level) * part(self.attack)).min(target);
                if self.level >= target {
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level = (1.0 - part(self.decay)) * self.level;
                if self.p.attack_decay_mode {
                    if self.level <= FLOOR {
                        self.stage = Stage::Idle;
                    }
                } else if self.level <= self.p.sustain {
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => return self.p.sustain * self.scale,
            Stage::Release => {
                self.level = (1.0 - part(self.release)) * self.level;
                if self.level <= FLOOR {
                    self.stage = Stage::Idle;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coefficient_follows_the_two_thirds_power_law_and_is_not_linear() {
        // Fs/32 = 1500 steps/s, T=0.01 -> 15 steps: a = 1 - (2/3)^(1/15).
        let c = coefficient(0.01, 0.0, 0.0, 60.0, 0.0, 48_000.0);
        let want = (1.0 - (2.0f64 / 3.0).powf(1.0 / 15.0)) as f32;
        assert_eq!(c, want);
        assert_eq!(coefficient(0.0, 0.0, 0.0, 60.0, 0.0, 48_000.0), 1.0);
    }

    #[test]
    fn stages_run_the_exponential_recurrence_and_release_reaches_the_floor() {
        let p = Params { sustain: 0.5, ..Default::default() };
        let mut v = Voice::start(p, 60.0, 127, 48_000.0);
        let c = v.attack;
        assert_eq!(v.step(32), 0.0);
        assert!((v.level - 1.5 * c).abs() < 1e-6);
        let mut last = 0.0;
        for _ in 0..2000 {
            last = v.step(32);
            if v.stage == Stage::Sustain { break; }
        }
        assert!(v.stage == Stage::Sustain && last > 0.5 && v.level <= 0.5);
        assert_eq!(v.step(32), 0.5);
        v.note_off();
        for _ in 0..10_000 {
            v.step(32);
            if v.finished() { return; }
        }
        panic!("release never reached the floor");
    }
}
