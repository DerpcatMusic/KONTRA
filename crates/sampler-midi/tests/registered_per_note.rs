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
    let source=Pcm::new(48000,(0..32768).map(|frame| {
        let phase=f64::from(frame)*std::f64::consts::TAU*440./48000.;
        [phase.sin() as f32*0.1,phase.cos() as f32*0.1]
    }).collect()).unwrap();
    let plan=Prepared::new(48000,vec![source],vec![Region {sample:0,key_low:48,key_high:84,
        root_key:Some(69),velocity_low:0.,velocity_high:1.,gain:1.,
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
