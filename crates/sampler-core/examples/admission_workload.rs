//! Prepared four-layer note lifecycles, timed by phase. Run with --release.
use sampler_core::{Envelope, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, Runtime};
use std::{hint::black_box, time::Instant};

fn main() {
    if cfg!(debug_assertions) {
        eprintln!("run with --release");
        std::process::exit(1);
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    let identified = match args.as_slice() {
        [] => false,
        [flag] if flag == "--ids" => true,
        _ => {
            eprintln!("usage: admission_workload [--ids]");
            std::process::exit(1);
        }
    };
    println!(
        "external_ids,notes,voices,reserved_voices,median_us,p99_us,note_off_median_us,note_off_p99_us,retire_median_us,retire_p99_us"
    );
    for (notes, reserved) in [(16, 64), (64, 256), (256, 1024), (1024, 4096), (16, 4096)] {
        let plan = Prepared::new(
            48000,
            vec![Pcm::new(48000, Box::from([[0.125; 2]; 8])).unwrap()],
            vec![
                Region {
                    sample: 0,
                    key_low: 60,
                    key_high: 60,
                    root_key: Some(60),
                    velocity_low: 0.0,
                    velocity_high: 1.0,
                    gain: 1.0,
                    envelope: Envelope::default(),
                    playback: Playback::default(),
                };
                4
            ],
            4,
        )
        .unwrap();
        let mut rt = Runtime::new(
            plan,
            Limits {
                notes: reserved / 4,
                channels: 1,
                families: reserved / 4,
                expressions: reserved / 4,
                voices: reserved,
                commands: 0,
                behaviors: 0,
                behavior_fuel: 0,
                behavior_cells: 0,
            },
        )
        .unwrap();
        let input = Input {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
            key: 60,
            external_id: None,
        };
        let mut times = [0u128; 128];
        let mut releases = times;
        let mut retirements = times;
        for iteration in 0..160 {
            let begin = Instant::now();
            for note in 0..notes {
                let input = Input {
                    external_id: identified.then_some(note as i32),
                    ..input
                };
                black_box(rt.trigger(input, 60, 1.0).unwrap());
            }
            let elapsed = begin.elapsed().as_nanos();
            assert_eq!((rt.note_count(), rt.voice_count()), (notes, notes * 4));
            if iteration >= 32 {
                times[iteration - 32] = elapsed;
            }
            let begin = Instant::now();
            for note in 0..notes {
                black_box(
                    rt.note_off(Input {
                        external_id: identified.then_some(note as i32),
                        ..input
                    })
                    .unwrap(),
                );
            }
            let elapsed = begin.elapsed().as_nanos();
            assert_eq!((rt.note_count(), rt.voice_count()), (notes, 0));
            if iteration >= 32 {
                releases[iteration - 32] = elapsed;
            }
            let mut ended = 0;
            let begin = Instant::now();
            rt.flush_ended(|_| {
                ended += 1;
                true
            });
            let elapsed = begin.elapsed().as_nanos();
            if iteration >= 32 {
                retirements[iteration - 32] = elapsed;
            }
            assert_eq!((ended, rt.note_count(), rt.voice_count()), (notes, 0, 0));
        }
        times.sort_unstable();
        releases.sort_unstable();
        retirements.sort_unstable();
        println!(
            "{identified},{notes},{},{reserved},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}",
            notes * 4,
            times[64] as f64 / 1000.0,
            times[126] as f64 / 1000.0,
            releases[64] as f64 / 1000.0,
            releases[126] as f64 / 1000.0,
            retirements[64] as f64 / 1000.0,
            retirements[126] as f64 / 1000.0,
        );
    }
}
