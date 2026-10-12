//! Local Mono allocation keeps shared volume waveform ownership in the group.
use super::*;
use crate::{
    engine::params::{Address, id},
    import::{DigitalLfo, Modulator, PitchLfo, VolumeLfo},
    ksp::EnginePar,
};

const FRAMES: usize = 1536;

fn render(e: &mut Engine, pcm: &mut [Vec<f32>; 2], from: usize, to: usize, parts: &[usize]) {
    // Fragment a fixed host range. Shared sources cache its complete waveform;
    // changing the host range itself changes native suffix/phase sampling.
    const WINDOW: usize = 256;
    let (mut at, mut part) = (from, 0);
    while at < to {
        if at % WINDOW == 0 {
            e.begin_audio_block(WINDOW.min(FRAMES - at), 1, false);
        }
        let n = parts[part % parts.len()]
            .min(to - at)
            .min(WINDOW - at % WINDOW);
        let [left, right] = pcm;
        e.render_checked(&mut left[at..at + n], &mut right[at..at + n])
            .unwrap();
        at += n;
        part += 1;
    }
}

fn group(shared: bool, pitch: bool) -> Group {
    let source = PitchLfo {
        slot: 7,
        count: 1.,
        note_value: 1. / 24.,
        sine: 0.,
        start_phase: 0.,
        fade_ms: 0.,
        extended_fade: false,
        fade_sync: None,
        depth: if pitch { 0.125 } else { 0. },
        targets: if pitch { vec![(2, 0.125)] } else { vec![] },
        bypassed: false,
        shared,
        digital: Some(DigitalLfo {
            rectangle: 0.,
            triangle: 1.,
            saw: 0.,
            normalize: false,
            pulse_width: 0.5,
        }),
    };
    let mut group = Group {
        pitch_lfos: if pitch { vec![source.clone()] } else { vec![] },
        volume_lfos: vec![VolumeLfo {
            source,
            target: 4,
            intensity: 0.75,
            negative: false,
            lag_ms: 15,
        }],
        ..Default::default()
    };
    group.modulators.resize_with(8, || Modulator {
        name: String::new(),
        targets: vec![],
        assignments: None,
        volume_env: false,
        bypassed: false,
        flex: false,
        primary_flex: false,
        envelope: None,
        kind: "unsupported".into(),
    });
    group.modulators[7].kind = "lfo".into();
    group.modulators[7].targets = vec![
        "Other".into(),
        "Other".into(),
        "Pitch".into(),
        "Other".into(),
        "Volume".into(),
    ];
    group
}

fn make(shared: bool, pitch: bool) -> Engine {
    let path = std::path::PathBuf::from("mono-shared-volume");
    let zones = [60, 62, 64, 65, 72]
        .into_iter()
        .map(|key| Zone {
            sample: path.clone(),
            group: usize::from(key == 72),
            root: key,
            low_key: key,
            high_key: key,
            low_velocity: 100,
            ..Default::default()
        })
        .collect();
    let bank = Bank::from_samples(
        vec![group(shared, pitch), Group::default()],
        zones,
        vec![(
            path,
            Sample {
                rate: 48000,
                frames: (0..8192)
                    .map(|n| [0.15 + n as f32 / 32768., -0.15 + n as f32 / 65536.])
                    .collect(),
            },
        )],
    )
    .unwrap();
    let mut engine = Engine::default();
    engine.reset(48000.);
    engine.attack = 0.;
    engine.release = 0.003;
    engine.set_transport(false, 120., 0., (4, 4));
    engine.set_bank(Some(Box::new(bank)));
    engine
}

pub(super) fn performance(parts: &[usize], pitch: bool) -> [Vec<f32>; 2] {
    let (mut actual, mut expected, mut restarted) =
        (make(true, pitch), make(true, pitch), make(false, pitch));
    actual.set_note_mode(7, NoteMode::MonoRetrigger).unwrap();
    let resolve = |id, generic| {
        Address::resolve(
            EnginePar {
                id,
                group: 0,
                slot: 7,
                generic,
            },
            &[group(true, pitch)],
        )
        .unwrap()
    };
    let frequency = resolve(id::INTMOD_FREQUENCY, -1);
    let depth = resolve(id::MOD_TARGET_MP_INTENSITY, 4);
    let bypass = resolve(id::INTMOD_BYPASS, -1);
    assert!(matches!(
        frequency,
        Address::LfoFrequency {
            synchronized: true,
            ..
        }
    ));
    let (b, c) = (host(940, 5, 64), host(941, 6, 62));
    let (mut pcm, mut reference, mut reset_pcm) = (
        std::array::from_fn(|_| vec![0.; FRAMES]),
        std::array::from_fn(|_| vec![0.; FRAMES]),
        std::array::from_fn(|_| vec![0.; FRAMES]),
    );
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for e in [&mut actual, &mut expected, &mut restarted] {
                e.begin_audio_block(256, 1, false);
                e.note_on_from(7, 4, 60, 127);
                e.note_on_from(9, 9, 72, 127);
                if pitch {
                    // Admission's copied cache cannot freeze a same-time edit
                    // before the first actual waveform generation.
                    assert!(e.write(frequency, 2.));
                }
            }
            let (a, wrong_a) = (
                expected.player.voices[0].event,
                restarted.player.voices[0].event,
            );
            let accepted = actual.player.input_keys[4][60].accepted.unwrap();
            let (mut b_source, mut wrong_b) = (None, None);
            let mut at = 0;
            for to in [
                64, 80, 96, 128, 137, 173, 193, 256, 257, 289, 320, 353, 576, 800, 832, 896, FRAMES,
            ] {
                render(&mut actual, &mut pcm, at, to, parts);
                render(&mut expected, &mut reference, at, to, parts);
                render(&mut restarted, &mut reset_pcm, at, to, parts);
                at = to;
                match to {
                    64 => {
                        for e in [&mut actual, &mut expected, &mut restarted] {
                            assert!(e.host_note_on(b, 7, 64, 127, 0.));
                        }
                        b_source = Some(last(&expected));
                        wrong_b = Some(last(&restarted));
                        expected.fade_event(a, STEAL_FADE, 0., true);
                        restarted.fade_event(wrong_a, STEAL_FADE, 0., true);
                    }
                    80 => {
                        for e in [&mut actual, &mut expected, &mut restarted] {
                            e.note_on_from(7, 4, 60, 1);
                        }
                        assert_eq!(
                            actual.player.input_keys[4][60].accepted.unwrap().ordinal,
                            accepted.ordinal
                        );
                    }
                    96 => {
                        for e in [&mut actual, &mut expected, &mut restarted] {
                            e.note_on_from(7, 8, 100, 127);
                        }
                    }
                    128 => {
                        for e in [&mut actual, &mut expected, &mut restarted] {
                            e.host_note_off(pattern(b));
                        }
                        replay(&mut expected, 60, 4, None);
                        replay(&mut restarted, 60, 4, None);
                        expected.fade_event(b_source.unwrap(), STEAL_FADE, 0., true);
                        restarted.fade_event(wrong_b.unwrap(), STEAL_FADE, 0., true);
                        assert_eq!(last(&actual), accepted.event);
                        assert_eq!(actual.player.voices.last().unwrap().counter_start, 128);
                    }
                    137 => {
                        for e in [&mut actual, &mut expected, &mut restarted] {
                            assert!(e.write(frequency, 2.));
                            assert_eq!(
                                crate::engine::params::read(e.settings().unwrap(), frequency),
                                Some(2.)
                            );
                        }
                    }
                    173 => {
                        for e in [&mut actual, &mut expected, &mut restarted] {
                            assert!(e.write(depth, -0.75));
                        }
                    }
                    193 | 257 => {
                        for e in [&mut actual, &mut expected, &mut restarted] {
                            e.player.midi.set_expression(
                                7,
                                60,
                                Expression {
                                    gain: if to == 193 { 0. } else { 1. },
                                    ..Default::default()
                                },
                            );
                        }
                    }
                    256 if pitch => {
                        for e in [&mut actual, &mut expected, &mut restarted] {
                            e.begin_audio_block(256, 1, false);
                        }
                        actual.player.midi.set_expression(
                            7,
                            65,
                            Expression {
                                tune: 10000.,
                                ..Default::default()
                            },
                        );
                        actual.note_on_from(7, 3, 65, 127);
                        assert!(actual.player.input_keys[3][65].accepted.is_none());
                        assert_eq!(last(&actual), accepted.event);
                        assert_eq!(
                            actual.take_stream_error().unwrap().kind,
                            crate::engine::stream::StreamErrorKind::UnrepresentablePitch
                        );
                        // A failed probe also cannot poison the later render
                        // cache when an ordered source edit follows it.
                        for e in [&mut actual, &mut expected, &mut restarted] {
                            assert!(e.write(frequency, 3.));
                        }
                    }
                    289 | 353 => {
                        for e in [&mut actual, &mut expected, &mut restarted] {
                            assert!(e.write(bypass, if to == 289 { 1. } else { 0. }));
                        }
                    }
                    320 => {
                        for e in [&mut actual, &mut expected, &mut restarted] {
                            e.note_off_from(7, 4, 60);
                        }
                    }
                    576 => {
                        for e in [&mut actual, &mut expected, &mut restarted] {
                            e.note_off_from(9, 9, 72);
                        }
                    }
                    800 => {
                        for e in [&mut actual, &mut expected, &mut restarted] {
                            assert_eq!(e.active_voices(), 0);
                        }
                    }
                    832 => {
                        for e in [&mut actual, &mut expected, &mut restarted] {
                            assert!(e.host_note_on(c, 7, 62, 127, 0.));
                        }
                    }
                    896 => {
                        for e in [&mut actual, &mut expected, &mut restarted] {
                            e.host_note_off(pattern(c));
                        }
                    }
                    _ => {}
                }
            }
            for e in [&mut actual, &mut expected, &mut restarted] {
                e.mark_host_notes();
                assert_eq!(e.host_note_end_at(b), Some(321));
                assert_eq!(e.host_note_end_at(c), Some(1089));
                assert!(!e.host_note_pending(c));
                assert_eq!(e.active_voices(), 0);
            }
        }),
        0
    );
    equal(&pcm, &reference);
    let changed = reference
        .iter()
        .flatten()
        .zip(reset_pcm.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0., f32::max);
    assert!(
        changed > 0.001,
        "per-note waveform restart must audibly differ from shared group ownership: {changed}"
    );
    pcm
}

#[test]
fn shared_volume_only_mono_fallback_live_controls_and_idle_match_manual_poly_pcm_without_heap() {
    // A shared pitch source needs a declared host range during ingress.
    let mut unsupported = group(true, false);
    unsupported
        .pitch_lfos
        .push(unsupported.volume_lfos[0].source.clone());
    let bank = Bank::from_samples(
        vec![unsupported],
        vec![Zone::default()],
        vec![(
            Default::default(),
            Sample {
                rate: 48000,
                frames: vec![[0.25; 2]; 4096],
            },
        )],
    )
    .unwrap();
    let mut engine = Engine::default();
    engine.set_bank(Some(Box::new(bank)));
    engine.set_note_mode(7, NoteMode::MonoRetrigger).unwrap();
    engine.note_on(7, 60, 127);
    assert_eq!(engine.active_voices(), 0);
    assert_eq!(
        engine.take_stream_error().unwrap().kind,
        crate::engine::stream::StreamErrorKind::UnsupportedWindow
    );
    let whole = performance(&[128], false);
    let split = performance(&[1, 7, 31, 127], false);
    equal_partitions(&whole, &split);
}
