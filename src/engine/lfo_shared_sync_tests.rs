//! Native prepared sync steps belong to shared source generations, not receivers.
use super::*;

const FINISH: usize = 3072;
const WINDOWS: [usize; 13] = [
    0, 257, 514, 771, 1028, 1285, 1542, 1799, 2056, 2313, 2570, 2827, FINISH,
];
const STARTS: [usize; 3] = [0, 37, 1542];
const ENDS: [usize; 3] = [975, 975, 2296];

fn step(tempo: f32, effective_count: f32) -> f64 {
    let duration = ((0.25f32 * 60.) / tempo) * effective_count;
    f64::from(duration.recip() * (1500f64.recip() as f32))
}

fn windows(effective_count: f32) -> Vec<(usize, usize, f64, f64)> {
    // These are whole generated buffers. Mid-buffer tempo changes cannot
    // replace their points, and zero-consumer buffers advance no phase.
    let mut phase = 0.;
    [
        (0, 120.),
        (257, 83.),
        (514, 83.),
        (771, 83.),
        (1542, 90.0005),
        (1799, 90.002),
        (2056, 90.002),
    ]
    .into_iter()
    .map(|(base, tempo)| {
        let delta = step(tempo, effective_count);
        let window = (base, base + 257, phase, delta);
        // Closed-form physical span, independently of the production
        // ceil-and-padding recurrence and its mutable phase Cell.
        phase += 257. / 32. * delta;
        window
    })
    .collect()
}

fn oracle(onset: usize, end: usize, source: &[(usize, usize, f64, f64)]) -> Vec<f32> {
    let mut tune = vec![0.; FINISH];
    let (mut previous, mut current, mut started) = (0., 0., false);
    let (mut entry, mut first_boundary) = (onset, onset);
    for (frame, value) in tune.iter_mut().enumerate().take(end).skip(onset) {
        let age = frame - onset;
        if frame == onset || WINDOWS.contains(&frame) {
            entry = frame;
            first_boundary = entry + ((32 - (age & 31)) & 31);
        }
        if (117..181).contains(&frame) {
            continue;
        }
        if age.is_multiple_of(32) || !started {
            let &(base, end, phase, delta) =
                source.iter().find(|w| frame >= w.0 && frame < w.1).unwrap();
            let count = (end - base).div_ceil(32);
            let suffix = count - (end - entry).div_ceil(32);
            let cursor = frame.saturating_sub(first_boundary) / 32;
            let mut p = phase + (suffix + cursor) as f64 * delta;
            // Exterior periodic triangle, including faster unsynchronized
            // windows that cross a complete cycle between consumer points.
            while p > 1. {
                p -= 1.;
            }
            let triangle = if p < 0.5 {
                4. * (p - 0.25)
            } else {
                4. * (0.75 - p)
            };
            previous = current;
            current = triangle as f32
                * (12.
                    * if frame >= 1409 {
                        0.125
                    } else if frame >= 799 {
                        0.3
                    } else if frame >= 301 {
                        0.25
                    } else {
                        0.125
                    });
            if !started {
                previous = current;
                started = true;
            }
        }
        *value = previous + (current - previous) * (age % 32) as f32 / 32.;
    }
    tune
}

fn root(index: usize) -> HostNote {
    HostNote {
        port: 0,
        channel: 4,
        key: 60,
        id: 101 + index as i32,
        clap: true,
    }
}
fn pattern(index: usize) -> HostPattern {
    HostPattern {
        port: 0,
        channel: 4,
        key: 60,
        id: 101 + index as i32,
        clap: true,
    }
}

fn performance(
    cache: &std::path::Path,
    instrument: &Instrument,
    fragments: &[usize],
    effective_count: f32,
) -> [Vec<f32>; 2] {
    let (mut actual, mut expected) = (
        engine(cache, instrument, false),
        engine(cache, instrument, true),
    );
    let source = windows(effective_count);
    let tune = std::array::from_fn::<_, 3, _>(|i| oracle(STARTS[i], ENDS[i], &source));
    let (mut pcm, mut reference) = (
        [vec![0.; FINISH], vec![0.; FINISH]],
        [vec![0.; FINISH], vec![0.; FINISH]],
    );
    let (mut at, mut fragment, mut window) = (0, 0, 0);
    let held = source[3].2 + 257. / 32. * source[3].3;
    let final_phase = source[6].2 + 257. / 32. * source[6].3;
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
    assert!(error < 3e-5, "saved shared sync complete PCM error {error}");
    assert!(pcm[0][32..411].iter().any(|x| x.abs() > 0.01));
    assert!(pcm[1][69..411].iter().any(|x| x.abs() > 0.01));
    assert!(
        pcm.iter()
            .all(|channel| channel[2296..].iter().all(|x| *x == 0.))
    );
    pcm
}

#[test]
fn saved_shared_sync_uses_prepared_generation_step_and_keeps_overlap_idle_and_tempo_pcm_without_heap()
 {
    let (files, instrument) = fixture_source(true, false, None, None, None, true);
    assert!(instrument.groups[0].pitch_lfos[0].shared);
    assert_eq!(instrument.groups[0].pitch_lfos[0].note_value, 0.25);
    assert_eq!(instrument.groups[0].pitch_lfos[0].count, 3.);
    let cache = files.path().join("cache");
    let whole = performance(&cache, &instrument, &[128], 3.);
    let split = performance(&cache, &instrument, &[1, 7, 31, 127, 13, 64], 3.);
    assert!(
        whole
            .iter()
            .flatten()
            .zip(split.iter().flatten())
            .all(|(a, b)| (a - b).abs() < 3e-5)
    );
}

#[test]
fn saved_shared_sync_counts_below_one_keep_native_floor_prepared_generations_and_whole_pcm_without_heap()
 {
    let mut one = None;
    for count in [1., -3.25, -0.0, 0., 0.25, 0.999] {
        // Actual creator-written NKI with a direct v71 Digital child and
        // Retrigger0. The fixture parses it and returns a fresh binary cache.
        let (files, instrument) = fixture_source(true, false, None, None, Some(count), true);
        let source = &instrument.groups[0].pitch_lfos[0];
        assert!(source.shared);
        assert_eq!(
            source.count.to_bits(),
            count.to_bits(),
            "preserve authored count units and signed zero"
        );
        assert_eq!(source.note_value, 0.25);
        let cache = files.path().join("cache");
        // The exterior native oracle receives literal effective ONE, never
        // this imported count or the production frequency/preparation helper.
        let block = performance(&cache, &instrument, &[128], 1.);
        let split = performance(&cache, &instrument, &[1, 7, 31, 127, 13, 64], 1.);
        assert!(
            block
                .iter()
                .flatten()
                .zip(split.iter().flatten())
                .all(|(a, b)| (a - b).abs() < 3e-5)
        );
        if let Some(ref expected) = one {
            assert_eq!(
                &block, expected,
                "saved shared coefficient {count} must have effective native count ONE"
            );
        } else {
            one = Some(block);
        }
    }
}

#[path = "lfo_shared_frequency_tests.rs"]
mod frequency_tests;
