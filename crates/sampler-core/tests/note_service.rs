use sampler_core::*;
mod support;

fn runtime() -> Runtime {
    let pcm = [[1.0, 0.0], [0.0, 1.0]]
        .map(|frame| Pcm::new(48000, vec![frame; 4096].into_boxed_slice()).unwrap());
    let regions = (0..2)
        .map(|sample| Region {
            sample,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.0,
            velocity_high: 1.0,
            gain: 1.0,
            envelope: Envelope::default(),
            playback: Playback::default(),
        })
        .collect();
    let plan = Prepared::new(48000, pcm.into(), regions, 2)
        .unwrap()
        .with_groups(2, vec![Some(0), Some(1)])
        .unwrap();
    Runtime::new(
        plan,
        Limits {
            notes: 2,
            channels: 1,
            performances: 1,
            families: 2,
            expressions: 2,
            voices: 4,
            decisions: 2,
            commands: 8,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap()
}
fn input() -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    }
}
#[test]
fn immediate_and_smoothed_note_edits_have_distinct_native_audio() {
    for target in [ModTarget::Decibels, ModTarget::Pan] {
        let sample = |immediate| {
            let mut rt = runtime();
            let mut out = [[0.0; 2]; 64];
            support::without_heap(|| {
                let note = rt.trigger(input(), 60, 1.0).unwrap();
                rt.render(&mut out).unwrap();
                rt.set_note_param_with_immediate(
                    note,
                    target,
                    if target == ModTarget::Pan { 1.0 } else { -20.0 },
                    false,
                    immediate,
                )
                .unwrap();
                rt.render(&mut out).unwrap();
            });
            out
        };
        let immediate = sample(true);
        let smooth = sample(false);
        assert!(immediate[0][0] < 0.11);
        assert!(smooth[0][0] > 0.9);
        assert!((smooth[63][0] - immediate[63][0]).abs() < 1e-5);
    }
}
#[test]
fn selected_group_fades_and_stops_without_affecting_sibling_voices() {
    let mut rt = runtime();
    let mut audio = [[0.0; 2]; 128];
    support::without_heap(|| {
        let note = rt.trigger(input(), 60, 1.0).unwrap();
        assert_eq!(
            rt.fade_note_group(note, 2, None, 0.0, 64, true),
            Err(Error::InvalidInput)
        );
        assert_eq!(
            rt.fade_note_group(note, 0, None, f64::NAN, 64, true),
            Err(Error::InvalidInput)
        );
        rt.fade_note_group(note, 0, None, 0.0, 64, true).unwrap();
        rt.render(&mut audio).unwrap();
    });
    assert!(audio[0][0] > 0.9);
    assert_eq!(audio[63][0], 0.0);
    assert!(audio[64..].iter().all(|f| *f == [0.0, 1.0]));
    assert_eq!(rt.voice_count(), 1);
}
