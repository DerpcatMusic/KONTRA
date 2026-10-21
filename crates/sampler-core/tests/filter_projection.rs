//! Final-topology validation for compiled per-voice modulation targets.
//! Invalid topology edits must fail on control before a runtime can render them.
use sampler_core::*;
mod support;

fn source_plan() -> Prepared {
    let pcm = (0..4096)
        .map(|i| {
            [
                (i as f32 * 0.071).sin() * 0.125,
                (i as f32 * 0.037).cos() * -0.1,
            ]
        })
        .collect();
    Prepared::new(
        48000,
        vec![Pcm::new(48000, pcm).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 0.75,
            envelope: Envelope::new(7, 3, 31, 0.6, 19).unwrap(),
            playback: Playback {
                loop_range: Some(Loop {
                    start: 0,
                    end: 4096,
                    passes: None,
                    shape: LoopShape::Wrap,
                    mode: LoopMode::Continuous,
                }),
                ..Playback::default()
            },
        }],
        1,
    )
    .unwrap()
    .with_velocity_curves(vec![VelocityCurve::Constant])
    .unwrap()
}

fn svf(hz: f64) -> Processor {
    Processor::StateVariable(StateVariableFilter {
        mode: SvfMode::LowPass,
        cutoff_hz: Parameter::Constant(hz),
        q: Parameter::Constant(0.7),
    })
}

fn native() -> Processor {
    Processor::Filter(FilterSettings::Ladder(LadderSettings {
        cutoff: Parameter::Constant(0.4),
        resonance: Parameter::Constant(0.2),
        gain: Parameter::Constant(0.),
        record_version: 99,
        taps: 4,
    }))
}

fn chain(plan: Prepared, stages: Vec<Processor>) -> Prepared {
    plan.with_voice_chains(
        vec![VoiceChain::new(vec![], stages, 0).unwrap()],
        vec![Some(0)],
    )
    .unwrap()
}

fn modulation(plan: Prepared, target: ModTarget) -> Prepared {
    plan.with_voice_modulation(
        vec![ModProgram {
            sources: vec![ModSource::Constant],
            routes: vec![ModRoute::new(0, target, 6.)],
            ..ModProgram::default()
        }],
        vec![Some(0)],
        vec![0],
    )
    .unwrap()
}

/// Genuine RED: the old public builder accepts a dangling target; do not render it
/// or use a callback panic as the desired rejection path.
#[test]
fn removing_addressed_filter_after_modulation_rejects_preparation() {
    for target in [
        ModTarget::ProcessorCutoff(0),
        ModTarget::ProcessorResonance(0),
    ] {
        let valid = modulation(chain(source_plan(), vec![svf(900.)]), target);
        assert!(
            matches!(
                valid.with_voice_chains(vec![], vec![None]),
                Err(Error::InvalidInput)
            ),
            "a consumed builder must not publish dangling target {target:?}"
        );
    }
}

#[test]
fn shrinking_filter_topology_after_modulation_rejects_removed_index() {
    let valid = modulation(
        chain(source_plan(), vec![svf(900.), svf(2200.)]),
        ModTarget::ProcessorCutoff(1),
    );
    let result = valid.with_voice_chains(
        vec![VoiceChain::new(vec![], vec![svf(1500.)], 0).unwrap()],
        vec![Some(0)],
    );
    assert!(matches!(result, Err(Error::InvalidInput)));
}

#[test]
fn replacing_addressed_filter_with_other_kind_rejects_preparation() {
    let sv = modulation(
        chain(source_plan(), vec![svf(900.)]),
        ModTarget::ProcessorCutoff(0),
    );
    assert!(matches!(
        sv.with_voice_chains(
            vec![VoiceChain::new(vec![], vec![native()], 0).unwrap()],
            vec![Some(0)]
        ),
        Err(Error::InvalidInput)
    ));
    let ni = modulation(
        chain(source_plan(), vec![native()]),
        ModTarget::ProcessorNativeCutoff(0),
    );
    assert!(matches!(
        ni.with_voice_chains(
            vec![VoiceChain::new(vec![], vec![svf(900.)], 0).unwrap()],
            vec![Some(0)]
        ),
        Err(Error::InvalidInput)
    ));
}

fn input() -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(1),
    }
}

fn limits() -> Limits {
    Limits {
        notes: 1,
        channels: 0,
        performances: 1,
        families: 1,
        voices: 1,
        expressions: 1,
        decisions: 0,
        commands: 1,
        behaviors: 0,
        behavior_cells: 0,
        behavior_fuel: 0,
        note_cells: 0,
    }
}

fn public_state(runtime: &Runtime) -> (u64, usize, usize, Vec<(u32, u8, u8, u8, u32)>) {
    (
        runtime.now(),
        runtime.voice_count(),
        runtime.note_count(),
        runtime
            .voice_taps()
            .into_iter()
            .flatten()
            .map(|v| (v.group, v.key, v.velocity, v.phase, v.level.to_bits()))
            .collect(),
    )
}

/// A legal replacement retains its explicit global target index; constructing
/// the same final topology before modulation is an independent builder-order oracle.
#[test]
fn compatible_replacement_matches_fresh_final_topology_pcm_and_state() {
    let actual = modulation(
        chain(source_plan(), vec![svf(900.), svf(2200.)]),
        ModTarget::ProcessorCutoff(1),
    )
    .with_voice_chains(
        vec![VoiceChain::new(vec![], vec![svf(1800.), svf(4100.)], 0).unwrap()],
        vec![Some(0)],
    )
    .unwrap();
    let expected = modulation(
        chain(source_plan(), vec![svf(1800.), svf(4100.)]),
        ModTarget::ProcessorCutoff(1),
    );
    let mut actual = Runtime::new(actual, limits()).unwrap();
    let mut expected = Runtime::new(expected, limits()).unwrap();
    support::without_heap(|| {
        actual.trigger(input(), 60, 1.).unwrap();
        expected.trigger(input(), 60, 1.).unwrap();
    });
    let mut a = [[0.; 2]; 128];
    let mut b = a;
    for (step, n) in [1, 7, 64, 17, 128, 63, 64, 3, 19, 128, 64]
        .into_iter()
        .enumerate()
    {
        support::without_heap(|| {
            if step == 8 {
                actual.note_off(input(), None).unwrap();
                expected.note_off(input(), None).unwrap();
            }
            actual.render(&mut a[..n]).unwrap();
            expected.render(&mut b[..n]).unwrap();
        });
        assert_eq!(
            a[..n]
                .iter()
                .map(|x| x.map(f32::to_bits))
                .collect::<Vec<_>>(),
            b[..n]
                .iter()
                .map(|x| x.map(f32::to_bits))
                .collect::<Vec<_>>()
        );
        assert_eq!(public_state(&actual), public_state(&expected));
        for rt in [&actual, &expected] {
            assert_eq!(rt.stats().nonfinite_frames, 0);
            assert_eq!(rt.stats().stream_underruns, 0);
        }
    }
    assert_eq!(actual.voice_count(), 0);
}

#[test]
fn removing_registered_dsp_lane_after_modulation_rejects_preparation() {
    const LEVEL: ControlId = ControlId(0xf123);
    let address = ParameterAddress {
        scope: ParameterScope::Voice,
        node: 7,
        parameter: 17,
    };
    let mut registry = ParameterRegistry::default();
    registry
        .register(ParameterDescriptor {
            address,
            control: LEVEL,
            name: "authored gain".into(),
            role: ParameterRole::Gain,
            unit: ParameterUnit::Linear,
            range: [0., 1.],
            default: 0.25,
            law: ParameterLaw::Linear,
            display: Default::default(),
        })
        .unwrap();
    let valid = source_plan()
        .with_controls(vec![ControlDefinition {
            id: LEVEL,
            domain: ControlDomain::Real { min: 0., max: 1. },
            default: ControlValue::Real(0.25),
        }])
        .unwrap();
    let valid = chain(
        valid,
        vec![Processor::ControlGain(ControlRange {
            control: LEVEL,
            low: 0.,
            high: 1.,
            ramp_frames: 0,
        })],
    )
    .with_parameter_registry(registry)
    .unwrap();
    let valid = modulation(valid, address);
    assert!(matches!(
        valid.with_voice_chains(vec![], vec![None]),
        Err(Error::InvalidInput)
    ));
}
