//! Ordinary encoded-source -> complete immutable resident storage component.
//! No hand-built decoded packs are used as the end-to-end witness.
use sampler_core::{Frame, SourceStorage};
use sampler_kontakt::EncodedChannelReader;
use std::io::{self, Cursor};
use symphonia::core::meta::MetadataOptions;

const FRAMES: usize = 6000;
const RATE: u32 = 48000;
fn sample(frame: usize, channel: usize) -> i16 {
    ((frame * 47 + channel * 1151) % 8191) as i16 - 4095
}
fn expected(frame: usize, channel: usize) -> f32 {
    f32::from(sample(frame, channel)) / 32768.0
}
fn wav(channels: usize) -> Vec<u8> {
    let data = FRAMES * channels * 2;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&(channels as u16).to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels as u16 * 2).to_le_bytes());
    out.extend_from_slice(&16_u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data as u32).to_le_bytes());
    for frame in 0..FRAMES {
        for channel in 0..channels {
            out.extend_from_slice(&sample(frame, channel).to_le_bytes());
        }
    }
    out
}
fn reader(bytes: Vec<u8>) -> EncodedChannelReader {
    EncodedChannelReader::open_legacy(Box::new(Cursor::new(bytes)), &MetadataOptions::default())
        .unwrap()
}
fn required(channels: usize) -> usize {
    channels.div_ceil(2) * FRAMES * size_of::<Frame>()
}
fn verify(storage: &SourceStorage, original_channels: usize) {
    let source = storage.as_source();
    let output_channels = original_channels.max(2);
    assert_eq!(
        (
            source.channels().get(),
            source.sample_rate(),
            source.frame_count()
        ),
        (output_channels, RATE, FRAMES)
    );
    let view = source.resident_view().unwrap();
    // Inspect every sample, including all normal packet boundaries and the end.
    for frame in 0..FRAMES {
        for channel in 0..output_channels {
            let ordinal = if original_channels == 1 { 0 } else { channel };
            assert_eq!(
                view.sample(channel, frame).unwrap().to_bits(),
                expected(frame, ordinal).to_bits(),
                "original channel {ordinal}, frame {frame}"
            );
        }
    }
}

#[test]
fn normal_six_channel_source_produces_complete_family_and_original_ordinals() {
    let mut input = reader(wav(6));
    let storage = input.read_resident_storage(required(6)).unwrap();
    assert!(matches!(storage, SourceStorage::Channels(_)));
    assert!(storage.as_source().stereo().is_none());
    verify(&storage, 6);
    let mut ids = Vec::new();
    storage.as_source().physical_dependencies(|pcm| {
        assert!(pcm.requires_resident());
        assert_eq!((pcm.sample_rate(), pcm.frame_count()), (RATE, FRAMES));
        assert!(pcm.resident_frames().is_some());
        assert!(!ids.contains(&pcm.asset_id()));
        ids.push(pcm.asset_id());
    });
    assert_eq!(ids.len(), 3);
    let first_id = storage.as_source().id();
    let another = reader(wav(6)).read_resident_storage(required(6)).unwrap();
    assert_ne!(first_id, another.as_source().id());
}

#[test]
fn normal_mono_stereo_producer_preserves_existing_stereo_contract_exactly() {
    for channels in [1, 2] {
        let mut input = reader(wav(channels));
        let storage = input.read_resident_storage(required(channels)).unwrap();
        assert!(matches!(storage, SourceStorage::Stereo(_)));
        assert!(storage.as_source().stereo().is_some());
        verify(&storage, channels);
        let mut count = 0;
        storage.as_source().physical_dependencies(|_| count += 1);
        assert_eq!(count, 1);
    }
}

#[test]
fn normal_odd_channel_family_zeros_only_the_unused_physical_lane() {
    let storage = reader(wav(3)).read_resident_storage(required(3)).unwrap();
    verify(&storage, 3);
    let mut packs = 0;
    storage.as_source().physical_dependencies(|pcm| {
        if packs == 1 {
            assert!(
                pcm.resident_frames()
                    .unwrap()
                    .iter()
                    .all(|frame| frame[1].to_bits() == 0)
            );
        }
        packs += 1;
    });
    assert_eq!(packs, 2);
    assert!(
        storage
            .as_source()
            .resident_view()
            .unwrap()
            .sample(3, 0)
            .is_none()
    );
}

#[test]
fn resident_pcm_limit_preflight_refuses_then_exact_limit_retries() {
    for channels in [1, 2, 3, 6] {
        let mut input = reader(wav(channels));
        assert_eq!(
            input.read_resident_storage(0).unwrap_err().kind(),
            io::ErrorKind::OutOfMemory
        );
        assert_eq!(
            input
                .read_resident_storage(required(channels) - 1)
                .unwrap_err()
                .kind(),
            io::ErrorKind::OutOfMemory
        );
        // Same real reader remains usable after both preflight refusals.
        let mut original = vec![0.0; channels];
        input.read_interleaved(0, &mut original).unwrap();
        for (channel, actual) in original.iter().enumerate() {
            assert_eq!(*actual, expected(0, channel));
        }
        let storage = input.read_resident_storage(required(channels)).unwrap();
        verify(&storage, channels);
    }
}

#[test]
fn truncated_late_encoded_payload_never_returns_a_partial_storage_family() {
    let mut bytes = wav(6);
    // Header declares 6000 frames, but the normal source ends partway through
    // a later packet. Early packets are valid; no padded or injected PCM.
    bytes.truncate(44 + 3000 * 6 * 2);
    let mut input = reader(bytes);
    let mut first = [0.0; 6];
    input.read_interleaved(0, &mut first).unwrap();
    assert_eq!(first, std::array::from_fn(|channel| expected(0, channel)));
    assert!(input.read_resident_storage(required(6)).is_err());
    // A fresh correct source of the same logical geometry succeeds normally.
    let storage = reader(wav(6)).read_resident_storage(required(6)).unwrap();
    verify(&storage, 6);
}
