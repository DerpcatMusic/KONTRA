//! The production attached-thread + dormant recipe + disk-streamed source path.
use sampler_core::{Input, Limits, Protocol, Runtime, SampleTargetId};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "kontra-dynamic-stream-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn wav(frames: usize) -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0".to_vec();
    bytes.extend(48000u32.to_le_bytes());
    bytes.extend(96000u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend((frames as u32 * 2).to_le_bytes());
    for i in 0..frames {
        bytes.extend((((i * 37) % 8000) as i16 - 4000).to_le_bytes());
    }
    let extent = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&extent.to_le_bytes());
    bytes
}
fn input() -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(1),
    }
}
#[test]
fn parked_owner_completes_a_real_dormant_stream_load_and_pages_match_resident_render() {
    let dir = Scratch::new();
    let frames = 40000;
    std::fs::write(dir.0.join("tone.wav"), wav(frames)).unwrap();
    let script = r#"<EventProcessors><ScriptProcessor><script><![CDATA[
      Ready=OnOffButton('Ready',false)
      function onInit()
        local osc=Program.layers[1].keygroups[1].oscillators[1]
        task=loadSample(osc,'tone.wav',function(t)
          assert(rawequal(task,t) and t.finished and t.success)
          assert(osc.sampleInfo.samplerate==48000)
          Ready:setValue(true)
        end)
      end
    ]]></script></ScriptProcessor></EventProcessors>"#;
    let content = |sample: &str, scripts: &str| {
        format!(
            "<UVI4><Program>{scripts}<Layers><Layer><Keygroups><Keygroup LowKey='60' HighKey='60'><Oscillators><SamplePlayer {sample} BaseNote='60' NoteTracking='0'/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program></UVI4>"
        )
    };
    let xml = content("", script);
    let path = dir.0.join("Dynamic.uvip");
    std::fs::write(&path, &xml).unwrap();
    let mut translated = sampler_uvi::translate_path(&path).unwrap();
    let attached = translated
        .attach_script(48000, Default::default())
        .unwrap()
        .unwrap();
    let target = SampleTargetId(translated.instrument.source_indices.sample_targets[0].node);
    let streamed = sampler_uvi::assemble_translated_streamed(
        translated,
        48000,
        &sampler_kontakt::StreamPolicy {
            voices: 8,
            decoders: 2,
            head_budget: 1 << 20,
            ..Default::default()
        },
    )
    .unwrap();
    let sampler_kontakt::Streamed {
        loaded,
        cache,
        streamer,
        ..
    } = streamed;
    assert_eq!(
        loaded.plan.sample_count(),
        0,
        "no fabricated initial sample"
    );
    let lim = Limits::for_plan(&loaded.plan, 8, 8);
    let (rt, control) = Runtime::new(loaded.plan, lim)
        .unwrap()
        .with_stream_cache(cache)
        .with_sample_updates(4)
        .unwrap();
    let mut rt = rt;
    let mut driver = attached.driver;
    driver
        .bind_sample_stream(streamer.dynamic_sources())
        .unwrap();
    driver.bind_sample_control(control).unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        rt.poll_sample_update().unwrap();
        if rt.sample_target_asset(target).is_ok()
            && driver.ui().values().iter().any(|(_, v)| *v == 1.)
        {
            break;
        }
        assert!(
            Instant::now() < until,
            "parked owner did not finish source task: {:?}",
            driver.ui().fault_counts()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let pcm = rt.sample_target_asset(target).unwrap();
    assert!(
        pcm.resident_frames().is_none(),
        "dynamic source must remain streamed"
    );
    assert_eq!((pcm.sample_rate(), pcm.frame_count()), (48000, frames));
    assert!(
        pcm.head_bytes() > 0 && pcm.head_bytes() < frames * 8 / 2,
        "only bounded source heads are resident"
    );
    let static_path = dir.0.join("Static.uvip");
    std::fs::write(&static_path, content("SamplePath='tone.wav'", "")).unwrap();
    let reference = sampler_uvi::load(&static_path, 48000).unwrap();
    let lim = Limits::for_plan(&reference.plan, 8, 8);
    let mut reference = Runtime::new(reference.plan, lim).unwrap();
    rt.trigger(input(), 60, 1.).unwrap();
    reference.trigger(input(), 60, 1.).unwrap();
    rt.set_offline(true);
    let mut actual = [[0.; 2]; 256];
    let mut expected = [[0.; 2]; 256];
    for _ in 0..frames.div_ceil(256) {
        rt.wait_streaming(8192, Duration::from_secs(2)).unwrap();
        rt.render(&mut actual).unwrap();
        reference.render(&mut expected).unwrap();
        assert_eq!(
            actual.map(|f| f.map(f32::to_bits)),
            expected.map(|f| f.map(f32::to_bits))
        );
    }
    assert_eq!(rt.stats().stream_underruns, 0);
    assert!(
        driver.ui().fault_counts().runtime.is_empty(),
        "{:?}",
        driver.ui().fault_counts()
    );
}

#[test]
fn authored_memory_requirements_keep_dynamic_source_body_resident_with_stream_service_bound() {
    let dir = Scratch::new();
    let frames = 40000;
    std::fs::write(dir.0.join("tone.wav"), wav(frames)).unwrap();
    for (program, oscillator) in [
        ("Streaming='1'", "AllowStreaming='0'"),
        ("Streaming='0'", "AllowStreaming='1'"),
    ] {
        let path = dir.0.join("Memory.uvip");
        std::fs::write(&path,format!(r#"<UVI4><Program {program}><EventProcessors><ScriptProcessor><script><![CDATA[
            Ready=OnOffButton('Ready',false)
            function onInit()
                local osc=Program.layers[1].keygroups[1].oscillators[1]
                task=loadSample(osc,'tone.wav',function(t)
                    assert(rawequal(task,t) and t.finished and t.success)
                    Ready:setValue(true)
                end)
            end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer><Keygroups><Keygroup LowKey='60' HighKey='60'><Oscillators><SamplePlayer {oscillator} BaseNote='60' NoteTracking='0'/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program></UVI4>"#)).unwrap();
        let mut translated = sampler_uvi::translate_path(&path).unwrap();
        let attached = translated
            .attach_script(48000, Default::default())
            .unwrap()
            .unwrap();
        let target = SampleTargetId(translated.instrument.source_indices.sample_targets[0].node);
        let streamed = sampler_uvi::assemble_translated_streamed(
            translated,
            48000,
            &sampler_kontakt::StreamPolicy {
                voices: 4,
                head_budget: 1 << 20,
                ..Default::default()
            },
        )
        .unwrap();
        let limits = Limits::for_plan(&streamed.loaded.plan, 4, 4);
        let (mut rt, control) = Runtime::new(streamed.loaded.plan, limits)
            .unwrap()
            .with_stream_cache(streamed.cache)
            .with_sample_updates(4)
            .unwrap();
        let mut driver = attached.driver;
        driver
            .bind_sample_stream(streamed.streamer.dynamic_sources())
            .unwrap();
        driver.bind_sample_control(control).unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            rt.poll_sample_update().unwrap();
            let status = driver.sample_load_status().unwrap();
            if status.successful == 1
                && status.pending == 0
                && driver.ui().values().iter().any(|(_, v)| *v == 1.)
            {
                break;
            }
            assert!(
                Instant::now() < until,
                "authored resident task failed: {status:?}"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let pcm = rt.sample_target_asset(target).unwrap();
        assert_eq!(pcm.head_bytes(), 0);
        let full = pcm
            .resident_frames()
            .expect("authored RAM law cannot silently stream tails");
        for (i, frame) in full.iter().enumerate() {
            let expected = (((i * 37) % 8000) as i16 - 4000) as f32 / 32768.;
            assert_eq!(frame.map(f32::to_bits), [expected; 2].map(f32::to_bits));
        }
        assert_eq!(
            rt.sample_physical_budget_status().unwrap().held_bytes,
            frames * 8
        );
        drop(full);
        rt.trigger(input(), 60, 1.).unwrap();
        let mut audio = [[0.; 2]; 256];
        for _ in 0..frames.div_ceil(256) {
            rt.render(&mut audio).unwrap();
        }
        assert_eq!(
            rt.stats().stream_underruns,
            0,
            "no page readiness wait was used for memory playback"
        );
        assert!(driver.ui().fault_counts().runtime.is_empty());
    }
}
