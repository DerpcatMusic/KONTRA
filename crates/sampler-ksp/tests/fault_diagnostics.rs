use sampler_core::*;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn runtime(source: &str) -> Runtime {
    let script = sampler_ksp::compile_with(
        source,
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        &sampler_ksp::Environment {
            slot: 3,
            ..Default::default()
        },
    )
    .unwrap();
    let prepared = script
        .bind(
            Prepared::new(
                48000,
                vec![Pcm::new(48000, vec![[0.5; 2]; 128].into_boxed_slice()).unwrap()],
                vec![Region {
                    sample: 0,
                    key_low: 0,
                    key_high: 127,
                    root_key: None,
                    velocity_low: 0.,
                    velocity_high: 1.,
                    gain: 1.,
                    envelope: Envelope::default(),
                    playback: Playback::default(),
                }],
                128,
            )
            .unwrap(),
        )
        .unwrap();
    let mut limits = Limits::for_plan(&prepared, 8, 8);
    limits.behaviors = 8;
    limits.behavior_cells = prepared.behavior_local_count() * 8;
    limits.behavior_fuel = 4096;
    Runtime::new(prepared, limits).unwrap()
}

#[test]
fn fault_retains_exact_function_site_operands_and_callers_until_accepted_without_heap() {
    let source = "on init\n declare $delay := -1\nend on\nfunction fail\n wait($delay)\nend function\non note\n call fail\nend on";
    let mut rt = runtime(source);
    support::without_heap(|| {
        rt.trigger(
            Input {
                protocol: Protocol::Midi1,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: None,
            },
            60,
            1.,
        )
        .unwrap();
        let mut retained = None;
        rt.flush_behavior_diagnostics(|id, _, outcome, program, context| {
            assert_eq!(outcome, Outcome::Fault(Error::InvalidInput));
            let f = context.unwrap();
            assert_eq!(f.source_slot, Some(3));
            assert_eq!(f.callback, Some("on note"));
            assert_eq!(f.site.unwrap().line, 5);
            assert_eq!(f.site.unwrap().builtin, Some("wait"));
            assert_eq!(f.callers[0].unwrap().line, 8);
            let Instruction::WaitMicros { local } = f.instruction.unwrap() else {
                panic!("{:?}", f.instruction)
            };
            assert_eq!(f.registers[local as usize], -1);
            assert_eq!(
                f.source_digest,
                Some(sampler_ksp::source_fingerprint(source))
            );
            retained = Some((id, program, f.pc));
            false
        });
        rt.flush_behavior_diagnostics(|id, _, _, program, context| {
            assert_eq!((id, program, context.unwrap().pc), retained.unwrap());
            true
        });
        rt.flush_behavior_diagnostics(|_, _, _, _, _| panic!("already consumed"));
    });
}

#[test]
fn shared_ui_entries_keep_the_failing_callback_and_statement() {
    let source = "on init\n declare ui_button $a\n declare ui_button $b\nend on\non ui_control($a)\n wait(-1)\nend on\non ui_control($b)\n wait(-2)\nend on";
    let mut rt = runtime(source);
    let context = ControlContext {
        performance: rt.performance(0).unwrap(),
        origin: ChannelAddress {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
        },
        channels: 1,
    };
    let plan = rt.active_plan();
    for (name, line, value) in [("$a", 6, -1), ("$b", 9, -2)] {
        let id = sampler_ksp::derived_control_id(3, name);
        support::without_heap(|| {
            rt.invoke_widget(
                context,
                plan,
                None,
                &[WidgetEdit {
                    id,
                    index: 0,
                    interaction: Default::default(),
                    value: WidgetValue::Integer(1),
                }],
            )
            .unwrap();
            let mut seen = false;
            rt.flush_behavior_diagnostics(|_, _, outcome, _, evidence| {
                assert_eq!(outcome, Outcome::Fault(Error::InvalidInput));
                let f = evidence.unwrap();
                assert_eq!(f.callback, Some("on ui_control"));
                assert_eq!(f.site.unwrap().line, line);
                assert!(f.ui_id >= 0);
                let Instruction::WaitMicros { local } = f.instruction.unwrap() else {
                    panic!()
                };
                assert_eq!(f.registers[local as usize], value);
                seen = true;
                true
            });
            assert!(seen);
        });
    }
}
