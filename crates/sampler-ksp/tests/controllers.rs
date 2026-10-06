use sampler_core::*;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn compile(source: &str) -> Result<sampler_ksp::Script, sampler_ksp::Error> {
    sampler_ksp::compile(
        source,
        48000,
        sampler_ksp::Limits {
            source_bytes: 8192,
            instructions: 512,
            variables: 16,
            array_cells: 16,
        },
        &[],
    )
}
fn runtime(source: &str) -> Runtime {
    let plan = compile(source)
        .unwrap()
        .bind(
            Prepared::new(
                48000,
                vec![Pcm::new(48000, Box::from([[1.; 2]; 16])).unwrap()],
                vec![Region {
                    sample: 0,
                    key_low: 60,
                    key_high: 60,
                    root_key: None,
                    velocity_low: 0.,
                    velocity_high: 1.,
                    gain: 1.,
                    envelope: Envelope::default(),
                    playback: Playback::default(),
                }],
                1,
            )
            .unwrap(),
        )
        .unwrap();
    let behavior_cells = plan.behavior_local_count() * 4;
    let note_cells = plan.note_cell_count() * 4;
    Runtime::new(
        plan,
        Limits {
            notes: 4,
            performances: 2,
            channels: 2,
            families: 4,
            voices: 4,
            expressions: 4,
            decisions: 0,
            commands: 4,
            behaviors: 4,
            behavior_cells,
            behavior_fuel: 512,
            note_cells,
        },
    )
    .unwrap()
}
fn origin() -> ChannelAddress {
    ChannelAddress {
        protocol: Protocol::Midi1,
        port: 1,
        group: 2,
        channel: 3,
    }
}

#[test]
fn source_controllers_consume_remap_and_forward_before_wait_without_reentering() {
    let mut rt = runtime(
        "on init declare $calls declare $number declare $latest end on
      on controller
        inc($calls)
        if ($CC_NUM = 1)
          ignore_controller
          set_controller(7, %CC[$CC_NUM])
        end if
        wait(125)
        $number := $CC_NUM
        $latest := %CC[$CC_NUM]
        ignore_controller
      end on",
    );
    support::without_heap(|| {
        let domain = rt.performance(1).unwrap();
        let a = rt
            .dispatch_controller(domain, origin(), 1 << 3, 1, u32::MAX)
            .unwrap()
            .unwrap();
        let b = rt
            .dispatch_controller(domain, origin(), 1 << 3, 11, 0x80000001)
            .unwrap()
            .unwrap();
        assert_eq!(rt.controller(domain, 1), Ok(0));
        assert_eq!(rt.controller(domain, 7), Ok(u32::MAX));
        // Automatic forwarding keeps MIDI 2 precision, not the source's 7-bit view.
        assert_eq!(rt.controller(domain, 11), Ok(0x80000001));
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
            Ok(2)
        );
        assert_eq!(rt.controller(rt.performance(0).unwrap(), 7), Ok(0));
        rt.render(&mut [[0.; 2]; 6]).unwrap();
        assert_eq!(rt.behavior_outcome(a), Ok(None));
        rt.render(&mut []).unwrap();
        for id in [a, b] {
            assert_eq!(rt.behavior_outcome(id), Ok(Some(Outcome::Finished)));
        }
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 1),
            Ok(11)
        );
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 2),
            Ok(64)
        );
        assert_eq!(rt.controller(domain, 11), Ok(0x80000001));
        rt.flush_behaviors(|_, _, _| true);
        assert_eq!((rt.note_count(), rt.pending_commands()), (0, 0));
    });
}

#[test]
fn remapped_pedals_use_the_shared_gate_while_consumed_pedals_leave_it_alone() {
    for source in [
        "on controller ignore_controller end on",
        "on controller if ($CC_NUM = 1) ignore_controller set_controller(64,%CC[1]) end if end on",
    ] {
        let mut rt = runtime(source);
        support::without_heap(|| {
            let domain = rt.performance(0).unwrap();
            let input = Input {
                protocol: origin().protocol,
                port: 1,
                group: 2,
                channel: 3,
                key: 60,
                external_id: Some(7),
            };
            let n = rt.trigger(input, 60, 1.).unwrap();
            let remap = source.contains("set_controller");
            rt.dispatch_controller(
                domain,
                origin(),
                1 << 3,
                if remap { 1 } else { 64 },
                u32::MAX,
            )
            .unwrap();
            rt.key_up(n, None).unwrap();
            assert_eq!(rt.note(n).unwrap().2, remap);
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                true
            });
            if remap {
                rt.dispatch_controller(domain, origin(), 1 << 3, 1, 0)
                    .unwrap();
                assert!(!rt.note(n).unwrap().2);
                rt.flush_behaviors(|_, _, outcome| {
                    assert_eq!(outcome, Outcome::Finished);
                    true
                });
            }
            rt.render(&mut [[0.; 2]; 32]).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));
        });
    }
    for invalid in [
        "on note ignore_controller end on",
        "on controller play_note(60,127,0,-1) end on",
        "on controller ignore_event($EVENT_ID) end on",
        "on init set_controller(1,1) end on",
        "on note $CC_NUM end on",
    ] {
        assert!(compile(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn note_and_release_controller_operations_retain_the_original_domain_across_waits() {
    for block in [1, 7, 64] {
        let mut rt = runtime(
            "on init declare polyphonic $saved end on
          on note
            $saved := %CC[1]
            wait(125)
            set_controller(7,$saved)
            set_controller(11,%CC[1])
          end on
          on release wait(250) set_controller(64,%CC[2]) end on",
        );
        support::without_heap(|| {
            for (index, value) in [(0, 63), (1, 127)] {
                let domain = rt.performance(index).unwrap();
                let address = ChannelAddress {
                    channel: index as u8 + 3,
                    ..origin()
                };
                rt.dispatch_controller(
                    domain,
                    address,
                    1 << address.channel,
                    1,
                    (value * u64::from(u32::MAX) / 127) as u32,
                )
                .unwrap();
                rt.dispatch_controller(domain, address, 1 << address.channel, 2, u32::MAX)
                    .unwrap();
                let note = rt
                    .trigger_in(
                        domain,
                        Input {
                            protocol: Protocol::Midi1,
                            port: 1,
                            group: 2,
                            channel: index as u8 + 3,
                            key: 60,
                            external_id: Some(index as i32),
                        },
                        NotePitch::Key(60),
                        1.,
                        Expression::default(),
                    )
                    .unwrap();
                rt.key_up(note, None).unwrap();
                rt.dispatch_controller(domain, address, 1 << address.channel, 1, 0)
                    .unwrap();
            }
            let mut audio = [[0.; 2]; 13];
            for part in audio.chunks_mut(block) {
                rt.render(part).unwrap();
            }
            for (index, value) in [(0, 63), (1, 127)] {
                let domain = rt.performance(index).unwrap();
                assert_eq!(
                    rt.controller(domain, 7),
                    Ok((value * u64::from(u32::MAX) / 127) as u32)
                );
                assert_eq!(rt.controller(domain, 11), Ok(0));
                assert_eq!(rt.controller(domain, 64), Ok(u32::MAX));
                let channel = rt
                    .register_channel(ChannelAddress {
                        protocol: Protocol::Midi1,
                        port: 1,
                        group: 2,
                        channel: index as u8 + 3,
                    })
                    .unwrap();
                assert_eq!(rt.pedals(channel), Ok((true, false)));
            }
            let mut completed = 0;
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                completed += 1;
                true
            });
            assert_eq!(completed, 4);
            rt.flush_ended(|_| true);
            assert_eq!((rt.note_count(), rt.pending_commands()), (0, 0));
        });
    }
    assert!(compile("on note set_controller(7,$CC_NUM) end on").is_err());
}
