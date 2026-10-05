//! Audible glide clocks against a route-free engine driven by exterior tuning.
use crate::{
    audio::Sample,
    engine::{
        Bank, Engine, HostExpression, HostNote, HostPattern, StreamErrorKind, params::Address,
    },
    import::{Group, Zone},
    modulation::{GlideMode, PitchGlide},
};

const HELD: usize = 6144;
const END: usize = HELD + 4096;
fn note(id: i32, channel: u8, key: u8) -> HostNote {
    HostNote {
        port: 0,
        channel,
        key,
        id,
        clap: true,
    }
}
fn pattern(n: HostNote) -> HostPattern {
    HostPattern {
        port: 0,
        channel: i32::from(n.channel),
        key: i32::from(n.key),
        id: n.id,
        clap: true,
    }
}
fn engine(source: Option<PitchGlide>, script: Option<&str>, only_key: Option<u8>) -> Engine {
    engine_sources(source.into_iter().collect(), script, only_key)
}
fn engine_sources(sources: Vec<PitchGlide>, script: Option<&str>, only_key: Option<u8>) -> Engine {
    let group = Group {
        pitch_glides: sources,
        ..Default::default()
    };
    let zone = Zone {
        low_key: only_key.unwrap_or(0),
        high_key: only_key.unwrap_or(127),
        root: 60,
        ..Default::default()
    };
    let bank = Bank::from_samples(
        vec![group],
        vec![zone],
        vec![(
            Default::default(),
            Sample {
                rate: 48000,
                frames: (0..65536)
                    .map(|i| {
                        let phase = std::f64::consts::TAU * i as f64 / 200.;
                        [(0.02 * phase.sin() + 0.006 * (2. * phase).sin()) as f32; 2]
                    })
                    .collect(),
            },
        )],
    )
    .unwrap();
    let mut e = Engine::default();
    e.reset(48000.);
    e.attack = 0.;
    e.release = 0.05;
    e.set_bank(Some(Box::new(bank)));
    if let Some(script) = script {
        let (runtime, errors) = crate::ksp::Runtime::with_scripts(
            &[script],
            &mut crate::ksp::LogEngine::new(Vec::new(), 48000.),
            1,
            Vec::new(),
        );
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        e.set_script(Some(Box::new(runtime)));
    }
    e
}
fn source(mode: GlideMode) -> PitchGlide {
    PitchGlide {
        slot: 3,
        mode,
        time_ms: 8.,
        depth: 1.,
        targets: vec![(0, 1.)],
        bypassed: false,
    }
}

// Closed-form control knots, not the production point/step recurrence. Exterior
// host tuning bypasses the Glide clock and integrates the same known source.
fn tuning(age: usize, interval: i16, mode: GlideMode, live: bool) -> f32 {
    if live && (64..128).contains(&age) {
        return 0.;
    }
    let interval = interval.clamp(-12, 12);
    let duration = 384.
        * match mode {
            GlideMode::Time => 1.,
            GlideMode::Speed => f64::from(interval.abs()),
        };
    let knot = |tick: usize| {
        let active = if live && tick >= 4 { tick - 2 } else { tick };
        let depth = if live && tick >= 4 { 0.5 } else { 1. };
        -f64::from(interval) * (1. - 32. * active as f64 / duration).max(0.) * depth
    };
    if age < 32 {
        return knot(0) as f32;
    }
    let tick = age / 32;
    // Resume interpolates from the last emitted (old-depth) knot.
    let previous = if live && tick == 4 {
        knot(1)
    } else {
        knot(tick - 1)
    };
    (previous + (knot(tick) - previous) * (age % 32) as f64 / 32.) as f32
}
fn close(actual: &[Vec<f32>; 2], expected: &[Vec<f32>; 2], label: &str) {
    let error = actual
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0_f32, f32::max);
    assert!(error < 3e-5, "{label}: complete waveform error {error}");
    assert!(expected.iter().flatten().any(|x| x.abs() > 0.01));
    assert!(
        actual
            .iter()
            .all(|channel| channel[END - 128..].iter().all(|x| *x == 0.))
    );
}
fn base(mode: GlideMode, interval: i16, live: bool, sizes: &[usize]) -> [Vec<f32>; 2] {
    let (mut actual, mut expected) = (
        engine(Some(source(mode)), None, Some(60)),
        engine(None, None, Some(60)),
    );
    let seed = note(1, 4, (60 - interval) as u8);
    let root = note(2, 7, 60);
    let mut pcm = [vec![0.; END], vec![0.; END]];
    let mut reference = [vec![0.; END], vec![0.; END]];
    let (mut at, mut block) = (0, 0);
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            // A raw root without a matching zone advances the shared history across
            // physical channels, without introducing an oracle voice.
            assert!(actual.host_note_on(seed, 4, seed.key, 127, 0.));
            assert!(actual.host_note_on(root, 7, 60, 127, 0.));
            assert!(expected.host_note_on(root, 7, 60, 127, 0.));
            while at < END {
                if at == 64 && live {
                    assert!(actual.write(Address::PitchSourceBypass(0, 3), 1.));
                }
                if at == 128 && live {
                    assert!(actual.write(
                        Address::PitchSourceIntensity {
                            group: 0,
                            slot: 3,
                            target: 0,
                            bipolar: false
                        },
                        0.5
                    ));
                    assert!(actual.write(Address::PitchSourceBypass(0, 3), 0.));
                }
                if at == HELD {
                    actual.host_note_off(pattern(root));
                    expected.host_note_off(pattern(root));
                }
                let boundary = [64, 128, HELD, END]
                    .into_iter()
                    .filter(|b| *b > at)
                    .min()
                    .unwrap();
                let count = sizes[block % sizes.len()].min(boundary - at);
                let [left, right] = &mut pcm;
                actual
                    .render_checked(&mut left[at..at + count], &mut right[at..at + count])
                    .unwrap();
                for frame in at..at + count {
                    expected.host_expression(
                        pattern(root),
                        HostExpression::Tune(tuning(frame, interval, mode, live)),
                    );
                    let [left, right] = &mut reference;
                    expected
                        .render_checked(&mut left[frame..frame + 1], &mut right[frame..frame + 1])
                        .unwrap();
                }
                at += count;
                block += 1;
            }
            actual.mark_host_notes();
            expected.mark_host_notes();
        }),
        0
    );
    close(&pcm, &reference, "analytic glide");
    assert_eq!(
        actual.host_note_end_at(root),
        expected.host_note_end_at(root)
    );
    assert!(actual.host_note_end_at(root).is_some());
    pcm
}
#[test]
fn glide_time_speed_clamp_and_live_source_controls_have_complete_pcm_without_heap() {
    for mode in [GlideMode::Time, GlideMode::Speed] {
        for interval in [12, -12, 24, -7] {
            let whole = base(mode, interval, false, &[128]);
            let fragmented = base(mode, interval, false, &[1, 7, 31, 127]);
            close(&whole, &fragmented, "fragmented glide");
        }
    }
    let whole = base(GlideMode::Time, 12, true, &[128]);
    let fragmented = base(GlideMode::Time, 12, true, &[1, 7, 31, 127]);
    close(&whole, &fragmented, "live source controls");
}

#[test]
fn positive_saved_glide_with_unrepresentable_duration_fails_before_pcm_commit() {
    let mut settings = source(GlideMode::Time);
    settings.time_ms = f32::from_bits(1);
    let mut e = engine(Some(settings), None, Some(60));
    let seed = note(3, 4, 48);
    let root = note(4, 7, 60);
    let (mut left, mut right) = ([1.; 128], [1.; 128]);
    let mut error = None;
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            assert!(e.host_note_on(seed, 4, 48, 127, 0.));
            assert!(e.host_note_on(root, 7, 60, 127, 0.));
            error = e.render_checked(&mut left, &mut right).err();
            e.mark_host_notes();
        }),
        0
    );
    assert_eq!(error.unwrap().kind, StreamErrorKind::UnrepresentablePitch);
    assert_eq!((left, right), ([0.; 128], [0.; 128]));
    assert_eq!(e.host_audio_end_at(root), Some(0));
    assert_eq!(e.host_note_end_at(root), None, "failed source does not release its physical held root");
    assert_eq!(e.active_voices(), 0);
    assert_eq!(crate::plugin::tests::allocations(|| {
        e.host_note_off(pattern(root));
        e.mark_host_notes();
    }), 0);
    assert_eq!(e.host_audio_end_at(root), Some(0));
    assert_eq!(e.host_note_end_at(root), Some(128), "host lifetime ends at actual key-up");
}

const HISTORY: &str = "on init\ndeclare $child := 0\nend on
on note
if ($EVENT_NOTE = 60 or $EVENT_NOTE = 36 or $EVENT_NOTE = 84)
ignore_event($EVENT_ID)
end if
if ($EVENT_NOTE = 72)
change_note($EVENT_ID,60)
end if
if ($EVENT_NOTE = 63)
ignore_event($EVENT_ID)
wait(1000)
$child := play_note(60,$EVENT_VELOCITY,0,-1)
end if
end on
on release
if ($EVENT_NOTE = 63)
note_off($child)
end if
end on";

fn history(sizes: &[usize]) -> [Vec<f32>; 2] {
    let (mut actual, mut expected) = (
        engine(Some(source(GlideMode::Time)), Some(HISTORY), None),
        engine(None, None, None),
    );
    let roots = [note(10, 4, 72), note(11, 7, 48), note(12, 4, 63)];
    let mut pcm = [vec![0.; END], vec![0.; END]];
    let mut reference = [vec![0.; END], vec![0.; END]];
    let (mut at, mut block) = (0, 0);
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            // The ignored raw 60 establishes history; changing raw72 to audible60
            // must retain the raw interval +12, not recapture a zero interval.
            assert!(actual.host_note_on(note(20, 7, 60), 7, 60, 127, 0.));
            assert!(actual.host_note_on(roots[0], 4, 72, 127, 0.));
            assert!(expected.host_note_on(roots[0], 4, 60, 127, 0.));
            while at < END {
                match at {
                    96 => {
                        assert!(actual.host_note_on(note(21, 4, 36), 4, 36, 127, 0.));
                        assert!(actual.host_note_on(roots[1], 7, 48, 127, 0.));
                        assert!(expected.host_note_on(roots[1], 7, 48, 127, 0.));
                    }
                    128 | 176 => {
                        for e in [&mut actual, &mut expected] {
                            e.host_expression(
                                pattern(roots[0]),
                                HostExpression::Gain(if at == 128 { 0. } else { 1. }),
                            );
                        }
                    }
                    192 => {
                        assert!(actual.host_note_on(roots[2], 4, 63, 127, 0.));
                    }
                    208 => {
                        assert!(actual.host_note_on(note(22, 7, 84), 7, 84, 127, 0.));
                    }
                    // The waited child captures the LIVE pair (84,60), clamped-12.
                    // It is owned by the exact parent63, rather than by raw60.
                    240 => {
                        assert!(expected.host_note_on(roots[2], 4, 60, 127, 0.));
                    }
                    256 => {
                        for e in [&mut actual, &mut expected] {
                            e.cc_from(4, 4, 64, 127);
                            for root in roots {
                                e.host_note_off(pattern(root));
                            }
                        }
                    }
                    768 => {
                        for e in [&mut actual, &mut expected] {
                            e.cc_from(4, 4, 64, 0);
                        }
                    }
                    _ => {}
                }
                let boundary = [96, 128, 176, 192, 208, 240, 256, 768, END]
                    .into_iter()
                    .filter(|b| *b > at)
                    .min()
                    .unwrap();
                let count = sizes[block % sizes.len()].min(boundary - at);
                let [l, r] = &mut pcm;
                actual
                    .render_checked(&mut l[at..at + count], &mut r[at..at + count])
                    .unwrap();
                for frame in at..at + count {
                    for (index, start, interval) in [(0, 0, 12), (1, 96, 12), (2, 240, -24)] {
                        if frame >= start {
                            expected.host_expression(
                                pattern(roots[index]),
                                HostExpression::Tune(tuning(
                                    frame - start,
                                    interval,
                                    GlideMode::Time,
                                    false,
                                )),
                            );
                        }
                    }
                    let [l, r] = &mut reference;
                    expected
                        .render_checked(&mut l[frame..frame + 1], &mut r[frame..frame + 1])
                        .unwrap();
                }
                at += count;
                block += 1;
            }
            actual.mark_host_notes();
            expected.mark_host_notes();
        }),
        0
    );
    close(&pcm, &reference, "raw and waited history");
    for root in roots {
        assert_eq!(
            actual.host_note_end_at(root),
            expected.host_note_end_at(root),
            "exact owner {:?} ended differently",
            root
        );
        assert!(actual.host_note_end_at(root).is_some());
    }
    pcm
}
#[test]
fn raw_history_live_waited_children_muted_clocks_and_physical_pedals_preserve_pcm_and_ends() {
    let whole = history(&[128]);
    let fragmented = history(&[1, 7, 31, 127]);
    close(&whole, &fragmented, "history partitions");
}

fn mixed_tuning(frame: usize) -> f32 {
    let knot = |tick: usize| {
        let other = -3. * (1. - tick as f64 / 12.).max(0.);
        let first = match tick {
            1 | 2 => 0., // bypass at16: absent from emissions at32 and64
            0 => -12.,
            _ => -12. * (1. - (tick - 2) as f64 / 12.).max(0.),
        };
        other + first
    };
    if frame < 32 {
        return knot(0) as f32;
    }
    let tick = frame / 32;
    (knot(tick - 1) + (knot(tick) - knot(tick - 1)) * (frame % 32) as f64 / 32.) as f32
}
fn mixed(sizes: &[usize]) -> [Vec<f32>; 2] {
    let mut other = source(GlideMode::Time);
    other.slot = 4;
    other.depth = 0.25;
    other.targets = vec![(0, 0.25)];
    let (mut actual, mut expected) = (
        engine_sources(vec![source(GlideMode::Time), other], None, Some(60)),
        engine(None, None, Some(60)),
    );
    let root = note(31, 7, 60);
    let mut pcm = [vec![0.; END], vec![0.; END]];
    let mut reference = [vec![0.; END], vec![0.; END]];
    let (mut at, mut block) = (0, 0);
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            assert!(actual.host_note_on(note(30, 4, 48), 4, 48, 127, 0.));
            for e in [&mut actual, &mut expected] {
                assert!(e.host_note_on(root, 7, 60, 127, 0.));
            }
            while at < END {
                if at == 16 || at == 80 {
                    assert!(actual.write(
                        Address::PitchSourceBypass(0, 3),
                        if at == 16 { 1. } else { 0. }
                    ));
                }
                if at == HELD {
                    for e in [&mut actual, &mut expected] {
                        e.host_note_off(pattern(root));
                    }
                }
                let boundary = [16, 80, HELD, END]
                    .into_iter()
                    .filter(|b| *b > at)
                    .min()
                    .unwrap();
                let count = sizes[block % sizes.len()].min(boundary - at);
                let [l, r] = &mut pcm;
                actual
                    .render_checked(&mut l[at..at + count], &mut r[at..at + count])
                    .unwrap();
                for frame in at..at + count {
                    expected
                        .host_expression(pattern(root), HostExpression::Tune(mixed_tuning(frame)));
                    let [l, r] = &mut reference;
                    expected
                        .render_checked(&mut l[frame..frame + 1], &mut r[frame..frame + 1])
                        .unwrap();
                }
                at += count;
                block += 1;
            }
            actual.mark_host_notes();
            expected.mark_host_notes();
        }),
        0
    );
    close(&pcm, &reference, "combined pitch destination");
    assert_eq!(
        actual.host_note_end_at(root),
        expected.host_note_end_at(root)
    );
    assert!(actual.host_note_end_at(root).is_some());
    pcm
}
#[test]
fn mixed_glide_bypass_retains_combined_destination_until_control_tick() {
    let whole = mixed(&[128]);
    let fragmented = mixed(&[1, 7, 31, 127]);
    close(&whole, &fragmented, "combined destination partitions");
}

#[path = "glide_direct_admission_tests.rs"]
mod direct_admission;
