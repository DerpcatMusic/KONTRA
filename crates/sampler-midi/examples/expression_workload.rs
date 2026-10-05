//! Run in release mode on an idle pinned CPU. Measures a whole-zone pitch gesture
//! separately from PCM filtering; preparation, note admission and rendering are untimed.
use sampler_core::{Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime};
use sampler_midi::{Applied, Mpe, Packets, Zone};
use std::{hint::black_box, time::Instant};

fn main() {
    println!("notes,reserved,median_ns,p99_ns,max_ns,median_ns_per_note");
    for (notes, reserved) in [(64, 64), (256, 256), (1024, 1024), (64, 4096)] {
        let plan = Prepared::new(
            48000,
            vec![Pcm {
                rate: 48000,
                frames: vec![[1.0; 2]; 64].into_boxed_slice(),
            }],
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
        let mut runtime = Runtime::new(
            plan,
            Limits {
                notes: reserved,
                channels: 1,
                expressions: reserved,
                families: reserved,
                voices: reserved,
                commands: 0,
                behaviors: 0,
                behavior_fuel: 0,
                behavior_cells: 0,
            },
        )
        .unwrap();
        let mut mpe = Mpe::new(&runtime, 0, 0, Zone::Lower, 1, reserved).unwrap();
        let note_words = [0x2091_3c7f];
        let note = Packets::new(&note_words).next().unwrap().unwrap();
        for _ in 0..notes {
            assert!(matches!(
                mpe.apply(&mut runtime, note),
                Ok(Applied::Started(_))
            ));
        }
        let center = [0x20e0_0040];
        let above = [0x20e0_0140];
        let packets = [
            Packets::new(&center).next().unwrap().unwrap(),
            Packets::new(&above).next().unwrap().unwrap(),
        ];
        let mut times = Vec::with_capacity(2048);
        for i in 0..2176 {
            let start = Instant::now();
            let result = black_box(&mut mpe).apply(black_box(&mut runtime), packets[i % 2]);
            let ns = start.elapsed().as_nanos();
            assert_eq!(result, Ok(Applied::Expression { owners: notes }));
            if i >= 128 {
                times.push(ns)
            }
        }
        times.sort_unstable();
        let median = times[times.len() / 2];
        println!(
            "{notes},{reserved},{median},{},{},{}",
            times[times.len() * 99 / 100],
            times[times.len() - 1],
            median as f64 / notes as f64
        );
        runtime.panic();
        runtime.flush_ended(|_| true);
        assert_eq!((runtime.note_count(), runtime.voice_count()), (0, 0));
    }
}
