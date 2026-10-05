//! Measures retirement only; preparation and admission stay outside the timer.
use sampler_core::{Inheritance, Input, Limits, Prepared, Protocol, Runtime};
use std::time::Instant;
fn main() {
    println!("notes,median_us,max_us");
    for count in [64, 256, 1024] {
        let mut times = Vec::new();
        for _ in 0..9 {
            let mut rt = Runtime::new(
                Prepared::new(48000, vec![], vec![], 0).unwrap(),
                Limits {
                    notes: count,
                    channels: 1,
                    expressions: 1,
                    families: 0,
                    voices: 0,
                    commands: 0,
                },
            )
            .unwrap();
            let mut note = rt
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
                    1.,
                )
                .unwrap();
            for _ in 1..count {
                note = rt.child(note, 60, 1., false, Inheritance::Linked).unwrap();
            }
            rt.panic();
            let start = Instant::now();
            let mut terminals = 0;
            rt.flush_ended(|_| {
                terminals += 1;
                true
            });
            times.push(start.elapsed().as_nanos());
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
