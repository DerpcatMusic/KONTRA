//! Received mode/reset scope is physical, even when user KSP hears a home.
use super::*;
use crate::articulate::{Articulate, Articulation, Mode, Mpe, Route};

const SCRIPT: &str = "on init\ndeclare %first[16]\ndeclare %child[128]\ndeclare %resumed[16]\nmake_persistent(%resumed)\nend on\non note\n%first[$MIDI_CHANNEL] := $EVENT_ID\n%child[$EVENT_NOTE] := play_note($EVENT_NOTE+12,$EVENT_VELOCITY,0,-1)\nend on\non release\nnote_off(%child[$EVENT_NOTE])\nend on\non controller\nif ($CC_NUM=1 and %CC[1]=99)\nwait(100000)\ninc(%resumed[$MIDI_CHANNEL])\nchange_vol(%first[$MIDI_CHANNEL],-12000,0)\nend if\nend on";

fn home(p: &mut Performance, channel: u8) {
    p.dsp.rack.controls[0].channel = i16::from(channel);
    p.dsp.rack.parts[0]
        .instrument_mut()
        .unwrap()
        .set_home_channel(channel);
}

fn fixture(partition: &'static [usize], scripted: bool, target: u8) -> Performance {
    let mut p = Performance::new(false, partition);
    // A pure fundamental avoids source harmonics colliding with octave owners.
    // WINDOW12800 spans eight periods of the lowest30Hz owner, keeping each
    // isolated owner's measured amplitude independent of its source phase.
    let frames: Vec<[f32; 2]> = (0..8_000)
        .map(|i| {
            let phase = std::f64::consts::TAU * i as f64 / 200.;
            [(0.02 * phase.sin()) as f32; 2]
        })
        .collect();
    let zones = [(0, 83, "left"), (84, 127, "right")]
        .into_iter()
        .map(|(low_key, high_key, path)| Zone {
            sample: PathBuf::from(path),
            low_key,
            high_key,
            root: 60,
            loop_range: Some(Loop {
                start: 0,
                end: 8_000,
                crossfade: 0,
                until_release: false,
                alternating: false,
            }),
            ..Zone::default()
        })
        .collect();
    let samples = ["left", "right"]
        .into_iter()
        .enumerate()
        .map(|(side, path)| {
            (
                PathBuf::from(path),
                Sample {
                    rate: RATE as u32,
                    frames: frames
                        .iter()
                        .map(|frame| {
                            if side == 0 {
                                [frame[0], 0.]
                            } else {
                                [0., frame[1]]
                            }
                        })
                        .collect(),
                },
            )
        })
        .collect();
    p.dsp.rack.parts[0]
        .instrument_mut()
        .unwrap()
        .set_bank(Some(Box::new(
            Bank::from_samples(vec![Group::default()], zones, samples).unwrap(),
        )));
    if scripted {
        install_performance_script(&mut p, SCRIPT);
    }
    let art = Articulate {
        source: "cleanup-fixture".into(),
        mode: Mode::Channel,
        articulations: (0..16)
            .map(|channel| Articulation {
                channel,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    p.dsp.routers[0].set_route(Route::new("cleanup-fixture", &art, &Mpe::default()));
    home(&mut p, target);
    let mut events = EventList::with_capacity(16);
    for manager in [0, 15] {
        for (controller, value) in [(101, 0), (100, 6), (6, 3), (101, 127), (100, 127)] {
            cc(&mut events, manager, controller, value);
        }
    }
    p.stage(
        "routed dual MPE negotiation",
        "physical lower0members1..3 and upper15members12..14",
        &events,
    );
    assert_eq!(
        p.dsp.rack.parts[0]
            .instrument()
            .unwrap()
            .mpe_zones()
            .manager(0)
            .unwrap()
            .members,
        3
    );
    assert_eq!(
        p.dsp.rack.parts[0]
            .instrument()
            .unwrap()
            .mpe_zones()
            .manager(15)
            .unwrap()
            .members,
        3
    );
    p
}

fn right_matches(p: &Performance, reference: &Performance, start: usize) {
    assert_eq!(p.pcm.len(), reference.pcm.len());
    assert!(
        p.pcm[start..]
            .iter()
            .zip(&reference.pcm[start..])
            .all(|(a, b)| (a[1] - b[1]).abs() < 1e-7),
        "received physical cleanup changed complete PCM of unrelated upper-zone owner/child/wait"
    );
    assert!(
        !p.ended.contains(&address(470, 13, 84)),
        "unrelated upper exact owner ended"
    );
}

fn matrix(partition: &'static [usize], scripted: bool, alias: bool, controller: u8) -> Performance {
    let target = if alias { 15 } else { 7 };
    let selected = if alias { 2 } else { 0 };
    let owners: &[u8] = if alias { &[2] } else { &[0, 1, 2, 3] };
    let mut p = fixture(partition, scripted, target);
    let mut reference = fixture(partition, scripted, target);
    let mut events = EventList::with_capacity(32);
    for (owner, key) in [(0, 24), (1, 36), (2, 48), (3, 60)] {
        note(
            &mut events,
            ExactNoteKind::On,
            460 + owner,
            owner as i16,
            key,
        );
    }
    p.stage(
        "captured lower MPE homes",
        "physical0..3 exact roots delivered to home7 or alias15",
        &events,
    );
    reference.stage(
        "unaffected upper source clock",
        "no left-zone roots in independent reference",
        &EventList::with_capacity(0),
    );
    let neutral: Vec<_> = [30., 60., 120., 240., 480.]
        .into_iter()
        .map(|hz| amplitude(p.measured(), 0, hz))
        .collect();
    assert!(neutral.iter().take(4).all(|level| *level > 0.0001));
    home(&mut p, 13);
    home(&mut reference, 13);
    events.clear();
    note(&mut events, ExactNoteKind::On, 470, 13, 84);
    p.stage(
        "unrelated upper owner",
        "upper member13 is captured on logical13 and has independent right PCM",
        &events,
    );
    reference.stage(
        "independent upper owner",
        "same source and exact upper root",
        &events,
    );
    let upper = p.pcm.len() - WINDOW;
    right_matches(&p, &reference, upper);
    home(&mut p, target);
    home(&mut reference, target);

    events.clear();
    for &owner in owners {
        cc(&mut events, owner, 64, 127);
        cc(&mut events, owner, 66, 127);
        cc(&mut events, owner, 1, 99);
        events.push(Event::on_port(
            0,
            0,
            EventBody::PitchBend {
                group: 0,
                channel: owner,
                value: 12288,
            },
        ));
        events.push(Event::on_port(
            0,
            0,
            EventBody::ChannelPressure {
                group: 0,
                channel: owner,
                pressure: 0,
            },
        ));
    }
    // This wait belongs to an unrelated physical upper owner and captured
    // logical13, so lower cleanup must not cancel its eventual audible edit.
    home(&mut p, 13);
    home(&mut reference, 13);
    let mut unaffected = EventList::with_capacity(4);
    cc(&mut unaffected, 13, 1, 99);
    p.stage_for(
        "upper wait before lower cleanup",
        "unrelated upper callback remains pending",
        &unaffected,
        128,
        0,
    );
    reference.stage_for(
        "independent upper wait",
        "same pending callback",
        &unaffected,
        128,
        0,
    );
    home(&mut p, target);
    home(&mut reference, target);
    p.stage_for(
        "lower pressure bend pedals and waits",
        "selected physical rows change while exact routes remain captured",
        &events,
        864,
        640,
    );
    reference.stage_for(
        "unaffected upper clock before cleanup",
        "independent upper callback still waiting",
        &EventList::with_capacity(0),
        864,
        640,
    );
    let cleanup = p.pcm.len();
    events.clear();
    cc(&mut events, selected, controller, 0);
    p.stage(
        "received routed cleanup",
        "physical manager/member scope must not be recomputed from home channel",
        &events,
    );
    reference.stage(
        "independent unaffected upper after cleanup",
        "upper wait resumes at its original deadline",
        &EventList::with_capacity(0),
    );
    right_matches(&p, &reference, cleanup);
    if controller == 121 {
        for &owner in owners {
            for cc in [1, 64, 66] {
                assert_eq!(
                    p.dsp.rack.parts[0].instrument().unwrap().cc_state()[owner as usize][cc],
                    0,
                    "CC121 must reset accepted physical cache owner{owner} controller{cc}"
                );
            }
        }
        if !scripted {
            for (i, hz) in [30., 60., 120., 240., 480.].into_iter().enumerate() {
                assert!(
                    (amplitude(p.measured(), 0, hz) - neutral[i]).abs() < 2e-6,
                    "reset did not restore selected pressure/bend or canceled wait changed live root"
                );
            }
        }
        // Scripted octave children overlap neighboring roots. Different MPE
        // bends change their relative phases, so restored individual gains need
        // not restore the coherent sum. isolated_scripted_reset checks each
        // family's two distinct fundamentals without that interference.
        assert_eq!(
            p.dsp.rack.parts[0].active_voices(),
            if scripted { 10 } else { 5 }
        );
        assert!(
            owners.iter().all(|owner| !p.ended.contains(&address(
                460 + i32::from(*owner),
                i16::from(*owner),
                24 + i16::from(*owner) * 12
            ))),
            "CC121 must retain held exact keys"
        );
    } else if controller == 120 {
        // Sound-off retires audible roots and descendants, while the held
        // physical root remains available for its eventual exact Off cleanup.
        assert_eq!(
            p.dsp.rack.parts[0].active_voices(),
            (5 - owners.len()) * if scripted { 2 } else { 1 },
            "physical sound-off left a selected root or generated child sounding"
        );
        for &owner in owners {
            assert!(
                !p.ended.contains(&address(
                    460 + i32::from(owner),
                    i16::from(owner),
                    24 + i16::from(owner) * 12
                )),
                "physical sound-off prematurely retired held owner{owner}"
            );
        }
    } else {
        assert!(
            owners.iter().all(|owner| !p.ended.contains(&address(
                460 + i32::from(*owner),
                i16::from(*owner),
                24 + i16::from(*owner) * 12
            ))),
            "channel-mode releases must respect physical sustain"
        );
    }
    if scripted && matches!(controller, 120 | 121) {
        let saved = p.dsp.rack.parts[0]
            .instrument()
            .unwrap()
            .script()
            .unwrap()
            .persistence();
        let crate::ksp::Value::IntArray(resumed) = &saved[0]["%resumed"] else {
            panic!("wait proof")
        };
        assert_eq!(
            resumed[target as usize], 0,
            "selected physical waits survived routed cleanup"
        );
        assert_eq!(
            resumed[13], 1,
            "unrelated upper wait was canceled or delivered to wrong home"
        );
    }
    events.clear();
    for &owner in owners {
        cc(&mut events, owner, 64, 0);
        cc(&mut events, owner, 66, 0);
    }
    p.stage(
        "routed physical pedal release",
        "released mode owners and generated descendants finish on their own physical pedals",
        &events,
    );
    reference.stage(
        "unaffected upper pedal-release clock",
        "upper remains held",
        &EventList::with_capacity(0),
    );
    right_matches(&p, &reference, cleanup);
    if !matches!(controller, 120 | 121) {
        for &owner in owners {
            assert!(p.ended.contains(&address(
                460 + i32::from(owner),
                i16::from(owner),
                24 + i16::from(owner) * 12
            )));
        }
    }
    if controller == 120 {
        for &owner in owners {
            assert!(!p.ended.contains(&address(
                460 + i32::from(owner),
                i16::from(owner),
                24 + i16::from(owner) * 12
            )));
        }
    }
    if alias {
        for owner in [0, 1, 3] {
            assert!(
                !p.ended
                    .contains(&address(460 + owner, owner as i16, 24 + owner as i16 * 12)),
                "physical2 delivered to15 must not stop another lower member or alias upper manager15"
            );
        }
    }
    events.clear();
    for (owner, key) in [(0, 24), (1, 36), (2, 48), (3, 60)] {
        note(
            &mut events,
            ExactNoteKind::Off,
            460 + owner,
            owner as i16,
            key,
        );
    }
    note(&mut events, ExactNoteKind::Off, 470, 13, 84);
    assert!(p
        .stage(
            "routed cleanup final exact Offs",
            "all original owners and generated children retire once",
            &events
        )
        .iter()
        .all(|v| *v < 1e-8));
    let mut upper_off = EventList::with_capacity(1);
    note(&mut upper_off, ExactNoteKind::Off, 470, 13, 84);
    reference.stage(
        "independent upper final Off",
        "same upper exact end clock",
        &upper_off,
    );
    assert_eq!(p.ended.len(), 5);
    for (owner, key) in [(0, 24), (1, 36), (2, 48), (3, 60)] {
        assert!(p.ended.contains(&address(460 + owner, owner as i16, key)));
    }
    let upper_index = p
        .ended
        .iter()
        .position(|a| *a == address(470, 13, 84))
        .unwrap();
    assert_eq!(p.ended_frames[upper_index], reference.ended_frames[0]);
    p
}

fn isolated_scripted_reset(partition: &'static [usize], alias: bool, owner: u8) -> Performance {
    let target = if alias { 15 } else { 7 };
    let selected = if alias { 2 } else { 0 };
    let owners: &[u8] = if alias { &[2] } else { &[0, 1, 2, 3] };
    assert!(owners.contains(&owner));
    let mut p = fixture(partition, true, target);
    let mut events = EventList::with_capacity(32);
    let id = 460 + i32::from(owner);
    let key = 24 + i16::from(owner) * 12;
    note(&mut events, ExactNoteKind::On, id, i16::from(owner), key);
    p.stage(
        "isolated scripted lower family",
        "one exact root and its unchanged octave child have distinct fundamentals",
        &events,
    );
    let frequencies = [
        30. * 2_f64.powi(i32::from(owner)),
        60. * 2_f64.powi(i32::from(owner)),
    ];
    // WINDOW contains complete periods of both fundamentals. Their amplitudes
    // are independent of the phases accumulated during manager/member bends.
    let neutral = frequencies.map(|hz| amplitude(p.measured(), 0, hz));
    assert!(neutral.into_iter().all(|level| level > 0.0001));
    assert_eq!(p.dsp.rack.parts[0].active_voices(), 2);
    events.clear();
    for &physical in owners {
        cc(&mut events, physical, 64, 127);
        cc(&mut events, physical, 66, 127);
        cc(&mut events, physical, 1, 99);
        events.push(Event::on_port(
            0,
            0,
            EventBody::PitchBend {
                group: 0,
                channel: physical,
                value: 12288,
            },
        ));
        events.push(Event::on_port(
            0,
            0,
            EventBody::ChannelPressure {
                group: 0,
                channel: physical,
                pressure: 0,
            },
        ));
    }
    p.stage_for(
        "isolated lower pressure bend pedals and waits",
        "same physical control sequence as the complete owner matrix",
        &events,
        864,
        640,
    );
    events.clear();
    cc(&mut events, selected, 121, 0);
    p.stage(
        "isolated received reset",
        "reset restores each root and octave child's gain and pitch and cancels its wait",
        &events,
    );
    for (hz, expected) in frequencies.into_iter().zip(neutral) {
        assert!(
            (amplitude(p.measured(), 0, hz) - expected).abs() < 2e-6,
            "isolated physical owner{owner} reset did not restore its {hz}Hz fundamental"
        );
    }
    let engine = p.dsp.rack.parts[0].instrument().unwrap();
    assert_eq!(engine.active_voices(), 2);
    for &physical in owners {
        for controller in [1, 64, 66] {
            assert_eq!(engine.cc_state()[physical as usize][controller], 0);
        }
    }
    let saved = engine.script().unwrap().persistence();
    let crate::ksp::Value::IntArray(resumed) = &saved[0]["%resumed"] else {
        panic!("isolated wait proof")
    };
    assert_eq!(resumed[target as usize], 0);
    assert!(
        p.ended.is_empty(),
        "reset must retain the isolated held exact root"
    );
    events.clear();
    note(&mut events, ExactNoteKind::Off, id, i16::from(owner), key);
    assert!(p
        .stage(
            "isolated final exact Off",
            "root and generated child retire after reset without a retained pedal",
            &events,
        )
        .into_iter()
        .all(|level| level < 1e-8));
    assert_eq!(p.dsp.rack.parts[0].active_voices(), 0);
    assert_eq!(p.ended, vec![address(id, i16::from(owner), key)]);
    p
}

#[test]
fn received_mpe_cleanup_uses_physical_zones_and_captured_homes_through_plugin_pcm() {
    for alias in [false, true] {
        let owners: &[u8] = if alias { &[2] } else { &[0, 1, 2, 3] };
        for &owner in owners {
            let fixed = isolated_scripted_reset(&[128], alias, owner);
            let fragmented = isolated_scripted_reset(&[1, 7, 31, 127], alias, owner);
            assert_eq!(fixed.ended, fragmented.ended);
            assert_eq!(fixed.ended_frames, fragmented.ended_frames);
            assert_eq!(fixed.pcm.len(), fragmented.pcm.len());
            assert!(
                fixed
                    .pcm
                    .iter()
                    .zip(&fragmented.pcm)
                    .all(|(a, b)| { [0, 1].into_iter().all(|ch| (a[ch] - b[ch]).abs() < 2e-6) }),
                "isolated scripted reset complete PCM depends on callback partition"
            );
        }
        for scripted in [false, true] {
            for controller in [121, 120, 123, 124, 125, 126, 127] {
                let fixed = matrix(&[128], scripted, alias, controller);
                let fragmented = matrix(&[1, 7, 31, 127], scripted, alias, controller);
                assert_eq!(fixed.ended, fragmented.ended);
                assert_eq!(fixed.ended_frames, fragmented.ended_frames);
                assert_eq!(fixed.pcm.len(), fragmented.pcm.len());
                assert!(
                    fixed
                        .pcm
                        .iter()
                        .zip(&fragmented.pcm)
                        .all(|(a, b)| [0, 1].into_iter().all(|ch| (a[ch] - b[ch]).abs() < 2e-6)),
                    "MPE routed cleanup complete PCM depends on callback partition"
                );
            }
        }
    }
}
