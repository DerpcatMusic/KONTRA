use sampler_core::{Instruction as I, *};
mod support;

fn address(protocol: Protocol, port: u16, group: u8, channel: u8) -> ChannelAddress {
    ChannelAddress { protocol, port, group, channel }
}
fn input(address: ChannelAddress, id: i32) -> Input {
    Input { protocol: address.protocol, port: address.port, group: address.group,
        channel: address.channel, key: 60, external_id: Some(id) }
}
fn scope(address: ChannelAddress, channels: u16) -> ChannelScope {
    ChannelScope { protocol: address.protocol, port: address.port, group: address.group, channels }
}
fn empty() -> Prepared { Prepared::new(48000, vec![], vec![], 0).unwrap() }
fn budget(plan: &Prepared) -> Limits {
    Limits { notes: 8, expressions: 8, performances: 2, channels: 0,
        families: 4, voices: 4, behaviors: 4, commands: 8,
        behavior_cells: plan.behavior_local_count() * 4, behavior_fuel: 64,
        ..Limits::for_plan(plan, 8, 4) }
}
fn runtime(prepared: Prepared) -> Runtime {
    let limits = budget(&prepared);
    Runtime::new(prepared, limits).unwrap()
}
fn wait_forward() -> Prepared {
    empty().with_programs(vec![Program::new(vec![I::Wait(4), I::ForwardController]).unwrap()
        .with_wait_lifetime(WaitLifetime::Callback)], None).unwrap()
        .with_controller_program(0).unwrap()
}

#[test]
fn physical_input_and_note_snapshots_are_isolated_from_the_explicit_global_domain() {
    let prepared = empty();
    let mut rt = runtime(prepared);
    let addresses = [address(Protocol::Midi1, 2, 3, 1), address(Protocol::Midi1, 2, 3, 2),
        address(Protocol::Midi1, 9, 3, 1), address(Protocol::Midi1, 2, 7, 1),
        address(Protocol::Midi2, 2, 3, 1), address(Protocol::Clap, 2, 3, 1)];
    support::without_heap(|| {
        let domain = rt.performance(0).unwrap();
        let other_domain = rt.performance(1).unwrap();
        rt.set_controller(domain, 1, 19).unwrap();
        rt.render(&mut []).unwrap();
        for (i, &address) in addresses.iter().enumerate() {
            let value = 1000 + i as u32;
            rt.dispatch_controller(domain, address, 1 << address.channel, 1, value).unwrap();
            assert_eq!(rt.channel_controller(domain, address, 1), Ok(value));
            assert_eq!(rt.channel_input_controller(domain, address, 1), Ok(value));
            assert_eq!(rt.channel_controller(other_domain, address, 1), Ok(0));
            let note = rt.note_on(input(address, i as i32), 60, 1.).unwrap();
            assert_eq!(rt.note_controller(note, 1), Ok(value));
            rt.dispatch_controller(domain, address, 1 << address.channel, 1, value + 100).unwrap();
            assert_eq!(rt.note_controller(note, 1), Ok(value));
        }
        assert_eq!(rt.controller(domain, 1), Ok(19));
        assert_eq!(rt.input_controller(domain, 1), Ok(0));
        // This API deliberately addresses the entire Performance, including
        // already-created physical banks; a physical input never does so.
        rt.set_controller(domain, 1, 77).unwrap();
        rt.render(&mut []).unwrap();
        for &address in &addresses { assert_eq!(rt.channel_controller(domain, address, 1), Ok(77)); }
        assert_eq!(rt.controller(domain, 1), Ok(77));
        assert_eq!(rt.now(), 0);
    });
}

#[test]
fn channel_reset_filters_each_captured_delayed_target_without_reviving_old_values() {
    let prepared = wait_forward();
    let mut rt = runtime(prepared);
    let origin = address(Protocol::Midi1, 7, 3, 0);
    let a = ChannelAddress { channel: 1, ..origin };
    let b = ChannelAddress { channel: 2, ..origin };
    support::without_heap(|| {
        let domain = rt.performance(0).unwrap();
        let pending = rt.dispatch_controller(domain, origin, (1 << 1) | (1 << 2), 11, 123).unwrap().unwrap();
        assert_eq!(rt.channel_input_controller(domain, a, 11), Ok(123));
        assert_eq!(rt.channel_input_controller(domain, b, 11), Ok(123));
        rt.reset_channel_controllers(domain, scope(origin, 1 << 2)).unwrap();
        assert_eq!(rt.channel_input_controller(domain, b, 11), Ok(u32::MAX));
        assert_eq!(rt.channel_controller(domain, b, 11), Ok(u32::MAX));
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        assert_eq!(rt.behavior_outcome(pending), Ok(Some(Outcome::Finished)));
        assert_eq!(rt.channel_controller(domain, a, 11), Ok(123));
        assert_eq!(rt.channel_controller(domain, b, 11), Ok(u32::MAX));
        assert_eq!(rt.controller(domain, 11), Ok(u32::MAX));
        rt.flush_behaviors(|_, _, _| true);
        rt.dispatch_controller(domain, b, 1 << b.channel, 11, 456).unwrap();
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        assert_eq!(rt.channel_controller(domain, b, 11), Ok(456));
        assert_eq!(rt.channel_controller(domain, a, 11), Ok(123));
        rt.flush_behaviors(|_, _, outcome| { assert_eq!(outcome, Outcome::Finished); true });
    });
}

#[test]
fn reset_invalidates_waiting_control_writes_instead_of_reauthoring_their_targets() {
    let control = ControlId(7);
    let prepared = empty().with_programs(vec![Program::new(vec![I::Wait(4),
        I::SetLocal { local: 0, value: 1 }, I::SetLocal { local: 1, value: 777 },
        I::WriteController { controller: 0, value: 1 }]).unwrap()
        .with_wait_lifetime(WaitLifetime::Callback)], None).unwrap()
        .with_controls(vec![ControlDefinition { id: control,
            domain: ControlDomain::Integer { min: 0, max: 10 }, default: ControlValue::Integer(0) }]).unwrap()
        .with_control_programs(vec![ControlCallback { control, program: 0, stage: 0 }]).unwrap();
    let mut rt = runtime(prepared);
    let origin = address(Protocol::Midi2, 2, 3, 0);
    let a = ChannelAddress { channel: 1, ..origin };
    let b = ChannelAddress { channel: 2, ..origin };
    support::without_heap(|| {
        let domain = rt.performance(0).unwrap();
        let context = ControlContext { performance: domain, origin, channels: (1 << 1) | (1 << 2) };
        let pending = rt.invoke_control(context, rt.active_plan(), None,
            ControlWrite { id: control, value: ControlValue::Integer(1) }).unwrap().1.unwrap();
        rt.reset_channel_controllers(domain, scope(origin, 1 << 2)).unwrap();
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        assert_eq!(rt.behavior_outcome(pending), Ok(Some(Outcome::Finished)));
        assert_eq!(rt.channel_controller(domain, a, 1), Ok(777));
        assert_eq!(rt.channel_controller(domain, b, 1), Ok(0));
        assert_eq!(rt.control_value(rt.active_plan(), control), Ok(ControlValue::Integer(1)));
        assert_eq!(rt.controller(domain, 1), Ok(0));
        rt.flush_behaviors(|_, _, _| true);
    });
}

#[test]
fn reset_reaches_old_stage_banks_and_old_generation_callbacks_keep_their_captured_epoch() {
    let prepared = wait_forward();
    let limits = budget(&prepared);
    let (mut rt, mut transfer) = Runtime::with_plan_updates(prepared, limits, 2, 1).unwrap();
    let origin = address(Protocol::Midi1, 5, 0, 3);
    let domain = rt.performance(0).unwrap();
    let old = rt.active_plan();
    let pending = rt.dispatch_controller(domain, origin, 1 << 3, 1, 99).unwrap().unwrap();
    transfer.submit(Box::new(empty())).unwrap();
    support::without_heap(|| {
        rt.poll_plan_update().unwrap();
        rt.reset_channel_controllers(domain, scope(origin, 1 << 3)).unwrap();
        rt.dispatch_controller(domain, origin, 1 << 3, 1, 42).unwrap();
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        assert_eq!(rt.behavior_outcome(pending), Ok(Some(Outcome::Finished)));
        assert_eq!(rt.channel_controller(domain, origin, 1), Ok(42));
        assert_eq!(rt.channel_input_controller(domain, origin, 1), Ok(42));
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_behaviors(|_, owner, outcome| {
            assert_eq!(owner, BehaviorOwner::Plan(old)); assert_eq!(outcome, Outcome::Finished); true
        });
        assert_eq!(rt.collect_retired_plans(), 1);
    });
    drop(transfer.retired().unwrap());
}

fn voice_plan() -> Prepared {
    let pcm = (0..4096).map(|frame| [((frame % 127) as f32 - 63.) / 256.,
        ((frame % 97) as f32 - 48.) / 256.]).collect();
    Prepared::new(48000, vec![Pcm::new(48000, pcm).unwrap()], vec![Region {
        sample: 0, key_low: 60, key_high: 60, root_key: None, velocity_low: 0., velocity_high: 1.,
        gain: 1., envelope: Envelope::default(), playback: Playback::default() }], 1).unwrap()
        .with_velocity_curves(vec![VelocityCurve::Constant]).unwrap()
        .with_voice_modulation(vec![ModProgram { sources: vec![ModSource::Controller(1), ModSource::Controller(11)],
            routes: vec![ModRoute::new(0, ModTarget::Pitch, 12.), ModRoute::new(1, ModTarget::Attenuate, 0.75)],
            controls: vec![], breakpoints: vec![], shapes: vec![], envelope_controls: vec![] }],
            vec![Some(0)], vec![0]).unwrap()
}

#[test]
fn held_cc1_and_cc11_follow_each_real_channel_and_match_independent_global_reference_voices() {
    for protocol in [Protocol::Midi1, Protocol::Midi2, Protocol::Clap] {
        let prepared = voice_plan();
        let mut actual = runtime(prepared);
        let mut reference_a = runtime(voice_plan());
        let mut reference_b = runtime(voice_plan());
        let a = address(protocol, 7, 3, 1);
        let b = ChannelAddress { channel: 2, ..a };
        support::without_heap(|| {
            let domain = actual.performance(0).unwrap();
            let da = reference_a.performance(0).unwrap();
            let db = reference_b.performance(0).unwrap();
            let first = actual.trigger(input(a, 101), 60, 1.).unwrap();
            let second = actual.trigger(input(b, 102), 60, 1.).unwrap();
            reference_a.trigger(input(a, 101), 60, 1.).unwrap();
            reference_b.trigger(input(b, 102), 60, 1.).unwrap();
            let first_expression = actual.expression_id(first).unwrap();
            let second_expression = actual.expression_id(second).unwrap();
            for (channel, cc, value) in [(1, 1, u32::MAX), (2, 11, 0), (1, 11, u32::MAX / 2),
                (2, 1, u32::MAX / 3), (1, 1, 0), (2, 11, u32::MAX)] {
                let target = ChannelAddress { channel, ..a };
                actual.dispatch_controller(domain, target, 1 << channel, cc, value).unwrap();
                if channel == 1 { reference_a.set_controller(da, cc, value).unwrap(); }
                else { reference_b.set_controller(db, cc, value).unwrap(); }
                let mut got = [[0.; 2]; 64]; let mut left = got; let mut right = got;
                actual.render(&mut got).unwrap(); reference_a.render(&mut left).unwrap(); reference_b.render(&mut right).unwrap();
                for ((got, left), right) in got.iter().zip(left).zip(right) {
                    for channel in 0..2 { assert!((got[channel] - (left[channel] + right[channel])).abs() <= 2e-6,
                        "physical-bank modulation differs from independently rendered global references"); }
                }
                assert_eq!(actual.note_count(), 2); assert_eq!(actual.voice_count(), 2);
                assert_eq!(actual.expression_id(first), Ok(first_expression));
                assert_eq!(actual.expression_id(second), Ok(second_expression));
                assert_eq!(actual.now(), reference_a.now()); assert_eq!(actual.now(), reference_b.now());
            }
            assert_eq!(actual.controller(domain, 1), Ok(0));
            assert_eq!(actual.controller(domain, 11), Ok(u32::MAX));
        });
    }
}

#[test]
fn physical_bank_capacity_refuses_before_input_or_callback_publication_and_reset_reuses_slots() {
    let prepared = empty();
    let mut rt = runtime(prepared);
    support::without_heap(|| {
        let domain = rt.performance(0).unwrap();
        // Explicit minimum 16 physical rows per Performance; two performances
        // provide 32 Runtime slots, without any callback-side growth.
        for port in 0..2 {
            let origin = address(Protocol::Midi1, port, 0, 0);
            rt.dispatch_controller(domain, origin, u16::MAX, 1, 100 + u32::from(port)).unwrap();
        }
        let missing = address(Protocol::Midi1, 2, 0, 0);
        assert_eq!(rt.dispatch_controller(domain, missing, 1, 1, 999), Err(Error::Capacity));
        assert_eq!(rt.channel_input_controller(domain, missing, 1), Ok(0));
        assert_eq!(rt.channel_controller(domain, missing, 1), Ok(0));
        rt.reset_channel_controllers(domain, scope(address(Protocol::Midi1, 0, 0, 0), u16::MAX)).unwrap();
        assert_eq!(rt.now(), 0);
        assert_eq!(rt.pending_commands(), 0);
        assert_eq!(rt.channel_controller(domain, address(Protocol::Midi1, 1, 0, 0), 1), Ok(101));
        assert_eq!(rt.channel_controller(domain, address(Protocol::Midi1, 0, 0, 0), 1), Ok(0));
    });
}

#[test]
fn a_waiting_note_callback_keeps_its_admitted_controller_epoch_even_without_an_input_stage() {
    let prepared = empty().with_programs(vec![Program::new(vec![I::Wait(4),
        I::SetLocal { local: 0, value: 1 }, I::SetLocal { local: 1, value: 888 },
        I::WriteController { controller: 0, value: 1 }]).unwrap()
        .with_wait_lifetime(WaitLifetime::Callback)], None).unwrap();
    let mut rt = runtime(prepared);
    let origin = address(Protocol::Midi1, 7, 3, 1);
    support::without_heap(|| {
        let domain = rt.performance(0).unwrap();
        let note = rt.note_on(input(origin, 101), 60, 1.).unwrap();
        let pending = rt.start_behavior(note, 0).unwrap();
        rt.reset_channel_controllers(domain, scope(origin, 1 << 1)).unwrap();
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        assert_eq!(rt.behavior_outcome(pending), Ok(Some(Outcome::Finished)));
        assert_eq!(rt.channel_controller(domain, origin, 1), Ok(0));
        assert!(rt.input_held(note).unwrap());
        rt.flush_behaviors(|_, _, _| true);
        // A newly admitted note has the new revision and can publish normally.
        let fresh = rt.note_on(input(origin, 102), 60, 1.).unwrap();
        rt.start_behavior(fresh, 0).unwrap();
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        assert_eq!(rt.channel_controller(domain, origin, 1), Ok(888));
        rt.flush_behaviors(|_, _, outcome| { assert_eq!(outcome, Outcome::Finished); true });
    });
}
