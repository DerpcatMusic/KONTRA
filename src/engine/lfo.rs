//! The independently decoded saved retriggered sine-only Multi LFO.
//! Other waveforms, free-running clocks and live frequency conversion remain
//! unsupported. See audits/MODULATION.md for the source-clock boundary.

use super::voice::{FIXED_ONE, MAX_STEP};
use crate::modulation::PitchLfo;

/// One note's native source phases and the shared pitch-buffer interpolator.
/// The native source clock pauses when bypassed; its audio interpolation
/// offset advances separately, including short event fragments.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Clock {
    phase: [f64; 16],
    previous: f32,
    current: f32,
    offset: u8,
    initialized: bool,
    fades: [Fade; 16],
    fade_started: u16,
}

#[derive(Clone, Copy, Debug, Default)]
struct Fade {
    remaining: u32,
    value: f32,
    factor: f32,
}

impl Fade {
    fn new(ms: f32, rate: f32) -> Self {
        // Native time getter returns milliseconds; DSP runs at rate / 32.
        let remaining = (ms * (rate / 32.) * 0.001) as u32;
        let factor = if remaining == 0 {
            1.
        } else {
            (1. + 1. / f64::from(0.3f32)).powf(1. / f64::from(remaining)) as f32
        };
        Self {
            remaining,
            value: 0.3,
            factor,
        }
    }

    fn next(&mut self) -> f32 {
        if self.remaining == 0 {
            return 1.;
        }
        let gain = self.value - 0.3;
        // v71/v72 set the native legacy switch false: its ceiling is 1,
        // unlike v73's optional .3..1.3 mode. Only N points are scaled.
        self.value = (self.value * self.factor).clamp(0., 1.);
        self.remaining -= 1;
        gain
    }
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
        let active = lfos.iter().any(|l| !l.bypassed);
        let mut position = 0u64;
        for (i, frame) in out.iter_mut().enumerate() {
            let pitch = if active {
                // Voice planning fragments are not native control intervals.
                // Sample the source at the retained note-clock boundary, even
                // when a command or another loop voice split this render call.
                if self.offset == 0 || !self.initialized {
                    let mut point = 0.;
                    for lfo in lfos.iter().filter(|l| !l.bypassed) {
                        let hz = f64::from(lfo.frequency(tempo));
                        let phase = (self.phase[lfo.slot as usize]
                            + i as f64 * hz / f64::from(rate))
                        .rem_euclid(1.);
                        let mut gain = 12. * lfo.depth * lfo.sine / lfo.sine.abs().max(1.);
                        if lfo.fade_ms > 0. {
                            let slot = usize::from(lfo.slot);
                            let bit = 1 << slot;
                            if self.fade_started & bit == 0 {
                                self.fades[slot] = Fade::new(lfo.fade_ms, rate);
                                self.fade_started |= bit;
                            }
                            gain *= self.fades[slot].next();
                        }
                        // Multi negates sine; ordinary sine is not admitted.
                        point -= (phase * std::f64::consts::TAU).sin() as f32 * gain;
                    }
                    self.previous = self.current;
                    self.current = point;
                    if !self.initialized {
                        self.previous = point;
                        self.initialized = true;
                    }
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
        for lfo in lfos.iter().filter(|l| !l.bypassed) {
            let phase = &mut self.phase[lfo.slot as usize];
            *phase = (*phase + f64::from(lfo.frequency(tempo)) * n as f64 / f64::from(rate))
                .rem_euclid(1.);
        }
        (position, out.last().copied().unwrap_or(0))
    }
}
