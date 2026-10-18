//! Independent public-path script modulation recipient contracts.
//! No processor-context or external-authority API dependency.
use sampler_uvi::script::{Config, ScriptHost};

fn host(source: &str) -> ScriptHost {
    ScriptHost::new(&format!("<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"), (), Config::default()).unwrap()
}

fn broadcast_source_plan()->sampler_core::Prepared{
    use sampler_core::*;
    Prepared::new(48000,
        vec![Pcm::new(48000,(0..128).map(|i|if i%2==0{[0.125,-0.25]}else{[-0.125,0.25]}).collect()).unwrap()],
        vec![Region{sample:0,key_low:0,key_high:127,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,
            envelope:Envelope::new(0,0,0,1.,64).unwrap(),
            playback:Playback{loop_range:Some(Loop{start:0,end:128,mode:LoopMode::UntilRelease,shape:LoopShape::Wrap,passes:None}),..Playback::default()}}],128).unwrap()
        .with_voice_modulation(vec![ModProgram{sources:vec![ModSource::Script(0)],routes:vec![ModRoute::new(0,ModTarget::Pan,1.)],..ModProgram::default()}],vec![Some(0)],vec![0]).unwrap()
}
fn broadcast_input(key:u8,id:i32)->sampler_core::Input{sampler_core::Input{protocol:sampler_core::Protocol::Midi1,port:0,group:0,channel:3,key,external_id:Some(id)}}
fn broadcast_runtimes(source:&str)->(sampler_core::Runtime,sampler_uvi::scripted::Driver<ScriptHost>,sampler_core::Runtime){
    let plan=broadcast_source_plan();let limits=sampler_core::Limits::for_plan(&plan,16,16);
    let candidate=sampler_core::Runtime::new(plan,limits).unwrap();
    let reference=sampler_core::Runtime::new(broadcast_source_plan(),limits).unwrap();
    let host=host(source);assert!(host.findings().is_empty());
    (candidate,sampler_uvi::scripted::Driver::new(host,vec![],48000),reference)
}
fn broadcast_candidate_on(rt:&mut sampler_core::Runtime,driver:&mut sampler_uvi::scripted::Driver<ScriptHost>,key:u8,id:i32)->sampler_core::NoteId{
    let note=rt.note_on(broadcast_input(key,id),key,1.).unwrap();driver.note_on(rt,note,key,1.).unwrap();note
}
fn broadcast_pcm_equal(rt:&mut sampler_core::Runtime,driver:&mut sampler_uvi::scripted::Driver<ScriptHost>,reference:&mut sampler_core::Runtime,phase:&str){
    driver.wake(rt).unwrap();let mut actual=[[0.;2];128];let mut expected=[[0.;2];128];
    rt.render(&mut actual).unwrap();reference.render(&mut expected).unwrap();
    assert!(expected.iter().flatten().any(|value|value.abs()>0.01),"independent {phase} PCM witness must be audible");
    for (frame,(a,b)) in actual.iter().zip(&expected).enumerate(){for channel in 0..2{assert!((a[channel]-b[channel]).abs()<1e-7,"{phase} frame {frame} channel {channel}: actual {}, independent current-recipient reference {}",a[channel],b[channel]);}}
    assert!(driver.unmodeled().is_empty());assert_eq!(rt.now(),reference.now());
}
fn broadcast_release(rt:&mut sampler_core::Runtime,driver:&mut sampler_uvi::scripted::Driver<ScriptHost>,reference:&mut sampler_core::Runtime,notes:&[(u8,sampler_core::NoteId)]){
    for &(key,note) in notes{driver.note_off(rt,key).unwrap();reference.key_up(note,None).unwrap();}
    driver.wake(rt).unwrap();rt.render(&mut [[0.;2];256]).unwrap();reference.render(&mut [[0.;2];256]).unwrap();
    rt.flush_ended(|_|true);reference.flush_ended(|_|true);
    assert_eq!((rt.voice_count(),rt.note_count()),(0,0));assert_eq!((reference.voice_count(),reference.note_count()),(0,0));
}

fn broadcast_future_voice_case(callback:&str){
    let source=format!("{callback}
function onController(e) sendScriptModulation(0,1,0) end");
    let (mut rt,mut driver,mut reference)=broadcast_runtimes(&source);
    broadcast_candidate_on(&mut rt,&mut driver,60,1);let a=reference.trigger(broadcast_input(60,1),60,1.).unwrap();
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"before broadcast");
    driver.input(&rt,sampler_uvi::scripted::HostInput::Controller{cc:1,value:127,channel:3});driver.wake(&mut rt).unwrap();reference.set_note_script_value(a,0,1.).unwrap();
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"existing voice after broadcast");
    broadcast_candidate_on(&mut rt,&mut driver,61,2);let b=reference.trigger(broadcast_input(61,2),61,1.).unwrap();
    assert_eq!((rt.voice_count(),reference.voice_count()),(2,2));
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"future voice keeps initial source value");
    broadcast_release(&mut rt,&mut driver,&mut reference,&[(60,a),(61,b)]);
}
#[test]
fn script_modulation_broadcast_does_not_seed_later_automatic_forwarded_voice(){broadcast_future_voice_case("");}
#[test]
fn script_modulation_broadcast_does_not_seed_later_play_note_generated_child(){broadcast_future_voice_case("function onNote(e) playNote(e.note,e.velocity,-1) end");}
#[test]
fn script_modulation_broadcast_without_a_current_voice_is_not_a_future_default(){
    let (mut rt,mut driver,mut reference)=broadcast_runtimes("function onController(e) sendScriptModulation(0,1,0) end");
    driver.input(&rt,sampler_uvi::scripted::HostInput::Controller{cc:1,value:127,channel:3});driver.wake(&mut rt).unwrap();
    assert_eq!(rt.voice_count(),0);
    broadcast_candidate_on(&mut rt,&mut driver,60,1);let a=reference.trigger(broadcast_input(60,1),60,1.).unwrap();
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"no recipient at broadcast");
    broadcast_release(&mut rt,&mut driver,&mut reference,&[(60,a)]);
}
#[test]
fn script_modulation_explicit_new_voice_seed_is_a_positive_control_for_the_same_pcm_oracle(){
    let source="function onNote(e) local id=postEvent(e);if e.note==61 then sendScriptModulation(0,1,0,id) end end;function onController(e) sendScriptModulation(0,1,0) end";
    let (mut rt,mut driver,mut reference)=broadcast_runtimes(source);
    broadcast_candidate_on(&mut rt,&mut driver,60,1);let a=reference.trigger(broadcast_input(60,1),60,1.).unwrap();
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"initial seeded-control voice");
    driver.input(&rt,sampler_uvi::scripted::HostInput::Controller{cc:1,value:127,channel:3});driver.wake(&mut rt).unwrap();reference.set_note_script_value(a,0,1.).unwrap();
    broadcast_candidate_on(&mut rt,&mut driver,61,2);let b=reference.trigger(broadcast_input(61,2),61,1.).unwrap();reference.set_note_script_value(b,0,1.).unwrap();
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"explicit future seed positive control");
    broadcast_release(&mut rt,&mut driver,&mut reference,&[(60,a),(61,b)]);
}

#[test]
fn script_modulation_current_read_uses_real_generation_and_existing_quantized_clamped_store(){
    use sampler_core::*;
    let plan=broadcast_source_plan();let limits=Limits::for_plan(&plan,16,16);let mut rt=Runtime::new(plan,limits).unwrap();
    let note=rt.trigger(broadcast_input(60,1),60,1.).unwrap();let at=rt.now();
    for (authored,accepted) in [(2.,1.),(-2.,-1.),(0.12345649,0.123456),(-0.1234566,-0.123457)]{
        rt.set_note_script_value(note,0,authored).unwrap();assert_eq!(rt.note_script_value(note,0).unwrap(),accepted);
        assert_eq!(rt.now(),at,"pure query cannot service or advance Core");
    }
    assert_eq!(rt.note_script_value(note,u16::MAX),Err(Error::InvalidInput));
    rt.key_up(note,None).unwrap();rt.render(&mut [[0.;2];512]).unwrap();rt.flush_ended(|_|true);
    assert_eq!(rt.note_script_value(note,0),Err(Error::StaleHandle));
    let replacement=rt.trigger(broadcast_input(60,1),60,1.).unwrap();assert_ne!(replacement,note);
    assert_eq!(rt.note_script_value(replacement,0),Ok(0.));assert_eq!(rt.note_script_value(note,0),Err(Error::StaleHandle));
}
#[test]
fn broadcast_ramp_does_not_enroll_a_note_created_after_its_recipient_snapshot(){
    let (mut rt,mut driver,mut reference)=broadcast_runtimes("function onController(e) sendScriptModulation(0,1,20) end");
    let a_actual=broadcast_candidate_on(&mut rt,&mut driver,60,1);let a=reference.trigger(broadcast_input(60,1),60,1.).unwrap();
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"mid-ramp setup");
    driver.input(&rt,sampler_uvi::scripted::HostInput::Controller{cc:1,value:127,channel:3});driver.wake(&mut rt).unwrap();
    rt.render(&mut [[0.;2];240]).unwrap();reference.render(&mut [[0.;2];240]).unwrap();driver.wake(&mut rt).unwrap();reference.set_note_script_value(a,0,0.25).unwrap();
    assert_eq!(rt.note_script_value(a_actual,0),Ok(0.25));
    let b_actual=broadcast_candidate_on(&mut rt,&mut driver,61,2);let b=reference.trigger(broadcast_input(61,2),61,1.).unwrap();
    rt.render(&mut [[0.;2];240]).unwrap();reference.render(&mut [[0.;2];240]).unwrap();driver.wake(&mut rt).unwrap();reference.set_note_script_value(a,0,0.5).unwrap();
    assert_eq!(rt.note_script_value(b_actual,0),Ok(0.),"a later voice is not a recipient of an already-running broadcast ramp");
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"future voice excluded from later ramp steps");
    broadcast_release(&mut rt,&mut driver,&mut reference,&[(60,a),(61,b)]);
}
#[test]
fn broadcast_ramp_starts_each_current_recipient_from_its_own_actual_stored_value(){
    let source="function onNote(e) local id=postEvent(e);sendScriptModulation(0,e.note==60 and 0.2 or 0.8,0,id) end;function onController(e) sendScriptModulation(0,1,20) end";
    let (mut rt,mut driver,mut reference)=broadcast_runtimes(source);
    let a_actual=broadcast_candidate_on(&mut rt,&mut driver,60,1);let a=reference.trigger(broadcast_input(60,1),60,1.).unwrap();reference.set_note_script_value(a,0,0.2).unwrap();
    let b_actual=broadcast_candidate_on(&mut rt,&mut driver,61,2);let b=reference.trigger(broadcast_input(61,2),61,1.).unwrap();reference.set_note_script_value(b,0,0.8).unwrap();
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"distinct current values before broadcast");
    driver.input(&rt,sampler_uvi::scripted::HostInput::Controller{cc:1,value:127,channel:3});driver.wake(&mut rt).unwrap();
    rt.render(&mut [[0.;2];240]).unwrap();reference.render(&mut [[0.;2];240]).unwrap();driver.wake(&mut rt).unwrap();
    reference.set_note_script_value(a,0,0.4).unwrap();reference.set_note_script_value(b,0,0.85).unwrap();
    assert_eq!(rt.note_script_value(a_actual,0),Ok(0.4));assert_eq!(rt.note_script_value(b_actual,0),Ok(0.85));
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"per-recipient ramp start values");
    broadcast_release(&mut rt,&mut driver,&mut reference,&[(60,a),(61,b)]);
}

#[test]
fn broadcast_recipient_is_same_frame_started_voice_even_before_first_pcm(){
    let (mut rt,mut driver,mut reference)=broadcast_runtimes("function onController(e) sendScriptModulation(0,1,0) end");
    let actual=broadcast_candidate_on(&mut rt,&mut driver,60,1);let note=reference.trigger(broadcast_input(60,1),60,1.).unwrap();
    assert_eq!(rt.now(),0);assert_eq!(rt.note_has_started_voices(actual),Ok(true));
    driver.input(&rt,sampler_uvi::scripted::HostInput::Controller{cc:1,value:127,channel:3});driver.wake(&mut rt).unwrap();reference.set_note_script_value(note,0,1.).unwrap();
    assert_eq!(rt.note_script_value(actual,0),Ok(1.));
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"same-frame admitted before first PCM");
    broadcast_release(&mut rt,&mut driver,&mut reference,&[(60,note)]);
}
#[test]
fn broadcast_excludes_admitted_future_deadline_voice_until_its_real_start(){
    let source="function onNote(e) end;function onController(e) sendScriptModulation(0,1,0) end";
    let (mut rt,mut driver,mut reference)=broadcast_runtimes(source);
    let actual=broadcast_candidate_on(&mut rt,&mut driver,60,1);let note=reference.note_on(broadcast_input(60,1),60,1.).unwrap();
    rt.start(actual,0,960,1.).unwrap();reference.start(note,0,960,1.).unwrap();
    assert_eq!((rt.voice_count(),reference.voice_count()),(1,1));assert_eq!(rt.note_has_started_voices(actual),Ok(false));
    driver.input(&rt,sampler_uvi::scripted::HostInput::Controller{cc:1,value:127,channel:3});driver.wake(&mut rt).unwrap();
    assert_eq!(rt.note_script_value(actual,0),Ok(0.),"reservation is not yet an authored-deadline-admitted recipient");
    rt.render(&mut [[0.;2];960]).unwrap();reference.render(&mut [[0.;2];960]).unwrap();rt.render(&mut []).unwrap();reference.render(&mut []).unwrap();
    assert_eq!(rt.note_has_started_voices(actual),Ok(true));
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"delayed source starts at initial value");
    broadcast_release(&mut rt,&mut driver,&mut reference,&[(60,note)]);
}
#[test]
fn started_query_keeps_zero_gain_and_release_tail_voices_and_rejects_retired_generation(){
    let plan=broadcast_source_plan();let limits=sampler_core::Limits::for_plan(&plan,16,16);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();
    let note=rt.note_on(broadcast_input(60,1),60,1.).unwrap();let family=rt.create_family(note).unwrap();
    rt.start_family(family,0,rt.now(),0.,sampler_core::Envelope::new(0,0,0,1.,64).unwrap(),sampler_core::Playback::default()).unwrap();rt.finish_family(family).unwrap();
    assert_eq!(rt.note_has_started_voices(note),Ok(true),"gain/mute/audibility cannot replace actual started ownership");
    rt.key_up(note,None).unwrap();assert_eq!(rt.note_has_started_voices(note),Ok(true),"release tails are existing recipients");
    rt.render(&mut [[0.;2];512]).unwrap();rt.flush_ended(|_|true);assert_eq!(rt.note_has_started_voices(note),Err(sampler_core::Error::StaleHandle));
    let replacement=rt.note_on(broadcast_input(60,1),60,1.).unwrap();assert_ne!(note,replacement);assert_eq!(rt.note_has_started_voices(replacement),Ok(false));
}

#[test]
fn refused_broadcast_time_overflow_preserves_current_value_and_existing_recipient_ramp(){
    for (bad,error) in [("1e30",sampler_core::Error::ClockOverflow),("math.huge",sampler_core::Error::InvalidInput)]{
        let source=format!("function onController(e) if e.controller==1 then sendScriptModulation(0,1,40) else sendScriptModulation(0,0.5,{bad}) end end");
        let (mut rt,mut driver,mut reference)=broadcast_runtimes(&source);
        let actual=broadcast_candidate_on(&mut rt,&mut driver,60,1);let note=reference.trigger(broadcast_input(60,1),60,1.).unwrap();
        broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"overflow setup");
        driver.input(&rt,sampler_uvi::scripted::HostInput::Controller{cc:1,value:127,channel:3});driver.wake(&mut rt).unwrap();
        rt.render(&mut [[0.;2];240]).unwrap();reference.render(&mut [[0.;2];240]).unwrap();driver.wake(&mut rt).unwrap();reference.set_note_script_value(note,0,0.125).unwrap();
        let at=rt.now();let value=rt.note_script_value(actual,0).unwrap();
        driver.input(&rt,sampler_uvi::scripted::HostInput::Controller{cc:2,value:127,channel:3});assert_eq!(driver.wake(&mut rt),Err(error));
        assert_eq!(rt.now(),at);assert_eq!(rt.note_script_value(actual,0),Ok(value));
        rt.render(&mut [[0.;2];240]).unwrap();reference.render(&mut [[0.;2];240]).unwrap();driver.wake(&mut rt).unwrap();reference.set_note_script_value(note,0,0.25).unwrap();
        broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"existing valid ramp survives rejected time");
        broadcast_release(&mut rt,&mut driver,&mut reference,&[(60,note)]);
    }
}
#[test]
fn panic_retirement_and_slot_reuse_cannot_apply_a_retained_old_ramp_to_a_new_note(){
    let (mut rt,mut driver,mut reference)=broadcast_runtimes("function onController(e) sendScriptModulation(0,1,40) end");
    let old=broadcast_candidate_on(&mut rt,&mut driver,60,1);reference.trigger(broadcast_input(60,1),60,1.).unwrap();
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"retirement setup");
    driver.input(&rt,sampler_uvi::scripted::HostInput::Controller{cc:1,value:127,channel:3});driver.wake(&mut rt).unwrap();
    rt.panic();reference.panic();rt.flush_ended(|_|true);reference.flush_ended(|_|true);
    assert_eq!(rt.note_script_value(old,0),Err(sampler_core::Error::StaleHandle));
    let current=broadcast_candidate_on(&mut rt,&mut driver,60,2);let note=reference.trigger(broadcast_input(60,2),60,1.).unwrap();assert_ne!(current,old);
    rt.render(&mut [[0.;2];240]).unwrap();reference.render(&mut [[0.;2];240]).unwrap();driver.wake(&mut rt).unwrap();
    assert_eq!(rt.note_script_value(current,0),Ok(0.));broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"retired generation ramp cannot reach replacement");
    driver.note_off(&mut rt,60).unwrap();assert!(rt.note(current).unwrap().2,"old physical release debt cannot close replacement");
    broadcast_release(&mut rt,&mut driver,&mut reference,&[(60,note)]);
}
#[test]
fn broadcast_glide_capacity_is_priced_for_the_whole_snapshot_before_replacing_any_current_ramp(){
    use sampler_core::*;
    use sampler_uvi::scripted::{Driver,HostInput,SCRIPT_MODULATION_GLIDE_CAPACITY};
    let recipients=SCRIPT_MODULATION_GLIDE_CAPACITY/128;assert_eq!(recipients*128,SCRIPT_MODULATION_GLIDE_CAPACITY);assert!(recipients+1<1024);
    let plan=broadcast_source_plan();let mut limits=Limits::for_plan(&plan,recipients+8,recipients+8);limits.families=recipients+8;limits.decisions=recipients+8;let mut rt=Runtime::new(plan,limits).unwrap();
    let source="function onController(e) if e.controller==1 then for id=0,127 do sendScriptModulation(id,1,1000) end else sendScriptModulation(0,0.5,1000) end end";
    let mut driver=Driver::new(host(source),vec![],48000);let mut notes=Vec::new();
    for index in 0..recipients{
        let input=Input{protocol:Protocol::Clap,port:0,group:0,channel:3,key:60,external_id:Some(index as i32)};
        let note=rt.note_on(input,60,1.).unwrap();driver.note_on(&mut rt,note,60,1.).unwrap();notes.push(note);
    }
    driver.input(&rt,HostInput::Controller{cc:1,value:127,channel:3});driver.wake(&mut rt).unwrap();
    let input=Input{protocol:Protocol::Clap,port:0,group:0,channel:3,key:61,external_id:Some(9999)};
    let extra=rt.note_on(input,61,1.).unwrap();driver.note_on(&mut rt,extra,61,1.).unwrap();notes.push(extra);
    let before=notes.iter().map(|&note|rt.note_script_value(note,0).unwrap()).collect::<Vec<_>>();let at=rt.now();
    driver.input(&rt,HostInput::Controller{cc:2,value:127,channel:3});assert_eq!(driver.wake(&mut rt),Err(Error::Capacity));
    assert_eq!(rt.now(),at);for (&note,&value) in notes.iter().zip(&before){assert_eq!(rt.note_script_value(note,0),Ok(value));}
    // Rejection did not remove/replace old recipient ramps. The extra note is
    // still outside them; no partial capacity admission is disguised as success.
    rt.render(&mut [[0.;2];240]).unwrap();driver.wake(&mut rt).unwrap();
    for &note in &notes[..recipients]{assert_eq!(rt.note_script_value(note,0),Ok(0.005));}
    assert_eq!(rt.note_script_value(extra,0),Ok(0.));
    rt.panic();rt.flush_ended(|_|true);driver.wake(&mut rt).unwrap();assert_eq!(rt.voice_count(),0);
}

#[test]
fn refused_modulation2_time_preserves_start_value_and_the_existing_ramp(){
    for (bad,error) in [("1e30",sampler_core::Error::ClockOverflow),("math.huge",sampler_core::Error::InvalidInput)]{
        let source=format!("function onController(e) if e.controller==1 then sendScriptModulation(0,1,40) else sendScriptModulation2(0,0.9,0.5,{bad}) end end");
        let (mut rt,mut driver,mut reference)=broadcast_runtimes(&source);
        let actual=broadcast_candidate_on(&mut rt,&mut driver,60,1);let note=reference.trigger(broadcast_input(60,1),60,1.).unwrap();
        broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"modulation2 overflow setup");
        driver.input(&rt,sampler_uvi::scripted::HostInput::Controller{cc:1,value:127,channel:3});driver.wake(&mut rt).unwrap();
        rt.render(&mut [[0.;2];240]).unwrap();reference.render(&mut [[0.;2];240]).unwrap();driver.wake(&mut rt).unwrap();reference.set_note_script_value(note,0,0.125).unwrap();
        let at=rt.now();let value=rt.note_script_value(actual,0).unwrap();
        driver.input(&rt,sampler_uvi::scripted::HostInput::Controller{cc:2,value:127,channel:3});assert_eq!(driver.wake(&mut rt),Err(error));
        assert_eq!(rt.now(),at);assert_eq!(rt.note_script_value(actual,0),Ok(value),"refused ramp must not commit its explicit start");
        rt.render(&mut [[0.;2];240]).unwrap();reference.render(&mut [[0.;2];240]).unwrap();driver.wake(&mut rt).unwrap();reference.set_note_script_value(note,0,0.25).unwrap();
        broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"old ramp survives refused modulation2");
        broadcast_release(&mut rt,&mut driver,&mut reference,&[(60,note)]);
    }
}
#[test]
fn modulation2_explicit_start_uses_accepted_core_quantization_then_one_recipient_ramp(){
    let (mut rt,mut driver,mut reference)=broadcast_runtimes("function onController(e) sendScriptModulation2(0,0.1234566,1,20) end");
    let actual=broadcast_candidate_on(&mut rt,&mut driver,60,1);let note=reference.trigger(broadcast_input(60,1),60,1.).unwrap();
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"modulation2 positive setup");
    driver.input(&rt,sampler_uvi::scripted::HostInput::Controller{cc:1,value:127,channel:3});driver.wake(&mut rt).unwrap();reference.set_note_script_value(note,0,0.1234566).unwrap();
    assert_eq!(rt.note_script_value(actual,0),Ok(0.123457));
    // No render between the explicit start and admitted ramp: same actual F.
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"accepted explicit start");
    rt.render(&mut [[0.;2];112]).unwrap();reference.render(&mut [[0.;2];112]).unwrap();driver.wake(&mut rt).unwrap();
    reference.set_note_script_value(note,0,0.123457+(1.-0.123457)*0.25).unwrap();
    assert_eq!(rt.note_script_value(actual,0),reference.note_script_value(note,0));
    broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"ramp from accepted quantized start");
    broadcast_release(&mut rt,&mut driver,&mut reference,&[(60,note)]);
}
#[test]
fn modulation2_capacity_refusal_does_not_commit_start_or_remove_existing_ramps(){
    use sampler_core::*;
    use sampler_uvi::scripted::{Driver,HostInput,SCRIPT_MODULATION_GLIDE_CAPACITY};
    let recipients=SCRIPT_MODULATION_GLIDE_CAPACITY/128;assert_eq!(recipients*128,SCRIPT_MODULATION_GLIDE_CAPACITY);assert!(recipients+1<1024);
    let plan=broadcast_source_plan();let mut limits=Limits::for_plan(&plan,recipients+8,recipients+8);limits.families=recipients+8;limits.decisions=recipients+8;let mut rt=Runtime::new(plan,limits).unwrap();
    let source="function onController(e) if e.controller==1 then for id=0,127 do sendScriptModulation(id,1,1000) end else sendScriptModulation2(0,0.9,0.5,1000) end end";
    let mut driver=Driver::new(host(source),vec![],48000);let mut notes=Vec::new();
    for index in 0..recipients{
        let input=Input{protocol:Protocol::Clap,port:0,group:0,channel:3,key:60,external_id:Some(index as i32)};
        let note=rt.note_on(input,60,1.).unwrap();driver.note_on(&mut rt,note,60,1.).unwrap();notes.push(note);
    }
    driver.input(&rt,HostInput::Controller{cc:1,value:127,channel:3});driver.wake(&mut rt).unwrap();
    let input=Input{protocol:Protocol::Clap,port:0,group:0,channel:3,key:61,external_id:Some(9999)};
    let extra=rt.note_on(input,61,1.).unwrap();driver.note_on(&mut rt,extra,61,1.).unwrap();notes.push(extra);
    let before=notes.iter().map(|&note|rt.note_script_value(note,0).unwrap()).collect::<Vec<_>>();let at=rt.now();
    driver.input(&rt,HostInput::Controller{cc:2,value:127,channel:3});assert_eq!(driver.wake(&mut rt),Err(Error::Capacity));
    assert_eq!(rt.now(),at);for (&note,&value) in notes.iter().zip(&before){assert_eq!(rt.note_script_value(note,0),Ok(value));}
    // Rejection did not remove/replace old recipient ramps. The extra note is
    // still outside them; no partial capacity admission is disguised as success.
    rt.render(&mut [[0.;2];240]).unwrap();driver.wake(&mut rt).unwrap();
    for &note in &notes[..recipients]{assert_eq!(rt.note_script_value(note,0),Ok(0.005));}
    assert_eq!(rt.note_script_value(extra,0),Ok(0.));
    rt.panic();rt.flush_ended(|_|true);driver.wake(&mut rt).unwrap();assert_eq!(rt.voice_count(),0);
}


// These use the existing public Lua/Driver/Core path; no new validation API.
// NaN and negative infinity duration were previously sanitized to zero by max.
#[test]
fn nonfinite_script_modulation_requests_refuse_without_replacing_the_current_ramp(){
    use sampler_core::Error;
    use sampler_uvi::scripted::HostInput;
    let cases=[
        ("single NaN duration","sendScriptModulation(0,0.9,0/0)"),
        ("single negative infinite duration","sendScriptModulation(0,0.9,-math.huge)"),
        ("single positive infinite duration","sendScriptModulation(0,0.9,math.huge)"),
        ("combined NaN duration","sendScriptModulation2(0,0.9,0.5,0/0)"),
        ("combined negative infinite duration","sendScriptModulation2(0,0.9,0.5,-math.huge)"),
        ("combined positive infinite duration","sendScriptModulation2(0,0.9,0.5,math.huge)"),
        ("single NaN target","sendScriptModulation(0,0/0,20)"),
        ("single negative infinite target","sendScriptModulation(0,-math.huge,20)"),
        ("single positive infinite target","sendScriptModulation(0,math.huge,20)"),
        ("combined NaN start","sendScriptModulation2(0,0/0,0.5,20)"),
        ("combined negative infinite start","sendScriptModulation2(0,-math.huge,0.5,20)"),
        ("combined positive infinite start","sendScriptModulation2(0,math.huge,0.5,20)"),
        ("combined NaN target","sendScriptModulation2(0,0.9,0/0,20)"),
        ("combined negative infinite target","sendScriptModulation2(0,0.9,-math.huge,20)"),
        ("combined positive infinite target","sendScriptModulation2(0,0.9,math.huge,20)"),
    ];
    for (label,request) in cases{
        let source=format!("function onController(e) if e.controller==1 then sendScriptModulation(0,1,40) else {request} end end");
        let (mut rt,mut driver,mut reference)=broadcast_runtimes(&source);
        let actual=broadcast_candidate_on(&mut rt,&mut driver,60,1);
        let note=reference.trigger(broadcast_input(60,1),60,1.).unwrap();
        broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"nonfinite setup");
        driver.input(&rt,HostInput::Controller{cc:1,value:127,channel:3});driver.wake(&mut rt).unwrap();
        rt.render(&mut [[0.;2];240]).unwrap();reference.render(&mut [[0.;2];240]).unwrap();
        driver.wake(&mut rt).unwrap();reference.set_note_script_value(note,0,0.125).unwrap();
        let at=rt.now();let value=rt.note_script_value(actual,0).unwrap();
        let counts=(rt.note_count(),rt.voice_count());
        driver.input(&rt,HostInput::Controller{cc:2,value:127,channel:3});
        assert_eq!(driver.wake(&mut rt),Err(Error::InvalidInput),"{label}: refusal must occur before any recipient write or ramp replacement");
        assert_eq!(rt.now(),at,"{label}");assert_eq!((rt.note_count(),rt.voice_count()),counts,"{label}");
        assert_eq!(rt.note_script_value(actual,0),Ok(value),"{label}: accepted value retained");
        rt.render(&mut [[0.;2];240]).unwrap();reference.render(&mut [[0.;2];240]).unwrap();
        driver.wake(&mut rt).unwrap();reference.set_note_script_value(note,0,0.25).unwrap();
        assert_eq!(rt.note_script_value(actual,0),Ok(0.25),"{label}: earlier ramp retained");
        broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,label);
        broadcast_release(&mut rt,&mut driver,&mut reference,&[(60,note)]);
    }
}

#[test]
fn finite_negative_script_modulation_duration_retains_the_existing_instant_policy(){
    use sampler_uvi::scripted::HostInput;
    for request in ["sendScriptModulation(0,-0.375,-1)","sendScriptModulation2(0,0.9,-0.375,-1)"]{
        let source=format!("function onController(e) {request} end");
        let (mut rt,mut driver,mut reference)=broadcast_runtimes(&source);
        let actual=broadcast_candidate_on(&mut rt,&mut driver,60,1);
        let note=reference.trigger(broadcast_input(60,1),60,1.).unwrap();
        broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"finite negative setup");
        let at=rt.now();driver.input(&rt,HostInput::Controller{cc:1,value:127,channel:3});
        driver.wake(&mut rt).unwrap();reference.set_note_script_value(note,0,-0.375).unwrap();
        assert_eq!(rt.now(),at);assert_eq!(rt.note_script_value(actual,0),Ok(-0.375));
        broadcast_pcm_equal(&mut rt,&mut driver,&mut reference,"finite negative remains instant");
        broadcast_release(&mut rt,&mut driver,&mut reference,&[(60,note)]);
    }
}
