//! The opt-in selection diagnostic names why each mapped region did not sound.
use sampler_core::{
    Envelope, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, Rejection, Runtime,
};

fn region(velocity_low: f64, velocity_high: f64) -> Region {
    Region {
        sample: 0,
        key_low: 60,
        key_high: 60,
        root_key: None,
        velocity_low,
        velocity_high,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback::default(),
    }
}

#[test]
fn records_the_reason_each_region_was_rejected() {
    let pcm = vec![Pcm::new(48000, vec![[1.; 2]; 480].into_boxed_slice()).unwrap()];
    let prepared = Prepared::new(48000, pcm, vec![region(0., 0.5), region(0.5, 1.)], 128).unwrap();
    let mut rt = Runtime::new(
        prepared,
        Limits {
            notes: 4,
            channels: 1,
            performances: 1,
            families: 4,
            voices: 4,
            expressions: 4,
            decisions: 0,
            commands: 4,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap();
    let input = Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(1),
    };
    rt.trigger(input, 60, 0.8).unwrap();
    assert!(rt.take_selection_records().is_empty(), "off by default");
    rt.record_selections(true);
    let input = Input {
        external_id: Some(2),
        ..input
    };
    rt.trigger(input, 60, 0.8).unwrap();
    let records = rt.take_selection_records();
    assert_eq!(records.len(), 1);
    let verdicts: Vec<_> = records[0].candidates.iter().map(|c| c.rejected).collect();
    assert_eq!(verdicts, [Some(Rejection::Velocity), None]);
}
