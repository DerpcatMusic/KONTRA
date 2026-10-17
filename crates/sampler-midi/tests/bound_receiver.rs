use sampler_core::{Instruction as I, *};
use sampler_midi::{Applied, BoundMpe, Mpe, Packets, Zone};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn runtime(code: Option<Vec<I>>) -> Runtime {
    runtime_with_root(code, 69)
}
fn runtime_with_root(code: Option<Vec<I>>, root: u8) -> Runtime {
    let pcm = Pcm::new(
        48000,
        (0..16384)
            .map(|frame| {
                let value = (frame as f64 * std::f64::consts::TAU * 440. / 48000.).sin() as f32;
                [value, value]
            })
            .collect(),
    )
    .unwrap();
    let mut prepared = Prepared::new(
        48000,
        vec![pcm],
        vec![Region {
            sample: 0,
            key_low: 48,
            key_high: 84,
            root_key: Some(root),
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::new(0, 0, 0, 1., 0).unwrap(),
            playback: Playback::default(),
        }],
        37,
    )
    .unwrap();
    if let Some(code) = code {
        prepared = prepared
            .with_programs(
                vec![
                    Program::new(code)
                        .unwrap()
                        .with_wait_lifetime(WaitLifetime::Callback),
                ],
                None,
            )
            .unwrap()
            .with_controller_program(0)
            .unwrap();
    }
    let mut limits = Limits::for_plan(&prepared, 8, 8);
    limits.channels = 48;
    limits.performances = 2;
    limits.expressions = 16;
    limits.commands = 32;
    Runtime::new(prepared, limits)
        .unwrap()
        .with_receiver_capacity(4)
        .unwrap()
}
fn wire(status: u8, channel: u8, key: u8, value: u32) -> [u32; 2] {
    [
        0x4000_0000 | (u32::from(status | channel) << 16) | (u32::from(key) << 8),
        value,
    ]
}
fn apply(owner: &BoundMpe, rt: &mut Runtime, words: [u32; 2]) -> Applied {
    owner
        .apply(rt, Packets::new(&words).next().unwrap().unwrap())
        .unwrap()
}
fn bind(rt: &mut Runtime, group: u8) -> BoundMpe {
    let mut facade = Mpe::new(rt, 0, group, Zone::Lower, 15, 8).unwrap();
    facade.enable_host_channel_projection().unwrap();
    facade.use_midi2_channel_voice().unwrap();
    facade.bind(rt).unwrap()
}
fn start(owner: &BoundMpe, rt: &mut Runtime, channel: u8, key: u8) -> NoteId {
    let Applied::Started(note) = apply(owner, rt, wire(0x90, channel, key, 0xffff_0000)) else {
        panic!("not started")
    };
    note
}
fn expression(rt: &Runtime, note: NoteId) -> Expression {
    rt.expression(rt.expression_id(note).unwrap()).unwrap()
}
fn assert_pcm(a: &mut Runtime, b: &mut Runtime, count: usize) {
    let mut a_frames = [[0.; 2]; 64];
    let mut b_frames = [[0.; 2]; 64];
    let mut at = 0;
    while at < count {
        let n = (count - at).min(64);
        a.render(&mut a_frames[..n]).unwrap();
        b.render(&mut b_frames[..n]).unwrap();
        assert_eq!(&a_frames[..n], &b_frames[..n], "PCM differs at block {at}");
        at += n;
    }
}

#[test]
fn captured_pressure_is_consumed_before_canonical_components_and_future_admission() {
    let mut rt = runtime(Some(vec![I::SuppressController]));
    let owner = bind(&mut rt, 0);
    let first = start(&owner, &mut rt, 3, 69);
    let before = expression(&rt, first);
    support::without_heap(|| {
        apply(&owner, &mut rt, wire(0xd0, 3, 0, 0x8123_4567));
        assert_eq!(expression(&rt, first), before);
        let performance = rt.performance(0).unwrap();
        let address = ChannelAddress {
            protocol: Protocol::Midi2,
            port: 0,
            group: 0,
            channel: 3,
        };
        assert_eq!(
            rt.channel_input_controller(performance, address, 129),
            Ok(0x8123_4567)
        );
        assert_eq!(rt.channel_controller(performance, address, 129), Ok(0));
        let next = start(&owner, &mut rt, 3, 70);
        assert_eq!(expression(&rt, next).pressure, 0);
    });
}

#[test]
fn waited_original_pitch_commits_at_exact_frame_and_retains_all_wire_bits() {
    let mut actual = runtime(Some(vec![I::Wait(4), I::ForwardController]));
    let owner = bind(&mut actual, 0);
    let mut reference = runtime(None);
    let reference_owner = bind(&mut reference, 0);
    let a = start(&owner, &mut actual, 5, 69);
    let b = start(&reference_owner, &mut reference, 5, 69);
    let raw = 0x8123_4567;
    apply(&owner, &mut actual, wire(0xe0, 5, 0, raw));
    assert_eq!(expression(&actual, a).pitch_semitones, 0.);
    assert_pcm(&mut actual, &mut reference, 4);
    // The reference is an independent ordinary no-stage receiver, not the new
    // staged projection helper and not a duplicate expected-value cache.
    apply(&reference_owner, &mut reference, wire(0xe0, 5, 0, raw));
    assert_pcm(&mut actual, &mut reference, 256);
    assert_eq!(expression(&actual, a), expression(&reference, b));
    assert_eq!(
        expression(&actual, a).pitch_semitones,
        2. * f64::from(raw - 0x8000_0000) / 2147483647.
    );
    assert_ne!(
        expression(&actual, a).pitch_semitones,
        2. * f64::from((raw >> 18) - 8192) / 8191.
    );
}

#[test]
fn same_value_script_replacement_is_explicit_script_precision_not_original_wire_provenance() {
    let mut rt = runtime(Some(vec![
        I::ReadControllerNumber { local: 0 },
        I::ReadControllerValue { local: 1 },
        I::SuppressController,
        I::WriteController {
            controller: 0,
            value: 1,
        },
    ]));
    let owner = bind(&mut rt, 0);
    let note = start(&owner, &mut rt, 4, 69);
    let raw = 0x8123_4567;
    apply(&owner, &mut rt, wire(0xe0, 4, 0, raw));
    let reduced = raw >> 18;
    assert_eq!(
        expression(&rt, note).pitch_semitones,
        2. * f64::from(reduced - 8192) / 8191.
    );
    assert_ne!(
        expression(&rt, note).pitch_semitones,
        2. * f64::from(raw - 0x8000_0000) / 2147483647.
    );
}

#[test]
fn detached_manual_follower_keeps_its_frozen_member_and_live_manager_after_channel_reuse() {
    // Key70 + 50 semitones remains below the existing MAX_STEP16 with root84.
    // The original root69 fixture exceeded admission before its ownership assertion.
    let mut rt = runtime_with_root(None, 84);
    let owner = bind(&mut rt, 0);
    let on = [0x2091_457f];
    let Applied::Started(parent) = owner
        .apply(&mut rt, Packets::new(&on).next().unwrap().unwrap())
        .unwrap()
    else {
        panic!()
    };
    let source = rt.expression_id(parent).unwrap();
    let child = rt
        .child(parent, 69, 1., false, Inheritance::Expression)
        .unwrap();
    rt.start(child, 0, rt.now(), 1.).unwrap();
    let off = [0x2081_4500];
    owner
        .apply(&mut rt, Packets::new(&off).next().unwrap().unwrap())
        .unwrap();
    let new = [0x2091_467f];
    let Applied::Started(reused) = owner
        .apply(&mut rt, Packets::new(&new).next().unwrap().unwrap())
        .unwrap()
    else {
        panic!()
    };
    let bend = [0x20e1_7f7f];
    owner
        .apply(&mut rt, Packets::new(&bend).next().unwrap().unwrap())
        .unwrap();
    assert_eq!(
        rt.expression(source).unwrap().pitch_semitones,
        0.,
        "released member snapshot must not take reused-channel bend"
    );
    assert_eq!(expression(&rt, reused).pitch_semitones, 48.);
    let manager = [0x20e0_7f7f];
    owner
        .apply(&mut rt, Packets::new(&manager).next().unwrap().unwrap())
        .unwrap();
    assert_eq!(rt.expression(source).unwrap().pitch_semitones, 2.);
    assert_eq!(expression(&rt, child).pitch_semitones, 2.);
    assert_eq!(expression(&rt, reused).pitch_semitones, 50.);
    assert_eq!(rt.unbind_receiver(owner.id()).err(), Some(Error::Busy));
    rt.panic();
    rt.flush_ended(|_| true);
    // Physical debt is real and survives Core panic; acknowledge the reused key.
    let off = [0x2081_4600];
    owner
        .apply(&mut rt, Packets::new(&off).next().unwrap().unwrap())
        .unwrap();
    assert_eq!(rt.expression(source), Err(Error::StaleHandle));
    let old = owner.id();
    owner.unbind(&mut rt).unwrap();
    let fresh = bind(&mut rt, 0);
    assert_ne!(fresh.id(), old);
}

#[test]
fn binding_conflicts_and_terminal_receipt_leases_use_real_generations_without_heap_work() {
    let mut rt = runtime(None);
    let owner = bind(&mut rt, 0);
    let duplicate = Mpe::new(&rt, 0, 0, Zone::Lower, 15, 8).unwrap();
    assert_eq!(duplicate.bind(&mut rt).err(), Some(Error::InvalidInput));
    let mut second =
        Mpe::new_in(&rt, rt.performance(1).unwrap(), 0, 0, Zone::Lower, 15, 8).unwrap();
    second.enable_host_channel_projection().unwrap();
    let _second = second.bind(&mut rt).unwrap();
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 69,
        external_id: Some(77),
    };
    let source = Input {
        protocol: Protocol::Clap,
        port: 4,
        group: 0,
        channel: 13,
        ..input
    };
    owner
        .trigger_silent_with_source(&mut rt, 0, input, source, 1., 0.)
        .unwrap();
    support::without_heap(|| {
        owner.queue_all_physical_releases(&mut rt).unwrap();
        let delivery = owner.next_terminal_release(&mut rt).unwrap().unwrap();
        assert_eq!(delivery.release.source, source);
        assert_eq!(owner.unbind(&mut rt).err(), Some(Error::Busy));
        assert!(
            rt.receiver(owner.id()).is_ok(),
            "callback query must see the actual owner, never an extracted state"
        );
        owner.complete_terminal_release(&mut rt, delivery).unwrap();
        assert_eq!(
            owner.complete_terminal_release(&mut rt, delivery),
            Err(Error::StaleHandle)
        );
    });
}

#[test]
fn receiver_reserve_growth_cannot_revive_a_stale_facade_in_the_same_runtime() {
    let mut rt = runtime(None);
    let owner = bind(&mut rt, 0);
    let stale = owner;
    let old = owner.id();
    let unbound = owner.unbind(&mut rt).unwrap();
    rt = rt.with_receiver_capacity(8).unwrap();
    // A smaller minimum reserve also cannot discard generation history.
    rt = rt.with_receiver_capacity(2).unwrap();
    let fresh = unbound.bind(&mut rt).unwrap();
    assert_ne!(fresh.id(), old);
    let words = wire(0x90, 1, 69, 0xffff_0000);
    let before = (rt.note_count(), rt.expression_count(), rt.now());
    assert_eq!(
        stale.apply(&mut rt, Packets::new(&words).next().unwrap().unwrap()),
        Err(sampler_midi::ApplyError::Core(Error::StaleHandle))
    );
    assert_eq!((rt.note_count(), rt.expression_count(), rt.now()), before);
    assert_eq!(stale.unbind(&mut rt).err(), Some(Error::StaleHandle));
    assert!(matches!(apply(&fresh, &mut rt, words), Applied::Started(_)));
}

#[test]
fn public_physical_dispatch_updates_the_actual_registered_components_once_and_keeps_wire_versions_independent()
 {
    let mut actual = runtime(None);
    let owner = bind(&mut actual, 0);
    owner.set_bend_range(&mut actual, Some(2)).unwrap();
    let first = [0x2091_457f];
    let Applied::Started(a) = owner
        .apply(&mut actual, Packets::new(&first).next().unwrap().unwrap())
        .unwrap()
    else {
        panic!()
    };
    let second = [0x2092_457f];
    let Applied::Started(b) = owner
        .apply(&mut actual, Packets::new(&second).next().unwrap().unwrap())
        .unwrap()
    else {
        panic!()
    };
    let manager = [0x20e0_0000];
    owner
        .apply(&mut actual, Packets::new(&manager).next().unwrap().unwrap())
        .unwrap();
    assert_eq!(
        (
            expression(&actual, a).pitch_semitones,
            expression(&actual, b).pitch_semitones
        ),
        (-2., -2.)
    );
    let perf = actual.performance(0).unwrap();
    let address = ChannelAddress {
        protocol: Protocol::Midi1,
        port: 0,
        group: 0,
        channel: 1,
    };
    support::without_heap(|| {
        actual
            .dispatch_controller(perf, address, 1 << 1, 128, 0)
            .unwrap();
    });
    assert_eq!(
        (
            expression(&actual, a).pitch_semitones,
            expression(&actual, b).pitch_semitones
        ),
        (-4., -2.),
        "identical numeric manager/member values are distinct authored components"
    );
    let c = start(&owner, &mut actual, 1, 69);
    assert_eq!(
        expression(&actual, c).pitch_semitones,
        0.,
        "ordinary MIDI2 is a distinct real domain"
    );
    let future = [0x2091_467f];
    let Applied::Started(future) = owner
        .apply(&mut actual, Packets::new(&future).next().unwrap().unwrap())
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(expression(&actual, future).pitch_semitones, -4.);
    actual.release(c).unwrap();
    actual.release(future).unwrap();
    let mut reference = runtime(None);
    let ra = reference
        .trigger_pitched(
            Input {
                protocol: Protocol::Midi1,
                port: 0,
                group: 0,
                channel: 1,
                key: 69,
                external_id: None,
            },
            NotePitch::Key(69),
            1.,
            Expression {
                pitch_semitones: -4.,
                ..Expression::default()
            },
        )
        .unwrap();
    let rb = reference
        .trigger_pitched(
            Input {
                protocol: Protocol::Midi1,
                port: 0,
                group: 0,
                channel: 2,
                key: 69,
                external_id: None,
            },
            NotePitch::Key(69),
            1.,
            Expression {
                pitch_semitones: -2.,
                ..Expression::default()
            },
        )
        .unwrap();
    assert_ne!(ra, rb);
    assert_pcm(&mut actual, &mut reference, 256);
}

#[test]
fn public_physical_dispatch_is_captured_before_consume_or_exact_native_wait() {
    for consume in [true, false] {
        let code = if consume {
            vec![I::SuppressController]
        } else {
            vec![I::Wait(4), I::ForwardController]
        };
        let mut rt = runtime(Some(code));
        let owner = bind(&mut rt, 0);
        let note = start(&owner, &mut rt, 3, 69);
        let perf = rt.performance(0).unwrap();
        let source = ChannelAddress {
            protocol: Protocol::Midi2,
            port: 0,
            group: 0,
            channel: 3,
        };
        let value = 0x8123_a5b7;
        rt.dispatch_controller(perf, source, 1 << 3, 129, value)
            .unwrap();
        assert_eq!(expression(&rt, note).pressure, 0);
        assert_eq!(rt.channel_input_controller(perf, source, 129), Ok(value));
        rt.render(&mut [[0.; 2]; 4]).unwrap();
        rt.render(&mut []).unwrap();
        assert_eq!(
            expression(&rt, note).pressure,
            if consume { 0 } else { value }
        );
        let future = start(&owner, &mut rt, 3, 70);
        assert_eq!(
            expression(&rt, future).pressure,
            if consume { 0 } else { value }
        );
        assert_eq!(
            rt.channel_controller(perf, source, 129),
            Ok(if consume { 0 } else { value })
        );
    }
}


#[test]
fn bound_registered_pitch_owns_current_future_detach_attribute_and_manual_followers(){
    let mut actual=runtime(None);let receiver=bind(&mut actual,0);let mut reference=runtime(None);
    let mut pitch=wire(0,1,60,69u32<<25);pitch[0]|=3;assert!(matches!(apply(&receiver,&mut actual,pitch),Applied::Expression{..}));
    let root=start(&receiver,&mut actual,1,60);
    let expected=reference.trigger_pitched(Input{protocol:Protocol::Native,port:0,group:0,channel:1,key:60,external_id:Some(601)},NotePitch::Absolute(69.),1.,Expression::default()).unwrap();
    let follower=actual.child(root,72,1.,false,Inheritance::Expression).unwrap();actual.forward_attack(follower).unwrap();
    let expected_follower=reference.trigger_pitched(Input{external_id:Some(602),key:72,..input_for_registered()},NotePitch::Absolute(81.),1.,Expression::default()).unwrap();
    support::without_heap(||{
        let mut a=[[0.;2];64];let mut b=a;actual.render(&mut a).unwrap();reference.render(&mut b).unwrap();for (x,y) in a.iter().zip(b){assert!((x[0]-y[0]).abs()<2e-6);}
        let mut update=wire(0,1,60,70u32<<25);update[0]|=3;apply(&receiver,&mut actual,update);
        for note in [expected,expected_follower]{reference.set_expression(reference.expression_id(note).unwrap(),Expression{pitch_semitones:1.,..Expression::default()}).unwrap();}
        actual.render(&mut a).unwrap();reference.render(&mut b).unwrap();for (x,y) in a.iter().zip(b){assert!((x[0]-y[0]).abs()<2e-6);}
        let mut management=wire(0xf0,1,60,0);management[0]|=3;apply(&receiver,&mut actual,management);
        let fresh=start(&receiver,&mut actual,1,60);assert_ne!(actual.expression_id(root).unwrap(),actual.expression_id(fresh).unwrap());
        assert_eq!(actual.note(fresh).unwrap().0,60,"PNCC3 selection remains the original logical key");
        assert_eq!(actual.intrinsic_pitch_base(actual.expression_id(fresh).unwrap()),Ok((60.,None)));
        assert_eq!(actual.intrinsic_pitch_base(actual.expression_id(root).unwrap()),Ok((60.,Some(70.))));
        assert_eq!(actual.intrinsic_pitch_base(actual.expression_id(follower).unwrap()),Ok((60.,Some(70.))),"actual expression lineage freezes the detached current base");
    });
}
fn input_for_registered()->Input{Input{protocol:Protocol::Native,port:0,group:0,channel:1,key:60,external_id:None}}


#[test]
fn bound_per_note_reset_validates_the_joint_base_and_bend_before_either_publication(){
    let mut actual=runtime(None);let receiver=bind(&mut actual,0);let mut reference=runtime(None);
    let mut on=wire(0x90,1,60,0xffff_0000|80*512);on[0]|=3;
    let Applied::Started(note)=apply(&receiver,&mut actual,on)else{panic!()};
    let expected=reference.trigger_pitched(Input{external_id:Some(701),..input_for_registered()},NotePitch::Absolute(80.),1.,Expression::default()).unwrap();
    support::without_heap(||{
        apply(&receiver,&mut actual,wire(0x60,1,60,0));
        reference.set_expression(reference.expression_id(expected).unwrap(),Expression{pitch_semitones:-48.,..Expression::default()}).unwrap();
        let mut pitch=wire(0,1,60,120u32<<25);pitch[0]|=3;apply(&receiver,&mut actual,pitch);
        reference.set_expression(reference.expression_id(expected).unwrap(),Expression{pitch_semitones:-8.,..Expression::default()}).unwrap();
        let mut got=[[0.;2];64];let mut oracle=got;actual.render(&mut got).unwrap();reference.render(&mut oracle).unwrap();for (x,y) in got.iter().zip(oracle){assert!((x[0]-y[0]).abs()<2e-6);}
        let before=actual.now();let mut reset=wire(0xf0,1,60,0);reset[0]|=1;apply(&receiver,&mut actual,reset);
        assert_eq!(actual.now(),before);assert_eq!(actual.intrinsic_pitch_base(actual.expression_id(note).unwrap()),Ok((80.,None)));
        reference.set_expression(reference.expression_id(expected).unwrap(),Expression::default()).unwrap();
        actual.render(&mut got).unwrap();reference.render(&mut oracle).unwrap();for (x,y) in got.iter().zip(oracle){assert!((x[0]-y[0]).abs()<2e-6);}
    });
}
