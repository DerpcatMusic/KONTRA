//! Frozen 608a20a1 equations, independent of the extracted sample helpers.
use super::*;
use control::PreparedParameter;
use lanes::{Batch, Cells, LaneBlock, VOICES};
use svf::{FilterBank, FilterContext, PreparedFilter};

fn reference(stages: &[PreparedProcessor], states: &mut [ProcessorState], block: &mut Planar,
    len: usize, parameters: &[ControlRamp], at: u64, bank: &mut FilterBank) {
    let mut index = 0;
    while index < stages.len() {
        let stage = &stages[index];
        let state = &mut states[index];
        index += 1;
        match stage {
            PreparedProcessor::Gain(gain) => {
                for channel in block.iter_mut() {
                    for x in &mut channel[..len] { *x *= gain; }
                }
            }
            PreparedProcessor::ControlGain(lane) => {
                for i in 0..len {
                    let g = parameters[*lane].value(at + i as u64);
                    block[0][i] *= g; block[1][i] *= g;
                }
            }
            PreparedProcessor::StereoMatrix(m) => {
                for i in 0..len {
                    let (l, r) = (block[0][i], block[1][i]);
                    block[0][i] = m[0][0] * l + m[0][1] * r;
                    block[1][i] = m[1][0] * l + m[1][1] * r;
                }
            }
            PreparedProcessor::Gainer { dry, gain, k } => {
                let mut current = state.z[0][0] as f32;
                for i in 0..len {
                    let target = gain.value(parameters, at + i as u64, None) as f32;
                    if state.aux[0] == 0. { (current, state.aux[0]) = (target, 1.); }
                    let m = dry + f64::from(current);
                    block[0][i] *= m; block[1][i] *= m;
                    current += (target - current) * *k as f32;
                }
                state.z[0][0] = f64::from(current);
            }
            PreparedProcessor::StereoModeller { stereo, .. } => {
                let (mut width, mut pan) = (state.aux[0] as f32, state.aux[1] as f32);
                let mut delta = 0.;
                for i in 0..len {
                    let [tw, tp] = stereo.targets(parameters, at + i as u64);
                    if state.aux[2] == 0. { (width, pan, state.aux[2]) = (tw, tp, 1.); }
                    let (l, r) = (block[0][i] as f32, block[1][i] as f32);
                    let (l, r) = if width >= 0.5 {
                        let spread = 2. * width - 1.;
                        ((1. + spread) * l - spread * r, (1. + spread) * r - spread * l)
                    } else {
                        let a = 0.5 - width;
                        (l + a * (r - l), r + a * (l - r))
                    };
                    block[0][i] = f64::from(l * (1. - pan.max(0.)));
                    block[1][i] = f64::from(r * (1. + pan.min(0.)));
                    width += (tw - width) * (1.0f32 / 180.);
                    if i >= len / 4 * 4 || i % 4 != 3 { delta = (tp - pan) * f32::from_bits(0x3a11a2b4); }
                    pan += delta;
                }
                (state.aux[0], state.aux[1]) = (f64::from(width), f64::from(pan));
            }
            PreparedProcessor::Mix { count, lanes } => {
                let inner = index..index + usize::from(*count);
                index = inner.end;
                let [dry, wet, bypass] = lanes.map(|lane| parameters[lane]);
                let last = at + len.saturating_sub(1) as u64;
                let off = bypass.value(at) >= 1. && bypass.value(last) >= 1.;
                let input = *block;
                if !off { reference(&stages[inner.clone()], &mut states[inner], block, len, parameters, at, bank); }
                for c in 0..2 { for i in 0..len {
                    let t = at + i as u64;
                    let b = bypass.value(t);
                    let wet_part = if off { 0. } else { wet.value(t) * (1. - b) * block[c][i] };
                    block[c][i] = (dry.value(t) * (1. - b) + b) * input[c][i] + wet_part;
                } }
            }
            PreparedProcessor::StateVariable(filter) => {
                let cache = bank.prepare(*filter, len, parameters, at, None);
                let cache = bank.cache(cache);
                for i in 0..len {
                    let c = cache.coefficients[if cache.uniform { 0 } else { i }];
                    for ch in 0..2 {
                        let x = block[ch][i];
                        if let Some(high) = cache.one_pole_high() {
                            state.z[0][ch] += (x - state.z[0][ch]) * c.a3;
                            block[ch][i] = if high { x - state.z[0][ch] } else { state.z[0][ch] };
                        } else {
                            let [m0, mk, m2] = cache.mix();
                            let (ic1, ic2) = (state.z[0][ch], state.z[1][ch]);
                            let v3 = x - ic2;
                            let band = c.a1 * ic1 + c.a2 * v3;
                            let low = ic2 + c.a2 * ic1 + c.a3 * v3;
                            state.z[0][ch] = 2. * band - ic1;
                            state.z[1][ch] = 2. * low - ic2;
                            block[ch][i] = m0 * x + (mk * c.k) * band + m2 * low;
                        }
                    }
                }
                state.z = state.z.map(|row| row.map(flush));
            }
            _ => unreachable!("outside this consolidation slice"),
        }
    }
}

fn bits(state: &ProcessorState) -> ([[u64; 2]; 2], [u64; 16], u32, u32) {
    (state.z.map(|r| r.map(f64::to_bits)), state.aux.map(f64::to_bits), state.delay_position, state.delay_filled)
}

fn check(stages: &[PreparedProcessor], filters: &[PreparedFilter], parameters: &[ControlRamp]) {
    for len in [0usize, 1, 3, 4, 5, 17, BLOCK] {
        for masked in [false, true] {
            for count in [1, 3, VOICES] {
                let batch = Batch { count, expressions: [None; VOICES],
                    ends: std::array::from_fn(|k| if masked { len.saturating_sub(k / 2) } else { len }), len };
                let slab = Slab::new(vec![ProcessorState::default(); count * stages.len()].into_boxed_slice(), stages.len());
                let mut cells: Cells<'_> = std::array::from_fn(|v| if v < count { Some(slab.claim(v)) } else { None });
                let mut scalar = vec![vec![ProcessorState::default(); stages.len()]; count];
                let mut expected_states = scalar.clone();
                let mut bank = FilterBank::new(filters, 0).unwrap();
                let mut scalar_bank = FilterBank::new(filters, 0).unwrap();
                let mut oracle_bank = FilterBank::new(filters, 0).unwrap();
                for block_index in 0..4 {
                    let at = block_index as u64 * BLOCK as u64;
                    let input: LaneBlock = std::array::from_fn(|i| std::array::from_fn(|k| match block_index {
                        0 => if i == 0 { 1. - 0.1 * k as f64 } else { 0. },
                        1 => ((i + k) as f64 * 0.137).sin() * 0.2,
                        2 => if i % 2 == 0 { -0. } else { f64::from_bits(1) },
                        _ => if i % 3 == 0 { f64::from(f32::from_bits(1)) } else { -0.25 },
                    }));
                    let mut block = input;
                    lanes::process(stages, 0, &mut cells, &batch, &mut block, parameters, at, &mut bank);
                    for v in 0..count {
                        let end = batch.ends[2 * v];
                        let mut expected: Planar = std::array::from_fn(|c| std::array::from_fn(|i| input[i][2 * v + c]));
                        let mut actual = expected;
                        reference(stages, &mut expected_states[v], &mut expected, end, parameters, at, &mut oracle_bank);
                        assert!(!super::process::<false>(stages, &mut scalar[v], &mut actual, end,
                            parameters, at, &mut [], &mut FilterContext { bank: &mut scalar_bank,
                                expression: None, reverbs: &mut [], convolutions: &mut [] }, None));
                        assert_eq!(actual.map(|c| c.map(f64::to_bits)), expected.map(|c| c.map(f64::to_bits)));
                        for c in 0..2 { for i in 0..end { assert_eq!(block[i][2 * v + c].to_bits(), expected[c][i].to_bits()); } }
                        for ((a, b), e) in scalar[v].iter().zip(cells[v].as_ref().unwrap().iter()).zip(&expected_states[v]) {
                            assert_eq!(bits(a), bits(e)); assert_eq!(bits(b), bits(e));
                        }
                    }
                    for i in len..BLOCK { assert_eq!(block[i].map(f64::to_bits), input[i].map(f64::to_bits)); }
                }
            }
        }
    }
}

#[test]
fn shared_dispatch_matches_frozen_pcm_and_state_bits() {
    let parameters = [ControlRamp::test_ramp(1., 0.1, 1, 193),
        ControlRamp::test_ramp(0.1, 0.9, 0, 101), ControlRamp::test_ramp(-0.8, 0.7, 0, 223),
        ControlRamp::test_ramp(0.25, 0.6, 0, 127), ControlRamp::test_ramp(0., 1., 0, 199)];
    for gain in [-1.5, 0., 0.75] { check(&[PreparedProcessor::Gain(gain)], &[], &[]); }
    check(&[PreparedProcessor::ControlGain(0)], &[], &parameters);
    check(&[PreparedProcessor::StereoMatrix([[0.3, -0.7], [1.2, 0.9]])], &[], &[]);
    for dry in [0., 0.5, 1.] {
        check(&[PreparedProcessor::Gainer { dry, gain: PreparedParameter::Control(0), k: f64::from(f32::from_bits(0x3a11a2b4)) }], &[], &parameters);
    }
    let mut bindings = Vec::new();
    // Width then pan map to parameter slots 1 and 2.
    bindings.push(ControlRange { control: crate::ControlId(0), low: 0., high: 1., ramp_frames: 0 });
    let stereo = StereoSettings { width: Parameter::Control(ControlRange { control: crate::ControlId(1), low: 0., high: 1., ramp_frames: 0 }),
        pan: Parameter::Control(ControlRange { control: crate::ControlId(2), low: -1., high: 1., ramp_frames: 0 }), pseudo: false }.compile(48000, &mut bindings);
    check(&[PreparedProcessor::StereoModeller { stereo, offset: 0 }], &[], &parameters);
    // A stateless inner stage avoids the existing shorter-voice bypass state limitation.
    check(&[PreparedProcessor::Mix { count: 1, lanes: [3, 0, 4] }, PreparedProcessor::Gain(-0.75)], &[], &parameters);
}

#[test]
fn shared_svf_matches_frozen_pcm_and_state_bits() {
    for rate in [44100, 48000, 96000] {
        for mode in [SvfMode::LowPass, SvfMode::HighPass, SvfMode::BandPass, SvfMode::Notch,
            SvfMode::AllPass, SvfMode::OnePoleLowPass, SvfMode::OnePoleHighPass] {
            for q in [0.2, 0.707, 4.] {
                for moving in [false, true] {
                    let parameters = [ControlRamp::test_ramp(40., f64::from(rate) * 0.49, 1, if moving { 193 } else { 0 })];
                    let mut bindings = Vec::new();
                    let filter = StateVariableFilter { mode, cutoff_hz: Parameter::Control(ControlRange {
                        control: crate::ControlId(0), low: 40., high: f64::from(rate) * 0.49, ramp_frames: 193 }), q: Parameter::Constant(q) }
                        .compile(rate, &mut bindings).unwrap();
                    check(&[PreparedProcessor::StateVariable(0)], &[filter], &parameters);
                }
            }
        }
    }
}
