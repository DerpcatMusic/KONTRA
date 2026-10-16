//! Positive normal-loader witnesses for explicit per-activation shared state.
//! Authored fixtures are not a UVI/Kontakt native-topology certificate.
mod support;
use sampler_core::lower::{LowerError, lower};
use sampler_core::{
    ControlValue, ControlWrite, Event, Input, Limits, Pcm, Prepared, Protocol, Runtime,
};
use sampler_ir as ir;

fn input(id: i32) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(id),
    }
}
fn asset(n: usize) -> ir::Asset {
    ir::Asset {
        location: ir::AssetLocation::Path(format!("authored-{n}.wav")),
        encoding: ir::Encoding::Wav,
        residency: ir::AssetResidency::Resident,
        root_key: None,
        loops: vec![],
    }
}
fn zone(n: usize) -> ir::Zone {
    ir::Zone {
        pitch: ir::KeyTracking::Fixed,
        velocity: ir::VelocityResponse::None,
        ..ir::Zone::new(ir::AssetRef(n))
    }
}
fn prepare(ir: &ir::Instrument, pcm: Vec<Pcm>) -> Prepared {
    lower(ir, 48000, pcm, |_, plan| Ok::<_, LowerError>(plan)).unwrap()
}
fn runtime(ir: &ir::Instrument, pcm: Vec<Pcm>) -> Runtime {
    let p = prepare(ir, pcm);
    let l = Limits::for_plan(&p, 16, 32);
    Runtime::new(p, l).unwrap()
}
fn shared_chain() -> ir::Chain {
    ir::Chain {
        scope: ir::Scope::Voice,
        pre_amplitude: vec![
            ir::Processor::Color(ir::fx::color::Config::rectifier(ir::Rectifier::Full)),
            ir::Processor::Filter(ir::Filter {
                kind: ir::FilterKind::LowPass { poles: 2 },
                cutoff: ir::Frequency::Hertz(2300.),
                resonance: ir::Resonance::Q(0.65),
            }),
        ],
        post_amplitude: vec![],
    }
}
fn graph(count: usize, chain: ir::Chain) -> ir::Instrument {
    ir::Instrument {
        assets: (0..count).map(asset).collect(),
        zones: (0..count).map(zone).collect(),
        chains: vec![chain],
        processing_scopes: vec![ir::ProcessingScope {
            members: (0..count).map(ir::ZoneRef).collect(),
            sum: ir::ProcessingSumLaw::OrderedF32,
            cadence: ir::ProcessingCadence::CoreCell64,
            chain: Some(ir::ChainRef(0)),
            amplitude: None,
            output: None,
        }],
        ..Default::default()
    }
}
fn pcm(rate: u32, frames: Vec<[f32; 2]>) -> Pcm {
    Pcm::new(rate, frames.into_boxed_slice()).unwrap()
}
fn wave(rate: u32, n: usize, sign: f32) -> Pcm {
    pcm(
        rate,
        (0..n)
            .map(|i| {
                let value = sign * ((i % 17) as f32 - 8.) / 16.;
                [value, value * -0.5]
            })
            .collect(),
    )
}
fn render(rt: &mut Runtime, frames: usize, block: usize) -> Vec<[f32; 2]> {
    let mut out = vec![[0.; 2]; frames];
    for part in out.chunks_mut(block) {
        rt.render(part).unwrap();
    }
    out
}
fn close(a: &[[f32; 2]], b: &[[f32; 2]]) {
    assert_eq!(a.len(), b.len());
    for (i, (a, b)) in a.iter().zip(b).enumerate() {
        for c in 0..2 {
            assert!(
                (a[c] - b[c]).abs() <= 2e-6,
                "frame {i} channel {c}: {} != {}",
                a[c],
                b[c]
            );
        }
    }
}
fn source_output(source: Pcm, z: ir::Zone, frames: usize) -> Vec<[f32; 2]> {
    let ir = ir::Instrument {
        assets: vec![asset(0)],
        zones: vec![z],
        ..Default::default()
    };
    let mut r = runtime(&ir, vec![source]);
    r.trigger(input(1), 60, 1.).unwrap();
    render(&mut r, frames, 17)
}

#[test]
fn independently_clocked_members_sum_before_one_shared_stateful_section() {
    let a = wave(48000, 1024, 0.75);
    let b = wave(24000, 512, -0.5);
    let mut ir = graph(2, shared_chain());
    ir.zones[1].playback.start = 5;
    let mut bz = ir.zones[1].clone();
    bz.asset = Some(ir::AssetRef(0));
    let aa = source_output(a.clone(), zone(0), 512);
    let bb = source_output(b.clone(), bz, 512);
    let expected_input: Vec<_> = aa
        .iter()
        .zip(&bb)
        .map(|(a, b)| [a[0] + b[0], a[1] + b[1]])
        .collect();
    // Independent signal oracle: ordinary one-source loader executes the same
    // shared section on an explicitly authored summed input. No product bake.
    let mut reference = ir::Instrument {
        assets: vec![asset(0)],
        zones: vec![ir::Zone {
            chain: Some(ir::ChainRef(0)),
            ..zone(0)
        }],
        chains: vec![shared_chain()],
        ..Default::default()
    };
    let mut rr = runtime(&reference, vec![pcm(48000, expected_input)]);
    rr.trigger(input(10), 60, 1.).unwrap();
    let expected = render(&mut rr, 512, 23);
    let mut rt = runtime(&ir, vec![a.clone(), b.clone()]);
    rt.trigger(input(1), 60, 1.).unwrap();
    assert_eq!((rt.processing_owner_count(), rt.voice_count()), (1, 2));
    let actual = render(&mut rt, 512, 31);
    close(&actual, &expected);
    // Causal contrast: cloned nonlinear sections per source are not equivalent.
    reference.assets.push(asset(1));
    reference.zones.push(ir::Zone {
        chain: Some(ir::ChainRef(0)),
        asset: Some(ir::AssetRef(1)),
        ..ir.zones[1].clone()
    });
    let mut wrong = runtime(&reference, vec![a, b]);
    wrong.trigger(input(1), 60, 1.).unwrap();
    let wrong = render(&mut wrong, 512, 31);
    assert!(
        actual
            .iter()
            .zip(wrong)
            .any(|(a, b)| (a[0] - b[0]).abs() > 0.01)
    );
}

#[test]
fn overlapping_attacks_have_independent_shared_histories() {
    let ir = graph(2, shared_chain());
    let sources = vec![wave(48000, 2048, 0.75), wave(32000, 2048, -0.5)];
    let mut rt = runtime(&ir, sources.clone());
    rt.trigger(input(1), 60, 1.).unwrap();
    let mut actual = render(&mut rt, 97, 17);
    rt.trigger(input(2), 60, 1.).unwrap();
    let mut ids = Vec::new();
    rt.visit_processing_owners(|o| ids.push((o.id, o.note, o.sources)));
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0].0, ids[1].0);
    assert_ne!(ids[0].1, ids[1].1);
    actual.extend(render(&mut rt, 415, 23));
    let mut a = runtime(&ir, sources.clone());
    a.trigger(input(1), 60, 1.).unwrap();
    let mut expected = render(&mut a, 512, 13);
    let mut b = runtime(&ir, sources);
    b.trigger(input(2), 60, 1.).unwrap();
    let second = render(&mut b, 415, 29);
    for (a, b) in expected[97..].iter_mut().zip(second) {
        a[0] += b[0];
        a[1] += b[1];
    }
    close(&actual, &expected);
}

#[test]
fn owner_release_keeps_default_member_gates_reading_through_release() {
    let mut ir = graph(
        2,
        ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![],
            post_amplitude: vec![],
        },
    );
    ir.modulators.push(ir::Modulator {
        scope: ir::Scope::Voice,
        source: ir::ModulationSource::Envelope(ir::Envelope {
            release: ir::Time::Milliseconds(1.),
            ..Default::default()
        }),
    });
    ir.processing_scopes[0].amplitude = Some(ir::ModulatorRef(0));
    let sources = vec![
        pcm(48000, vec![[0.25; 2]; 4096]),
        pcm(24000, vec![[0.125; 2]; 2048]),
    ];
    let mut rt = runtime(&ir, sources);
    let note = rt.trigger(input(1), 60, 1.).unwrap();
    assert!(render(&mut rt, 64, 17).iter().all(|x| x[0] > 0.3));
    rt.note_off_exact(note, None).unwrap();
    let release = render(&mut rt, 64, 7);
    // A default member gate cut on note-off would make this entire window zero.
    assert!(release[..16].iter().all(|x| x[0] > 0.1));
    assert!(release[16..47].iter().any(|x| x[0] > 0.));
    assert!(release[49..].iter().all(|x| *x == [0.; 2]));
    assert_eq!((rt.voice_count(), rt.processing_owner_count()), (0, 0));
}

#[test]
fn selected_subset_is_not_forced_to_start_unmatched_authored_members() {
    let mut ir = graph(2, shared_chain());
    ir.zones[1].keys = ir::KeyRange { low: 70, high: 80 };
    let mut rt = runtime(&ir, vec![wave(48000, 1024, 1.), wave(48000, 1024, -1.)]);
    rt.trigger(input(1), 60, 1.).unwrap();
    assert_eq!((rt.voice_count(), rt.processing_owner_count()), (1, 1));
    assert!(render(&mut rt, 128, 17).iter().any(|x| x[0] > 0.01));
}

#[test]
fn ordinary_shared_control_and_scheduled_recall_bind_the_actual_section() {
    let mut ir = graph(
        2,
        ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Gain(ir::Gain::UNITY)],
            post_amplitude: vec![],
        },
    );
    ir.controls.push(ir::Control {
        key: "authored-shared-gain".into(),
        label: "shared gain".into(),
        value: ir::ControlValue::Continuous {
            min: 0.,
            max: 1.,
            default: 1.,
            unit: ir::ControlUnit::None,
        },
        automation: ir::Automation::None,
    });
    ir.processor_controls.push(ir::ProcessorControl {
        control: ir::ControlRef(0),
        chain: ir::ChainRef(0),
        index: 0,
        parameter: ir::ProcessorParameter::Gain,
        ramp: ir::Time::ZERO,
    });
    let p = prepare(
        &ir,
        vec![
            pcm(48000, vec![[0.25; 2]; 4096]),
            pcm(48000, vec![[0.125; 2]; 4096]),
        ],
    );
    let control = p.controls()[0].id;
    let l = Limits::for_plan(&p, 8, 16);
    let mut rt = Runtime::new(p, l).unwrap();
    let note = rt.trigger(input(1), 60, 1.).unwrap();
    let plan = rt.active_plan();
    let unity = render(&mut rt, 64, 17);
    assert!((unity[63][0] - 0.375).abs() < 1e-6);
    let now = rt.now();
    let revision = rt.control_revision(plan).unwrap();
    rt.edit_controls(
        plan,
        Some(revision),
        &[ControlWrite {
            id: control,
            value: ControlValue::Real(0.5),
        }],
    )
    .unwrap();
    let half = render(&mut rt, 64, 17);
    assert!((half[63][0] - 0.1875).abs() < 1e-6);
    assert_eq!(rt.note_plan(note), Ok(plan));
    assert_eq!(rt.processing_owner_count(), 1);
    rt.schedule_event(
        rt.now() + 13,
        Event::Control(
            plan,
            ControlWrite {
                id: control,
                value: ControlValue::Real(0.),
            },
        ),
    )
    .unwrap();
    let mute = render(&mut rt, 64, 23);
    assert!(mute[..13].iter().any(|x| x[0] > 0.));
    assert!(mute[13..].iter().all(|x| *x == [0.; 2]));
    rt.recall_controls(
        plan,
        None,
        &[ControlWrite {
            id: control,
            value: ControlValue::Real(1.),
        }],
    )
    .unwrap();
    assert!((render(&mut rt, 64, 11)[63][0] - 0.375).abs() < 1e-6);
    assert!(rt.now() > now);
    assert_eq!((rt.voice_count(), rt.processing_owner_count()), (2, 1));
}

#[test]
fn late_invalid_typed_control_row_keeps_owner_source_and_revision_unchanged() {
    let mut ir = graph(
        2,
        ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Gain(ir::Gain::UNITY)],
            post_amplitude: vec![],
        },
    );
    ir.controls.push(ir::Control {
        key: "owner-gain".into(),
        label: "".into(),
        value: ir::ControlValue::Continuous {
            min: 0.,
            max: 1.,
            default: 1.,
            unit: ir::ControlUnit::None,
        },
        automation: ir::Automation::None,
    });
    ir.processor_controls.push(ir::ProcessorControl {
        control: ir::ControlRef(0),
        chain: ir::ChainRef(0),
        index: 0,
        parameter: ir::ProcessorParameter::Gain,
        ramp: ir::Time::ZERO,
    });
    let p = prepare(&ir, vec![wave(48000, 4096, 1.), wave(32000, 4096, -0.5)]);
    let control = p.controls()[0].id;
    let l = Limits::for_plan(&p, 8, 16);
    let mut rt = Runtime::new(p, l).unwrap();
    rt.trigger(input(1), 60, 1.).unwrap();
    let plan = rt.active_plan();
    let revision = rt.control_revision(plan).unwrap();
    let now = rt.now();
    let error = rt.edit_controls(
        plan,
        Some(revision),
        &[ControlWrite {
            id: control,
            value: ControlValue::Toggle(false),
        }],
    );
    assert!(error.is_err());
    assert_eq!(rt.now(), now);
    assert_eq!(rt.control_revision(plan), Ok(revision));
    assert_eq!(rt.control_value(plan, control), Ok(ControlValue::Real(1.)));
    assert_eq!((rt.voice_count(), rt.processing_owner_count()), (2, 1));
}

#[test]
fn member_capacity_failure_publishes_no_partial_processing_owner() {
    let ir = graph(2, shared_chain());
    let p = prepare(&ir, vec![wave(48000, 1024, 1.), wave(48000, 1024, -0.5)]);
    let mut l = Limits::for_plan(&p, 8, 1);
    l.voices = 1;
    let mut rt = Runtime::new(p, l).unwrap();
    assert!(rt.trigger(input(1), 60, 1.).is_err());
    assert_eq!(
        (rt.voice_count(), rt.processing_owner_count(), rt.now()),
        (0, 0, 0)
    );
    assert!(render(&mut rt, 64, 17).iter().all(|x| *x == [0.; 2]));
}

#[test]
fn owner_audio_path_has_no_callback_heap_calls() {
    let ir = graph(2, shared_chain());
    let mut rt = runtime(&ir, vec![wave(48000, 4096, 1.), wave(32000, 4096, -0.5)]);
    let mut out = [[0.; 2]; 257];
    support::without_heap(|| {
        let note = rt.trigger(input(1), 60, 1.).unwrap();
        rt.render(&mut out).unwrap();
        rt.note_off_exact(note, None).unwrap();
        rt.render(&mut out).unwrap();
        rt.panic();
    });
    assert_eq!((rt.voice_count(), rt.processing_owner_count()), (0, 0));
}

#[test]
fn authored_member_order_defines_each_binary32_sum_boundary() {
    let mut ir = graph(
        3,
        ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![],
            post_amplitude: vec![],
        },
    );
    ir.processing_scopes[0].members = vec![ir::ZoneRef(2), ir::ZoneRef(0), ir::ZoneRef(1)];
    let sources: Vec<_> = [4_194_304., -4_194_304., 0.125]
        .into_iter()
        .map(|v| pcm(48000, vec![[v; 2]; 128]))
        .collect();
    let mut rt = runtime(&ir, sources.clone());
    rt.trigger(input(1), 60, 1.).unwrap();
    // Physical slot order gives 0.125; authored 0.125 + large - large gives0.
    assert!(render(&mut rt, 64, 17).iter().all(|x| *x == [0.; 2]));

    // Same immutable sources, alternate authored permutation. A dropped or
    // muted owner cannot satisfy this nonzero positive control.
    ir.processing_scopes[0].members = vec![ir::ZoneRef(0), ir::ZoneRef(1), ir::ZoneRef(2)];
    let mut positive = runtime(&ir, sources);
    positive.trigger(input(1), 60, 1.).unwrap();
    assert!(
        render(&mut positive, 64, 17)
            .iter()
            .all(|x| *x == [0.125; 2])
    );
}

#[test]
fn owner_amplitude_and_real_taps_keep_their_declared_bus_boundaries() {
    let mut ir = graph(
        2,
        ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Gain(ir::Gain::Linear(2.))],
            post_amplitude: vec![ir::Processor::Gain(ir::Gain::Linear(0.5))],
        },
    );
    ir.modulators.push(ir::Modulator {
        scope: ir::Scope::Voice,
        source: ir::ModulationSource::Envelope(ir::Envelope {
            sustain: 0.5,
            ..Default::default()
        }),
    });
    ir.processing_scopes[0].amplitude = Some(ir::ModulatorRef(0));
    ir.processing_scopes[0].output = Some(ir::BusRef(0));
    ir.buses = vec![
        ir::Bus {
            name: "main muted".into(),
            chain: None,
            sends: vec![],
            output: ir::Output::Master,
            gain: ir::Gain::Linear(0.),
        },
        ir::Bus {
            name: "tap".into(),
            chain: None,
            sends: vec![],
            output: ir::Output::Master,
            gain: ir::Gain::UNITY,
        },
    ];
    ir.voice_send_taps.push(ir::VoiceSendTap {
        chain: ir::ChainRef(0),
        position: ir::VoiceSendPosition::BeforeAmplitude(1),
        bus: ir::BusRef(1),
        gain: ir::Gain::UNITY,
        gain_control: None,
        bypass: false,
        bypass_control: None,
        ramp: ir::Time::ZERO,
    });
    let sources = vec![
        pcm(48000, vec![[0.25; 2]; 1024]),
        pcm(48000, vec![[0.125; 2]; 1024]),
    ];
    let mut before = runtime(&ir, sources.clone());
    before.trigger(input(1), 60, 1.).unwrap();
    assert!((render(&mut before, 64, 17)[63][0] - 0.75).abs() < 1e-6);
    ir.voice_send_taps[0].position = ir::VoiceSendPosition::AfterAmplitude(0);
    let mut after = runtime(&ir, sources);
    after.trigger(input(1), 60, 1.).unwrap();
    assert!((render(&mut after, 64, 17)[63][0] - 0.375).abs() < 1e-6);
}

#[test]
fn member_local_tap_retains_note_expression_while_main_is_captured_for_owner() {
    let mut ir = graph(
        1,
        ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Gain(ir::Gain::Linear(0.))],
            post_amplitude: vec![],
        },
    );
    ir.chains.push(ir::Chain {
        scope: ir::Scope::Voice,
        pre_amplitude: vec![ir::Processor::Gain(ir::Gain::UNITY)],
        post_amplitude: vec![],
    });
    ir.zones[0].chain = Some(ir::ChainRef(1));
    ir.processing_scopes[0].output = Some(ir::BusRef(0));
    ir.buses = vec![
        ir::Bus {
            name: "muted final".into(),
            chain: None,
            sends: vec![],
            output: ir::Output::Master,
            gain: ir::Gain::Linear(0.),
        },
        ir::Bus {
            name: "source tap".into(),
            chain: None,
            sends: vec![],
            output: ir::Output::Master,
            gain: ir::Gain::UNITY,
        },
    ];
    ir.voice_send_taps.push(ir::VoiceSendTap {
        chain: ir::ChainRef(1),
        position: ir::VoiceSendPosition::AfterAmplitude(0),
        bus: ir::BusRef(1),
        gain: ir::Gain::UNITY,
        gain_control: None,
        bypass: false,
        bypass_control: None,
        ramp: ir::Time::ZERO,
    });
    let source = pcm(48000, vec![[0.25; 2]; 1024]);
    let mut candidate = runtime(&ir, vec![source.clone()]);
    let note = candidate.trigger(input(1), 60, 1.).unwrap();
    let id = candidate.expression_id(note).unwrap();
    candidate
        .set_expression(
            id,
            sampler_core::Expression {
                gain: 0.25,
                pan: 0.5,
                ..Default::default()
            },
        )
        .unwrap();
    let actual = render(&mut candidate, 64, 17);
    // The legacy source tap is the control; final source output is muted on a
    // normal bus without affecting its explicit source-local send tap.
    ir.processing_scopes.clear();
    let mut group = ir::Group::default();
    group.output = ir::Output::Bus(ir::BusRef(0));
    ir.groups.push(group);
    ir.zones[0].group = Some(ir::GroupRef(0));
    let mut reference = runtime(&ir, vec![source]);
    let note = reference.trigger(input(1), 60, 1.).unwrap();
    let id = reference.expression_id(note).unwrap();
    reference
        .set_expression(
            id,
            sampler_core::Expression {
                gain: 0.25,
                pan: 0.5,
                ..Default::default()
            },
        )
        .unwrap();
    close(&actual, &render(&mut reference, 64, 17));
    assert!((actual[63][0] - 0.03125).abs() < 1e-6);
    assert!((actual[63][1] - 0.0625).abs() < 1e-6);
}

/// Uses the same pinned public SourceModulator/SourceAhdsr provenance and
/// parameter geometry as lower.rs::native_primary_fixture. The data is an
/// existing supported native primary, not a guessed UVI amplitude mapping.
fn native_primary_scope_fixture() -> ir::Instrument {
    let mut instrument = ir::Instrument {
        assets: vec![asset(0)],
        groups: vec![ir::Group::default()],
        zones: vec![ir::Zone {
            group: Some(ir::GroupRef(0)),
            amplitude: Some(ir::ModulatorRef(0)),
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        modulators: vec![ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Envelope(ir::Envelope {
                attack: ir::Time::Milliseconds(125.01293),
                attack_shape: {
                    let b = f64::from((-20_000f64.ln()).exp() as f32);
                    ir::Curve::Exponential(((1. + b) / b).ln())
                },
                release_shape: ir::Curve::Exponential((3.0f64 / 43.).ln()),
                release: ir::Time::Milliseconds(250.0013),
                ..Default::default()
            }),
        }],
        ..Default::default()
    };
    instrument
        .source_indices
        .modulators
        .push(ir::SourceModulator {
            group: 0,
            slot: 7,
            external: false,
            name: "primary".into(),
            runtime: Some(ir::ModulatorRef(0)),
            settings: None,
        });
    instrument.source_indices.ahdsrs.push(ir::SourceAhdsr {
        group: 0,
        slot: 7,
        attack_ms: 125.01293,
        attack_curve: -1.,
        hold_ms: 0.,
        decay_ms: 0.,
        sustain: 1.,
        release_ms: 250.0013,
        ahd_only: false,
        native_amplitude: true,
    });
    instrument
}

#[test]
fn native_amplitude_provenance_refuses_owner_migration_but_preserves_source_law() {
    use sampler_core::v1_voice_controls::{Ahdsr, ControlDescription, ControlPlan, ControlState};
    // Both the existing singleton and a source-local native amplitude beneath
    // an identity owner stay on the supported pinned production evaluator.
    for identity_owner in [false, true] {
        let mut instrument = native_primary_scope_fixture();
        if identity_owner {
            instrument.processing_scopes.push(ir::ProcessingScope {
                members: vec![ir::ZoneRef(0)],
                sum: ir::ProcessingSumLaw::OrderedF32,
                cadence: ir::ProcessingCadence::CoreCell64,
                chain: None,
                amplitude: None,
                output: None,
            });
        }
        let reference = ControlPlan::prepare(
            ControlDescription {
                amplitude: Ahdsr::from(&instrument.source_indices.ahdsrs[0]),
                native_amplitude: true,
                ..Default::default()
            },
            48000.,
        )
        .unwrap();
        let mut expected = ControlState::new(&reference);
        let mut rt = runtime(&instrument, vec![pcm(48000, vec![[1.; 2]; 4800])]);
        let note = rt.trigger(input(1), 60, 1.).unwrap();
        let mut any_positive = false;
        for (block, n) in [1usize, 7, 31, 32, 33, 64, 17, 3].into_iter().enumerate() {
            if block == 5 {
                expected.release(&reference);
                rt.note_off_exact(note, None).unwrap();
            }
            let mut output = [[0.; 2]; 128];
            let mut amplitudes = [0.; 128];
            let mut positions = [0; 128];
            expected
                .render(
                    &reference,
                    120.,
                    1.,
                    &mut amplitudes[..n],
                    None,
                    None,
                    &mut positions[..n],
                )
                .unwrap();
            support::without_heap(|| rt.render(&mut output[..n]).unwrap());
            for (frame, value) in output[..n].iter().zip(&amplitudes) {
                assert_eq!(
                    frame.map(f32::to_bits),
                    [value.to_bits(); 2],
                    "identity_owner={identity_owner} block={block}"
                );
                any_positive |= *value > 0.;
            }
        }
        assert!(any_positive);
        rt.panic();
    }
    // The same declaration cannot silently become a generic shared owner
    // envelope merely by clearing the zone's source identity during lowering.
    let mut instrument = native_primary_scope_fixture();
    instrument.processing_scopes.push(ir::ProcessingScope {
        members: vec![ir::ZoneRef(0)],
        sum: ir::ProcessingSumLaw::OrderedF32,
        cadence: ir::ProcessingCadence::CoreCell64,
        chain: None,
        amplitude: Some(ir::ModulatorRef(0)),
        output: None,
    });
    assert!(matches!(
        lower(
            &instrument,
            48000,
            vec![pcm(48000, vec![[1.; 2]; 4800])],
            |_, p| Ok::<_, LowerError>(p)
        ),
        Err(LowerError::Unsupported {
            feature: sampler_core::lower::Feature::Controls,
            ..
        })
    ));
}
