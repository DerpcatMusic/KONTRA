//! The independently decoded saved retriggered sine-only Multi LFO.
//! Other waveforms, free-running clocks and live frequency conversion remain
//! unsupported. See audits/MODULATION.md for the source-clock boundary.

use super::voice::{FIXED_ONE, MAX_STEP};
use crate::modulation::{PitchLfo, VolumeLfo};

/// One note's native source phases and retained pitch/volume interpolators.
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
    volume: [Option<super::lfo_volume::Target>; 16],
    volume_previous: f32,
    volume_current: f32,
    volume_initialized: bool,
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
        volume_lfos: &[VolumeLfo],
        rate: f32,
        tempo: f32,
        step: f64,
        out: &mut [u64],
        mut volume_out: Option<&mut [f32]>,
    ) -> (u64, u64) {
        let n = out.len();
        if n == 0 {
            return (0, 0);
        }
        let active = lfos.iter().any(|l| !l.bypassed);
        let volume_active = volume_lfos.iter().any(|l| !l.source.bypassed);
        // A coefficient is prepared once per note/target, outside the retained
        // control-point loop. Bypassing never discards this target state.
        for lfo in volume_lfos.iter().filter(|l| !l.source.bypassed) {
            self.volume[usize::from(lfo.source.slot)].get_or_insert_with(|| {
                super::lfo_volume::Target::new(lfo.lag_ms, rate)
                    .expect("prepared nonnegative volume target lag")
            });
        }
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
            let volume = if volume_active {
                if self.offset == 0 || !self.volume_initialized {
                    let mut point = 1.;
                    for lfo in volume_lfos.iter().filter(|l| !l.source.bypassed) {
                        let source = &lfo.source;
                        let slot = usize::from(source.slot);
                        let phase = (self.phase[slot]
                            + i as f64 * f64::from(source.frequency(tempo)) / f64::from(rate))
                        .rem_euclid(1.);
                        // The admitted volume source has no fade. Its bipolar
                        // signal enters the native target before unipolar range
                        // conversion and target lag; no intermediate clamp.
                        let signal = -(phase * std::f64::consts::TAU).sin() as f32 * source.sine
                            / source.sine.abs().max(1.);
                        let target = self.volume[slot].as_mut().expect("prepared volume target");
                        point = target.point(signal, point, lfo.intensity, lfo.negative);
                    }
                    self.volume_previous = self.volume_current;
                    self.volume_current = point.max(0.);
                    if !self.volume_initialized {
                        self.volume_previous = self.volume_current;
                        self.volume_initialized = true;
                    }
                }
                self.volume_previous
                    + (self.volume_current - self.volume_previous) * (f32::from(self.offset) / 32.)
            } else {
                1.
            };
            if let Some(output) = volume_out.as_deref_mut() {
                output[i] = volume;
            }
            *frame = position;
            let ratio = if active {
                2f64.powf(f64::from(pitch) / 12.)
            } else {
                1.
            };
            position += ((step * ratio).min(MAX_STEP) * FIXED_ONE) as u64;
            self.offset = (self.offset + 1) & 31;
        }
        for lfo in lfos.iter().filter(|l| !l.bypassed) {
            let phase = &mut self.phase[lfo.slot as usize];
            *phase = (*phase + f64::from(lfo.frequency(tempo)) * n as f64 / f64::from(rate))
                .rem_euclid(1.);
        }
        for lfo in volume_lfos.iter().filter(|l| !l.source.bypassed) {
            let source = &lfo.source;
            // A single native source can feed pitch and volume. Neither the
            // second target nor a copied planning clock advances it twice.
            if !lfos.iter().any(|l| l.slot == source.slot && !l.bypassed) {
                let phase = &mut self.phase[usize::from(source.slot)];
                *phase = (*phase + f64::from(source.frequency(tempo)) * n as f64 / f64::from(rate))
                    .rem_euclid(1.);
            }
        }
        (position, out.last().copied().unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_sine_volume_shares_clock_lag_and_fragmented_interpolation() {
        let source = PitchLfo {
            slot: 7,
            count: 16.,
            note_value: 1. / 24.,
            sine: 1.,
            fade_ms: 0.,
            depth: 0.25,
            targets: vec![(2, 0.25)],
            bypassed: false,
        };
        let volume = VolumeLfo {
            source: source.clone(),
            target: 4,
            intensity: 1.,
            negative: false,
            lag_ms: 15,
        };
        let mut only = Clock::default();
        let mut shared = Clock::default();
        let mut pitch = Clock::default();
        let mut split = Clock::default();
        let (mut a, mut b, mut c) = ([0.; 512], [0.; 512], [0.; 512]);
        for at in (0..512).step_by(128) {
            let (mut positions, mut shared_positions, mut pitch_positions) =
                ([0; 128], [0; 128], [0; 128]);
            only.positions(
                &[],
                std::slice::from_ref(&volume),
                48_000.,
                120.,
                1.,
                &mut positions,
                Some(&mut a[at..at + 128]),
            );
            shared.positions(
                std::slice::from_ref(&source),
                std::slice::from_ref(&volume),
                48_000.,
                120.,
                1.,
                &mut shared_positions,
                Some(&mut b[at..at + 128]),
            );
            pitch.positions(
                std::slice::from_ref(&source),
                &[],
                48_000.,
                120.,
                1.,
                &mut pitch_positions,
                None,
            );
            assert_eq!(
                shared_positions, pitch_positions,
                "volume cannot advance pitch a second time"
            );
            for (from, to) in [(at, at + 17), (at + 17, at + 128)] {
                split.positions(
                    std::slice::from_ref(&source),
                    std::slice::from_ref(&volume),
                    48_000.,
                    120.,
                    1.,
                    &mut positions[..to - from],
                    Some(&mut c[from..to]),
                );
            }
        }
        assert_eq!(
            a, b,
            "adding a pitch destination cannot advance the volume source twice"
        );
        assert!(a.iter().zip(c).all(|(a, c)| (a - c).abs() < 1e-7));
        assert!((shared.phase[7] - 3. * 512. / 48_000.).abs() < 1e-8);
        // Independent native control-point reference: first bipolar zero maps
        // to .5; the next 32-frame point applies the positive15ms coefficient.
        let signal = -(std::f64::consts::TAU * 3. * 32. / 48_000.).sin() as f32;
        let next = 0.5 + ((signal + 1.) * 0.5 - 0.5) * 0.18508725;
        assert_eq!(a[0], 0.5);
        assert_eq!(a[32], 0.5);
        assert!((a[64] - next).abs() < 1e-7);
        assert!((a[48] - (0.5 + (next - 0.5) * 0.5)).abs() < 1e-7);

        let mut paused = only;
        let mut reference = only;
        let mut bypass = volume.clone();
        bypass.source.bypassed = true;
        let phase = paused.phase;
        let (mut positions, mut unity) = ([0; 64], [0.; 64]);
        paused.positions(
            &[],
            &[bypass],
            48_000.,
            120.,
            1.,
            &mut positions,
            Some(&mut unity),
        );
        assert_eq!(unity, [1.; 64]);
        assert_eq!(
            paused.phase, phase,
            "bypass pauses the source, retaining lag state"
        );
        reference.skip_bypassed(64);
        let (mut resumed, mut expected) = ([0.; 64], [0.; 64]);
        paused.positions(
            &[],
            std::slice::from_ref(&volume),
            48_000.,
            120.,
            1.,
            &mut positions,
            Some(&mut resumed),
        );
        reference.positions(
            &[],
            std::slice::from_ref(&volume),
            48_000.,
            120.,
            1.,
            &mut positions,
            Some(&mut expected),
        );
        assert_eq!(resumed, expected, "resume cannot restart the target lag");
        let mut zero = volume;
        zero.intensity = 0.;
        let mut clock = Clock::default();
        clock.positions(
            &[],
            std::slice::from_ref(&zero),
            48_000.,
            120.,
            1.,
            &mut positions,
            Some(&mut unity),
        );
        assert_eq!(unity, [1.; 64]);

        // Partial source bypass keeps the other source advancing while the
        // paused source retains its own phase/lag and aggregate interpolation.
        let mut first = zero.clone();
        first.intensity = 1.;
        let mut second = first.clone();
        second.source.slot = 3;
        second.source.count = 24.;
        second.intensity = 0.4;
        second.negative = true;
        let mut whole = Clock::default();
        let mut fragmented = Clock::default();
        let mut sources = [first, second];
        let (mut x, mut y) = ([0.; 128], [0.; 128]);
        let mut positions = [0; 128];
        for (n, bypass) in [(13, false), (73, true), (111, false)] {
            sources[0].source.bypassed = bypass;
            whole.positions(
                &[],
                &sources,
                48_000.,
                120.,
                1.,
                &mut positions[..n],
                Some(&mut x[..n]),
            );
            let mut at = 0;
            while at < n {
                let end = (at + 17).min(n);
                fragmented.positions(
                    &[],
                    &sources,
                    48_000.,
                    120.,
                    1.,
                    &mut positions[..end - at],
                    Some(&mut y[at..end]),
                );
                at = end;
            }
            assert!(
                x[..n]
                    .iter()
                    .zip(&y[..n])
                    .all(|(x, y)| (x - y).abs() < 1e-7)
            );
        }
        assert!((whole.phase[7] - 3. * 124. / 48_000.).abs() < 1e-8);
        assert!((whole.phase[3] - 2. * 197. / 48_000.).abs() < 1e-8);
    }
}
