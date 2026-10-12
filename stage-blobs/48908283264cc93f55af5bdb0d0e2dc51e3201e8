//! Source compatibility is independent of authored parameter layers and residency.
use crate::{
    audio::Sample,
    engine::{
        Bank, Engine, HostNote, HostPattern,
        overrides::{Override, Param},
        params::{Address, GroupPar, Stage},
    },
    fx::{Chain, Effect, Kind, Params, params::Filter},
    import::{DigitalLfo, FlexEnvelope, FlexPoint, Group, Modulator, PitchLfo, VolumeLfo, Zone},
    modulation::{Ahdsr, GlideMode, ModAssignment, ModEnvelope, ModSource, ModTarget, PitchGlide},
};

const SWITCH: usize = 613;
const END: usize = 12_000;

fn group() -> Group {
    let source = PitchLfo { extended_fade: false, fade_sync: None, shared: false,
        start_phase: 0.25,
        slot: 3,
        count: 1.,
        note_value: 1. / 64.,
        sine: 0.25,
        digital: Some(DigitalLfo {
            rectangle: 0.5,
            triangle: -0.25,
            saw: 0.25,
            normalize: true,
            pulse_width: 0.5,
        }),
        fade_ms: 40.,
        depth: 0.125,
        targets: vec![(0, 0.125)],
        bypassed: false,
    };
    let mut volume_source = source.clone();
    volume_source.slot = 5;
    let target = |target, intensity| ModAssignment {
        name: "internal envelope target".into(),
        source: ModSource::Unassigned,
        cc_default: None,
        target,
        intensity,
        invert: false,
        lag_ms: 0,
        shaper: None,
    };
    Group {
        gain: 0.8,
        volume_env: Some(Ahdsr {
            attack_curve: 0.,
            attack_ms: 0.,
            hold_ms: 0.,
            decay_ms: 0.,
            sustain: 1.,
            release_ms: 10.,
            unknown_flag: 0,
            unknown_tail: Vec::new(),
        }),
        flex_env: Some(FlexEnvelope {
            points: vec![
                FlexPoint {
                    time_ms: 20.,
                    level: 1.,
                    curve: 0.5,
                },
                FlexPoint {
                    time_ms: 100.,
                    level: 0.8,
                    curve: 0.5,
                },
                FlexPoint {
                    time_ms: 80.,
                    level: 0.,
                    curve: 0.5,
                },
            ],
            sustain: 1,
            unknown_index: 0,
            unknown_tail: Vec::new(),
        }),
        pitch_lfos: vec![source.clone()],
        pitch_glides: vec![PitchGlide {
            slot: 6,
            mode: GlideMode::Time,
            time_ms: 100.,
            depth: 0.125,
            targets: vec![(0, 0.125)],
            bypassed: false,
        }],
        volume_lfos: vec![VolumeLfo {
            source: volume_source,
            target: 1,
            intensity: 0.4,
            negative: false,
            lag_ms: 15,
        }],
        mods: vec![ModAssignment {
            name: "receipt-sensitive volume".into(),
            source: ModSource::MidiCc(1),
            cc_default: Some(-1),
            target: ModTarget::Volume,
            intensity: 1.,
            invert: false,
            lag_ms: 0,
            shaper: None,
        }],
        envelopes: vec![ModEnvelope {
            native_ahd_only: false,
            env: Ahdsr {
                attack_curve: 0.,
                attack_ms: 30.,
                hold_ms: 0.,
                decay_ms: 70.,
                sustain: 0.5,
                release_ms: 50.,
                unknown_flag: 0,
                unknown_tail: Vec::new(),
            },
            targets: vec![
                target(ModTarget::Pitch, 0.125),
                target(
                    ModTarget::Module {
                        param: "filterCutoff".into(),
                        slot: 0,
                    },
                    0.3,
                ),
            ],
        }],
        modulators: vec![Modulator {
            name: "pitch and cutoff envelope".into(),
            targets: vec!["pitch".into(), "filterCutoff".into()],
            assignments: None,
            volume_env: false,
            bypassed: false,
            flex: false, primary_flex: false,
            envelope: Some(0),
            kind: "ahdsr".into(),
        }],
        fx: Chain {
            slots: vec![Effect {
                slot: 0,
                kind: Kind::Filter,
                version: 0,
                bypass: false,
                output_gain: 1.,
                dry_level: 0.,
                params: Params::Filter(Filter {
                    filter_type: 2,
                    cutoff: 0.2,
                    resonance: 0.1,
                    extra: [0.; 3],
                    native_flag: None,
                }),
            }],
        },
        ..Default::default()
    }
}

fn bank(group: Group) -> Box<Bank> {
    Box::new(
        Bank::from_samples(
            vec![group],
            vec![Zone {
                low_key: 60,
                high_key: 60,
                ..Default::default()
            }],
            vec![(
                Default::default(),
                Sample {
                    rate: 48_000,
                    frames: (0..32_768)
                        .map(|at| [0.125 + at as f32 / 65_536., 0.25 - at as f32 / 131_072.])
                        .collect(),
                },
            )],
        )
        .unwrap(),
    )
}

fn engine_group(group: Group) -> Engine {
    let mut engine = Engine::default();
    engine.reset(48_000.);
    // A previously installed custom effective source must survive a genuine
    // fill; the immutable Group and authored base retain the original saw.
    let prepared = (*bank(group))
        .with_prepared_settings(|rows| {
            rows[0].pitch_lfos[0].digital.as_mut().unwrap().saw = -0.5;
        })
        .unwrap();
    engine.set_bank(Some(Box::new(prepared)));
    engine
}

fn pattern(note: HostNote) -> HostPattern {
    HostPattern {
        port: i32::from(note.port),
        channel: i32::from(note.channel),
        key: i32::from(note.key),
        id: note.id,
        clap: note.clap,
    }
}

fn authored(engine: &mut Engine) {
    assert!(engine.write(Address::Group(0, GroupPar::Volume), 0.625));
    assert!(engine.write(Address::Envelope(0, Stage::Sustain), 0.75));
    assert!(engine.write(Address::Envelope(0, Stage::Attack), 0.005));
    assert!(engine.write(Address::ModEnvelope(0, 0, Stage::Attack), 0.012));
    engine.set_override(Override {
        group: Some(0),
        param: Param::Sustain,
        offset: 0.1,
    });
    assert!(engine.write(
        Address::PitchSourceIntensity {
            group: 0,
            slot: 3,
            target: 0,
            bipolar: false
        },
        0.35
    ));
    assert!(engine.write(
        Address::PitchSourceIntensity {
            group: 0,
            slot: 6,
            target: 0,
            bipolar: false
        },
        0.3
    ));
    assert!(engine.write(
        Address::Intensity {
            group: 0,
            index: 0,
            bipolar: false,
            cubic: None
        },
        0.75
    ));
}

fn performance(change: usize, fragments: &[usize]) -> [[Vec<f32>; 2]; 2] {
    let mut baseline = group();
    if change == 20 {
        // Same raw points in both banks. Only the admitted saved loop marker
        // changes: wider loop start0/end2 becomes an ordinary sustain at2.
        baseline.native_flex_loop = true;
        baseline.flex_env = Some(FlexEnvelope {
            points: vec![
                FlexPoint {
                    time_ms: 4.,
                    level: 1.,
                    curve: 0.5,
                },
                FlexPoint {
                    time_ms: 6.,
                    level: 0.25,
                    curve: 0.5,
                },
                FlexPoint {
                    time_ms: 8.,
                    level: 0.75,
                    curve: 0.5,
                },
                FlexPoint {
                    time_ms: 80.,
                    level: 0.,
                    curve: 0.5,
                },
            ],
            sustain: 2,
            unknown_index: 0,
            unknown_tail: Vec::new(),
        });
    }
    let (mut actual, mut expected) = (
        engine_group(baseline.clone()),
        engine_group(baseline.clone()),
    );
    let mut incoming_group = baseline;
    match change {
        0 => {} // Genuine residency fill; no metadata changes.
        1 => incoming_group.pitch_lfos[0].digital.as_mut().unwrap().saw = -0.75,
        2 => {
            incoming_group.volume_lfos[0]
                .source
                .digital
                .as_mut()
                .unwrap()
                .pulse_width = 0.8
        }
        3 => incoming_group.flex_env.as_mut().unwrap().points[0].time_ms = 40.,
        4 => incoming_group.native_flex_oneshot = true,
        5 => incoming_group.native_volume_env = true,
        6 => incoming_group.volume_env.as_mut().unwrap().attack_ms = 40.,
        7 => incoming_group.envelopes[0].env.attack_ms = 60.,
        8 => incoming_group.modulators[0].bypassed = true,
        9 => incoming_group.mods[0].intensity = 0.5,
        10 => incoming_group.mods[0].lag_ms = 33,
        11 => incoming_group.mods[0].cc_default = Some(0),
        12 => incoming_group.envelopes[0].native_ahd_only = true,
        13 => incoming_group.envelopes[0].targets[1].intensity = 0.6,
        14 => {} // Only the incoming effective seed changes, below.
        15 => incoming_group.pitch_glides[0].time_ms = 200.,
        16 => incoming_group.pitch_glides[0].mode = GlideMode::Speed,
        17 => {
            incoming_group.pitch_glides[0].depth = 0.25;
            incoming_group.pitch_glides[0].targets[0].1 = 0.25;
        }
        18 => incoming_group.pitch_glides[0].targets[0].0 = 4,
        19 => incoming_group.pitch_glides[0].slot = 7,
        20 => incoming_group.native_flex_loop = false,
        _ => unreachable!(),
    }
    // Two independently prepared seeds: the reference explicitly installs the
    // requested source configuration, without using the compatibility guard.
    let mut incoming_seed = bank(incoming_group.clone());
    if change == 14 {
        incoming_seed = Box::new(
            (*incoming_seed)
                .with_prepared_settings(|rows| {
                    rows[0].pitch_lfos[0].digital.as_mut().unwrap().saw = -0.75;
                })
                .unwrap(),
        );
        assert_eq!(incoming_seed.groups()[0].pitch_lfos, group().pitch_lfos);
        // Independent source configuration for explicit installation: the same
        // audible shape is authored here rather than prepared through the API.
        incoming_group.pitch_lfos[0].digital.as_mut().unwrap().saw = -0.75;
    }
    if change == 20 {
        assert_eq!(
            incoming_seed.groups()[0].flex_env,
            actual.bank().unwrap().groups()[0].flex_env,
            "marker-only replacement keeps all saved raw points and times"
        );
        assert_eq!(
            actual.state.settings[0].flex.as_ref().unwrap().loop_start,
            Some(0)
        );
        assert!(!incoming_seed.groups()[0].native_flex_loop);
    }
    assert_eq!(incoming_seed.prepared_settings_edited, change == 14);
    let mut incoming = Some(incoming_seed);
    let mut explicit = (change != 0).then(|| bank(incoming_group));
    let (mut retired, mut expected_retired) = (None, None);
    let old = HostNote {
        port: 0,
        channel: 4,
        key: 60,
        id: 91,
        clap: true,
    };
    let new = HostNote { id: 92, ..old };
    let mut pcm: [[Vec<f32>; 2]; 2] =
        std::array::from_fn(|_| std::array::from_fn(|_| vec![0.; END]));
    let mut ends = [[None; 2]; 2];
    let (base, effective) = (actual.state.base.as_ptr(), actual.state.settings.as_ptr());
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for e in [&mut actual, &mut expected] {
                authored(e);
                // A genuine raw admission without a zone seeds the old Glide with
                // interval+12; otherwise a restarted zero clock could pass a fill.
                let seed = HostNote {
                    key: 48,
                    id: 90,
                    ..old
                };
                assert!(e.host_note_on(seed, 7, 48, 127, 0.));
                assert!(e.host_note_on(old, 4, 60, 127, 0.));
            }
            let (mut at, mut fragment) = (0, 0);
            for stop in [211, 379, 571, SWITCH, 617, 2501, END] {
                while at < stop {
                    let n = fragments[fragment % fragments.len()].min(stop - at);
                    for (e, output) in [&mut actual, &mut expected].into_iter().zip(&mut pcm) {
                        let [left, right] = output;
                        e.render_checked(&mut left[at..at + n], &mut right[at..at + n])
                            .unwrap();
                    }
                    at += n;
                    fragment += 1;
                }
                match stop {
                    211 => {
                        for e in [&mut actual, &mut expected] {
                            e.cc_from(4, 4, 1, 64);
                        }
                    }
                    379 => {
                        for e in [&mut actual, &mut expected] {
                            assert!(e.write(Address::PitchSourceBypass(0, 3), 1.));
                            assert!(e.write(Address::PitchSourceBypass(0, 6), 1.));
                            assert!(e.write(Address::InternalBypass(0, 0), 1.));
                        }
                    }
                    571 => {
                        for e in [&mut actual, &mut expected] {
                            assert!(e.write(Address::PitchSourceBypass(0, 3), 0.));
                            assert!(e.write(Address::PitchSourceBypass(0, 6), 0.));
                            assert!(e.write(Address::InternalBypass(0, 0), 0.));
                        }
                    }
                    SWITCH => {
                        let receipts = actual.player.midi.cc_received;
                        let now = actual.player.now;
                        assert_eq!(actual.active_voices(), 1);
                        assert!(
                            actual.bank().unwrap().prepared_settings_edited,
                            "the old custom seed must not make a genuine fill ineligible"
                        );
                        retired = actual.upgrade_bank(incoming.take().unwrap());
                        if let Some(seed) = explicit.take() {
                            expected_retired = expected.set_bank(Some(seed));
                            assert_eq!(
                                actual.active_voices(),
                                0,
                                "change{change}: source replacement must close the old voice"
                            );
                            assert_eq!(actual.player.midi.cc_received, [[0; 2]; 16]);
                            assert_ne!(actual.state.base.as_ptr(), base);
                            assert_ne!(actual.state.settings.as_ptr(), effective);
                        } else {
                            assert_eq!(actual.active_voices(), 1);
                            assert_eq!(actual.player.midi.cc_received, receipts);
                            assert_ne!(receipts[4], [0; 2]);
                            assert_eq!(
                                (actual.state.base.as_ptr(), actual.state.settings.as_ptr()),
                                (base, effective)
                            );
                            assert_eq!(actual.state.base[0].gain, 0.625);
                            assert_eq!(actual.state.base[0].pitch_lfos[0].depth, 0.35);
                            assert_eq!(actual.state.base[0].pitch_glides[0].depth, 0.3);
                            assert_eq!(actual.state.settings[0].pitch_glides[0].depth, 0.3);
                            assert!(!actual.state.settings[0].pitch_glides[0].bypassed);
                            assert_eq!(actual.state.settings[0].pitch_lfos[0].depth, 0.35);
                            assert_eq!(
                                actual.state.settings[0].pitch_lfos[0]
                                    .digital
                                    .as_ref()
                                    .unwrap()
                                    .saw,
                                -0.5
                            );
                            assert_ne!(
                                actual.state.base[0].envelope.unwrap().sustain,
                                actual.state.settings[0].envelope.unwrap().sustain
                            );
                            assert_eq!(actual.state.base[0].envelope.unwrap().attack, 0.005);
                            assert_eq!(actual.state.settings[0].envelope.unwrap().attack, 0.005);
                            assert_eq!(
                                actual.state.settings[0].pitch_envelopes[0].env.attack,
                                0.012
                            );
                            assert!(!actual.state.settings[0].pitch_envelopes[0].bypass);
                            let filter = actual.state.settings[0].filter.as_ref().unwrap();
                            assert_eq!(filter.envelope_at(0).unwrap().attack, 0.012);
                            assert_eq!(filter.envelope_bypass_at(0), Some(false));
                        }
                        assert_eq!(
                            actual.player.now, now,
                            "handoff does not advance the voice clock"
                        );
                    }
                    617 => {
                        for e in [&mut actual, &mut expected] {
                            assert!(e.host_note_on(new, 4, 60, 127, 0.));
                        }
                    }
                    2501 => {
                        for e in [&mut actual, &mut expected] {
                            e.host_note_off(pattern(old));
                            e.host_note_off(pattern(new));
                        }
                    }
                    _ => {}
                }
            }
            for (e, result) in [&mut actual, &mut expected].into_iter().zip(&mut ends) {
                e.mark_host_notes();
                *result = [e.host_note_end_at(old), e.host_note_end_at(new)];
                assert_eq!(e.active_voices(), 0);
                assert_eq!(e.underruns(), 0);
            }
        }),
        0
    );
    assert_eq!(
        ends[0], ends[1],
        "change{change}: complete owner lifetimes must match explicit configuration"
    );
    // The ordinary 10 ms/-60 dB coefficient crosses the relative -80 dB
    // floor after 640 frames, so release at 2501 ends at 3141. Native
    // change5 instead owns 15 control points plus interpolation: the new
    // note's age1884 has offset28, giving 2501 + 4 + 16*32 = 3017.
    let expected_ends = [
        Some(if change == 0 { 3141 } else { SWITCH as u64 }),
        Some(if change == 5 { 3017 } else { 3141 }),
    ];
    assert_eq!(
        ends[0], expected_ends,
        "change{change}: authored sources own absolute terminal samples"
    );
    let error = pcm[0]
        .iter()
        .flatten()
        .zip(pcm[1].iter().flatten())
        .fold(0_f32, |error, (a, b)| error.max((a - b).abs()));
    assert!(
        error < 2e-5,
        "change{change}: complete final Engine PCM differs from explicit configuration by {error}"
    );
    assert!(pcm[0][0][211..SWITCH].iter().any(|x| x.abs() > 0.01));
    if change == 11 {
        assert!(
            pcm[0]
                .iter()
                .all(|channel| channel[617..].iter().all(|x| *x == 0.)),
            "the new zero default must mute volume until accepted input arrives"
        );
    } else {
        assert!(pcm[0][0][617..2501].iter().any(|x| x.abs() > 0.01));
    }
    assert!(
        pcm[0]
            .iter()
            .all(|channel| channel[END - 128..].iter().all(|x| *x == 0.))
    );
    drop(retired);
    drop(expected_retired); // Off-callback retirement.
    pcm
}

#[test]
fn source_metadata_changes_replace_dsp_while_residency_fills_keep_authored_state_and_complete_pcm_without_heap()
 {
    let no_op = (*bank(group())).with_prepared_settings(|_| {}).unwrap();
    assert!(
        no_op.prepared_settings_edited,
        "explicit no-op edits also request installation"
    );
    for change in 0..=20 {
        let fixed = performance(change, &[128]);
        let fragmented = performance(change, &[1, 7, 31, 127]);
        let error = fixed
            .iter()
            .flatten()
            .flatten()
            .zip(fragmented.iter().flatten().flatten())
            .fold(0_f32, |error, (a, b)| error.max((a - b).abs()));
        assert!(
            error < 2e-5,
            "change{change}: host fragments changed final Engine PCM by {error}"
        );
    }
}

#[path = "instrument_sample_upgrade_tests.rs"]
mod sample_geometry;
