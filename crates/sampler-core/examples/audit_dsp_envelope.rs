//! Current Kontakt curve translation through the real core on authored unity PCM.
use sampler_core::{
    Envelope, EnvelopeCurve, Event, Input, Limits, Pcm, Playback, Prepared, Protocol, Region,
    Runtime,
};

fn main() {
    for c in [-1.0_f64, 0.0, 1.0] {
        let b = ((1.0 - c.abs()) * 500_000_f64.ln() - 20_000_f64.ln()).exp() as f32;
        let b = f64::from(b);
        let attack = if c > 0.0 {
            (b / (1.0 + b)).ln()
        } else {
            ((1.0 + b) / b).ln()
        };
        let fall = (3.0_f64 / 43.0).ln();
        for lengths in [
            [7, 3, 11, 13],
            [0, 3, 11, 13],
            [0, 0, 11, 13],
            [0, 0, 0, 13],
            [7, 0, 0, 0],
        ] {
            for release in [0, 5, 25] {
                let [a, h, d, r] = lengths;
                let envelope = Envelope::new(a, h, d, 0.125, r).unwrap().with_curves(
                    EnvelopeCurve::exponential(attack).unwrap(),
                    EnvelopeCurve::exponential(fall).unwrap(),
                    EnvelopeCurve::exponential(fall).unwrap(),
                );
                let plan = Prepared::new(
                    1000,
                    vec![Pcm::new(1000, vec![[1.0; 2]; 70].into_boxed_slice()).unwrap()],
                    vec![Region {
                        sample: 0,
                        key_low: 60,
                        key_high: 60,
                        root_key: None,
                        velocity_low: 0.0,
                        velocity_high: 1.0,
                        gain: 1.0,
                        envelope,
                        playback: Playback::default(),
                    }],
                    1,
                )
                .unwrap();
                let limits = Limits {
                    notes: 1,
                    channels: 1,
                    performances: 1,
                    expressions: 1,
                    families: 1,
                    voices: 1,
                    decisions: 0,
                    commands: 1,
                    behaviors: 0,
                    behavior_fuel: 0,
                    behavior_cells: 0,
                    note_cells: 0,
                };
                let mut rt = Runtime::new(plan, limits).unwrap();
                let note = rt
                    .trigger(
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
                rt.schedule_event(release, Event::KeyUp(note, None))
                    .unwrap();
                let mut audio = [[0.0; 2]; 70];
                for chunk in audio.chunks_mut(7) {
                    rt.render(chunk).unwrap();
                }
                assert!(audio.iter().all(|f| f[0].is_finite() && f[0] == f[1]));
                assert_eq!(audio[69], [0.0; 2]);
                let values = audio
                    .iter()
                    .map(|f| f[0].to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                println!("{c}\t{a},{h},{d},{r}\t{release}\t{values}");
            }
        }
    }
}
