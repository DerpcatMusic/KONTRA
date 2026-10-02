//! The independently decoded saved retriggered, zero-delay sine-only Multi LFO.
//! Other waveforms, free-running clocks and live frequency conversion remain
//! unsupported. See audits/MODULATION.md for the source-clock boundary.

use super::voice::{FIXED_ONE, MAX_STEP};
use crate::modulation::PitchLfo;

/// One note's native source phases and the shared pitch-buffer interpolator.
/// The native source clock pauses when bypassed; its audio interpolation
/// offset advances separately, including short event fragments.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Clock {
    phase: [f32; 16],
    previous: f32,
    current: f32,
    offset: u8,
    initialized: bool,
}

impl Clock {
    pub fn skip_bypassed(&mut self, n: usize) {
        self.offset = ((usize::from(self.offset) + n) & 31) as u8;
    }

    /// Relative source positions for one ordinary sampler block. Previewing a
    /// copied clock during planning gives the same reach as rendering it.
    pub fn positions(
        &mut self,
        lfos: &[PitchLfo],
        rate: f32,
        tempo: f32,
        step: f64,
        out: &mut [u64],
    ) -> (u64, u64) {
        let n = out.len();
        if n == 0 {
            return (0, 0);
        }
        let mut points = [0.; 4];
        let active = lfos.iter().any(|l| !l.bypassed);
        for lfo in lfos.iter().filter(|l| !l.bypassed) {
            let phase = &mut self.phase[lfo.slot as usize];
            let hz = lfo.frequency(tempo);
            let increment = hz * 32. / rate;
            let gain = 12. * lfo.depth * lfo.sine / lfo.sine.abs().max(1.);
            for (i, point) in points.iter_mut().take(n.div_ceil(32)).enumerate() {
                // Native Multi negates its sine component. Ordinary sine has
                // a different sign and is deliberately not accepted here.
                let angle = (*phase + i as f32 * increment).rem_euclid(1.) * std::f32::consts::TAU;
                *point -= angle.sin() * gain;
            }
            *phase = (*phase + hz * n as f32 / rate).rem_euclid(1.);
        }
        if active && !self.initialized {
            self.current = points[0];
            self.previous = self.current;
            self.initialized = true;
        }
        let mut next = 0;
        let mut position = 0u64;
        for frame in out.iter_mut() {
            let pitch = if active {
                if self.offset == 0 {
                    self.previous = self.current;
                    self.current = points[next];
                    next += 1;
                }
                self.previous + (self.current - self.previous) * (f32::from(self.offset) / 32.)
            } else {
                0.
            };
            *frame = position;
            let ratio = 2f64.powf(f64::from(pitch) / 12.);
            position += ((step * ratio).min(MAX_STEP) * FIXED_ONE) as u64;
            self.offset = (self.offset + 1) & 31;
        }
        (position, out.last().copied().unwrap_or(0))
    }
}
