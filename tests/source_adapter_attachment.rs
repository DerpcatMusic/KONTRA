//! Owned ordinary sources -> both adapters -> the same logical source owner.
//! Explicit core graph witnesses, never native UVI layout/processor admission.
use sampler_core::{Input, Limits, Protocol, Runtime, SourceStorage};
use sampler_ir as ir;
use std::path::PathBuf;
#[path = "../crates/sampler-core/tests/support/mod.rs"]
mod support;
const FRAMES: usize = 128;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".test-artifacts");
        std::fs::create_dir_all(&base).unwrap();
        let dir = base.join(format!(
            "source-adapter-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }
    fn program(&self, name: &str, sample: &str, options: &str) -> PathBuf {
        self.file(name,format!(r#"<Program Name='Owned source' Gain='1'><Layers><Layer Name='Original Layer' Gain='1'><Keygroups><Keygroup LowKey='60' HighKey='60'><Oscillators>
  <SamplePlayer Name='First' SamplePath='{sample}' BaseNote='60' Gain='1'>{options}</SamplePlayer>
  <SamplePlayer Name='Second' SamplePath='{sample}' BaseNote='60' Gain='1'>{options}</SamplePlayer>
  <SamplePlayer Name='Third' SamplePath='{sample}' BaseNote='60' Gain='1'>{options}</SamplePlayer>
  </Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).as_bytes())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn value(frame: usize, channel: usize) -> i16 {
    ((frame * (17 + channel * 2) + channel * 73) % 4096) as i16 - 2048
}
fn wav(channels: usize, projected: bool, looped: bool) -> Vec<u8> {
    let out_channels = if projected { 2 } else { channels };
    let width = if projected { 4 } else { 2 };
    let mut body = b"WAVEfmt ".to_vec();
    body.extend(16u32.to_le_bytes());
    body.extend((if projected { 3u16 } else { 1 }).to_le_bytes());
    body.extend((out_channels as u16).to_le_bytes());
    body.extend(48000u32.to_le_bytes());
    body.extend(((48000 * out_channels * width) as u32).to_le_bytes());
    body.extend(((out_channels * width) as u16).to_le_bytes());
    body.extend(((width * 8) as u16).to_le_bytes());
    body.extend(b"data");
    body.extend(((FRAMES * out_channels * width) as u32).to_le_bytes());
    for frame in 0..FRAMES {
        if projected {
            let x = f32::from(value(frame, channels - 1)) / 32768.;
            body.extend(x.to_le_bytes());
            body.extend((-0.5 * x).to_le_bytes());
        } else {
            for channel in 0..channels {
                body.extend(value(frame, channel).to_le_bytes());
            }
        }
    }
    if looped {
        let mut smpl = vec![0; 60];
        smpl[12..16].copy_from_slice(&67u32.to_le_bytes());
        smpl[28..32].copy_from_slice(&1u32.to_le_bytes());
        smpl[44..48].copy_from_slice(&16u32.to_le_bytes());
        smpl[48..52].copy_from_slice(&95u32.to_le_bytes());
        body.extend(b"smpl");
        body.extend(60u32.to_le_bytes());
        body.extend(smpl);
    }
    let mut out = b"RIFF".to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}
fn verify(source: &SourceStorage, channels: usize) {
    let v = source.as_source();
    assert_eq!((v.sample_rate(), v.frame_count()), (48000, FRAMES));
    assert_eq!(v.channels().get(), channels.max(2));
    let view = v.resident_view().unwrap();
    for frame in 0..FRAMES {
        for channel in 0..channels.max(2) {
            let original = if channels == 1 { 0 } else { channel };
            assert_eq!(
                view.sample(channel, frame).unwrap().to_bits(),
                (f32::from(value(frame, original)) / 32768.).to_bits()
            );
        }
    }
    let mut ids = Vec::new();
    v.physical_dependencies(|p| {
        assert!(!ids.contains(&p.asset_id()));
        ids.push(p.asset_id());
    });
    assert_eq!(ids.len(), channels.div_ceil(2));
    assert_eq!(v.stereo().is_some(), channels <= 2);
}
#[test]
fn both_normal_resident_readers_retain_all_original_ordinals_and_physical_assets() {
    let f = Fixture::new();
    for channels in [1, 2, 3, 5, 8] {
        let path = f.file(
            &format!("owned-{channels}.wav"),
            &wav(channels, false, false),
        );
        let mut samples = sampler_kontakt::Samples::new(&f.0);
        let resolved = samples
            .resolve(&f.0, path.file_name().unwrap().to_str().unwrap())
            .unwrap()
            .unwrap();
        let kontakt = samples.decode_source(&resolved).unwrap();
        let uvi = sampler_uvi::decode_source(&path).unwrap();
        verify(&kontakt, channels);
        verify(&uvi, channels);
        if channels <= 2 {
            let old = samples.decode(&resolved).unwrap();
            let old_uvi = sampler_uvi::decode_sample(&path).unwrap();
            for (new, old) in [(&kontakt, old), (&uvi, old_uvi)] {
                let pcm = new.as_source().stereo().unwrap();
                assert_eq!(&*pcm.resident_frames().unwrap(), &old.frames);
            }
        }
    }
}
fn graph(channels: usize, members: usize) -> ir::ProcessingScope {
    let mut cells = vec![0.; channels * 2];
    cells[channels - 1] = 1.;
    cells[channels * 2 - 1] = -0.5;
    ir::ProcessingScope {
        members: (0..members).map(ir::ZoneRef).collect(),
        sum: ir::ProcessingSumLaw::OrderedF32,
        cadence: ir::ProcessingCadence::CoreCell64,
        chain: None,
        amplitude: None,
        output: None,
        signal: Some(ir::ProcessingSignal {
            source_law: ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,
            input: ir::SignalLayout { channels },
            nodes: vec![ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix {
                input: ir::SignalLayout { channels },
                output: ir::SignalLayout { channels: 2 },
                cells,
                arithmetic: ir::MatrixArithmetic::SeparateMulAddF32,
            })],
            controls: vec![],
        }),
    }
}
fn runtime(loaded: sampler_kontakt::Loaded) -> Runtime {
    let limits = Limits::for_plan(&loaded.plan, 8, 16);
    Runtime::new(loaded.plan, limits).unwrap()
}
fn note(rt: &mut Runtime) {
    rt.trigger(
        Input {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
            key: 60,
            external_id: Some(901),
        },
        60,
        1.,
    )
    .unwrap();
}
fn loop_range(zone: &ir::Zone) -> (u64, u64) {
    match zone.playback.looping {
        ir::Looping::Continuous(r) => (r.start, r.end),
        other => panic!("missing file playback: {other:?}"),
    }
}
#[test]
fn normal_uvi_loader_with_explicit_core_graph_retains_three_players_groups_targets_and_file_loop() {
    let f = Fixture::new();
    for channels in [3, 5, 8] {
        f.file("wide.wav", &wav(channels, false, true));
        f.file("reference.wav", &wav(channels, true, true));
        let input = f.program("wide.uvip", "wide.wav", "");
        let reference = f.program("reference.uvip", "reference.wav", "");
        let mut t = sampler_uvi::translate_path(&input).unwrap();
        assert_eq!(t.instrument.zones.len(), 3);
        assert_eq!(t.instrument.assets.len(), 1);
        let groups = t.instrument.groups.len();
        let homes: Vec<_> = t.instrument.zones.iter().map(|z| z.group).collect();
        let targets: Vec<_> = t
            .instrument
            .source_indices
            .sample_targets
            .iter()
            .map(|t| (t.node, t.zones.clone()))
            .collect();
        assert_eq!(targets.len(), 3);
        // Application-authored generic core contract, not a native auto-admission.
        // Original groups/targets/source identities are deliberately NOT removed.
        t.instrument.processing_scopes.push(graph(channels, 3));
        let loaded = sampler_uvi::assemble_translated(t, 48000).unwrap();
        assert_eq!(loaded.instrument.zones.len(), 3);
        assert_eq!(loaded.instrument.groups.len(), groups);
        assert_eq!(
            loaded
                .instrument
                .zones
                .iter()
                .map(|z| z.group)
                .collect::<Vec<_>>(),
            homes
        );
        assert_eq!(
            loaded
                .instrument
                .source_indices
                .sample_targets
                .iter()
                .map(|t| (t.node, t.zones.clone()))
                .collect::<Vec<_>>(),
            targets
        );
        for zone in &loaded.instrument.zones {
            assert_eq!(zone.pitch, ir::KeyTracking::Tracked { root: 60 });
            assert_eq!(zone.tune.semitones(), 0.);
            assert_eq!(loop_range(zone), (16, 96));
        }
        let retained = loaded.plan.source_storage(0).unwrap();
        assert_eq!(retained.channels().get(), channels);
        assert!(retained.stereo().is_none());
        assert_eq!(retained.frame_count(), FRAMES);
        let mut a = runtime(loaded);
        let mut b = runtime(sampler_uvi::load(&reference, 48000).unwrap());
        note(&mut a);
        note(&mut b);
        let mut x = [[0.; 2]; 64];
        let mut y = x;
        let mut energy = 0f64;
        support::without_heap(|| {
            for _ in 0..16 {
                a.render(&mut x).unwrap();
                b.render(&mut y).unwrap();
                for (x, y) in x.iter().zip(y) {
                    for c in 0..2 {
                        assert!((x[c] - y[c]).abs() <= 2e-6);
                        energy += f64::from(x[c]).powi(2);
                    }
                }
            }
        });
        assert!(energy > 0.01);
        assert_eq!((a.voice_count(), a.processing_owner_count()), (3, 1));
        assert_eq!((a.nonfinite_frames(), a.stream_underruns()), (0, 0));
    }
}
#[test]
fn stored_empty_playback_keeps_original_root_and_disables_file_loop_in_normal_stereo_control() {
    let f = Fixture::new();
    f.file("stereo.wav", &wav(2, false, true));
    let path = f.program("explicit.uvip", "stereo.wav", "<PlaybackOptions/>");
    let loaded = sampler_uvi::load(&path, 48000).unwrap();
    for z in &loaded.instrument.zones {
        assert_eq!(z.playback.looping, ir::Looping::None);
        assert_eq!(z.pitch, ir::KeyTracking::Tracked { root: 60 });
    }
}
#[test]
fn corrupt_new_source_is_refused_without_replacing_a_live_old_runtime() {
    let f = Fixture::new();
    f.file("old.wav", &wav(2, false, true));
    let path = f.program("old.uvip", "old.wav", "");
    let mut old = runtime(sampler_uvi::load(&path, 48000).unwrap());
    note(&mut old);
    let plan = old.active_plan();
    let before = old.now();
    let corrupt = f.file("bad.wav", b"RIFF\0\0\0\0WAVE");
    assert!(sampler_uvi::decode_source(&corrupt).is_err());
    let mut samples = sampler_kontakt::Samples::new(&f.0);
    assert!(samples.decode_source(&corrupt).is_err());
    assert_eq!(old.active_plan(), plan);
    assert_eq!(old.now(), before);
    let mut out = [[0.; 2]; 64];
    old.render(&mut out).unwrap();
    assert!(out.iter().flatten().any(|v| *v != 0.));
}

#[test]
fn complete_original_channel_packing_checks_geometry_and_staging_budget() {
    let samples = [0.125f32; 15];
    assert!(sampler_kontakt::resident_storage_from_interleaved(48000, 0, &samples, 140).is_err());
    assert!(
        sampler_kontakt::resident_storage_from_interleaved(48000, 3, &samples[..14], 140).is_err()
    );
    // 5 original frames * 3 channels * 4 + 5 frames * 2 physical pairs * 8.
    assert!(sampler_kontakt::resident_storage_from_interleaved(48000, 3, &samples, 139).is_err());
    let admitted =
        sampler_kontakt::resident_storage_from_interleaved(48000, 3, &samples, 140).unwrap();
    assert_eq!(
        (
            admitted.as_source().channels().get(),
            admitted.as_source().frame_count()
        ),
        (3, 5)
    );
    let view = admitted.as_source().resident_view().unwrap();
    for c in 0..3 {
        for frame in 0..5 {
            assert_eq!(view.sample(c, frame), Some(0.125));
        }
    }
    assert_eq!(samples, [0.125; 15]);
}

#[test]
fn normal_uvi_family_group_purge_and_reload_use_the_frozen_original_source_and_keep_identity() {
    use sampler_core::{Pcm, ScriptInstanceId};
    let fixture = Fixture::new();
    fixture.file("wide.wav", &wav(6, false, true));
    fixture.file("reference.wav", &wav(6, true, true));
    let program = fixture.program("wide.uvip", "wide.wav", "");
    let reference = fixture.program("reference.uvip", "reference.wav", "");
    let mut translated = sampler_uvi::translate_path(&program).unwrap();
    translated.instrument.processing_scopes.push(graph(6, 3));
    let loaded = sampler_uvi::assemble_translated(translated, 48000).unwrap();
    let targets: Vec<_> = loaded
        .instrument
        .source_indices
        .sample_targets
        .iter()
        .map(|target| sampler_core::SampleTargetId(target.node))
        .collect();
    let groups = loaded.plan.group_count();
    assert_eq!(groups, 1, "three players retain their ONE authored Keygroup owner");
    assert_eq!(loaded.instrument.zones.len(), 3);
    let mut packs = Vec::<Pcm>::new();
    let source = loaded.plan.source_storage(0).unwrap();
    let identity = source.id();
    source.physical_dependencies(|pack| packs.push(pack.clone()));
    let assets: Vec<_> = packs.iter().map(Pcm::asset_id).collect();
    let mut actual = runtime(loaded);
    let plan = actual.active_plan();
    note(&mut actual);
    actual.render(&mut [[0.; 2]; 64]).unwrap();
    assert_eq!(actual.voice_count(), 3);
    for group in 0..groups {
        actual
            .request_group_residency(plan, group, false, ScriptInstanceId(0))
            .unwrap();
    }
    let settle = |runtime: &mut Runtime| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            runtime.render(&mut [[0.; 2]; 64]).unwrap();
            if runtime.residency_progress(plan).unwrap().pending == 0 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "ordinary source residency did not complete"
            );
            std::thread::yield_now();
        }
    };
    settle(&mut actual);
    assert_eq!(actual.voice_count(), 0);
    for group in 0..groups {
        assert!(!actual.group_resident(plan, group).unwrap());
    }
    assert!(
        packs
            .iter()
            .all(|pack| pack.resident_non_head_bytes() == 0 && pack.residency_owners() == 0)
    );
    // A real whole-file source recipe reloads every original physical ordinal.
    // No injected PCM, alternate IDs, retrigger workaround, or first-pack alias.
    for group in 0..groups {
        actual
            .request_group_residency(plan, group, true, ScriptInstanceId(0))
            .unwrap();
    }
    settle(&mut actual);
    for group in 0..groups {
        assert!(actual.group_resident(plan, group).unwrap());
    }
    assert_eq!(packs.iter().map(Pcm::asset_id).collect::<Vec<_>>(), assets);
    for pack in &packs {
        assert_eq!(pack.residency_owners(), groups as usize);
    }
    let status = actual.residency_progress(plan).unwrap();
    assert_eq!(
        (
            status.accepted,
            status.committed,
            status.failed,
            status.pending
        ),
        (u64::from(groups) * 2, u64::from(groups) * 2, 0, 0)
    );
    let mut expected = runtime(sampler_uvi::load(&reference, 48000).unwrap());
    // The absolute control-cell clock is part of the reference, even when this
    // fixture's authored parameters are constant.
    let mut silent = actual.now();
    while silent > 0 {
        let n = silent.min(64) as usize;
        expected.render(&mut [[0.; 2]; 64][..n]).unwrap();
        silent -= n as u64;
    }
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(902),
    };
    actual.trigger(input, 60, 1.).unwrap();
    expected.trigger(input, 60, 1.).unwrap();
    assert_eq!(actual.active_plan(), plan);
    for target in targets {
        assert_eq!(
            actual.sample_target_source_storage(target).unwrap().id(),
            identity
        );
    }
    let mut energy = 0.;
    support::without_heap(|| {
        for _ in 0..16 {
            let mut a = [[0.; 2]; 64];
            let mut b = a;
            actual.render(&mut a).unwrap();
            expected.render(&mut b).unwrap();
            for (a, b) in a.iter().zip(b) {
                for channel in 0..2 {
                    assert!((a[channel] - b[channel]).abs() <= 2e-6);
                    energy += f64::from(a[channel]).powi(2);
                }
            }
        }
    });
    assert!(energy > 0.01);
    assert_eq!(
        (
            actual.voice_count(),
            actual.processing_owner_count(),
            actual.nonfinite_frames(),
            actual.stream_underruns()
        ),
        (3, 1, 0, 0)
    );
}
