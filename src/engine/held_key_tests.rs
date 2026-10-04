//! Scoped physical cleanup preserves an exact survivor's delivered key latch.
use super::{Bank, Engine, HostExpression, HostNote, HostPattern};
use crate::{
    audio::Sample,
    import::{Group, Zone},
    midi::{Control, ControlValue, ReceivedControl},
};

fn note(input: u8, id: i32, key: u8) -> HostNote {
    HostNote {
        port: 0,
        channel: input,
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
fn rig(mpe: bool) -> Engine {
    let frames = (0..8192)
        .map(|i| {
            let p = std::f64::consts::TAU * i as f64 / 200.;
            [(0.15 * p.sin() + 0.025 * (3. * p).sin()) as f32; 2]
        })
        .collect();
    let bank = Bank::from_samples(
        vec![Group::default()],
        vec![Zone {
            root: 60,
            ..Default::default()
        }],
        vec![(
            Default::default(),
            Sample {
                rate: 48000,
                frames,
            },
        )],
    )
    .unwrap();
    let mut e = Engine::default();
    e.reset(48000.);
    e.attack = 0.;
    e.release = 0.005;
    e.set_bank(Some(Box::new(bank)));
    if mpe {
        e.configure_mpe(Some((0, 3, 48)));
    }
    e
}
fn run(mpe: bool, sizes: &[usize]) -> (Vec<[f32; 2]>, [Option<u64>; 3]) {
    let (mut actual, mut reference) = (rig(mpe), rig(mpe));
    let (selected, survivor, accent) = (note(4, 10, 60), note(5, 11, 60), note(6, 12, 67));
    let mut pcm = vec![[0.; 2]; 4096];
    let (mut at, mut block) = (0, 0);
    while at < pcm.len() {
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                match at {
                    0 => {
                        for e in [&mut actual, &mut reference] {
                            assert!(e.host_note_on(selected, 7, 60, 127, 0.));
                            e.host_expression(pattern(selected), HostExpression::Pan(-1.));
                            assert!(e.host_note_on(survivor, 7, 60, 127, 0.));
                            e.host_expression(pattern(survivor), HostExpression::Pan(1.));
                        }
                    }
                    128 => {
                        if mpe {
                            actual.configure_mpe(Some((0, 4, 48)));
                        } else {
                            actual.control_from_received(
                                7,
                                4,
                                ReceivedControl::without_parameter(Control::Cc(
                                    120,
                                    ControlValue::ZERO,
                                )),
                            );
                        }
                        // Exact targeted choke is an independent public ownership path;
                        // both selected cleanup modes have the ordinary 5ms fade here.
                        reference.host_note_choke(pattern(selected));
                        assert_eq!(actual.host_key_held(4, 60), !mpe);
                        assert!(
                            actual
                                .player
                                .host_notes
                                .active
                                .iter()
                                .filter_map(|r| actual.player.host_notes.get(*r))
                                .any(|o| o.note == selected && o.silenced)
                        );
                        assert!(actual.host_key_held(5, 60));
                        assert!(
                            actual.key_down(7, 60),
                            "physical4 stop erased physical5 exact delivered latch"
                        );
                    }
                    256 => {
                        assert!(
                            actual.key_down(7, 60),
                            "selected fade still must preserve the surviving held root"
                        );
                    }
                    512 => {
                        // A foreground host articulation uses the actual public key
                        // latch to add an accent while the original exact root is held.
                        // The independent reference schedules that known accent directly.
                        assert!(reference.host_note_on(accent, 7, 67, 127, 0.));
                        if actual.key_down(7, 60) {
                            assert!(actual.host_note_on(accent, 7, 67, 127, 0.));
                        }
                        actual.host_note_off(pattern(selected));
                        reference.host_note_off(pattern(selected));
                        assert!(!actual.host_key_held(4, 60));
                        assert!(actual.key_down(7, 60));
                    }
                    768 => {
                        for e in [&mut actual, &mut reference] {
                            e.host_note_off(pattern(accent));
                        }
                    }
                    900 => {
                        for e in [&mut actual, &mut reference] {
                            e.host_expression(pattern(survivor), HostExpression::Tune(7.));
                            e.host_expression(pattern(survivor), HostExpression::Gain(0.5));
                        }
                    }
                    1024 => {
                        for e in [&mut actual, &mut reference] {
                            e.cc_from(7, 5, 64, 127);
                        }
                    }
                    1100 => {
                        for e in [&mut actual, &mut reference] {
                            e.host_note_off(pattern(survivor));
                        }
                        assert!(!actual.key_down(7, 60));
                        assert!(actual.host_note_end_at(survivor).is_none());
                    }
                    1280 => {
                        for e in [&mut actual, &mut reference] {
                            e.cc_from(7, 5, 64, 0);
                        }
                    }
                    _ => {}
                }
            }),
            0
        );
        let end = [128, 256, 512, 768, 900, 1024, 1100, 1280, 4096]
            .into_iter()
            .filter(|x| *x > at)
            .min()
            .unwrap();
        let n = sizes[block % sizes.len()].min(end - at);
        let (mut l, mut r, mut x, mut y) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                actual.render(&mut l[..n], &mut r[..n]);
                reference.render(&mut x[..n], &mut y[..n]);
                actual.mark_host_notes();
                reference.mark_host_notes();
            }),
            0
        );
        for i in 0..n {
            pcm[at + i] = [l[i], r[i]];
            assert!(
                (l[i] - x[i]).abs() < 2e-6 && (r[i] - y[i]).abs() < 2e-6,
                "held exact mpe{mpe} frame{}",
                at + i
            );
        }
        at += n;
        block += 1;
    }
    let ends = [selected, survivor, accent].map(|n| actual.host_note_end_at(n));
    assert_eq!(actual.host_audio_end_at(selected), Some(368));
    assert_eq!(reference.host_audio_end_at(selected), Some(368));
    // CC120 silences the selected voice but keeps its exact physical owner
    // pinned until the later Off. MPE reconciliation relinquishes that owner.
    assert_eq!(ends[0], Some(if mpe { 368 } else { 512 }));
    assert_eq!(reference.host_note_end_at(selected), Some(368));
    for (i, note) in [(1, survivor), (2, accent)] {
        assert_eq!(ends[i], reference.host_note_end_at(note));
    }
    assert!(ends[1].is_some_and(|at| at > 1280));
    assert!(ends[2].is_some());
    assert!(
        pcm[512..768].iter().any(|p| p[0].abs() > 0.01),
        "foreground accent must be audible after scoped cleanup"
    );
    assert!(
        pcm[1100..1280].iter().any(|p| p[1].abs() > 0.01),
        "unrelated exact owner must retain physical5 sustain"
    );
    assert!(actual.player.voices.is_empty());
    assert!(!actual.key_down(7, 60));
    (pcm, ends)
}
#[test]
fn scoped_cc120_and_mpe_reconciliation_preserve_unrelated_exact_latched_key_and_future_pcm_without_heap()
 {
    for mpe in [false, true] {
        let (fixed, ends) = run(mpe, &[128]);
        let (fragmented, fragmented_ends) = run(mpe, &[1, 7, 31, 127]);
        assert_eq!(ends, fragmented_ends);
        assert!(
            fixed
                .iter()
                .zip(fragmented)
                .all(|(a, b)| (a[0] - b[0]).abs() < 2e-6 && (a[1] - b[1]).abs() < 2e-6)
        );
    }
}
