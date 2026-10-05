//! Synthetic resident-PCM microbenchmark, not a DAW or competitor benchmark.
use sampler_core::{Envelope, Input, Limits, Loop, LoopMode, Pcm, Playback, Protocol, Runtime};
use std::{hint::black_box, time::Instant};

fn main() {
    const BLOCK: usize = 64;
    const BLOCKS: usize = 2000;
    let mode = std::env::args().nth(1);
    let envelope = match mode.as_deref() {
        None | Some("--loop") => Envelope::default(),
        Some("--envelope") => Envelope::new((BLOCK * (BLOCKS + 100)) as u32, 0, 0, 1.0, 0).unwrap(),
        _ => panic!("usage: render_bench [--envelope | --loop]"),
    };
    let playback = Playback {
        loop_range: (mode.as_deref() == Some("--loop")).then_some(Loop {
            start: 0,
            end: 127,
            mode: LoopMode::Continuous,
        }),
        ..Playback::default()
    };
    let pcm = vec![[0.001, -0.001]; BLOCK * (BLOCKS + 100)];
    let samples = [Pcm {
        rate: 48000,
        frames: pcm.into_boxed_slice(),
    }];
    println!("active,capacity,p50_us,p99_us,max_us,deadline_misses,checksum");
    for (active, capacity) in [(16, 64), (16, 4096), (256, 256)] {
        let mut rt = fixture_runtime(
            48000,
            &samples,
            Limits {
                notes: 1,
                channels: 4,
                families: capacity,
                expressions: 1,
                voices: capacity,
                commands: 0,
                behaviors: 0,
                behavior_fuel: 0,
            },
        )
        .unwrap();
        let note = rt
            .note_on(
                Input {
                    protocol: Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 60,
                    external_id: None,
                },
                60,
                1.0,
            )
            .unwrap();
        for _ in 0..active {
            let family = rt.create_family(note).unwrap();
            rt.start_family(family, 0, 0, 1.0, envelope, playback)
                .unwrap();
            rt.finish_family(family).unwrap();
        }
        let mut audio = [[0.0; 2]; BLOCK];
        for _ in 0..100 {
            rt.render(black_box(&mut audio)).unwrap();
        }
        let mut times = Vec::with_capacity(BLOCKS);
        let mut checksum = 0.0f64;
        for _ in 0..BLOCKS {
            let start = Instant::now();
            rt.render(black_box(&mut audio)).unwrap();
            times.push(start.elapsed().as_nanos() as u64);
            checksum += f64::from(black_box(audio[0][0]));
        }
        times.sort_unstable();
        let misses = times
            .iter()
            .filter(|&&n| n > 1_000_000_000 * BLOCK as u64 / 48000)
            .count();
        println!(
            "{active},{capacity},{:.3},{:.3},{:.3},{misses},{checksum:.9}",
            times[BLOCKS / 2] as f64 / 1000.0,
            times[BLOCKS * 99 / 100] as f64 / 1000.0,
            times[BLOCKS - 1] as f64 / 1000.0
        );
    }
}

fn fixture_runtime(rate: u32, pcm: &[Pcm], limits: Limits) -> Result<Runtime, sampler_core::Error> {
    Runtime::new(
        sampler_core::Prepared::new(rate, pcm.to_vec(), Vec::new(), 0)?,
        limits,
    )
}
