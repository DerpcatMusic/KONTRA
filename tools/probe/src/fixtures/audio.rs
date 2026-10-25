//! The fixture samples: a square wave at the note's pitch, written as 16-bit mono WAV.
//!
//! A square wave never crosses zero between frames, so a voice that starts at any
//! source offset is audible on its first rendered frame. That is what the probe's
//! mixed-output onset measurement needs: a sine could start at a zero crossing and
//! report a one-frame error that the engine did not introduce.

use std::f32::consts::TAU;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

/// Sample rate of every fixture's WAV files.
pub(super) const RATE: u32 = 48_000;
/// Length of the fade-out at the end of every sample, in frames.
const RELEASE: usize = (RATE / 100) as usize;

/// Writes `samples` as a mono 16-bit PCM WAV file.
pub(super) fn write_wav(path: &Path, samples: &[f32]) -> Result<()> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend(b"RIFF");
    out.extend((36 + data).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // PCM
    out.extend(1u16.to_le_bytes()); // mono
    out.extend(RATE.to_le_bytes());
    out.extend((RATE * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(data.to_le_bytes());
    for sample in samples {
        out.extend(((sample.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    let mut file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    file.write_all(&out)
        .with_context(|| format!("writing {}", path.display()))
}

/// A square wave at the pitch of `midi` (A3 = 69 is 440 Hz), `amp` high, with
/// `noise` of hiss, a 10 ms release and a full-level first frame.
pub(super) fn tone(midi: u8, seconds: f32, amp: f32, noise: f32, seed: u32) -> Vec<f32> {
    let hz = 440.0 * 2f32.powf((f32::from(midi) - 69.0) / 12.0);
    let frames = (seconds * RATE as f32) as usize;
    let mut state = seed;
    (0..frames)
        .map(|i| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let hiss = (state >> 8) as f32 / (1 << 23) as f32 - 1.0;
            let wave = if (TAU * hz * i as f32 / RATE as f32).cos() < 0.0 {
                -1.0
            } else {
                1.0
            };
            let fade = ((frames - i) as f32 / RELEASE as f32).min(1.0);
            (amp * wave + noise * hiss) * fade
        })
        .collect()
}
