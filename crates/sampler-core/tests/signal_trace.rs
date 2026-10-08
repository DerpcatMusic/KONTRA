mod support;
use sampler_core::{
    Bus, BusSend, Envelope, Input, Limits, Pcm, Playback, Prepared, Processor, Protocol, Region,
    Runtime, VelocityCurve, VoiceChain,
};
fn input() -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    }
}
fn plan(traced: bool, records: usize) -> Prepared {
    let pcm = Pcm::new(48000, vec![[0.125; 2]; 4096].into_boxed_slice()).unwrap();
    let mut p = Prepared::new(
        48000,
        vec![pcm],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 0.4,
            envelope: Envelope::default(),
            playback: Playback::default(),
        }],
        1,
    )
    .unwrap()
    .with_velocity_curves(vec![VelocityCurve::Linear])
    .unwrap()
    .with_voice_chains(
        vec![VoiceChain::new(vec![Processor::Gain(2.)], vec![Processor::Gain(0.5)], 0).unwrap()],
        vec![Some(0)],
    )
    .unwrap()
    .with_buses(
        vec![Bus {
            processors: vec![Processor::Gain(3.)],
            sends: vec![BusSend {
                bus: None,
                gain: 0.5,
            }],
            tail_frames: 0,
        }],
        vec![Some(0)],
    )
    .unwrap();
    if traced {
        p = p.with_signal_trace(records).unwrap();
    }
    p
}
fn limits() -> Limits {
    Limits {
        notes: 8,
        channels: 1,
        performances: 1,
        families: 8,
        expressions: 8,
        voices: 16,
        decisions: 8,
        commands: 8,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    }
}
#[test]
fn signal_trace_measures_source_amp_ordered_fx_sum_and_send_without_heap() {
    let mut off = Runtime::new(plan(false, 0), limits()).unwrap();
    assert!(off.signal_trace_reader().is_none());
    let mut on = Runtime::new(plan(true, 1024), limits()).unwrap();
    let reader = on.signal_trace_reader().unwrap();
    let mut a = [[0.; 2]; 64];
    let mut b = a;
    for rt in [&mut off, &mut on] {
        rt.trigger(input(), 60, 0.5).unwrap();
    }
    support::without_heap(|| {
        off.render(&mut a).unwrap();
        on.render(&mut b).unwrap();
    });
    assert_eq!(a, b);
    assert!((b[32][0] - 0.0375).abs() < 1e-7);
    let rows = reader.drain();
    assert_eq!(reader.dropped(), 0);
    let level = |kind: &str, processor: &str| {
        let node = reader
            .graph
            .nodes
            .iter()
            .find(|n| n.kind == kind && n.processor == processor)
            .unwrap();
        let row = rows.iter().find(|r| r.node == node.id).unwrap();
        assert!((row.output.dc[0] - row.output.rms[0]).abs() < 1e-12);
        row.output.rms[0]
    };
    for (kind, proc, expected) in [
        ("sample_source", "resampler", 0.125),
        ("group_fx_pre", "gain", 0.25),
        ("amplifier", "envelope_velocity_gain", 0.05),
        ("group_fx_post", "gain", 0.025),
        ("bus_input", "sum", 0.025),
        ("bus_fx", "gain", 0.075),
        ("bus_send", "send_gain", 0.0375),
        ("master", "sum", 0.0375),
    ] {
        assert!((level(kind, proc) - expected).abs() < 1e-7, "{kind}");
    }
    assert!(!reader.graph.edges.is_empty());
    let text = serde_json::to_string(&*reader.graph).unwrap();
    assert!(!text.contains(".wav"));
    assert!(!text.contains("script"));
}
#[test]
fn signal_trace_capacity_loss_is_explicit_and_drain_is_control_side() {
    let mut rt = Runtime::new(plan(true, 2), limits()).unwrap();
    let reader = rt.signal_trace_reader().unwrap();
    rt.trigger(input(), 60, 0.5).unwrap();
    support::without_heap(|| rt.render(&mut [[0.; 2]; 64]).unwrap());
    assert_eq!(reader.drain().len(), 2);
    assert!(reader.dropped() > 0);
}
#[test]
fn signal_trace_coherent_bus_sum_and_valid_privacy_safe_reports() {
    let mut rt = Runtime::new(plan(true, 1024), limits()).unwrap();
    let reader = rt.signal_trace_reader().unwrap();
    for _ in 0..2 {
        rt.trigger(input(), 60, 0.5).unwrap();
    }
    support::without_heap(|| rt.render(&mut [[0.; 2]; 64]).unwrap());
    let rows = reader.drain();
    let master = rows.iter().find(|r| r.node == 0).unwrap();
    assert!((master.output.rms[0] - 0.075).abs() < 1e-7);
    let contributions: Vec<_> = rows.iter().filter(|r| r.contribution).collect();
    assert_eq!(contributions.len(), 2);
    assert_ne!(
        contributions[0].identity.family,
        contributions[1].identity.family
    );
    assert!(
        contributions
            .iter()
            .all(|r| r.identity.xfade_weight == 1. && r.identity.velocity_gain == 0.5)
    );
    let source = rows
        .iter()
        .find(|r| reader.graph.nodes[r.node].kind == "sample_source")
        .unwrap();
    assert_eq!(source.contributors, 2);
    assert!((source.output.rms[0] - 0.25).abs() < 1e-7);
    let json = serde_json::to_value(&rows).unwrap();
    assert!(json.as_array().unwrap().iter().all(|r| r["frames"] == 64));
    assert!(!json.to_string().contains("pcm"));
}
#[test]
fn signal_trace_export_is_complete_json_and_chart_without_pcm() {
    if std::env::var_os("SIGNAL_TRACE_TEST_CHILD").is_some() {
        let mut rt = Runtime::new(plan(false, 0), limits()).unwrap();
        rt.trigger(input(), 60, 0.5).unwrap();
        support::without_heap(|| rt.render(&mut [[0.; 2]; 64]).unwrap());
        drop(rt);
        assert!(sampler_core::trace_report::flush(
            std::time::Duration::from_secs(5)
        ));
        return;
    }
    let directory = std::env::temp_dir().join(format!("signal-trace-test-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "signal_trace_export_is_complete_json_and_chart_without_pcm",
        ])
        .env("SIGNAL_TRACE_TEST_CHILD", "1")
        .env("KONTRA_SIGNAL_TRACE", "1")
        .env("KONTRA_REPORT_DIR", &directory)
        .status()
        .unwrap();
    assert!(status.success());
    let text = std::fs::read_to_string(directory.join("signal-trace.json")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["complete"], true);
    assert_eq!(value["dropped"], 0);
    assert!(!value["records"].as_array().unwrap().is_empty());
    assert!(!text.contains("pcm"));
    assert!(!text.contains("script_text"));
    assert!(!text.contains(".wav"));
    let svg = std::fs::read_to_string(directory.join("signal-trace.svg")).unwrap();
    assert!(svg.contains("sample_source"));
    assert!(svg.contains("master"));
    assert!(svg.contains("parents"));
    std::fs::remove_file(directory.join("signal-trace.json")).unwrap();
    std::fs::remove_file(directory.join("signal-trace.svg")).unwrap();
    std::fs::remove_dir(directory).unwrap();
}
#[test]
fn signal_trace_reports_live_bypass_and_its_physical_native_boolean() {
    use sampler_core::{
        ControlDefinition, ControlDomain, ControlRange, ControlValue, EngineParameterAddress,
        SlotKind, engine_parameter_id, slot_control,
    };
    let range = |kind: SlotKind| ControlRange {
        control: slot_control(kind, -1, 3, 1),
        low: 0.,
        high: kind.max(),
        ramp_frames: 0,
    };
    let definitions = [
        (SlotKind::Dry, 0.),
        (SlotKind::Output, 1.),
        (SlotKind::Bypass, 0.),
    ]
    .map(|(kind, default)| ControlDefinition {
        id: slot_control(kind, -1, 3, 1),
        domain: ControlDomain::Real {
            min: 0.,
            max: kind.max(),
        },
        default: ControlValue::Real(default),
    });
    let p = plan(false, 0)
        .with_controls(definitions.to_vec())
        .unwrap()
        .with_buses(
            vec![Bus {
                processors: vec![
                    Processor::Mix {
                        count: 1,
                        dry: range(SlotKind::Dry),
                        wet: range(SlotKind::Output),
                        bypass: range(SlotKind::Bypass),
                    },
                    Processor::Gain(3.),
                ],
                sends: vec![BusSend {
                    bus: None,
                    gain: 1.,
                }],
                tail_frames: 0,
            }],
            vec![Some(0)],
        )
        .unwrap()
        .with_signal_trace(1024)
        .unwrap();
    let mut rt = Runtime::new(p, limits()).unwrap();
    let reader = rt.signal_trace_reader().unwrap();
    rt.trigger(input(), 60, 0.5).unwrap();
    rt.set_engine_parameter(
        EngineParameterAddress {
            parameter: engine_parameter_id("ENGINE_PAR_EFFECT_BYPASS").unwrap(),
            group: -1,
            slot: 3,
            generic: 1,
        },
        1,
    )
    .unwrap();
    support::without_heap(|| rt.render(&mut [[0.; 2]; 64]).unwrap());
    let rows = reader.drain();
    let mix = rows
        .iter()
        .find(|r| reader.graph.nodes[r.node].processor == "slot_mix")
        .unwrap();
    assert!(!mix.enabled);
    assert_eq!(mix.normalized[2], Some(1));
    let gain = rows
        .iter()
        .find(|r| {
            reader.graph.nodes[r.node].kind == "bus_fx"
                && reader.graph.nodes[r.node].processor == "gain"
        })
        .unwrap();
    assert!(!gain.enabled);
    assert_eq!(gain.input.rms, gain.output.rms);
}

#[test]
fn signal_trace_host_fader_rack_and_master_hooks_are_bounded_and_sample_clocked() {
    use sampler_core::trace::HostStage;
    let mut rt = Runtime::new(plan(true, 4096), limits()).unwrap();
    let reader = rt.signal_trace_reader().unwrap();
    rt.trigger(input(), 60, 0.5).unwrap();
    let mut output = [[0.; 2]; 64];
    rt.render(&mut output).unwrap();
    let left = [0.1f32; 64];
    let right = [0.2f32; 64];
    let master = [0.25f32; 64];
    support::without_heap(|| {
        rt.trace_host_frames(HostStage::PartFader, &output, [0.5, 0.75], true, 2);
        rt.trace_host_planar(
            HostStage::RackBus(2),
            &left,
            &right,
            [0.5, 0.75],
            None,
            true,
            2,
        );
        rt.trace_host_planar(
            HostStage::Master(2),
            &left,
            &right,
            [1.; 2],
            Some(&master),
            true,
            2,
        );
    });
    let rows = reader.drain();
    for (kind, left, right) in [
        ("host_part_fader", 0.01875, 0.028125),
        ("host_rack_bus", 0.05, 0.15),
        ("host_master", 0.025, 0.05),
    ] {
        let row = rows
            .iter()
            .find(|r| reader.graph.nodes[r.node].kind == kind)
            .unwrap();
        assert_eq!(row.at, 0);
        assert_eq!(row.frames, 64);
        assert_eq!(row.identity.external_port, Some(2));
        assert!((row.output.rms[0] - left).abs() < 1e-7);
        assert!((row.output.rms[1] - right).abs() < 1e-7);
    }
}

#[test]
fn signal_trace_amp_parameters_describe_the_live_envelope_not_its_authored_default() {
    use sampler_core::{EngineParameterAddress, EngineParameterLaw, engine_parameter_id};
    let plan = plan(false, 0).with_groups(1, vec![Some(0)]).unwrap()
        .with_group_envelope_parameters(0, 88, 0, Envelope::default()).unwrap()
        .with_signal_trace(4096).unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    let reader = rt.signal_trace_reader().unwrap();
    let address = EngineParameterAddress {parameter:engine_parameter_id("ENGINE_PAR_ATTACK").unwrap(),group:88,slot:0,generic:-1};
    rt.set_engine_parameter(address, 449120).unwrap();
    rt.trigger(input(), 60, 0.5).unwrap();
    support::without_heap(|| {rt.render(&mut [[0.;2];64]).unwrap();});
    let row=reader.drain().into_iter().find(|r|reader.graph.nodes[r.node].kind=="amplifier").unwrap();
    let index=reader.graph.nodes[row.node].parameters.iter().position(|p|p.name=="attack_frames").unwrap();
    let expected=EngineParameterLaw::ShiftedExponential {low:96.,high:15002.*48.,offset:96.}.decode(449120).round();
    assert!((row.values[index]-expected).abs()<1.,"{} vs {}",row.values[index],expected);
    assert_eq!(row.normalized[index],Some(EngineParameterLaw::ShiftedExponential {low:96.,high:15002.*48.,offset:96.}.encode(expected)));
}
