//! Final native group admission is compared with explicit script-free group masks.
use crate::{
    audio::Sample,
    engine::{Bank, Engine, HostNote, HostPattern},
    import::{Group, Zone},
    midi::{Control, ControlValue},
    modulation::{ModAssignment, ModSource, ModTarget},
};
use ni_file::kontakt::objects::{StartCriteriaList, StartCriteriaParams};

fn row(mode: i32, next: i32, lo: i16, hi: i16, controller: i16, class: i32) -> StartCriteriaParams {
    StartCriteriaParams {
        mode,
        next_criteria: next,
        key_min: lo,
        key_max: hi,
        controller,
        cc_min: lo,
        cc_max: hi,
        cycle_class: class,
        slice_zone_idx: 0,
        slice_zone_slice_idx: 0,
        sequencer_only: false,
    }
}
fn criteria(rows: Vec<StartCriteriaParams>) -> StartCriteriaList {
    let list = StartCriteriaList {
        mask: (1 << rows.len()) - 1,
        items: rows,
        unknown_tail: vec![],
    };
    let mut bytes = Vec::new();
    list.write(&mut bytes).unwrap();
    StartCriteriaList::read(std::io::Cursor::new(bytes)).unwrap()
}
fn bank(native: bool) -> Bank {
    let rules = [
        vec![],
        vec![row(1, 0, 24, 26, 0, 0)],
        vec![row(1, 0, 26, 26, 0, 0)],
        vec![row(1, 0, 29, 30, 0, 0)],
        vec![
            row(2, 1, 64, 127, 1, 0),
            row(2, 2, 64, 127, 2, 0),
            row(2, 0, 64, 127, 3, 0),
        ],
        vec![row(3, 0, 0, 0, 0, 0)],
        vec![row(3, 0, 0, 0, 0, 1)],
        vec![row(3, 0, 0, 0, 0, 0)],
        vec![row(3, 0, 0, 0, 0, 1)],
        vec![row(2, 0, 1, 8191, 128, 0), row(2, 0, 64, 127, 129, 0)],
    ];
    let groups = rules
        .into_iter()
        .enumerate()
        .map(|(index, rows)| {
            let mut group = Group {
                name: format!("audible{index}"),
                release_trigger: matches!(index, 7 | 8),
                start_criteria: if native {
                    criteria(rows)
                } else {
                    Default::default()
                },
                ..Default::default()
            };
            if index == 4 {
                if native {
                    group.mods = vec![ModAssignment {
                        cc_default: None,
                        name: "accepted CC1 volume".into(),
                        source: ModSource::MidiCc(1),
                        target: ModTarget::Volume,
                        intensity: 0.5,
                        invert: false,
                        lag_ms: 0,
                        shaper: None,
                    }];
                } else {
                    group.gain = 0.5;
                }
            }
            group
        })
        .collect();
    let zones = (0..10)
        .map(|group| Zone {
            group,
            sample: format!("group{group}").into(),
            root: 60,
            ..Default::default()
        })
        .collect();
    let samples = (0..10)
        .map(|group| {
            (
                format!("group{group}").into(),
                Sample {
                    rate: 48000,
                    frames: (0..if matches!(group, 7 | 8) { 128 } else { 8192 })
                        .map(|i| {
                            let phase =
                                std::f64::consts::TAU * (group + 1) as f64 * i as f64 / 200.;
                            [(phase.sin() * 0.003) as f32; 2]
                        })
                        .collect(),
                },
            )
        })
        .collect();
    Bank::from_samples(groups, zones, samples).unwrap()
}
const SCRIPT: &str = r#"on init
declare $child
end on
on controller
if ($CC_NUM=1)
ignore_controller
end if
if ($CC_NUM=2)
wait(1000)
end if
if ($CC_NUM=21)
wait(500)
set_controller(3,%CC[21])
end if
end on
on note
disallow_group(0)
disallow_group(6)
disallow_group(7)
disallow_group(8)
if ($EVENT_NOTE=62)
set_event_par($EVENT_ID,$EVENT_PAR_MIDI_CHANNEL,8)
change_note($EVENT_ID,60)
end if
if ($EVENT_NOTE=63)
ignore_event($EVENT_ID)
wait(1000)
$child:=play_note(60,127,0,-1)
else
wait(500)
end if
end on
on release
if ($EVENT_NOTE=63)
note_off($child)
end if
end on"#;
fn engine(native: bool, bypass: bool) -> Engine {
    let mut e = Engine::default();
    e.reset(48000.);
    e.attack = 0.0001;
    e.release = 0.001;
    e.set_home_channel(7);
    e.set_bank(Some(Box::new(bank(native))));
    if native {
        let script = if bypass {
            SCRIPT.replace(
                "on init",
                "on init\nSET_CONDITION(NO_SYS_SCRIPT_GROUP_START)",
            )
        } else {
            SCRIPT.to_owned()
        };
        let (rt, errors) = crate::ksp::Runtime::with_scripts(
            &[&script],
            &mut crate::ksp::LogEngine::new(Vec::new(), 48000.),
            10,
            Vec::new(),
        );
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        e.set_script(Some(Box::new(rt)));
    }
    e
}
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
fn mask(e: &mut Engine, groups: &[usize]) {
    e.set_all_groups_allowed(false);
    for &group in groups {
        e.set_group_allowed(group, true);
    }
}
fn cc(e: &mut Engine, channel: u8, controller: u8, value: u8) {
    assert_eq!(
        crate::plugin::tests::allocations(|| e.control_from_received(
            channel,
            4,
            crate::midi::ReceivedControl::without_parameter(Control::Cc(controller, ControlValue::from_midi1(value)))
        )),
        0
    );
}
fn render(e: &mut Engine, pcm: &mut Vec<[f32; 2]>, frames: usize, partition: &[usize]) {
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
    e.mark_host_notes();
}
fn same(a: &[[f32; 2]], b: &[[f32; 2]]) {
    assert_eq!(a.len(), b.len());
    assert!(
        a.iter()
            .zip(b)
            .all(|(a, b)| (0..2).all(|ch| (a[ch] - b[ch]).abs() < 2e-5)),
        "native selection changed complete audible group PCM"
    );
}
fn step(
    a: &mut Engine,
    b: &mut Engine,
    pa: &mut Vec<[f32; 2]>,
    pb: &mut Vec<[f32; 2]>,
    n: usize,
    partition: &[usize],
) {
    render(a, pa, n, partition);
    render(b, pb, n, partition);
    same(pa, pb);
}
fn performance(partition: &[usize], bypass: bool) -> Vec<[f32; 2]> {
    let mut a = engine(true, bypass);
    let mut b = engine(false, false);
    let (mut p, mut q) = (Vec::new(), Vec::new());
    // These masks are musical expectations, independent of the prepared selector.
    let cases: &[(u8, &[usize])] = if bypass {
        &[(60, &[1, 2, 3, 4, 5, 7, 8, 9])]
    } else {
        &[
            (60, &[1, 5, 7]),    // saved-negative/absent key defaults to first key24; RR0
            (26, &[1, 2, 8]),    // overlapping keys, user disallows ordinary RR1
            (27, &[5, 7]),       // inside combined span but in a gap: clears key groups
            (60, &[8]),          // outside key span preserves the gap; release-only owner
            (29, &[3, 5, 7]),    // same event updates shared key before predicates
            (60, &[3, 8]),       // A AND NOT B OR C: 1 AND NOT1 OR0 is false
            (60, &[3, 4, 5, 7]), // authored C=1 makes the independent OR branch true
            (60, &[3, 4, 8]),    // A=1 B=0 C=0 admits the ANDNOT branch
            (62, &[3, 5, 7, 9]), // final mutable channel8 sees its own signedbend/pressure
            (63, &[3, 4, 8]),    // ignored parent does not advance; generated child advances once
        ]
    };
    for (index, (key, expected)) in cases.iter().enumerate() {
        if !bypass {
            match index {
                5 => {
                    cc(&mut a, 7, 1, 127);
                    cc(&mut a, 7, 2, 127);
                }
                6 => {
                    cc(&mut a, 7, 21, 127);
                    step(&mut a, &mut b, &mut p, &mut q, 48, partition);
                }
                7 => {
                    cc(&mut a, 7, 2, 0);
                    cc(&mut a, 7, 3, 0);
                }
                8 => {
                    a.pitch_bend_from(8, 4, 12288);
                    b.pitch_bend_from(8, 4, 12288);
                    a.channel_pressure_from(8, 4, 64);
                }
                _ => {}
            }
        }
        mask(&mut b, expected);
        let note = host(610 + index as i32, *key);
        let final_key = if *key >= 62 { 60 } else { *key };
        assert_eq!(
            crate::plugin::tests::allocations(|| assert!(a.host_note_on(note, 7, *key, 127, 0.))),
            0
        );
        if *key == 63 {
            // Explicit clock reference for the waited generated child.
            step(&mut a, &mut b, &mut p, &mut q, 48, partition);
        }
        assert!(b.host_note_on(note, if *key == 62 { 8 } else { 7 }, final_key, 127, 0.));
        step(&mut a, &mut b, &mut p, &mut q, 256, partition);
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                a.host_note_off(pattern(note));
                b.host_note_off(pattern(note));
            }),
            0
        );
        step(&mut a, &mut b, &mut p, &mut q, 2048, partition);
        assert_eq!(
            a.host_note_end_at(note),
            b.host_note_end_at(note),
            "case{index}: exact owner end"
        );
        assert!(
            a.host_note_end_at(note).is_some(),
            "case{index}: all ordinary and release groups must retire"
        );
    }
    assert!(
        p.iter().any(|frame| frame[0].abs() > 0.001),
        "group fixture must contain audible source"
    );
    p
}
#[test]
fn native_group_admission_preserves_key_raw_cc_rr_release_and_exact_owner_pcm() {
    for bypass in [false, true] {
        let fixed = performance(&[128], bypass);
        same(&fixed, &performance(&[1, 7, 31, 127], bypass));
    }
}
