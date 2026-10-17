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
