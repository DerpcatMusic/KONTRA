//! Ignored CPU receipt for one plain resident sample voice: `render` over 64-frame blocks at 48 kHz.
//! Run: `nice -n 10 build-slot cargo test --release -p sampler-core --lib voice_receipt -- --ignored --nocapture`.
use super::*;
use crate::Envelope;
use std::{hint::black_box, time::Instant};

const BLOCK: usize = 64;
const BLOCKS: usize = 2048;
const FRAMES: usize = 1 << 22;
const REPEATS: usize = 9;

fn pcm() -> Vec<Frame> {
    (0..FRAMES)
        .map(|i| {
            let x = (i as f32 * 0.0137).sin() * 0.5;
            let n = ((i as u32).wrapping_mul(2_654_435_761) as f32 / u32::MAX as f32 - 0.5) * 0.05;
            [x + n, -x * 0.7 + n]
        })
        .collect()
}

/// Voices at `step` (phase `fraction`), each rendered for BLOCKS blocks; returns ns per voice-block.
fn measure(kernel: &Kernel, pcm: &[Frame], voices: usize, step: f64, fraction: f64) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..REPEATS {
        let mut cursors: Vec<Cursor> = (0..voices)
            .map(|_| {
                let mut c = Playback::default().cursor(FRAMES, 48000, 48000).unwrap();
                c.step = step;
                c.fraction = fraction;
                c
            })
            .collect();
        let mut envs = vec![EnvelopeState::new(Envelope::GATE); voices];
        let mut out = [[0.0_f32; 2]; BLOCK];
        let started = Instant::now();
        for _ in 0..BLOCKS {
            for (cursor, env) in cursors.iter_mut().zip(envs.iter_mut()) {
                let n = cursor.render(pcm, &mut out, env, 0.731, [0.7, -0.2], kernel);
                black_box(n);
            }
        }
        black_box(&out);
        let ns = started.elapsed().as_nanos() as f64 / (BLOCKS * voices) as f64;
        best = best.min(ns);
    }
    best
}

#[test]
#[ignore = "plain-voice CPU receipt; run --release with --nocapture"]
fn voice_receipt() {
    let pcm = pcm();
    let realtime = Kernel::new(crate::ResampleQuality::Realtime);
    let high = Kernel::new(crate::ResampleQuality::High);
    let scenarios: [(&str, &Kernel, f64, f64); 6] = [
        ("unity_copy      step=1.0  frac=0.0", &realtime, 1.0, 0.0),
        ("unity_cubic     step=1.0  frac=0.5", &realtime, 1.0, 0.5),
        ("down_cubic      step=0.5  frac=0.3", &realtime, 0.5, 0.3),
        ("up_polyphase    step=1.37 frac=0.2", &realtime, 1.37, 0.2),
        ("up_shortsinc    step=3.0  frac=0.2", &realtime, 3.0, 0.2),
        ("high_longsinc   step=1.37 frac=0.2", &high, 1.37, 0.2),
    ];
    for (name, kernel, step, fraction) in scenarios {
        for voices in [1, 16, 64, 256] {
            let ns = measure(kernel, &pcm, voices, step, fraction);
            println!(
                "voice_receipt {name} voices={voices:<3} ns/voice-block={ns:>9.1} us/voice-block={:.3} ns/frame={:.2}",
                ns / 1000.0,
                ns / BLOCK as f64
            );
        }
    }
}
