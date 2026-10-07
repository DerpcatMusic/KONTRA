use sampler_core::{Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime};
use sampler_midi::mpe_response;

fn runtime() -> Runtime {
    let (channels, modulation) = (4, sampler_core::Modulation::default());
    Runtime::new(
        Prepared::new(
            48000,
            vec![
                Pcm::new(
                    48000,
                    (0..60000)
                        .map(|i| {
                            let phase = f64::from(i) * std::f64::consts::TAU * 0.017;
                            [phase.cos() as f32, phase.sin() as f32]
                        })
                        .collect(),
                )
                .unwrap(),
            ],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 61,
                root_key: None,
                velocity_low: 0.0,
                velocity_high: 1.0,
                gain: 1.0,
                envelope: Envelope::new(0, 0, 0, 1.0, 256).unwrap(),
                playback: Playback::default(),
            }],
            2,
        )
        .unwrap()
        .with_modulation(modulation),
        Limits {
            notes: 8,
            channels,
            performances: 1,
            families: 8,
            decisions: 0,
            expressions: 8,
            voices: 8,
            commands: 8,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap()
}

#[test]
fn a_member_bend_of_two_semitones_is_read_as_a_pitch_response() {
    let r = mpe_response(runtime, 60).unwrap();
    assert!(r.pitch_responds(), "{r:?}");
    assert!(r.pressure_db.is_finite());
}
