//! Source-free diagnostics. The key interval never excludes rendering/selection.
mod support;
use sampler_core::trace::{SignalTraceScope, onset::Record};
use sampler_core::*;

fn input(key: u8) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: None,
    }
}
fn region(sample: usize, key: u8) -> Region {
    Region {
        sample,
        key_low: key,
        key_high: key,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::GATE,
        playback: Playback::default(),
    }
}
fn prepared(outside: usize, generated: bool) -> Prepared {
    let a = Pcm::new(48000, vec![[0.125, 0.]; 512].into()).unwrap();
    let b = Pcm::new(48000, vec![[0., 0.25]; 512].into()).unwrap();
    let mut regions = vec![region(0, 60)];
    regions.extend((0..outside).map(|_| region(1, 72)));
    let plan = Prepared::new(48000, vec![a, b], regions, outside + 1).unwrap();
    if !generated {
        return plan;
    }
    let play = |transpose| Instruction::Play {
        transpose,
        velocity: Velocity::Scale(1.),
        inheritance: Inheritance::Linked,
        duration: Duration::FramesOrGate(512),
    };
    plan.with_programs(
        vec![Program::new(vec![play(0), play(12), Instruction::End]).unwrap()],
        Some(0),
    )
    .unwrap()
}
fn run(plan: Prepared, generated: bool) -> (Runtime, Vec<Frame>) {
    let limits = Limits::for_plan(&plan, 8, 16);
    let mut runtime = Runtime::new(plan, limits).unwrap();
    let mut output = vec![[0.; 2]; 512];
    support::without_heap(|| {
        runtime.trigger(input(60), 60, 1.).unwrap();
        if !generated {
            runtime.trigger(input(72), 72, 1.).unwrap();
        }
        for chunk in output.chunks_mut(128) {
            runtime.render(chunk).unwrap();
        }
    });
    (runtime, output)
}

#[test]
fn simultaneous_inside_and_outside_scope_voices_both_sound_and_keep_every_birth_window() {
    for generated in [false, true] {
        let (plan, reader) = prepared(1, generated)
            .with_scoped_signal_trace_reader(8192, SignalTraceScope::new(60, 60, 128).unwrap())
            .unwrap();
        let (_traced, on) = run(plan, generated);
        let (_plain, off) = run(prepared(1, generated), generated);
        assert_eq!(on, off, "all rendered PCM must be identical");
        assert!(on.iter().any(|f| f[0] != 0.));
        assert!(
            on.iter().any(|f| f[1] != 0.),
            "outside-scope voice must audibly contribute"
        );
        let onset = reader.onset_reader();
        let rows = onset.drain().unwrap();
        let births: Vec<_> = rows
            .iter()
            .filter_map(|r| match r {
                Record::Birth(b) => Some(b),
                _ => None,
            })
            .collect();
        assert_eq!(births.len(), 2);
        assert_eq!(
            rows.iter()
                .filter(|r| matches!(r, Record::Window(_)))
                .count(),
            2
        );
        let inside = births.iter().find(|b| b.key == 60).unwrap();
        let outside = births.iter().find(|b| b.key == 72).unwrap();
        if generated {
            assert!(inside.parent_event.is_some());
            assert_eq!(inside.parent_event, outside.parent_event);
        }
        assert!(reader.graph.contribution_node(inside.zone).is_some());
        assert!(
            reader.graph.contribution_node(outside.zone).is_none(),
            "actual generated birth, not input key, determines coverage"
        );
        let signal = reader.drain();
        assert!(
            signal
                .iter()
                .any(|r| r.contribution && r.identity.voice == Some(inside.voice))
        );
        assert!(
            !signal
                .iter()
                .any(|r| r.contribution && r.identity.voice == Some(outside.voice))
        );
        assert_eq!((reader.dropped(), onset.dropped()), (0, 0));
    }
}

#[test]
fn widening_is_explicit_and_legacy_graph_has_every_region() {
    let full = prepared(1, false)
        .with_signal_trace(8192)
        .unwrap()
        .signal_trace_reader()
        .unwrap();
    let (_, wide) = prepared(1, false)
        .with_scoped_signal_trace_reader(8192, SignalTraceScope::new(60, 72, 128).unwrap())
        .unwrap();
    let mut original = serde_json::to_value(&*full.graph).unwrap();
    let mut scoped = serde_json::to_value(&*wide.graph).unwrap();
    assert!(original.get("scope").is_none());
    scoped.as_object_mut().unwrap().remove("scope");
    assert_eq!(original.take(), scoped);
    assert!(wide.graph.contribution_node(2).is_some());
    let (_, audio) = run(prepared(1, false), false);
    let (plan, _) = prepared(1, false)
        .with_scoped_signal_trace_reader(8192, SignalTraceScope::new(60, 72, 128).unwrap())
        .unwrap();
    assert_eq!(audio, run(plan, false).1);
}

#[test]
fn excluded_large_region_set_allocates_no_additional_graph_storage() {
    let build = |outside| {
        let plan = prepared(outside, false);
        let mut result = None;
        let bytes = support::allocated_bytes(|| {
            result =
                Some(plan.with_scoped_signal_trace_reader(
                    128,
                    SignalTraceScope::new(60, 60, 128).unwrap(),
                ))
        });
        let (_, reader) = result.unwrap().unwrap();
        (bytes, reader.graph.nodes.len())
    };
    let small = build(1);
    let large = build(65537); // Entire legacy graph exceeds 65,536 nodes.
    assert_eq!(
        small, large,
        "filter before constructing region nodes or growing their storage"
    );
}

#[test]
fn graph_over_budget_is_refused_before_any_builder_allocation() {
    let plan = prepared(1, false);
    let mut result = None;
    let bytes = support::allocated_bytes(|| {
        result = Some(
            plan.with_scoped_signal_trace_reader(128, SignalTraceScope::new(60, 72, 1).unwrap()),
        )
    });
    assert_eq!(bytes, 0, "preflight must precede even the fixed host graph");
    assert!(matches!(result.unwrap(), Err(Error::Capacity)));
}

fn complex() -> Prepared {
    let id = ControlId(42);
    let range = ControlRange {
        control: id,
        low: 0.25,
        high: 1.25,
        ramp_frames: 0,
    };
    prepared(1, false)
        .with_controls(vec![ControlDefinition {
            id,
            domain: ControlDomain::Real { min: 0., max: 1. },
            default: ControlValue::Real(0.5),
        }])
        .unwrap()
        .with_buses(
            vec![
                Bus {
                    processors: vec![Processor::ControlGain(range)],
                    sends: vec![BusSend {
                        bus: Some(1),
                        gain: 1.,
                    }],
                    tail_frames: 0,
                },
                Bus {
                    processors: vec![Processor::Gain(0.5)],
                    sends: vec![BusSend {
                        bus: None,
                        gain: 1.,
                    }],
                    tail_frames: 0,
                },
                Bus {
                    processors: vec![Processor::Gain(2.)],
                    sends: vec![],
                    tail_frames: 0,
                }, // unused bus retained
            ],
            vec![Some(0), Some(0)],
        )
        .unwrap()
        .with_input_bus(0)
        .unwrap()
        .with_voice_chains(
            vec![
                VoiceChain::new(
                    vec![
                        Processor::ControlGain(range),
                        Processor::Branch {
                            count: 1,
                            gain: 0.5,
                            first: true,
                            last: true,
                        },
                        Processor::Gain(2.),
                    ],
                    vec![Processor::Gain(0.5)],
                    0,
                )
                .unwrap()
                .with_taps(vec![VoiceSendTap {
                    position: VoiceSendPosition::AfterAmplitude(1),
                    bus: 1,
                    gain: Parameter::Constant(0.25),
                    bypass: Parameter::Constant(0.),
                }])
                .unwrap(),
            ],
            vec![Some(0), Some(0)],
        )
        .unwrap()
}

#[test]
fn all_buses_taps_and_control_defaults_are_counted_before_construction() {
    let full = complex()
        .with_signal_trace(8192)
        .unwrap()
        .signal_trace_reader()
        .unwrap();
    let count = full.graph.nodes.len();
    let (_, scoped) = complex()
        .with_scoped_signal_trace_reader(8192, SignalTraceScope::new(0, 127, count).unwrap())
        .unwrap();
    let mut graph = serde_json::to_value(&*scoped.graph).unwrap();
    graph.as_object_mut().unwrap().remove("scope");
    assert_eq!(serde_json::to_value(&*full.graph).unwrap(), graph);
    assert!(
        scoped.graph.nodes.iter().any(|n| n.bus == Some(2)),
        "unreferenced buses remain present"
    );
    let plan = complex();
    let mut result = None;
    let bytes = support::allocated_bytes(|| {
        result = Some(plan.with_scoped_signal_trace_reader(
            8192,
            SignalTraceScope::new(0, 127, count - 1).unwrap(),
        ))
    });
    assert_eq!(bytes, 0);
    assert!(matches!(result.unwrap(), Err(Error::Capacity)));
    let (_, a) = run(complex(), false);
    let (plan, _) = complex()
        .with_scoped_signal_trace_reader(8192, SignalTraceScope::new(60, 60, count).unwrap())
        .unwrap();
    assert_eq!(a, run(plan, false).1);
}

#[test]
fn invalid_scope_cannot_bypass_validation_by_constructing_public_fields() {
    for scope in [
        SignalTraceScope {
            prepared_key_low: 61,
            prepared_key_high: 60,
            max_nodes: 128,
        },
        SignalTraceScope {
            prepared_key_low: 0,
            prepared_key_high: 128,
            max_nodes: 128,
        },
        SignalTraceScope {
            prepared_key_low: 0,
            prepared_key_high: 127,
            max_nodes: 65537,
        },
    ] {
        assert!(matches!(
            prepared(1, false).with_scoped_signal_trace_reader(128, scope),
            Err(Error::InvalidInput)
        ));
    }
    assert!(
        prepared(1, false)
            .with_scoped_signal_trace_reader(65537, SignalTraceScope::new(60, 60, 128).unwrap())
            .is_err()
    );
    assert!(
        prepared(1, false)
            .with_scoped_signal_trace_reader(0, SignalTraceScope::new(60, 60, 128).unwrap())
            .is_err()
    );
}

#[test]
fn selected_region_budget_also_refuses_before_graph_allocation() {
    let plan = prepared(300, false);
    let mut result = None;
    let bytes = support::allocated_bytes(|| {
        result = Some(
            plan.with_scoped_signal_trace_reader(128, SignalTraceScope::new(60, 72, 128).unwrap()),
        )
    });
    assert_eq!(bytes, 0);
    assert!(matches!(result.unwrap(), Err(Error::Capacity)));
}
#[test]
fn empty_contribution_scope_still_renders_and_emits_all_births_and_windows() {
    let (plan, reader) = prepared(1, false)
        .with_scoped_signal_trace_reader(8192, SignalTraceScope::new(61, 61, 128).unwrap())
        .unwrap();
    let (_, on) = run(plan, false);
    let (_, off) = run(prepared(1, false), false);
    assert_eq!(on, off);
    let rows = reader.onset_reader().drain().unwrap();
    let births: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            Record::Birth(b) => Some(b),
            _ => None,
        })
        .collect();
    assert_eq!(births.len(), 2);
    assert!(
        births
            .iter()
            .all(|b| reader.graph.contribution_node(b.zone).is_none())
    );
    assert_eq!(
        rows.iter()
            .filter(|r| matches!(r, Record::Window(_)))
            .count(),
        2
    );
    assert_eq!(reader.dropped(), 0);
    assert!(!reader.drain().iter().any(|r| r.contribution));
}

#[test]
fn duplicate_source_zone_ids_never_borrow_an_in_scope_contribution_graph() {
    let plan = prepared(1, false).with_source_zones(vec![7, 7]).unwrap();
    let (plan, reader) = plan
        .with_scoped_signal_trace_reader(8192, SignalTraceScope::new(60, 60, 128).unwrap())
        .unwrap();
    let (_, on) = run(plan, false);
    let (_, off) = run(
        prepared(1, false).with_source_zones(vec![7, 7]).unwrap(),
        false,
    );
    assert_eq!(on, off);
    let rows = reader.onset_reader().drain().unwrap();
    let births: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            Record::Birth(b) => Some(b),
            _ => None,
        })
        .collect();
    assert_eq!(births.len(), 2);
    assert!(
        births
            .iter()
            .all(|b| reader.graph.contribution_node(b.zone).is_none())
    );
    assert!(!reader.drain().iter().any(|r| r.contribution));
    assert_eq!(reader.dropped(), 0);
}
