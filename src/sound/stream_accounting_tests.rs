use super::Stream;

#[test]
fn auto_streaming_preloads_authored_attacks_before_publication() {
    use super::{CoreLoader, LoadRequest, Streaming};
    use super::v2::{V2Loader, V2Core};
    let temp = tempfile::tempdir().unwrap();
    let wav = temp.path().join("attack.wav");
    let mut writer = hound::WavWriter::create(&wav, hound::WavSpec {
        channels: 1, sample_rate: 48000, bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    }).unwrap();
    for _ in 0..48000 { writer.write_sample(16384_i16).unwrap(); }
    writer.finalize().unwrap();
    let program = temp.path().join("attack.uvip");
    std::fs::write(&program, r#"<UVI4><Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="attack.wav" BaseNote="60"><PlaybackOptions Start="0" Stop="48000"/></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program></UVI4>"#).unwrap();
    let loaded = V2Loader.prepare(&LoadRequest {
        path: program, sample_rate: 48000.0, streaming: Streaming::Auto,
        ..Default::default()
    }, &mut |_| {}, &|| false).unwrap();
    let stream = loaded.stream.as_ref().unwrap();
    let readiness = stream.streamer.head_readiness(&stream.assets);
    assert!(readiness.ranges > 0);
    assert!(readiness.complete(), "{readiness:?}");
    assert!(readiness.packed_bytes > 0);
    assert_eq!(readiness.budget_bytes, 256 << 20);
    assert!(readiness.packed_bytes <= readiness.budget_bytes);
    assert!(loaded.report.stream_startup.as_ref().unwrap().warning().is_none());
    assert!(stream.assets.iter().all(|pcm| pcm.head_bytes() > 0));
    let mut core = V2Core::with_parts(1, 48000.0);
    use super::Core;
    use super::event::{Event, HostNote};
    core.install(0, loaded.part);
    core.event(0, Event::NoteOn { note: HostNote {
        port: 0, channel: 0, key: 60, id: 42, clap: true,
    }, velocity: 0.8, tune: 0.0 });
    let output = core.render(64);
    assert!(output.buses.iter().flat_map(|bus| bus.iter()).flatten().any(|v| v.abs() > 0.01),
        "first callback must contain the authored attack");
    assert_eq!(core.problems(0).cold_starts, 0);
    assert_eq!(core.problems(0).underruns, 0);
}

#[test]
fn stream_memory_includes_staged_and_audio_pinned_retired_head_generations() {
    let temp = tempfile::tempdir().unwrap();
    let wav = temp.path().join("sample.wav");
    let mut writer = hound::WavWriter::create(
        &wav,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..64 {
        writer.write_sample(16384_i16).unwrap();
    }
    writer.finalize().unwrap();
    let program = temp.path().join("memory.uvip");
    std::fs::write(&program, r#"<UVI4><Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="sample.wav" BaseNote="60"><PlaybackOptions Start="0" Stop="64"/></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program></UVI4>"#).unwrap();
    let streamed = sampler_uvi::load_streamed(
        &program,
        48000,
        &sampler_kontakt::StreamPolicy {
            lazy: true,
            head_budget: 32,
            voices: 1,
            decoders: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let stream = Stream {
        streamer: streamed.streamer,
        assets: streamed.assets,
        report: streamed.report,
    };
    let pcm = &stream.assets[0];
    let pool = stream.report.pool_bytes as u64;
    assert_eq!(stream.resident_bytes(), pool);
    pcm.set_ranges(vec![(0, vec![[0.5; 2]; 8].into_boxed_slice())])
        .unwrap();
    assert_eq!(stream.resident_bytes(), pool + 16);
    let retired = pcm.set_ranges(Vec::new()).unwrap();
    assert_eq!(pcm.head_bytes(), 0);
    assert_eq!(
        stream.resident_bytes(),
        pool + 16,
        "an audio-pinned old generation is still physically held"
    );
    let staged = pcm
        .prepare_ranges(vec![(16, vec![[0.5; 2]; 8].into_boxed_slice())])
        .unwrap();
    assert_eq!(
        stream.resident_bytes(),
        pool + 32,
        "inert prepared payloads also consume the quota"
    );
    assert_eq!(crate::plugin::tests::allocations(|| drop(retired)), 0);
    assert_eq!(
        stream.streamer.head_budget_status().held_bytes,
        32,
        "audio guard release cannot destroy its payload"
    );
    assert_eq!(
        stream.resident_bytes(),
        pool + 16,
        "control collection releases the retired payload"
    );
    drop(staged);
    assert_eq!(stream.resident_bytes(), pool);
}

#[test]
fn uvi_cancellation_before_resolve_and_at_assembly_remains_canceled() {
    use super::{CoreLoader, CoreError, LoadRequest};
    use super::v2::V2Loader;
    use std::sync::atomic::{AtomicBool, Ordering};
    let temp = tempfile::tempdir().unwrap();
    let program = temp.path().join("cancel.uvip");
    std::fs::write(&program, r#"<UVI4><Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="missing.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program></UVI4>"#).unwrap();
    for stage in [0, 300, 600] {
        let canceled = AtomicBool::new(stage == 0);
        let result = V2Loader.prepare(&LoadRequest {
            path: program.clone(), sample_rate: 48000., ..Default::default()
        }, &mut |p| { if p.0 >= stage { canceled.store(true, Ordering::Relaxed); } },
            &|| canceled.load(Ordering::Relaxed));
        assert!(matches!(result, Err(CoreError::Canceled)), "stage {stage}");
    }
}

#[test]
fn initial_required_full_pcm_is_charged_once_in_product_memory() {
    let temp = tempfile::tempdir().unwrap();
    let wav = temp.path().join("resident.wav");
    let mut writer = hound::WavWriter::create(&wav, hound::WavSpec {
        channels: 1, sample_rate: 48000, bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    }).unwrap();
    for _ in 0..64 { writer.write_sample(16384_i16).unwrap(); }
    writer.finalize().unwrap();
    let program = temp.path().join("resident.uvip");
    std::fs::write(&program, r#"<UVI4><Program Streaming="0"><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="resident.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program></UVI4>"#).unwrap();
    let streamed = sampler_uvi::load_streamed(&program, 48000, &sampler_kontakt::StreamPolicy {
        lazy: true, head_budget: 512, voices: 1, decoders: 1, ..Default::default()
    }).unwrap();
    let stream = Stream {streamer:streamed.streamer, assets:streamed.assets, report:streamed.report};
    assert_eq!(stream.report.required_resident_bytes, 512);
    let startup = super::report::StreamStartup::of(&stream.streamer, &stream.assets, &stream.report);
    assert_eq!(startup.required_resident_assets, 1);
    assert_eq!(startup.required_resident_bytes, 512);
    assert_eq!(startup.packed_head_bytes, 0);
    assert_eq!(startup.physical_payload_bytes, 512);
    assert_eq!(stream.resident_bytes(), stream.report.pool_bytes as u64 + 512);
    assert_eq!(stream.assets[0].resident_frames().unwrap().len(), 64);
}
