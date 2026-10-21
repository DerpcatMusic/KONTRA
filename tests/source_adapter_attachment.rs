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
fn projection(channels: usize) -> ir::ChannelMatrix {
    let mut cells = vec![0.; channels * 2];
    cells[channels - 1] = 1.;
    cells[channels * 2 - 1] = -0.5;
    ir::ChannelMatrix {
        input: ir::SignalLayout { channels },
        output: ir::SignalLayout { channels: 2 },
        cells,
        arithmetic: ir::MatrixArithmetic::SeparateMulAddF32,
    }
}
fn graph(channels: usize, members: usize) -> ir::ProcessingScope {
    let edges = projection(channels);
    let cells = edges.cells.clone();
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
        // The authored scope is the single membership, so the adapter's
        // uncompiled keygroup recipe is dropped rather than duplicated. Its
        // shared keygroup chain stays the authored scope's own output chain.
        let chain = t.instrument.source_indices.processing.iter().find_map(|r| r.chain);
        t.instrument.source_indices.processing.clear();
        t.instrument.source_indices.processing_matrices.clear();
        t.instrument.source_indices.processing_parameters.clear();
        t.instrument.source_indices.processing_ports.clear();
        t.instrument.source_indices.uvi_native_processing.clear();
        let mut scope = graph(channels, 3);
        scope.chain = chain;
        t.instrument.processing_scopes.push(scope);
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
    // The caller supplies the actual project graph for this wide source. Drop
    // the adapter's uncompiled keygroup recipe so the authored scope below is
    // the single membership for these zones; the shared keygroup chain remains
    // that authored scope's own output chain.
    let chain=translated.instrument.source_indices.processing.iter().find_map(|r|r.chain);
    translated.instrument.source_indices.processing.clear();
    translated.instrument.source_indices.processing_matrices.clear();
    translated.instrument.source_indices.processing_parameters.clear();
    translated.instrument.source_indices.processing_ports.clear();
    translated.instrument.source_indices.uvi_native_processing.clear();
    let mut scope=graph(6, 3);
    scope.chain=chain;
    translated.instrument.processing_scopes.push(scope);
    let loaded = sampler_uvi::assemble_translated(translated, 48000).unwrap();
    let targets: Vec<_> = loaded
        .instrument
        .source_indices
        .sample_targets
        .iter()
        .map(|target| sampler_core::SampleTargetId(target.node))
        .collect();
    let groups = loaded.plan.group_count();
    assert_eq!(groups, 3, "three players retain their authored per-oscillator groups");
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

// S: generated normal native-source fixtures. These are Core source/matrix laws,
// not native UVI f32 source-clock/filter/layout parity.
#[path="../crates/sampler-kontakt/tests/support/chunks.rs"]
mod streamed_wire;
const STREAM_SOURCE_FRAMES:usize=sampler_core::PAGE_FRAMES*17+127;
fn streamed_utf16(value:&str)->Vec<u8> {
    let mut out=(value.encode_utf16().count() as u32).to_le_bytes().to_vec();
    out.extend(value.encode_utf16().flat_map(u16::to_le_bytes));out
}
fn streamed_native_nki(sample:&str)->Vec<u8> {
    let mut group=streamed_utf16("Owned nonlinear and filter");for v in [1f32,0.,1.] {group.extend(v.to_le_bytes());}
    group.extend([1,0,0,0]);group.extend(0i32.to_le_bytes());group.extend((-1i16).to_le_bytes());
    group.extend((-1i32).to_le_bytes());group.extend(2i32.to_le_bytes());group.extend([0,0]);group.extend(0i32.to_le_bytes());
    let mut private=Vec::new();for _ in 0..136 {private.extend(8u32.to_le_bytes());private.extend([0;8]);}private.extend([0;24]);
    private.extend([0,0x13,0]);private.extend(8u32.to_le_bytes());
    private.extend([0;8]);
    private.push(1);private.extend([0,6,1]);private.extend(0u32.to_le_bytes());private.extend([0;23]);private.extend([255;22]);
    let mut groups=1u32.to_le_bytes().to_vec();groups.extend(streamed_wire::object(0x95,&private,&group,&streamed_wire::chunk(0x38,&0u32.to_le_bytes())));
    let mut zone=vec![0;12];for v in [1i16,127,60,61,0,0,0,0,60] {zone.extend(v.to_le_bytes());}
    for v in [1f32,0.,1.] {zone.extend(v.to_le_bytes());}zone.extend([0,1,0,0,0,0]);
    for v in [0i32,0,48000] {zone.extend(v.to_le_bytes());}zone.push(1);
    for v in [STREAM_SOURCE_FRAMES as i32,0,60] {zone.extend(v.to_le_bytes());}zone.extend(0f32.to_le_bytes());zone.push(0);zone.extend(0i32.to_le_bytes());
    let mut zones=1u32.to_le_bytes().to_vec();zones.extend(0u32.to_le_bytes());zones.extend(streamed_wire::object(0x9a,&[],&zone,&[]));
    let mut program=streamed_utf16("Owned MPE source-free fixture");program.extend(0f64.to_le_bytes());program.push(0);
    for v in [1f32,0.,1.] {program.extend(v.to_le_bytes());}program.extend([1,127,0,127]);program.extend((-1i16).to_le_bytes());program.extend([0;16]);program.push(0);program.extend(0i32.to_le_bytes());
    for _ in 0..3 {program.extend(streamed_utf16(""));}program.extend([0;6]);
    let mut children=streamed_wire::chunk(0x33,&groups);children.extend(streamed_wire::chunk(0x34,&zones));
    let mut payload=streamed_wire::chunk(0x28,&streamed_wire::object(0xae,&[],&program,&children));
    let mut table=vec![0;4];table.extend(1u32.to_le_bytes());table.extend(1i32.to_le_bytes());table.push(4);table.extend(streamed_utf16(sample));table.extend([0;12]);payload.extend(streamed_wire::chunk(0x3d,&table));
    let packed:Vec<_>=payload.chunks(32).flat_map(|b|std::iter::once((b.len()-1) as u8).chain(b.iter().copied())).collect();
    let mut nks=vec![0;222];nks[..4].copy_from_slice(&0x7fa89012u32.to_le_bytes());nks[4..8].copy_from_slice(&(packed.len() as u32).to_le_bytes());nks[8..10].copy_from_slice(&0x110u16.to_le_bytes());nks[10..14].copy_from_slice(&0xea37631au32.to_le_bytes());nks[186..190].copy_from_slice(&(payload.len() as u32).to_le_bytes());nks.extend(packed);nks.extend(0xb00ee1aeu32.to_le_bytes());nks.extend([1,1,12,0]);nks
}
fn streamed_wave(channels:usize,projected:bool)->Vec<u8> {
    let output=if projected {2}else{channels};let width=if projected {4}else{2};
    let mut b=b"RIFF".to_vec();b.extend(((36+STREAM_SOURCE_FRAMES*output*width) as u32).to_le_bytes());
    b.extend(b"WAVEfmt ");b.extend(16u32.to_le_bytes());b.extend((if projected {3u16}else{1}).to_le_bytes());b.extend((output as u16).to_le_bytes());
    b.extend(48000u32.to_le_bytes());b.extend(((48000*output*width) as u32).to_le_bytes());b.extend(((output*width) as u16).to_le_bytes());b.extend(((width*8) as u16).to_le_bytes());
    b.extend(b"data");b.extend(((STREAM_SOURCE_FRAMES*output*width) as u32).to_le_bytes());
    for frame in 0..STREAM_SOURCE_FRAMES {if projected {
        let x=f32::from(value(frame,channels-1))/32768.;b.extend(x.to_le_bytes());b.extend((-0.5*x).to_le_bytes());
    } else {for channel in 0..channels {b.extend(value(frame,channel).to_le_bytes());}}}b
}
fn streamed_aiff(channels:usize)->Vec<u8> {
    let mut comm=(channels as u16).to_be_bytes().to_vec();comm.extend((STREAM_SOURCE_FRAMES as u32).to_be_bytes());comm.extend(16u16.to_be_bytes());
    comm.extend(0x400eu16.to_be_bytes());comm.extend(0xbb80000000000000u64.to_be_bytes());
    let mut body=b"AIFFCOMM".to_vec();body.extend(18u32.to_be_bytes());body.extend(comm);body.extend(b"SSND");body.extend(((8+STREAM_SOURCE_FRAMES*channels*2) as u32).to_be_bytes());body.extend([0;8]);
    for frame in 0..STREAM_SOURCE_FRAMES {for channel in 0..channels {body.extend(value(frame,channel).to_be_bytes());}}
    let mut out=b"FORM".to_vec();out.extend((body.len() as u32).to_be_bytes());out.extend(body);out
}
fn streamed_ncw(channels:usize)->Vec<u8> {
    // Valid direct raw PCM16 blocks, not the legacy zero-width spelling.
    // The public encoder currently supports only mono/stereo; this explicit
    // authored framing keeps the wide reader fixture independent of it.
    let blocks=STREAM_SOURCE_FRAMES.div_ceil(512);let mut header=vec![0u8;120];
    header[..8].copy_from_slice(&0x01A89ED631010000u64.to_be_bytes());
    header[8..10].copy_from_slice(&(channels as u16).to_le_bytes());header[10..12].copy_from_slice(&16u16.to_le_bytes());
    header[12..16].copy_from_slice(&48000u32.to_le_bytes());header[16..20].copy_from_slice(&(STREAM_SOURCE_FRAMES as u32).to_le_bytes());
    header[20..24].copy_from_slice(&120u32.to_le_bytes());header[24..28].copy_from_slice(&(120+((blocks+1)*4) as u32).to_le_bytes());
    let mut data=Vec::new();let mut offsets=Vec::new();
    for block in 0..blocks {offsets.push(data.len() as u32);for channel in 0..channels {
        data.extend(0x3e9a0c16u32.to_le_bytes());data.extend(0i32.to_le_bytes());data.extend((-16i16).to_le_bytes());data.extend(0u16.to_le_bytes());data.extend(0u32.to_le_bytes());
        for frame in 0..512 {data.extend(value((block*512+frame).min(STREAM_SOURCE_FRAMES-1),channel).to_le_bytes());}
    }}
    offsets.push(data.len() as u32);header[28..32].copy_from_slice(&(data.len() as u32).to_le_bytes());
    for offset in offsets {header.extend(offset.to_le_bytes());}header.extend(data);header
}

fn streamed_kontakt(f:&Fixture,path:&std::path::Path,channels:usize)->sampler_kontakt::Kontakt {
    let mut translated=sampler_kontakt::read(path).unwrap();
    assert_eq!(translated.instrument.zones.len(),1);assert_eq!(translated.instrument.assets.len(),1);
    assert_eq!(translated.instrument.assets[0].residency,ir::AssetResidency::Streamable,"fixture must author a disk-streamable source");
    assert_eq!(translated.locations.len(),1);assert!(translated.locations[0].starts_with(&f.0));
    if channels>2 {translated.instrument.processing_scopes.push(graph(channels,1));}
    translated
}
#[test]
fn normal_kontakt_streamed_wav_aiff_ncw_keep_every_original_channel_and_match_resident_audio() {
    let fixture=Fixture::new();
    for encoding in ["wav","aif","ncw"] {for channels in [1,2,3,5,8] {
        let bytes=match encoding {"wav"=>streamed_wave(channels,false),"aif"=>streamed_aiff(channels),_=>streamed_ncw(channels)};
        let sample=format!("paged-{channels}.{encoding}");fixture.file(&sample,&bytes);
        let preset=fixture.file(&format!("paged-{channels}-{encoding}.nki"),&streamed_native_nki(&sample));
        let options=sampler_kontakt::Options{rate:48000,..Default::default()};
        let resident=sampler_kontakt::load_read(streamed_kontakt(&fixture,&preset,channels),&options,|_|(),&||false).unwrap();
        let groups=resident.instrument.groups.len();let homes=resident.instrument.zones.iter().map(|z|z.group).collect::<Vec<_>>();
        let policy=sampler_kontakt::StreamPolicy{voices:8,decoders:1,slack:std::time::Duration::ZERO,head_budget:128<<10,allow_cold_fallback:true,..Default::default()};
        let streamed=sampler_kontakt::load_read_streamed(streamed_kontakt(&fixture,&preset,channels),&options,&policy,|_|()).unwrap();
        assert_eq!(streamed.loaded.instrument.groups.len(),groups);assert_eq!(streamed.loaded.instrument.zones.iter().map(|z|z.group).collect::<Vec<_>>(),homes);
        assert_eq!(streamed.loaded.instrument.assets.len(),1);assert!(streamed.loaded.plan.source_storage(1).is_none());
        let storage=streamed.loaded.plan.source_storage(0).unwrap();assert_eq!(storage.channels().get(),channels.max(2));
        assert!(storage.is_paged());assert!(storage.resident_view().is_none());
        let mut physical=Vec::new();storage.physical_dependencies(|p|physical.push(p.clone()));assert_eq!(physical.len(),channels.div_ceil(2));
        assert!(physical.iter().all(|p|p.resident_frames().is_none()));
        let ids=physical.iter().map(sampler_core::Pcm::asset_id).collect::<Vec<_>>();
        let head_frames=streamed.report.head_frames;let keep_streamer=streamed.streamer;
        let mut actual=runtime(streamed.loaded).with_stream_cache(streamed.cache);let mut expected=runtime(resident);
        let horizon=u32::try_from(head_frames.max(512)).unwrap();actual.set_stream_horizon(horizon).unwrap();actual.set_cold_starts(true);
        let input=Input{protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:Some(1701)};
        let note=actual.trigger(input,60,1.).unwrap();let reference=expected.trigger(input,60,1.).unwrap();
        let mut a=[[0.;2];64];let mut b=a;let mut energy=0.;
        for block in 0..258 {
            actual.wait_streaming(horizon,std::time::Duration::from_secs(3)).unwrap();
            support::without_heap(||{actual.render(&mut a).unwrap();expected.render(&mut b).unwrap();});
            if block>=16 {for (a,b) in a.iter().zip(b) {for channel in 0..2 {assert!((a[channel]-b[channel]).abs()<=2e-6,"{encoding}/{channels}/{block}: {a:?} != {b:?}");energy+=f64::from(a[channel]).powi(2);}}}
        }
        assert!(energy>0.01);assert_eq!((actual.voice_count(),actual.nonfinite_frames(),actual.stream_underruns()),(1,0,0));
        assert!(keep_streamer.head_budget_status().held_bytes<=policy.head_budget);
        // Original ordinal evidence at a real late page, not only a nonzero
        // projection/attack. Mono uses its unchanged duplicated stereo lane.
        let frame=actual.now() as usize-16;
        let cache=actual.stream_cache_mut().unwrap().reader();
        for (pair,id) in ids.iter().copied().enumerate() {
            let read=cache.frame(id,frame).unwrap_or_else(|| {
                let mut owned=[[0.;2];1];physical[pair].copy_resident_range(frame,&mut owned).expect("late physical frame must be in admitted head or page");owned[0]
            });
            for lane in 0..2 {let ordinal=pair*2+lane;let expected=if channels==1 {f32::from(value(frame,0))/32768.} else if ordinal<channels {f32::from(value(frame,ordinal))/32768.} else {0.};assert_eq!(read[lane].to_bits(),expected.to_bits(),"{encoding}/{channels}/{ordinal}");}
        }
        actual.release(note).unwrap();expected.release(reference).unwrap();
        for _ in 0..16 {actual.wait_streaming(horizon,std::time::Duration::from_secs(3)).unwrap();actual.render(&mut a).unwrap();expected.render(&mut b).unwrap();}
        assert_eq!((actual.voice_count(),actual.processing_owner_count()),(0,0));
        assert_eq!(physical.iter().map(sampler_core::Pcm::asset_id).collect::<Vec<_>>(),ids);
    }}
}
#[test]
fn uncertified_wide_uvi_flac_stream_preserves_old_live_audio_without_custom_authority() {
    let fixture=Fixture::new();fixture.file("old.wav",&wav(2,false,true));let old=fixture.program("old.uvip","old.wav","");
    let mut runtime=runtime(sampler_uvi::load(&old,48000).unwrap());note(&mut runtime);runtime.render(&mut [[0.;2];64]).unwrap();
    let plan=runtime.active_plan();let clock=runtime.now();
    let flac=fixture.file("wide.flac",&w_owned_flac(5));
    verify(&sampler_uvi::decode_source(&flac).unwrap(),5); // Valid ordinary FLAC; no decoder-failure substitute.
    let preset=fixture.program("wide.uvip","wide.flac","");
    let mut translated=sampler_uvi::translate_path(&preset).unwrap();translated.instrument.processing_scopes.push(graph(5,3));
    let error=match sampler_uvi::assemble_translated_streamed(translated,48000,&Default::default()) {Ok(_)=>panic!("uncertified custom reader acquired wide streaming authority"),Err(error)=>error};
    assert!(error.to_string().contains("owned decoder/container admission"),"{error}");
    assert_eq!((runtime.active_plan(),runtime.now()),(plan,clock));let mut out=[[0.;2];64];runtime.render(&mut out).unwrap();assert!(out.iter().flatten().any(|v|*v!=0.));
}

#[test]
fn normal_streamed_family_group_purge_reload_retains_real_ids_and_late_pcm() {
    let fixture=Fixture::new();fixture.file("paged.wav",&streamed_wave(5,false));
    let path=fixture.file("paged.nki",&streamed_native_nki("paged.wav"));
    let options=sampler_kontakt::Options{rate:48000,..Default::default()};
    let policy=sampler_kontakt::StreamPolicy{voices:8,decoders:1,slack:std::time::Duration::ZERO,head_budget:1<<20,..Default::default()};
    let streamed=sampler_kontakt::load_read_streamed(streamed_kontakt(&fixture,&path,5),&options,&policy,|_|()).unwrap();
    let source=streamed.loaded.plan.source_storage(0).unwrap();let identity=source.id();
    let retained=match source {sampler_core::SourceStorageRef::Channels(family)=>family.clone(),_=>panic!("wide source lost its logical family")};
    let mut packs=Vec::new();source.physical_dependencies(|p|packs.push(p.clone()));let ids=packs.iter().map(sampler_core::Pcm::asset_id).collect::<Vec<_>>();
    let groups=streamed.loaded.plan.group_count();assert_eq!(groups,1);
    let horizon=u32::try_from(streamed.report.head_frames.max(512)).unwrap();let keep=streamed.streamer;
    let mut actual=runtime(streamed.loaded).with_stream_cache(streamed.cache);actual.set_stream_horizon(horizon).unwrap();actual.set_cold_starts(true);
    let plan=actual.active_plan();let input=Input{protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:Some(1801)};
    actual.trigger(input,60,1.).unwrap();actual.wait_streaming(horizon,std::time::Duration::from_secs(3)).unwrap();actual.render(&mut [[0.;2];64]).unwrap();
    actual.request_group_residency(plan,0,false,sampler_core::ScriptInstanceId(0)).unwrap();
    let settle=|runtime:&mut Runtime| {let deadline=std::time::Instant::now()+std::time::Duration::from_secs(3);loop {
        runtime.render(&mut [[0.;2];64]).unwrap();if runtime.residency_progress(plan).unwrap().pending==0 {break;}
        assert!(std::time::Instant::now()<deadline,"logical group worker did not finish");std::thread::yield_now();
    }};
    settle(&mut actual);assert_eq!(actual.voice_count(),0);assert!(!actual.group_resident(plan,0).unwrap());assert!(packs.iter().all(|p|p.residency_owners()==0 && p.head_bytes()==0));
    actual.request_group_residency(plan,0,true,sampler_core::ScriptInstanceId(0)).unwrap();settle(&mut actual);
    assert!(actual.group_resident(plan,0).unwrap());assert_eq!(packs.iter().map(sampler_core::Pcm::asset_id).collect::<Vec<_>>(),ids);
    assert_eq!(actual.active_plan(),plan);assert!(packs.iter().all(|p|p.residency_owners()==1));
    let progress=actual.residency_progress(plan).unwrap();assert_eq!((progress.accepted,progress.committed,progress.failed,progress.pending),(2,2,0,0));
    assert!(keep.head_budget_status().held_bytes<=policy.head_budget);
    let mut expected=runtime(sampler_kontakt::load_read(streamed_kontakt(&fixture,&path,5),&options,|_|(),&||false).unwrap());
    let mut before=actual.now();while before!=0 {let n=before.min(64) as usize;expected.render(&mut [[0.;2];64][..n]).unwrap();before-=n as u64;}
    let input=Input{external_id:Some(1802),..input};actual.trigger(input,60,1.).unwrap();expected.trigger(input,60,1.).unwrap();
    let mut energy=0.;for block in 0..128 {actual.wait_streaming(horizon,std::time::Duration::from_secs(3)).unwrap();let mut a=[[0.;2];64];let mut b=a;
        support::without_heap(||{actual.render(&mut a).unwrap();expected.render(&mut b).unwrap();});
        if block>=16 {for (a,b) in a.iter().zip(b) {for channel in 0..2 {assert!((a[channel]-b[channel]).abs()<=2e-6);energy+=f64::from(a[channel]).powi(2);}}}
    }
    assert!(energy>0.01);assert_eq!((actual.voice_count(),actual.processing_owner_count(),actual.stream_underruns(),actual.nonfinite_frames()),(1,1,0,0));
    // Same logical family and source generation survives ordinary reload.
    assert_eq!(retained.id(),identity);
    let mut after=Vec::new();retained.as_source().physical_dependencies(|p|after.push(p.asset_id()));assert_eq!(after,ids);
}
#[test]
fn cancellation_after_normal_logical_storage_construction_does_not_replace_live_plan() {
    use std::sync::atomic::{AtomicBool,Ordering};
    let fixture=Fixture::new();fixture.file("old.wav",&wav(2,false,true));let old=fixture.program("old.uvip","old.wav","");
    let mut old=runtime(sampler_uvi::load(&old,48000).unwrap());note(&mut old);old.render(&mut [[0.;2];64]).unwrap();let before=(old.active_plan(),old.now(),old.voice_count());
    fixture.file("paged.wav",&streamed_wave(5,false));let preset=fixture.file("paged.nki",&streamed_native_nki("paged.wav"));
    let canceled=AtomicBool::new(false);let options=sampler_kontakt::Options{rate:48000,..Default::default()};
    let result=sampler_kontakt::load_read_streamed_cancelable(streamed_kontakt(&fixture,&preset,5),&options,&Default::default(),|progress| {
        if matches!(progress,sampler_kontakt::Progress::Decoding{done,total,..} if done==total && total!=0) {canceled.store(true,Ordering::Release);}
    },&||canceled.load(Ordering::Acquire));
    assert!(canceled.load(Ordering::Acquire));assert!(matches!(result,Err(sampler_kontakt::LoadError::Canceled)));
    assert_eq!((old.active_plan(),old.now(),old.voice_count()),before);let mut out=[[0.;2];64];old.render(&mut out).unwrap();assert!(out.iter().flatten().any(|v|*v!=0.));
}

// W is an ordinary guarded PCM decoder capability, distinct from native UVI
// matrix/shared-filter admission. All three authored players remain retained.
fn w_uvi_translated(path:&std::path::Path,channels:usize)->sampler_uvi::Translated {
    let mut translated=sampler_uvi::translate_path(path).unwrap();
    if channels>2 {
        // Wide source owners require the caller's concrete output matrix; the
        // decoder does not infer a stereo fold from a wider working layout.
        translated.instrument.source_indices.processing_ports=
            translated.instrument.source_indices.processing.iter()
                .map(|recipe|ir::SourceProcessingPort{owner:recipe.owner,conversion:projection(channels)})
                .collect();
    }
    translated
}
fn w_crc8(bytes:&[u8])->u8 {let mut crc=0u8;for byte in bytes {crc^=*byte;for _ in 0..8 {crc=if crc&0x80!=0 {(crc<<1)^7}else{crc<<1};}}crc}
fn w_crc16(bytes:&[u8])->u16 {let mut crc=0u16;for byte in bytes {crc^=u16::from(*byte)<<8;for _ in 0..8 {crc=if crc&0x8000!=0 {(crc<<1)^0x8005}else{crc<<1};}}crc}
fn w_owned_flac(channels:usize)->Vec<u8> {
    // Independent verbatim channels, existing public decoder framing/CRC law.
    assert!((1..=8).contains(&channels));
    let mut info=vec![0;34];info[0..2].copy_from_slice(&(FRAMES as u16).to_be_bytes());info[2..4].copy_from_slice(&(FRAMES as u16).to_be_bytes());
    let packed=(48000u64<<44)|(((channels-1) as u64)<<41)|(15u64<<36)|FRAMES as u64;info[10..18].copy_from_slice(&packed.to_be_bytes());
    let mut out=b"fLaC".to_vec();out.extend([0x80,0,0,34]);out.extend(info);
    let mut frame=vec![0xff,0xf8,0x60,(((channels-1)<<4)|8) as u8,0,(FRAMES-1) as u8];frame.push(w_crc8(&frame));
    for channel in 0..channels {frame.push(2);for i in 0..FRAMES {frame.extend(value(i,channel).to_be_bytes());}}
    frame.extend(w_crc16(&frame).to_be_bytes());out.extend(frame);out
}
#[test]
fn normal_uvi_checked_pcm_streams_retain_original_ordinals_and_match_resident_late_audio() {
    let fixture=Fixture::new();
    for encoding in ["wav","aif"] {for channels in [1,2,3,5,8] {
        let sample=format!("certified-{channels}.{encoding}");
        fixture.file(&sample,&if encoding=="wav" {streamed_wave(channels,false)} else {streamed_aiff(channels)});
        let path=fixture.program(&format!("certified-{channels}-{encoding}.uvip"),&sample,"");
        let resident=sampler_uvi::assemble_translated(w_uvi_translated(&path,channels),48000).unwrap();
        let homes=resident.instrument.zones.iter().map(|z|z.group).collect::<Vec<_>>();
        let groups=resident.instrument.groups.len();
        let targets=resident.instrument.source_indices.sample_targets.clone();
        let policy=sampler_kontakt::StreamPolicy{voices:8,decoders:1,slack:std::time::Duration::ZERO,head_budget:128<<10,allow_cold_fallback:true,..Default::default()};
        let streamed=sampler_uvi::assemble_translated_streamed(w_uvi_translated(&path,channels),48000,&policy).unwrap();
        assert_eq!(streamed.loaded.instrument.groups.len(),groups);assert_eq!(streamed.loaded.instrument.zones.len(),3);
        assert_eq!(streamed.loaded.instrument.zones.iter().map(|z|z.group).collect::<Vec<_>>(),homes);
        assert_eq!(streamed.loaded.instrument.source_indices.sample_targets,targets);
        assert_eq!(streamed.loaded.instrument.assets.len(),1);
        let source=streamed.loaded.plan.source_storage(0).unwrap();assert_eq!(source.channels().get(),channels.max(2));assert!(source.is_paged());assert!(source.resident_view().is_none());
        let mut physical=Vec::new();source.physical_dependencies(|p|physical.push(p.clone()));let ids=physical.iter().map(sampler_core::Pcm::asset_id).collect::<Vec<_>>();assert_eq!(ids.len(),channels.div_ceil(2));
        let horizon=u32::try_from(streamed.report.head_frames.max(512)).unwrap();let keep=streamed.streamer;
        let mut actual=runtime(streamed.loaded).with_stream_cache(streamed.cache);let mut expected=runtime(resident);
        actual.set_stream_horizon(horizon).unwrap();actual.set_cold_starts(true);note(&mut actual);note(&mut expected);
        let mut a=[[0.;2];64];let mut b=a;let mut energy=0.;
        for block in 0..258 {
            actual.wait_streaming(horizon,std::time::Duration::from_secs(3)).unwrap();
            support::without_heap(||{actual.render(&mut a).unwrap();expected.render(&mut b).unwrap();});
            if block>=16 {for (x,y) in a.iter().zip(b) {for lane in 0..2 {assert!((x[lane]-y[lane]).abs()<=2e-6,"{encoding}/{channels}/{block}: {x:?} != {y:?}");energy+=f64::from(x[lane]).powi(2);}}}
        }
        assert!(energy>0.01);assert_eq!((actual.voice_count(),actual.nonfinite_frames(),actual.stream_underruns()),(3,0,0));
        assert_eq!(actual.processing_owner_count(),1);assert!(keep.head_budget_status().held_bytes<=policy.head_budget);
        let frame=actual.now() as usize-16;let cache=actual.stream_cache_mut().unwrap().reader();
        for (pair,id) in ids.iter().copied().enumerate() {
            let found=cache.frame(id,frame).unwrap_or_else(||{let mut out=[[0.;2];1];physical[pair].copy_resident_range(frame,&mut out).expect("late original ordinal absent from admitted head/page");out[0]});
            for lane in 0..2 {let ordinal=pair*2+lane;let expected=if channels==1 {f32::from(value(frame,0))/32768.} else if ordinal<channels {f32::from(value(frame,ordinal))/32768.} else {0.};assert_eq!(found[lane].to_bits(),expected.to_bits(),"{encoding}/{channels}/{ordinal}");}
        }
        assert_eq!(physical.iter().map(sampler_core::Pcm::asset_id).collect::<Vec<_>>(),ids);
    }}
}
#[test]
fn normal_uvi_checked_pcm_preserves_file_loop_and_explicit_empty_options_and_program_root() {
    let fixture=Fixture::new();fixture.file("looped.wav",&wav(5,false,true));
    for (name,options,looped) in [("inherit","",true),("empty","<PlaybackOptions/>",false),("explicit","<PlaybackOptions><Loop Start='32' End='64' Type='0'/></PlaybackOptions>",true)] {
        let path=fixture.program(&format!("{name}.uvip"),"looped.wav",options);
        let policy=sampler_kontakt::StreamPolicy{voices:8,decoders:1,..Default::default()};
        let streamed=sampler_uvi::assemble_translated_streamed(w_uvi_translated(&path,5),48000,&policy).unwrap();
        let resident=sampler_uvi::assemble_translated(w_uvi_translated(&path,5),48000).unwrap();
        for zone in &streamed.loaded.instrument.zones {assert_eq!(zone.pitch,ir::KeyTracking::Tracked{root:60});assert_eq!(zone.tune.semitones(),0.);match zone.playback.looping {
            ir::Looping::Continuous(range)=>{assert!(looped);assert_eq!((range.start,range.end),if name=="explicit" {(32,64)} else {(16,96)});},ir::Looping::None=>assert!(!looped),other=>panic!("unexpected file loop {other:?}")
        }}
        let horizon=u32::try_from(streamed.report.head_frames.max(128)).unwrap();let _keep=streamed.streamer;let mut actual=runtime(streamed.loaded).with_stream_cache(streamed.cache);let mut expected=runtime(resident);
        actual.set_stream_horizon(horizon).unwrap();actual.set_cold_starts(true);note(&mut actual);note(&mut expected);
        let mut energy=0.;for block in 0..32 {actual.wait_streaming(horizon,std::time::Duration::from_secs(3)).unwrap();let mut a=[[0.;2];64];let mut b=a;
            support::without_heap(||{actual.render(&mut a).unwrap();expected.render(&mut b).unwrap();});
            if block>=16 {for (a,b) in a.iter().zip(b) {for lane in 0..2 {assert!((a[lane]-b[lane]).abs()<=2e-6);energy+=f64::from(a[lane]).powi(2);}}}
        }
        if looped {assert!(energy>0.01);assert_eq!(actual.voice_count(),3);} else {assert_eq!(energy,0.);assert_eq!(actual.voice_count(),0);}
        assert_eq!((actual.nonfinite_frames(),actual.stream_underruns()),(0,0));
    }
}
#[test]
fn normal_uvi_certified_stream_group_purge_reload_preserves_all_real_pack_ids_and_late_pcm() {
    let fixture=Fixture::new();fixture.file("certified.wav",&streamed_wave(5,false));let path=fixture.program("certified.uvip","certified.wav","");
    let policy=sampler_kontakt::StreamPolicy{voices:8,decoders:1,slack:std::time::Duration::ZERO,head_budget:1<<20,..Default::default()};
    let streamed=sampler_uvi::assemble_translated_streamed(w_uvi_translated(&path,5),48000,&policy).unwrap();
    let source=streamed.loaded.plan.source_storage(0).unwrap();let identity=source.id();let family=match source {sampler_core::SourceStorageRef::Channels(f)=>f.clone(),_=>panic!("lost logical family")};
    let mut physical=Vec::new();source.physical_dependencies(|p|physical.push(p.clone()));let ids=physical.iter().map(sampler_core::Pcm::asset_id).collect::<Vec<_>>();
    let horizon=u32::try_from(streamed.report.head_frames.max(512)).unwrap();let keep=streamed.streamer;let groups=streamed.loaded.plan.group_count();let mut actual=runtime(streamed.loaded).with_stream_cache(streamed.cache);actual.set_stream_horizon(horizon).unwrap();actual.set_cold_starts(true);
    let plan=actual.active_plan();let input=Input{protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:Some(901)};
    let first=actual.trigger(input,60,1.).unwrap();actual.wait_streaming(horizon,std::time::Duration::from_secs(3)).unwrap();actual.render(&mut [[0.;2];64]).unwrap();assert_eq!(actual.voice_count(),3);
    let settle=|rt:&mut Runtime| {let deadline=std::time::Instant::now()+std::time::Duration::from_secs(3);loop {rt.render(&mut [[0.;2];64]).unwrap();if rt.residency_progress(plan).unwrap().pending==0 {break;}assert!(std::time::Instant::now()<deadline);std::thread::yield_now();}};
    for group in 0..groups {actual.request_group_residency(plan,group,false,sampler_core::ScriptInstanceId(0)).unwrap();}
    settle(&mut actual);
    for group in 0..groups {assert!(!actual.group_resident(plan,group).unwrap());}
    assert_eq!(actual.voice_count(),0);assert!(physical.iter().all(|p|p.residency_owners()==0 && p.head_bytes()==0));
    // Purging source voices does not synthesize a key-up for the real host
    // input. The post-reload attack must have its own external note identity.
    assert!(actual.input_held(first).unwrap());
    for group in 0..groups {actual.request_group_residency(plan,group,true,sampler_core::ScriptInstanceId(0)).unwrap();}
    settle(&mut actual);
    for group in 0..groups {assert!(actual.group_resident(plan,group).unwrap());}
    assert_eq!(actual.active_plan(),plan);assert_eq!(family.id(),identity);assert_eq!(physical.iter().map(sampler_core::Pcm::asset_id).collect::<Vec<_>>(),ids);assert!(physical.iter().all(|p|p.residency_owners()==groups as usize));
    let mut expected=runtime(sampler_uvi::assemble_translated(w_uvi_translated(&path,5),48000).unwrap());let mut before=actual.now();while before!=0 {let n=before.min(64) as usize;expected.render(&mut [[0.;2];64][..n]).unwrap();before-=n as u64;}
    let input=Input{external_id:Some(902),..input};actual.trigger(input,60,1.).unwrap();expected.trigger(input,60,1.).unwrap();let mut energy=0.;for block in 0..128 {actual.wait_streaming(horizon,std::time::Duration::from_secs(3)).unwrap();let mut a=[[0.;2];64];let mut b=a;
        support::without_heap(||{actual.render(&mut a).unwrap();expected.render(&mut b).unwrap();});if block>=16 {for (a,b) in a.iter().zip(b) {for lane in 0..2 {assert!((a[lane]-b[lane]).abs()<=2e-6);energy+=f64::from(a[lane]).powi(2);}}}}
    assert!(energy>0.01);assert_eq!((actual.voice_count(),actual.processing_owner_count(),actual.stream_underruns(),actual.nonfinite_frames()),(3,1,0,0));assert!(keep.head_budget_status().held_bytes<=policy.head_budget);
}
#[test]
fn canceled_normal_wide_uvi_pcm_assembly_preserves_old_live_plan_and_pcm() {
    use std::sync::atomic::{AtomicUsize,Ordering};let fixture=Fixture::new();fixture.file("old.wav",&wav(2,false,true));let old=fixture.program("old.uvip","old.wav","");let mut old=runtime(sampler_uvi::load(&old,48000).unwrap());note(&mut old);old.render(&mut [[0.;2];64]).unwrap();let before=(old.active_plan(),old.now(),old.voice_count());
    fixture.file("incoming.wav",&streamed_wave(5,false));let path=fixture.program("incoming.uvip","incoming.wav","");let calls=AtomicUsize::new(0);
    // Entry and source-loop checks pass. The next ordinary assembly check
    // cancels after initial guarded source/header discovery, before publication.
    let result=sampler_uvi::assemble_translated_streamed_cancelable(w_uvi_translated(&path,5),48000,&Default::default(),&||calls.fetch_add(1,Ordering::AcqRel)>=2);
    assert!(calls.load(Ordering::Acquire)>=3);assert!(result.is_err());assert!(result.err().unwrap().to_string().contains("cancel"));assert_eq!((old.active_plan(),old.now(),old.voice_count()),before);
    let mut out=[[0.;2];64];old.render(&mut out).unwrap();assert!(out.iter().flatten().any(|s|*s!=0.));
}
#[test]
fn new_wide_pcm_allocating_metadata_is_refused_before_ordinary_probe() {
    let fixture=Fixture::new();let mut bytes=streamed_wave(5,false);let mut info=b"LIST".to_vec();info.extend(16u32.to_le_bytes());info.extend(b"INFOINAM");info.extend((1u32<<20).to_le_bytes());info.extend([0;4]);bytes.splice(36..36,info);let n=(bytes.len()-8) as u32;bytes[4..8].copy_from_slice(&n.to_le_bytes());fixture.file("unaccounted.wav",&bytes);
    let path=fixture.program("unaccounted.uvip","unaccounted.wav","");let error=match sampler_uvi::assemble_translated_streamed(w_uvi_translated(&path,5),48000,&Default::default()) {Ok(_)=>panic!("unaccounted allocating metadata entered checked owner"),Err(e)=>e};
    assert!(error.to_string().contains("uncertified"),"{error}");
}

#[test]
fn uncertified_wide_flac_impulse_only_keeps_its_existing_normal_reader_domain() {
    let fixture=Fixture::new();fixture.file("tone.wav",&wav(2,false,true));fixture.file("impulse.flac",&w_owned_flac(5));
    let path=fixture.program("impulse-only.uvip","tone.wav","");let xml=std::fs::read_to_string(&path).unwrap();let xml=xml.replacen("<Layers>","<Inserts><Convolver Dry='0.5' Wet='0.5' NormalizePower='0' SamplePath='impulse.flac'/></Inserts><Layers>",1);std::fs::write(&path,xml).unwrap();
    let streamed=sampler_uvi::assemble_translated_streamed(sampler_uvi::translate_path(&path).unwrap(),48000,&Default::default()).unwrap();
    assert_eq!(streamed.loaded.instrument.zones.len(),3);assert!(streamed.loaded.instrument.zones.iter().all(|z|z.asset.is_some()));
    assert_eq!(streamed.loaded.instrument.impulses.len(),1);let impulse=&streamed.loaded.instrument.impulses[0];assert_eq!(impulse.rate,48000);assert_eq!(impulse.left.len(),FRAMES);assert_eq!(impulse.right.len(),FRAMES);
    for frame in 0..FRAMES {assert_eq!(impulse.left[frame].to_bits(),(f32::from(value(frame,0))/32768.).to_bits());assert_eq!(impulse.right[frame].to_bits(),(f32::from(value(frame,1))/32768.).to_bits());}
    let _keep=streamed.streamer;let mut rt=runtime(streamed.loaded).with_stream_cache(streamed.cache);note(&mut rt);let mut out=[[0.;2];64];support::without_heap(||rt.render(&mut out).unwrap());assert!(out.iter().flatten().any(|s|*s!=0.));assert_eq!((rt.voice_count(),rt.nonfinite_frames(),rt.stream_underruns()),(3,0,0));
}

#[test]
fn known_wide_admission_failures_never_erase_bad_player_beside_a_valid_sibling() {
    let fixture=Fixture::new();fixture.file("old.wav",&wav(2,false,true));let old_path=fixture.program("old-held.uvip","old.wav","");
    let loaded=sampler_uvi::load(&old_path,48000).unwrap();let targets=loaded.instrument.source_indices.sample_targets.iter().map(|t|sampler_core::SampleTargetId(t.node)).collect::<Vec<_>>();let mut old=runtime(loaded);note(&mut old);old.render(&mut [[0.;2];64]).unwrap();
    let old_sources=targets.iter().map(|target|old.sample_target_source_storage(*target).unwrap().id()).collect::<Vec<_>>();let before=(old.active_plan(),old.now(),old.voice_count());
    fixture.file("sibling.wav",&wav(2,false,true));
    for malformed in ["unknown-code","missing-data","aiff-zero-rate","aiff-negative-rate","aiff-nan-rate","aiff-inf-rate","aiff-subunit-rate","aiff-overflow-rate"] {
        let (sample,bytes)=if malformed.starts_with("aiff-") {
            let mut b=streamed_aiff(5);
            let (exponent,mantissa)=match malformed {
                "aiff-zero-rate"=>(0u16,0u64),
                "aiff-negative-rate"=>(0xc00e,0xbb80000000000000),
                "aiff-nan-rate"=>(0x7fff,0xc000000000000000),
                "aiff-inf-rate"=>(0x7fff,0),
                "aiff-subunit-rate"=>(0x3ffe,0x8000000000000000),
                "aiff-overflow-rate"=>(0x401f,0x8000000000000000),
                _=>panic!("authored malformed AIFF case"),
            };
            b[28..30].copy_from_slice(&exponent.to_be_bytes());b[30..38].copy_from_slice(&mantissa.to_be_bytes());("bad.aif",b)
        } else {let mut b=streamed_wave(5,false);if malformed=="unknown-code" {b[20..22].copy_from_slice(&0x1234u16.to_le_bytes());} else {b.truncate(36);b[4..8].copy_from_slice(&28u32.to_le_bytes());}("bad.wav",b)};
        fixture.file(sample,&bytes);let path=fixture.program("mixed-admission.uvip","sibling.wav","");let xml=std::fs::read_to_string(&path).unwrap();let added=format!("<Keygroup LowKey='61' HighKey='61'><Oscillators><SamplePlayer SamplePath='{sample}' BaseNote='61' Gain='1'/></Oscillators></Keygroup>");let xml=xml.replacen("</Keygroups>",&format!("{added}</Keygroups>"),1);std::fs::write(&path,xml).unwrap();
        let mut translated=sampler_uvi::translate_path(&path).unwrap();assert_eq!(translated.instrument.zones.len(),4);assert_eq!(translated.instrument.source_indices.sample_targets.len(),4);let mut scope=graph(5,1);scope.members=vec![ir::ZoneRef(3)];translated.instrument.processing_scopes.push(scope);
        // An Ok result with only the valid sibling would silently narrow the
        // authored graph. This entire incoming assembly must instead fail.
        let error=match sampler_uvi::assemble_translated_streamed(translated,48000,&Default::default()) {Ok(_)=>panic!("known-wide {malformed} player was filtered beside valid sibling"),Err(e)=>e};
        assert!(error.to_string().contains("wide UVI"),"{malformed}: {error}");assert_eq!((old.active_plan(),old.now(),old.voice_count()),before);
        for (target,id) in targets.iter().zip(&old_sources) {assert_eq!(old.sample_target_source_storage(*target).unwrap().id(),*id);}
    }
    let mut out=[[0.;2];64];support::without_heap(||old.render(&mut out).unwrap());assert!(out.iter().flatten().any(|v|*v!=0.));
}

fn f_crc8(bytes:&[u8])->u8 {let mut crc=0u8;for byte in bytes {crc^=*byte;for _ in 0..8 {crc=if crc&0x80!=0 {(crc<<1)^7}else{crc<<1};}}crc}
fn f_crc16(bytes:&[u8])->u16 {let mut crc=0u16;for byte in bytes {crc^=u16::from(*byte)<<8;for _ in 0..8 {crc=if crc&0x8000!=0 {(crc<<1)^0x8005}else{crc<<1};}}crc}
fn f_number(value:u64)->Vec<u8> {
    if value<128 {return vec![value as u8];}
    let count=(2usize..=7).find(|&n|value<(1u64<<(5*n+1))).expect("authored FLAC coded-number range");
    let mut bytes=vec![((0xffu16<<(8-count)) as u8)|((value>>(6*(count-1))) as u8)];
    for shift in (0..count-1).rev() {bytes.push(0x80|((value>>(6*shift)) as u8&63));}bytes
}
fn f_flac(channels:usize,blocks:&[usize],variable:bool,metadata:&[(u8,Vec<u8>)],raw:impl Fn(usize,usize)->i16)->Vec<u8> {
    assert!((1..=8).contains(&channels));assert!(!blocks.is_empty());assert!(blocks.iter().all(|n|(16..=65535).contains(n)));
    if !variable {assert!(blocks.iter().all(|n|*n==blocks[0]));}
    let frames=blocks.iter().sum::<usize>();let mut audio=Vec::new();let mut lengths=Vec::new();let mut at=0;
    for (ordinal,&count) in blocks.iter().enumerate() {
        let mut frame=vec![0xff,if variable {0xf9}else{0xf8},0x70,(((channels-1)<<4)|8) as u8];
        frame.extend(f_number(if variable {at as u64}else{ordinal as u64}));frame.extend(((count-1) as u16).to_be_bytes());frame.push(f_crc8(&frame));
        for channel in 0..channels {frame.push(2);for i in 0..count {frame.extend(raw(at+i,channel).to_be_bytes());}}
        frame.extend(f_crc16(&frame).to_be_bytes());lengths.push(frame.len());audio.extend(frame);at+=count;
    }
    let mut info=vec![0;34];info[..2].copy_from_slice(&(*blocks.iter().min().unwrap() as u16).to_be_bytes());info[2..4].copy_from_slice(&(*blocks.iter().max().unwrap() as u16).to_be_bytes());
    for (at,n) in [(4,*lengths.iter().min().unwrap()),(7,*lengths.iter().max().unwrap())] {assert!(n<1<<24);info[at..at+3].copy_from_slice(&(n as u32).to_be_bytes()[1..]);}
    let packed=(48000u64<<44)|(((channels-1) as u64)<<41)|(15u64<<36)|frames as u64;info[10..18].copy_from_slice(&packed.to_be_bytes());
    let mut bytes=b"fLaC".to_vec();bytes.extend([if metadata.is_empty(){128}else{0},0,0,34]);bytes.extend(info);
    for (i,(kind,body)) in metadata.iter().enumerate() {assert!(body.len()<1<<24);bytes.push(*kind|if i+1==metadata.len(){128}else{0});bytes.extend(&(body.len() as u32).to_be_bytes()[1..]);bytes.extend(body);}
    bytes.extend(audio);bytes
}

#[test]
fn normal_uvi_checked_flac_keeps_original_channels_and_all_players_through_late_streamed_pcm() {
    let fixture=Fixture::new();
    for channels in [1usize,2,3,5,6,8] {
        let file=format!("owned-{channels}.flac");let path=fixture.file(&file,&f_flac(channels,&vec![512;160],false,&[],value));
        let preset=fixture.program("owned-flac.uvip",&file,"");
        let policy=sampler_kontakt::StreamPolicy{voices:8,decoders:1,slack:std::time::Duration::ZERO,head_budget:1<<20,..Default::default()};
        let translated=w_uvi_translated(&preset,channels);
        let groups=translated.instrument.groups.len();
        let streamed=sampler_uvi::assemble_translated_streamed(translated,48000,&policy).unwrap();
        assert_eq!(streamed.loaded.instrument.zones.len(),3);assert_eq!(streamed.loaded.instrument.groups.len(),groups);
        let targets=streamed.loaded.instrument.source_indices.sample_targets.iter().map(|t|sampler_core::SampleTargetId(t.node)).collect::<Vec<_>>();assert_eq!(targets.len(),3);
        let source=streamed.loaded.plan.source_storage(0).unwrap();let source_id=source.id();assert_eq!(source.channels().get(),channels.max(2));assert!(source.is_paged());assert!(source.resident_view().is_none());
        let mut packs=Vec::new();source.physical_dependencies(|p|packs.push(p.clone()));let ids=packs.iter().map(sampler_core::Pcm::asset_id).collect::<Vec<_>>();assert_eq!(ids.len(),channels.div_ceil(2));
        let horizon=u32::try_from(streamed.report.head_frames.max(512)).unwrap();let keep=streamed.streamer;
        let mut actual=runtime(streamed.loaded).with_stream_cache(streamed.cache);let mut expected=runtime(sampler_uvi::assemble_translated(w_uvi_translated(&preset,channels),48000).unwrap());
        for target in &targets {assert_eq!(actual.sample_target_source_storage(*target).unwrap().id(),source_id);}
        actual.set_stream_horizon(horizon).unwrap();actual.set_cold_starts(true);note(&mut actual);note(&mut expected);
        let mut energy=0.;for block in 0..258 {
            actual.wait_streaming(horizon,std::time::Duration::from_secs(3)).unwrap();let mut a=[[0.;2];64];let mut b=a;
            support::without_heap(||{actual.render(&mut a).unwrap();expected.render(&mut b).unwrap();});
            if block>=16 {for (a,b) in a.iter().zip(b) {for lane in 0..2 {assert!((a[lane]-b[lane]).abs()<=2e-6,"{channels}/{block}: {a:?} != {b:?}");energy+=f64::from(a[lane]).powi(2);}}}
        }
        assert!(energy>0.01);assert_eq!((actual.voice_count(),actual.stream_underruns(),actual.nonfinite_frames()),(3,0,0));assert_eq!(actual.processing_owner_count(),1);
        let frame=actual.now() as usize-16;let cache=actual.stream_cache_mut().unwrap().reader();
        for (pair,id) in ids.iter().copied().enumerate() {
            let found=cache.frame(id,frame).unwrap_or_else(||{let mut out=[[0.;2];1];packs[pair].copy_resident_range(frame,&mut out).expect("FLAC late original ordinal not in admitted head/page");out[0]});
            for lane in 0..2 {let ordinal=pair*2+lane;let expected=if channels==1 {f32::from(value(frame,0))/32768.} else if ordinal<channels {f32::from(value(frame,ordinal))/32768.} else {0.};assert_eq!(found[lane].to_bits(),expected.to_bits());}
        }
        assert_eq!(packs.iter().map(sampler_core::Pcm::asset_id).collect::<Vec<_>>(),ids);assert!(keep.head_budget_status().held_bytes<=policy.head_budget);assert!(path.is_file());
    }
}
#[test]
fn normal_uvi_checked_flac_foreign_loop_options_and_program_root_keep_existing_precedence() {
    let fixture=Fixture::new();let encoded=wav(5,false,true);
    // Xiph preserves the RIFF header in one APPLICATION and each original
    // chunk in a separate APPLICATION. Audio data retains only its header.
    let mut applications=vec![(2,[b"riff".as_slice(),&encoded[..12]].concat())];
    let mut at=12;
    while at<encoded.len() {
        let bytes=u32::from_le_bytes(encoded[at+4..at+8].try_into().unwrap()) as usize;
        let end=at+8+bytes+(bytes&1);
        let preserved=if &encoded[at..at+4]==b"data" {&encoded[at..at+8]} else {&encoded[at..end]};
        applications.push((2,[b"riff".as_slice(),preserved].concat()));at=end;
    }
    assert_eq!(at,encoded.len());
    fixture.file("loop.flac",&f_flac(5,&[128],false,&applications,value));
    for (name,options,looped) in [("inherit","",true),("empty","<PlaybackOptions/>",false),("explicit","<PlaybackOptions><Loop Start='32' End='64' Type='0'/></PlaybackOptions>",true)] {
        let preset=fixture.program(&format!("f-{name}.uvip"),"loop.flac",options);let policy=sampler_kontakt::StreamPolicy{voices:8,decoders:1,..Default::default()};
        let streamed=sampler_uvi::assemble_translated_streamed(w_uvi_translated(&preset,5),48000,&policy).unwrap();
        let resident=sampler_uvi::assemble_translated(w_uvi_translated(&preset,5),48000).unwrap();
        for zone in &streamed.loaded.instrument.zones {assert_eq!(zone.pitch,ir::KeyTracking::Tracked{root:60});assert_eq!(zone.tune.semitones(),0.);match zone.playback.looping {
            ir::Looping::Continuous(range)=>{assert!(looped);assert_eq!((range.start,range.end),if name=="explicit"{(32,64)}else{(16,96)});},ir::Looping::None=>assert!(!looped),other=>panic!("unexpected FLAC file loop {other:?}")
        }}
        let horizon=u32::try_from(streamed.report.head_frames.max(128)).unwrap();let _keep=streamed.streamer;let mut actual=runtime(streamed.loaded).with_stream_cache(streamed.cache);let mut expected=runtime(resident);
        actual.set_stream_horizon(horizon).unwrap();actual.set_cold_starts(true);note(&mut actual);note(&mut expected);let mut energy=0.;
        for block in 0..32 {actual.wait_streaming(horizon,std::time::Duration::from_secs(3)).unwrap();let mut a=[[0.;2];64];let mut b=a;support::without_heap(||{actual.render(&mut a).unwrap();expected.render(&mut b).unwrap();});if block>=16 {for (a,b) in a.iter().zip(b) {for lane in 0..2 {assert!((a[lane]-b[lane]).abs()<=2e-6);energy+=f64::from(a[lane]).powi(2);}}}}
        if looped {assert!(energy>0.01);assert_eq!(actual.voice_count(),3);} else {assert_eq!(energy,0.);assert_eq!(actual.voice_count(),0);}
        assert_eq!((actual.stream_underruns(),actual.nonfinite_frames()),(0,0));
    }
}
#[test]
fn malformed_whole_riff_flac_application_cannot_publish_a_wide_processing_source() {
    let fixture=Fixture::new();fixture.file("old.wav",&wav(2,false,true));
    let old=fixture.program("old-foreign.uvip","old.wav","");
    let mut old=runtime(sampler_uvi::load(&old,48000).unwrap());note(&mut old);
    old.render(&mut [[0.;2];64]).unwrap();let before=(old.active_plan(),old.now(),old.voice_count());
    let foreign=[b"riff".as_slice(),wav(5,false,true).as_slice()].concat();
    fixture.file("malformed-foreign.flac",&f_flac(5,&[128],false,&[(2,foreign)],value));
    let preset=fixture.program("malformed-foreign.uvip","malformed-foreign.flac","");
    // The audio reader rejects the whole-RIFF application as malformed
    // playback metadata, so neither loader may publish a wide source.
    let loaded=sampler_uvi::assemble_translated(w_uvi_translated(&preset,5),48000).unwrap();
    assert_eq!(loaded.instrument.zones.len(),3);
    assert!(loaded.instrument.zones.iter().all(|zone|zone.asset.is_none()),"malformed whole-RIFF application must not publish a wide source");
    assert!(loaded.plan.source_storage(0).is_none());
    let streamed=sampler_uvi::assemble_translated_streamed(w_uvi_translated(&preset,5),48000,&Default::default()).unwrap();
    assert!(streamed.loaded.instrument.zones.iter().all(|zone|zone.asset.is_none()),"malformed whole-RIFF application must not publish a streamed wide source");
    assert_eq!((old.active_plan(),old.now(),old.voice_count()),before);
    let mut output=[[0.;2];64];old.render(&mut output).unwrap();assert!(output.iter().flatten().any(|value|*value!=0.));
}
#[test]
fn normal_uvi_checked_flac_group_reload_reuses_all_real_pack_ids_and_recovers_late_pcm() {
    let fixture=Fixture::new();fixture.file("family.flac",&f_flac(6,&vec![512;160],false,&[],value));let preset=fixture.program("family-flac.uvip","family.flac","");
    let policy=sampler_kontakt::StreamPolicy{voices:8,decoders:1,slack:std::time::Duration::ZERO,head_budget:1<<20,..Default::default()};
    let streamed=sampler_uvi::assemble_translated_streamed(w_uvi_translated(&preset,6),48000,&policy).unwrap();let source=streamed.loaded.plan.source_storage(0).unwrap();let identity=source.id();
    let family=match source {sampler_core::SourceStorageRef::Channels(f)=>f.clone(),_=>panic!("FLAC logical family was narrowed")};let mut packs=Vec::new();source.physical_dependencies(|p|packs.push(p.clone()));let ids=packs.iter().map(sampler_core::Pcm::asset_id).collect::<Vec<_>>();assert_eq!(ids.len(),3);
    let horizon=u32::try_from(streamed.report.head_frames.max(512)).unwrap();let keep=streamed.streamer;let groups=streamed.loaded.plan.group_count();let mut actual=runtime(streamed.loaded).with_stream_cache(streamed.cache);actual.set_stream_horizon(horizon).unwrap();actual.set_cold_starts(true);
    let plan=actual.active_plan();let input=Input{protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:Some(1901)};let first=actual.trigger(input,60,1.).unwrap();actual.wait_streaming(horizon,std::time::Duration::from_secs(3)).unwrap();actual.render(&mut [[0.;2];64]).unwrap();assert_eq!(actual.voice_count(),3);
    let settle=|rt:&mut Runtime| {let deadline=std::time::Instant::now()+std::time::Duration::from_secs(3);loop {rt.render(&mut [[0.;2];64]).unwrap();if rt.residency_progress(plan).unwrap().pending==0 {break;}assert!(std::time::Instant::now()<deadline);std::thread::yield_now();}};
    for group in 0..groups {actual.request_group_residency(plan,group,false,sampler_core::ScriptInstanceId(0)).unwrap();}
    settle(&mut actual);for group in 0..groups {assert!(!actual.group_resident(plan,group).unwrap());}assert_eq!(actual.voice_count(),0);assert!(actual.input_held(first).unwrap());assert!(packs.iter().all(|p|p.residency_owners()==0 && p.head_bytes()==0));
    for group in 0..groups {actual.request_group_residency(plan,group,true,sampler_core::ScriptInstanceId(0)).unwrap();}
    settle(&mut actual);for group in 0..groups {assert!(actual.group_resident(plan,group).unwrap());}assert_eq!(actual.active_plan(),plan);assert_eq!(family.id(),identity);assert_eq!(packs.iter().map(sampler_core::Pcm::asset_id).collect::<Vec<_>>(),ids);assert!(packs.iter().all(|p|p.residency_owners()==groups as usize));
    let mut expected=runtime(sampler_uvi::assemble_translated(w_uvi_translated(&preset,6),48000).unwrap());let mut before=actual.now();while before!=0 {let n=before.min(64) as usize;expected.render(&mut [[0.;2];64][..n]).unwrap();before-=n as u64;}
    let input=Input{external_id:Some(1902),..input};actual.trigger(input,60,1.).unwrap();expected.trigger(input,60,1.).unwrap();let mut energy=0.;
    for block in 0..128 {actual.wait_streaming(horizon,std::time::Duration::from_secs(3)).unwrap();let mut a=[[0.;2];64];let mut b=a;support::without_heap(||{actual.render(&mut a).unwrap();expected.render(&mut b).unwrap();});if block>=16 {for (a,b) in a.iter().zip(b) {for lane in 0..2 {assert!((a[lane]-b[lane]).abs()<=2e-6);energy+=f64::from(a[lane]).powi(2);}}}}
    assert!(energy>0.01);assert_eq!((actual.voice_count(),actual.processing_owner_count(),actual.nonfinite_frames(),actual.stream_underruns()),(3,1,0,0));assert!(keep.head_budget_status().held_bytes<=policy.head_budget);
}
#[test]
fn normal_uvi_unaccounted_flac_metadata_and_cancel_keep_old_plan_and_target_identities() {
    use std::sync::atomic::{AtomicUsize,Ordering};let fixture=Fixture::new();fixture.file("old.wav",&wav(2,false,true));let old_path=fixture.program("f-old.uvip","old.wav","");let loaded=sampler_uvi::load(&old_path,48000).unwrap();let targets=loaded.instrument.source_indices.sample_targets.iter().map(|t|sampler_core::SampleTargetId(t.node)).collect::<Vec<_>>();let mut old=runtime(loaded);note(&mut old);old.render(&mut [[0.;2];64]).unwrap();let before=(old.active_plan(),old.now(),old.voice_count());let ids=targets.iter().map(|t|old.sample_target_source_storage(*t).unwrap().id()).collect::<Vec<_>>();
    fixture.file("bad.flac",&f_flac(5,&[128],false,&[(6,vec![0;4])],value));let path=fixture.program("f-bad.uvip","bad.flac","");
    let error=match sampler_uvi::assemble_translated_streamed(w_uvi_translated(&path,5),48000,&Default::default()) {Ok(_)=>panic!("unaccounted FLAC picture reached ordinary allocation"),Err(e)=>e};assert!(error.to_string().contains("wide UVI"));assert!(error.to_string().contains("picture/cuesheet"));assert_eq!((old.active_plan(),old.now(),old.voice_count()),before);
    fixture.file("incoming.flac",&f_flac(5,&[128,128],false,&[],value));let path=fixture.program("f-cancel.uvip","incoming.flac","");let calls=AtomicUsize::new(0);
    assert!(sampler_uvi::assemble_translated_streamed_cancelable(w_uvi_translated(&path,5),48000,&Default::default(),&||calls.fetch_add(1,Ordering::AcqRel)>=2).is_err());assert!(calls.load(Ordering::Acquire)>=3);assert_eq!((old.active_plan(),old.now(),old.voice_count()),before);
    for (target,id) in targets.iter().zip(ids) {assert_eq!(old.sample_target_source_storage(*target).unwrap().id(),id);}
    let mut out=[[0.;2];64];support::without_heap(||old.render(&mut out).unwrap());assert!(out.iter().flatten().any(|v|*v!=0.));
}
