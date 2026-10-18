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

#[test]
fn bound_program_state_is_canonical_and_has_no_native_host_or_other_version_alias(){
    let mut rt=runtime(None);let owner=bind(&mut rt,0);let performance=rt.performance(0).unwrap();
    let address=ChannelAddress{protocol:Protocol::Midi2,port:0,group:0,channel:3};
    let first=start(&owner,&mut rt,3,69);let before=expression(&rt,first);
    support::without_heap(||{
        assert!(matches!(apply(&owner,&mut rt,[0x40c3_0001,0x7f00_057e]),Applied::ProgramChange{targets:1}));
        assert_eq!(rt.program_selection(performance,address).unwrap(),Some(ProgramSelection{program:127,bank:[5,126]}));
        assert_eq!(rt.program_selection(performance,ChannelAddress{protocol:Protocol::Native,..address}).unwrap(),None);
        assert_eq!(rt.program_selection(performance,ChannelAddress{protocol:Protocol::Midi1,..address}).unwrap(),None);
        assert_eq!(expression(&rt,first),before,"Program receipt does not invent a preset reload or voice reset");
        apply(&owner,&mut rt,[0x40c3_0000,0x0000_0000]);
        assert_eq!(rt.program_selection(performance,address).unwrap(),Some(ProgramSelection{program:0,bank:[5,126]}));
    });
    let mut reference=runtime(None);let reference_owner=bind(&mut reference,0);start(&reference_owner,&mut reference,3,69);assert_pcm(&mut rt,&mut reference,256);
}

fn raw_fifo_rejection_runtime()->sampler_core::Runtime{
    let samples=(0..8192).map(|frame|{let x=frame as f64;[(0.07*x).sin()as f32*0.25,(0.031*x).cos()as f32*0.125]}).collect::<Vec<_>>();
    let plan=sampler_core::Prepared::new(48000,vec![sampler_core::Pcm::new(48000,samples.into_boxed_slice()).unwrap()],vec![sampler_core::Region{sample:0,key_low:60,key_high:60,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,envelope:sampler_core::Envelope::new(0,0,0,1.,0).unwrap(),playback:sampler_core::Playback::default()}],1).unwrap();
    let mut limits=sampler_core::Limits::for_plan(&plan,2,4);limits.channels=48;limits.expressions=2;
    sampler_core::Runtime::new(plan,limits).unwrap().with_receiver_capacity(1).unwrap()
}
fn raw_fifo_packet(midi2:bool,on:bool)->[u32;2]{
    [(if midi2{0x4000_0000}else{0x2000_0000})|(if on{0x0091_0000}else{0x0081_0000})|0x3c00|if !midi2&&on{127}else{0},if midi2&&on{0xffff_0000}else{0}]
}
fn raw_fifo_source(midi2:bool,port:u16,group:u8)->sampler_core::Input{
    sampler_core::Input{protocol:if midi2{sampler_core::Protocol::Midi2}else{sampler_core::Protocol::Midi1},port,group,channel:1,key:60,external_id:None}
}
fn raw_fifo_filler(channel:u8)->sampler_core::Input{
    sampler_core::Input{protocol:sampler_core::Protocol::Native,port:0,group:0,channel,key:60,external_id:None}
}
fn raw_fifo_pcm(actual:&mut sampler_core::Runtime,reference:&mut sampler_core::Runtime,frames:usize){
    let mut a=[[0.;2];64];let mut b=a;let mut remaining=frames;let mut energy=0.;
    while remaining!=0{let n=remaining.min(64);actual.render(&mut a[..n]).unwrap();reference.render(&mut b[..n]).unwrap();for(frame,want)in a[..n].iter().zip(&b[..n]){for c in 0..2{assert_eq!(frame[c],want[c],"exact source PCM/phase diverged after a rejected physical attack terminal");energy+=f64::from(want[c]).abs();}}remaining-=n;}
    assert!(energy>0.,"reference source must be independently sounding");assert_eq!(actual.now(),reference.now());
}

fn raw_fifo_bound(rt:&mut sampler_core::Runtime)->sampler_midi::BoundMpe{
    let mut owner=Mpe::new(rt,0,0,Zone::Lower,15,8).unwrap();owner.use_midi2_channel_voice().unwrap();owner.bind(rt).unwrap()
}
fn raw_fifo_bound_apply(owner:BoundMpe,rt:&mut Runtime,midi2:bool,on:bool,source:Input)->Result<Applied,sampler_midi::ApplyError>{
    let words=raw_fifo_packet(midi2,on);owner.apply_with_source(rt,Packets::new(&words).next().unwrap().unwrap(),source)
}
#[test]
fn raw_rejected_older_bound_attack_cannot_release_a_newer_same_physical_tuple(){
    for midi2 in [false,true]{
        let mut rt=raw_fifo_rejection_runtime();let owner=raw_fifo_bound(&mut rt);let mut reference=raw_fifo_rejection_runtime();let reference_owner=raw_fifo_bound(&mut reference);let source=raw_fifo_source(midi2,7,3);
        let first=rt.note_on(raw_fifo_filler(14),60,1.).unwrap();let second=rt.note_on(raw_fifo_filler(15),60,1.).unwrap();let before=(rt.now(),rt.note_count(),rt.voice_count(),rt.expression_count());
        support::without_heap(||{assert_eq!(raw_fifo_bound_apply(owner,&mut rt,midi2,true,source),Err(sampler_midi::ApplyError::Core(Error::Capacity)));});assert_eq!((rt.now(),rt.note_count(),rt.voice_count(),rt.expression_count()),before);
        rt.cancel_input_note(first).unwrap();rt.cancel_input_note(second).unwrap();rt.flush_ended(|_|true);assert_eq!((rt.note_count(),rt.expression_count()),(0,0));
        let Applied::Started(note)=raw_fifo_bound_apply(owner,&mut rt,midi2,true,source).unwrap()else{panic!("newer raw attack not admitted after actual unrelated capacity freed")};let Applied::Started(reference_note)=raw_fifo_bound_apply(reference_owner,&mut reference,midi2,true,source).unwrap()else{panic!()};
        assert_eq!(rt.input_source(note),Ok(Some(source)));let expression=rt.expression_id(note).unwrap();let value=rt.expression(expression).unwrap();
        support::without_heap(||{raw_fifo_pcm(&mut rt,&mut reference,64);let now=rt.now();let result=raw_fifo_bound_apply(owner,&mut rt,midi2,false,source).unwrap();assert!(rt.note(note).unwrap().2,"older refused raw attack stole the newer accepted physical gate");assert_eq!(result,Applied::Ignored);assert_eq!(rt.now(),now);assert_eq!(rt.expression(expression),Ok(value));assert_eq!(rt.voice_count(),1);raw_fifo_pcm(&mut rt,&mut reference,128);});
        assert!(owner.next_terminal_release(&mut rt).unwrap().is_none(),"rejected attack has no script terminal receipt or fabricated NoteId");
        let Applied::Released{note:released,..}=raw_fifo_bound_apply(owner,&mut rt,midi2,false,source).unwrap()else{panic!("newer physical Off must release its actual owner")};assert_eq!(released,note);
        let Applied::Released{note:released,..}=raw_fifo_bound_apply(reference_owner,&mut reference,midi2,false,source).unwrap()else{panic!()};assert_eq!(released,reference_note);
        let mut a=[[0.;2];64];let mut b=a;rt.render(&mut a).unwrap();reference.render(&mut b).unwrap();assert_eq!(a,b);rt.flush_ended(|_|true);reference.flush_ended(|_|true);assert_eq!((rt.note_count(),rt.voice_count(),rt.expression_count()),(0,0,0));assert_eq!(rt.expression(expression),Err(Error::StaleHandle));assert_eq!(rt.stats().nonfinite_frames,0);assert!(owner.next_terminal_release(&mut rt).unwrap().is_none());
    }
}
#[test]
fn raw_accepted_older_rejected_middle_and_newer_bound_attacks_keep_exact_fifo(){
    for midi2 in [false,true]{
        let mut rt=raw_fifo_rejection_runtime();let owner=raw_fifo_bound(&mut rt);let mut reference=raw_fifo_rejection_runtime();let reference_owner=raw_fifo_bound(&mut reference);let source=raw_fifo_source(midi2,2,4);
        let Applied::Started(older)=raw_fifo_bound_apply(owner,&mut rt,midi2,true,source).unwrap()else{panic!()};let Applied::Started(ref_older)=raw_fifo_bound_apply(reference_owner,&mut reference,midi2,true,source).unwrap()else{panic!()};
        let filler=rt.note_on(raw_fifo_filler(15),60,1.).unwrap();assert_eq!(raw_fifo_bound_apply(owner,&mut rt,midi2,true,source),Err(sampler_midi::ApplyError::Core(Error::Capacity)));assert!(rt.note(older).unwrap().2);
        rt.cancel_input_note(filler).unwrap();rt.flush_ended(|_|true);let Applied::Started(newer)=raw_fifo_bound_apply(owner,&mut rt,midi2,true,source).unwrap()else{panic!()};let Applied::Started(ref_newer)=raw_fifo_bound_apply(reference_owner,&mut reference,midi2,true,source).unwrap()else{panic!()};assert_ne!(older,newer);
        support::without_heap(||{raw_fifo_pcm(&mut rt,&mut reference,64);let Applied::Released{note,..}=raw_fifo_bound_apply(owner,&mut rt,midi2,false,source).unwrap()else{panic!()};assert_eq!(note,older);let Applied::Released{note,..}=raw_fifo_bound_apply(reference_owner,&mut reference,midi2,false,source).unwrap()else{panic!()};assert_eq!(note,ref_older);assert!(!rt.note(older).unwrap().2);assert!(rt.note(newer).unwrap().2);raw_fifo_pcm(&mut rt,&mut reference,64);
            let outcome=raw_fifo_bound_apply(owner,&mut rt,midi2,false,source).unwrap();assert!(rt.note(newer).unwrap().2,"rejected middle attack released the newer owner after correctly releasing the accepted older owner");assert_eq!(outcome,Applied::Ignored);raw_fifo_pcm(&mut rt,&mut reference,128);
        });
        let Applied::Released{note,..}=raw_fifo_bound_apply(owner,&mut rt,midi2,false,source).unwrap()else{panic!()};assert_eq!(note,newer);let Applied::Released{note,..}=raw_fifo_bound_apply(reference_owner,&mut reference,midi2,false,source).unwrap()else{panic!()};assert_eq!(note,ref_newer);
        rt.render(&mut [[0.;2];64]).unwrap();rt.flush_ended(|_|true);assert_eq!((rt.note_count(),rt.voice_count(),rt.expression_count()),(0,0,0));assert_eq!(rt.stats().nonfinite_frames,0);assert!(owner.next_terminal_release(&mut rt).unwrap().is_none());
    }
}
#[test]
fn raw_rejected_bound_decisions_keep_two_actual_ports_and_groups_independent(){
    for midi2 in [false,true]{
        let mut rt=raw_fifo_rejection_runtime();let owner=raw_fifo_bound(&mut rt);let mut reference=raw_fifo_rejection_runtime();let reference_owner=raw_fifo_bound(&mut reference);let sources=[raw_fifo_source(midi2,2,3),raw_fifo_source(midi2,7,4)];
        let a=rt.note_on(raw_fifo_filler(14),60,1.).unwrap();let b=rt.note_on(raw_fifo_filler(15),60,1.).unwrap();for source in sources{assert_eq!(raw_fifo_bound_apply(owner,&mut rt,midi2,true,source),Err(sampler_midi::ApplyError::Core(Error::Capacity)));}
        rt.cancel_input_note(a).unwrap();rt.cancel_input_note(b).unwrap();rt.flush_ended(|_|true);let mut notes=[None;2];
        for(index,source)in sources.into_iter().enumerate(){let Applied::Started(note)=raw_fifo_bound_apply(owner,&mut rt,midi2,true,source).unwrap()else{panic!()};notes[index]=Some(note);assert_eq!(rt.input_source(note),Ok(Some(source)));assert!(matches!(raw_fifo_bound_apply(reference_owner,&mut reference,midi2,true,source),Ok(Applied::Started(_))));}
        support::without_heap(||{raw_fifo_pcm(&mut rt,&mut reference,64);for source in sources.into_iter().rev(){let result=raw_fifo_bound_apply(owner,&mut rt,midi2,false,source).unwrap();for note in notes{assert!(rt.note(note.unwrap()).unwrap().2,"a refused source's old Off cannot release either live port/group");}assert_eq!(result,Applied::Ignored);}raw_fifo_pcm(&mut rt,&mut reference,64);});
        for(index,source)in sources.into_iter().enumerate(){let Applied::Released{note,..}=raw_fifo_bound_apply(owner,&mut rt,midi2,false,source).unwrap()else{panic!()};assert_eq!(Some(note),notes[index]);assert!(matches!(raw_fifo_bound_apply(reference_owner,&mut reference,midi2,false,source),Ok(Applied::Released{..})));if index==0{assert!(rt.note(notes[1].unwrap()).unwrap().2);}}
        rt.render(&mut [[0.;2];64]).unwrap();rt.flush_ended(|_|true);assert_eq!((rt.note_count(),rt.expression_count()),(0,0));assert_eq!(rt.stats().nonfinite_frames,0);assert!(owner.next_terminal_release(&mut rt).unwrap().is_none());
    }
}


#[test]
fn raw_synchronous_bound_malformed_sources_leave_no_rows_and_preserve_valid_remap_and_host_ids() {
    for midi2 in [false,true] {
        let mut rt=raw_fifo_rejection_runtime();let mut unbound=Mpe::new(&rt,0,0,Zone::Lower,15,8).unwrap();unbound.use_midi2_channel_voice().unwrap();unbound.enable_host_channel_projection().unwrap();let owner=unbound.bind(&mut rt).unwrap();
        let source=raw_fifo_source(midi2,7,3);let logical=Input{port:0,group:0,channel:1,key:60,..source};let before=owner.wire_status(&rt).unwrap();let clock=rt.now();
        support::without_heap(||{for bad in [Input{group:16,..source},Input{channel:16,..source},Input{key:128,..source}] {
            assert_eq!(raw_fifo_bound_apply(owner,&mut rt,midi2,true,bad),Err(sampler_midi::ApplyError::Core(Error::InvalidInput)));
            assert_eq!(owner.trigger_with_source(&mut rt,1,logical,bad,0.5,0.),Err(Error::InvalidInput));
            assert_eq!(owner.wire_status(&rt).unwrap(),before);assert_eq!((rt.note_count(),rt.voice_count(),rt.expression_count(),rt.now()),(0,0,0,clock));
        }});
        let valid=Input{group:15,channel:15,key:127,..source};let Applied::Started(note)=raw_fifo_bound_apply(owner,&mut rt,midi2,true,valid).unwrap()else{panic!()};assert_eq!(rt.input_source(note),Ok(Some(valid)));assert_eq!(rt.note(note).unwrap().0,60);
        let host=Input{protocol:Protocol::Clap,port:9,group:15,channel:15,key:127,external_id:Some(8901)};let target=Input{protocol:Protocol::Native,external_id:Some(8901),..logical};let native=owner.trigger_with_source(&mut rt,1,target,host,1.,0.).unwrap();assert_eq!(rt.input_source(native),Ok(Some(host)));
        owner.release_native_physical(&mut rt,|id,input|id==native&&input==host,None).unwrap();
        assert!(matches!(raw_fifo_bound_apply(owner,&mut rt,midi2,false,valid),Ok(Applied::Released{note:released,..})if released==note));
        rt.render(&mut [[0.;2];64]).unwrap();rt.flush_ended(|_|true);assert_eq!(owner.wire_status(&rt).unwrap().decisions,0);assert_eq!((rt.note_count(),rt.expression_count(),rt.voice_count()),(0,0,0));assert!(!owner.wire_status(&rt).unwrap().fenced_unknown_debt);assert_eq!(rt.stats().nonfinite_frames,0);
    }
}

#[test]
fn bound_explicit_midi_host_ids_and_remapped_host_sources_preserve_public_trigger_law(){
    for protocol in [Protocol::Midi1,Protocol::Midi2] {
        let mut rt=runtime(None);let owner=bind(&mut rt,0);
        let first=Input{protocol,port:0,group:0,channel:1,key:60,external_id:Some(9301)};
        let second=Input{external_id:Some(9302),..first};
        let logical=Input{key:61,external_id:None,..first};
        let source=Input{protocol:Protocol::Vst3,port:12,group:4,channel:9,key:55,external_id:Some(9303)};
        let raw_input=Input{external_id:None,..first};
        support::without_heap(||{
            let a=owner.trigger_with_tuning(&mut rt,1,first,1.,0.25).unwrap();
            let b=owner.trigger_with_tuning(&mut rt,1,second,1.,-0.5).unwrap();
            let c=owner.trigger_with_source(&mut rt,1,logical,source,0.75,0.375).unwrap();
            assert_eq!(rt.input_source(c),Ok(Some(source)));assert_eq!(rt.note(c).unwrap().0,61);assert_eq!(rt.note(c).unwrap().1,0.75);
            let on=if protocol==Protocol::Midi1{[0x2091_3c7f,0]}else{wire(0x90,1,60,0xffff_0000)};
            let Applied::Started(raw)=owner.apply(&mut rt,Packets::new(if protocol==Protocol::Midi1{&on[..1]}else{&on[..]}).next().unwrap().unwrap()).unwrap()else{panic!("raw admission")};
            let bend=if protocol==Protocol::Midi1{[0x20e1_0060,0]}else{wire(0xe0,1,0,0xa000_0000)};
            owner.apply(&mut rt,Packets::new(if protocol==Protocol::Midi1{&bend[..1]}else{&bend[..]}).next().unwrap().unwrap()).unwrap();
            let expected=if protocol==Protocol::Midi1{48.*4096./8191.}else{2.*536_870_912./2_147_483_647.};
            for (note,tuning) in [(a,0.25),(b,-0.5),(c,0.375),(raw,0.)]{assert_eq!(expression(&rt,note).pitch_semitones,expected+tuning);}
            let off=if protocol==Protocol::Midi1{[0x2081_3c00,0]}else{wire(0x80,1,60,0)};
            let released=owner.apply(&mut rt,Packets::new(if protocol==Protocol::Midi1{&off[..1]}else{&off[..]}).next().unwrap().unwrap()).unwrap();
            assert!(matches!(released,Applied::Released{note,..} if note==raw));
            assert!(rt.input_held(a).unwrap()&&rt.input_held(b).unwrap()&&rt.input_held(c).unwrap());
            rt.note_off(first,None).unwrap();assert!(!rt.input_held(a).unwrap());assert!(rt.input_held(b).unwrap()&&rt.input_held(c).unwrap());
            rt.note_off(second,None).unwrap();rt.note_off(logical,None).unwrap();assert!(!rt.input_held(b).unwrap()&&!rt.input_held(c).unwrap());
            assert_eq!(rt.input_source(c),Ok(Some(source)));
            let before=(rt.now(),rt.note_count(),rt.expression_count(),rt.voice_count(),owner.wire_status(&rt).unwrap());
            let bad=owner.apply_with_source(&mut rt,Packets::new(if protocol==Protocol::Midi1{&on[..1]}else{&on[..]}).next().unwrap().unwrap(),first);
            assert_eq!(bad,Err(sampler_midi::ApplyError::Core(Error::InvalidInput)));
            assert_eq!((rt.now(),rt.note_count(),rt.expression_count(),rt.voice_count(),owner.wire_status(&rt).unwrap()),before);
            rt.panic();let mut seen=0u8;rt.flush_ended(|input|{let bit=if input==first{1}else if input==second{2}else if input==logical{4}else{assert_eq!(input,raw_input);8};assert_eq!(seen&bit,0);seen|=bit;true});assert_eq!(seen,15);
        });
    }
}
