use super::*;
use crate::script::control_id;
use crate::scripted::Driver;
use sampler_core::{Input, Limits, Pcm, Protocol, Runtime};
use std::time::Instant;

fn xml(script: &str) -> String {
    format!(
        "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    )
}

fn until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !done() {
        assert!(
            Instant::now() < deadline,
            "bounded ready-batch fixture timeout"
        );
        std::thread::yield_now();
    }
}

fn runtime() -> Runtime {
    use sampler_ir as ir;
    let source = ir::Instrument {
        assets: vec![ir::Asset {
            location: ir::AssetLocation::Path("authored-memory.wav".into()),
            residency: Default::default(),
            encoding: ir::Encoding::Wav,
            root_key: Some(60),
            loops: Vec::new(),
        }],
        zones: vec![ir::Zone {
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        ..Default::default()
    };
    let pcm = Pcm::new(48000, vec![[0.25; 2]; 2048].into_boxed_slice()).unwrap();
    let plan = sampler_core::lower::lower(&source, 48000, vec![pcm], |_, _| {
        unreachable!("authored fixture declares no behaviors")
    })
    .unwrap();
    let limits = Limits::for_plan(&plan, 8, 4);
    Runtime::new(plan, limits).unwrap()
}

fn physical(rt: &mut Runtime) -> sampler_core::NoteId {
    rt.note_on(
        Input {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
            key: 60,
            external_id: Some(1),
        },
        60,
        64. / 127.,
    )
    .unwrap()
}

#[test]
fn ten_thousand_widget_publication_does_not_hold_ready_note_pcm_or_complete_its_ack() {
    let source = xml(r#"
      calls=0
      local k=Knob('Control',0,0,127)
      local labels={}
      for i=1,10000 do labels[i]=Label('Label'..i) end
      function onNote(e)
        calls=calls+1
        k:setValue(e.velocity,false)
        labels[1].text='changed'
        playNote(e.note,e.velocity)
        error('authored note fault')
      end
      function onSave() return {calls=calls} end
    "#);
    let (thread, loaded) = ScriptThread::spawn(source.clone(), (), Config::default()).unwrap();
    loaded.ui.enable_diagnostics();
    let face = loaded.ui.face.lock().unwrap();
    let mut rt = runtime();
    let mut driver = Driver::new(thread, Vec::new(), 48000);
    let note = physical(&mut rt);
    driver.note_on(&mut rt, note, 60, 64. / 127.).unwrap();
    let mut request = loaded
        .ui
        .request_diagnostics(Duration::from_secs(8))
        .unwrap();
    until(|| loaded.ui.diagnostic_progress().unwrap().phase == DiagnosticPhase::UiExport);
    until(|| {
        driver.wake(&mut rt).unwrap();
        rt.voice_count() == 1
    });
    assert_eq!(request.poll(), DiagnosticPoll::Pending);
    assert!(
        !request.commands_drained(),
        "ordered marker still follows full UI/fault publication"
    );
    let mut pcm = [[0.; 2]; 256];
    rt.render(&mut pcm).unwrap();
    assert!(pcm.iter().flatten().any(|value| *value != 0.));
    assert_eq!(request.progress().commands_drained, 1);
    drop(face);
    let mut ack = None;
    until(|| {
        driver.wake(&mut rt).unwrap();
        if let DiagnosticPoll::Complete(value) = request.poll() {
            ack = Some(value);
        }
        ack.is_some() && request.commands_drained()
    });
    let ack = ack.unwrap();
    assert_eq!((ack.commands_collected, ack.commands_pushed), (1, 1));
    assert_eq!(ack.runtime_faults, 1);
    assert_eq!(loaded.ui.value(control_id(1, 0)), Some(64.));

    // Same callback, complete model and PCM through the ordinary inline owner.
    let reference = ScriptHost::new(&source, (), Config::default()).unwrap();
    let mut reference = Driver::new(reference, Vec::new(), 48000);
    let mut expected_rt = runtime();
    let note = physical(&mut expected_rt);
    reference
        .note_on(&mut expected_rt, note, 60, 64. / 127.)
        .unwrap();
    let expected_face = reference.host.interface();
    assert_eq!(*loaded.ui.interface(), expected_face);
    assert_eq!(loaded.ui.fault_counts(), reference.host.fault_counts());
    assert_eq!(
        loaded.ui.state().unwrap().custom,
        reference.host.save_ui_state().unwrap().custom
    );
    assert_eq!(reference.host.global_text("calls"), "1");
    let mut expected = [[0.; 2]; 256];
    expected_rt.render(&mut expected).unwrap();
    assert_eq!(pcm, expected);
}

#[test]
fn partial_or_zero_initial_queue_space_never_splits_a_ready_batch_before_ui() {
    for first in [1023, 1024] {
        let source = xml(&format!(
            "local l=Label('State'); function onNote(e) if e.note==60 then for i=1,{first} do postEvent{{type=Event.Controller,controller=1,value=17}} end else l.text='later'; for i=1,2 do postEvent{{type=Event.Controller,controller=1,value=18}} end end end"
        ));
        let (mut thread, loaded) = ScriptThread::spawn(source, (), Config::default()).unwrap();
        loaded.ui.enable_diagnostics();
        thread.note_on(1, 60, 64);
        until(|| loaded.ui.diagnostic_progress().unwrap().commands_pushed == first);
        let face = loaded.ui.face.lock().unwrap();
        thread.note_on(2, 61, 64);
        let mut request = loaded
            .ui
            .request_diagnostics(Duration::from_secs(8))
            .unwrap();
        until(|| {
            request.progress().events_executed == 2
                && request.progress().phase == DiagnosticPhase::UiExport
        });
        let mut commands = Vec::new();
        thread.drain(&mut commands);
        assert_eq!(
            commands.len(),
            first as usize,
            "second batch must be entirely late"
        );
        assert_eq!(request.poll(), DiagnosticPoll::Pending);
        drop(face);
        until(|| {
            thread.drain(&mut commands);
            matches!(request.poll(), DiagnosticPoll::Complete(_)) && request.commands_drained()
        });
        assert_eq!(commands.len(), first as usize + 2);
        assert!(
            commands[..first as usize]
                .iter()
                .all(|c| matches!(c,Command::Midi(m) if m.b==17))
        );
        assert!(
            commands[first as usize..]
                .iter()
                .all(|c| matches!(c,Command::Midi(m) if m.b==18))
        );
        assert_eq!(loaded.ui.interface().widgets[0].text, "later");
    }
}

#[test]
fn unsafe_large_batch_still_publishes_ui_before_waiting_for_audio_queue_space() {
    let (mut thread,loaded)=ScriptThread::spawn(xml("local l=Label('State'); function onNote(e) l.text='published'; for i=1,1100 do postEvent{type=Event.Controller,controller=1,value=17} end end"),(),Config::default()).unwrap();
    loaded.ui.enable_diagnostics();
    thread.note_on(1, 60, 64);
    until(|| loaded.ui.diagnostic_progress().unwrap().commands_pushed == 1024);
    assert_eq!(loaded.ui.interface().widgets[0].text, "published");
    let mut commands = Vec::new();
    until(|| {
        thread.drain(&mut commands);
        commands.len() == 1100
    });
}

#[test]
fn export_getter_commands_follow_the_ready_batch_and_precede_the_marker() {
    let source = xml(r#"
      armed=false
      local l=Label('State')
      setmetatable(rawget(l,'__data'),{__index=function(_,key)
        if armed and key=='fontSize' then armed=false;postEvent{type=Event.Controller,controller=1,value=18};return 12 end
      end})
      function onNote(e) l.text='changed';armed=true;postEvent{type=Event.Controller,controller=1,value=17} end
    "#);
    let (mut thread, loaded) = ScriptThread::spawn(source.clone(), (), Config::default()).unwrap();
    loaded.ui.enable_diagnostics();
    let face = loaded.ui.face.lock().unwrap();
    thread.note_on(1, 60, 64);
    let mut request = loaded
        .ui
        .request_diagnostics(Duration::from_secs(8))
        .unwrap();
    until(|| request.progress().phase == DiagnosticPhase::UiExport);
    let mut commands = Vec::new();
    thread.drain(&mut commands);
    assert!(matches!(&commands[..],[Command::Midi(m)] if m.b==17));
    assert_eq!(request.poll(), DiagnosticPoll::Pending);
    drop(face);
    until(|| {
        thread.drain(&mut commands);
        matches!(request.poll(), DiagnosticPoll::Complete(_)) && request.commands_drained()
    });
    assert!(matches!(&commands[..],[Command::Midi(a),Command::Midi(b)] if a.b==17 && b.b==18));
    let mut reference = ScriptHost::new(&source, (), Config::default()).unwrap();
    reference.note_on(1, 60, 64, 0);
    assert_eq!(*loaded.ui.interface(), reference.interface());
    assert_eq!(commands, reference.take_commands());
}

#[test]
fn dropping_an_owner_with_a_full_ready_batch_does_not_wait_for_an_audio_consumer() {
    let (mut thread,loaded)=ScriptThread::spawn(xml("function onNote(e) for i=1,1024 do postEvent{type=Event.Controller,controller=1,value=17} end end"),(),Config::default()).unwrap();
    loaded.ui.enable_diagnostics();
    thread.note_on(1, 60, 64);
    until(|| loaded.ui.diagnostic_progress().unwrap().commands_pushed == 1024);
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        drop(thread);
        done.send(()).unwrap();
    });
    finished.recv_timeout(Duration::from_secs(8)).unwrap();
    assert!(loaded.ui.interface().widgets.is_empty());
}

#[cfg(feature = "scan")]
#[test]
fn seeded_owner_keeps_the_existing_barrier_path_without_ready_batch_collection() {
    use crate::script::diagnostics::OwnerAudit;
    let audit = Arc::new(OwnerAudit::default());
    let config = Config {
        audit_seed: Some(7),
        ..Config::default()
    };
    let (mut thread,loaded)=ScriptThread::spawn_owner(xml("local l=Label('State');function onNote(e) l.text='changed';playNote(e.note,e.velocity) end").into(),(),config,None,Some(audit)).unwrap();
    thread.note_on(1, 60, 64);
    let mut commands = Vec::new();
    thread.drain(&mut commands);
    assert!(matches!(&commands[..], [Command::Play(_)]));
    assert_eq!(loaded.ui.interface().widgets[0].text, "changed");
    let audit = loaded.ui.owner_audit().unwrap();
    assert_eq!(
        audit
            .stages
            .iter()
            .find(|s| s.name == "ready_commands_collect")
            .unwrap()
            .calls,
        0
    );
    assert_eq!(
        audit
            .counters
            .iter()
            .find(|s| s.name == "ready_commands_pushed")
            .unwrap()
            .value,
        0
    );
}
