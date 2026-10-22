//! Kontakt `Stereorizer<T>`: the stereo wrapper of a mono core `T`.
//!
//! Decoded from the Skreamer Deluxe driver `140902480` (stereo branch, flag `+0x1c0 == 0`):
//! the left core runs on `[L, L]` and its channel 0 goes to the left output; the right
//! core runs on `[R, R]` and its channel 0 goes to the right output (`memmove` into
//! `+0x25d8`). Each core keeps its own state, and both take the same knobs.
//! The mono branch (flag `!= 0`) is not modeled here.
use super::super::Planar;

/// A mono core: both input channels carry the signal, channel 0 is the output.
pub(super) trait Mono {
    type State;
    fn run(&self, state: &mut Self::State, block: &mut Planar, len: usize);
}

/// One core per channel, with the core's configuration shared.
pub(super) struct Stereorizer<M> {
    core: M,
}

impl<M: Mono> Stereorizer<M> {
    pub(super) fn new(core: M) -> Self {
        Self { core }
    }

    /// `state[c]` is channel `c`'s history.
    pub(super) fn process(&self, state: &mut [M::State; 2], block: &mut Planar, len: usize) {
        for (c, st) in state.iter_mut().enumerate() {
            let mut pair: Planar = [block[c]; 2];
            self.core.run(st, &mut pair, len);
            block[c][..len].copy_from_slice(&pair[0][..len]);
        }
    }
}

impl Mono for super::tubescreamer::Engine {
    type State = super::tubescreamer::State;

    fn run(&self, state: &mut Self::State, block: &mut Planar, len: usize) {
        self.process(state, block, len);
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::BLOCK;
    use super::super::tubescreamer::{DEFAULTS, Engine, State};
    use super::*;
    use sampler_ir::fx::color::Tubescreamer;

    /// The native mono vectors (kontakt_tubescreamer_vectors.json, both channels = x) define
    /// one channel's law. The driver proves the stereo wiring, so a stereo run must reproduce
    /// the mono native output on each channel, whatever the other channel carries.
    #[test]
    fn stereo_channels_match_the_native_mono_vectors() {
        let cases: serde_json::Value = serde_json::from_str(include_str!("tables/kontakt_tubescreamer_vectors.json")).unwrap();
        let sine = |i: usize| (0.5 * (std::f64::consts::TAU * 200.0 * i as f64 / 48_000.0).sin()) as f32;
        for case in cases.as_array().unwrap() {
            let mut knobs = DEFAULTS;
            for (k, v) in case["params"].as_object().unwrap() {
                knobs[k.parse::<usize>().unwrap()] = v.as_f64().unwrap() as f32;
            }
            let unit = Stereorizer::new(Engine::new(Tubescreamer { knobs }, 48_000));
            // Feeds `live` on the channel under test and `other` on the other one; returns that channel's output.
            let run = |live: &dyn Fn(usize) -> f32, other: &dyn Fn(usize) -> f32, channel: usize, frames: usize| {
                let mut s = [State::default(); 2];
                for _ in 0..4800 / BLOCK {
                    unit.process(&mut s, &mut [[0.0; BLOCK]; 2], BLOCK);
                }
                let mut out = Vec::new();
                for start in (0..frames).step_by(BLOCK) {
                    let mut block: Planar = [[0.0; BLOCK]; 2];
                    for i in 0..BLOCK {
                        block[channel][i] = f64::from(live(start + i));
                        block[1 - channel][i] = f64::from(other(start + i));
                    }
                    unit.process(&mut s, &mut block, BLOCK);
                    out.extend(block[channel].iter().map(|&v| v as f32));
                }
                out
            };
            let check = |name: &str, out: &[f32], native: &serde_json::Value, at: usize| {
                for (i, n) in native.as_array().unwrap().iter().enumerate() {
                    let n = n.as_f64().unwrap() as f32;
                    assert!((out[at + i] - n).abs() < 2e-6, "{knobs:?} {name} {i}: {} vs {n}", out[at + i]);
                }
            };
            let impulse = |i: usize| if i == 0 { 0.5 } else { 0.0 };
            let quiet = |_: usize| 0.0;
            let other = |i: usize| (0.3 * (std::f64::consts::TAU * 1_000.0 * i as f64 / 48_000.0).cos()) as f32;
            check("left impulse", &run(&impulse, &other, 0, 128), &case["impulse_L"], 0);
            check("right impulse", &run(&impulse, &quiet, 1, 128), &case["impulse_L"], 0);
            check("left sine", &run(&sine, &other, 0, 2400), &case["sine_L_settled_1920_2400"], 1920);
            check("right sine", &run(&sine, &other, 1, 2400), &case["sine_L_settled_1920_2400"], 1920);
        }
    }
}
