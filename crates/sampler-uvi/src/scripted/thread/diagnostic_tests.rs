use super::*;
use crate::script::control_id;
use std::time::Instant;

fn xml(script: &str) -> String {
    format!(
        "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    )
}
fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "bounded diagnostic fixture timeout"
        );
        std::thread::yield_now();
    }
}
fn acknowledged(request: &mut DiagnosticRequest) -> DiagnosticAcknowledgement {
    let mut ack = None;
    until(|| match request.poll() {
        DiagnosticPoll::Pending => false,
        DiagnosticPoll::Complete(reply) => {
            ack = Some(reply);
            true
        }
        status => panic!("unexpected diagnostic terminal state {status:?}"),
    });
    ack.unwrap()
}

#[test]
fn unseeded_real_worker_ack_waits_for_callback_ui_faults_and_ordered_command_drain() {
    struct HeldModule {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    impl Files for HeldModule {
        fn script(&self, name: &str) -> Option<String> {
            assert_eq!(name, "hold");
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(3)).unwrap();
            Some("return {}".into())
        }
    }
    let (entered, waiting) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let (mut thread, loaded) = ScriptThread::spawn(xml(
        "local k=Knob('Control',0,0,127); local labels={}; for n=1,4096 do labels[n]=Label('State') end; function onNote(e) require('hold'); k:setValue(e.velocity,false); for n=1,#labels do labels[n].text='released' end; playNote(e.note,e.velocity); error('authored fault') end"
    ), HeldModule { entered, release: held }, Config::default()).unwrap();
    assert!(loaded.ui.diagnostic_progress().is_none());
    #[cfg(feature = "scan")]
    assert!(loaded.ui.scan_progress().is_none());
    loaded.ui.enable_diagnostics();
    // Hold publication itself as well as the callback, without a guessed delay.
    let face = loaded.ui.face.lock().unwrap();
    thread.note_on(1, 60, 64);
    waiting.recv_timeout(Duration::from_secs(3)).unwrap();
    let mut request = loaded
        .ui
        .request_diagnostics(Duration::from_secs(3))
        .unwrap();
    let busy = request.progress();
    assert_eq!(busy.events_enqueued, 1);
    assert_eq!(busy.events_executed, 0);
    assert_eq!(busy.owner_acknowledged, 0);
    assert_eq!(request.poll(), DiagnosticPoll::Pending);
    assert!(!request.commands_drained());
    let mut commands = Vec::new();
    thread.drain(&mut commands);
    assert!(commands.is_empty());
    release.send(()).unwrap();
    until(|| request.progress().phase == DiagnosticPhase::UiExport);
    assert_eq!(request.progress().events_executed, 1);
    assert_eq!(request.poll(), DiagnosticPoll::Pending);
    assert!(!request.commands_drained());
    drop(face);
    let ack = acknowledged(&mut request);
    assert_eq!(ack.events_executed, 1);
    assert_eq!((ack.commands_collected, ack.commands_pushed), (1, 1));
    assert_eq!(ack.runtime_faults, 1);
    assert!(ack.sample_counts_conserved());
    assert_eq!(loaded.ui.value(control_id(1, 0)), Some(64.));
    assert!(
        loaded.ui.interface().widgets[1..]
            .iter()
            .all(|w| w.text == "released")
    );
    assert!(
        !request.commands_drained(),
        "owner ack is not a command drain"
    );
    thread.drain(&mut commands);
    assert!(matches!(&commands[..], [Command::Play(p)] if p.key == 60));
    assert!(request.commands_drained());
    assert_eq!(request.progress().commands_drained, 1);
}

#[test]
fn request_during_backpressure_waits_for_next_turns_events_and_ui() {
    let (mut thread, loaded) = ScriptThread::spawn(xml(
        "local label=Label('State'); function onNote(e) if e.note==60 then for n=1,1100 do playNote(60,64) end else label.text='later event'; playNote(61,64) end end"
    ), (), Config::default()).unwrap();
    loaded.ui.enable_diagnostics();
    thread.note_on(1, 60, 64);
    until(|| loaded.ui.diagnostic_progress().unwrap().commands_pushed == QUEUE as u64);
    thread.note_on(2, 61, 64);
    let mut request = loaded
        .ui
        .request_diagnostics(Duration::from_secs(3))
        .unwrap();
    assert_eq!(request.poll(), DiagnosticPoll::Pending);
    let mut commands = Vec::new();
    let mut ack = None;
    until(|| {
        thread.drain(&mut commands);
        match request.poll() {
            DiagnosticPoll::Pending => false,
            DiagnosticPoll::Complete(reply) => {
                ack = Some(reply);
                true
            }
            status => panic!("backpressure observation {status:?}"),
        }
    });
    let ack = ack.unwrap();
    assert_eq!(ack.events_executed, 2);
    assert_eq!(ack.commands_collected, 1101);
    assert_eq!(loaded.ui.interface().widgets[0].text, "later event");
    until(|| {
        thread.drain(&mut commands);
        request.commands_drained()
    });
    assert_eq!(commands.len(), 1101);
    assert!(matches!(commands.last(), Some(Command::Play(p)) if p.key == 61));
}

#[test]
fn acknowledgement_reports_due_work_without_advancing_the_audio_clock() {
    let (mut thread, loaded) = ScriptThread::spawn(
        xml("function onNote(e) spawn(function() wait(2); playNote(e.note,e.velocity) end) end"),
        (),
        Config::default(),
    )
    .unwrap();
    loaded.ui.enable_diagnostics();
    thread.note_on(1, 60, 64);
    let mut request = loaded
        .ui
        .request_diagnostics(Duration::from_secs(3))
        .unwrap();
    let first = acknowledged(&mut request);
    assert_eq!(first.next_due_ms, Some(2.));
    assert_eq!(first.commands_pushed, 0);
    let mut commands = Vec::new();
    thread.drain(&mut commands);
    assert!(commands.is_empty());
    thread.tick(2.);
    let mut request = loaded
        .ui
        .request_diagnostics(Duration::from_secs(3))
        .unwrap();
    let second = acknowledged(&mut request);
    assert_eq!(second.next_due_ms, None);
    assert_eq!(second.commands_pushed, 1);
    thread.drain(&mut commands);
    assert!(matches!(&commands[..], [Command::Play(p)] if p.at_ms == 2.));
    assert!(request.commands_drained());
}

#[test]
fn requests_are_bounded_cancel_timeout_and_disconnect_without_blocking() {
    let (mut thread, loaded) = ScriptThread::spawn(
        xml("local k=Knob('Control',0,0,1); k.changed=function() playNote(60,64) end"),
        (),
        Config::default(),
    )
    .unwrap();
    loaded.ui.enable_diagnostics();
    let face = loaded.ui.face.lock().unwrap();
    assert!(loaded.ui.edit(control_id(1, 0), 1.));
    until(|| loaded.ui.diagnostic_progress().unwrap().phase == DiagnosticPhase::UiExport);
    let mut cancelled = loaded
        .ui
        .request_diagnostics(Duration::from_secs(3))
        .unwrap();
    assert!(matches!(
        loaded.ui.request_diagnostics(Duration::from_secs(3)),
        Err(DiagnosticRequestError::Busy)
    ));
    assert_eq!(cancelled.cancel(), DiagnosticPoll::Cancelled);
    assert_eq!(cancelled.poll(), DiagnosticPoll::Cancelled);
    let mut expired = loaded
        .ui
        .request_diagnostics(Duration::from_nanos(1))
        .unwrap();
    until(|| expired.poll() == DiagnosticPoll::TimedOut);
    assert_eq!(expired.poll(), DiagnosticPoll::TimedOut);
    assert_eq!(expired.poll(), DiagnosticPoll::TimedOut);
    for _ in 0..300 {
        match loaded.ui.request_diagnostics(Duration::from_secs(3)) {
            Ok(request) => drop(request),
            Err(DiagnosticRequestError::QueueFull) => break,
            Err(error) => panic!("unexpected capacity error {error:?}"),
        }
    }
    assert!(matches!(
        loaded.ui.request_diagnostics(Duration::from_secs(3)),
        Err(DiagnosticRequestError::QueueFull)
    ));
    assert!(matches!(
        loaded.ui.request_diagnostics(Duration::ZERO),
        Err(DiagnosticRequestError::InvalidTimeout)
    ));
    drop(face);
    // Consume the authored command and let the owner retire cancelled requests.
    let mut commands = Vec::new();
    until(|| {
        thread.drain(&mut commands);
        !commands.is_empty()
    });
    // A parked phase is an independent observation, not a queue-empty fence:
    // it may precede the turn which consumes the abandoned requests above.
    // Enqueue behind them and observe the real owner/command FIFO boundary.
    let mut retirement = None;
    until(|| match loaded.ui.request_diagnostics(Duration::from_secs(3)) {
        Ok(request) => {
            retirement = Some(request);
            true
        }
        Err(DiagnosticRequestError::QueueFull) => false,
        Err(error) => panic!("unexpected retirement barrier error {error:?}"),
    });
    let mut retirement = retirement.unwrap();
    let ack = acknowledged(&mut retirement);
    assert_eq!((ack.commands_collected, ack.commands_pushed), (1, 1));
    thread.drain(&mut commands);
    assert!(retirement.commands_drained());
    assert_eq!(commands.len(), 1);
    assert_eq!(loaded.ui.value(control_id(1, 0)), Some(1.));
    let face = loaded.ui.face.lock().unwrap();
    assert!(loaded.ui.edit(control_id(1, 0), 0.));
    until(|| loaded.ui.diagnostic_progress().unwrap().phase == DiagnosticPhase::UiExport);
    let mut stopped = loaded
        .ui
        .request_diagnostics(Duration::from_secs(3))
        .unwrap();
    thread.stop.store(true, Ordering::Release);
    thread.wake();
    drop(face);
    drop(thread);
    // The request was queued behind held publication, never executed. Retaining
    // its bridge cannot turn a stopped owner into a successful acknowledgement.
    assert_eq!(stopped.poll(), DiagnosticPoll::OwnerStopped);
    assert!(matches!(
        loaded.ui.request_diagnostics(Duration::from_secs(3)),
        Err(DiagnosticRequestError::OwnerStopped)
    ));
}

#[cfg(feature = "scan")]
#[test]
fn seeded_transport_cannot_impersonate_an_ordinary_owner_acknowledgement() {
    let (_, loaded) = ScriptThread::spawn(
        xml("function onNote(e) playNote(e.note,e.velocity) end"),
        (),
        Config {
            audit_seed: Some(42),
            ..Config::default()
        },
    )
    .unwrap();
    assert!(matches!(
        loaded.ui.request_diagnostics(Duration::from_secs(3)),
        Err(DiagnosticRequestError::OwnerStopped | DiagnosticRequestError::SeededOwner)
    ));
}

#[test]
fn real_sample_worker_status_ack_requires_actual_core_commit_and_preserves_failed_cancelled() {
    use crate::script::{SampleResource, SampleSource};
    use sampler_core::{
        Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime, SampleRevisionLimits,
        SampleTarget, SampleTargetId,
    };
    struct Sources {
        entered: mpsc::Sender<()>,
        release: Arc<Mutex<mpsc::Receiver<()>>>,
    }
    impl Files for Sources {
        fn script(&self, _: &str) -> Option<String> {
            None
        }
        fn sample_source(&self) -> Option<SampleSource> {
            let entered = self.entered.clone();
            let release = self.release.clone();
            Some(Arc::new(move |request, _| {
                if request.path == "missing" {
                    return Err("authored missing source".into());
                }
                entered.send(()).unwrap();
                release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap();
                struct Source;
                impl sampler_kontakt::AssetSource for Source {
                    fn open(&self) -> std::io::Result<sampler_kontakt::SampleReader> {
                        Ok(sampler_kontakt::SampleReader::custom(
                            48000,
                            256,
                            |_, out| {
                                out.fill([0.25; 2]);
                                Ok(())
                            },
                        ))
                    }
                }
                Ok(SampleResource {
                    source: Arc::new(Source),
                    reader: None,
                    playback: Playback::default(),
                    base_note: 60,
                    fine_tune: 0.,
                    geometry_error: None,
                    format_code: 0,
                    channels: 2,
                    bits: 32,
                    tempo: 0.,
                    timings: Default::default(),
                })
            }))
        }
    }
    let content = "<UVI4><Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer BaseNote='60'/></Oscillators></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script><![CDATA[
        local osc=Program.layers[1].keygroups[1].oscillators[1]
        local ready=OnOffButton('Ready',false)
        function onInit() task=loadSample(osc,'held',function(t) assert(t.success); ready:setValue(true) end) end
        function onNote(e)
          if e.note==61 then loadSample(osc,'missing',function(t) assert(not t.success) end)
          elseif e.note==62 then local cancelled=loadSample(osc,'cancelled',function(t) assert(not t.success) end); cancelled:cancel() end
        end
    ]]></script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let target = roxmltree::Document::parse(content)
        .unwrap()
        .descendants()
        .find(|n| n.has_tag_name("SamplePlayer"))
        .unwrap()
        .id()
        .get_usize() as u32;
    let plan = Prepared::new(
        48000,
        vec![Pcm::new(48000, vec![[0.1; 2]; 256].into()).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: Some(60),
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::new(0, 0, 0, 1., 1).unwrap(),
            playback: Playback::default(),
        }],
        1,
    )
    .unwrap()
    .with_sample_targets(
        vec![SampleTarget {
            id: SampleTargetId(target),
            regions: vec![0].into(),
        }],
        SampleRevisionLimits {
            slots: 4,
            resident_bytes: 1 << 20,
        },
    )
    .unwrap();
    let limits = Limits::for_plan(&plan, 8, 8);
    let (mut rt, control) = Runtime::new(plan, limits)
        .unwrap()
        .with_sample_updates(4)
        .unwrap();
    let (entered, waiting) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let (mut thread, loaded) = ScriptThread::spawn(
        content.into(),
        Sources {
            entered,
            release: Arc::new(Mutex::new(held)),
        },
        Config::default(),
    )
    .unwrap();
    fn observe(thread: &mut ScriptThread) -> DiagnosticAcknowledgement {
        let mut request = thread
            .ui()
            .request_diagnostics(Duration::from_secs(3))
            .unwrap();
        let ack = acknowledged(&mut request);
        assert!(ack.sample_counts_conserved());
        thread.drain(&mut Vec::new());
        assert!(request.commands_drained());
        ack
    }
    let queued = observe(&mut thread).samples;
    assert_eq!(
        (queued.requested, queued.queued_metadata, queued.successful),
        (1, 1, 0)
    );
    thread.bind_sample_control(control).unwrap();
    waiting.recv_timeout(Duration::from_secs(3)).unwrap();
    let active = observe(&mut thread).samples;
    assert_eq!(
        (active.worker_active, active.pending, active.successful),
        (1, 1, 0)
    );
    release.send(()).unwrap();
    let mut submitted = None;
    until(|| {
        let s = observe(&mut thread).samples;
        if s.submitted == 1 {
            submitted = Some(s);
            true
        } else {
            false
        }
    });
    assert_eq!(
        submitted.unwrap().successful,
        0,
        "decode/submit is not a core commit"
    );
    // A collection transaction may precede the submitted source replacement.
    // Service the bounded real control queue, rather than assume one poll is it.
    for _ in 0..8 {
        if !rt.poll_sample_update().unwrap() {
            break;
        }
    }
    until(|| observe(&mut thread).samples.successful == 1);
    let committed = observe(&mut thread).samples;
    assert_eq!(
        (committed.requested, committed.successful, committed.pending),
        (1, 1, 0)
    );
    assert_eq!(
        loaded
            .ui
            .values()
            .iter()
            .map(|(_, value)| *value)
            .collect::<Vec<_>>(),
        [1.]
    );
    thread.note_on(1, 61, 64);
    until(|| observe(&mut thread).samples.failed == 1);
    thread.note_on(2, 62, 64);
    until(|| observe(&mut thread).samples.cancelled == 1);
    let final_status = observe(&mut thread).samples;
    assert_eq!(
        (
            final_status.requested,
            final_status.successful,
            final_status.failed,
            final_status.cancelled,
            final_status.pending
        ),
        (3, 1, 1, 1, 0)
    );
}
