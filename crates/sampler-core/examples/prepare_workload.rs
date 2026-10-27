//! Off-audio preparation after one immutable PCM validation. Run in release mode.
use sampler_core::{Envelope, Pcm, Playback, Prepared, Region};
use std::{hint::black_box, time::Instant};

fn main() {
    if cfg!(debug_assertions) {
        eprintln!("run with --release");
        std::process::exit(1);
    }
    println!("frames,pcm_bytes,validation_us,prepare_median_us,prepare_p99_us");
    for frames in [1, 4096, 1 << 20] {
        let data = vec![[0.25; 2]; frames].into_boxed_slice();
        let begin = Instant::now();
        let pcm = Pcm::new(48000, data).unwrap();
        let validation = begin.elapsed().as_secs_f64() * 1_000_000.0;
        let mut times = [0u128; 256];
        for i in 0..288 {
            let begin = Instant::now();
            let plan = Prepared::new(
                48000,
                vec![pcm.clone()],
                vec![Region {
                    sample: 0,
                    key_low: 60,
                    key_high: 60,
                    root_key: None,
                    velocity_low: 0.0,
                    velocity_high: 1.0,
                    gain: 1.0,
                    envelope: Envelope::default(),
                    playback: Playback::default(),
                }],
                1,
            )
            .unwrap();
            let ns = begin.elapsed().as_nanos();
            assert_eq!(black_box(&plan).sample_count(), 1);
            if i >= 32 {
                times[i - 32] = ns
            }
            drop(plan);
        }
        times.sort_unstable();
        println!(
            "{frames},{},{validation:.3},{:.3},{:.3}",
            frames * std::mem::size_of::<sampler_core::Frame>(),
            times[128] as f64 / 1000.0,
            times[253] as f64 / 1000.0
        );
    }
}
