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
