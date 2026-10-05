//! Independent saved synchronized-fade prototype/voice ownership oracle.
use super::*;
const SECOND: usize = 509;
const FRESH: usize = 1029;

fn prepared_points(tempo: f32) -> u32 {
    // Explicit native delay conversion for this authored note1/8, count4.
    let duration = (((0.125f32 * 60.) / tempo) * 4.) * 1000.;
    (duration.clamp(0., 5000.) * 1500. * 0.001) as u32
}

fn factor(points: u32) -> f32 {
    (1. + 1. / f64::from(0.3f32)).powf(1. / f64::from(points)) as f32
}

fn oracle(onset: usize, raw_install: bool) -> Vec<f32> {
    let mut tune = vec![0.; END];
    let mut phase = f64::from(0.23f32);
    let mut level = 0.3f32;
    // Paused-onset509 captures374 points before the529 refresh; fresh1029 captures450.
    // Neither may inherit the first voice's mutable countdown.
    let mut remaining = prepared_points(if onset == FRESH {
        100.
    } else if onset == SECOND || raw_install {
        120.0005
    } else {
        120.
    });
    let (mut previous, mut current, mut started) = (0., 0., false);
    for (frame, value) in tune.iter_mut().enumerate().skip(onset) {
        if (137..229).contains(&frame)
            || (479..613).contains(&frame)
            || (1509..1601).contains(&frame)
        {
            continue;
        }
        let age = frame - onset;
        if age.is_multiple_of(32) || !started {
            // Global reload529; ignored raw1409 change does not reload a prototype.
            // Group bypass1509/resume1601 do prepare at current raw BPM.
            let prepared_tempo = if frame >= 1701 {
                100.002
            } else if frame >= 1509 {
                100.0005
            } else if frame >= 529 {
                100.
            } else if frame >= 137 || raw_install {
                120.0005
            } else {
                120.
            };
            let fade = if remaining == 0 {
                1.
            } else {
                let gain = level - 0.3;
                level = (level * factor(prepared_points(prepared_tempo))).clamp(0.3, 1.3);
                remaining -= 1;
                gain
            };
            let triangle = if phase < 0.5 {
                4. * (phase - 0.25)
            } else {
                4. * (0.75 - phase)
            };
            previous = current;
            current = (triangle as f32 * fade) * (12. * if frame >= 311 { 0.25 } else { 0.125 });
            if !started {
                previous = current;
                started = true;
            }
            // Native GroupPrep/source+18 refreshes the same retained frequency step.
            // The f32 product is widened only after rounding, independently of Clock.
            phase += f64::from(frequency(true, prepared_tempo) * (1500f64.recip() as f32));
            if phase > 1. {
                phase -= 1.;
            }
        }
        *value = previous + (current - previous) * (age % 32) as f32 / 32.;
    }
    tune
}

fn installed(
    cache: &std::path::Path,
    instrument: &Instrument,
    plain: bool,
    raw_install: bool,
) -> Engine {
    let mut source = crate::cache::load(cache, &instrument.path, 0).unwrap();
    if plain {
        source.groups[0].pitch_lfos.clear();
    }
    let bank = Bank::load_counting(
        &source,
        MEMORY_LIMIT,
        Streaming::RamOnly,
        &[],
        &std::sync::atomic::AtomicU32::new(0),
    )
    .unwrap();
    let mut engine = Engine::default();
    engine.reset(48000.);
    engine.attack = 0.;
    engine.release = 0.05;
    // Seed the global owner before its below-threshold raw update. Bank preparation
    // uses current raw BPM without resetting that previously accepted comparison.
    engine.set_transport(true, 120., 0., (4, 4));
    if raw_install {
        engine.set_transport(true, 120.0005, 0., (4, 4));
    }
    engine.set_bank(Some(Box::new(bank)));
    engine
}

fn sync_performance(
    cache: &std::path::Path,
    instrument: &Instrument,
    raw_install: bool,
    fragments: &[usize],
) -> [Vec<f32>; 2] {
    let (mut actual, mut expected) = (
        installed(cache, instrument, false, raw_install),
        installed(cache, instrument, true, raw_install),
    );
    let roots = [
        HostNote {
            port: 0,
            channel: 4,
            key: 60,
            id: 92,
            clap: true,
        },
        HostNote {
            port: 0,
            channel: 4,
            key: 60,
            id: 93,
            clap: true,
        },
        HostNote {
            port: 0,
            channel: 4,
            key: 60,
            id: 94,
            clap: true,
        },
    ];
    let patterns = roots.map(|r| HostPattern {
        port: 0,
        channel: 4,
        key: i32::from(r.key),
        id: r.id,
        clap: true,
    });
    let tunes = [
        oracle(0, raw_install),
        oracle(SECOND, raw_install),
        oracle(FRESH, raw_install),
    ];
    let (mut pcm, mut reference) = (
        [vec![0.; END], vec![0.; END]],
        [vec![0.; END], vec![0.; END]],
    );
    let (mut at, mut block) = (0, 0);
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for e in [&mut actual, &mut expected] {
                assert!(e.host_note_on(roots[0], 4, 60, 127, 0.));
            }
            while at < END {
                match at {
                    101 | 529 | 1409 | 1701 => {
                        let tempo = match at {
                            101 => 120.0005,
                            529 => 100.,
                            1409 => 100.0005,
                            _ => 100.002,
                        };
                        for e in [&mut actual, &mut expected] {
                            e.set_transport(true, tempo, 0., (4, 4));
                        }
                    }
                    137 | 229 | 479 | 613 | 1509 | 1601 => assert!(actual.write(
                        Address::PitchSourceBypass(0, 0),
                        if at == 137 || at == 479 || at == 1509 {
                            1.
                        } else {
                            0.
                        }
                    )),
                    311 => assert!(actual.write(
                        Address::PitchSourceIntensity {
                            group: 0,
                            slot: 0,
                            target: 0,
                            bipolar: false
                        },
                        0.25
                    )),
                    SECOND | FRESH => {
                        for e in [&mut actual, &mut expected] {
                            assert!(e.host_note_on(
                                roots[if at == SECOND { 1 } else { 2 }],
                                4,
                                60,
                                127,
                                0.
                            ));
                        }
                    }
                    711 | 1037 => {
                        for e in [&mut actual, &mut expected] {
                            for pattern in patterns {
                                e.host_expression(
                                    pattern,
                                    HostExpression::Gain(if at == 711 { 0. } else { 1. }),
                                );
                            }
                        }
                    }
                    OFF => {
                        for e in [&mut actual, &mut expected] {
                            for pattern in patterns {
                                e.host_note_off(pattern);
                            }
                        }
                    }
                    _ => {}
                }
                let boundary = [
                    101, 137, 229, 311, 479, SECOND, 529, 613, 711, FRESH, 1037, 1409, 1509, 1601,
                    1701, OFF, END,
                ]
                .into_iter()
                .filter(|b| *b > at)
                .min()
                .unwrap();
                let n = fragments[block % fragments.len()].min(boundary - at);
                let [l, r] = &mut pcm;
                actual
                    .render_checked(&mut l[at..at + n], &mut r[at..at + n])
                    .unwrap();
                for frame in at..at + n {
                    for (i, onset) in [0, SECOND, FRESH].into_iter().enumerate() {
                        if frame >= onset {
                            expected.host_expression(
                                patterns[i],
                                HostExpression::Tune(tunes[i][frame]),
                            );
                        }
                    }
                    let [l, r] = &mut reference;
                    expected
                        .render_checked(&mut l[frame..frame + 1], &mut r[frame..frame + 1])
                        .unwrap();
                }
                at += n;
                block += 1;
            }
            actual.mark_host_notes();
            expected.mark_host_notes();
        }),
        0
    );
    let error = pcm
        .iter()
        .flatten()
        .zip(reference.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    assert!(
        error < 3e-5,
        "saved sync-fade completePCM error {error}, rawbank={raw_install}"
    );
    assert!(pcm[0][1037..OFF].iter().any(|x| x.abs() > 0.01));
    assert!(
        pcm.iter()
            .all(|channel| channel[END - 128..].iter().all(|x| *x == 0.))
    );
    for root in roots {
        assert_eq!(
            actual.host_note_end_at(root),
            expected.host_note_end_at(root)
        );
        assert!(actual.host_note_end_at(root).is_some());
    }
    pcm
}

#[test]
fn saved_sync_fade_preserves_voice_countdown_across_global_and_authored_refresh_without_heap() {
    let (files, instrument) = fixture_timing(true, false, Some(true), Some(0.125));
    let source = &instrument.groups[0].pitch_lfos[0];
    assert_eq!(source.fade_ms, 0.);
    assert_eq!(
        source.fade_sync,
        Some(crate::import::LfoFadeSync {
            note_value: 0.125,
            count: 4.
        })
    );
    assert!(source.extended_fade);
    let cache = files.path().join("cache");
    for raw_install in [false, true] {
        let whole = sync_performance(&cache, &instrument, raw_install, &[128]);
        let split = sync_performance(&cache, &instrument, raw_install, &[1, 7, 31, 127]);
        let error = whole
            .iter()
            .flatten()
            .zip(split.iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(
            error < 3e-5,
            "sync-fade host partitions changed PCM by{error}"
        );
    }
}
