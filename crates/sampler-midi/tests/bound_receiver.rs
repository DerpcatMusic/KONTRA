use sampler_core::{Instruction as I,*};
use sampler_midi::{Applied,BoundMpe,Mpe,Packets,Zone};
#[path="../../sampler-core/tests/support/mod.rs"]
mod support;

fn runtime(code:Option<Vec<I>>)->Runtime { runtime_with_root(code,69) }
fn runtime_with_root(code:Option<Vec<I>>,root:u8)->Runtime {
    let pcm=Pcm::new(48000,(0..16384).map(|frame|{
        let value=(frame as f64*std::f64::consts::TAU*440./48000.).sin() as f32;[value,value]
    }).collect()).unwrap();
    let mut prepared=Prepared::new(48000,vec![pcm],vec![Region{sample:0,key_low:48,key_high:84,root_key:Some(root),velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::new(0,0,0,1.,0).unwrap(),playback:Playback::default()}],37).unwrap();
    if let Some(code)=code {
        prepared=prepared.with_programs(vec![Program::new(code).unwrap().with_wait_lifetime(WaitLifetime::Callback)],None).unwrap().with_controller_program(0).unwrap();
    }
    let mut limits=Limits::for_plan(&prepared,8,8);limits.channels=48;limits.performances=2;limits.expressions=16;limits.commands=32;
    Runtime::new(prepared,limits).unwrap().with_receiver_capacity(4).unwrap()
}
fn wire(status:u8,channel:u8,key:u8,value:u32)->[u32;2]{[0x4000_0000|(u32::from(status|channel)<<16)|(u32::from(key)<<8),value]}
fn apply(owner:&BoundMpe,rt:&mut Runtime,words:[u32;2])->Applied {owner.apply(rt,Packets::new(&words).next().unwrap().unwrap()).unwrap()}
fn bind(rt:&mut Runtime,group:u8)->BoundMpe {
    let mut facade=Mpe::new(rt,0,group,Zone::Lower,15,8).unwrap();facade.enable_host_channel_projection().unwrap();facade.use_midi2_channel_voice().unwrap();facade.bind(rt).unwrap()
}
fn start(owner:&BoundMpe,rt:&mut Runtime,channel:u8,key:u8)->NoteId {
    let Applied::Started(note)=apply(owner,rt,wire(0x90,channel,key,0xffff_0000))else{panic!("not started")};note
}
fn expression(rt:&Runtime,note:NoteId)->Expression{rt.expression(rt.expression_id(note).unwrap()).unwrap()}
fn assert_pcm(a:&mut Runtime,b:&mut Runtime,count:usize){
    let mut a_frames=[[0.;2];64];let mut b_frames=[[0.;2];64];let mut at=0;
    while at<count{let n=(count-at).min(64);a.render(&mut a_frames[..n]).unwrap();b.render(&mut b_frames[..n]).unwrap();assert_eq!(&a_frames[..n],&b_frames[..n],"PCM differs at block {at}");at+=n;}
}

#[test]
fn captured_pressure_is_consumed_before_canonical_components_and_future_admission() {
    let mut rt=runtime(Some(vec![I::SuppressController]));let owner=bind(&mut rt,0);
    let first=start(&owner,&mut rt,3,69);let before=expression(&rt,first);
    support::without_heap(||{
        apply(&owner,&mut rt,wire(0xd0,3,0,0x8123_4567));
        assert_eq!(expression(&rt,first),before);
        let performance=rt.performance(0).unwrap();let address=ChannelAddress{protocol:Protocol::Midi2,port:0,group:0,channel:3};
        assert_eq!(rt.channel_input_controller(performance,address,129),Ok(0x8123_4567));
        assert_eq!(rt.channel_controller(performance,address,129),Ok(0));
        let next=start(&owner,&mut rt,3,70);assert_eq!(expression(&rt,next).pressure,0);
    });
}

#[test]
fn waited_original_pitch_commits_at_exact_frame_and_retains_all_wire_bits() {
    let mut actual=runtime(Some(vec![I::Wait(4),I::ForwardController]));let owner=bind(&mut actual,0);
    let mut reference=runtime(None);let reference_owner=bind(&mut reference,0);
    let a=start(&owner,&mut actual,5,69);let b=start(&reference_owner,&mut reference,5,69);
    let raw=0x8123_4567;
    apply(&owner,&mut actual,wire(0xe0,5,0,raw));
    assert_eq!(expression(&actual,a).pitch_semitones,0.);
    assert_pcm(&mut actual,&mut reference,4);
    // The reference is an independent ordinary no-stage receiver, not the new
    // staged projection helper and not a duplicate expected-value cache.
    apply(&reference_owner,&mut reference,wire(0xe0,5,0,raw));
    assert_pcm(&mut actual,&mut reference,256);
    assert_eq!(expression(&actual,a),expression(&reference,b));
    assert_eq!(expression(&actual,a).pitch_semitones,2.*f64::from(raw-0x8000_0000)/2147483647.);
    assert_ne!(expression(&actual,a).pitch_semitones,2.*f64::from((raw>>18)-8192)/8191.);
}

#[test]
fn same_value_script_replacement_is_explicit_script_precision_not_original_wire_provenance() {
    let mut rt=runtime(Some(vec![I::ReadControllerNumber{local:0},I::ReadControllerValue{local:1},I::SuppressController,I::WriteController{controller:0,value:1}]));
    let owner=bind(&mut rt,0);let note=start(&owner,&mut rt,4,69);let raw=0x8123_4567;
    apply(&owner,&mut rt,wire(0xe0,4,0,raw));
    let reduced=raw>>18;
    assert_eq!(expression(&rt,note).pitch_semitones,2.*f64::from(reduced-8192)/8191.);
    assert_ne!(expression(&rt,note).pitch_semitones,2.*f64::from(raw-0x8000_0000)/2147483647.);
}

#[test]
fn detached_manual_follower_keeps_its_frozen_member_and_live_manager_after_channel_reuse() {
    // Key70 + 50 semitones remains below the existing MAX_STEP16 with root84.
    // The original root69 fixture exceeded admission before its ownership assertion.
    let mut rt=runtime_with_root(None,84);let owner=bind(&mut rt,0);
    let on=[0x2091_457f];let Applied::Started(parent)=owner.apply(&mut rt,Packets::new(&on).next().unwrap().unwrap()).unwrap()else{panic!()};
    let source=rt.expression_id(parent).unwrap();
    let child=rt.child(parent,69,1.,false,Inheritance::Expression).unwrap();rt.start(child,0,rt.now(),1.).unwrap();
    let off=[0x2081_4500];owner.apply(&mut rt,Packets::new(&off).next().unwrap().unwrap()).unwrap();
    let new=[0x2091_467f];let Applied::Started(reused)=owner.apply(&mut rt,Packets::new(&new).next().unwrap().unwrap()).unwrap()else{panic!()};
    let bend=[0x20e1_7f7f];owner.apply(&mut rt,Packets::new(&bend).next().unwrap().unwrap()).unwrap();
    assert_eq!(rt.expression(source).unwrap().pitch_semitones,0.,"released member snapshot must not take reused-channel bend");
    assert_eq!(expression(&rt,reused).pitch_semitones,48.);
    let manager=[0x20e0_7f7f];owner.apply(&mut rt,Packets::new(&manager).next().unwrap().unwrap()).unwrap();
    assert_eq!(rt.expression(source).unwrap().pitch_semitones,2.);
    assert_eq!(expression(&rt,child).pitch_semitones,2.);
    assert_eq!(expression(&rt,reused).pitch_semitones,50.);
    assert_eq!(rt.unbind_receiver(owner.id()).err(),Some(Error::Busy));
    rt.panic();rt.flush_ended(|_|true);
    // Physical debt is real and survives Core panic; acknowledge the reused key.
    let off=[0x2081_4600];owner.apply(&mut rt,Packets::new(&off).next().unwrap().unwrap()).unwrap();
    assert_eq!(rt.expression(source),Err(Error::StaleHandle));
    let old=owner.id();owner.unbind(&mut rt).unwrap();let fresh=bind(&mut rt,0);assert_ne!(fresh.id(),old);
}

#[test]
fn binding_conflicts_and_terminal_receipt_leases_use_real_generations_without_heap_work() {
    let mut rt=runtime(None);let owner=bind(&mut rt,0);
    let duplicate=Mpe::new(&rt,0,0,Zone::Lower,15,8).unwrap();
    assert_eq!(duplicate.bind(&mut rt).err(),Some(Error::InvalidInput));
    let mut second=Mpe::new_in(&rt,rt.performance(1).unwrap(),0,0,Zone::Lower,15,8).unwrap();second.enable_host_channel_projection().unwrap();let _second=second.bind(&mut rt).unwrap();
    let input=Input{protocol:Protocol::Native,port:0,group:0,channel:0,key:69,external_id:Some(77)};
    let source=Input{protocol:Protocol::Clap,port:4,group:0,channel:13,..input};
    owner.trigger_silent_with_source(&mut rt,0,input,source,1.,0.).unwrap();
    support::without_heap(||{
        owner.queue_all_physical_releases(&mut rt).unwrap();
        let delivery=owner.next_terminal_release(&mut rt).unwrap().unwrap();
        assert_eq!(delivery.release.source,source);
        assert_eq!(owner.unbind(&mut rt).err(),Some(Error::Busy));
        assert!(rt.receiver(owner.id()).is_ok(),"callback query must see the actual owner, never an extracted state");
        owner.complete_terminal_release(&mut rt,delivery).unwrap();
        assert_eq!(owner.complete_terminal_release(&mut rt,delivery),Err(Error::StaleHandle));
    });
}

#[test]
fn receiver_reserve_growth_cannot_revive_a_stale_facade_in_the_same_runtime() {
    let mut rt=runtime(None);let owner=bind(&mut rt,0);let stale=owner;let old=owner.id();
    let unbound=owner.unbind(&mut rt).unwrap();
    rt=rt.with_receiver_capacity(8).unwrap();
    // A smaller minimum reserve also cannot discard generation history.
    rt=rt.with_receiver_capacity(2).unwrap();
    let fresh=unbound.bind(&mut rt).unwrap();assert_ne!(fresh.id(),old);
    let words=wire(0x90,1,69,0xffff_0000);let before=(rt.note_count(),rt.expression_count(),rt.now());
    assert_eq!(stale.apply(&mut rt,Packets::new(&words).next().unwrap().unwrap()),Err(sampler_midi::ApplyError::Core(Error::StaleHandle)));
    assert_eq!((rt.note_count(),rt.expression_count(),rt.now()),before);
    assert_eq!(stale.unbind(&mut rt).err(),Some(Error::StaleHandle));
    assert!(matches!(apply(&fresh,&mut rt,words),Applied::Started(_)));
}
