#![cfg(feature = "scan")]
use sampler_uvi::script::{Config, ScriptHost};

fn draws(seed: Option<u32>) -> String {
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script>draws=''; for n=1,16 do draws=draws..math.random()..',' end</script></ScriptProcessor></EventProcessors></Program></UVI4>";
    ScriptHost::new(
        xml,
        (),
        Config {
            audit_seed: seed,
            ..Config::default()
        },
    )
    .unwrap()
    .global_text("draws")
}
#[test]
fn explicit_audit_seed_repeats_but_default_rng_stays_random() {
    assert_eq!(draws(Some(42)), draws(Some(42)));
    assert_ne!(draws(Some(42)), draws(Some(43)));
    assert_ne!(draws(None), draws(None));
}

use sampler_uvi::{
    script::Command,
    scripted::{Script, ScriptThread},
};
#[test]
fn audit_barrier_drains_events_and_due_coroutines_at_virtual_clock() {
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script>function onNote(e) playNote(e.note,e.velocity); spawn(function() wait(2); playNote(e.note+1,e.velocity) end) end</script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let (mut thread, _) = ScriptThread::spawn(
        xml.into(),
        (),
        Config {
            audit_seed: Some(42),
            ..Config::default()
        },
    )
    .unwrap();
    thread.set_time(0.);
    thread.note_on(1, 60, 64);
    let mut commands = Vec::new();
    thread.drain(&mut commands);
    assert!(matches!(&commands[..], [Command::Play(p)] if p.key==60 && p.at_ms==0.));
    assert_eq!(thread.next_due(), Some(2.));
    commands.clear();
    thread.tick(1.);
    thread.drain(&mut commands);
    assert!(commands.is_empty());
    thread.tick(2.);
    thread.drain(&mut commands);
    assert!(matches!(&commands[..], [Command::Play(p)] if p.key==61 && p.at_ms==2.));
    assert_eq!(thread.next_due(), None);
    assert!(thread.ui().scan_progress().is_none(), "progress must be explicitly enabled");
}
#[test]
fn audit_barrier_survives_command_and_event_queue_backpressure() {
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script>function onNote(e) for n=1,e.velocity do playNote(e.note,64) end end</script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let (mut thread, _) = ScriptThread::spawn(
        xml.into(),
        (),
        Config {
            audit_seed: Some(42),
            ..Config::default()
        },
    )
    .unwrap();
    for id in 1..=1100 {
        thread.note_on(id, 60, 2);
    }
    let mut commands = Vec::new();
    thread.drain(&mut commands);
    assert_eq!(commands.len(), 2200);
    assert!(commands.iter().all(|c| matches!(c, Command::Play(_))));
}

#[test]
fn audit_rng_cannot_change_user_persistence() {
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script>function onSave() return {value=73} end</script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let ordinary = ScriptHost::new(xml, (), Config::default())
        .unwrap()
        .save_ui_state()
        .unwrap();
    let seeded = ScriptHost::new(
        xml,
        (),
        Config {
            audit_seed: Some(42),
            ..Config::default()
        },
    )
    .unwrap()
    .save_ui_state()
    .unwrap();
    assert_eq!(ordinary, seeded);
}

#[test]
fn audit_barrier_publishes_ui_and_faults_before_reply() {
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script>
      local k=Knob('Velocity',0,0,127)
      local labels={}; for n=1,4096 do labels[n]=Label('Status'); labels[n].text='0' end
      function onNote(e)
        k:setValue(e.velocity,false)
        for n=1,#labels do labels[n].text=tostring(e.velocity) end
        playNote(e.note,e.velocity)
        error('synthetic callback fault')
      end
    </script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let (mut thread, loaded) = ScriptThread::spawn(xml.into(), (), Config {
        audit_seed: Some(42), ..Config::default()
    }).unwrap();
    thread.note_on(1,60,64);
    let mut commands=Vec::new();
    thread.drain(&mut commands);
    assert!(commands.iter().any(|c| matches!(c,Command::Play(_))));
    assert_eq!(loaded.ui.value(sampler_uvi::script::control_id(1,0)), Some(64.),
        "a completed audit barrier must publish control readback");
    let face=loaded.ui.interface();
    assert_eq!(face.widgets.len(),4097);
    assert!(face.widgets[1..].iter().all(|w| w.text=="64"),
        "a completed audit barrier must publish the authored UI: {:?}",
        face.widgets[1..].iter().enumerate().filter(|(_,w)| w.text!="64").take(4)
            .map(|(i,w)| (i,w.source_id,&w.name,&w.text,&w.kind)).collect::<Vec<_>>());
    assert_eq!(loaded.ui.runtime_faults(),(1,0),
        "a completed audit barrier must publish callback faults");
    assert_eq!(thread.scan_faults().runtime_count,1);
}

#[test]
fn owner_progress_remains_readable_while_callback_is_blocked() {
    use sampler_uvi::script::{Files, diagnostics::OwnerPhase};
    use std::{sync::mpsc, time::Duration};
    struct HeldModule { entered: mpsc::Sender<()>, release: mpsc::Receiver<()> }
    impl Files for HeldModule {
        fn script(&self, module: &str) -> Option<String> {
            assert_eq!(module, "hold");
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(5)).unwrap();
            Some("return {}".into())
        }
    }
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script>
      function onNote(e) require('hold'); wait(2); playNote(e.note,e.velocity) end
    </script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let (entered, waiting) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let (mut thread, _) = ScriptThread::spawn(xml.into(), HeldModule { entered, release: held }, Config {
        audit_seed: Some(42), audit_progress: true, ..Config::default()
    }).unwrap();
    let progress = thread.ui().scan_progress().unwrap();
    let initial = progress.snapshot();
    thread.set_time(5.);
    let worker = std::thread::spawn(move || {
        thread.note_on(1,60,64);
        let mut commands = Vec::new();
        thread.drain(&mut commands);
        assert!(commands.is_empty());
        assert_eq!(thread.next_due(), Some(7.));
        thread.tick(7.);
        thread.drain(&mut commands);
        assert!(matches!(&commands[..], [Command::Play(p)] if p.key==60 && p.at_ms==7.));
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    // A blocked Lua callback cannot service an owner-thread readback request.
    // These observations use only atomics and must return before release.
    let busy = progress.snapshot();
    assert_eq!(busy.phase, OwnerPhase::Events);
    assert_eq!(busy.clock_ms,5.);
    assert!(busy.vm_checkpoints > initial.vm_checkpoints);
    assert!(busy.coroutine_resumes > initial.coroutine_resumes);
    assert!(busy.work_remaining > 0 && !busy.work_exhausted);
    assert_eq!(busy.completed_barriers,0);
    release.send(()).unwrap();
    worker.join().unwrap();
    let done = progress.snapshot();
    assert_eq!((done.requested_barriers,done.completed_barriers),(2,2));
    assert_eq!((done.requested_clock_ms,done.completed_clock_ms,done.clock_ms),(7.,7.,7.));
    assert!(done.vm_checkpoints >= busy.vm_checkpoints);
    assert!(done.coroutine_resumes > busy.coroutine_resumes);
    println!("synthetic owner busy={busy:?}; complete={done:?}");
}

#[test]
fn owner_progress_reports_exhaustion_without_refilling_work() {
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script>
      function onNote(e) while true do end end
    </script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let mut host = ScriptHost::new(xml, (), Config {
        audit_seed: Some(42), audit_progress: true, callback_work: 16, ..Config::default()
    }).unwrap();
    host.note_on(1,60,64,0);
    let progress = host.scan_progress().unwrap();
    let stopped = progress.snapshot();
    assert_eq!(stopped.work_remaining,0);
    assert!(stopped.work_exhausted);
    host.interface();
    let inspected = progress.snapshot();
    assert_eq!(inspected.work_remaining,0);
    assert!(inspected.work_exhausted);
    let ordinary = ScriptHost::new(xml, (), Config {
        audit_progress: true, ..Config::default()
    }).unwrap();
    assert!(ordinary.scan_progress().is_none(), "unseeded hosts never collect progress");
}
