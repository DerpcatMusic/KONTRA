//! Bench-only (feature `dspbench`): one dynamics stage driven as a stereo-in, left-out mono block.
//! Uses the same `prepare`/`process` calls as the engine's dynamics dispatch.

use super::{Dynamics, DynamicsSettings};
use crate::dsp::{BLOCK, Planar, ProcessorState};

pub struct Stage {
    dynamics: Dynamics,
    state: ProcessorState,
    slab: Vec<[f64; 2]>,
    at: u64,
}

impl Stage {
    /// Panics if the settings are invalid for the law (the bench builds only valid ones).
    pub fn new(settings: DynamicsSettings, rate: u32) -> Self {
        let mut cells = 0;
        let dynamics = settings.prepare(rate, &mut Vec::new(), &mut cells).expect("dynamics settings");
        Self { dynamics, state: ProcessorState::default(), slab: vec![[0.; 2]; cells], at: 0 }
    }

    /// Mono in (fed to both channels), left out.
    pub fn process(&mut self, input: &[f32], out: &mut [f32]) {
        for (xs, ys) in input.chunks(BLOCK).zip(out.chunks_mut(BLOCK)) {
            let len = xs.len();
            let mut block: Planar = [[0.0; BLOCK]; 2];
            for (i, &x) in xs.iter().enumerate() {
                block[0][i] = f64::from(x);
                block[1][i] = f64::from(x);
            }
            self.dynamics.process(&mut self.state, &mut self.slab, &[], &mut block, len, self.at);
            for (dst, src) in ys.iter_mut().zip(&block[0]) {
                *dst = *src as f32;
            }
            self.at += len as u64;
        }
    }
}
