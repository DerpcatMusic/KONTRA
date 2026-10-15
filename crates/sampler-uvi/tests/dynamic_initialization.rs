//! Native initialization queues metadata before the production control and
//! streamed-source service binds. Every success below follows a real commit.
use sampler_core::{Limits, Runtime, SampleTargetId};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "kontra-init-samples-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn wav() -> Vec<u8> {
    let mut b = b"RIFF".to_vec();
    b.extend(52u32.to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(48000u32.to_le_bytes());
    b.extend(96000u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(16u32.to_le_bytes());
    for i in 1..=8i16 {
        b.extend((i * 1024).to_le_bytes());
    }
    b
}
fn run(targets: usize, requests: usize, supersede: bool) {
    let dir = Scratch::new();
    std::fs::write(dir.0.join("tiny.wav"), wav()).unwrap();
    let script = format!(
        r#"<EventProcessors><ScriptProcessor><script><![CDATA[
        Ready=OnOffButton('Ready',false)
        local completed,cancelled=0,0
        function onInit()
          for i=1,{requests} do
            local target=Program.layers[1].keygroups[{target_expr}].oscillators[1]
            local task
            task=loadSample(target,'tiny.wav',function(t)
              assert(rawequal(t,task) and t.finished)
              if t.state=='cancelled' then cancelled=cancelled+1
              else assert(t.success); completed=completed+1 end
              if completed=={successes} and cancelled=={cancelled} then Ready:setValue(true) end
            end)
          end
        end
    ]]></script></ScriptProcessor></EventProcessors>"#,
        target_expr = if supersede { "1" } else { "i" },
        successes = if supersede { 1 } else { requests },
        cancelled = if supersede { requests - 1 } else { 0 }
    );
    let groups="<Keygroup LowKey='60' HighKey='60'><Oscillators><SamplePlayer BaseNote='60' NoteTracking='0'/></Oscillators></Keygroup>".repeat(targets);
    let xml = format!(
        "<UVI4><Program>{script}<Layers><Layer><Keygroups>{groups}</Keygroups></Layer></Layers></Program></UVI4>"
    );
    let path = dir.0.join("Queued.uvip");
    std::fs::write(&path, xml).unwrap();
    let mut translated = sampler_uvi::translate_path(&path).unwrap();
    let ids: Vec<_> = translated
        .instrument
        .source_indices
        .sample_targets
        .iter()
        .map(|t| SampleTargetId(t.node))
        .collect();
    assert_eq!(ids.len(), targets);
    let mut attached = translated
        .attach_script(48000, Default::default())
        .unwrap()
        .unwrap();
    assert!(
        attached.driver.ui().fault_counts().runtime.is_empty(),
        "initial metadata requests must not fail before the service binds: {:?}",
        attached.driver.ui().fault_counts()
    );
    let before = attached.driver.sample_load_status().unwrap();
    assert_eq!(before.requested, requests);
    assert_eq!(before.successful, 0);
    assert_eq!(before.failed, 0);
    assert_eq!(before.cancelled, if supersede { requests - 1 } else { 0 });
    assert_eq!(before.pending, targets);
    let streamed = sampler_uvi::assemble_translated_streamed(
        translated,
        48000,
        &sampler_kontakt::StreamPolicy {
            voices: 8,
            decoders: 1,
            head_budget: 1024,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        streamed.loaded.plan.sample_count(),
        0,
        "no fabricated initial sources"
    );
    let limits = Limits::for_plan(&streamed.loaded.plan, 8, 8);
    let (mut runtime, control) = Runtime::new(streamed.loaded.plan, limits)
        .unwrap()
        .with_stream_cache(streamed.cache)
        .with_sample_updates(64)
        .unwrap();
    let mut driver = attached.driver;
    driver
        .bind_sample_stream(streamed.streamer.dynamic_sources())
        .unwrap();
    driver.bind_sample_control(control).unwrap();
    let until = Instant::now() + Duration::from_secs(30);
    while !driver.ui().values().iter().any(|(_, v)| *v == 1.) {
        runtime.poll_sample_update().unwrap();
        assert!(
            Instant::now() < until,
            "not every initialized task reached its callback after commit: {:?}",
            driver.ui().fault_counts()
        );
        std::thread::sleep(Duration::from_micros(100));
    }
    for target in ids {
        let pcm = runtime.sample_target_asset(target).unwrap();
        assert_eq!(
            (pcm.sample_rate(), pcm.frame_count(), pcm.head_bytes()),
            (48000, 8, 16)
        );
        assert!(pcm.resident_frames().is_none());
    }
    let tasks = driver.sample_load_status().unwrap();
    assert_eq!(tasks.requested, requests);
    assert_eq!(tasks.successful, targets);
    assert_eq!(tasks.cancelled, requests - targets);
    assert_eq!((tasks.failed, tasks.pending), (0, 0));
    assert_eq!(
        tasks.requested,
        tasks.successful + tasks.failed + tasks.cancelled + tasks.pending
    );
    let budget = runtime.sample_head_budget_status().unwrap();
    assert_eq!(budget.held_bytes, targets * 16);
    assert!(budget.peak_bytes <= budget.limit_bytes);
    assert!(
        driver.ui().fault_counts().runtime.is_empty(),
        "{:?}",
        driver.ui().fault_counts()
    );
}
#[test]
fn distinct_authored_sources_queued_during_init_all_complete_after_worker_and_audio_commit() {
    run(1746, 1746, false);
}
#[test]
fn superseded_prebind_requests_preserve_every_cancelled_task_callback_and_final_commit() {
    run(1, 1024, true);
}
