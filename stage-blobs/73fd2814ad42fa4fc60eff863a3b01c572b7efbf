//! Equal-pitch victims retain admission order after natural source retirement.
use crate::{
    audio::Sample,
    engine::{Bank, Engine, HostExpression, HostNote, HostPattern},
    import::{Group, VoiceLimit, Zone},
};

fn host(id: i32, key: u8) -> HostNote {
    HostNote {
        port: 0,
        channel: 4,
        key,
        id,
        clap: true,
    }
}
fn pattern(n: HostNote) -> HostPattern {
    HostPattern {
        port: i32::from(n.port),
        channel: i32::from(n.channel),
        key: i32::from(n.key),
        id: n.id,
        clap: n.clap,
    }
}
fn bank(mode: i16, layered: bool, view: u8) -> Bank {
    let tie_key = if mode == 4 { 48 } else { 72 };
    let paths = ["short", "first", "second", "other"];
    let zones = [
        (61, 0, 0.),
        (tie_key, 1, if layered { -1. } else { 0. }),
        (tie_key, 2, 1.),
        (60, 3, 0.),
    ]
    .into_iter()
    .filter(|(_, layer, _)| {
        (*layer != 2 || layered)
            && match view {
                1 => *layer != 1 || !layered,
                2 => *layer == 1,
                _ => true,
            }
    })
    .map(|(key, layer, pan)| Zone {
        sample: paths[layer].into(),
        root: key,
        low_key: key,
        high_key: key,
        pan,
        ..Default::default()
    })
    .collect();
    let samples = paths
        .into_iter()
        .enumerate()
        .map(|(layer, path)| {
            (
                path.into(),
                Sample {
                    rate: 48000,
                    frames: (0..if layer == 0 { 64 } else { 8192 })
                        .map(|i| {
                            let phase =
                                std::f64::consts::TAU * (layer + 1) as f64 * i as f64 / 200.;
                            [(phase.sin() * 0.01) as f32; 2]
                        })
                        .collect(),
                },
            )
        })
        .collect();
    let mut bank = Bank::from_samples(
        vec![Group {
            key_tracking: false,
            ..Default::default()
        }],
        zones,
        samples,
    )
    .unwrap();
    if view == 0 {
        bank.polyphony = crate::engine::bank::VoiceGroup::from(&VoiceLimit {
            max_voices: 3,
            kill_mode: mode,
            prefer_released: mode == 2,
            fade_ms: 10,
            exclusion_group: -1,
        });
    }
    bank
}
fn engine(mode: i16, layered: bool, view: u8) -> Engine {
    let mut e = Engine::default();
    e.reset(48000.);
    e.attack = 0.;
    e.release = 0.05;
    e.set_bank(Some(Box::new(bank(mode, layered, view))));
    e
}
fn start(e: &mut Engine, n: HostNote, pan: f32) {
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            assert!(e.host_note_on(n, 7, n.key, 127, 0.));
            e.host_expression(pattern(n), HostExpression::Pan(pan));
        }),
        0
    );
}
fn render(e: &mut Engine, frames: usize, partition: &[usize]) -> Vec<[f32; 2]> {
    let mut pcm = Vec::with_capacity(frames);
    let mut at = 0;
    let mut block = 0;
    while at < frames {
        let n = partition[block % partition.len()].min(frames - at);
        let (mut l, mut r) = ([0.; 128], [0.; 128]);
        assert_eq!(
            crate::plugin::tests::allocations(|| e
                .render_checked(&mut l[..n], &mut r[..n])
                .unwrap()),
            0
        );
        pcm.extend(l[..n].iter().zip(&r[..n]).map(|(&l, &r)| [l, r]));
        at += n;
        block += 1;
    }
    pcm
}
fn same(a: &[[f32; 2]], b: &[[f32; 2]]) {
    assert_eq!(a.len(), b.len());
    assert!(
        a.iter()
            .zip(b)
            .all(|(a, b)| (0..2).all(|ch| (a[ch] - b[ch]).abs() < 2e-6)),
        "voice victim changed independent full PCM"
    );
}
fn step(
    actual: &mut Engine,
    main: &mut Engine,
    fader: &mut Engine,
    pcm: &mut Vec<[f32; 2]>,
    frames: usize,
    partition: &[usize],
) {
    let a = render(actual, frames, partition);
    let m = render(main, frames, partition);
    let f = render(fader, frames, partition);
    let expected: Vec<_> = m
        .iter()
        .zip(f)
        .map(|(m, f)| [m[0] + f[0], m[1] + f[1]])
        .collect();
    same(&a, &expected);
    pcm.extend(a);
}
fn performance(mode: i16, layered: bool, partition: &[usize]) -> Vec<[f32; 2]> {
    let tie_key = if mode == 4 { 48 } else { 72 };
    let [a, b, c, d, e] = [
        host(630, 61),
        host(631, tie_key),
        host(632, tie_key),
        host(633, 60),
        host(634, 60),
    ];
    let (mut actual, mut main, mut fader) = (
        engine(mode, layered, 0),
        engine(mode, layered, 1),
        engine(mode, layered, 2),
    );
    let mut pcm = Vec::new();
    start(&mut actual, a, 0.);
    start(&mut main, a, 0.);
    step(&mut actual, &mut main, &mut fader, &mut pcm, 16, partition);
    start(&mut actual, b, if layered { 0. } else { -1. });
    start(&mut fader, b, if layered { 0. } else { -1. });
    if layered {
        start(&mut main, b, 0.);
    }
    step(&mut actual, &mut main, &mut fader, &mut pcm, 16, partition);
    if !layered {
        start(&mut actual, c, 1.);
        start(&mut main, c, 1.);
    }
    step(&mut actual, &mut main, &mut fader, &mut pcm, 48, partition);
    for engine in [&mut actual, &mut main] {
        engine.mark_host_notes();
        assert_eq!(engine.host_audio_end_at(a), Some(64));
        assert_eq!(engine.host_note_end_at(a), None, "physical key still held after audio EOF");
        engine.host_note_off(pattern(a));
        engine.mark_host_notes();
        assert_eq!(engine.host_note_end_at(a), Some(80));
    }
    // Short A's swap-remove now puts the newer tied sample before the oldest.
    // Newest still ties on the note's age; prefer-released isolates its two layers.
    if mode == 2 {
        assert!(layered);
        for engine in [&mut actual, &mut main, &mut fader] {
            engine.host_note_off(pattern(b));
        }
    }
    start(&mut actual, d, 0.);
    start(&mut main, d, 0.);
    step(&mut actual, &mut main, &mut fader, &mut pcm, 16, partition);
    // The unlimited oracle names a known single sample; it never asks the victim selector.
    let target = fader.player.voices[0].event;
    fader.fade_event(target, 0.010, 0., true);
    start(&mut actual, e, 0.);
    start(&mut main, e, 0.);
    step(&mut actual, &mut main, &mut fader, &mut pcm, 479, partition);
    actual.mark_host_notes();
    fader.mark_host_notes();
    assert_eq!(fader.host_note_end_at(b), None);
    assert_eq!(
        actual.host_note_end_at(b),
        None,
        "owner must survive the entire saved10ms fade"
    );
    step(&mut actual, &mut main, &mut fader, &mut pcm, 1, partition);
    fader.mark_host_notes();
    assert_eq!(fader.host_audio_end_at(b), Some(576));
    if mode != 2 {
        assert_eq!(fader.host_note_end_at(b), None, "stolen audio does not release the physical key");
        fader.host_note_off(pattern(b));
        fader.mark_host_notes();
    }
    assert_eq!(fader.host_note_end_at(b), Some(576));
    actual.mark_host_notes();
    if layered {
        assert_eq!(
            actual.host_note_end_at(b),
            None,
            "the surviving same-note layer owns its root"
        );
    } else {
        assert_eq!(actual.host_audio_end_at(b), Some(576));
        assert_eq!(actual.host_note_end_at(b), None, "oldest victim's physical key remains held");
        actual.host_note_off(pattern(b));
        actual.mark_host_notes();
        assert_eq!(actual.host_note_end_at(b), Some(576));
        assert_eq!(
            actual.host_note_end_at(c),
            None,
            "newer same-key C must survive the old victim"
        );
    }
    step(&mut actual, &mut main, &mut fader, &mut pcm, 64, partition);
    if mode != 2 && layered {
        actual.host_note_off(pattern(b));
        main.host_note_off(pattern(b));
    }
    if !layered {
        actual.host_note_off(pattern(c));
        main.host_note_off(pattern(c));
    }
    for note in [d, e] {
        actual.host_note_off(pattern(note));
        main.host_note_off(pattern(note));
    }
    step(
        &mut actual,
        &mut main,
        &mut fader,
        &mut pcm,
        4096,
        partition,
    );
    actual.mark_host_notes();
    main.mark_host_notes();
    for note in if layered {
        vec![b, d, e]
    } else {
        vec![c, d, e]
    } {
        assert_eq!(actual.host_note_end_at(note), main.host_note_end_at(note));
        assert!(
            actual.host_note_end_at(note).is_some(),
            "real Off retires every survivor exactly"
        );
    }
    assert!(
        pcm.iter()
            .any(|frame| frame[0].abs() > 0.005 && frame[1].abs() > 0.005)
    );
    pcm
}
#[test]
fn natural_expiry_keeps_oldest_equal_pitch_victim_and_same_note_layer_order_in_pcm() {
    for (mode, layered) in [(3, false), (4, false), (3, true), (4, true), (2, true)] {
        let fixed = performance(mode, layered, &[128]);
        same(&fixed, &performance(mode, layered, &[1, 7, 31, 127]));
    }
}
