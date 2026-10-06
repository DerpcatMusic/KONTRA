use sampler_core::{
    Envelope, Error, Input, Limits, Pcm, Playback, Prepared, Protocol, Runtime, Stealing,
};
mod support;

fn runtime() -> Runtime {
    Runtime::new(
        Prepared::new(
            48000,
            vec![Pcm::new(48000, vec![[1.; 2]; 64].into_boxed_slice()).unwrap()],
            vec![],
            0,
        )
        .unwrap(),
        Limits {
            notes: 1,
            channels: 1,
            performances: 1,
            expressions: 1,
            families: 1,
            decisions: 0,
            voices: 4,
            commands: 0,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap()
}

#[test]
fn full_polyphony_steals_quietest_with_a_fade_instead_of_rejecting() {
    let mut rt = runtime();
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
            1.,
        )
        .unwrap();
    let family = rt.create_family(note).unwrap();
    let start = |rt: &mut Runtime, gain: f32| {
        rt.start_family(
            family,
            0,
            rt.now(),
            gain,
            Envelope::default(),
            Playback::default(),
        )
    };
    // Strict by default.
    for gain in [0.5, 0.125, 1.] {
        start(&mut rt, gain).unwrap();
    }
    start(&mut rt, 0.25).unwrap();
    assert_eq!(start(&mut rt, 0.25), Err(Error::Capacity));
    rt.stop_family(family).unwrap();

    let family = rt.create_family(note).unwrap();
    let start = |rt: &mut Runtime, gain: f32| {
        rt.start_family(
            family,
            0,
            rt.now(),
            gain,
            Envelope::default(),
            Playback::default(),
        )
    };
    rt.set_voice_stealing(Some(Stealing {
        fade: 8,
        headroom: 1,
    }))
    .unwrap();
    support::without_heap(|| {
        // Polyphony 3: the fourth start fades the quietest (0.125) out.
        for gain in [0.5, 0.125, 1., 0.25] {
            start(&mut rt, gain).unwrap();
        }
        assert_eq!((rt.voice_count(), rt.stolen_voices()), (4, 1));
        let mut audio = [[0.; 2]; 16];
        rt.render(&mut audio).unwrap();
        // 0.5 + 1 + 0.25 after the 8-frame fade of 0.125.
        assert!((audio[15][0] - 1.75).abs() < 1e-6, "{audio:?}");
        assert!(audio[0][0] > 1.75 && audio[0][0] < 1.875 + 1e-6);
        assert_eq!((rt.voice_count(), rt.stolen_voices(), rt.steals()), (3, 0, 1));
        // More than the headroom at once: the oldest stolen voice is cut.
        for _ in 0..3 {
            start(&mut rt, 1.).unwrap();
        }
        assert_eq!(rt.voice_count(), 4);
    });
}
