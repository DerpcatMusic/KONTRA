//! Unwired stage-one acceptance source. NOT_RUN / NOT_COMPILED.
//! CURRENT13 is expected to FAIL the multichannel rejection test.
//! Move into sampler-kontakt's integration tests only with separate authorization.
use sampler_kontakt::{SampleReader, Samples, decode};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn chunk(id: &[u8; 4], body: &[u8], big: bool) -> Vec<u8> {
    let mut out = id.to_vec();
    let size = body.len() as u32;
    out.extend(if big {
        size.to_be_bytes()
    } else {
        size.to_le_bytes()
    });
    out.extend(body);
    if body.len() % 2 != 0 {
        out.push(0);
    }
    out
}

// Each channel has a distinct signed PCM16 marker. The tail-only case makes
// first-two-channel selection erase all nonzero audio, rather than downmix it.
fn fixture(channels: u16, aiff: bool, tail_only: bool) -> Vec<u8> {
    let mut pcm = Vec::new();
    for frame in 0..8 {
        for channel in 0..channels {
            let value = if tail_only && channel < 2 {
                0i16
            } else {
                (channel as i16 + 1) * 1024 + frame * 16
            };
            pcm.extend(if aiff {
                value.to_be_bytes()
            } else {
                value.to_le_bytes()
            });
        }
    }
    let mut body;
    if aiff {
        let mut comm = channels.to_be_bytes().to_vec();
        comm.extend(8u32.to_be_bytes());
        comm.extend(16u16.to_be_bytes());
        // Exact 48000 Hz in AIFF's 80-bit extended representation.
        comm.extend([0x40, 0x0e, 0xbb, 0x80, 0, 0, 0, 0, 0, 0]);
        let mut ssnd = vec![0; 8];
        ssnd.extend(pcm);
        body = b"AIFF".to_vec();
        body.extend(chunk(b"COMM", &comm, true));
        body.extend(chunk(b"SSND", &ssnd, true));
    } else {
        let mut fmt = 1u16.to_le_bytes().to_vec();
        fmt.extend(channels.to_le_bytes());
        fmt.extend(48000u32.to_le_bytes());
        fmt.extend((48000u32 * u32::from(channels) * 2).to_le_bytes());
        fmt.extend((channels * 2).to_le_bytes());
        fmt.extend(16u16.to_le_bytes());
        body = b"WAVE".to_vec();
        body.extend(chunk(b"fmt ", &fmt, false));
        body.extend(chunk(b"data", &pcm, false));
    }
    chunk(if aiff { b"FORM" } else { b"RIFF" }, &body, aiff)
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "multichannel-admission-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn stage_one_rejects_multichannel_before_full_decode_or_stream_open() {
    let root = Scratch::new();
    for aiff in [false, true] {
        for channels in [3, 4, 6, 8] {
            let bytes = fixture(channels, aiff, true);
            assert!(decode(&bytes).is_err(), "{channels} channels; AIFF={aiff}");
            let path = root.0.join(if aiff { "sample.aiff" } else { "sample.wav" });
            std::fs::write(&path, bytes).unwrap();
            let mut samples = Samples::new(&root.0);
            assert!(samples.decode(&path).is_err());
            let source = samples.source(&path).unwrap();
            assert!(SampleReader::open(&source).is_err());
        }
    }
}

#[test]
fn stage_one_preserves_mono_stereo_identity_and_random_frame_offsets() {
    let root = Scratch::new();
    for aiff in [false, true] {
        for channels in [1, 2] {
            let bytes = fixture(channels, aiff, false);
            let expected: Vec<_> = (0..8)
                .map(|frame| {
                    let first = (1024 + frame * 16) as f32 / 32768.0;
                    let last = (channels as i32 * 1024 + frame * 16) as f32 / 32768.0;
                    [first, last]
                })
                .collect();
            let decoded = decode(&bytes).unwrap();
            assert_eq!(decoded.rate, 48000);
            assert_eq!(decoded.frames, expected);
            let path = root.0.join(if aiff { "sample.aiff" } else { "sample.wav" });
            std::fs::write(&path, bytes).unwrap();
            let mut samples = Samples::new(&root.0);
            let source = samples.source(&path).unwrap();
            let mut reader = SampleReader::open(&source).unwrap();
            for start in [5, 0, 2] {
                let mut out = [[0.0; 2]; 2];
                reader.read(start, &mut out).unwrap();
                assert_eq!(out.as_slice(), &expected[start..start + 2]);
            }
        }
    }
}

#[test]
fn malformed_extents_remain_errors_not_multichannel_successes() {
    for aiff in [false, true] {
        let mut bytes = fixture(2, aiff, false);
        bytes.pop();
        assert!(decode(&bytes).is_err());
    }
}
