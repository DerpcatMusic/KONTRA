//! Independent PCM witnesses for M2-104-UM v1.1.1 section 7.4.15.2:
//! Registered Per-Note Controller 3 sets persistent absolute Q7.25 pitch.
//! Attribute 3 overrides that state for one note; management owns cohorts.
use sampler_core::{Envelope, Expression, Input, Limits, NoteId, NotePitch, Pcm, Playback,
    Prepared, Protocol, Region, Runtime};
use sampler_midi::{Applied, Ingress, Mpe, Packets, Version, Zone};
#[path = "../../sampler-core/tests/support/mod.rs"] mod support;

enum Receiver { Mpe(Box<Mpe>), Ingress(Box<Ingress>) }
impl Receiver {
    fn new(runtime: &Runtime, mpe: bool) -> Self {
        if mpe {
            let mut receiver=Mpe::new(runtime,7,3,Zone::Lower,1,8).unwrap();
            receiver.use_midi2_channel_voice().unwrap();Self::Mpe(Box::new(receiver))
        } else {
            let mut groups=[None;16];groups[3]=Some(Version::Midi2);
            Self::Ingress(Box::new(Ingress::new_polyphonic(runtime,7,groups,8).unwrap()))
        }
    }
    fn apply(&mut self, runtime:&mut Runtime, channel:u8, status:u8, key:u8, index:u8, value:u32)->Applied {
        let words=[0x4300_0000|u32::from(status|channel)<<16|u32::from(key)<<8|u32::from(index),value];
        let packet=Packets::new(&words).next().unwrap().unwrap();
        match self {Self::Mpe(receiver)=>receiver.apply(runtime,packet),
            Self::Ingress(receiver)=>receiver.apply(runtime,packet)}.unwrap()
    }
    fn pitch(&mut self, runtime:&mut Runtime, key:u8, value:u32) {
        let result=self.apply(runtime,1,0,key,3,value);
        assert!(!matches!(result,Applied::Unsupported|Applied::Ignored),
            "registered PNCC3 was discarded before its actual note-number receiver: {result:?}");
    }
    fn on(&mut self, runtime:&mut Runtime, key:u8, attribute:u16)->NoteId {
        let kind=if attribute==0 {0} else {3};
        let Applied::Started(note)=self.apply(runtime,1,0x90,key,kind,0xffff_0000|u32::from(attribute))
            else {panic!("missing physical MIDI2 note")};note
    }
}
fn runtime()->Runtime {
    runtime_rooted(69)
}
fn runtime_rooted(root:u8)->Runtime {
    let source=Pcm::new(48000,(0..32768).map(|frame| {
        let phase=f64::from(frame)*std::f64::consts::TAU*440./48000.;
        [phase.sin() as f32*0.1,phase.cos() as f32*0.1]
    }).collect()).unwrap();
    let plan=Prepared::new(48000,vec![source],vec![Region {sample:0,key_low:48,key_high:84,
        root_key:Some(root),velocity_low:0.,velocity_high:1.,gain:1.,
        envelope:Envelope::new(0,0,0,1.,1024).unwrap(),playback:Playback::default()}],37).unwrap();
    let limits=Limits::for_plan(&plan,8,8);Runtime::new(plan,limits).unwrap()
}
fn reference_note(runtime:&mut Runtime,key:u8,pitch:f64)->NoteId {
    runtime.trigger_pitched(Input {protocol:Protocol::Native,port:7,group:3,channel:1,key,
        external_id:None},NotePitch::Absolute(pitch),1.,Expression::default()).unwrap()
}
fn compare(actual:&mut Runtime,reference:&mut Runtime) {
    let mut got=[[0.;2];128];let mut expected=got;
    actual.render(&mut got).unwrap();reference.render(&mut expected).unwrap();
    assert!(expected.iter().flatten().any(|sample|sample.abs()>0.01));
    for (a,b) in got.iter().zip(expected) {for lane in 0..2 {
        assert!((a[lane]-b[lane]).abs()<2e-6,"registered per-note pitch PCM differs: {a:?} != {b:?}");
    }}
}

#[test]
fn registered_pitch_persists_and_attribute_pitch_overrides_only_one_note() {
    for mpe in [false,true] {
        let mut actual=runtime();let mut reference=runtime();let mut receiver=Receiver::new(&actual,mpe);
        support::without_heap(|| {
            receiver.pitch(&mut actual,60,(69u32<<25)+(1<<23)); // 69.25, independent integer Q7.25.
            let first=receiver.on(&mut actual,60,0);reference_note(&mut reference,60,69.25);
            assert_eq!(actual.input_source(first).unwrap().unwrap().key,60);
            compare(&mut actual,&mut reference);
            receiver.on(&mut actual,60,70*512+256);reference_note(&mut reference,60,70.5);
            compare(&mut actual,&mut reference);
            receiver.on(&mut actual,60,0);reference_note(&mut reference,60,69.25);
            compare(&mut actual,&mut reference);
            receiver.on(&mut actual,61,0);reference_note(&mut reference,61,61.);
            compare(&mut actual,&mut reference);
        });
    }
}

#[test]
fn registered_pitch_detach_then_reset_keeps_old_owner_and_changes_only_new_cohort() {
    for mpe in [false,true] {
        let mut actual=runtime();let mut reference=runtime();let mut receiver=Receiver::new(&actual,mpe);
        support::without_heap(|| {
            receiver.pitch(&mut actual,60,69u32<<25);
            let old=receiver.on(&mut actual,60,0);reference_note(&mut reference,60,69.);
            compare(&mut actual,&mut reference);
            receiver.apply(&mut actual,1,0xf0,60,3,0); // D then S, before the new note.
            let fresh=receiver.on(&mut actual,60,0);let plain=reference_note(&mut reference,60,60.);
            assert_ne!(actual.expression_id(old),actual.expression_id(fresh));
            compare(&mut actual,&mut reference);
            receiver.pitch(&mut actual,60,72u32<<25);
            reference.set_expression(reference.expression_id(plain).unwrap(),Expression {
                pitch_semitones:12.,..Expression::default()}).unwrap();
            compare(&mut actual,&mut reference);
            // CC121 does not reset MIDI2 per-note registered controllers.
            receiver.apply(&mut actual,1,0xb0,121,0,0);
            receiver.on(&mut actual,60,0);reference_note(&mut reference,60,72.);
            compare(&mut actual,&mut reference);
            assert!(actual.input_held(old).unwrap());assert!(actual.input_held(fresh).unwrap());
        });
    }
}


fn tuned_pitch_runtime(mode:u8,tuning:f64)->Runtime{
    let mut offsets=[0.;128];offsets[60]=tuning;offsets[72]=tuning+0.5;
    let source=|fixed:bool|Pcm::new(48000,(0..32768).map(|frame|{
        let phase=f64::from(frame)*std::f64::consts::TAU*440./48000.;
        if fixed{[0.,phase.cos() as f32*0.05]}else{[phase.sin() as f32*0.05,0.]}
    }).collect()).unwrap();
    let kinds:Vec<bool>=match mode{0=>vec![false,true],1=>vec![false],2=>vec![true],_=>panic!()};
    let sources=kinds.iter().map(|&fixed|source(fixed)).collect();
    let regions=kinds.iter().enumerate().map(|(sample,&fixed)|Region{sample,key_low:48,key_high:84,root_key:(!fixed).then_some(69),velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::new(0,0,0,1.,1024).unwrap(),playback:Playback::default()}).collect();
    let plan=Prepared::new_tuned(48000,sources,regions,37*kinds.len(),&sampler_core::Tuning::new(offsets).unwrap()).unwrap();
    let limits=Limits::for_plan(&plan,16,16);Runtime::new(plan,limits).unwrap()
}
fn compare_independent_sources(actual:&mut Runtime,tracked:&mut Runtime,fixed:&mut Runtime){
    let mut a=[[0.;2];128];let mut b=a;let mut c=a;actual.render(&mut a).unwrap();tracked.render(&mut b).unwrap();fixed.render(&mut c).unwrap();
    assert!(b.iter().flatten().any(|sample|sample.abs()>0.01));
    for ((actual,tracked),fixed) in a.iter().zip(b).zip(c){for lane in 0..2{assert!((actual[lane]-(tracked[lane]+fixed[lane])).abs()<2e-6,"tracked absolute/MTS and fixed source disagree: {actual:?} vs {tracked:?}+{fixed:?}");}}
}
fn receiver_pitch_result(receiver:&mut Receiver,runtime:&mut Runtime,key:u8,value:u32)->Result<Applied,sampler_midi::ApplyError>{
    let words=[0x4300_0000|0x0001_0000|u32::from(key)<<8|3,value];let packet=Packets::new(&words).next().unwrap().unwrap();
    match receiver{Receiver::Mpe(mpe)=>mpe.apply(runtime,packet),Receiver::Ingress(ingress)=>ingress.apply(runtime,packet)}
}
#[test]
fn registered_pitch_uses_actual_mts_base_and_does_not_retune_untracked_sources_or_reseek(){
    for mpe in [false,true]{
        let mut actual=tuned_pitch_runtime(0,0.75);let mut tracked=tuned_pitch_runtime(1,-0.25);let mut fixed=tuned_pitch_runtime(2,7.);
        let mut receiver=Receiver::new(&actual,mpe);
        receiver.pitch(&mut actual,60,(69u32<<25)+(1<<23));let physical=receiver.on(&mut actual,60,0);
        let reference=reference_note(&mut tracked,60,69.25);reference_note(&mut fixed,60,60.);
        support::without_heap(||{
            compare_independent_sources(&mut actual,&mut tracked,&mut fixed);
            let mut before=[None;2];let mut at=0;assert!(actual.visit_onset_voices(|voice|{before[at]=Some((voice.voice,voice.cursor.base_start,voice.cursor.traversal,voice.cursor.frame,voice.cursor.fraction));at+=1;true}));assert_eq!(at,2);
            let clock=actual.now();let voices=actual.voice_count();receiver.pitch(&mut actual,60,(72u32<<25)+(1<<24));
            let mut after=[None;2];let mut at=0;assert!(actual.visit_onset_voices(|voice|{after[at]=Some((voice.voice,voice.cursor.base_start,voice.cursor.traversal,voice.cursor.frame,voice.cursor.fraction));at+=1;true}));
            assert_eq!(after,before,"held absolute pitch cannot reposition or replace physical sources");assert_eq!((actual.now(),actual.voice_count()),(clock,voices));
            tracked.set_expression(tracked.expression_id(reference).unwrap(),Expression{pitch_semitones:3.25,..Expression::default()}).unwrap();
            compare_independent_sources(&mut actual,&mut tracked,&mut fixed);
            receiver.apply(&mut actual,1,0xf0,60,1,0); // S only: actual original MTS60.75.
            tracked.set_expression(tracked.expression_id(reference).unwrap(),Expression{pitch_semitones:-8.5,..Expression::default()}).unwrap();
            compare_independent_sources(&mut actual,&mut tracked,&mut fixed);assert_eq!(actual.input_source(physical).unwrap().unwrap().key,60);
        });
    }
}
#[test]
fn attribute_initial_override_is_not_reloaded_by_bend_but_later_registered_pitch_can_set_it(){
    for mpe in [false,true]{
        let mut actual=runtime();let mut reference=runtime();let mut receiver=Receiver::new(&actual,mpe);
        receiver.pitch(&mut actual,60,69u32<<25);let note=receiver.on(&mut actual,60,70*512+256);let expected=reference_note(&mut reference,60,70.5);
        support::without_heap(||{
            compare(&mut actual,&mut reference);
            receiver.apply(&mut actual,1,0xe0,0,0,u32::MAX);reference.set_expression(reference.expression_id(expected).unwrap(),Expression{pitch_semitones:2.,bend:1.,..Expression::default()}).unwrap();compare(&mut actual,&mut reference);
            receiver.pitch(&mut actual,60,72u32<<25);reference.set_expression(reference.expression_id(expected).unwrap(),Expression{pitch_semitones:3.5,bend:1.,..Expression::default()}).unwrap();compare(&mut actual,&mut reference);
            receiver.apply(&mut actual,1,0x20,0,7,4u32<<25);receiver.apply(&mut actual,1,0x60,60,0,u32::MAX);
            reference.set_expression(reference.expression_id(expected).unwrap(),Expression{pitch_semitones:7.5,bend:1.,..Expression::default()}).unwrap();compare(&mut actual,&mut reference);
            receiver.apply(&mut actual,1,0xf0,60,1,0); // Restore original Attribute70.5, retain channel+2.
            reference.set_expression(reference.expression_id(expected).unwrap(),Expression{pitch_semitones:2.,bend:1.,..Expression::default()}).unwrap();compare(&mut actual,&mut reference);
            assert!(actual.input_held(note).unwrap());
        });
    }
}
// Step is 2^((pitch - root)/12), limited to [1/256, 256]: the same 96-semitone window as the
// bend path. Root 31 puts the registered limit at 127 (root + 96), so u32::MAX (127.99...)
// is the first rejected pitch. Root 69 admits every registered pitch, and root 0 silences
// the reference by pushing the 440 Hz source above Nyquist, so neither can test rejection.
#[test]
#[ignore = "MAX_STEP is 2048 (132 semitones); no registered pitch (max 127.99) can exceed it now, so this rejection is unreachable. Re-enable if a smaller step limit returns."]
fn rejected_live_registered_pitch_keeps_current_pcm_and_future_key_default(){
    for mpe in [false,true]{
        let mut actual=runtime_rooted(31);let mut reference=runtime_rooted(31);let mut receiver=Receiver::new(&actual,mpe);
        receiver.pitch(&mut actual,60,69u32<<25);let held=receiver.on(&mut actual,60,0);reference_note(&mut reference,60,69.);
        support::without_heap(||{
            compare(&mut actual,&mut reference);let before=(actual.now(),actual.note_count(),actual.voice_count(),actual.expression(actual.expression_id(held).unwrap()).unwrap());
            assert!(matches!(receiver_pitch_result(&mut receiver,&mut actual,60,u32::MAX),Err(sampler_midi::ApplyError::Core(sampler_core::Error::InvalidInput))));
            assert_eq!((actual.now(),actual.note_count(),actual.voice_count(),actual.expression(actual.expression_id(held).unwrap()).unwrap()),before);
            compare(&mut actual,&mut reference);receiver.on(&mut actual,60,0);reference_note(&mut reference,60,69.);compare(&mut actual,&mut reference);
        });
    }
}
#[test]
fn intrinsic_pitch_follows_actual_tuned_child_interval_and_old_plan_authority(){
    let mut offsets=[0.;128];offsets[60]=0.75;offsets[72]=1.25;
    let source=Pcm::new(48000,(0..32768).map(|i|{let value=(i as f64*std::f64::consts::TAU*440./48000.).sin() as f32*0.1;[value;2]}).collect()).unwrap();
    let region=Region{sample:0,key_low:48,key_high:84,root_key:Some(69),velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::new(0,0,0,1.,0).unwrap(),playback:Playback::default()};
    let plan=Prepared::new_tuned(48000,vec![source.clone()],vec![region],37,&sampler_core::Tuning::new(offsets).unwrap()).unwrap();let limits=Limits::for_plan(&plan,8,8);
    let (mut actual,mut control)=Runtime::with_plan_updates(plan,limits,2,1).unwrap();let old=actual.active_plan();
    let root=actual.note_on(Input{protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:Some(77)},60,1.).unwrap();
    let child=actual.child(root,72,1.,false,sampler_core::Inheritance::Expression).unwrap();actual.forward_attack(child).unwrap();
    let reference_plan=Prepared::new(48000,vec![source],vec![region],37).unwrap();let limits=Limits::for_plan(&reference_plan,8,8);let mut reference=Runtime::new(reference_plan,limits).unwrap();let expected=reference_note(&mut reference,72,73.25);
    let replacement=Prepared::new(48000,vec![],vec![],0).unwrap();let request=control.submit(Box::new(replacement)).unwrap();assert_eq!(actual.poll_plan_update(),Ok(Some(request)));
    support::without_heap(||{
        compare(&mut actual,&mut reference);
        let expression=actual.expression_id(root).unwrap();actual.set_intrinsic_pitch(&[(expression,actual.expression(expression).unwrap())],Some(69.)).unwrap();
        // Actual old-plan interval73.25-60.75=12.5, so the child becomes81.5.
        reference.set_expression(reference.expression_id(expected).unwrap(),Expression{pitch_semitones:8.25,..Expression::default()}).unwrap();compare(&mut actual,&mut reference);
        actual.set_intrinsic_pitch(&[(expression,actual.expression(expression).unwrap())],None).unwrap();reference.set_expression(reference.expression_id(expected).unwrap(),Expression::default()).unwrap();compare(&mut actual,&mut reference);
        assert_eq!(actual.note_plan(child),Ok(old));assert_eq!(actual.collect_retired_plans(),0);
        actual.release(child).unwrap();actual.note_off_exact(root,None).unwrap();actual.render(&mut [[0.;2];64]).unwrap();actual.flush_ended(|_|true);assert_eq!(actual.collect_retired_plans(),1);
    });
}


#[test]
fn registered_pitch_retains_all_q7_25_bits_in_the_actual_owner_without_a_key_delta(){
    for mpe in [false,true]{
        let mut actual=tuned_pitch_runtime(1,0.75);let mut receiver=Receiver::new(&actual,mpe);
        let raw=(69u32<<25)+1;receiver.pitch(&mut actual,60,raw);let note=receiver.on(&mut actual,60,0);
        let expression=actual.expression_id(note).unwrap();
        assert_eq!(actual.intrinsic_pitch_base(expression),Ok((60.75,Some(f64::from(raw)/33_554_432.))));
        assert_eq!(actual.expression(expression).unwrap().pitch_semitones,0.,"absolute Q7.25 is not stored as a fake relative key offset");
        let attr=receiver.on(&mut actual,60,70*512+256);assert_eq!(actual.intrinsic_pitch_base(actual.expression_id(attr).unwrap()),Ok((70.5,None)));
        support::without_heap(||{
            let later=(71u32<<25)+3;receiver.pitch(&mut actual,60,later);
            assert_eq!(actual.intrinsic_pitch_base(expression),Ok((60.75,Some(f64::from(later)/33_554_432.))));
            assert_eq!(actual.intrinsic_pitch_base(actual.expression_id(attr).unwrap()),Ok((70.5,Some(f64::from(later)/33_554_432.))));
        });
    }
}

#[test]
fn reset_projects_the_new_base_and_relative_state_together_not_a_never_published_old_bend(){
    for mpe in [false,true]{
        let mut actual=runtime();let mut reference=runtime();let mut receiver=Receiver::new(&actual,mpe);
        receiver.on(&mut actual,60,80*512);let expected=reference_note(&mut reference,60,80.);
        support::without_heap(||{
            receiver.pitch(&mut actual,60,30u32<<25);reference.set_expression(reference.expression_id(expected).unwrap(),Expression{pitch_semitones:-50.,..Expression::default()}).unwrap();compare(&mut actual,&mut reference);
            receiver.apply(&mut actual,1,0x60,60,0,u32::MAX);reference.set_expression(reference.expression_id(expected).unwrap(),Expression{pitch_semitones:-2.,..Expression::default()}).unwrap();compare(&mut actual,&mut reference);
            // The valid commit is80+0. The never-published80+48 must not reject it.
            receiver.apply(&mut actual,1,0xf0,60,1,0);reference.set_expression(reference.expression_id(expected).unwrap(),Expression::default()).unwrap();compare(&mut actual,&mut reference);
            receiver.apply(&mut actual,1,0x60,60,0,0);reference.set_expression(reference.expression_id(expected).unwrap(),Expression{pitch_semitones:-48.,..Expression::default()}).unwrap();compare(&mut actual,&mut reference);
            receiver.pitch(&mut actual,60,120u32<<25);reference.set_expression(reference.expression_id(expected).unwrap(),Expression{pitch_semitones:-8.,..Expression::default()}).unwrap();compare(&mut actual,&mut reference);
            // Also avoid the invalid never-published120+0 projection.
            receiver.apply(&mut actual,1,0xf0,60,1,0);reference.set_expression(reference.expression_id(expected).unwrap(),Expression::default()).unwrap();compare(&mut actual,&mut reference);
        });
    }
}
