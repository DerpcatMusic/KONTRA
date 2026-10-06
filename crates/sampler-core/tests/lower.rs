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
            ir::Depth::Pitch(ir::Pitch::Cents(200.0)),
        ),
    ];
    let plan = lower(&ir, 48000, vec![constant(0.5)], no_behaviors).unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
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
