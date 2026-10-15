//! Authored serial wire fixture, real sample path and meaningful KSP callbacks.
#[path = "../../../../crates/sampler-kontakt/tests/support/chunks.rs"]
mod wire;
use wire::{chunk, object, sized};
fn wide(s: &str) -> Vec<u8> {
    let mut out = (s.encode_utf16().count() as u32).to_le_bytes().to_vec();
    out.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
    out
}
pub fn directory() -> tempfile::TempDir {
    let root = std::env::var_os("KONTRA_TEST_SCRATCH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&root).unwrap();
    tempfile::Builder::new()
        .prefix("onset-loader-authored-")
        .tempdir_in(root)
        .unwrap()
}
pub fn wav(path: &std::path::Path) {
    wav_frames(path, 4096);
}
pub fn wav_frames(path: &std::path::Path, frames: usize) {
    let mut wav = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for frame in 0..frames {
        wav.write_sample(if frame < 32 { 0f32 } else { 0.25f32 })
            .unwrap();
    }
    wav.finalize().unwrap();
}
pub fn nki(root: &std::path::Path, script: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(root.join("Samples")).unwrap();
    std::fs::create_dir_all(root.join("Resources")).unwrap();
    wav(&root.join("owned.wav"));
    let mut group = wide("Owned modulation");
    for v in [1f32, 0., 1.] {
        group.extend(v.to_le_bytes());
    }
    group.extend([1, 0, 0, 0]);
    group.extend(0i32.to_le_bytes());
    group.extend((-1i16).to_le_bytes());
    group.extend([0; 10]);
    group.extend(0i32.to_le_bytes());
    let slots = vec![0; 16];
    let mut children = chunk(0x38, &0u32.to_le_bytes());
    children.extend(chunk(0x3b, &object(0x10, &[], &slots, &[])));
    let mut groups = 1u32.to_le_bytes().to_vec();
    groups.extend(object(0x95, &[], &group, &children));

    let mut zone = vec![0; 12];
    for v in [1i16, 127, 60, 60, 0, 0, 0, 0, 60] {
        zone.extend(v.to_le_bytes());
    }
    for v in [1f32, 0., 1.] {
        zone.extend(v.to_le_bytes());
    }
    zone.extend([0, 1, 0, 0, 0, 0]);
    zone.extend(0i32.to_le_bytes());
    zone.extend(0i32.to_le_bytes());
    zone.extend(48000i32.to_le_bytes());
    zone.push(1);
    for v in [4096i32, 0, 60] {
        zone.extend(v.to_le_bytes());
    }
    zone.extend(0f32.to_le_bytes());
    zone.push(0);
    zone.extend(0i32.to_le_bytes());
    let mut zones = 1u32.to_le_bytes().to_vec();
    zones.extend(0u32.to_le_bytes());
    zones.extend(object(0x9a, &[], &zone, &[]));
    let mut public = wide("Owned regression");
    public.extend(0f64.to_le_bytes());
    public.push(0);
    for v in [1f32, 0., 1.] {
        public.extend(v.to_le_bytes());
    }
    public.extend([1, 127, 0, 127]);
    public.extend((-1i16).to_le_bytes());
    public.extend([0; 16]);
    public.push(0);
    public.extend(0i32.to_le_bytes());
    for _ in 0..3 {
        public.extend(wide(""));
    }
    public.extend([0; 6]);
    let mut children = chunk(0x33, &groups);
    children.extend(chunk(0x34, &zones));
    {
        let source = script;
        let mut public = sized(source.as_bytes());
        public.extend([0, 0, 0]);
        public.extend(sized(&[])); // No password.
        public.extend(sized(&[])); // Empty description.
        public.extend(u32::MAX.to_le_bytes()); // No linked script.
        public.extend(0u32.to_le_bytes()); // No saved variables.
        children.extend(chunk(6, &object(0x60, &[], &public, &[])));
    }
    let mut payload = chunk(0x28, &object(0xae, &[], &public, &children));
    let mut table = vec![0; 4];
    table.extend(1u32.to_le_bytes());
    table.extend(1i32.to_le_bytes());
    table.push(4);
    table.extend(wide("owned.wav"));
    table.extend([0; 12]);
    payload.extend(chunk(0x3d, &table));
    // Authored literal FastLZ runs; same independent framing as monolith.rs.
    let packed: Vec<_> = payload
        .chunks(32)
        .flat_map(|b| std::iter::once((b.len() - 1) as u8).chain(b.iter().copied()))
        .collect();
    let mut nks = vec![0; 222];
    nks[..4].copy_from_slice(&0x7fa89012u32.to_le_bytes());
    nks[4..8].copy_from_slice(&(packed.len() as u32).to_le_bytes());
    nks[8..10].copy_from_slice(&0x110u16.to_le_bytes());
    nks[10..14].copy_from_slice(&0xea37631au32.to_le_bytes());
    nks[186..190].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    nks.extend(packed);
    nks.extend(0xb00ee1aeu32.to_le_bytes());
    nks.extend([1, 1, 12, 0]);
    std::fs::write(root.join("owned.nki"), nks).unwrap();
    root.join("owned.nki")
}

/// Two independently decoded assets selected by one real Runtime note.
/// Completion order is controlled by the test, never by sleeps or a decoder.
pub(super) fn streamed_layers(
    traced: bool,
    trace_capacity: usize,
) -> (
    super::V2Core,
    Option<super::Owner>,
    sampler_core::StreamWorker,
    [sampler_core::Pcm; 2],
) {
    use sampler_core::{Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime, StreamCache};
    let assets = std::array::from_fn(|_| Pcm::streamed(48000, 512).unwrap());
    assert_ne!(assets[0].asset_id(), assets[1].asset_id());
    let regions = (0..2)
        .map(|sample| Region {
            sample,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        })
        .collect();
    let plan = Prepared::new(48000, assets.to_vec(), regions, 2).unwrap();
    let limits = Limits::for_plan(&plan, 8, 16);
    let (plan, reader) = if traced {
        let (plan, reader) = plan.with_signal_trace_reader(trace_capacity).unwrap();
        (plan, Some(reader))
    } else {
        (plan, None)
    };
    let (cache, worker) = StreamCache::new(2).unwrap();
    let mut runtime = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
    runtime.set_cold_starts(true);
    let mut part = super::Part::new(
        runtime,
        super::super::super::MixTree::instrument("authored two asset cold onset"),
    )
    .unwrap();
    part.epoch = 109;
    part.horizon = Some(128);
    for (index, pcm) in assets.iter().enumerate() {
        part.waveform_sources.insert(
            index as u32,
            crate::sound::waveform::Source {
                pcm: pcm.clone(),
                stream: None,
            },
        );
    }
    let plan = part.runtime.active_plan();
    let mut core = super::V2Core::with_parts(1, 48000.);
    drop(crate::sound::Core::install(
        &mut core,
        0,
        Some(Box::new(part)),
    ));
    (
        core,
        reader.map(|reader| super::Owner {
            epoch: 109,
            plan,
            reader,
            canceled: false,
        }),
        worker,
        assets,
    )
}
