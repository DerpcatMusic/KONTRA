//! Run in release mode on an idle pinned CPU. Measures a whole-zone pitch gesture
//! separately from PCM filtering; preparation, note admission and rendering are untimed.
use sampler_core::{
    Destination, Envelope, ExpressionSource, Limits, Modulation, Pcm, Playback, Prepared, Region,
    Route, Runtime,
};
use sampler_midi::{Applied, Mpe, Packets, Zone};
use std::{hint::black_box, time::Instant};

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let route_count = match args.as_slice() {
        [] => 0,
        [flag] if flag == "--modulated" => 3,
        _ => panic!("usage: expression_workload [--modulated]"),
    };
    println!("routes,control,notes,reserved,median_ns,p99_ns,max_ns,median_ns_per_note");
    for (control, center, above) in [
        ("pitch", 0x20e0_0040, 0x20e0_0140),
        ("pressure", 0x20d0_0000, 0x20d0_7f00),
        ("timbre", 0x20b0_4a40, 0x20b0_4a41),
    ] {
        for (notes, reserved) in [(64, 64), (256, 256), (1024, 1024), (64, 4096)] {
            let plan = Prepared::new(
                48000,
                vec![Pcm::new(48000, vec![[1.0; 2]; 64].into_boxed_slice()).unwrap()],
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
            let routes = if route_count == 0 {
                vec![]
            } else {
                vec![
                    Route {
                        source: ExpressionSource::Pressure,
                        destination: Destination::LinearGain {
                            zero: 0.0,
                            one: 1.0,
                        },
                    },
                    Route {
                        source: ExpressionSource::Timbre,
                        destination: Destination::StereoBalance {
                            zero: -0.25,
                            one: 0.25,
                        },
                    },
                    Route {
                        source: ExpressionSource::Timbre,
                        destination: Destination::PitchSemitones {
                            zero: 0.0,
                            one: 12.0,
                        },
                    },
                ]
            };
            let plan = plan.with_modulation(Modulation::new(routes, route_count).unwrap());
            let mut runtime = Runtime::new(
                plan,
                Limits {
                    notes: reserved,
                    channels: 1,
                    performances: 1,
                    expressions: reserved,
                    families: reserved,
                    decisions: 0,
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
            let center = [center];
            let above = [above];
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
                "{route_count},{control},{notes},{reserved},{median},{},{},{}",
                times[times.len() * 99 / 100],
                times[times.len() - 1],
                median as f64 / notes as f64
            );
            runtime.panic();
            runtime.flush_ended(|_| true);
            assert_eq!((runtime.note_count(), runtime.voice_count()), (0, 0));
        }
    }
}
