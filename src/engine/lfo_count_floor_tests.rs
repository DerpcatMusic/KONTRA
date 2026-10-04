//! Saved native coefficients below one use the converter's one-note floor.
use super::*;

fn oracle(fade_mode: Option<bool>) -> Vec<f32> {
    let mut tune = vec![0.; END];
    let mut phase = f64::from(0.23f32);
    let (mut previous, mut current, mut started) = (0., 0., false);
    let (mut remaining, mut level) = (60usize, 0.3f32);
    let factor = (1. + 1. / f64::from(0.3f32)).powf(1. / 60.) as f32;
    for frame in 0..END {
        if (137..229).contains(&frame) {
            continue;
        }
        if frame % 32 == 0 || !started {
            let triangle = if phase < 0.5 {
                4. * (phase - 0.25)
            } else {
                4. * (0.75 - phase)
            };
            let fade = if let Some(extended) = fade_mode {
                if remaining == 0 {
                    1.
                } else {
                    let value = level - 0.3;
                    level = (level * factor).clamp(
                        if extended { 0.3 } else { 0. },
                        if extended { 1.3 } else { 1. },
                    );
                    remaining -= 1;
                    value
                }
            } else {
                1.
            };
            previous = current;
            current = (triangle as f32 * fade) * (12. * if frame >= 311 { 0.25 } else { 0.125 });
            if !started {
                previous = current;
                started = true;
            }
            // This exterior oracle uses literal effective count ONE. It does
            // not read the authored coefficient or call a production converter.
            let tempo = if frame >= 529 { 83. } else { 120. };
            let duration = ((0.25f32 * 60.) / tempo) * 1.;
            phase += f64::from(duration.recip() * (1500f64.recip() as f32));
            if phase > 1. {
                phase -= 1.;
            }
        }
        tune[frame] = previous + (current - previous) * (frame % 32) as f32 / 32.;
    }
    tune
}

fn performance(
    cache: &std::path::Path,
    instrument: &Instrument,
    fade_mode: Option<bool>,
    fragments: &[usize],
) -> ([Vec<f32>; 2], u64) {
    let (mut actual, mut expected) = (
        engine(cache, instrument, false),
        engine(cache, instrument, true),
    );
    let root = HostNote {
        port: 0,
        channel: 4,
        key: 60,
        id: 96,
        clap: true,
    };
    let pattern = HostPattern {
        port: 0,
        channel: 4,
        key: 60,
        id: 96,
        clap: true,
    };
    let tune = oracle(fade_mode);
    let (mut pcm, mut reference) = (
        [vec![0.; END], vec![0.; END]],
        [vec![0.; END], vec![0.; END]],
    );
    let (mut at, mut block) = (0, 0);
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for e in [&mut actual, &mut expected] {
                e.set_transport(true, 120., 0., (4, 4));
                assert!(e.host_note_on(root, 4, 60, 127, 0.));
            }
            while at < END {
                match at {
                    137 | 229 => assert!(actual.write(
                        Address::PitchSourceBypass(0, 0),
                        if at == 137 { 1. } else { 0. }
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
                    529 => {
                        for e in [&mut actual, &mut expected] {
                            e.set_transport(true, 83., 0., (4, 4));
                        }
                    }
                    711 | 1037 => {
                        for e in [&mut actual, &mut expected] {
                            e.host_expression(
                                pattern,
                                HostExpression::Gain(if at == 711 { 0. } else { 1. }),
                            );
                        }
                    }
                    OFF => {
                        for e in [&mut actual, &mut expected] {
                            e.host_note_off(pattern);
                        }
                    }
                    _ => {}
                }
                let boundary = [137, 229, 311, 529, 711, 1037, OFF, END]
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
                    expected.host_expression(pattern, HostExpression::Tune(tune[frame]));
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
        "floor-one saved native full PCM error {error}"
    );
    assert!(pcm[0][32..711].iter().any(|x| x.abs() > 0.01));
    assert!(
        pcm.iter()
            .all(|channel| channel[END - 128..].iter().all(|x| *x == 0.))
    );
    let end = expected.host_note_end_at(root).unwrap();
    assert_eq!(actual.host_note_end_at(root), Some(end));
    assert!(end < (OFF + 3201) as u64);
    (pcm, end)
}

#[test]
fn saved_sync_frequency_counts_below_one_reach_native_floor_and_whole_pcm_without_heap() {
    let mut one = None;
    for count in [1., -3.25, -0.0, 0., 0.25, 0.999] {
        let (files, instrument) = fixture_count(true, false, None, None, Some(count));
        assert_eq!(
            instrument.groups[0].pitch_lfos[0].count.to_bits(),
            count.to_bits()
        );
        let cache = files.path().join("cache");
        let block = performance(&cache, &instrument, None, &[128]);
        let split = performance(&cache, &instrument, None, &[1, 7, 31, 127, 13, 64]);
        assert_eq!(block.1, split.1);
        assert!(
            block
                .0
                .iter()
                .flatten()
                .zip(split.0.iter().flatten())
                .all(|(a, b)| (a - b).abs() < 3e-5)
        );
        if let Some((ref pcm, end)) = one {
            assert_eq!(block.1, end);
            assert_eq!(
                &block.0, pcm,
                "authored coefficient {count} must convert to the same effective count ONE"
            );
        } else {
            one = Some(block);
        }
    }
    for mode in [false, true] {
        let (files, instrument) = fixture_count(true, false, Some(mode), None, Some(0.25));
        performance(
            &files.path().join("cache"),
            &instrument,
            Some(mode),
            &[1, 7, 31, 127],
        );
    }
}
