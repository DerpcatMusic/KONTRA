//! Private raw-sample preview preparation. Off audio; no note/script/Player.
//! Host-rate conversion is cubic Catmull-Rom, not bandlimited or native parity.
use crate::{audio::{Frame, Source}, cache::{self, Dependency}};
use anyhow::{Context, Result, ensure};

pub(crate) const MAX_OUTPUT_BYTES: usize = 32 << 20;
const MAX_SOURCE_BYTES: usize = 32 << 20;
const MAX_ENCODED_BYTES: u64 = 16 << 20;
const CHUNK_FRAMES: usize = 4096;

pub(crate) struct DecodedPreview {
    pub frames: Box<[Frame]>,
    pub source_rate: u32,
    pub source_frames: u64,
    /// Kontakt's codec-declared count if available; UVI's exact decoded count.
    pub source_channels: Option<usize>,
}
#[derive(Debug)]
pub(crate) struct PreviewCanceled;
impl std::fmt::Display for PreviewCanceled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("Sample preview canceled") }
}
impl std::error::Error for PreviewCanceled {}
fn check_cancel(canceled: &dyn Fn() -> bool) -> Result<()> {
    if canceled() { return Err(PreviewCanceled.into()); }
    Ok(())
}
pub(crate) fn is_canceled(error: &anyhow::Error) -> bool { error.is::<PreviewCanceled>() }
#[derive(Debug)]
pub(crate) struct PreviewUnsupported(pub &'static str);
impl std::fmt::Display for PreviewUnsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.0) }
}
impl std::error::Error for PreviewUnsupported {}
pub(crate) fn unsupported_reason(error: &anyhow::Error) -> Option<&'static str> {
    error.downcast_ref::<PreviewUnsupported>().map(|error| error.0)
}
fn classify_layout(error: anyhow::Error) -> anyhow::Error {
    if error.chain().any(|error| matches!(error.to_string().as_str(),
        "Only mono/stereo samples are supported" |
        "Image wavetable resources cannot be loaded through audio callbacks")) {
        PreviewUnsupported("Raw sample preview supports mono/stereo audio only").into()
    } else { error }
}

fn output_frames(source_frames: u64, source_rate: u32, host_rate: u32) -> Result<usize> {
    ensure!((8000..=192000).contains(&host_rate), "Sample preview needs a host rate in 8..192 kHz");
    ensure!(source_frames > 0 && source_rate > 0, "Invalid sample preview dimensions");
    ensure!(source_frames <= (MAX_SOURCE_BYTES / size_of::<Frame>()) as u64,
        "Sample preview decoded source exceeds limit");
    let numerator = source_frames.checked_mul(u64::from(host_rate))
        .context("Sample preview duration overflow")?;
    let frames = numerator.div_ceil(u64::from(source_rate));
    let frames = usize::try_from(frames).context("Sample preview duration exceeds address space")?;
    ensure!(frames > 0 && frames <= MAX_OUTPUT_BYTES / size_of::<Frame>(),
        "Sample preview host-rate output exceeds limit");
    Ok(frames)
}

fn convert(source_frames: u64, source_rate: u32, host_rate: u32,
    mut at: impl FnMut(u64) -> Result<Frame>, canceled: &dyn Fn() -> bool) -> Result<Box<[Frame]>> {
    check_cancel(canceled)?;
    let frames = output_frames(source_frames, source_rate, host_rate)?;
    let mut output = Vec::new();
    output.try_reserve_exact(frames)?;
    for frame in 0..frames {
        if frame.is_multiple_of(256) { check_cancel(canceled)?; }
        let value = if source_rate == host_rate {
            // Exact physical source bits, including signed zero; no interpolation.
            at(frame as u64)?
        } else {
            let position = (frame as u64).checked_mul(u64::from(source_rate))
                .context("Sample preview position overflow")?;
            let index = position / u64::from(host_rate);
            let fraction = (position % u64::from(host_rate)) as f32 / host_rate as f32;
            let mut q = [[0.; 2]; 4];
            for (slot, input) in q.iter_mut().enumerate() {
                let index = match slot { 0 => index.checked_sub(1), _ => index.checked_add((slot - 1) as u64) };
                if let Some(index) = index.filter(|index| *index < source_frames) { *input = at(index)?; }
            }
            crate::engine::hermite(&q, fraction)
        };
        ensure!(value.iter().all(|value| value.is_finite()), "Nonfinite sample preview PCM");
        output.push(value);
    }
    check_cancel(canceled)?;
    Ok(output.into_boxed_slice())
}

/// Caller supplies an exact retained-zone Source, never an untrusted UI path.
/// Initial owner/final commit validation remains the coordinator's authority.
pub(crate) fn prepare_kontakt(source: &Source, dependencies: &[Dependency], host_rate: u32,
    canceled: &dyn Fn() -> bool) -> Result<DecodedPreview> {
    check_cancel(canceled)?;
    ensure!(cache::current(dependencies) && source.current(), "Sample preview source dependencies changed");
    let mut reader = source.open_bounded(MAX_ENCODED_BYTES, (MAX_SOURCE_BYTES / size_of::<Frame>()) as u64).map_err(classify_layout)?;
    let header = reader.header();
    let channels = reader.declared_channels();
    output_frames(header.frames, header.rate, host_rate)?;
    check_cancel(canceled)?;
    // Decode every declared physical frame before conversion, even when a
    // downsampling window would omit it. Existing reader EOF/cleanup applies.
    let source_frames = usize::try_from(header.frames)?;
    let mut source_pcm = Vec::new();
    source_pcm.try_reserve_exact(source_frames)?;
    source_pcm.resize(source_frames, [0.; 2]);
    for (index, chunk) in source_pcm.chunks_mut(CHUNK_FRAMES).enumerate() {
        check_cancel(canceled)?;
        reader.read((index * CHUNK_FRAMES) as u64, chunk).map_err(classify_layout)?;
        // Genuine codec failure above wins concurrently arriving cancellation.
        check_cancel(canceled)?;
    }
    let frames = convert(header.frames, header.rate, host_rate,
        |frame| Ok(source_pcm[usize::try_from(frame)?]), canceled)?;
    ensure!(cache::current(dependencies) && source.current(), "Sample preview source dependencies changed during decode");
    check_cancel(canceled)?;
    Ok(DecodedPreview { frames, source_rate: header.rate, source_frames: header.frames, source_channels: channels })
}

#[cfg(feature = "uvi")]
pub(crate) fn prepare_uvi(library: &crate::uvi::library::Library, program_member: &str,
    exact_resource: &str, host_rate: u32, canceled: &dyn Fn() -> bool) -> Result<DecodedPreview> {
    use crate::uvi::{library::resources, sample};
    check_cancel(canceled)?;
    ensure!((8000..=192000).contains(&host_rate), "Sample preview needs a host rate in 8..192 kHz");
    // UUID/content-bank/program/zone binding is checked by the ticket coordinator.
    let members = resources(&library.directory, program_member, exact_resource)?;
    if members.len() > 2 { return Err(PreviewUnsupported("Raw sample preview supports mono/stereo audio only").into()); }
    let decoded = match library.audio_preview(program_member, exact_resource, MAX_ENCODED_BYTES,
        MAX_SOURCE_BYTES, canceled) {
        Err(error) if error.is::<sample::LoadCancelled>() => return Err(PreviewCanceled.into()),
        other => other.map_err(classify_layout)?,
    };
    if decoded.wavetable_image || !(1..=2).contains(&decoded.channels) {
        return Err(PreviewUnsupported("Raw sample preview supports mono/stereo audio only").into());
    }
    ensure!(decoded.frames.checked_mul(decoded.channels) == Some(decoded.interleaved.len()),
        "Sample preview PCM dimensions differ");
    let source_frames = u64::try_from(decoded.frames)?;
    let frames = convert(source_frames, decoded.rate, host_rate, |frame| {
        let offset = usize::try_from(frame)?.checked_mul(decoded.channels)
            .context("Sample preview PCM position overflow")?;
        let left = decoded.interleaved.value(offset).context("Sample preview PCM ended early")?;
        let right = decoded.interleaved.value(offset + decoded.channels - 1)
            .context("Sample preview PCM ended early")?;
        Ok([left, right])
    }, canceled)?;
    // Ordinary replacement/in-place writes must fail before off-audio publication.
    library.validate_preview_source()?;
    check_cancel(canceled)?;
    Ok(DecodedPreview { frames, source_rate: decoded.rate, source_frames,
        source_channels: Some(decoded.channels) })
}

#[cfg(test)]
mod preparation_proof {
    use super::*;
    use std::{cell::Cell, io::Cursor, sync::atomic::{AtomicU64, Ordering}};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn fixture(channels: u16, rate: u32, values: &[[i16; 2]]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("kontra-raw-preview-{}-{}.wav",
            std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let spec = hound::WavSpec { channels, sample_rate: rate, bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int };
        let mut bytes = Vec::new();
        let mut writer = hound::WavWriter::new(Cursor::new(&mut bytes), spec).unwrap();
        for frame in values { for value in &frame[..usize::from(channels)] { writer.write_sample(*value).unwrap(); } }
        writer.finalize().unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }
    #[test]
    fn kontakt_equal_rate_known_physical_pcm_and_source_changes() {
        for channels in [1, 2] {
            let path = fixture(channels, 48000, &[[16384, -16384], [-32768, 32767]]);
            let source = crate::audio::Sources::default().source(&path).unwrap();
            let deps = cache::dependencies([path.clone()]);
            let prepared = prepare_kontakt(&source, &deps, 48000, &|| false).unwrap();
            assert_eq!(prepared.source_rate, 48000);
            assert_eq!(prepared.source_frames, 2);
            assert_eq!(prepared.source_channels, Some(usize::from(channels)));
            let right = if channels == 1 { [0.5, -1.] } else { [-0.5, 32767. / 32768.] };
            assert_eq!(&*prepared.frames, &[[0.5, right[0]], [-1., right[1]]]);
            let error = source.open_bounded(1, 1).err().unwrap();
            assert!(error.to_string().contains("encoded source exceeds limit"));
            // Length changes force the pinned source/version policy to reject.
            use std::io::Write;
            std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(&[0]).unwrap();
            assert!(prepare_kontakt(&source, &deps, 48000, &|| false).is_err());
            std::fs::remove_file(path).unwrap();
        }
    }
    #[test]
    fn kontakt_preview_header_bounds_precede_codec_and_offset_table_allocations() {
        let path = fixture(1, 48000, &[[123, 0]; 4]);
        let source = crate::audio::Sources::default().source(&path).unwrap();
        assert!(source.open_bounded(MAX_ENCODED_BYTES, 3).err().unwrap().to_string()
            .contains("decoded source exceeds limit"));
        // The existing instrument loader still accepts the four declared frames.
        assert_eq!(source.open().unwrap().header().frames, 4);
        std::fs::remove_file(&path).unwrap();
        let path = path.with_extension("ncw");
        for (frames, data_offset, expected) in [
            (u32::MAX, 120u32, "decoded source exceeds limit"),
            (1, u32::MAX, "offset table exceeds limit"),
        ] {
            let mut header = vec![0u8; 120];
            header[..8].copy_from_slice(&0x01A89ED631010000u64.to_be_bytes());
            header[8..10].copy_from_slice(&1u16.to_le_bytes());
            header[10..12].copy_from_slice(&16u16.to_le_bytes());
            header[12..16].copy_from_slice(&48000u32.to_le_bytes());
            header[16..20].copy_from_slice(&frames.to_le_bytes());
            header[20..24].copy_from_slice(&120u32.to_le_bytes());
            header[24..28].copy_from_slice(&data_offset.to_le_bytes());
            std::fs::write(&path, header).unwrap();
            let source = crate::audio::Sources::default().source(&path).unwrap();
            let error = source.open_bounded(MAX_ENCODED_BYTES, 4).err().unwrap();
            assert!(error.to_string().contains(expected), "{error:#}");
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn shared_cubic_known_edges_duration_exact_bypass_and_budget() {
        let source = [[0., -0.], [1., -1.], [0., 0.], [-1., 1.]];
        let equal = convert(4, 8000, 8000, |i| Ok(source[i as usize]), &|| false).unwrap();
        for (a, b) in equal.iter().zip(source) { assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits)); }
        let converted = convert(4, 8000, 16000, |i| Ok(source[i as usize]), &|| false).unwrap();
        let expected = [0., 0.5625, 1., 0.625, 0., -0.625, -1., -0.5625];
        assert_eq!(converted.len(), expected.len());
        for (frame, expected) in converted.iter().zip(expected) { assert_eq!(frame[0], expected); assert_eq!(frame[1], -expected); }
        assert_eq!(output_frames(3, 44100, 48000).unwrap(), 4);
        let calls = Cell::new(0);
        assert!(convert((MAX_SOURCE_BYTES / size_of::<Frame>() + 1) as u64, 8000, 8000,
            |_| { calls.set(calls.get() + 1); Ok([0.; 2]) }, &|| false).is_err());
        assert_eq!(calls.get(), 0);
        assert!(output_frames(u64::MAX, 1, 192000).is_err());
        assert!(output_frames(1, 0, 48000).is_err());
        assert!(output_frames(1, 48000, 7999).is_err());
    }
    #[test]
    fn preview_cancel_is_typed_and_never_reclassifies_genuine_failure() {
        let calls = Cell::new(0);
        let error = convert(512, 48000, 48000, |_| { calls.set(calls.get() + 1); Ok([0.; 2]) },
            &|| calls.get() >= 256).unwrap_err();
        assert!(is_canceled(&error));
        assert_eq!(calls.get(), 256);
        let stopped = Cell::new(false);
        let error = convert(1, 48000, 48000, |_| {
            stopped.set(true); anyhow::bail!("authored codec failure")
        }, &|| stopped.get()).unwrap_err();
        assert_eq!(error.to_string(), "authored codec failure");
        assert!(!is_canceled(&error));
        assert!(convert(1, 48000, 48000, |_| Ok([f32::NAN, 0.]), &|| false).is_err());
        let path = fixture(1, 44100, &vec![[16384, 0]; CHUNK_FRAMES + 1]);
        let source = crate::audio::Sources::default().source(&path).unwrap();
        let counter = Cell::new(0);
        let error = prepare_kontakt(&source, &[], 48000, &|| {
            let count = counter.get() + 1; counter.set(count); count >= 4
        }).err().unwrap();
        assert!(is_canceled(&error));
        std::fs::remove_file(path).unwrap();
    }
}
