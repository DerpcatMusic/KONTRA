//! Frozen 5fc362f3 processing order; coefficient math itself is unchanged.
use super::super::BLOCK;
use super::*;

impl Daft {
    fn frozen_process(
        &self,
        state: &mut ProcessorState,
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
        modulation: [f64; 4],
    ) {
        let mut i = 0;
        while i < len {
            let t = at + i as u64;
            if t % QUANTUM == 0 || state.aux[STARTED] == 0. {
                self.control(state, parameters, t, modulation);
            }
            let run = ((QUANTUM - t % QUANTUM) as usize).min(len - i);
            let ramping = state.aux[DELTA..DELTA + 4].iter().any(|d| *d != 0.);
            let mut coefficients = self.coefficients(&state.aux[CURRENT..CURRENT + 4]);
            for n in i..i + run {
                let x = [block[0][n], block[1][n]];
                let inputs = [0, 1].map(|c| [0.5 * (state.aux[PREVIOUS + c] + x[c]), x[c]]);
                state.aux[PREVIOUS..PREVIOUS + 2].copy_from_slice(&x);
                let mut out = [0.; 2];
                for half in 0..2 {
                    if ramping {
                        for k in 0..4 {
                            state.aux[CURRENT + k] += state.aux[DELTA + k];
                        }
                        coefficients = self.coefficients(&state.aux[CURRENT..CURRENT + 4]);
                    }
                    for c in 0..2 {
                        out[c] += Self::tick(&coefficients, &mut state.z[c], inputs[c][half]);
                    }
                }
                block[0][n] = 0.5 * out[0];
                block[1][n] = 0.5 * out[1];
            }
            i += run;
        }
        for v in &mut state.z.iter_mut().flatten() {
            *v = super::flush(*v);
        }
    }
}

fn settings() -> DaftSettings {
    let parameter = |i| {
        Parameter::Control(ControlRange {
            control: crate::ControlId(i),
            low: 0.,
            high: 1.,
            ramp_frames: 64,
        })
    };
    DaftSettings {
        gain: parameter(0),
        cutoff: parameter(1),
        resonance: parameter(2),
        response: parameter(3),
    }
}

#[test]
fn daft_settled_block_prepares_coefficients_once() {
    let kernel = settings().compile(48000, &mut Vec::new());
    let parameters = [0.2, 0.6, 0.4, 0.].map(|v| ControlRamp::test_ramp(v, v, 0, 0));
    for at in [0, 1, 31, 32] {
        let mut state = ProcessorState::default();
        for _ in 0..2 {
            let mut block = [[0.1; BLOCK]; 2];
            COEFFICIENT_CALLS.set(0);
            kernel.process(&mut state, &parameters, &mut block, BLOCK, at, [0.; 4]);
            assert_eq!(COEFFICIENT_CALLS.get(), 1, "settled block at {at}");
        }
    }
}

#[test]
fn daft_cache_matches_frozen_pcm_and_state_bits() {
    for rate in [44100, 48000, 96000] {
        let kernel = settings().compile(rate, &mut Vec::new());
        for start in [0, 1, 31] {
            for len in [0, 1, 31, 32, 33, BLOCK] {
                let mut actual = ProcessorState::default();
                let mut expected = ProcessorState::default();
                let mut at = start;
                for pass in 0..12 {
                    let parameters = std::array::from_fn::<_, 4, _>(|i| {
                        let from = [0.1, 0.6, 0.3, 0.][i];
                        let target = if pass < 3 {
                            from
                        } else {
                            [0.7, 0.2, 0.8, 1.][i]
                        };
                        ControlRamp::test_ramp(
                            from,
                            target,
                            start + 32,
                            if pass < 6 { 0 } else { 96 },
                        )
                    });
                    let modulation = if pass < 9 {
                        [0.; 4]
                    } else {
                        [0.1, -0.2, 0.05, 0.]
                    };
                    let input = std::array::from_fn(|c| {
                        std::array::from_fn(|i| {
                            if i >= len {
                                0.123
                            } else {
                                match pass % 3 {
                                    0 => {
                                        if i == 0 {
                                            0.3 * (c + 1) as f64
                                        } else {
                                            0.
                                        }
                                    }
                                    1 => (i as f64 * 0.21 + c as f64).sin() * 0.2,
                                    _ => [0., -0., f64::from_bits(1), -f64::from_bits(1)][i % 4],
                                }
                            }
                        })
                    });
                    let mut output = input;
                    let mut reference = input;
                    kernel.process(&mut actual, &parameters, &mut output, len, at, modulation);
                    kernel.frozen_process(
                        &mut expected,
                        &parameters,
                        &mut reference,
                        len,
                        at,
                        modulation,
                    );
                    assert_eq!(
                        output.map(|ch| ch.map(f64::to_bits)),
                        reference.map(|ch| ch.map(f64::to_bits))
                    );
                    assert_eq!(
                        actual.z.map(|ch| ch.map(f64::to_bits)),
                        expected.z.map(|ch| ch.map(f64::to_bits))
                    );
                    assert_eq!(actual.aux.map(f64::to_bits), expected.aux.map(f64::to_bits));
                    at += len as u64;
                }
            }
        }
    }
}
