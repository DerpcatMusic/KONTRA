use sampler_core::{Envelope,Error,Expression,Inheritance,Input,Limits,Pcm,Playback,Prepared,Protocol,Region,Runtime};
mod support;

fn runtime(notes:usize)->Runtime {
    let prepared=Prepared::new(48000,
        vec![Pcm::new(48000,Box::from([[0.25;2];4096])).unwrap()],
        vec![Region{sample:0,key_low:60,key_high:60,root_key:None,
            velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::new(0,0,0,1.,0).unwrap(),
            playback:Playback::default()}],1).unwrap();
    let mut limits=Limits::for_plan(&prepared,notes,notes);limits.expressions=notes;
    Runtime::new(prepared,limits).unwrap()
}
fn input(id:i32)->Input {Input{protocol:Protocol::Native,port:0,group:0,channel:1,key:60,external_id:Some(id)}}

#[test]
fn manual_expression_child_keeps_exact_source_and_reclaims_its_expression_generation() {
    let mut rt=runtime(8);
    let parent=rt.note_on(input(1),60,1.).unwrap();
    let source=rt.expression_id(parent).unwrap();
    let child=rt.child(parent,60,1.,false,Inheritance::Expression).unwrap();
    let child_expression=rt.expression_id(child).unwrap();
    assert_ne!(source,child_expression);
    rt.start(child,0,rt.now(),1.).unwrap();
    rt.note_off_exact(parent,None).unwrap();
    rt.flush_ended(|_|true);
    assert!(rt.note(parent).is_ok(),"manual child retains real parent provenance under current Note tree contract");
    let reused=rt.note_on(input(2),60,1.).unwrap();
    let independent=rt.expression_id(reused).unwrap();
    support::without_heap(|| {
        rt.set_expression(source,Expression{pitch_semitones:7.,pressure:0x8765_4321,timbre:0x1234_5678,..Expression::default()}).unwrap();
        assert_eq!(rt.expression(child_expression).unwrap().pitch_semitones,7.);
        assert_eq!(rt.expression(child_expression).unwrap().pressure,0x8765_4321);
        assert_eq!(rt.expression(child_expression).unwrap().timbre,0x1234_5678);
        assert_eq!(rt.expression(independent).unwrap(),Expression::default());
        let mut frames=[[0.;2];64];rt.render(&mut frames).unwrap();
        assert!(frames.iter().all(|frame|frame.iter().all(|sample|sample.is_finite())));
        assert!(frames.iter().any(|frame|frame[0]!=0.));
        rt.release(child).unwrap();rt.cancel_input_note(parent).unwrap();rt.cancel_input_note(reused).unwrap();
        rt.render(&mut [[0.;2];64]).unwrap();rt.flush_ended(|_|true);
    });
    assert_eq!((rt.note_count(),rt.expression_count()),(0,0));
    assert_eq!(rt.expression(source),Err(Error::StaleHandle));
    assert_eq!(rt.expression(child_expression),Err(Error::StaleHandle));
    let fresh=rt.note_on(input(1),60,1.).unwrap();
    assert_ne!(rt.expression_id(fresh).unwrap(),source,"reused arena slot must carry a fresh actual generation");
    assert_eq!(rt.expression(rt.expression_id(fresh).unwrap()),Ok(Expression::default()));
}

#[test]
fn deep_manual_child_retirement_is_bounded_and_performs_no_heap_work() {
    let mut rt=runtime(1024);
    let root=rt.note_on(input(7),60,1.).unwrap();let source=rt.expression_id(root).unwrap();
    let mut last=root;
    support::without_heap(|| {
        for _ in 0..1000 {last=rt.child(last,60,1.,false,Inheritance::Expression).unwrap();}
        assert_eq!(rt.expression_count(),1001);
        rt.set_expression(source,Expression{pressure:0xa123_4567,..Expression::default()}).unwrap();
        assert_eq!(rt.expression(rt.expression_id(last).unwrap()).unwrap().pressure,0xa123_4567);
        rt.panic();rt.flush_ended(|_|true);
    });
    assert_eq!((rt.note_count(),rt.expression_count()),(0,0));
    assert_eq!(rt.expression(source),Err(Error::StaleHandle));
}
fn external_plan(note:bool)->Prepared {
    use sampler_ir as ir;
    use sampler_core::{Instruction as I,Program,WaitLifetime};
    let processor=ir::SourceEventProcessorId::UviNode(11);
    let owner=ir::SourceOwnerId::UviNode(10);
    let mut instrument=ir::Instrument{
        source:ir::SourceFormat::Uvi,
        assets:vec![ir::Asset{location:ir::AssetLocation::Path("authored.wav".into()),encoding:ir::Encoding::Wav,
            residency:Default::default(),root_key:None,loops:vec![]}],
        zones:vec![ir::Zone{keys:ir::KeyRange{low:60,high:60},pitch:ir::KeyTracking::Fixed,
            velocity:ir::VelocityResponse::None,..ir::Zone::new(ir::AssetRef(0))}],
        ..ir::Instrument::default()
    };
    instrument.source_indices.owners.push(ir::SourceOwner{id:owner,parent:None,kind:ir::SourceOwnerKind::Instrument,
        children:vec![],groups:vec![],zones:vec![ir::ZoneRef(0)],predicates:vec![],bypass:None,mute:None,
        event_processors:vec![ir::SourceEventProcessor{id:processor,owner,ordinal:0,kind:"ScriptProcessor".into(),
            bypass:ir::SourceSwitch{saved:false,control:None}},
            ir::SourceEventProcessor{id:ir::SourceEventProcessorId::UviNode(12),owner,ordinal:1,kind:"ScriptProcessor".into(),
                bypass:ir::SourceSwitch{saved:false,control:None}}]});
    let plan=sampler_core::lower::lower(&instrument,48000,
        vec![Pcm::new(48000,Box::from([[0.25;2];4096])).unwrap()],|_,_|unreachable!("no source behaviors"))
        .unwrap();
    assert_eq!(plan.source_owners().rows().len(),1);
    let index=plan.source_owner_index(owner).unwrap();
    assert_eq!(plan.source_owners().get(index).unwrap().regions.as_ref(),[0]);
    assert_eq!(plan.source_owners().processor(processor).unwrap().owner,index);
    let program=Program::new(vec![I::AwaitExternal{processor},I::End]).unwrap().with_wait_lifetime(WaitLifetime::Callback);
    let plan=plan.with_programs(vec![program],note.then_some(0)).unwrap();
    if note{plan}else{plan.with_controller_program(0).unwrap()}
}
fn external_runtime(note:bool)->Runtime {
    let plan=external_plan(note);let limits=Limits::for_plan(&plan,16,16);
    Runtime::new(plan,limits).unwrap()
}

#[test]
fn external_controller_turn_consumes_or_transparently_posts_full_precision_once() {
    use sampler_core::{ChannelAddress,ExternalEvent,ExternalOutput,ExternalOutputResult};
    let mut rt=external_runtime(false);let perf=rt.performance(0).unwrap();
    let address=ChannelAddress{protocol:Protocol::Midi2,port:7,group:3,channel:13};
    let value=0x8192_a5b7;
    let callback=rt.dispatch_controller(perf,address,1<<13,1,value).unwrap().unwrap();
    let turn=rt.pending_external_turn().unwrap().unwrap();
    assert_eq!(turn.input.behavior(),callback);assert_eq!(turn.original_frame,0);assert_eq!(turn.resume_frame,0);
    assert!(matches!(turn.event,ExternalEvent::Controller{source,number:1,value:v,poly_key:None}if source==address&&v==value));
    assert_eq!(rt.channel_controller(perf,address,1).unwrap(),0);
    rt.begin_external_turn(turn.input).unwrap();
    assert_eq!(rt.post_external_output(turn.input,ExternalOutput::Original).unwrap(),ExternalOutputResult::Forwarded);
    assert_eq!(rt.channel_controller(perf,address,1).unwrap(),value);
    rt.complete_external_segment(turn.input,None).unwrap();
    assert!(rt.pending_external_turn().unwrap().is_none());
    assert!(matches!(rt.begin_external_turn(turn.input),Err(Error::StaleHandle)));
    rt.dispatch_controller(perf,address,1<<13,1,0x1234_5678).unwrap();
    let consumed=rt.pending_external_turn().unwrap().unwrap();rt.begin_external_turn(consumed.input).unwrap();
    rt.complete_external_segment(consumed.input,None).unwrap();
    assert_eq!(rt.channel_controller(perf,address,1).unwrap(),value,"defined external callback does not implicitly forward");
    assert_eq!(rt.channel_input_controller(perf,address,1).unwrap(),0x1234_5678);
}

#[test]
fn external_segment_wait_retains_real_creator_after_physical_keyup_and_posts_manual_voice() {
    use sampler_core::{ExternalEvent,ExternalOutput,ExternalOutputResult};
    let mut rt=external_runtime(true);
    let original=input(91);let parent=rt.trigger(original,60,1.).unwrap();
    let first=rt.pending_external_turn().unwrap().unwrap();rt.begin_external_turn(first.input).unwrap();
    rt.complete_external_segment(first.input,Some(32)).unwrap();
    rt.note_off_exact(parent,Some(0.)).unwrap();rt.flush_ended(|_|true);
    assert!(rt.note(parent).is_ok(),"actual continuation owns note work through physical key-up");
    let mut silence=[[7.;2];32];rt.render(&mut silence).unwrap();
    assert!(silence.iter().all(|frame|*frame==[0.;2]));assert_eq!(rt.now(),32);
    let next=rt.service_external_boundary().unwrap().unwrap();
    assert_ne!(next.input,first.input);assert_eq!((next.original_frame,next.resume_frame),(0,32));
    assert!(matches!(next.event,ExternalEvent::Note{note,source,..}if note==parent&&source==original));
    assert!(matches!(rt.begin_external_turn(first.input),Err(Error::StaleHandle)));
    rt.begin_external_turn(next.input).unwrap();
    let child=match rt.post_external_output(next.input,ExternalOutput::Original).unwrap(){ExternalOutputResult::Note(child)=>child,_=>panic!("expected actual note")};
    assert_ne!(child,parent);rt.complete_external_segment(next.input,None).unwrap();
    assert!(!rt.note(parent).unwrap().2);assert!(rt.note(child).unwrap().2);
    let mut output=[[0.;2];128];rt.render(&mut output).unwrap();
    let reference_plan=external_plan(true).with_programs(vec![],None).unwrap();
    let reference_limits=Limits::for_plan(&reference_plan,16,16);
    let mut reference=Runtime::new(reference_plan,reference_limits).unwrap();
    reference.trigger(input(92),60,1.).unwrap();
    let mut reference_output=[[0.;2];128];reference.render(&mut reference_output).unwrap();
    assert!(output.iter().any(|frame|frame[0]!=0.));
    assert_eq!(output,reference_output,"late manual source must render the independently admitted ordinary source, not only be nonzero");
    rt.key_up(child,None).unwrap();rt.render(&mut [[0.;2];128]).unwrap();rt.flush_behaviors(|_,_,_|true);rt.flush_ended(|_|true);
}

#[test]
fn external_segment_wrong_sequence_and_rejected_future_ack_leave_the_real_turn_unchanged() {
    let mut rt=external_runtime(false);let perf=rt.performance(0).unwrap();
    let address=sampler_core::ChannelAddress{protocol:Protocol::Midi1,port:1,group:0,channel:2};
    rt.dispatch_controller(perf,address,1<<2,11,0x9876_5432).unwrap();
    let turn=rt.pending_external_turn().unwrap().unwrap();rt.begin_external_turn(turn.input).unwrap();
    let before=(rt.now(),rt.channel_controller(perf,address,11).unwrap());
    assert_eq!(rt.complete_external_segment(turn.input,Some(rt.now())),Err(Error::PastEvent));
    assert_eq!((rt.now(),rt.channel_controller(perf,address,11).unwrap()),before);
    assert_eq!(rt.pending_external_turn().unwrap().unwrap().input,turn.input);
    rt.complete_external_segment(turn.input,None).unwrap();
    rt.flush_behaviors(|_,_,_|true);
    rt.dispatch_controller(perf,address,1<<2,11,0x1234_5678).unwrap();
    let fresh=rt.pending_external_turn().unwrap().unwrap();assert_ne!(fresh.input,turn.input);
    assert!(matches!(rt.begin_external_turn(turn.input),Err(Error::StaleHandle)));
    rt.begin_external_turn(fresh.input).unwrap();rt.complete_external_segment(fresh.input,None).unwrap();
}

#[test]
fn external_wait_capacity_refusal_preserves_current_turn_and_callback_has_no_heap_work() {
    use sampler_core::{ChannelAddress,Event,ExternalOutput};
    let mut rt=external_runtime(false);let perf=rt.performance(0).unwrap();
    let address=ChannelAddress{protocol:Protocol::Midi2,port:1,group:2,channel:3};
    rt.dispatch_controller(perf,address,1<<3,1,0xa123_4567).unwrap();
    let turn=rt.pending_external_turn().unwrap().unwrap();
    // Occupy the actual bounded scheduler through its ordinary public API.
    let mut queued=0usize;
    loop {
        match rt.schedule_event(100,Event::Articulation(perf,0)) {
            Ok(())=>{queued+=1;assert!(queued<1_000_000);}
            Err(Error::Capacity)=>break,
            Err(error)=>panic!("unexpected scheduler setup refusal: {error:?}"),
        }
    }
    support::without_heap(||{
        rt.begin_external_turn(turn.input).unwrap();
        assert_eq!(rt.complete_external_segment(turn.input,Some(1)),Err(Error::Capacity));
        assert_eq!(rt.now(),0);assert_eq!(rt.pending_external_turn().unwrap().unwrap().input,turn.input);
        assert_eq!(rt.channel_controller(perf,address,1).unwrap(),0);
        rt.post_external_output(turn.input,ExternalOutput::Original).unwrap();
        assert_eq!(rt.channel_controller(perf,address,1).unwrap(),0xa123_4567);
        rt.complete_external_segment(turn.input,None).unwrap();
    });
}

#[test]
fn external_capture_keeps_original_frame_after_native_wait_and_blocks_later_equal_frame_work() {
    use sampler_core::{ChannelAddress,Event,Instruction as I,Program,WaitLifetime};
    let wait=Program::new(vec![I::Wait(4),I::ForwardController,I::End]).unwrap().with_wait_lifetime(WaitLifetime::Callback);
    let external=Program::new(vec![I::AwaitExternal{processor:sampler_ir::SourceEventProcessorId::UviNode(11)},I::End]).unwrap().with_wait_lifetime(WaitLifetime::Callback);
    let plan=external_plan(false).with_programs(vec![wait,external],None).unwrap().with_controller_programs(vec![0,1]).unwrap();
    let limits=Limits::for_plan(&plan,16,16);let mut rt=Runtime::new(plan,limits).unwrap();
    let perf=rt.performance(0).unwrap();let address=ChannelAddress{protocol:Protocol::Midi2,port:3,group:1,channel:6};
    rt.dispatch_controller(perf,address,1<<6,1,0x8765_4321).unwrap();
    rt.schedule_event(4,Event::Controller(perf,11,0x1234_5678)).unwrap();
    rt.render(&mut [[0.;2];4]).unwrap();assert_eq!(rt.now(),4);
    let turn=rt.service_external_boundary().unwrap().unwrap();
    assert_eq!((turn.original_frame,turn.resume_frame),(0,4));
    assert_eq!(rt.next_scheduled_frame(),Some(4),"later equal-time operation must remain in its real queue");
    assert_eq!(rt.controller(perf,11).unwrap(),u32::MAX);
    rt.begin_external_turn(turn.input).unwrap();
    assert_eq!(rt.service_external_boundary().unwrap().unwrap().input,turn.input);
    assert_eq!(rt.controller(perf,11).unwrap(),u32::MAX);
    rt.complete_external_segment(turn.input,None).unwrap();
    assert!(rt.service_external_boundary().unwrap().is_none());
    assert_eq!(rt.controller(perf,11).unwrap(),0x1234_5678);
    assert_eq!(rt.channel_controller(perf,address,1).unwrap(),0,"unposted original remains consumed");
}

#[test]
fn external_intentional_repeated_original_posts_have_distinct_real_next_stage_owners() {
    use sampler_core::{ChannelAddress,ExternalEvent,ExternalOutput,Instruction as I,Program,WaitLifetime};
    let programs=[11,12].map(|id|Program::new(vec![I::AwaitExternal{processor:sampler_ir::SourceEventProcessorId::UviNode(id)},I::End]).unwrap().with_wait_lifetime(WaitLifetime::Callback)).to_vec();
    let plan=external_plan(false).with_programs(programs,None).unwrap().with_controller_programs(vec![0,1]).unwrap();
    let limits=Limits::for_plan(&plan,16,16);let mut rt=Runtime::new(plan,limits).unwrap();
    let perf=rt.performance(0).unwrap();let address=ChannelAddress{protocol:Protocol::Midi2,port:4,group:2,channel:7};
    let value=0x9234_b5c6;rt.dispatch_controller(perf,address,1<<7,1,value).unwrap();
    let first=rt.pending_external_turn().unwrap().unwrap();rt.begin_external_turn(first.input).unwrap();
    rt.post_external_output(first.input,ExternalOutput::Original).unwrap();
    rt.post_external_output(first.input,ExternalOutput::Original).unwrap();
    assert_eq!(rt.channel_controller(perf,address,1).unwrap(),0,"neither downstream callback has forwarded");
    rt.complete_external_segment(first.input,None).unwrap();
    let a=rt.pending_external_turn().unwrap().unwrap();rt.begin_external_turn(a.input).unwrap();
    assert_eq!(a.processor,sampler_ir::SourceEventProcessorId::UviNode(12));
    assert!(matches!(a.event,ExternalEvent::Controller{source,value:v,..}if source==address&&v==value));
    rt.complete_external_segment(a.input,None).unwrap();
    let b=rt.pending_external_turn().unwrap().unwrap();assert_ne!(a.input,b.input);
    assert_ne!(a.input.behavior(),b.input.behavior(),"each intentional post owns actual bounded continuation capacity");
    rt.begin_external_turn(b.input).unwrap();rt.post_external_output(b.input,ExternalOutput::Original).unwrap();
    rt.complete_external_segment(b.input,None).unwrap();
    assert!(rt.pending_external_turn().unwrap().is_none());assert_eq!(rt.channel_controller(perf,address,1).unwrap(),value);
}
#[test]
fn pending_external_turn_refuses_pcm_before_output_clock_or_cursor_advance() {
    use sampler_core::{ExternalOutput,ExternalOutputResult};
    let mut rt=external_runtime(true);let parent=rt.trigger(input(201),60,1.).unwrap();
    let turn=rt.pending_external_turn().unwrap().unwrap();
    let before=(rt.now(),rt.note(parent).unwrap(),rt.expression(rt.expression_id(parent).unwrap()).unwrap());
    let mut output=[[71.;2];16];
    support::without_heap(||{
        assert_eq!(rt.external_render_frames(16),Err(Error::Busy));
        assert_eq!(rt.render(&mut output),Err(Error::Busy));
        assert_eq!(output,[[71.;2];16]);
        assert_eq!((rt.now(),rt.note(parent).unwrap(),rt.expression(rt.expression_id(parent).unwrap()).unwrap()),before);
        rt.begin_external_turn(turn.input).unwrap();
        let child=match rt.post_external_output(turn.input,ExternalOutput::Original).unwrap(){ExternalOutputResult::Note(note)=>note,_=>panic!("actual note expected")};
        assert_eq!(rt.render(&mut output),Err(Error::Busy));assert_eq!(output,[[71.;2];16]);
        assert_eq!(rt.now(),0);assert_eq!(rt.note(child).unwrap().0,60);
        rt.complete_external_segment(turn.input,None).unwrap();
        rt.render(&mut output).unwrap();assert_eq!(rt.now(),16);
    });
    let reference_plan=external_plan(true).with_programs(vec![],None).unwrap();
    let limits=Limits::for_plan(&reference_plan,16,16);let mut reference=Runtime::new(reference_plan,limits).unwrap();
    reference.trigger(input(202),60,1.).unwrap();let mut expected=[[0.;2];16];reference.render(&mut expected).unwrap();
    assert_eq!(output,expected,"Busy attempts must not consume one source/envelope sample");
}

#[test]
fn external_render_prefix_stops_before_real_due_frame_and_zero_frame_phase_discovers_turn() {
    use sampler_core::{ChannelAddress,Instruction as I,Program,WaitLifetime};
    let program=Program::new(vec![I::Wait(1),I::AwaitExternal{processor:sampler_ir::SourceEventProcessorId::UviNode(11)},I::End]).unwrap().with_wait_lifetime(WaitLifetime::Callback);
    let plan=external_plan(false).with_programs(vec![program],None).unwrap().with_controller_program(0).unwrap();
    let limits=Limits::for_plan(&plan,16,16);let mut rt=Runtime::new(plan,limits).unwrap();
    let perf=rt.performance(0).unwrap();let address=ChannelAddress{protocol:Protocol::Midi2,port:2,group:4,channel:9};
    rt.dispatch_controller(perf,address,1<<9,1,0x8765_4321).unwrap();
    assert!(rt.pending_external_turn().unwrap().is_none());assert_eq!(rt.external_render_frames(8).unwrap(),1);
    let mut full=[[37.;2];8];assert_eq!(rt.render(&mut full),Err(Error::Busy));
    assert_eq!(rt.now(),0);assert_eq!(full,[[37.;2];8]);
    let mut prefix=[[0.;2];1];rt.render(&mut prefix).unwrap();assert_eq!(rt.now(),1);
    assert_eq!(rt.render(&mut []),Err(Error::Busy),"zero-frame startup/due phase must expose Await without advancing PCM");
    let turn=rt.pending_external_turn().unwrap().unwrap();assert_eq!((turn.original_frame,turn.resume_frame),(0,1));
    rt.begin_external_turn(turn.input).unwrap();rt.complete_external_segment(turn.input,None).unwrap();
    assert_eq!(rt.external_render_frames(8).unwrap(),8);rt.render(&mut full).unwrap();assert_eq!(rt.now(),9);
    assert_eq!(rt.channel_controller(perf,address,1).unwrap(),0);
}
fn external_physical_receiver(rt:&mut Runtime,notes:usize)->sampler_core::ReceiverId {
    let mut receiver=sampler_core::OwnedReceiver::new(rt,0,0,sampler_core::ReceiverZone::Lower,15,notes).unwrap();
    receiver.enable_host_channel_projection().unwrap();rt.bind_receiver(receiver).unwrap()
}
fn physical_native(id:i32)->(Input,Input){
    (Input{protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:Some(id)},
     Input{protocol:Protocol::Clap,port:7,group:3,channel:13,key:60,external_id:Some(id)})
}
fn physical_raw()->Input{Input{protocol:Protocol::Midi1,port:0,group:0,channel:1,key:60,external_id:None}}
fn raw_off(rt:&mut Runtime,receiver:sampler_core::ReceiverId,source:Input){
    use sampler_core::{ReceiverAttribute,ReceiverChannelVoice,ReceiverMessage,ReceiverValue,ReceiverVersion};
    rt.apply_receiver(receiver,Some(ReceiverChannelVoice{version:ReceiverVersion::Midi1,group:0,channel:source.channel,
        message:ReceiverMessage::NoteOff{key:60,velocity:Some(ReceiverValue::Bits7(0)),attribute:ReceiverAttribute{kind:0,data:0}}}),true,Some(source)).unwrap();
}

#[test]
fn physical_event_retains_the_real_old_plan_after_note_end_and_creates_a_plan_owned_release_turn(){
    use sampler_core::{ExternalEvent,ExternalOutput,ExternalOutputResult,NotePitch};
    let old=external_plan(false);let limits=Limits::for_plan(&old,16,16);
    let (rt,mut control)=Runtime::with_plan_updates(old,limits,2,1).unwrap();
    let mut rt=rt.with_receiver_capacity(1).unwrap();let receiver=external_physical_receiver(&mut rt,4);
    let old_plan=rt.active_plan();let (execution,source)=physical_native(701);
    let note=rt.trigger_receiver(receiver,0,execution,Some(source),1.,0.,true).unwrap();
    assert_eq!(rt.plan_physical_event_count(old_plan),Ok(1));
    rt.cancel_input_note(note).unwrap();let mut ended=0;rt.flush_ended(|input|{assert_eq!(input,execution);ended+=1;true});
    assert_eq!(ended,1,"physical debt must NOT delay actual note-end");assert!(matches!(rt.note(note),Err(Error::StaleHandle)));
    let replacement=external_plan(false).with_programs(vec![],None).unwrap();
    let request=control.submit(Box::new(replacement)).unwrap();assert_eq!(rt.poll_plan_update(),Ok(Some(request)));
    assert_ne!(rt.active_plan(),old_plan);assert_eq!(rt.collect_retired_plans(),0);
    assert_eq!(rt.release_receiver_native(receiver,|id,input|id==note&&input==source,Some(0.25)),Ok(1));
    assert!(rt.receiver_has_native_note(receiver,note),"the exact pending receipt still owns its callback suppression");
    let delivery=rt.next_receiver_terminal(receiver).unwrap().unwrap();
    assert_eq!((delivery.release.note,delivery.release.source,delivery.release.velocity),(note,source,Some(0.25)));
    assert_eq!(rt.receiver_terminal_plan(delivery),Ok(Some(old_plan)));
    assert_eq!(rt.plan_physical_event_count(old_plan),Ok(1),"row reference transfers into receipt, never copied on actual Off");
    let behavior=rt.start_receiver_release_behavior(delivery,0,0).unwrap();
    assert_eq!(rt.start_receiver_release_behavior(delivery,0,0),Err(Error::Busy));
    let turn=rt.pending_external_turn().unwrap().unwrap();assert_eq!(turn.input.behavior(),behavior);assert_eq!(turn.plan,old_plan);
    match turn.event {ExternalEvent::PhysicalRelease(event)=>{assert_eq!(event.plan,old_plan);assert_eq!(event.source,source);assert_eq!(event.execution,execution);assert_eq!(event.velocity,Some(0.25));},_=>panic!("release is a new physical Event creator, not a stale Note creator")}
    rt.complete_receiver_terminal(delivery).unwrap();assert_eq!(rt.plan_physical_event_count(old_plan),Ok(0));
    assert_eq!(rt.collect_retired_plans(),0,"actual generation callback owns the plan after receipt ACK");
    rt.begin_external_turn(turn.input).unwrap();
    let child=match rt.post_external_output(turn.input,ExternalOutput::Note{pitch:NotePitch::Key(60),velocity:1.}).unwrap(){ExternalOutputResult::Note(note)=>note,_=>panic!()};
    assert_eq!(rt.note_plan(child),Ok(old_plan));rt.complete_external_segment(turn.input,None).unwrap();
    let mut actual=[[0.;2];128];rt.render(&mut actual).unwrap();
    let reference_plan=external_plan(false).with_programs(vec![],None).unwrap();let reference_limits=Limits::for_plan(&reference_plan,16,16);
    let mut reference=Runtime::new(reference_plan,reference_limits).unwrap();reference.trigger(input(702),60,1.).unwrap();
    let mut expected=[[0.;2];128];reference.render(&mut expected).unwrap();assert_eq!(actual,expected,"plan-owned late play must equal ordinary real source PCM");
    rt.release(child).unwrap();rt.render(&mut [[0.;2];128]).unwrap();rt.flush_behaviors(|_,_,_|true);rt.flush_ended(|_|true);
    assert_eq!(rt.collect_retired_plans(),1);assert!(matches!(rt.plan_physical_event_count(old_plan),Err(Error::StaleHandle)));
    assert_eq!(rt.complete_receiver_terminal(delivery),Err(Error::StaleHandle));
    assert!(matches!(rt.start_receiver_release_behavior(delivery,0,0),Err(Error::StaleHandle)));
}

#[test]
fn pending_real_physical_event_blocks_pcm_and_explicit_dead_owner_cancel_releases_only_its_plan_ref(){
    let plan=external_plan(false);let limits=Limits::for_plan(&plan,4,4);let mut rt=Runtime::new(plan,limits).unwrap().with_receiver_capacity(1).unwrap();
    let receiver=external_physical_receiver(&mut rt,2);let plan=rt.active_plan();let (execution,source)=physical_native(801);
    let note=rt.trigger_receiver(receiver,0,execution,Some(source),1.,0.,true).unwrap();
    rt.cancel_input_note(note).unwrap();rt.flush_ended(|_|true);rt.release_receiver_native(receiver,|_,input|input==source,Some(0.)).unwrap();
    let mut sentinel=[[19.;2];8];let now=rt.now();
    support::without_heap(||{
        assert_eq!(rt.render(&mut sentinel),Err(Error::Busy));assert_eq!(sentinel,[[19.;2];8]);assert_eq!(rt.now(),now);
        let delivery=rt.next_receiver_terminal(receiver).unwrap().unwrap();
        assert_eq!(rt.start_receiver_release_behavior(delivery,usize::MAX,0),Err(Error::InvalidInput));
        assert_eq!(rt.plan_physical_event_count(plan),Ok(1));
        rt.cancel_receiver_physical_event(delivery).unwrap();assert_eq!(rt.plan_physical_event_count(plan),Ok(0));
        assert_eq!(rt.cancel_receiver_physical_event(delivery),Err(Error::StaleHandle));
        rt.render(&mut sentinel).unwrap();assert_eq!(sentinel,[[0.;2];8]);
    });
}

#[test]
fn physical_event_terminal_union_capacity_refuses_before_any_new_row_or_core_gate_changes(){
    let plan=external_plan(false);let limits=Limits::for_plan(&plan,8,8);let mut rt=Runtime::new(plan,limits).unwrap().with_receiver_capacity(1).unwrap();
    let receiver=external_physical_receiver(&mut rt,1);let plan=rt.active_plan();let raw=physical_raw();
    let (execution,source)=physical_native(901);
    let first=rt.trigger_receiver(receiver,0,execution,Some(source),1.,0.,true).unwrap();
    let raw_first=rt.trigger_receiver(receiver,1,raw,None,1.,0.,true).unwrap();
    assert_eq!(rt.queue_receiver_all_releases(receiver),Ok(2));
    rt.release_receiver_native(receiver,|_,input|input==source,None).unwrap();raw_off(&mut rt,receiver,raw);
    assert_eq!(rt.plan_physical_event_count(plan),Ok(2),"two queued receipts remain after actual row acknowledgment");
    let (execution2,source2)=physical_native(902);
    let second=rt.trigger_receiver(receiver,0,execution2,Some(source2),1.,0.,true).unwrap();
    let raw_second=rt.trigger_receiver(receiver,1,raw,None,1.,0.,true).unwrap();
    let before=(rt.now(),rt.note_count(),rt.plan_physical_event_count(plan));
    support::without_heap(||{
        assert_eq!(rt.queue_receiver_all_releases(receiver),Err(Error::Capacity));
        assert_eq!((rt.now(),rt.note_count(),rt.plan_physical_event_count(plan)),before);
        assert!(rt.note(second).unwrap().2&&rt.note(raw_second).unwrap().2);
        let a=rt.next_receiver_terminal(receiver).unwrap().unwrap();assert_eq!(a.release.note,first);rt.cancel_receiver_physical_event(a).unwrap();
        let b=rt.next_receiver_terminal(receiver).unwrap().unwrap();assert_eq!(b.release.note,raw_first);rt.cancel_receiver_physical_event(b).unwrap();
        assert_eq!(rt.queue_receiver_all_releases(receiver),Ok(2));
        assert_eq!(rt.plan_physical_event_count(plan),Ok(4),"synthetic copy owns a separate checked plan ref beside each physical row");
        rt.release_receiver_native(receiver,|_,input|input==source2,None).unwrap();raw_off(&mut rt,receiver,raw);
        assert_eq!(rt.plan_physical_event_count(plan),Ok(2));
        for _ in 0..2{let delivery=rt.next_receiver_terminal(receiver).unwrap().unwrap();rt.cancel_receiver_physical_event(delivery).unwrap();}
        assert_eq!(rt.plan_physical_event_count(plan),Ok(0));assert!(rt.next_receiver_terminal(receiver).unwrap().is_none());
    });
}
#[test]
fn fatal_external_execution_never_advances_or_replays_and_real_terminal_plan_debt_still_drains(){
    use sampler_core::{Event,ExternalOutput,ExternalOutputResult,NotePitch};
    let plan=external_plan(false);let limits=Limits::for_plan(&plan,8,8);
    let mut rt=Runtime::new(plan,limits).unwrap().with_receiver_capacity(1).unwrap();let receiver=external_physical_receiver(&mut rt,4);
    let plan=rt.active_plan();let (execution,source)=physical_native(1001);
    let parent=rt.trigger_receiver(receiver,0,execution,Some(source),1.,0.,true).unwrap();
    rt.start_behavior(parent,0).unwrap();let attack=rt.pending_external_turn().unwrap().unwrap();rt.begin_external_turn(attack.input).unwrap();
    let child=match rt.post_external_output(attack.input,ExternalOutput::Note{pitch:NotePitch::Key(60),velocity:1.}).unwrap(){ExternalOutputResult::Note(note)=>note,_=>panic!()};
    rt.complete_external_segment(attack.input,Some(32)).unwrap();assert!(rt.voice_count()>0);
    rt.release_receiver_native(receiver,|_,input|input==source,Some(1.)).unwrap();
    let delivery=rt.next_receiver_terminal(receiver).unwrap().unwrap();rt.start_receiver_release_behavior(delivery,0,0).unwrap();
    let release=rt.pending_external_turn().unwrap().unwrap();rt.begin_external_turn(release.input).unwrap();
    let recovered=rt.active_receiver_terminal(receiver).unwrap().unwrap();
    assert_eq!(recovered.release.note,delivery.release.note);assert_eq!(recovered.release.source,delivery.release.source);
    let
perf=rt.performance(0).unwrap();let before=rt.now();
    support::without_heap(||{
        let report=rt.abort_external_execution();assert_eq!((report.callbacks_cancelled,report.external_turns_cancelled),(2,2));
        assert!(rt.external_execution_aborted());assert_eq!(rt.now(),before);assert_eq!(rt.voice_count(),0);
        assert_eq!(rt.abort_external_execution().callbacks_cancelled,0);
        assert_eq!(rt.external_render_frames(128),Err(Error::Canceled));
        let mut sentinel=[[23.;2];128];assert_eq!(rt.render(&mut sentinel),Err(Error::Canceled));assert_eq!(sentinel,[[23.;2];128]);assert_eq!(rt.now(),before);
        assert!(matches!(rt.pending_external_turn(),Err(Error::Canceled)));
        assert!(matches!(rt.post_external_output(attack.input,ExternalOutput::Original),Err(Error::Canceled)));
        assert_eq!(rt.complete_external_segment(release.input,None),Err(Error::Canceled));
        assert_eq!(rt.schedule_event(64,Event::Articulation(perf,1)),Err(Error::Canceled));
        let (new_execution,new_source)=physical_native(1002);
        assert_eq!(rt.trigger_receiver(receiver,0,new_execution,Some(new_source),1.,0.,true),Err(Error::Canceled));
        assert_eq!(rt.start(child,0,before,1.),Err(Error::Canceled));
        // A real already-started physical receipt is still ACKable after its
        // actual callback is cancelled. No fake key-up or note ID is fabricated.
        rt.cancel_receiver_physical_event(recovered).unwrap();assert_eq!(rt.plan_physical_event_count(plan),Ok(0));
        assert!(rt.active_receiver_terminal(receiver).unwrap().is_none());
        assert_eq!(rt.cancel_receiver_physical_event(delivery),Err(Error::StaleHandle));
        rt.flush_behaviors(|_,_,_|true);rt.flush_ended(|_|true);
        assert_eq!(rt.note_count(),0);assert_eq!(rt.receiver(receiver).unwrap().native_physical_slots(),0);
        assert!(rt.next_receiver_terminal(receiver).unwrap().is_none());
        assert_eq!(rt.now(),before);
    });
}

fn source_frontier_plan(layer_key:u8,external:bool)->Prepared{
    use sampler_ir as ir;
    let ids=[10,20,30,40].map(ir::SourceOwnerId::UviNode);let processor=ir::SourceEventProcessorId::UviNode(211);
    let make_owner=|at:usize,parent:Option<usize>,kind,children:Vec<usize>,zones:Vec<usize>|ir::SourceOwner{
        id:ids[at],parent:parent.map(|at|ids[at]),kind,children:children.into_iter().map(|at|ids[at]).collect(),
        groups:zones.iter().map(|&zone|ir::GroupRef(zone)).collect(),zones:zones.into_iter().map(ir::ZoneRef).collect(),
        predicates:if at==1{vec![ir::SourceInputPredicate::Keys(ir::KeyRange{low:layer_key,high:layer_key}),ir::SourceInputPredicate::Channel(1)]}else{vec![]},
        bypass:None,mute:None,event_processors:if at==1{vec![ir::SourceEventProcessor{id:processor,owner:ids[at],ordinal:0,kind:"ScriptProcessor".into(),bypass:ir::SourceSwitch{saved:false,control:None}}]}else{vec![]}};
    let mut instrument=ir::Instrument{source:ir::SourceFormat::Uvi,
        groups:vec![ir::Group::default(),ir::Group::default()],
        assets:(0..2).map(|index|ir::Asset{location:ir::AssetLocation::Path(format!("authored{index}.wav").into()),encoding:ir::Encoding::Wav,residency:Default::default(),root_key:None,loops:vec![]}).collect(),
        zones:(0..2).map(|index|ir::Zone{keys:ir::KeyRange{low:60,high:60},group:Some(ir::GroupRef(index)),pitch:ir::KeyTracking::Fixed,velocity:ir::VelocityResponse::None,..ir::Zone::new(ir::AssetRef(index))}).collect(),
        ..ir::Instrument::default()};
    instrument.source_indices.owners=vec![
        make_owner(0,None,ir::SourceOwnerKind::Instrument,vec![1],vec![0,1]),
        make_owner(1,Some(0),ir::SourceOwnerKind::Layer,vec![2,3],vec![0,1]),
        make_owner(2,Some(1),ir::SourceOwnerKind::Keygroup,vec![],vec![0]),
        make_owner(3,Some(1),ir::SourceOwnerKind::Keygroup,vec![],vec![1])];
    let plan=sampler_core::lower::lower(&instrument,48000,vec![Pcm::new(48000,Box::from([[0.125;2];4096])).unwrap(),Pcm::new(48000,Box::from([[0.5;2];4096])).unwrap()],|_,_|unreachable!("no native behavior")).unwrap();
    let plan=plan.with_variation(vec![sampler_core::Sequence{takes:2,policy:sampler_core::TakePolicy::Sequential,scope:sampler_core::SequenceScope::Global,capacity:1}],vec![Some(sampler_core::Take{sequence:0,index:0}),Some(sampler_core::Take{sequence:0,index:1})],1,2).unwrap();
    if external{plan.with_programs(vec![sampler_core::Program::new(vec![sampler_core::Instruction::AwaitExternal{processor},sampler_core::Instruction::End]).unwrap().with_wait_lifetime(sampler_core::WaitLifetime::Callback)],None).unwrap()}else{plan}
}

#[test]
fn actual_source_frontier_retains_the_chosen_kg_and_take_across_wait_and_audio_does_not_choose_twice(){
    use sampler_core::{ExternalEvent,Trigger};
    let plan=source_frontier_plan(60,true);let layer=plan.source_owner_index(sampler_ir::SourceOwnerId::UviNode(20)).unwrap();
    let a=plan.source_owner_index(sampler_ir::SourceOwnerId::UviNode(30)).unwrap();let b=plan.source_owner_index(sampler_ir::SourceOwnerId::UviNode(40)).unwrap();
    let mut limits=Limits::for_plan(&plan,8,8);limits.decisions=16;let mut rt=Runtime::new(plan,limits).unwrap();
    let first=rt.note_on(input(1101),60,1.).unwrap();
    support::without_heap(||{
        assert_eq!(rt.commit_source_frontier(first,layer,0,1),Ok(1));assert_eq!(rt.note_take(first,Trigger::Attack,0),Ok(Some(0)));
        assert_eq!(rt.source_child_selected(first,a),Ok(true));assert_eq!(rt.source_child_selected(first,b),Ok(false));
        let count=rt.decision_count();assert_eq!(rt.commit_source_frontier(first,layer,0,1),Ok(1));assert_eq!(rt.decision_count(),count);
    });
    rt.start_behavior(first,0).unwrap();let turn=rt.pending_external_turn().unwrap().unwrap();
    assert!(matches!(turn.event,ExternalEvent::Note{note,..}if note==first));rt.begin_external_turn(turn.input).unwrap();rt.complete_external_segment(turn.input,Some(32)).unwrap();
    let second=rt.trigger(input(1102),60,1.).unwrap();assert_eq!(rt.note_take(second,Trigger::Attack,0),Ok(Some(1)),"the real sequence advanced at frontier commit");
    rt.cancel_input_note(second).unwrap();rt.flush_ended(|_|true);
    let mut silence=[[1.;2];32];rt.render(&mut silence).unwrap();assert_eq!(silence,[[0.;2];32]);
    let resumed=rt.service_external_boundary().unwrap().unwrap();rt.begin_external_turn(resumed.input).unwrap();rt.complete_external_segment(resumed.input,None).unwrap();
    assert_eq!(rt.forward_attack(first),Ok(true));assert_eq!(rt.note_take(first,Trigger::Attack,0),Ok(Some(0)));
    let third=rt.note_on(input(1103),60,1.).unwrap();assert_eq!(rt.commit_source_frontier(third,layer,0,1),Ok(1));
    assert_eq!(rt.note_take(third,Trigger::Attack,0),Ok(Some(0)),"final audio must NOT advance the selected sequence a second time");
    assert_eq!(rt.source_child_selected(third,a),Ok(true));
    let reference_plan=source_frontier_plan(60,false);let reference_limits=Limits::for_plan(&reference_plan,8,8);
    let mut reference=Runtime::new(reference_plan,reference_limits).unwrap();let reference_note=reference.trigger(input(1199),60,1.).unwrap();assert_eq!(reference.note_take(reference_note,Trigger::Attack,0),Ok(Some(0)));
    let mut got=[[0.;2];128];let mut expected=[[0.;2];128];rt.render(&mut got).unwrap();reference.render(&mut expected).unwrap();
    assert!(expected.iter().any(|frame|frame[0]!=0.));assert_eq!(got,expected,"the frozen native take must render the independent ordinary selection PCM");
}

#[test]
fn source_frontier_refusal_is_transactional_and_empty_completion_is_distinct_from_no_choice(){
    use sampler_core::Trigger;
    let plan=source_frontier_plan(60,false);let layer=plan.source_owner_index(sampler_ir::SourceOwnerId::UviNode(20)).unwrap();let kg=plan.source_owner_index(sampler_ir::SourceOwnerId::UviNode(30)).unwrap();
    let mut limits=Limits::for_plan(&plan,4,4);limits.decisions=2;let mut rt=Runtime::new(plan,limits).unwrap();let pending=rt.note_on(input(1201),60,1.).unwrap();
    let before=(rt.now(),rt.note_count(),rt.decision_count());
    support::without_heap(||{
        assert_eq!(rt.commit_source_frontier(pending,layer,0,1),Err(Error::Capacity));assert_eq!((rt.now(),rt.note_count(),rt.decision_count()),before);
        assert_eq!(rt.source_selection_frontier(pending),Ok(None));assert_eq!(rt.source_child_selected(pending,kg),Err(Error::NotReady));
        let ordinary=rt.trigger(input(1202),60,1.).unwrap();assert_eq!(rt.note_take(ordinary,Trigger::Attack,0),Ok(Some(0)),"rejected frontier cannot advance the native sequence");
    });
    let plan=source_frontier_plan(61,false);let layer=plan.source_owner_index(sampler_ir::SourceOwnerId::UviNode(20)).unwrap();let kg=plan.source_owner_index(sampler_ir::SourceOwnerId::UviNode(30)).unwrap();
    let limits=Limits::for_plan(&plan,4,4);let mut empty=Runtime::new(plan,limits).unwrap();let note=empty.note_on(input(1203),60,1.).unwrap();
    assert_eq!(empty.commit_source_frontier(note,layer,0,1),Ok(0));assert_eq!(empty.source_selection_frontier(note),Ok(Some((layer,0))));assert_eq!(empty.source_child_selected(note,kg),Ok(false));
    assert_eq!(empty.forward_attack(note),Ok(true));assert_eq!(empty.voice_count(),0);assert_eq!(empty.note_take(note,Trigger::Attack,0),Ok(None));
}

fn hierarchy_note_plan(compiled:bool,both_layers:bool)->Prepared{
    use sampler_ir as ir;
    let ids=[100,200,210,300,310,400,410].map(ir::SourceOwnerId::UviNode);
    let processor=|id,owner,ordinal,saved|ir::SourceEventProcessor{id:ir::SourceEventProcessorId::UviNode(id),owner,ordinal,kind:"ScriptProcessor".into(),bypass:ir::SourceSwitch{saved,control:None}};
    let owner=|index:usize,parent:Option<usize>,kind,children:Vec<usize>,zones:Vec<usize>,key:Option<u8>,channel:Option<u8>,event_processors|ir::SourceOwner{
        id:ids[index],parent:parent.map(|index|ids[index]),kind,children:children.into_iter().map(|index|ids[index]).collect(),
        groups:zones.iter().copied().map(ir::GroupRef).collect(),zones:zones.into_iter().map(ir::ZoneRef).collect(),
        predicates:key.map(|key|ir::SourceInputPredicate::Keys(ir::KeyRange{low:key,high:key})).into_iter()
            .chain(channel.map(ir::SourceInputPredicate::Channel)).collect(),bypass:None,mute:None,event_processors};
    let mut instrument=ir::Instrument{source:ir::SourceFormat::Uvi,
        groups:vec![ir::Group::default();3],
        assets:(0..3).map(|index|ir::Asset{location:ir::AssetLocation::Path(format!("layer{index}.wav").into()),encoding:ir::Encoding::Wav,residency:Default::default(),root_key:None,loops:vec![]}).collect(),
        zones:[62,if both_layers{62}else{60},61].into_iter().enumerate().map(|(index,key)|ir::Zone{
            keys:ir::KeyRange{low:key,high:key},group:Some(ir::GroupRef(index)),pitch:ir::KeyTracking::Fixed,
            velocity:ir::VelocityResponse::None,..ir::Zone::new(ir::AssetRef(index))}).collect(),..ir::Instrument::default()};
    instrument.source_indices.owners=vec![
        owner(0,None,ir::SourceOwnerKind::Instrument,vec![1,3,5],vec![0,1,2],None,None,
            vec![processor(101,ids[0],0,false),processor(102,ids[0],1,true)]),
        owner(1,Some(0),ir::SourceOwnerKind::Layer,vec![2],vec![0],Some(61),Some(2),vec![processor(201,ids[1],0,false)]),
        owner(2,Some(1),ir::SourceOwnerKind::Keygroup,vec![],vec![0],None,None,vec![]),
        owner(3,Some(0),ir::SourceOwnerKind::Layer,vec![4],vec![1],Some(if both_layers{61}else{60}),Some(2),vec![processor(301,ids[3],0,false)]),
        owner(4,Some(3),ir::SourceOwnerKind::Keygroup,vec![],vec![1],None,None,vec![]),
        owner(5,Some(0),ir::SourceOwnerKind::Layer,vec![6],vec![2],Some(61),Some(3),vec![processor(401,ids[5],0,false)]),
        owner(6,Some(5),ir::SourceOwnerKind::Keygroup,vec![],vec![2],None,None,vec![])];
    let pcm=[0.125,0.5,0.25].into_iter().map(|level|Pcm::new(48000,Box::from([[level;2];4096])).unwrap()).collect();
    let prepared=sampler_core::lower::lower(&instrument,48000,pcm,|_,_|unreachable!("no native behavior")).unwrap();
    if compiled{prepared.with_source_note_hierarchy().unwrap()}else{prepared}
}
fn hierarchy_runtime(both:bool)->Runtime{
    let plan=hierarchy_note_plan(true,both);let limits=Limits::for_plan(&plan,32,16);Runtime::new(plan,limits).unwrap()
}
fn hierarchy_source(id:i32)->(Input,Input){
    (Input{protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:Some(id)},
     Input{protocol:Protocol::Clap,port:7,group:3,channel:13,key:60,external_id:Some(id)})
}
fn hierarchy_begin(rt:&mut Runtime,id:u32)->sampler_core::ExternalTurn{
    let turn=rt.pending_external_turn().unwrap().unwrap();assert_eq!(turn.processor,sampler_ir::SourceEventProcessorId::UviNode(id));
    rt.begin_external_turn(turn.input).unwrap()
}
fn hierarchy_post(rt:&mut Runtime,turn:sampler_core::ExternalTurn,output:sampler_core::ExternalOutput)->sampler_core::NoteId{
    let note=match rt.post_external_output(turn.input,output).unwrap(){sampler_core::ExternalOutputResult::Note(note)=>note,_=>panic!("actual note output required")};
    rt.complete_external_segment(turn.input,None).unwrap();note
}

#[test]
fn normal_source_note_hierarchy_runs_program_before_layer_filter_then_reuses_actual_kg_for_pcm(){
    use sampler_core::{ExternalEvent,ExternalOutput,NotePitch};
    let plan=hierarchy_note_plan(true,false);let layer_owner=plan.source_owner_index(sampler_ir::SourceOwnerId::UviNode(200)).unwrap();
    let limits=Limits::for_plan(&plan,32,16);let mut rt=Runtime::new(plan,limits).unwrap();let (execution,source)=hierarchy_source(1301);let perf=rt.performance(0).unwrap();
    let physical=rt.trigger_in_with_source(perf,execution,source,NotePitch::Key(60),1.,Expression::default()).unwrap();
    assert_eq!(rt.voice_count(),0);
    let first=hierarchy_begin(&mut rt,101);assert_eq!(first.logical_channel,Some(13));
    match first.event{ExternalEvent::Note{note,source:actual,pitch,..}=>{assert_eq!((note,actual,pitch),(physical,source,NotePitch::Key(60)));},_=>panic!()}
    support::without_heap(||{hierarchy_post(&mut rt,first,ExternalOutput::RoutedNote{pitch:NotePitch::Key(61),velocity:0.5,channel:2});});
    let second=hierarchy_begin(&mut rt,102);
    assert_eq!(second.logical_channel,Some(2),"authored channel is logical metadata, not rewritten physical provenance");
    match second.event{ExternalEvent::Note{source:actual,pitch,velocity,..}=>{assert_eq!(actual,source);assert_eq!(pitch,NotePitch::Key(61));assert_eq!(velocity,0.5);},_=>panic!()}
    // Its saved bypass=true has no bound signal. The real owner must still
    // receive this turn and make its accepted delivery-time bypass decision.
    hierarchy_post(&mut rt,second,ExternalOutput::Original);
    let layer=hierarchy_begin(&mut rt,201);assert_eq!(layer.owner,layer_owner);
    let voice=hierarchy_post(&mut rt,layer,ExternalOutput::Note{pitch:NotePitch::Key(62),velocity:0.375});
    assert!(rt.pending_external_turn().unwrap().is_none(),"ineligible sibling Layers never run");
    assert_eq!(rt.input_source(voice),Ok(None),"generated output is not a second physical input");
    assert_eq!(rt.input_source(physical),Ok(Some(source)));
    assert_eq!(rt.voice_count(),1);
    let reference_plan=hierarchy_note_plan(false,false);let limits=Limits::for_plan(&reference_plan,32,16);let mut reference=Runtime::new(reference_plan,limits).unwrap();
    reference.trigger(input(1399),62,0.375).unwrap();
    let mut actual=[[0.;2];128];let mut expected=[[0.;2];128];
    support::without_heap(||{rt.render(&mut actual).unwrap();reference.render(&mut expected).unwrap();});
    assert!(expected.iter().any(|frame|frame[0]!=0.));assert_eq!(actual,expected,"normal native source PCM is the independent reference");
}

#[test]
fn source_note_hierarchy_consumes_before_branching_and_each_selected_layer_has_its_own_turn(){
    use sampler_core::{ExternalOutput,NotePitch};
    let mut rt=hierarchy_runtime(true);let (execution,source)=hierarchy_source(1401);let perf=rt.performance(0).unwrap();
    rt.trigger_in_with_source(perf,execution,source,NotePitch::Key(60),1.,Expression::default()).unwrap();
    let consumed=hierarchy_begin(&mut rt,101);rt.complete_external_segment(consumed.input,None).unwrap();
    assert!(rt.pending_external_turn().unwrap().is_none());assert_eq!(rt.voice_count(),0);
    let mut silent=[[9.;2];32];rt.render(&mut silent).unwrap();assert_eq!(silent,[[0.;2];32]);
    let (execution,source)=hierarchy_source(1402);rt.trigger_in_with_source(perf,execution,source,NotePitch::Key(60),1.,Expression::default()).unwrap();
    let first=hierarchy_begin(&mut rt,101);hierarchy_post(&mut rt,first,ExternalOutput::RoutedNote{pitch:NotePitch::Key(61),velocity:0.375,channel:2});
    let second=hierarchy_begin(&mut rt,102);hierarchy_post(&mut rt,second,ExternalOutput::Original);
    let a=hierarchy_begin(&mut rt,201);hierarchy_post(&mut rt,a,ExternalOutput::Note{pitch:NotePitch::Key(62),velocity:0.375});
    let b=hierarchy_begin(&mut rt,301);assert_ne!(a.owner,b.owner);assert!(b.order>a.order);
    rt.complete_external_segment(b.input,None).unwrap();assert!(rt.pending_external_turn().unwrap().is_none());assert_eq!(rt.voice_count(),1,"consuming one Layer cannot replay or suppress its sibling output");
    // The untouched ordinary reference admits only the source that Layer A
    // selected, so the PCM oracle does not call the new hierarchy interpreter.
    let reference_plan=hierarchy_note_plan(false,false);let limits=Limits::for_plan(&reference_plan,32,16);let mut reference=Runtime::new(reference_plan,limits).unwrap();reference.trigger(input(1499),62,0.375).unwrap();
    let mut actual=[[0.;2];128];let mut expected=[[0.;2];128];rt.render(&mut actual).unwrap();reference.render(&mut expected).unwrap();assert_eq!(actual,expected);
}

fn channel_frontier_plan(compiled:bool)->Prepared{
    let plan=source_frontier_plan(60,false).with_variation(vec![sampler_core::Sequence{takes:2,
        policy:sampler_core::TakePolicy::Sequential,scope:sampler_core::SequenceScope::Channel,capacity:16}],
        vec![Some(sampler_core::Take{sequence:0,index:0}),Some(sampler_core::Take{sequence:0,index:1})],16,0).unwrap();
    if compiled{plan.with_source_note_hierarchy().unwrap()}else{plan}
}
#[test]
fn authored_logical_channel_owns_rr_while_repeated_original_physical_tuple_stays_immutable(){
    use sampler_core::{ExternalEvent,ExternalOutput,NotePitch,Trigger};
    let plan=channel_frontier_plan(true);let limits=Limits::for_plan(&plan,32,16);let mut actual=Runtime::new(plan,limits).unwrap();
    let source=Input{protocol:Protocol::Midi1,port:0,group:0,channel:1,key:60,external_id:None};
    let mut outputs=[None;3];
    for (at,channel) in [2,3,2].into_iter().enumerate(){
        actual.trigger(source,60,1.).unwrap();let turn=hierarchy_begin(&mut actual,211);
        match turn.event{ExternalEvent::Note{source:original,..}=>assert_eq!(original,source),_=>panic!()}
        outputs[at]=Some(hierarchy_post(&mut actual,turn,ExternalOutput::RoutedNote{pitch:NotePitch::Key(60),velocity:1.,channel}));
    }
    assert_eq!(actual.note_take(outputs[0].unwrap(),Trigger::Attack,0),Ok(Some(0)));
    assert_eq!(actual.note_take(outputs[1].unwrap(),Trigger::Attack,0),Ok(Some(0)),"different authored channel begins its own real sequence owner");
    assert_eq!(actual.note_take(outputs[2].unwrap(),Trigger::Attack,0),Ok(Some(1)),"same authored channel advances its retained sequence");
    let plan=channel_frontier_plan(false);let limits=Limits::for_plan(&plan,32,16);let mut reference=Runtime::new(plan,limits).unwrap();
    for channel in [2,3,2]{reference.trigger(Input{channel,..source},60,1.).unwrap();}
    let mut got=[[0.;2];128];let mut expected=[[0.;2];128];
    support::without_heap(||{actual.render(&mut got).unwrap();reference.render(&mut expected).unwrap();});
    assert!(expected.iter().any(|frame|frame[0]!=0.));assert_eq!(got,expected);
}

#[test]
fn source_note_output_invalid_channel_and_exhausted_note_capacity_do_not_publish_a_child(){
    use sampler_core::{ExternalOutput,NotePitch};
    let plan=channel_frontier_plan(true);let mut limits=Limits::for_plan(&plan,2,4);limits.expressions=2;
    let mut rt=Runtime::new(plan,limits).unwrap();let physical=rt.trigger(input(1601),60,1.).unwrap();
    let turn=hierarchy_begin(&mut rt,211);let before=(rt.now(),rt.note_count(),rt.expression_count(),rt.decision_count(),rt.voice_count());
    support::without_heap(||{
        assert_eq!(rt.post_external_output(turn.input,ExternalOutput::RoutedNote{pitch:NotePitch::Key(60),velocity:1.,channel:255}),Err(Error::InvalidInput));
        assert_eq!((rt.now(),rt.note_count(),rt.expression_count(),rt.decision_count(),rt.voice_count()),before);
        assert_eq!(rt.post_external_output(turn.input,ExternalOutput::RoutedNote{pitch:NotePitch::Key(60),velocity:1.,channel:2}),Err(Error::Capacity));
        assert_eq!((rt.now(),rt.note_count(),rt.expression_count(),rt.decision_count(),rt.voice_count()),before);
        rt.complete_external_segment(turn.input,None).unwrap();rt.note_off_exact(physical,None).unwrap();rt.flush_behaviors(|_,_,_|true);rt.flush_ended(|_|true);
    });
}

#[test]
fn identical_raw_physical_tuples_keep_distinct_historical_event_ids_across_note_and_plan_retirement(){
    use sampler_core::{ExternalEvent,ExternalOutput,ExternalOutputResult,NotePitch};
    let old=external_plan(false);let limits=Limits::for_plan(&old,16,16);let (rt,mut control)=Runtime::with_plan_updates(old,limits,2,1).unwrap();
    let mut rt=rt.with_receiver_capacity(1).unwrap();let receiver=external_physical_receiver(&mut rt,4);let old_plan=rt.active_plan();let source=physical_raw();
    let first=rt.trigger_receiver(receiver,1,source,None,1.,0.,true).unwrap();
    let second=rt.trigger_receiver(receiver,1,source,None,1.,0.,true).unwrap();assert_ne!(first,second);
    rt.cancel_input_note(first).unwrap();rt.cancel_input_note(second).unwrap();let mut ended=0;rt.flush_ended(|actual|{assert_eq!(actual,source);ended+=1;true});assert_eq!(ended,2);
    assert!(matches!(rt.note(first),Err(Error::StaleHandle)));assert!(matches!(rt.note(second),Err(Error::StaleHandle)));
    let replacement=external_plan(false).with_programs(vec![],None).unwrap();let request=control.submit(Box::new(replacement)).unwrap();assert_eq!(rt.poll_plan_update(),Ok(Some(request)));assert_ne!(rt.active_plan(),old_plan);
    raw_off(&mut rt,receiver,source);raw_off(&mut rt,receiver,source);assert_eq!(rt.plan_physical_event_count(old_plan),Ok(2));
    let mut mapped=[None;2];
    for (at,historical) in [first,second].into_iter().enumerate(){
        let receipt=rt.next_receiver_terminal(receiver).unwrap().unwrap();assert_eq!(receipt.release.note,historical);assert_eq!(receipt.release.source,source);
        rt.start_receiver_release_behavior(receipt,0,0).unwrap();let turn=rt.pending_external_turn().unwrap().unwrap();
        match turn.event{ExternalEvent::PhysicalRelease(event)=>{
            assert_eq!(event.physical_note,historical);assert_eq!(event.plan,old_plan);assert_eq!(event.source,source);
            assert!(matches!(rt.note(event.physical_note),Err(Error::StaleHandle)),"historical identity must NOT become a live Note parent");
        },_=>panic!("actual plan-owned physical event required")}
        rt.complete_receiver_terminal(receipt).unwrap();rt.begin_external_turn(turn.input).unwrap();
        let child=match rt.post_external_output(turn.input,ExternalOutput::Note{pitch:NotePitch::Key(60),velocity:1.}).unwrap(){ExternalOutputResult::Note(note)=>note,_=>panic!()};
        assert_eq!(rt.note_plan(child),Ok(old_plan));assert_eq!(rt.input_source(child),Ok(None));mapped[at]=Some((historical,child));
        rt.complete_external_segment(turn.input,None).unwrap();
    }
    assert_ne!(mapped[0].unwrap().1,mapped[1].unwrap().1,"each historical VM event can retain its OWN actual returned manual child");
    assert_eq!(rt.plan_physical_event_count(old_plan),Ok(0));assert_eq!(rt.collect_retired_plans(),0);
    let first_child=mapped[0].unwrap().1;let second_child=mapped[1].unwrap().1;
    rt.release(first_child).unwrap();assert!(rt.note(second_child).unwrap().2,"exact first returned voice release cannot close the same-key sibling");
    rt.release(second_child).unwrap();rt.render(&mut [[0.;2];128]).unwrap();rt.flush_behaviors(|_,_,_|true);rt.flush_ended(|_|true);assert_eq!(rt.collect_retired_plans(),1);
}

#[test]
fn source_owner_query_borrows_the_actual_retained_generation_and_never_aliases_replacement(){
    let plan=external_plan(false);let limits=Limits::for_plan(&plan,8,8);let (mut rt,mut control)=Runtime::with_plan_updates(plan,limits,2,1).unwrap();
    let old=rt.active_plan();let note=rt.note_on(input(1801),60,1.).unwrap();
    let empty=Prepared::new(48000,vec![],vec![],0).unwrap();let request=control.submit(Box::new(empty)).unwrap();assert_eq!(rt.poll_plan_update(),Ok(Some(request)));let new=rt.active_plan();assert_ne!(old,new);
    let processor=sampler_ir::SourceEventProcessorId::UviNode(11);
    support::without_heap(||{
        let table=rt.source_owners(old).unwrap();let actual=table.processor(processor).unwrap();
        assert_eq!(table.get(actual.owner).unwrap().id,sampler_ir::SourceOwnerId::UviNode(10));assert_eq!(actual.ordinal,0);
        assert!(rt.source_owners(new).unwrap().rows().is_empty());assert_eq!(rt.collect_retired_plans(),0);
        rt.cancel_input_note(note).unwrap();rt.flush_ended(|_|true);assert_eq!(rt.collect_retired_plans(),1);
        assert!(matches!(rt.source_owners(old),Err(Error::StaleHandle)));assert!(rt.source_owners(new).unwrap().rows().is_empty());
    });
}


fn hierarchy_two_layer_callbacks()->Prepared{
    use sampler_ir as ir;
    let ids=[10,20,21,30,31].map(ir::SourceOwnerId::UviNode);
    let owner=|at:usize,parent:Option<usize>,kind,children:Vec<usize>,zones:Vec<usize>,script:Option<u32>|ir::SourceOwner{
        id:ids[at],parent:parent.map(|i|ids[i]),kind,children:children.into_iter().map(|i|ids[i]).collect(),
        groups:zones.iter().copied().map(ir::GroupRef).collect(),zones:zones.into_iter().map(ir::ZoneRef).collect(),
        predicates:vec![],bypass:None,mute:None,event_processors:script.into_iter().map(|id|ir::SourceEventProcessor{
            id:ir::SourceEventProcessorId::UviNode(id),owner:ids[at],ordinal:0,kind:"ScriptProcessor".into(),
            bypass:ir::SourceSwitch{saved:false,control:None}}).collect()};
    let mut instrument=ir::Instrument{source:ir::SourceFormat::Uvi,groups:vec![ir::Group::default();2],
        assets:(0..2).map(|i|ir::Asset{location:ir::AssetLocation::Path(format!("sibling{i}.wav").into()),encoding:ir::Encoding::Wav,residency:Default::default(),root_key:None,loops:vec![]}).collect(),
        zones:(0..2).map(|i|ir::Zone{keys:ir::KeyRange{low:60,high:60},group:Some(ir::GroupRef(i)),pitch:ir::KeyTracking::Fixed,velocity:ir::VelocityResponse::None,..ir::Zone::new(ir::AssetRef(i))}).collect(),..ir::Instrument::default()};
    instrument.source_indices.owners=vec![
        owner(0,None,ir::SourceOwnerKind::Instrument,vec![1,3],vec![0,1],None),
        owner(1,Some(0),ir::SourceOwnerKind::Layer,vec![2],vec![0],Some(201)),
        owner(2,Some(1),ir::SourceOwnerKind::Keygroup,vec![],vec![0],None),
        owner(3,Some(0),ir::SourceOwnerKind::Layer,vec![4],vec![1],Some(301)),
        owner(4,Some(3),ir::SourceOwnerKind::Keygroup,vec![],vec![1],None)];
    sampler_core::lower::lower(&instrument,48000,vec![Pcm::new(48000,Box::from([[0.25;2];128])).unwrap(),Pcm::new(48000,Box::from([[0.5;2];128])).unwrap()],|_,_|unreachable!("no native behavior")).unwrap().with_source_note_hierarchy().unwrap()
}
#[test]
fn later_layer_callback_capacity_cancels_only_unpublished_pending_batch_and_retains_physical_key(){
    let plan=hierarchy_two_layer_callbacks();let mut limits=Limits::for_plan(&plan,16,8);limits.behaviors=2;
    let (mut rt,mut control)=Runtime::with_plan_updates(plan,limits,2,1).unwrap();let old=rt.active_plan();
    let physical=rt.trigger(input(1901),60,1.).unwrap();
    support::without_heap(||{
        assert_eq!(rt.take_fault().map(|(_,error)|error),Some(Error::Capacity));
        assert!(rt.pending_external_turn().unwrap().is_none(),"first Layer's callback-lifetime work must not survive a later sibling admission fault");
        assert_eq!(rt.voice_count(),0);assert_eq!(rt.input_held(physical),Ok(true),"a callback fault is not physical key-up");
        rt.flush_behaviors(|_,_,_|true);rt.flush_ended(|_|true);
        assert_eq!((rt.note_count(),rt.expression_count()),(1,1),"only the actual still-held physical owner remains");
    });
    let request=control.submit(Box::new(Prepared::new(48000,vec![],vec![],0).unwrap())).unwrap();assert_eq!(rt.poll_plan_update(),Ok(Some(request)));
    support::without_heap(||{
        assert_eq!(rt.collect_retired_plans(),0);rt.note_off_exact(physical,None).unwrap();rt.flush_behaviors(|_,_,_|true);rt.flush_ended(|_|true);
        assert_eq!((rt.note_count(),rt.expression_count()),(0,0));assert_eq!(rt.collect_retired_plans(),1);assert!(matches!(rt.source_owners(old),Err(Error::StaleHandle)));
    });
}

#[test]
fn hierarchy_structural_work_is_charged_before_sibling_note_or_callback_publication(){
    let plan=hierarchy_two_layer_callbacks();let mut limits=Limits::for_plan(&plan,16,8);limits.behaviors=8;limits.behavior_fuel=2;
    let mut rt=Runtime::new(plan,limits).unwrap();
    support::without_heap(||{
        let physical=rt.trigger(input(1902),60,1.).unwrap();
        assert_eq!(rt.take_fault().map(|(_,error)|error),Some(Error::Capacity));
        assert_eq!((rt.note_count(),rt.expression_count(),rt.voice_count()),(1,1,0));assert!(rt.pending_external_turn().unwrap().is_none());
        assert_eq!(rt.input_held(physical),Ok(true));rt.flush_behaviors(|_,_,_|true);rt.note_off_exact(physical,None).unwrap();rt.flush_ended(|_|true);
        assert_eq!((rt.note_count(),rt.expression_count()),(0,0));
    });
}
#[test]
fn consumed_reposts_retire_generated_inputs_and_preserve_exact_root_event_origin_for_same_tuple(){
    use sampler_core::{ExternalOutput,NotePitch};
    let plan=hierarchy_note_plan(true,false);let limits=Limits::for_plan(&plan,32,16);let (mut rt,mut control)=Runtime::with_plan_updates(plan,limits,2,1).unwrap();let old=rt.active_plan();
    let raw=Input{protocol:Protocol::Midi1,port:2,group:0,channel:1,key:60,external_id:None};
    let mut roots=[None;2];
    for slot in &mut roots{
        let root=rt.trigger(raw,60,1.).unwrap();*slot=Some(root);
        let first=hierarchy_begin(&mut rt,101);assert_eq!(first.event_origin,Some(root));
        let repost=hierarchy_post(&mut rt,first,ExternalOutput::Note{pitch:NotePitch::Key(61),velocity:1.});
        let second=hierarchy_begin(&mut rt,102);assert_eq!(second.event_origin,Some(root));
        support::without_heap(||{rt.complete_external_segment(second.input,None).unwrap();rt.flush_behaviors(|_,_,_|true);rt.flush_ended(|_|true);});
        assert!(matches!(rt.note(repost),Err(Error::StaleHandle)),"consumed generated input must retire after its actual callback completes");
        assert_eq!(rt.input_held(root),Ok(true));
    }
    assert_ne!(roots[0],roots[1]);assert_eq!((rt.note_count(),rt.expression_count(),rt.voice_count()),(2,2,0));
    let request=control.submit(Box::new(Prepared::new(48000,vec![],vec![],0).unwrap())).unwrap();assert_eq!(rt.poll_plan_update(),Ok(Some(request)));
    support::without_heap(||{
        assert_eq!(rt.collect_retired_plans(),0);
        for root in roots.into_iter().flatten(){rt.note_off_exact(root,None).unwrap();}
        rt.flush_behaviors(|_,_,_|true);rt.flush_ended(|_|true);assert_eq!((rt.note_count(),rt.expression_count()),(0,0));assert_eq!(rt.collect_retired_plans(),1);
        assert!(matches!(rt.source_owners(old),Err(Error::StaleHandle)));
    });
}
