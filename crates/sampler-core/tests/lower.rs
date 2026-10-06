use sampler_core::lower::{Feature, LowerError, lower};
use sampler_core::{Input, Limits, Pcm, Protocol, Runtime};
use sampler_ir as ir;

fn input(key: u8) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: None,
    }
}

fn limits() -> Limits {
    Limits {
        notes: 16,
        channels: 1,
        performances: 1,
        families: 16,
        expressions: 16,
        voices: 32,
        decisions: 32,
        commands: 16,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    }
}

/// Constant stereo frames, so each asset is recognizable in the output.
fn constant(value: f32) -> Pcm {
    Pcm::new(48000, vec![[value; 2]; 4800].into_boxed_slice()).unwrap()
}

fn asset(name: &str) -> ir::Asset {
    ir::Asset {
        location: ir::AssetLocation::Path(name.into()),
        encoding: ir::Encoding::Wav,
        root_key: None,
        loops: Vec::new(),
    }
}

fn no_behaviors(
    _: &[ir::Behavior],
    _: sampler_core::Prepared,
) -> Result<sampler_core::Prepared, LowerError> {
    unreachable!("no behaviors declared")
}

fn rejected(ir: &ir::Instrument, pcm: Vec<Pcm>) -> LowerError {
    lower(ir, 48000, pcm, no_behaviors)
        .err()
        .expect("lowering should fail")
}

fn play(rt: &mut Runtime, key: u8, velocity: f64) -> [f32; 2] {
    let mut out = [[0.0; 2]; 64];
    rt.trigger(input(key), key, velocity).unwrap();
    rt.render(&mut out).unwrap();
    rt.note_off(input(key), None).unwrap();
    out[32]
}

/// Two velocity layers, a two-take round robin on the soft layer, a release
/// zone and a panned, filtered loud layer.
fn instrument() -> ir::Instrument {
    let zone = |asset, low, high| ir::Zone {
        keys: ir::KeyRange { low: 60, high: 60 },
        velocities: ir::VelocityRange { low, high },
        pitch: ir::KeyTracking::Fixed,
        velocity: ir::VelocityResponse::None,
        ..ir::Zone::new(ir::AssetRef(asset))
    };
    let take = |index| {
        Some(ir::Selection {
            sequence: ir::SequenceRef(0),
            take: ir::Take::Index(index),
        })
    };
    ir::Instrument {
        name: "lowering".into(),
        assets: ["a", "b", "c", "d"].map(asset).to_vec(),
        sequences: vec![ir::Sequence {
            policy: ir::SequencePolicy::RoundRobin,
            takes: 2,
            counter: ir::CounterScope::Key,
        }],
        zones: vec![
            ir::Zone {
                selection: take(0),
                ..zone(0, 1, 63)
            },
            ir::Zone {
                selection: take(1),
                ..zone(1, 1, 63)
            },
            ir::Zone {
                pan: ir::Pan {
                    position: 1.0,
                    law: ir::PanLaw::Balance,
                },
                gain: ir::Gain::Decibels(6.0),
                ..zone(2, 64, 127)
            },
            ir::Zone {
                trigger: ir::Trigger::GateRelease,
                ..zone(3, 1, 127)
            },
        ],
        ..ir::Instrument::default()
    }
}

#[test]
fn layers_round_robin_pan_gain_and_release_zones_render() {
    let pcm = vec![constant(0.1), constant(0.2), constant(0.3), constant(0.05)];
    let plan = lower(&instrument(), 48000, pcm, no_behaviors).unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    // Soft notes alternate between the two takes.
    assert_eq!(play(&mut rt, 60, 0.3), [0.1; 2]);
    let mut tail = [[0.0; 2]; 64];
    rt.render(&mut tail).unwrap();
    assert_eq!(
        tail[32], [0.05; 2],
        "the release zone sounds after note-off"
    );
    assert_eq!(play(&mut rt, 60, 0.3)[0], 0.2 + 0.05);
    rt.render(&mut [[0.0; 2]; 4800]).unwrap();
    // +6 dB boosts past unity through the voice chain; pan right silences left.
    let loud = play(&mut rt, 60, 1.0);
    assert_eq!(loud[0], 0.0, "pan right silences the left channel");
    assert!((loud[1] - 0.3 * ir::Gain::Decibels(6.0).linear() as f32).abs() < 1e-4);
}

#[test]
fn amplitude_envelope_converts_source_time_to_frames() {
    let mut ir = instrument();
    ir.modulators.push(ir::Modulator {
        scope: ir::Scope::Voice,
        source: ir::ModulationSource::Envelope(ir::Envelope {
            attack: ir::Time::Milliseconds(1.0),
            ..ir::Envelope::default()
        }),
    });
    ir.zones[0].amplitude = Some(ir::ModulatorRef(0));
    let pcm = vec![constant(0.1), constant(0.2), constant(0.3), constant(0.05)];
    let mut rt = Runtime::new(lower(&ir, 48000, pcm, no_behaviors).unwrap(), limits()).unwrap();
    let mut out = [[0.0; 2]; 96];
    rt.trigger(input(60), 60, 0.3).unwrap();
    rt.render(&mut out).unwrap();
    // 1 ms at 48 kHz is 48 frames of linear attack.
    assert!((out[24][0] - 0.05).abs() < 0.003, "{}", out[24][0]);
    assert_eq!(out[60], [0.1; 2]);
}

#[test]
fn unexecutable_meaning_is_rejected_with_its_owner() {
    let pcm = || vec![constant(0.1), constant(0.2), constant(0.3), constant(0.05)];
    let mut ir = instrument();
    ir.zones[1].trigger = ir::Trigger::Legato;
    assert_eq!(
        rejected(&ir, pcm()),
        LowerError::Unsupported {
            owner: "zone 1".into(),
            feature: Feature::Trigger(ir::Trigger::Legato)
        }
    );
    let mut ir = instrument();
    ir.zones[2].selection = Some(ir::Selection {
        sequence: ir::SequenceRef(0),
        take: ir::Take::Probability {
            low: 0.0,
            high: 0.3,
        },
    });
    assert!(matches!(
        lower(&ir, 48000, pcm(), no_behaviors),
        Err(LowerError::Unsupported {
            feature: Feature::MixedTakeKinds,
            ..
        })
    ));
    let mut ir = instrument();
    ir.zones[0].asset = ir::AssetRef(9);
    let error = rejected(&ir, pcm());
    assert_eq!(
        error.to_string(),
        "invalid instrument: zone 0 refers to missing Asset(9)"
    );
    assert!(matches!(
        lower(&instrument(), 48000, pcm()[..2].to_vec(), no_behaviors),
        Err(LowerError::AssetCount {
            assets: 4,
            supplied: 2
        })
    ));
}

#[test]
fn even_probability_takes_lower_to_a_random_sequence() {
    let mut ir = instrument();
    for (zone, (low, high)) in ir.zones[..2].iter_mut().zip([(0.0, 0.5), (0.5, 1.0)]) {
        zone.selection = Some(ir::Selection {
            sequence: ir::SequenceRef(0),
            take: ir::Take::Probability { low, high },
        });
    }
    let pcm = || vec![constant(0.1), constant(0.2), constant(0.3), constant(0.05)];
    let mut rt = Runtime::new(lower(&ir, 48000, pcm(), no_behaviors).unwrap(), limits()).unwrap();
    let mut seen = [false; 2];
    for _ in 0..16 {
        let out = play(&mut rt, 60, 0.3)[0];
        seen[0] |= (out - 0.1).abs() < 1e-6 || (out - 0.15).abs() < 1e-6;
        seen[1] |= (out - 0.2).abs() < 1e-6 || (out - 0.25).abs() < 1e-6;
        rt.render(&mut [[0.0; 2]; 4800]).unwrap();
        rt.flush_ended(|_| true);
    }
    assert_eq!(seen, [true; 2], "both takes are drawn");
    ir.zones[1].selection = Some(ir::Selection {
        sequence: ir::SequenceRef(0),
        take: ir::Take::Probability {
            low: 0.5,
            high: 0.9,
        },
    });
    assert!(matches!(
        lower(&ir, 48000, pcm(), no_behaviors),
        Err(LowerError::Unsupported {
            feature: Feature::UnevenProbabilities,
            ..
        })
    ));
}

#[test]
fn zone_routes_lower_to_voice_modulation() {
    let mut ir = ir::Instrument {
        assets: vec![asset("a")],
        zones: vec![ir::Zone {
            keys: ir::KeyRange { low: 60, high: 60 },
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            routes: vec![ir::RouteRef(0), ir::RouteRef(1)],
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        ..Default::default()
    };
    ir.modulators = vec![
        ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Velocity,
        },
        ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::PitchBend,
        },
    ];
    ir.routes = vec![
        ir::Route::new(
            ir::ModulatorRef(0),
            ir::Target::Amplitude,
            ir::Depth::Normalized(1.0),
        ),
        // Native expression bend owns this one.
        ir::Route::new(
            ir::ModulatorRef(1),
            ir::Target::Pitch,
            ir::Depth::Pitch(ir::Pitch::Cents(1200.0)),
        ),
    ];
    let plan = lower(&ir, 48000, vec![constant(0.5)], no_behaviors).unwrap();
    // The authored bend depth is the plain-MIDI default range.
    assert_eq!(plan.bend_range(), 12.0);
    let mut rt = Runtime::new(plan, limits()).unwrap();
    assert_eq!(rt.bend_range(), 12.0);
    let out = play(&mut rt, 60, 0.5);
    assert!((out[0] - 0.25).abs() < 1e-6, "{out:?}");

    ir.routes[1].target = ir::Target::Pan;
    ir.routes[1].depth = ir::Depth::Normalized(1.0);
    assert!(matches!(
        rejected(&ir, vec![constant(0.5)]),
        LowerError::Unsupported {
            feature: Feature::PitchBendSource,
            ..
        }
    ));
}

#[test]
fn native_mpe_defaults_are_identity_at_rest_and_follow_pressure_and_timbre() {
    use sampler_core::{
        Expression,
        lower::{Options, lower_with},
    };
    let ir = ir::Instrument {
        assets: vec![asset("a")],
        zones: vec![ir::Zone {
            keys: ir::KeyRange { low: 60, high: 60 },
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        ..Default::default()
    };
    // Alternating samples: all energy at Nyquist, which the tone filter removes.
    let nyquist = || {
        let frames = (0..4800).map(|i| [if i % 2 == 0 { 0.25 } else { -0.25 }; 2]);
        Pcm::new(48000, frames.collect()).unwrap()
    };
    let play = |options: &Options, expression: Expression| {
        let plan = lower_with(&ir, 48000, vec![nyquist()], options, no_behaviors).unwrap();
        let mut rt = Runtime::new(plan, limits()).unwrap();
        rt.trigger_with_expression(input(60), 60, 1.0, expression)
            .unwrap();
        let mut out = [[0.0; 2]; 512];
        rt.render(&mut out).unwrap();
        out[256..].iter().map(|f| f[0].abs()).fold(0f32, f32::max)
    };
    let on = Options::default();
    let off = Options { mpe: None };
    let rest = Expression::default();
    assert_eq!(play(&on, rest), 0.25);
    assert_eq!(play(&off, rest), 0.25);
    let pressed = Expression {
        pressure: u32::MAX,
        ..rest
    };
    assert!((play(&on, pressed) - 0.25 * 10f32.powf(6.0 / 20.0)).abs() < 1e-5);
    assert_eq!(play(&off, pressed), 0.25);
    let dark = Expression { timbre: 0, ..rest };
    assert!(play(&on, dark) < 0.001, "{}", play(&on, dark));
    assert_eq!(play(&off, dark), 0.25);
}

#[test]
fn group_voice_limits_fade_out_the_oldest_member_instead_of_rejecting() {
    let limit = |voices, kill| ir::VoiceLimit {
        voices,
        kill,
        prefer_released: true,
        fade: ir::Time::Milliseconds(0.0),
    };
    let zone = |group| ir::Zone {
        keys: ir::KeyRange { low: 0, high: 127 },
        pitch: ir::KeyTracking::Fixed,
        velocity: ir::VelocityResponse::None,
        group: Some(ir::GroupRef(group)),
        ..ir::Zone::new(ir::AssetRef(group))
    };
    let ir = ir::Instrument {
        assets: ["a", "b"].map(asset).to_vec(),
        groups: vec![
            ir::Group {
                voice_limit: Some(0),
                ..Default::default()
            },
            ir::Group::default(),
        ],
        voice_limit: Some(limit(5, ir::Kill::Oldest)),
        voice_limits: vec![limit(2, ir::Kill::Highest)],
        zones: vec![zone(0), zone(1)],
        ..ir::Instrument::default()
    };
    let plan = lower(&ir, 48000, vec![constant(0.1), constant(0.2)], no_behaviors).unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    let mut out = [[0.0; 2]; 64];
    // Each note starts one voice per group (0.1 in group 0, 0.2 in group 1).
    let mut note = |rt: &mut Runtime, key| {
        rt.trigger(input(key), key, 1.).unwrap();
        rt.render(&mut out).unwrap();
        (rt.voice_count(), out[63][0])
    };
    note(&mut rt, 60);
    note(&mut rt, 72);
    // Group 0 holds two: 72's group-0 voice (the highest) goes.
    let (voices, level) = note(&mut rt, 48);
    assert_eq!(voices, 5);
    assert!((level - 0.8).abs() < 1e-6, "{level}");
    // The instrument holds five: both voices of 60 (the oldest) go.
    let (voices, level) = note(&mut rt, 36);
    assert_eq!(voices, 5);
    assert!((level - 0.8).abs() < 1e-6, "{level}");
    // Group 0 is now 48 and 36; 84 replaces 48 there.
    let (voices, _) = note(&mut rt, 84);
    assert_eq!(voices, 5);
}
