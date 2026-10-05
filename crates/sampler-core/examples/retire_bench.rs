//! Measures retirement, or --release propagation through reverse-index chains.
//! Preparation and admission stay outside the timer.
use sampler_core::{Inheritance, Input, Limits, Prepared, Protocol, Runtime};
use std::time::Instant;
fn main() {
    let release = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--release") => true,
        _ => panic!("usage: retire_bench [--release]"),
    };
    println!("notes,median_us,max_us");
    for count in [64, 256, 1024] {
        let mut times = Vec::new();
        for _ in 0..9 {
            let mut rt = Runtime::new(
                Prepared::new(48000, vec![], vec![], 0).unwrap(),
                Limits {
                    notes: count,
                    channels: 1,
                    performances: 1,
                    expressions: if release { count } else { 1 },
                    families: 0,
                    decisions: 0,
                    voices: 0,
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
            let fillers: Vec<_> = (0..if release { count - 1 } else { 0 })
                .map(|_| rt.note_on(input, 60, 1.).unwrap())
                .collect();
            let root = rt.note_on(input, 60, 1.).unwrap();
            let mut note = root;
            if release {
                for filler in fillers.into_iter().rev() {
                    rt.release(filler).unwrap();
                    rt.flush_ended(|_| true);
                    note = rt.child(note, 60, 1., true, Inheritance::Linked).unwrap();
                }
            } else {
                for _ in 1..count {
                    note = rt.child(note, 60, 1., false, Inheritance::Linked).unwrap();
                }
                rt.panic();
            }
            let start = Instant::now();
            if release {
                rt.release(root).unwrap();
            }
            let release_time = start.elapsed().as_nanos();
            let mut terminals = 0;
            rt.flush_ended(|_| {
                terminals += 1;
                true
            });
            times.push(if release {
                release_time
            } else {
                start.elapsed().as_nanos()
            });
            assert_eq!(
                (rt.note_count(), rt.expression_count(), terminals),
                (0, 0, 1)
            );
        }
        times.sort_unstable();
        println!(
            "{count},{:.3},{:.3}",
            times[4] as f64 / 1000.,
            times[8] as f64 / 1000.
        );
    }
}
