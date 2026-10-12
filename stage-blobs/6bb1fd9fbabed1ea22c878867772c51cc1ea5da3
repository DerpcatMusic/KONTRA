//! Live native shared synchronized frequency changes only future generations.
use super::*;

#[path = "lfo_shared_unsync_frequency_tests.rs"]
mod unsync_tests;

fn native_windows() -> Vec<(usize, usize, f64, f64)> {
    // Actual PE timing getter140a736a0 captures; no production converter here.
    // The authored write at799 prepares Count12 but the committed771 buffer
    // stays Count1; idle write1409 then supersedes that unused timing row.
    let mut phase = 0.;
    [
        (0, 0x3f5d208a60000000),
        (257, 0x3f542570c0000000),
        (514, 0x3f6e382920000000),
        (771, 0x3f6e382920000000),
        (1542, 0x3f45d86fc0000000),
        (1799, 0x3f45d887a0000000),
        (2056, 0x3f60626ba0000000),
    ]
    .into_iter()
    .map(|(base, bits)| {
        let delta = f64::from_bits(bits);
        let window = (base, base + 257, phase, delta);
        phase += 257. / 32. * delta;
        window
    })
    .collect()
}

fn performance(
    cache: &std::path::Path,
    instrument: &Instrument,
    fragments: &[usize],
) -> [Vec<f32>; 2] {
    performance_mode(cache, instrument, fragments, true)
}
fn performance_mode(
    cache: &std::path::Path,
    instrument: &Instrument,
    fragments: &[usize],
    synchronized: bool,
) -> [Vec<f32>; 2] {
    let (mut actual, mut expected) = (
        engine(cache, instrument, false),
        engine(cache, instrument, true),
    );
    let source = if synchronized {
        native_windows()
    } else {
        unsync_tests::native_windows()
    };
    let (runtime, errors) = crate::engine::load_scripts(instrument, vec![], 48_000.);
    assert!(errors.is_empty(), "{errors:?}");
    actual.set_script(runtime);
    let mut persistent = actual.script().unwrap().persistence();
    let frequency = crate::ksp::EnginePar {
        id: crate::engine::params::id::INTMOD_FREQUENCY,
        group: 0,
        slot: 0,
        generic: -1,
    };
    let address = Address::resolve(frequency, &instrument.groups).unwrap();
    let tune = std::array::from_fn::<_, 3, _>(|i| oracle(STARTS[i], ENDS[i], &source));
    let (mut pcm, mut reference) = (
        [vec![0.; FINISH], vec![0.; FINISH]],
        [vec![0.; FINISH], vec![0.; FINISH]],
    );
    let (mut at, mut fragment, mut window) = (0, 0, 0);
    let held = source[3].2 + 257. / 32. * source[3].3;
    let mut final_phase = source[6].2 + 257. / 32. * source[6].3;
    while final_phase > 1. {
        final_phase -= 1.;
    }
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for e in [&mut actual, &mut expected] {
                e.set_transport(true, 120., 0., (4, 4));
            }
            while at < FINISH {
                if at == WINDOWS[window] {
                    let width = WINDOWS[window + 1] - at;
                    actual.begin_audio_block(width, 1, false);
                    expected.begin_audio_block(width, 1, false);
                    window += 1;
                }
                for (i, start) in STARTS.into_iter().enumerate() {
                    if at == start {
                        for e in [&mut actual, &mut expected] {
                            assert!(e.host_note_on(root(i), 4, 60, 127, 0.));
                            e.host_expression(
                                pattern(i),
                                HostExpression::Pan(if i == 1 { 1. } else { -1. }),
                            );
                        }
                    }
                }
                match at {
                    101 | 139 | 529 | 843 | 1301 | 1701 | 1901 => {
                        let tempo = match at {
                            101 => 120.0005,
                            139 => 83.,
                            529 => 83.0005,
                            843 => 90.,
                            1301 => 90.0005,
                            1701 => 90.002,
                            _ => 90.0025,
                        };
                        for e in [&mut actual, &mut expected] {
                            e.set_transport(true, tempo, 0., (4, 4));
                        }
                    }
                    117 | 181 => assert!(actual.write(
                        Address::PitchSourceBypass(0, 0),
                        if at == 117 { 1. } else { 0. }
                    )),
                    301 | 799 | 1409 => assert!(actual.write(
                        Address::PitchSourceIntensity {
                            group: 0,
                            slot: 0,
                            target: 0,
                            bipolar: false
                        },
                        if at == 1409 {
                            0.125
                        } else if at == 799 {
                            0.3
                        } else {
                            0.25
                        }
                    )),
                    411 | 699 => {
                        for e in [&mut actual, &mut expected] {
                            for i in 0..2 {
                                e.host_expression(
                                    pattern(i),
                                    HostExpression::Gain(if at == 411 { 0. } else { 1. }),
                                );
                            }
                        }
                    }
                    735 => {
                        for e in [&mut actual, &mut expected] {
                            for i in 0..2 {
                                e.host_note_choke(pattern(i));
                            }
                        }
                    }
                    2056 => {
                        for e in [&mut actual, &mut expected] {
                            e.host_note_choke(pattern(2));
                        }
                    }
                    _ => {}
                }
                let before_frequency = matches!(at, 301 | 799 | 1409 | 1901)
                    .then(|| actual.state.shared_lfos.rows[0].state.get());
                if before_frequency.is_some() {
                    actual.cc(
                        4,
                        1,
                        match at {
                            301 => 127,
                            799 => 0,
                            1409 => 6,
                            _ => 2,
                        },
                    );
                    // The callback getter sees its queued write immediately;
                    // source settings are committed before the next PCM interval.
                    actual
                        .script()
                        .unwrap()
                        .refresh_persistence(&mut persistent);
                    let crate::ksp::Value::Int(after) = persistent[0]["$after"] else {
                        panic!("missing immediate KSP readback");
                    };
                    assert_eq!(
                        after,
                        if synchronized {
                            match at {
                                301 => 1000000,
                                799 => 0,
                                1409 => 545455,
                                _ => 909091,
                            }
                        } else {
                            match at {
                                301 => 500072,
                                799 => 0,
                                1409 => 748822,
                                _ => 1000000,
                            }
                        }
                    );
                }
                if matches!(at, 1028 | 1285 | 1542) {
                    assert!(
                        (actual.state.shared_lfos.rows[0].state.get().phase - held).abs() < 1e-14,
                        "idle retains source phase after the last committed buffer"
                    );
                }
                let event = [
                    37, 101, 117, 139, 181, 301, 411, 529, 699, 735, 799, 975, 1301, 1409, 1542,
                    1701, 1901, 2056, 2296, FINISH,
                ]
                .into_iter()
                .filter(|b| *b > at)
                .min()
                .unwrap();
                let end = event.min(WINDOWS[window]);
                let n = fragments[fragment % fragments.len()].min(end - at);
                let [l, r] = &mut pcm;
                actual
                    .render_checked(&mut l[at..at + n], &mut r[at..at + n])
                    .unwrap();
                if let Some(before) = before_frequency {
                    let count = if synchronized {
                        match at {
                            301 => 1.,
                            799 => 12.,
                            1409 => 6.,
                            _ => 2.,
                        }
                    } else {
                        f32::from_bits(match at {
                            301 => 0x3fbecec0,
                            799 => 0x3c2aa580,
                            1409 => 0x418dee00,
                            _ => 0x43552480,
                        })
                    };
                    assert_eq!(
                        crate::engine::params::read(&actual.state.base, address),
                        Some(count)
                    );
                    assert_eq!(
                        crate::engine::params::read(actual.settings().unwrap(), address),
                        Some(count)
                    );
                    let state = actual.state.shared_lfos.rows[0].state.get();
                    assert_eq!(
                        (
                            state.phase,
                            state.next_phase,
                            state.step,
                            state.generation,
                            state.end,
                            state.count,
                            state.points
                        ),
                        (
                            before.phase,
                            before.next_phase,
                            before.step,
                            before.generation,
                            before.end,
                            before.count,
                            before.points
                        ),
                        "applying same-kind frequency copy cannot replace cached points or rewind phase"
                    );
                    if at == 799 && synchronized {
                        assert_eq!(
                            actual.state.lfo_timings.group(0)[0].step,
                            f64::from_bits(0x3f342578c0000000),
                            "authored refresh captures raw83.0005 even below global threshold"
                        );
                    }
                }
                for frame in at..at + n {
                    for i in 0..3 {
                        if frame >= STARTS[i] && frame < ENDS[i] {
                            expected
                                .host_expression(pattern(i), HostExpression::Tune(tune[i][frame]));
                        }
                    }
                    let [l, r] = &mut reference;
                    expected
                        .render_checked(&mut l[frame..frame + 1], &mut r[frame..frame + 1])
                        .unwrap();
                }
                at += n;
                fragment += 1;
                if matches!(at, 975 | 2296) {
                    assert_eq!(actual.active_voices(), 0);
                }
            }
            actual.mark_host_notes();
            expected.mark_host_notes();
            for (i, end) in ENDS.into_iter().enumerate() {
                assert_eq!(actual.host_note_end_at(root(i)), Some(end as u64));
                assert_eq!(expected.host_note_end_at(root(i)), Some(end as u64));
            }
            assert!(
                (actual.state.shared_lfos.rows[0].state.get().phase - final_phase).abs() < 1e-14,
                "ignored raw BPM and idle cannot advance or retime the shared source"
            );
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
        "saved shared frequency complete PCM error {error}, synchronized={synchronized}"
    );
    assert!(pcm[0][32..411].iter().any(|x| x.abs() > 0.01));
    assert!(pcm[1][69..411].iter().any(|x| x.abs() > 0.01));
    assert!(
        pcm.iter()
            .all(|channel| channel[2296..].iter().all(|x| *x == 0.))
    );
    pcm
}

#[test]
fn saved_shared_sync_live_frequency_keeps_committed_points_and_next_generation_pcm_without_heap() {
    let (files, mut instrument) = fixture_source(true, false, None, None, Some(3.), true);
    assert!(instrument.groups[0].pitch_lfos[0].shared);
    let par = crate::ksp::EnginePar {
        id: crate::engine::params::id::INTMOD_FREQUENCY,
        group: 0,
        slot: 0,
        generic: -1,
    };
    for count in [-3.25, -0., 0., 0.25] {
        let mut groups = instrument.groups.clone();
        groups[0].pitch_lfos[0].count = count;
        let address = Address::resolve(par, &groups).unwrap();
        assert_eq!(
            address.encode(count),
            1000000,
            "saved raw floor counts remain readable"
        );
    }
    instrument.scripts = vec![
        "on init
        declare $after := 0
        declare $write := 0
        make_persistent($after)
        end on
        on controller
        select ($write)
        case 0
            set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,1000000,0,0,-1)
        case 1
            set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,0,0,0,-1)
        case 2
            set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,545455,0,0,-1)
        case 3
            set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,909091,0,0,-1)
        end select
        inc($write)
        $after := get_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,0,0,-1)
        end on"
            .into(),
    ];
    let cache = files.path().join("cache");
    let cached = crate::cache::load(&cache, &instrument.path, 0).unwrap();
    assert_eq!(
        cached.groups[0].pitch_lfos[0],
        instrument.groups[0].pitch_lfos[0]
    );
    let whole = performance(&cache, &instrument, &[128]);
    let split = performance(&cache, &instrument, &[1, 7, 31, 127, 13, 64]);
    assert!(
        whole
            .iter()
            .flatten()
            .zip(split.iter().flatten())
            .all(|(a, b)| (a - b).abs() < 3e-5)
    );
}
