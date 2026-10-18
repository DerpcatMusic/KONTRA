//! Authored public-receiver witnesses. References use the ordinary core with an
//! independently specified pitch, never the new receiver's key/cohort helpers.
use sampler_core::{Envelope, Error, Expression, Input, Limits, NoteId, NotePitch, Pcm, Playback, Prepared, Protocol, Region, Runtime};
use sampler_midi::{Applied, ApplyError, Ingress, Mpe, Packets, Version, Zone};
#[path = "../../sampler-core/tests/support/mod.rs"] mod support;

enum Receiver { Mpe(Box<Mpe>), Ingress(Box<Ingress>) }
impl Receiver {
    fn new(runtime: &Runtime, mpe: bool, port: u16, notes: usize) -> Self {
        if mpe {
            let mut receiver = Mpe::new(runtime, port, 3, Zone::Lower, 1, notes).unwrap();
            receiver.use_midi2_channel_voice().unwrap();
            Self::Mpe(Box::new(receiver))
        } else {
            let mut groups = [None;16]; groups[3] = Some(Version::Midi2);
            Self::Ingress(Box::new(Ingress::new_polyphonic(runtime, port, groups, notes).unwrap()))
        }
    }
    fn apply(&mut self, runtime: &mut Runtime, channel: u8, status: u8, a: u8, b: u8, data: u32) -> Result<Applied, ApplyError> {
        let words = [0x4300_0000 | u32::from(status | channel) << 16 | u32::from(a) << 8 | u32::from(b), data];
        let packet = Packets::new(&words).next().unwrap().unwrap();
        match self { Self::Mpe(receiver) => receiver.apply(runtime, packet), Self::Ingress(receiver) => receiver.apply(runtime, packet) }
    }
    fn on(&mut self, runtime: &mut Runtime, channel: u8, key: u8, velocity: u16, attribute: u8, value: u16) -> NoteId {
        let Applied::Started(note) = self.apply(runtime, channel, 0x90, key, attribute, u32::from(velocity) << 16 | u32::from(value)).unwrap() else { panic!("no real owner") };
        note
    }
}
fn runtime() -> Runtime {
    let pcm = Pcm::new(48000, (0..32768).map(|frame| {
        let phase = f64::from(frame) * std::f64::consts::TAU * 440. / 48000.;
        [phase.sin() as f32 * 0.1, phase.cos() as f32 * 0.1]
    }).collect()).unwrap();
    let prepared = Prepared::new(48000, vec![pcm], vec![Region { sample: 0,
        key_low: 48, key_high: 84, root_key: Some(69), velocity_low: 0., velocity_high: 1., gain: 1.,
        envelope: Envelope::new(0,0,0,1.,1024).unwrap(), playback: Playback::default() }], 37).unwrap();
    let limits = Limits::for_plan(&prepared, 8, 8); Runtime::new(prepared, limits).unwrap()
}
fn expression(runtime: &Runtime, note: NoteId) -> Expression { runtime.expression(runtime.expression_id(note).unwrap()).unwrap() }
fn input(protocol: Protocol, port: u16, channel: u8, key: u8) -> Input {
    Input { protocol, port, group: 3, channel, key, external_id: None }
}
fn positive(value: u32, range: f64) -> f64 { range * (f64::from(value) - 2_147_483_648.) / 2_147_483_647. }

#[test]
fn raw_attribute_pitch_uses_an_independent_ordinary_core_reference_and_does_not_retune_the_index() {
    for mpe in [false,true] {
        let mut actual = runtime(); let mut reference = runtime(); let mut receiver = Receiver::new(&actual, mpe, 7, 8);
        let note = receiver.on(&mut actual, 1, 60, 65535, 3, 69*512+128);
        let plain = reference.trigger_pitched(input(Protocol::Native, 7, 1, 69), NotePitch::Key(69), 1.,
            Expression { pitch_semitones: 0.25, ..Expression::default() }).unwrap();
        support::without_heap(|| {
            assert_eq!(actual.note_pitch(note), Ok(NotePitch::Absolute(69.25)));
            assert_eq!(actual.input_source(note), Ok(Some(input(Protocol::Midi2,7,1,60))));
            let mut got = [[0.;2];128]; let mut expected = got;
            actual.render(&mut got).unwrap(); reference.render(&mut expected).unwrap();
            for (a,b) in got.iter().zip(expected) { for channel in 0..2 { assert!((a[channel]-b[channel]).abs() < 2e-6); } }
            assert!(got.iter().flatten().any(|value| value.abs() > 0.01));
            assert!(reference.input_held(plain).unwrap());
            let next = receiver.on(&mut actual, 1, 60, 65535, 0, 0);
            assert_eq!(actual.note_pitch(next), Ok(NotePitch::Key(60)), "one-note attribute cannot become persistent key tuning");
            let released = receiver.apply(&mut actual,1,0x80,60,0,0).unwrap();
            assert!(matches!(released, Applied::Released { note: owner, .. } if owner == note));
            assert!(actual.input_held(next).unwrap());
            let unknown = receiver.on(&mut actual,1,61,65535,2,0x1234);
            assert_eq!(actual.note_pitch(unknown),Ok(NotePitch::Key(61)), "unknown profile data does not discard its note");
        });
    }
}

#[test]
fn zero_velocity_is_a_real_silent_midi2_gate_with_exact_release_velocity() {
    for mpe in [false,true] {
        let mut runtime = runtime(); let mut receiver = Receiver::new(&runtime,mpe,7,8);
        support::without_heap(|| {
            let note = receiver.on(&mut runtime,1,69,0,0,0);
            assert_eq!(runtime.note(note).unwrap().1,0.); assert!(runtime.input_held(note).unwrap());
            let mut output = [[0.;2];128]; runtime.render(&mut output).unwrap(); assert_eq!(output,[[0.;2];128]);
            let Applied::Released { note: released, velocity, .. } = receiver.apply(&mut runtime,1,0x80,69,0,0xffff_0000).unwrap() else { panic!() };
            assert_eq!(released,note); assert_eq!(velocity.map(sampler_midi::Value::normalized),Some(1.));
            assert!(!matches!(runtime.input_held(note),Ok(true)));
            assert_eq!(runtime.note_count(),1, "terminal ownership remains available for acknowledgement");
        });
    }
}

#[test]
fn per_note_components_and_management_cohorts_keep_overlap_tails_and_other_keys_independent() {
    for mpe in [false,true] {
        let mut runtime = runtime(); let mut receiver = Receiver::new(&runtime,mpe,7,8);
        // Q7.25 sensitivity6.25, not RPN0:0's cents byte layout.
        receiver.apply(&mut runtime,1,0x20,0,7,209_715_200).unwrap();
        receiver.apply(&mut runtime,1,0x60,60,0,0xa000_0000).unwrap();
        receiver.apply(&mut runtime,1,0xe0,0,0,0xa000_0000).unwrap();
        let a = receiver.on(&mut runtime,1,60,65535,0,0); let b = receiver.on(&mut runtime,1,60,65535,0,0);
        let other = receiver.on(&mut runtime,1,61,65535,0,0);
        let channel = positive(0xa000_0000,2.); let first = positive(0xa000_0000,6.25);
        let half = positive(0xc000_0000,6.25);
        support::without_heap(|| {
            assert_ne!(runtime.expression_id(a).unwrap(),runtime.expression_id(b).unwrap());
            assert!((expression(&runtime,a).pitch_semitones-(channel+first)).abs()<1e-12);
            assert_eq!(expression(&runtime,other).pitch_semitones,channel);
            receiver.apply(&mut runtime,1,0x80,60,0,0).unwrap(); assert!(!runtime.input_held(a).unwrap());
            receiver.apply(&mut runtime,1,0x60,60,0,0xc000_0000).unwrap();
            for note in [a,b] { assert!((expression(&runtime,note).pitch_semitones-(channel+half)).abs()<1e-12); }
            assert_eq!(expression(&runtime,other).pitch_semitones,channel);
            receiver.apply(&mut runtime,1,0xf0,60,2,0).unwrap(); // detach without reset
            let future = receiver.on(&mut runtime,1,60,65535,0,0);
            assert!((expression(&runtime,future).pitch_semitones-(channel+half)).abs()<1e-12);
            receiver.apply(&mut runtime,1,0x60,60,0,0x8000_0000).unwrap();
            assert!((expression(&runtime,a).pitch_semitones-(channel+half)).abs()<1e-12);
            assert!((expression(&runtime,b).pitch_semitones-(channel+half)).abs()<1e-12);
            assert!((expression(&runtime,future).pitch_semitones-channel).abs()<1e-12);
            receiver.apply(&mut runtime,1,0x60,60,0,0x4000_0000).unwrap();
            let frozen = expression(&runtime,future);
            receiver.apply(&mut runtime,1,0xf0,60,3,0).unwrap(); // detach THEN reset
            let fresh = receiver.on(&mut runtime,1,60,65535,0,0);
            assert!((expression(&runtime,fresh).pitch_semitones-channel).abs()<1e-12);
            assert_eq!(expression(&runtime,future),frozen);
            receiver.apply(&mut runtime,1,0x60,60,0,0xa000_0000).unwrap();
            assert_eq!(expression(&runtime,future),frozen);
            receiver.apply(&mut runtime,1,0xf0,60,1,0).unwrap(); // reset active cohort only
            assert!((expression(&runtime,fresh).pitch_semitones-channel).abs()<1e-12);
            assert_eq!(expression(&runtime,future),frozen);
            assert_eq!(expression(&runtime,other).pitch_semitones,channel);
            let identity = runtime.expression_id(fresh).unwrap();
            receiver.apply(&mut runtime,1,0xe0,0,0,0x8000_0000).unwrap();
            assert!((expression(&runtime,a).pitch_semitones-half).abs()<1e-12);
            assert!((expression(&runtime,fresh).pitch_semitones).abs()<1e-12);
            assert_eq!(runtime.expression_id(fresh),Ok(identity));
            assert_eq!(runtime.note_count(),5); assert_eq!(runtime.now(),0);
        });
    }
}

#[test]
fn rejected_per_note_sensitivity_keeps_current_and_future_owners_and_exact_pcm_unchanged() {
    for mpe in [false,true] {
        let mut actual = runtime(); let mut reference = runtime();
        let mut receiver = Receiver::new(&actual,mpe,7,8); let mut reference_receiver = Receiver::new(&reference,mpe,7,8);
        let seed = |receiver: &mut Receiver, runtime: &mut Runtime| {
            receiver.apply(runtime,1,0x20,0,7,0).unwrap(); receiver.apply(runtime,1,0x60,69,0,u32::MAX).unwrap();
            receiver.on(runtime,1,69,65535,0,0)
        };
        let note = seed(&mut receiver,&mut actual); seed(&mut reference_receiver,&mut reference);
        support::without_heap(|| {
            let before = expression(&actual,note); let owner = actual.expression_id(note).unwrap();
            assert_eq!(receiver.apply(&mut actual,1,0x20,0,7,96u32<<25),Err(ApplyError::Core(Error::InvalidInput)));
            assert_eq!(expression(&actual,note),before); assert_eq!(actual.expression_id(note),Ok(owner));
            assert_eq!((actual.now(),actual.voice_count(),actual.pending_commands()),(0,1,0));
            let mut got = [[0.;2];128]; let mut expected = got; actual.render(&mut got).unwrap(); reference.render(&mut expected).unwrap(); assert_eq!(got,expected);
            let future = receiver.on(&mut actual,1,69,65535,0,0); reference_receiver.on(&mut reference,1,69,65535,0,0);
            assert_eq!(expression(&actual,future).pitch_semitones,0.);
            actual.render(&mut got).unwrap(); reference.render(&mut expected).unwrap(); assert_eq!(got,expected);
        });
    }
}

#[test]
fn raw_pressure_and_channel_terminals_cannot_borrow_native_or_other_address_owners() {
    for mpe in [false,true] {
        let mut runtime = runtime(); let mut receiver = Receiver::new(&runtime,mpe,7,8); let mut foreign = Receiver::new(&runtime,mpe,9,8);
        let own = receiver.on(&mut runtime,1,69,65535,0,0); let sibling = receiver.on(&mut runtime,2,69,65535,0,0);
        let other_port = foreign.on(&mut runtime,1,69,65535,0,0);
        let native = runtime.trigger(input(Protocol::Native,7,1,69),69,1.).unwrap();
        support::without_heap(|| {
            receiver.apply(&mut runtime,1,0xa0,69,0,0x1234_5678).unwrap();
            assert_eq!(expression(&runtime,own).pressure,0x1234_5678);
            for note in [sibling,other_port,native] { assert_eq!(expression(&runtime,note).pressure,0); assert!(runtime.input_held(note).unwrap()); }
            assert!(matches!(receiver.apply(&mut runtime,1,0xb0,123,0,0),Ok(Applied::AllNotesOff { released:1 })));
            assert!(!runtime.input_held(own).unwrap());
            for note in [sibling,other_port,native] { assert!(runtime.input_held(note).unwrap()); }
            assert!(matches!(receiver.apply(&mut runtime,2,0xb0,120,0,0),Ok(Applied::AllSoundOff { stopped:1 })));
            assert!(!runtime.note(sibling).unwrap().2);
            for note in [other_port,native] { assert!(runtime.note(note).unwrap().2); }
            receiver.apply(&mut runtime,2,0x80,69,0,0).unwrap();
            assert!(!matches!(runtime.input_held(sibling),Ok(true)));
        });
    }
}

#[test]
fn prepared_owner_capacity_refuses_before_note_publication_and_terminal_work_stays_usable() {
    for mpe in [false,true] {
        let mut runtime = runtime(); let mut receiver = Receiver::new(&runtime,mpe,7,2);
        let a = receiver.on(&mut runtime,1,69,65535,0,0); let b = receiver.on(&mut runtime,2,69,65535,0,0);
        support::without_heap(|| {
            assert_eq!(receiver.apply(&mut runtime,3,0x90,69,0,0xffff_0000),Err(ApplyError::Core(Error::Capacity)));
            assert_eq!((runtime.note_count(),runtime.voice_count(),runtime.now()),(2,2,0));
            assert!(runtime.input_held(a).unwrap()); assert!(runtime.input_held(b).unwrap());
            receiver.apply(&mut runtime,1,0x80,69,0,0).unwrap(); receiver.apply(&mut runtime,2,0x80,69,0,0).unwrap();
            runtime.render(&mut [[0.;2];2048]).unwrap(); runtime.flush_ended(|_| true);
            let new = receiver.on(&mut runtime,3,69,65535,0,0); assert!(runtime.input_held(new).unwrap());
        });
        let mut groups = [None;16]; groups[3] = Some(Version::Midi2);
        assert!(matches!(Ingress::new_polyphonic(&runtime,7,groups,usize::MAX),Err(Error::Capacity)));
    }
}

#[test]
fn captured_physical_fifo_pairs_collapsed_channels_groups_and_protocols_after_core_cancellation() {
    let mut runtime=runtime();let mut receiver=Mpe::new(&runtime,7,3,Zone::Lower,15,8).unwrap();receiver.set_manual_active(false);receiver.use_midi2_channel_voice().unwrap();
    let words=[0x2390_4500|127]; // logical manager0, key69
    let words2=[0x4390_4500,0xffff_0000];
    let source_a=input(Protocol::Midi1,7,3,69);let source_b=input(Protocol::Midi1,7,4,69);
    let source_c=Input{group:9,..source_a};let source_d=Input{protocol:Protocol::Midi2,..source_a};
    let mut owners=[None;4];
    for (at,source) in [source_a,source_b,source_c,source_d].into_iter().enumerate() {
        let packet=if source.protocol==Protocol::Midi2 {Packets::new(&words2).next().unwrap().unwrap()} else {Packets::new(&words).next().unwrap().unwrap()};
        let Applied::Started(note)=receiver.apply_silent_with_source(&mut runtime,packet,source).unwrap() else {panic!()};owners[at]=Some(note);
    }
    let [a,b,c,d]=owners.map(Option::unwrap);runtime.cancel_input_note(a).unwrap();runtime.render(&mut []).unwrap();runtime.flush_ended(|_|true);
    let Applied::Started(reused)=receiver.apply_silent_with_source(&mut runtime,Packets::new(&words).next().unwrap().unwrap(),source_a).unwrap() else {panic!()};assert_ne!(a,reused);
    let off=[0x2380_4500];
    support::without_heap(|| {
        for (source,expected) in [(source_b,b),(source_c,c),(source_d,d),(source_a,a)] {
            let off2=[0x4380_4500,0];let packet=if source.protocol==Protocol::Midi2 {Packets::new(&off2).next().unwrap().unwrap()} else {Packets::new(&off).next().unwrap().unwrap()};
            let Applied::Released{note,..}=receiver.apply_silent_with_source(&mut runtime,packet,source).unwrap() else {panic!()};
            assert_eq!(note,expected,"exact original tuple must win over an earlier collapsed same-key owner");
        }
        assert!(runtime.input_held(reused).unwrap(),"retired generation's key-up must not close its reused source slot");
        let Applied::Released{note,..}=receiver.apply_silent_with_source(&mut runtime,Packets::new(&off).next().unwrap().unwrap(),source_a).unwrap() else {panic!()};assert_eq!(note,reused);
        assert_eq!(receiver.apply_silent_with_source(&mut runtime,Packets::new(&off).next().unwrap().unwrap(),source_a),Ok(Applied::Ignored));
    });
}

#[test]
fn all_notes_off_queues_once_but_hard_sound_kill_keeps_the_real_physical_terminal_owed() {
    let mut runtime=runtime();let mut receiver=Mpe::new(&runtime,7,3,Zone::Lower,15,4).unwrap();receiver.use_midi2_channel_voice().unwrap();
    let attack=[0x4391_4500,0xffff_0000];let source=input(Protocol::Midi2,7,1,69);
    let mut on=|receiver:&mut Mpe,runtime:&mut Runtime| {let Applied::Started(note)=receiver.apply_silent_with_source(runtime,Packets::new(&attack).next().unwrap().unwrap(),source).unwrap() else {panic!()};note};
    let a=on(&mut receiver,&mut runtime);let b=on(&mut receiver,&mut runtime);
    support::without_heap(|| {
        let terminal=[0x43b1_7b00,0];receiver.apply(&mut runtime,Packets::new(&terminal).next().unwrap().unwrap()).unwrap();
        let mut ids=[None;2];let mut n=0;receiver.drain_terminal_releases(|release|{assert_eq!(release.source,source);ids[n]=Some(release.note);n+=1;});assert_eq!(ids,[Some(a),Some(b)]);
        receiver.apply(&mut runtime,Packets::new(&terminal).next().unwrap().unwrap()).unwrap();receiver.drain_terminal_releases(|_|panic!("repeated synthetic release"));
        let off=[0x4381_4500,0];for expected in [a,b] {let Applied::Released{note,..}=receiver.apply_with_source(&mut runtime,Packets::new(&off).next().unwrap().unwrap(),source).unwrap() else {panic!()};assert_eq!(note,expected);}
        let killed=on(&mut receiver,&mut runtime);let hard=[0x43b1_7800,0];receiver.apply(&mut runtime,Packets::new(&hard).next().unwrap().unwrap()).unwrap();
        receiver.drain_terminal_releases(|_|panic!("hard kill must not invent physical key-up"));
        let Applied::Released{note,..}=receiver.apply_silent_with_source(&mut runtime,Packets::new(&off).next().unwrap().unwrap(),source).unwrap() else {panic!()};assert_eq!(note,killed);
    });
}

#[test]
fn enabled_ump_groups_and_midi_versions_keep_per_note_state_and_owner_cohorts_separate() {
    let mut runtime=runtime();let mut groups=[None;16];groups[3]=Some(Version::Midi2);groups[9]=Some(Version::Midi2);
    let mut ingress=Ingress::new_polyphonic(&runtime,7,groups,8).unwrap();
    let send=|receiver:&mut Ingress,runtime:&mut Runtime,group:u8,status:u8,data:u32| {
        let words=[0x4000_4500|u32::from(group)<<24|u32::from(status|1)<<16,data];receiver.apply(runtime,Packets::new(&words).next().unwrap().unwrap()).unwrap()
    };
    let Applied::Started(a)=send(&mut ingress,&mut runtime,3,0x90,0xffff_0000) else {panic!()};
    let Applied::Started(b)=send(&mut ingress,&mut runtime,9,0x90,0xffff_0000) else {panic!()};
    send(&mut ingress,&mut runtime,3,0x60,0x9000_0000);
    assert!((expression(&runtime,a).pitch_semitones-positive(0x9000_0000,48.)).abs()<1e-12);assert_eq!(expression(&runtime,b).pitch_semitones,0.);
    send(&mut ingress,&mut runtime,9,0xf0,0); // detach/reset flags are zero, no-op
    assert_eq!(expression(&runtime,b).pitch_semitones,0.);
    let mut mixed=Mpe::new(&runtime,7,3,Zone::Lower,2,4).unwrap();mixed.use_midi2_channel_voice().unwrap();
    let midi1=[0x2391_4500|127];let midi2=[0x4391_4500,0xffff_0000];
    let Applied::Started(wire1)=mixed.apply(&mut runtime,Packets::new(&midi1).next().unwrap().unwrap()).unwrap() else {panic!()};
    let Applied::Started(wire2)=mixed.apply(&mut runtime,Packets::new(&midi2).next().unwrap().unwrap()).unwrap() else {panic!()};
    let pn=[0x4361_4500,0x9000_0000];mixed.apply(&mut runtime,Packets::new(&pn).next().unwrap().unwrap()).unwrap();
    assert_eq!(expression(&runtime,wire1).pitch_semitones,0.);assert!((expression(&runtime,wire2).pitch_semitones-positive(0x9000_0000,48.)).abs()<1e-12);
    assert_eq!(runtime.input_source(wire1).unwrap().unwrap().protocol,Protocol::Midi1);assert_eq!(runtime.input_source(wire2).unwrap().unwrap().protocol,Protocol::Midi2);
}


#[test]
fn cc121_selectively_resets_channel_state_without_resetting_per_note_pitch_or_other_addresses() {
    for mpe in [false,true] {
        let mut rt=runtime();let mut receiver=Receiver::new(&rt,mpe,7,8);
        // Authored/global bank state is independent of the MIDI2 wire CC0,
        // which is deprecated and must remain ignored by both receivers.
        rt.set_controller(rt.performance(0).unwrap(),0,0x3456_789a).unwrap();
        receiver.apply(&mut rt,1,0x20,0,7,209_715_200).unwrap(); // PN sensitivity6.25
        receiver.apply(&mut rt,1,0x60,69,0,0xa000_0000).unwrap();
        receiver.apply(&mut rt,1,0x20,0,0,12<<25).unwrap();
        receiver.apply(&mut rt,1,0xe0,0,0,0xa000_0000).unwrap();
        receiver.apply(&mut rt,1,0xd0,0,0,0x1234_5678).unwrap();
        for (cc,value) in [(1,0x1234_5678),(11,0x4567_89ab),(7,0x7654_3210),(10,0x6543_210f),(74,0x5432_10fe),(0,0x3456_789a),(91,0x2345_6789)] {
            let result=receiver.apply(&mut rt,1,0xb0,cc,0,value).unwrap();if cc==0 {assert_eq!(result,Applied::Ignored);}
        }
        receiver.apply(&mut rt,2,0xe0,0,0,0xc000_0000).unwrap();
        receiver.apply(&mut rt,2,0xd0,0,0,0x9876_5432).unwrap();
        let note=receiver.on(&mut rt,1,69,65535,0,0);let sibling=receiver.on(&mut rt,2,70,65535,0,0);
        let sibling_before=expression(&rt,sibling);let identity=rt.expression_id(note).unwrap();
        let perf=rt.performance(0).unwrap();let address=input(Protocol::Midi2,7,1,69).channel_address();
        let old_timbre=expression(&rt,note).timbre;
        support::without_heap(|| {
            assert_eq!(receiver.apply(&mut rt,1,0xb0,121,0,0),Ok(Applied::ResetControllers));
            let value=expression(&rt,note);
            assert!((value.pitch_semitones-positive(0xa000_0000,6.25)).abs()<1e-12);
            assert_eq!((value.bend,value.pressure,value.timbre),(0.,0,old_timbre));
            assert_eq!(rt.expression_id(note),Ok(identity));assert!(rt.input_held(note).unwrap());
            assert_eq!(expression(&rt,sibling),sibling_before);
            for (cc,value) in [(1,0),(11,u32::MAX),(64,0),(66,0),(7,0x7654_3210),(10,0x6543_210f),(74,0x5432_10fe),(0,0x3456_789a),(91,0x2345_6789)] {
                assert_eq!(rt.channel_controller(perf,address,cc),Ok(value),"CC{cc} selective reset law");
                assert_eq!(rt.channel_input_controller(perf,address,cc),Ok(if cc==0 {0} else {value}),"ignored MIDI2 CC0 cannot enter the raw bank through an authored global write");
            }
            let fresh=receiver.on(&mut rt,1,69,65535,0,0);
            assert!((expression(&rt,fresh).pitch_semitones-positive(0xa000_0000,6.25)).abs()<1e-12);
            receiver.apply(&mut rt,1,0xe0,0,0,u32::MAX).unwrap();
            assert!((expression(&rt,note).pitch_semitones-(12.+positive(0xa000_0000,6.25))).abs()<1e-12,"RPN sensitivity is retained");
        });
    }
}

#[test]
fn rejected_cc121_keeps_pedals_current_future_controls_and_exact_reference_pcm() {
    for mpe in [false,true] {
        let mut actual=runtime();let mut reference=runtime();
        let mut receiver=Receiver::new(&actual,mpe,7,8);let mut independent=Receiver::new(&reference,mpe,7,8);
        let seed=|r:&mut Receiver,rt:&mut Runtime| {
            r.apply(rt,1,0xe0,0,0,0).unwrap();
            for (cc,value) in [(1,0x1234_5678),(11,0x4567_89ab),(7,0x7654_3210),(64,u32::MAX),(66,u32::MAX)] {r.apply(rt,1,0xb0,cc,0,value).unwrap();}
            let note=r.on(rt,1,69,65535,0,0);
            match r {Receiver::Mpe(owner)=>{owner.set_note_tuning(rt,note,49.).unwrap();},Receiver::Ingress(_)=>{let id=rt.expression_id(note).unwrap();let mut value=rt.expression(id).unwrap();value.pitch_semitones=47.;rt.set_expression(id,value).unwrap();}}
            let sustained=r.on(rt,1,70,65535,0,0);r.apply(rt,1,0x80,70,0,0).unwrap();
            (note,sustained)
        };
        let (note,sustained)=seed(&mut receiver,&mut actual);seed(&mut independent,&mut reference);
        let before=expression(&actual,note);let address=input(Protocol::Midi2,7,1,69).channel_address();let perf=actual.performance(0).unwrap();
        support::without_heap(|| {
            assert_eq!(receiver.apply(&mut actual,1,0xb0,121,0,0),Err(ApplyError::Core(Error::InvalidInput)));
            assert_eq!(expression(&actual,note),before);assert_eq!((actual.now(),actual.voice_count()),(0,2));
            assert!(!actual.input_held(sustained).unwrap());
            for (cc,value) in [(1,0x1234_5678),(11,0x4567_89ab),(7,0x7654_3210),(64,u32::MAX),(66,u32::MAX)] {assert_eq!(actual.channel_controller(perf,address,cc),Ok(value));}
            let mut got=[[0.;2];128];let mut expected=got;actual.render(&mut got).unwrap();reference.render(&mut expected).unwrap();assert_eq!(got,expected);
            let future=receiver.on(&mut actual,1,71,65535,0,0);independent.on(&mut reference,1,71,65535,0,0);
            assert_eq!(expression(&actual,future).pitch_semitones,-2.);
            actual.render(&mut got).unwrap();reference.render(&mut expected).unwrap();assert_eq!(got,expected);
        });
    }
}


#[test]
fn selected_mpe_profile_accepts_manager_reset_only_and_preserves_ranges_and_released_member_snapshots() {
    for (zone,manager,member) in [(Zone::Lower,0u8,1u8),(Zone::Upper,15,14)] {
        let mut rt=runtime();let mut mpe=Mpe::new(&rt,7,3,zone,2,8).unwrap();
        let send=|mpe:&mut Mpe,rt:&mut Runtime,status:u8,a:u8,b:u8| {
            let words=[0x2300_0000|u32::from(status)<<16|u32::from(a)<<8|u32::from(b)];
            mpe.apply(rt,Packets::new(&words).next().unwrap().unwrap())
        };
        for (cc,value) in [(101,0),(100,0),(6,12)] {send(&mut mpe,&mut rt,0xb0|member,cc,value).unwrap();}
        send(&mut mpe,&mut rt,0xe0|member,0,80).unwrap(); // bend14=10240, +quarter of12
        send(&mut mpe,&mut rt,0xe0|manager,0,80).unwrap(); // +quarter of2
        send(&mut mpe,&mut rt,0xd0|member,63,0).unwrap();
        let Applied::Started(held)=send(&mut mpe,&mut rt,0x90|member,69,127).unwrap() else {panic!()};
        let Applied::Started(tail)=send(&mut mpe,&mut rt,0x90|member,70,127).unwrap() else {panic!()};
        send(&mut mpe,&mut rt,0x80|member,70,0).unwrap();
        let before=expression(&rt,held);let frozen=expression(&rt,tail);
        support::without_heap(|| {
            assert_eq!(send(&mut mpe,&mut rt,0xb0|member,121,0),Ok(Applied::Ignored));
            assert_eq!(expression(&rt,held),before);assert_eq!(expression(&rt,tail),frozen);
            assert_eq!(send(&mut mpe,&mut rt,0xb0|manager,121,0),Ok(Applied::ResetControllers));
            let value=expression(&rt,held);assert_eq!((value.pitch_semitones,value.bend,value.pressure),(0.,0.,0));
            let old=expression(&rt,tail);assert!((old.pitch_semitones-12.*2048./8191.).abs()<1e-12);
            assert_eq!(old.pressure,frozen.pressure,"released member state freezes while manager component resets");
            // Reset closes the RPN transaction but retains its12-semitone value.
            send(&mut mpe,&mut rt,0xb0|member,6,3).unwrap();
            send(&mut mpe,&mut rt,0xe0|member,127,127).unwrap();
            assert_eq!(expression(&rt,held).pitch_semitones,12.);
            let Applied::Started(fresh)=send(&mut mpe,&mut rt,0x90|member,71,127).unwrap() else {panic!()};
            assert_eq!(expression(&rt,fresh).pitch_semitones,12.);
            assert_eq!(expression(&rt,tail),old);
        });
    }
}


#[test]
fn transport_stop_queues_all_exact_raw_pairs_after_retirement_once_and_absorbs_old_terminals() {
    let mut rt=runtime();let mut mpe=Mpe::new(&rt,7,3,Zone::Lower,15,8).unwrap();mpe.set_manual_active(false);mpe.use_midi2_channel_voice().unwrap();mpe.enable_host_channel_projection().unwrap();
    let wire1=[0x2390_457f];let wire2=[0x4390_4500,0xffff_0000];
    let sources=[input(Protocol::Midi1,7,3,69),input(Protocol::Midi1,7,4,69),input(Protocol::Midi2,7,5,69)];
    let mut owners=[None;3];
    for (index,source) in sources.into_iter().enumerate() {
        let packet=if source.protocol==Protocol::Midi2 {Packets::new(&wire2).next().unwrap().unwrap()} else {Packets::new(&wire1).next().unwrap().unwrap()};
        let Applied::Started(note)=mpe.apply_silent_with_source(&mut rt,packet,source).unwrap() else {panic!()};owners[index]=Some(note);
    }
    let native=Input{protocol:Protocol::Native,port:7,group:3,channel:0,key:69,external_id:Some(71)};
    let native_source=Input{protocol:Protocol::Clap,port:9,group:0,channel:3,key:69,external_id:Some(71)};
    let host=mpe.trigger_silent_with_source(&mut rt,0,native,native_source,1.,0.).unwrap();
    let owners=owners.map(Option::unwrap);rt.cancel_input_note(owners[0]).unwrap();rt.flush_ended(|_|true);
    support::without_heap(|| {
        mpe.validate_all_physical_releases().unwrap();assert_eq!(mpe.queue_all_physical_releases(),Ok(4));
        let mut index=0;mpe.drain_terminal_releases(|row|{if index<3 {assert_eq!((row.note,row.source),(owners[index],sources[index]));} else {assert_eq!((row.note,row.source),(host,native_source));} index+=1;});assert_eq!(index,4);
        assert!(rt.input_held(host).unwrap(),"queue publication alone never mutates the Native Core gate");
        rt.release_all_notes();rt.flush_ended(|_|true);
        assert_eq!(mpe.queue_all_physical_releases(),Ok(0));mpe.drain_terminal_releases(|_|panic!("repeated stop release"));
        let Applied::Started(fresh)=mpe.apply_silent_with_source(&mut rt,Packets::new(&wire1).next().unwrap().unwrap(),sources[0]).unwrap() else {panic!()};
        let off=[0x2380_4500];let off2=[0x4380_4500,0];
        for (index,source) in sources.into_iter().enumerate() {
            let packet=if source.protocol==Protocol::Midi2 {Packets::new(&off2).next().unwrap().unwrap()} else {Packets::new(&off).next().unwrap().unwrap()};
            let Applied::Released{note,..}=mpe.apply_silent_with_source(&mut rt,packet,source).unwrap() else {panic!()};assert_eq!(note,owners[index]);
            assert!(rt.input_held(fresh).unwrap(),"old physical key-up cannot close a reused logical owner");
        }
        let Applied::Released{note,..}=mpe.apply_silent_with_source(&mut rt,Packets::new(&off).next().unwrap().unwrap(),sources[0]).unwrap() else {panic!()};assert_eq!(note,fresh);
        assert!(!rt.input_held(fresh).unwrap_or(false));mpe.drain_terminal_releases(|_|panic!("real key-up must not manufacture another synthetic callback"));
        assert_eq!(mpe.release_native_physical(&mut rt,|_,candidate|candidate==native_source,Some(0.)),Ok(1));
        mpe.drain_terminal_releases(|_|panic!("real Native Off absorbs its already queued transport release"));
    });
}


#[test]
fn transport_stop_queue_capacity_refusal_preserves_raw_gate_and_existing_exact_pending_releases() {
    let mut rt=runtime();let mut mpe=Mpe::new(&rt,7,3,Zone::Lower,1,1).unwrap();mpe.use_midi2_channel_voice().unwrap();mpe.enable_host_channel_projection().unwrap();
    let mut old=[None;2];
    for at in 0..2 {
        let native=Input{protocol:Protocol::Native,port:7,group:3,channel:0,key:69,external_id:Some(at as i32)};
        let source=Input{protocol:Protocol::Clap,port:9,group:0,channel:3,key:69,external_id:Some(at as i32)};
        old[at]=Some(mpe.trigger_silent_with_source(&mut rt,0,native,source,1.,0.).unwrap());
        let cc123=[0x43b0_7b00,0];mpe.apply(&mut rt,Packets::new(&cc123).next().unwrap().unwrap()).unwrap();
        rt.flush_ended(|_|true);
        assert_eq!(mpe.release_native_physical(&mut rt,|_,candidate|candidate==source,None),Ok(1));
        // Retain both exact queued callbacks, but acknowledge each real key-up
        // before reusing this one-entry physical-debt budget.
    }
    let on=[0x4390_4500,0xffff_0000];let source=input(Protocol::Midi2,7,0,69);
    let Applied::Started(raw)=mpe.apply_silent_with_source(&mut rt,Packets::new(&on).next().unwrap().unwrap(),source).unwrap() else {panic!()};
    support::without_heap(|| {
        let before=expression(&rt,raw);
        assert_eq!(mpe.validate_all_physical_releases(),Err(Error::Capacity));assert_eq!(mpe.queue_all_physical_releases(),Err(Error::Capacity));
        assert!(rt.input_held(raw).unwrap());assert_eq!(expression(&rt,raw),before);assert_eq!(rt.now(),0);
        let mut index=0;mpe.drain_terminal_releases(|release|{assert_eq!(release.note,old[index].unwrap());index+=1;});assert_eq!(index,2);
        assert_eq!(mpe.queue_all_physical_releases(),Ok(1));let mut count=0;mpe.drain_terminal_releases(|release|{assert_eq!((release.note,release.source),(raw,source));count+=1;});assert_eq!(count,1);
        assert!(rt.input_held(raw).unwrap(),"queue preparation alone never mutates Core gates");
        rt.release_all_notes();assert!(!rt.input_held(raw).unwrap_or(false));
    });
}

#[test]
fn native_physical_source_survives_core_retirement_and_preserves_exact_terminal_velocity() {
    let mut rt=runtime();let mut mpe=Mpe::new(&rt,7,3,Zone::Lower,1,8).unwrap();mpe.enable_host_channel_projection().unwrap();
    let logical=Input{protocol:Protocol::Native,port:7,group:3,channel:0,key:69,external_id:Some(71)};
    let source=Input{protocol:Protocol::Clap,port:9,group:0,channel:13,key:69,external_id:Some(71)};
    let old=mpe.trigger_silent_with_source(&mut rt,0,logical,source,1.,0.).unwrap();
    rt.cancel_input_note(old).unwrap();rt.flush_ended(|_|true);
    assert!(rt.note(old).is_err(),"the real Core owner must be retired before the physical release");
    support::without_heap(|| {
        assert!(mpe.has_native_physical_note(old));assert!(mpe.has_native_physical_source(source));
        assert_eq!(mpe.trigger_silent_with_source(&mut rt,0,logical,source,1.,0.),Err(ApplyError::Core(Error::DuplicateInput)));
        for wrong in [Input{protocol:Protocol::Vst3,..source},Input{port:10,..source},Input{channel:12,..source},Input{external_id:Some(72),..source}] {
            assert_eq!(mpe.release_native_physical(&mut rt,|_,candidate|candidate==wrong,Some(0.)),Ok(0));
        }
        mpe.drain_terminal_releases(|_|panic!("another source cannot release the retired owner"));
        assert_eq!(mpe.release_native_physical(&mut rt,|_,candidate|candidate==source,Some(0.25)),Ok(1));
        let mut count=0;mpe.drain_terminal_releases(|release|{assert_eq!((release.note,release.source,release.velocity),(old,source,Some(0.25)));count+=1;});assert_eq!(count,1);
        assert!(!mpe.has_native_physical_source(source));
        assert_eq!(mpe.release_native_physical(&mut rt,|_,candidate|candidate==source,Some(1.)),Ok(0));
        let fresh=mpe.trigger_silent_with_source(&mut rt,0,logical,source,1.,0.).unwrap();assert_ne!(fresh,old);assert!(rt.input_held(fresh).unwrap());
        assert_eq!(mpe.release_native_physical(&mut rt,|id,_|id==old,None),Ok(0));assert!(rt.input_held(fresh).unwrap());
    });
}

#[test]
fn source_backed_native_synthetic_and_choke_terminals_keep_distinct_ids_and_real_keyup_debt() {
    let mut rt=runtime();let mut mpe=Mpe::new(&rt,7,3,Zone::Lower,1,8).unwrap();mpe.enable_host_channel_projection().unwrap();
    let logical=Input{protocol:Protocol::Native,port:7,group:3,channel:0,key:69,external_id:Some(11)};
    let source=Input{protocol:Protocol::Clap,port:9,group:0,channel:13,key:69,external_id:Some(11)};
    let a=mpe.trigger_silent_with_source(&mut rt,0,logical,source,1.,0.).unwrap();
    let b=mpe.trigger_silent_with_source(&mut rt,0,Input{external_id:Some(12),..logical},Input{external_id:Some(12),..source},1.,0.).unwrap();
    support::without_heap(|| {
        assert_eq!(mpe.choke_native_physical(&mut rt,|id,_|id==a),Ok(1));
        assert_eq!(mpe.choke_native_physical(&mut rt,|id,_|id==a),Ok(0));
        rt.flush_ended(|_|true);assert!(rt.note(a).is_err());assert!(rt.input_held(b).unwrap());
        mpe.drain_terminal_releases(|_|panic!("choke is not physical key-up"));
        assert_eq!(mpe.queue_all_physical_releases(),Ok(2));let mut seen=[false;2];
        mpe.drain_terminal_releases(|release|{assert_eq!(release.velocity,None);if release.note==a {seen[0]=true;} else {assert_eq!(release.note,b);seen[1]=true;}});assert_eq!(seen,[true,true]);
        assert_eq!(mpe.queue_all_physical_releases(),Ok(0));
        assert_eq!(mpe.release_native_physical(&mut rt,|_,candidate|candidate==source,Some(0.)),Ok(1));
        mpe.drain_terminal_releases(|_|panic!("real Off absorbs the already synthesized old release"));
        assert!(mpe.has_native_physical_note(b));
        assert_eq!(mpe.release_native_physical(&mut rt,|_,candidate|candidate.protocol==Protocol::Clap&&candidate.port==9&&candidate.channel==13&&candidate.key==69,None),Ok(1));
        mpe.drain_terminal_releases(|_|panic!("wildcard Off cannot repeat a synthetic callback"));
        assert_eq!(mpe.native_physical_slots(),0);
    });
}

#[test]
fn native_and_raw_physical_capacity_are_separate_bounded_debts_without_callback_allocation() {
    let mut rt=runtime();let mut mpe=Mpe::new(&rt,7,3,Zone::Lower,1,1).unwrap();mpe.enable_host_channel_projection().unwrap();mpe.use_midi2_channel_voice().unwrap();
    let logical=Input{protocol:Protocol::Native,port:7,group:3,channel:0,key:69,external_id:Some(1)};
    let source=Input{protocol:Protocol::Vst3,port:9,group:0,channel:4,key:69,external_id:Some(1)};
    let owner=mpe.trigger_silent_with_source(&mut rt,0,logical,source,1.,0.).unwrap();rt.cancel_input_note(owner).unwrap();rt.flush_ended(|_|true);
    support::without_heap(|| {
        assert_eq!(mpe.trigger_silent_with_source(&mut rt,0,Input{external_id:Some(2),..logical},Input{external_id:Some(2),..source},1.,0.),Err(ApplyError::Core(Error::Capacity)));
        assert_eq!(mpe.trigger_silent(&mut rt,0,Input{external_id:Some(3),..logical},1.),Err(ApplyError::Core(Error::Capacity)),"unpaired legacy Native is included in the same Native debt budget");
        let words=[0x4390_4500,0xffff_0000];let raw_source=input(Protocol::Midi2,7,1,69);
        let Applied::Started(raw)=mpe.apply_silent_with_source(&mut rt,Packets::new(&words).next().unwrap().unwrap(),raw_source).unwrap() else {panic!()};
        assert_eq!(mpe.native_physical_slots(),2);assert!(rt.input_held(raw).unwrap());
        assert_eq!(mpe.queue_all_physical_releases(),Ok(2));let mut count=0;mpe.drain_terminal_releases(|_|count+=1);assert_eq!(count,2);
        assert_eq!(mpe.release_native_physical(&mut rt,|_,candidate|candidate==source,None),Ok(1));mpe.drain_terminal_releases(|_|panic!("synthetic Native release repeated"));
        assert_eq!(mpe.native_physical_slots(),1);assert!(rt.input_held(raw).unwrap(),"queue admission never publishes a Core release");
    });
}


#[test]
fn legacy_no_source_native_does_not_invent_a_clap_physical_owner() {
    let mut rt=runtime();let mut mpe=Mpe::new(&rt,7,3,Zone::Lower,1,8).unwrap();mpe.enable_host_channel_projection().unwrap();
    let logical=Input{protocol:Protocol::Native,port:7,group:3,channel:0,key:69,external_id:Some(1)};
    let owner=mpe.trigger_silent(&mut rt,0,logical,1.).unwrap();
    support::without_heap(|| {
        assert!(!mpe.has_native_physical_note(owner));assert_eq!(mpe.native_physical_slots(),0);
        assert_eq!(mpe.queue_all_physical_releases(),Ok(0));
        mpe.drain_terminal_releases(|_|panic!("legacy Native admission has no fabricated physical source"));
        assert!(rt.input_held(owner).unwrap());
    });
}


#[test]
fn ordinary_midi2_cc74_uses_its_exact_channel_bank_and_captured_script_boundary() {
    use sampler_core::{Instruction as I, Program, ScriptInstanceId, WaitLifetime};
    for consume in [false,true] {
        let mut code=vec![I::ReadControllerNumber{local:0},I::WriteScriptCell{cell:0,local:0},
            I::ReadControllerValue{local:1},I::WriteScriptCell{cell:1,local:1},
            I::ReadControllerChannel{local:2},I::WriteScriptCell{cell:2,local:2},
            I::ReadInputController{controller:0,local:3},I::WriteScriptCell{cell:3,local:3}];
        if consume {code.push(I::SuppressController);} else {code.push(I::ForwardController);}
        let plan=Prepared::new(48000,vec![],vec![],0).unwrap()
            .with_script_instances(vec![vec![0;4]]).unwrap()
            .with_programs(vec![Program::new(code).unwrap().with_wait_lifetime(WaitLifetime::Callback).with_script_instance(ScriptInstanceId(0))],None).unwrap()
            .with_controller_program(0).unwrap();
        let limits=Limits{notes:4,performances:1,channels:16,families:0,voices:0,expressions:4,
            decisions:0,commands:0,behaviors:4,behavior_cells:plan.behavior_local_count()*4,behavior_fuel:64,note_cells:0};
        let mut rt=Runtime::new(plan,limits).unwrap();let mut receiver=Receiver::new(&rt,true,7,4);
        let Receiver::Mpe(mpe)=&mut receiver else {unreachable!()};
        // An explicitly configured selected-MPE bridge must not rename an
        // ordinary MIDI2 channel controller into modulation CC1.
        mpe.set_timbre_controller(Some(1));
        let note=receiver.on(&mut rt,1,69,65535,0,0);let sibling=receiver.on(&mut rt,2,69,65535,0,0);
        let before=expression(&rt,note);let sibling_before=expression(&rt,sibling);
        let performance=rt.performance(0).unwrap();let address=input(Protocol::Midi2,7,1,69).channel_address();
        let other=input(Protocol::Midi2,7,2,69).channel_address();let active=rt.active_plan();let value=0x5432_10fe;
        support::without_heap(|| {
            assert_eq!(receiver.apply(&mut rt,1,0xb0,74,0,value),Ok(Applied::Controller));
            for (cell,expected) in [74,i64::from(value),1,i64::from(value)].into_iter().enumerate() {
                assert_eq!(rt.script_cell(active,ScriptInstanceId(0),cell as u32),Ok(expected),"script observes actual CC number/value/channel/input seed before downstream publication");
            }
            assert_eq!(rt.channel_input_controller(performance,address,74),Ok(value));
            assert_eq!(rt.channel_controller(performance,address,74),Ok(if consume {0} else {value}));
            assert_eq!(rt.channel_input_controller(performance,address,1),Ok(0));assert_eq!(rt.channel_controller(performance,address,1),Ok(0));
            assert_eq!(rt.channel_controller(performance,other,74),Ok(0));
            assert_eq!(expression(&rt,note),before);assert_eq!(expression(&rt,sibling),sibling_before);
            let future=receiver.on(&mut rt,1,70,65535,0,0);assert_eq!(expression(&rt,future).timbre,before.timbre,"ordinary CC74 is not a hidden MPE source component for future notes");
            assert!(rt.input_held(note).unwrap());assert!(rt.input_held(sibling).unwrap());assert!(rt.input_held(future).unwrap());
        });
    }
}
