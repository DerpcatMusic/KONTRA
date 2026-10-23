mod support;
use sampler_core::lower::{Feature, LowerError, lower};
use sampler_core::{Input, Limits, Pcm, Protocol, Runtime};
use sampler_ir as ir;

#[test]
fn readonly_physical_catalog_exceeds_routing_count_but_keeps_its_own_byte_budget() {
    let count = sampler_core::lower::ZONE_ROUTING_LIMITS.zones + 1;
    assert!(count < sampler_core::lower::SOURCE_ZONE_CATALOG_ZONES);
    let home = ir::SourceZoneHome {
        group: 17,
        low_key: 0,
        high_key: 127,
    };
    let mut instrument = ir::Instrument::default();
    instrument.source_indices.zones = vec![None; count];
    instrument.source_indices.zone_catalog = (0..count)
        .map(|n| ir::SourceZone {
            id: (n != 1).then_some(n as u32 * 2),
            home,
        })
        .collect();
    let plan = lower(&instrument, 48000, vec![], no_behaviors).unwrap();
    assert_eq!(plan.source_zone_identity_count(), count);
    assert_eq!(plan.source_zone_identity(0), Some(0));
    assert_eq!(plan.source_zone_identity(1), None);
    assert_eq!(
        plan.source_zone_identity(count - 1),
        Some((count as u32 - 1) * 2)
    );
    assert_eq!(plan.source_zone_identity(count), None);
    let limits = Limits::for_plan(&plan, 1, 1);
    let runtime = Runtime::new(plan, limits).unwrap();
    support::without_heap(|| {
        assert_eq!(
            runtime.zone_parameter_in(
                runtime.active_plan(),
                (count as u32 - 1) * 2,
                sampler_core::ZoneParameter::Group
            ),
            Ok(17)
        );
        assert!(
            runtime
                .zone_routing_report(runtime.active_plan())
                .unwrap()
                .is_none()
        );
    });
    let over_budget = sampler_core::lower::SOURCE_ZONE_CATALOG_ZONES + 1;
    instrument.source_indices.zones.resize(over_budget, None);
    instrument
        .source_indices
        .zone_catalog
        .resize(over_budget, ir::SourceZone { id: None, home });
    assert!(matches!(
        lower(&instrument, 48000, vec![], no_behaviors),
        Err(LowerError::Core {
            stage: sampler_core::lower::Stage::ZoneRouting,
            error: sampler_core::Error::Capacity,
            ..
        })
    ));
}

#[test]
fn authored_inactive_envelope_values_remain_readable_without_a_dsp_write_owner() {
    use sampler_core::{EngineParameterAddress, Error, engine_parameter_id};
    let address = EngineParameterAddress {
        parameter: engine_parameter_id("ENGINE_PAR_ATTACK").unwrap(),
        group: 7,
        slot: 9,
        generic: -1,
    };
    let mut instrument = ir::Instrument::default();
    instrument
        .source_indices
        .engine_values
        .push(ir::SourceEngineValue {
            parameter: address.parameter,
            group: address.group,
            slot: address.slot,
            generic: address.generic,
            value: 234567,
        });
    let plan = lower(&instrument, 48000, vec![], no_behaviors).unwrap();
    assert!(plan.engine_parameter_bindings().is_empty());
    let limits = Limits::for_plan(&plan, 4, 0);
    let mut runtime = Runtime::new(plan, limits).unwrap();
    support::without_heap(|| {
        assert_eq!(
            runtime.engine_parameter(address),
            Ok(234567),
            "authored source value was dropped with its inactive DSP owner"
        );
        assert_eq!(
            runtime.set_engine_parameter(address, 765432),
            Err(Error::InvalidInput)
        );
        assert_eq!(runtime.engine_parameter(address), Ok(234567));
        assert_eq!(
            runtime.engine_parameter(EngineParameterAddress { slot: 8, ..address }),
            Err(Error::InvalidInput)
        );
    });
}

#[test]
fn live_envelope_owner_takes_precedence_over_authored_source_defaults() {
    use sampler_core::{EngineParameterAddress, Envelope, engine_parameter_id};
    let address = EngineParameterAddress {
        parameter: engine_parameter_id("ENGINE_PAR_ATTACK").unwrap(),
        group: 7,
        slot: 9,
        generic: -1,
    };
    let mut instrument = ir::Instrument::default();
    instrument.groups.push(ir::Group::default());
    instrument
        .source_indices
        .engine_values
        .push(ir::SourceEngineValue {
            parameter: address.parameter,
            group: address.group,
            slot: address.slot,
            generic: address.generic,
            value: 234567,
        });
    let plan = lower(&instrument, 48000, vec![], no_behaviors)
        .unwrap()
        .with_group_envelope_parameters(0, 7, 9, Envelope::default())
        .unwrap();
    let limits = Limits::for_plan(&plan, 4, 0);
    let mut runtime = Runtime::new(plan, limits).unwrap();
    support::without_heap(|| {
        runtime.set_engine_parameter(address, 765432).unwrap();
        assert_eq!(runtime.engine_parameter(address), Ok(765432));
    });
}

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
        residency: Default::default(),
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

/// An AHD (one-shot) envelope on a looped release zone lowers, ignores the
/// gate and ends its voice after the decay; the loop never sounds past it.
#[test]
fn one_shot_envelope_bounds_a_looped_release_zone() {
    let mut ir = instrument();
    ir.zones.truncate(1);
    ir.zones[0].selection = None;
    ir.sequences.clear();
    let mut release = ir.zones[0].clone();
    release.asset = Some(ir::AssetRef(3));
    release.trigger = ir::Trigger::KeyRelease;
    release.playback.looping = ir::Looping::Continuous(ir::LoopRange {
        start: 0,
        end: 480,
        crossfade: ir::Span::Frames(0),
        alternating: false,
    });
    ir.modulators.push(ir::Modulator {
        scope: ir::Scope::Voice,
        source: ir::ModulationSource::Envelope(ir::Envelope {
            hold: ir::Time::Milliseconds(1.0),
            decay: ir::Time::Milliseconds(1.0),
            sustain: 1.0,
            release: ir::Time::Seconds(10.0),
            one_shot: true,
            ..ir::Envelope::default()
        }),
    });
    release.amplitude = Some(ir::ModulatorRef(0));
    ir.zones.push(release);
    let pcm = vec![constant(0.1), constant(0.2), constant(0.3), constant(0.05)];
    let mut rt = Runtime::new(lower(&ir, 48000, pcm, no_behaviors).unwrap(), limits()).unwrap();
    assert_eq!(play(&mut rt, 60, 0.3), [0.1; 2]);
    let mut tail = [[0.0; 2]; 192];
    rt.render(&mut tail).unwrap();
    assert_eq!(tail[24], [0.05; 2], "hold at full level");
    assert_eq!(tail[120], [0.0; 2], "silent after the decay");
    assert_eq!(rt.voice_count(), 0);
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
    ir.zones[0].asset = Some(ir::AssetRef(9));
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
fn saved_digital_sine_lowers_with_its_bipolar_level_without_callback_heap() {
    for (phase, expected) in [(0., 0.25), (0.25, 0.175), (0.5, 0.25), (0.75, 0.325)] {
        let mut instrument = ir::Instrument {
            assets: vec![asset("a")],
            zones: vec![ir::Zone {
                keys: ir::KeyRange { low: 60, high: 60 },
                pitch: ir::KeyTracking::Fixed,
                velocity: ir::VelocityResponse::None,
                routes: vec![ir::RouteRef(0)],
                ..ir::Zone::new(ir::AssetRef(0))
            }],
            modulators: vec![ir::Modulator {
                scope: ir::Scope::Voice,
                source: ir::ModulationSource::Lfo(ir::Lfo {
                    shape: ir::LfoShape::SineScaled(-f64::from(0.3f32)),
                    rate: ir::Frequency::Hertz(1e-10),
                    delay: ir::Time::ZERO,
                    fade_in: ir::Time::ZERO,
                    phase,
                    retrigger: true,
                }),
            }],
            routes: vec![ir::Route::new(
                ir::ModulatorRef(0),
                ir::Target::Amplitude,
                ir::Depth::Normalized(1.),
            )],
            ..Default::default()
        };
        for block in [1, 7, 32, 61, 256] {
            let plan = lower(&instrument, 48000, vec![constant(0.5)], no_behaviors).unwrap();
            let mut rt = Runtime::new(plan, limits()).unwrap();
            for _ in 0..2 {
                let mut audio = [[0.; 2]; 384];
                support::without_heap(|| {
                    rt.trigger(input(60), 60, 1.).unwrap();
                    for chunk in audio.chunks_mut(block) {
                        rt.render(chunk).unwrap();
                    }
                    rt.note_off(input(60), None).unwrap();
                    rt.render(&mut [[0.; 2]; 128]).unwrap();
                });
                assert_eq!(rt.voice_count(), 0);
                assert!(
                    audio
                        .iter()
                        .flatten()
                        .all(|v| (f64::from(*v) - expected).abs() < 1e-6)
                );
            }
        }
        if let ir::ModulationSource::Lfo(lfo) = &mut instrument.modulators[0].source {
            lfo.shape = ir::LfoShape::SineScaled(f64::NAN);
        }
        assert!(instrument.validate().is_err());
        assert!(lower(&instrument, 48000, vec![constant(0.5)], no_behaviors).is_err());
    }
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
    // Bend anywhere else is a bipolar voice modulation source.
    assert!(lower(&ir, 48000, vec![constant(0.5)], no_behaviors).is_ok());
}

/// Kontakt's own plain-MIDI bend range is not recovered (audit rows
/// CORE-SOUND-005 NOT-PROBED, CORE-SOUND-006 BLOCKED-RUNTIME), so without a
/// Pitch Bend route to Pitch the plan keeps the MIDI default of 2 semitones.
#[test]
fn plain_midi_bend_range_is_two_semitones_without_a_bend_route() {
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
    let plan = lower(&ir, 48000, vec![constant(0.5)], no_behaviors).unwrap();
    assert_eq!(plan.bend_range(), 2.0);
    let rt = Runtime::new(plan, limits()).unwrap();
    assert_eq!(rt.bend_range(), 2.0);
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
        exclusive_set: None,
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

/// A 4-pole filter lowers as two cascaded 2-pole sections (6-pole: three): each
/// attenuates a tone well above the cutoff more than the one before.
#[test]
fn four_pole_filters_lower_to_two_cascaded_sections() {
    let tone = |poles| {
        let mut ir = instrument();
        ir.chains.push(ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Filter(ir::Filter {
                kind: ir::FilterKind::LowPass { poles },
                cutoff: ir::Frequency::Hertz(500.0),
                resonance: ir::Resonance::Decibels(0.0),
            })],
            post_amplitude: vec![],
        });
        ir.zones[0].chain = Some(ir::ChainRef(0));
        // 6 kHz square-ish tone: alternating pairs of frames.
        let wave = (0..4800)
            .map(|i| [if (i / 4) % 2 == 0 { 0.5 } else { -0.5 }; 2])
            .collect::<Vec<_>>();
        let pcm = vec![
            Pcm::new(48000, wave.into_boxed_slice()).unwrap(),
            constant(0.2),
            constant(0.3),
            constant(0.05),
        ];
        let plan = lower(&ir, 48000, pcm, no_behaviors).unwrap();
        let mut rt = Runtime::new(plan, limits()).unwrap();
        let mut out = [[0.0f32; 2]; 512];
        rt.trigger(input(60), 60, 0.3).unwrap();
        rt.render(&mut out).unwrap();
        out[256..].iter().map(|f| f[0] * f[0]).sum::<f32>()
    };
    let (two, four, six) = (tone(2), tone(4), tone(6));
    assert!(two.is_finite() && four.is_finite() && six.is_finite());
    assert!(four < two * 0.5, "two {two}, four {four}");
    // Three sections: SV LP6 is six cascaded one-poles at zero resonance.
    assert!(six < four * 0.5, "four {four}, six {six}");
}

#[test]
fn one_pole_filters_follow_the_6_db_per_octave_law() {
    // 3 kHz sine through a 500 Hz one-pole: |H| = 1 / sqrt(1 + (tan(pi f / fs) / tan(pi fc / fs))^2).
    let energy = |kind: Option<ir::FilterKind>| {
        let mut ir = instrument();
        if let Some(kind) = kind {
            ir.chains.push(ir::Chain {
                scope: ir::Scope::Voice,
                pre_amplitude: vec![ir::Processor::Filter(ir::Filter {
                    kind,
                    cutoff: ir::Frequency::Hertz(500.0),
                    resonance: ir::Resonance::Decibels(0.0),
                })],
                post_amplitude: vec![],
            });
            ir.zones[0].chain = Some(ir::ChainRef(0));
        }
        let wave = (0..4800)
            .map(|i| [(std::f32::consts::TAU * 3000.0 * i as f32 / 48000.0).sin() * 0.5; 2])
            .collect::<Vec<_>>();
        let pcm = vec![
            Pcm::new(48000, wave.into_boxed_slice()).unwrap(),
            constant(0.2),
            constant(0.3),
            constant(0.05),
        ];
        let plan = lower(&ir, 48000, pcm, no_behaviors).unwrap();
        let mut rt = Runtime::new(plan, limits()).unwrap();
        let mut out = [[0.0f32; 2]; 1024];
        rt.trigger(input(60), 60, 0.3).unwrap();
        rt.render(&mut out).unwrap();
        out[512..].iter().map(|f| f[0] * f[0]).sum::<f32>()
    };
    let plain = energy(None);
    // y += (x - y) b, b = 1 - exp(-2 pi fc / fs): H = b / (1 - (1 - b) e^-jw).
    let b = 1.0 - (-std::f64::consts::TAU * 500.0 / 48000.0).exp();
    let w = std::f64::consts::TAU * 3000.0 / 48000.0;
    let h = num(b, 0.0) / (num(1.0, 0.0) - num(1.0 - b, 0.0) * num(w.cos(), -w.sin()));
    let low = f64::from(energy(Some(ir::FilterKind::LowPass { poles: 1 })) / plain);
    let high = f64::from(energy(Some(ir::FilterKind::HighPass { poles: 1 })) / plain);
    let (want_low, want_high) = (h.0 * h.0 + h.1 * h.1, (1.0 - h.0).powi(2) + h.1 * h.1);
    assert!(
        (low - want_low).abs() < want_low * 0.03,
        "low {low}, want {want_low}"
    );
    assert!(
        (high - want_high).abs() < want_high * 0.03,
        "high {high}, want {want_high}"
    );
}

/// Minimal complex arithmetic for the one-pole response.
#[derive(Clone, Copy)]
struct C(f64, f64);
fn num(re: f64, im: f64) -> C {
    C(re, im)
}
impl std::ops::Sub for C {
    type Output = C;
    fn sub(self, o: C) -> C {
        C(self.0 - o.0, self.1 - o.1)
    }
}
impl std::ops::Mul for C {
    type Output = C;
    fn mul(self, o: C) -> C {
        C(self.0 * o.0 - self.1 * o.1, self.0 * o.1 + self.1 * o.0)
    }
}
impl std::ops::Div for C {
    type Output = (f64, f64);
    fn div(self, o: C) -> (f64, f64) {
        let d = o.0 * o.0 + o.1 * o.1;
        (
            (self.0 * o.0 + self.1 * o.1) / d,
            (self.1 * o.0 - self.0 * o.1) / d,
        )
    }
}

#[test]
fn monophonic_release_groups_cut_the_same_notes_earlier_voices_only() {
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
                monophonic_release: true,
                ..Default::default()
            },
            ir::Group::default(),
        ],
        zones: vec![zone(0), zone(1)],
        ..ir::Instrument::default()
    };
    let plan = lower(&ir, 48000, vec![constant(0.1), constant(0.2)], no_behaviors).unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    let voices = |rt: &mut Runtime, key| {
        rt.trigger(input(key), key, 1.).unwrap();
        rt.render(&mut [[0.0; 2]; 1024]).unwrap();
        rt.voice_count()
    };
    assert_eq!(voices(&mut rt, 60), 2);
    // Repeating the note cuts its earlier group-0 voice, not group 1's.
    assert_eq!(voices(&mut rt, 60), 3);
    // Another key leaves both alone.
    assert_eq!(voices(&mut rt, 72), 5);
}

/// A recorded-legato layout: a plain attack on a first note, and one transition
/// sample per interval (a step up, a step down) when another key is held.
#[test]
fn transition_zones_follow_the_interval_from_the_held_key() {
    let zone = |asset, trigger| ir::Zone {
        keys: ir::KeyRange { low: 0, high: 127 },
        pitch: ir::KeyTracking::Fixed,
        velocity: ir::VelocityResponse::None,
        trigger,
        ..ir::Zone::new(ir::AssetRef(asset))
    };
    let ir = ir::Instrument {
        assets: ["first", "up", "down"].map(asset).to_vec(),
        zones: vec![
            zone(0, ir::Trigger::First),
            zone(1, ir::Trigger::Transition { low: 1, high: 12 }),
            zone(2, ir::Trigger::Transition { low: -12, high: -1 }),
        ],
        ..Default::default()
    };
    let plan = lower(
        &ir,
        48000,
        vec![constant(0.1), constant(0.2), constant(0.4)],
        no_behaviors,
    )
    .unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    let mut out = [[0.0; 2]; 64];
    let mut start = |rt: &mut Runtime, key| {
        rt.trigger(input(key), key, 1.0).unwrap();
        rt.render(&mut out).unwrap();
        out[32][0]
    };
    // Nothing held: the first-note sample.
    assert!((start(&mut rt, 60) - 0.1).abs() < 1e-6);
    rt.note_off(input(60), None).unwrap();
    // A fresh first note again after release (the old note is no longer held).
    assert!((start(&mut rt, 64) - 0.1).abs() < 1e-6);
    // 64 held: 67 is a step up (+3) and sounds with the held note.
    assert!((start(&mut rt, 67) - (0.1 + 0.2)).abs() < 1e-6);
    // 67 is the most recent held key: 62 is 5 below it.
    assert!((start(&mut rt, 62) - (0.1 + 0.2 + 0.4)).abs() < 1e-6);
}

/// A frame ramp, 1/8 apart, so each output frame shows where it was read.
fn ramp(frames: usize) -> Pcm {
    let frames = (0..frames).map(|i| [i as f32 / 8.; 2]).collect::<Vec<_>>();
    Pcm::new(48000, frames.into_boxed_slice()).unwrap()
}

/// BASIC_BEHAVIORS row 2: a round-robin sequence hands out its takes in order
/// on successive note-ons and wraps after the last one.
#[test]
fn basic_behavior_round_robin_takes_in_order_across_note_ons() {
    let zone = |take: usize| ir::Zone {
        keys: ir::KeyRange { low: 60, high: 60 },
        pitch: ir::KeyTracking::Fixed,
        velocity: ir::VelocityResponse::None,
        selection: Some(ir::Selection {
            sequence: ir::SequenceRef(0),
            take: ir::Take::Index(take as u32),
        }),
        ..ir::Zone::new(ir::AssetRef(take))
    };
    let ir = ir::Instrument {
        assets: ["a", "b", "c"].map(asset).to_vec(),
        sequences: vec![ir::Sequence {
            policy: ir::SequencePolicy::RoundRobin,
            takes: 3,
            counter: ir::CounterScope::Instrument,
        }],
        zones: (0..3).map(zone).collect(),
        ..ir::Instrument::default()
    };
    let pcm = vec![constant(0.1), constant(0.2), constant(0.4)];
    let plan = lower(&ir, 48000, pcm, no_behaviors).unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    let mut heard = Vec::new();
    for _ in 0..6 {
        heard.push(play(&mut rt, 60, 1.0)[0]);
        rt.render(&mut [[0.0; 2]; 4800]).unwrap();
    }
    assert_eq!(heard, [0.1, 0.2, 0.4, 0.1, 0.2, 0.4]);
}

/// BASIC_BEHAVIORS row 9: a transition zone sounds for an interval inside its
/// inclusive range (the top of +1..=+12 included) and not one just outside it.
#[test]
fn basic_behavior_transition_range_is_inclusive_and_excludes_outside_intervals() {
    let zone = |asset, trigger| ir::Zone {
        keys: ir::KeyRange { low: 0, high: 127 },
        pitch: ir::KeyTracking::Fixed,
        velocity: ir::VelocityResponse::None,
        trigger,
        ..ir::Zone::new(ir::AssetRef(asset))
    };
    let ir = ir::Instrument {
        assets: ["first", "up"].map(asset).to_vec(),
        zones: vec![
            zone(0, ir::Trigger::First),
            zone(1, ir::Trigger::Transition { low: 1, high: 12 }),
        ],
        ..ir::Instrument::default()
    };
    let build = || lower(&ir, 48000, vec![constant(0.1), constant(0.2)], no_behaviors).unwrap();
    let start = |rt: &mut Runtime, key| {
        rt.trigger(input(key), key, 1.0).unwrap();
        let mut out = [[0.0; 2]; 64];
        rt.render(&mut out).unwrap();
        out[32][0]
    };
    // 60 stays held (its 0.1 voice keeps sounding); 72 is +12, the top of the
    // range, so the transition zone adds its 0.2 voice.
    let mut rt = Runtime::new(build(), limits()).unwrap();
    assert!((start(&mut rt, 60) - 0.1).abs() < 1e-6);
    assert!((start(&mut rt, 72) - (0.1 + 0.2)).abs() < 1e-6);
    // Fresh runtime: 60 held, 73 is +13, outside the range, so only 60 sounds.
    let mut rt = Runtime::new(build(), limits()).unwrap();
    assert!((start(&mut rt, 60) - 0.1).abs() < 1e-6);
    assert!((start(&mut rt, 73) - 0.1).abs() < 1e-6);
}

/// BASIC_BEHAVIORS row 17: an alternating loop plays forward to its last frame,
/// reverses there without repeating that frame, and reverses again at its start.
#[test]
fn basic_behavior_alternating_loop_reverses_direction_at_its_end() {
    let ir = ir::Instrument {
        assets: vec![asset("ramp")],
        zones: vec![ir::Zone {
            keys: ir::KeyRange { low: 0, high: 127 },
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            playback: ir::Playback {
                looping: ir::Looping::Continuous(ir::LoopRange {
                    start: 2,
                    end: 6,
                    crossfade: ir::Span::Frames(0),
                    alternating: true,
                }),
                ..ir::Playback::default()
            },
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        ..ir::Instrument::default()
    };
    let plan = lower(&ir, 48000, vec![ramp(8)], no_behaviors).unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    rt.trigger(input(60), 60, 1.0).unwrap();
    let mut out = [[0.0; 2]; 12];
    rt.render(&mut out).unwrap();
    assert_eq!(
        out.map(|f| f[0]),
        [0, 1, 2, 3, 4, 5, 4, 3, 2, 3, 4, 5].map(|i| i as f32 / 8.)
    );
}

/// Two nested selectors (outer A/B, inner x/y): each keeps its choice while the
/// other changes, and a zone sounds only under the pair it names.
#[test]
fn nested_selectors_are_independent_axes() {
    let axis = |names: [&str; 2], keys: [u8; 2]| ir::Axis {
        name: names.join("/"),
        choices: names
            .iter()
            .zip(keys)
            .map(|(n, k)| ir::AxisChoice {
                name: (*n).into(),
                switch_keys: vec![k],
            })
            .collect(),
    };
    let zone = |asset, outer, inner| ir::Zone {
        keys: ir::KeyRange { low: 60, high: 60 },
        pitch: ir::KeyTracking::Fixed,
        velocity: ir::VelocityResponse::None,
        axes: vec![
            ir::AxisPick {
                axis: 0,
                choice: outer,
            },
            ir::AxisPick {
                axis: 1,
                choice: inner,
            },
        ],
        ..ir::Zone::new(ir::AssetRef(asset))
    };
    let ir = ir::Instrument {
        assets: ["ax", "ay", "bx", "by"].map(asset).to_vec(),
        axes: vec![axis(["A", "B"], [10, 11]), axis(["x", "y"], [20, 21])],
        zones: vec![zone(0, 0, 0), zone(1, 0, 1), zone(2, 1, 0), zone(3, 1, 1)],
        ..Default::default()
    };
    let pcm = vec![constant(0.1), constant(0.2), constant(0.4), constant(0.8)];
    let mut rt = Runtime::new(lower(&ir, 48000, pcm, no_behaviors).unwrap(), limits()).unwrap();
    let mut out = [[0.0; 2]; 64];
    let mut sound = |rt: &mut Runtime, key| {
        rt.trigger(input(key), key, 1.0).unwrap();
        rt.render(&mut out).unwrap();
        rt.note_off(input(key), None).unwrap();
        out[32][0]
    };
    assert_eq!((sound(&mut rt, 60) * 10.0).round() as i32, 1); // A.x before any switch
    sound(&mut rt, 21);
    assert_eq!((sound(&mut rt, 60) * 10.0).round() as i32, 2); // inner y under outer A
    sound(&mut rt, 11);
    assert_eq!((sound(&mut rt, 60) * 10.0).round() as i32, 8); // outer B keeps inner y
    sound(&mut rt, 20);
    assert_eq!((sound(&mut rt, 60) * 10.0).round() as i32, 4); // inner x under outer B
}

#[test]
fn native_articulation_input_and_source_keys_use_the_same_composed_predicates() {
    let key = |key| ir::GroupStart {
        slot: 2,
        test: ir::StartTest::Key {
            low: key,
            high: key,
        },
        next: ir::StartJoin::And,
    };
    let mut ir = ir::Instrument {
        assets: vec![asset("first"), asset("second")],
        default_keyswitch: Some(13),
        groups: vec![
            ir::Group {
                start: vec![key(12)],
                ..Default::default()
            },
            ir::Group {
                start: vec![key(13)],
                ..Default::default()
            },
        ],
        articulations: vec![
            ir::Articulation {
                switch_keys: vec![12],
                ..Default::default()
            },
            ir::Articulation {
                switch_keys: vec![13],
                default: true,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    ir.zones = (0..2)
        .map(|index| ir::Zone {
            group: Some(ir::GroupRef(index)),
            keys: ir::KeyRange { low: 60, high: 60 },
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(index))
        })
        .collect();
    let plan = lower(&ir, 48000, vec![constant(0.1), constant(0.2)], no_behaviors).unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    assert_eq!(play(&mut rt, 60, 1.), [0.2; 2]);
    let p = rt.performance(0).unwrap();
    rt.set_articulation(p, 1).unwrap(); // Overlay selects authored index 0, lowered ID 1.
    let mut silence = [[0.; 2]; 128];
    rt.render(&mut silence).unwrap();
    assert_eq!(play(&mut rt, 60, 1.), [0.1; 2]);
    rt.trigger(input(13), 13, 1.).unwrap();
    rt.render(&mut silence).unwrap();
    assert_eq!(play(&mut rt, 60, 1.), [0.2; 2]);
}

#[test]
fn addressed_gain_and_filter_controls_drive_real_audio_lanes() {
    use sampler_core::{
        ControlValue, EngineParameterAddress, EngineParameterBinding, EngineParameterLaw,
        engine_parameter_id,
    };
    let key = "authored/group0/insert9/gain";
    let mut instrument = ir::Instrument {
        assets: vec![asset("test")],
        zones: vec![ir::Zone {
            keys: ir::KeyRange { low: 60, high: 60 },
            chain: Some(ir::ChainRef(0)),
            velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        controls: vec![ir::Control {
            key: key.into(),
            label: "gain".into(),
            value: ir::ControlValue::Continuous {
                min: 0.,
                max: 1.,
                default: 1.,
                unit: ir::ControlUnit::None,
            },
            automation: ir::Automation::None,
        }],
        processor_controls: vec![ir::ProcessorControl {
            control: ir::ControlRef(0),
            chain: ir::ChainRef(0),
            index: 0,
            parameter: ir::ProcessorParameter::Gain,
            ramp: ir::Time::ZERO,
        }],
        chains: vec![ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Gain(ir::Gain::UNITY)],
            post_amplitude: vec![],
        }],
        ..Default::default()
    };
    let address = EngineParameterAddress {
        parameter: engine_parameter_id("$ENGINE_PAR_VOLUME").unwrap(),
        group: 0,
        slot: 9,
        generic: 1,
    };
    let id = sampler_core::lower::ir_control_id(key);
    let prepare = |ir: &ir::Instrument, pcm| {
        lower(ir, 48000, vec![pcm], no_behaviors)
            .unwrap()
            .with_engine_parameters(
                vec![EngineParameterBinding {
                    address,
                    control: id,
                    law: EngineParameterLaw::Linear { low: 0., high: 1. },
                }],
                vec![],
            )
            .unwrap()
    };
    let plan = prepare(&instrument, constant(0.5));
    let mut rt = Runtime::new(plan, limits()).unwrap();
    rt.trigger(input(60), 60, 1.).unwrap();
    let mut out = [[0.; 2]; 64];
    rt.render(&mut out).unwrap();
    let before = out[32][0];
    rt.set_engine_parameter(address, 250000).unwrap();
    rt.render(&mut out).unwrap();
    assert!((out[32][0] / before - 0.25).abs() < 0.001);
    assert_eq!(rt.engine_parameter(address), Ok(250000));
    assert_eq!(
        rt.control_value(rt.active_plan(), id),
        Ok(ControlValue::Real(0.25))
    );
    // v1's user offset rides this same physical gain lane; the script base is unchanged.
    rt.set_engine_offsets(&[sampler_core::EngineParameterOffset {
        address,
        offset: 0.1,
    }])
    .unwrap();
    rt.render(&mut out).unwrap();
    assert!((out[32][0] / before - 0.35).abs() < 0.001);
    assert_eq!(rt.engine_parameter(address), Ok(250000));
    rt.set_engine_offsets(&[]).unwrap();
    rt.render(&mut out).unwrap();
    assert!((out[32][0] / before - 0.25).abs() < 0.001);
    instrument.chains[0].pre_amplitude[0] =
        ir::Processor::Color(ir::fx::color::Config::gainer(ir::Gain::UNITY, 0.));
    let mut rt = Runtime::new(prepare(&instrument, constant(0.5)), limits()).unwrap();
    rt.trigger(input(60), 60, 1.).unwrap();
    rt.render(&mut out).unwrap();
    rt.set_engine_parameter(address, 250000).unwrap();
    let mut settled = [[0.; 2]; 2048];
    rt.render(&mut settled).unwrap();
    let expected = 0.25 + 0.75 * (1. - f64::from(f32::from_bits(0x3a11a2b4))).powi(2047);
    assert!((f64::from(settled[2047][0]) / 0.5 - expected).abs() < 1e-5);
    instrument.controls[0].value = ir::ControlValue::Continuous {
        min: 100.,
        max: 12000.,
        default: 100.,
        unit: ir::ControlUnit::None,
    };
    instrument.processor_controls[0].parameter = ir::ProcessorParameter::Cutoff;
    instrument.chains[0].pre_amplitude[0] = ir::Processor::Filter(ir::Filter {
        kind: ir::FilterKind::LowPass { poles: 1 },
        cutoff: ir::Frequency::Hertz(100.),
        resonance: ir::Resonance::Q(0.7),
    });
    let pcm = Pcm::new(
        48000,
        (0..4096)
            .map(|i| [(i as f32 * std::f32::consts::TAU / 8.).sin(); 2])
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
    .unwrap();
    let plan = lower(&instrument, 48000, vec![pcm], no_behaviors)
        .unwrap()
        .with_engine_parameters(
            vec![EngineParameterBinding {
                address,
                control: id,
                law: EngineParameterLaw::Linear {
                    low: 100.,
                    high: 12000.,
                },
            }],
            vec![],
        )
        .unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    rt.trigger(input(60), 60, 1.).unwrap();
    let mut out = [[0.; 2]; 1024];
    rt.render(&mut out).unwrap();
    let low: f32 = out[512..].iter().map(|f| f[0] * f[0]).sum();
    rt.set_engine_parameter(address, 1000000).unwrap();
    rt.render(&mut out).unwrap();
    let high: f32 = out[512..].iter().map(|f| f[0] * f[0]).sum();
    assert!(high > low * 100., "{low} {high}");
    instrument.controls[0].value = ir::ControlValue::Continuous {
        min: 0.,
        max: 1.,
        default: 0.,
        unit: ir::ControlUnit::None,
    };
    instrument.chains[0].pre_amplitude[0] = ir::Processor::Daft(ir::Daft {
        address: None,
        gain: 0.,
        cutoff: 0.,
        resonance: 0.,
        highpass: false,
    });
    let pcm = Pcm::new(
        48000,
        (0..4096)
            .map(|i| [(i as f32 * std::f32::consts::TAU / 8.).sin(); 2])
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
    .unwrap();
    let plan = prepare(&instrument, pcm);
    let mut rt = Runtime::new(plan, limits()).unwrap();
    rt.trigger(input(60), 60, 1.).unwrap();
    rt.render(&mut out).unwrap();
    let low: f32 = out[512..].iter().map(|f| f[0] * f[0]).sum();
    rt.set_engine_parameter(address, 1000000).unwrap();
    rt.render(&mut out).unwrap();
    let high: f32 = out[512..].iter().map(|f| f[0] * f[0]).sum();
    assert!(high > low * 100., "Daft {low} {high}");
    instrument.chains[0].pre_amplitude[0] = ir::Processor::LadderLP4(ir::LadderLP4 {
        address: None,
        gain: 0.,
        cutoff: 0.,
        resonance: 0.,
        cutoff_path: true,
        taps: 4,
        high_quality: false,
    });
    let pcm = Pcm::new(
        48000,
        (0..4096)
            .map(|i| [(i as f32 * std::f32::consts::TAU / 8.).sin() * 0.001; 2])
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
    .unwrap();
    let mut rt = Runtime::new(prepare(&instrument, pcm), limits()).unwrap();
    rt.trigger(input(60), 60, 1.).unwrap();
    rt.render(&mut out).unwrap();
    let low: f32 = out[512..].iter().map(|f| f[0] * f[0]).sum();
    rt.set_engine_parameter(address, 1000000).unwrap();
    rt.render(&mut out).unwrap();
    let high: f32 = out[512..].iter().map(|f| f[0] * f[0]).sum();
    assert!(high > low * 100., "Ladder LP4 {low} {high}");
}

#[test]
fn native_stereo_width_and_pan_controls_reach_the_processor() {
    use sampler_core::{
        ControlValue, EngineParameterAddress, EngineParameterBinding, EngineParameterLaw,
        engine_parameter_id,
    };
    let keys = [
        "xml/program/layer2/node7/width",
        "xml/program/layer2/node7/pan",
    ];
    let addresses =
        ["ENGINE_PAR_STEREO", "ENGINE_PAR_STEREO_PAN"].map(|name| EngineParameterAddress {
            parameter: engine_parameter_id(name).unwrap(),
            group: 2,
            slot: 7,
            generic: -1,
        });
    let ir = ir::Instrument {
        assets: vec![asset("stereo")],
        zones: vec![ir::Zone {
            keys: ir::KeyRange { low: 60, high: 60 },
            chain: Some(ir::ChainRef(0)),
            velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        chains: vec![ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Color(ir::fx::color::Config::stereo(0.5, 0., false))],
            post_amplitude: vec![],
        }],
        controls: keys
            .iter()
            .enumerate()
            .map(|(i, key)| ir::Control {
                key: (*key).into(),
                label: String::new(),
                value: ir::ControlValue::Continuous {
                    min: if i == 0 { 0. } else { -1. },
                    max: 1.,
                    default: if i == 0 { 0.5 } else { 0. },
                    unit: ir::ControlUnit::None,
                },
                automation: ir::Automation::None,
            })
            .collect(),
        processor_controls: [ir::ProcessorParameter::Width, ir::ProcessorParameter::Pan]
            .into_iter()
            .enumerate()
            .map(|(i, parameter)| ir::ProcessorControl {
                control: ir::ControlRef(i),
                chain: ir::ChainRef(0),
                index: 0,
                parameter,
                ramp: ir::Time::ZERO,
            })
            .collect(),
        ..Default::default()
    };
    let plan = lower(
        &ir,
        48000,
        vec![Pcm::new(48000, vec![[0.1, 0.2]; 4096].into_boxed_slice()).unwrap()],
        no_behaviors,
    )
    .unwrap()
    .with_engine_parameters(
        addresses
            .into_iter()
            .enumerate()
            .map(|(i, address)| EngineParameterBinding {
                address,
                control: sampler_core::lower::ir_control_id(keys[i]),
                law: EngineParameterLaw::Linear {
                    low: if i == 0 { 0. } else { -1. },
                    high: 1.,
                },
            })
            .collect(),
        vec![],
    )
    .unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    rt.trigger(input(60), 60, 1.).unwrap();
    let mut out = [[0.; 2]; 1024];
    rt.render(&mut out).unwrap();
    assert!((out[1023][0] - 0.1).abs() < 1e-5);
    rt.set_engine_parameter(addresses[0], 0).unwrap();
    rt.render(&mut out).unwrap();
    let remaining_width = 0.5 * (1. - 1. / 180f64).powi(1023);
    assert!((f64::from(out[1023][0]) - (0.15 - 0.1 * remaining_width)).abs() < 1e-5);
    assert!((f64::from(out[1023][1]) - (0.15 + 0.1 * remaining_width)).abs() < 1e-5);
    rt.set_engine_parameter(addresses[1], 1_000_000).unwrap();
    rt.render(&mut out).unwrap();
    assert!(out[1023][0] < 0.1 && out[1023][1] > 0.149);
    assert_eq!(
        rt.control_value(
            rt.active_plan(),
            sampler_core::lower::ir_control_id(keys[1])
        ),
        Ok(ControlValue::Real(1.))
    );
}

#[test]
fn authored_delay_runs_existing_dsp_at_the_requested_time() {
    let mut instrument = ir::Instrument {
        assets: vec![asset("impulse")],
        zones: vec![ir::Zone {
            keys: ir::KeyRange { low: 60, high: 60 },
            chain: Some(ir::ChainRef(0)),
            velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        chains: vec![ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Delay {
                time: ir::Time::Seconds(4. / 48000.),
                feedback: 0.5,
                mix: 1.,
            }],
            post_amplitude: vec![],
        }],
        ..Default::default()
    };
    let mut samples = vec![[0.; 2]; 64];
    samples[0] = [1.; 2];
    let plan = lower(
        &instrument,
        48000,
        vec![Pcm::new(48000, samples.clone().into_boxed_slice()).unwrap()],
        no_behaviors,
    )
    .unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    rt.trigger(input(60), 60, 1.).unwrap();
    let mut output = [[0.; 2]; 16];
    rt.render(&mut output).unwrap();
    assert!(output[..4].iter().all(|sample| sample[0] == 0.));
    assert!(output[4][0] > 0.);
    assert!((output[8][0] / output[4][0] - 0.5).abs() < 1e-6);
    instrument.chains[0].pre_amplitude[0] = ir::Processor::Delay {
        time: ir::Time::ZERO,
        feedback: 0.5,
        mix: 1.,
    };
    let plan = lower(
        &instrument,
        48000,
        vec![Pcm::new(48000, samples.into_boxed_slice()).unwrap()],
        no_behaviors,
    )
    .unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    rt.trigger(input(60), 60, 1.).unwrap();
    rt.render(&mut output).unwrap();
    assert!(output[0][0] > 0.);
    assert!(output[1..].iter().all(|sample| sample[0] == 0.));
}

#[test]
fn processor_modulation_keeps_filter_identity_in_multiple_chains_and_cascades() {
    let render = |poles: u8, routed: bool, target: usize| {
        let filter = |hz| {
            ir::Processor::Filter(ir::Filter {
                kind: ir::FilterKind::LowPass { poles },
                cutoff: ir::Frequency::Hertz(hz),
                resonance: ir::Resonance::Decibels(0.0),
            })
        };
        let mut ir = ir::Instrument {
            assets: vec![asset("probe")],
            chains: vec![ir::Chain {
                scope: ir::Scope::Voice,
                pre_amplitude: vec![filter(if !routed && target == 0 {
                    2000.0
                } else {
                    500.0
                })],
                post_amplitude: vec![filter(if !routed && target == 1 {
                    8000.0
                } else {
                    2000.0
                })],
            }],
            modulators: vec![ir::Modulator {
                scope: ir::Scope::Voice,
                source: ir::ModulationSource::Constant,
            }],
            ..Default::default()
        };
        // Separate lowered chains share the same authored chain. Routes must
        // address each compiled chain, not reuse the first zone's filter IDs.
        for key in [60, 61] {
            ir.zones.push(ir::Zone {
                keys: ir::KeyRange {
                    low: key,
                    high: key,
                },
                velocity: ir::VelocityResponse::None,
                pitch: ir::KeyTracking::Fixed,
                chain: Some(ir::ChainRef(0)),
                gain: ir::Gain::Decibels(6.0),
                pan: ir::Pan {
                    position: 0.25,
                    law: ir::PanLaw::Balance,
                },
                routes: if routed {
                    vec![ir::RouteRef(0)]
                } else {
                    vec![]
                },
                ..ir::Zone::new(ir::AssetRef(0))
            });
        }
        ir.routes.push(ir::Route {
            source: ir::ModulatorRef(0),
            target: ir::Target::Processor {
                chain: ir::ChainRef(0),
                index: target,
                parameter: ir::ProcessorParameter::Cutoff,
            },
            depth: ir::Depth::Pitch(ir::Pitch::Semitones(24.0)),
            invert: false,
            shape: None,
            smoothing: ir::Time::Seconds(0.0),
            scale: None,
        });
        let wave = (0..4096)
            .map(|i| [(std::f32::consts::TAU * 3000.0 * i as f32 / 48000.0).sin() * 0.5; 2])
            .collect::<Vec<_>>();
        let plan = sampler_core::lower::lower_with(
            &ir,
            48000,
            vec![Pcm::new(48000, wave.into_boxed_slice()).unwrap()],
            &sampler_core::lower::Options { mpe: None },
            no_behaviors,
        )
        .unwrap();
        let mut rt = Runtime::new(plan, limits()).unwrap();
        let mut out = [[0.0; 2]; 2048];
        support::without_heap(|| {
            rt.trigger(input(60), 60, 1.0).unwrap();
            rt.trigger(input(61), 61, 1.0).unwrap();
            rt.render(&mut out).unwrap();
        });
        out
    };
    for poles in [1, 2, 4] {
        for target in [0, 1] {
            let expected = render(poles, false, target);
            let actual = render(poles, true, target);
            for (a, b) in actual[512..].iter().zip(&expected[512..]) {
                assert!(
                    (a[0] - b[0]).abs() < 1e-5,
                    "poles={poles}, target={target}: {a:?} vs {b:?}"
                );
            }
        }
    }
}

#[test]
fn authored_voice_taps_reach_summed_buses_and_real_engine_controls() {
    use sampler_core::{EngineParameterAddress, EngineParameterBinding, EngineParameterLaw};
    let mut instrument = ir::Instrument {
        assets: vec![asset("send probe")],
        zones: vec![ir::Zone {
            keys: ir::KeyRange { low: 60, high: 60 },
            velocity: ir::VelocityResponse::None,
            pitch: ir::KeyTracking::Fixed,
            amplitude: Some(ir::ModulatorRef(0)),
            chain: Some(ir::ChainRef(0)),
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        modulators: vec![ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Envelope(ir::Envelope {
                sustain: 0.25,
                ..Default::default()
            }),
        }],
        chains: vec![ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Gain(ir::Gain::Linear(2.0))],
            post_amplitude: vec![
                ir::Processor::Gain(ir::Gain::Linear(3.0)),
                ir::Processor::Gain(ir::Gain::Linear(0.0)),
            ],
        }],
        buses: vec![ir::Bus {
            name: "return".into(),
            chain: None,
            gain: ir::Gain::Linear(2.0),
            output: ir::Output::Master,
            sends: vec![],
        }],
        controls: vec![ir::Control {
            key: "original/send/gain".into(),
            label: "send".into(),
            value: ir::ControlValue::Continuous {
                min: 0.0,
                max: 1.0,
                default: 0.5,
                unit: ir::ControlUnit::None,
            },
            automation: ir::Automation::default(),
        }],
        voice_send_taps: vec![ir::VoiceSendTap {
            chain: ir::ChainRef(0),
            position: ir::VoiceSendPosition::BeforeAmplitude(1),
            bus: ir::BusRef(0),
            gain: ir::Gain::Linear(0.5),
            bypass: false,
            gain_control: Some(ir::ControlRef(0)),
            bypass_control: None,
            ramp: ir::Time::Seconds(0.0),
        }],
        ..Default::default()
    };
    let render = |instrument: &ir::Instrument| {
        let plan = lower(instrument, 48000, vec![constant(1.0)], no_behaviors).unwrap();
        let address = EngineParameterAddress {
            parameter: sampler_core::engine_parameter_id("ENGINE_PAR_SENDLEVEL_0").unwrap(),
            group: 0,
            slot: 3,
            generic: 0,
        };
        let plan = plan
            .with_engine_parameters(
                vec![EngineParameterBinding {
                    address,
                    control: sampler_core::lower::ir_control_id("original/send/gain"),
                    law: EngineParameterLaw::Linear {
                        low: 0.0,
                        high: 1.0,
                    },
                }],
                vec![],
            )
            .unwrap();
        let mut rt = Runtime::new(plan, limits()).unwrap();
        let mut out = [[0.0; 2]; 64];
        support::without_heap(|| {
            rt.trigger(input(60), 60, 1.0).unwrap();
            rt.render(&mut out).unwrap();
        });
        let before = out[32];
        support::without_heap(|| {
            rt.set_engine_parameter(address, 0).unwrap();
            rt.render(&mut out).unwrap();
        });
        assert_eq!(out[32], [0.0; 2]);
        before
    };
    assert_eq!(render(&instrument), [2.0; 2]);
    instrument.voice_send_taps[0].position = ir::VoiceSendPosition::AfterAmplitude(1);
    assert_eq!(render(&instrument), [1.5; 2]);
    instrument.voice_send_taps[0].position = ir::VoiceSendPosition::BeforeAmplitude(2);
    assert!(matches!(
        lower(&instrument, 48000, vec![constant(1.0)], no_behaviors),
        Err(LowerError::Invalid(_))
    ));
}

#[test]
fn lower_retains_pure_delay_and_pseudo_stereo_after_source_end() {
    for processor in [
        ir::Processor::Delay {
            time: ir::Time::Seconds(3.0 / 48000.0),
            feedback: 0.0,
            mix: 1.0,
        },
        ir::Processor::Color(ir::fx::color::Config::stereo(0.5, 0.0, true)),
    ] {
        let instrument = ir::Instrument {
            assets: vec![asset("tail probe")],
            zones: vec![ir::Zone {
                velocity: ir::VelocityResponse::None,
                pitch: ir::KeyTracking::Fixed,
                chain: Some(ir::ChainRef(0)),
                ..ir::Zone::new(ir::AssetRef(0))
            }],
            chains: vec![ir::Chain {
                scope: ir::Scope::Voice,
                pre_amplitude: vec![],
                post_amplitude: vec![processor],
            }],
            ..Default::default()
        };
        let plan = lower(
            &instrument,
            48000,
            vec![Pcm::new(48000, vec![[1.0; 2]].into_boxed_slice()).unwrap()],
            no_behaviors,
        )
        .unwrap();
        let mut rt = Runtime::new(plan, limits()).unwrap();
        let mut out = [[0.0; 2]; 128];
        support::without_heap(|| {
            rt.trigger(input(60), 60, 1.0).unwrap();
            rt.render(&mut out).unwrap();
        });
        let (index, channel) = if matches!(processor, ir::Processor::Delay { .. }) {
            (3, 0)
        } else {
            (60, 1)
        };
        assert_eq!(out[index][channel], 1.0);
    }
}

#[test]
fn stale_physical_zone_maps_return_invalid_instead_of_panicking() {
    for runtime in [0, usize::MAX] {
        let mut instrument = instrument();
        instrument.source_indices.zones = vec![None, Some(ir::ZoneRef(runtime))];
        instrument.zones.clear();
        instrument.assets.clear();
        assert!(matches!(
            rejected(&instrument, vec![]),
            LowerError::Invalid(ir::ValidationError::Dangling { owner, .. })
                if owner == "source zone 1"
        ));
    }
    let mut holes = ir::Instrument::default();
    holes.source_indices.zones = vec![None, None];
    assert!(lower(&holes, 48000, vec![], no_behaviors).is_ok());
}

#[test]
fn physical_ladder_lanes_keep_signed_gain_and_drive_audio_without_heap() {
    use sampler_core::{EngineParameterAddress, engine_parameter_id};
    let address = ir::SlotAddress {
        group: 4,
        slot: 3,
        generic: -1,
    };
    let instrument = ir::Instrument {
        assets: vec![asset("tone")],
        chains: vec![ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::LadderLP4(ir::LadderLP4 {
                address: Some(address),
                gain: -0.25,
                cutoff: 1.,
                resonance: 0.,
                cutoff_path: true,
                taps: 4,
                high_quality: false,
            })],
            post_amplitude: vec![],
        }],
        zones: vec![ir::Zone {
            keys: ir::KeyRange { low: 60, high: 60 },
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            chain: Some(ir::ChainRef(0)),
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        ..Default::default()
    };
    let pcm = Pcm::new(
        48000,
        (0..10000)
            .map(|i| [(i as f32 * std::f32::consts::TAU / 100.).sin() * 0.0001; 2])
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
    .unwrap();
    let plan = lower(&instrument, 48000, vec![pcm], no_behaviors).unwrap();
    assert_eq!(plan.engine_parameter_bindings().len(), 3);
    let gain = EngineParameterAddress {
        parameter: engine_parameter_id("ENGINE_PAR_GAIN").unwrap(),
        group: 4,
        slot: 3,
        generic: -1,
    };
    let mut rt = Runtime::new(plan, limits()).unwrap();
    assert_eq!(rt.engine_parameter(gain), Ok(-250000));
    let mut out = [[0.; 2]; 2000];
    rt.trigger(input(60), 60, 1.).unwrap();
    support::without_heap(|| rt.render(&mut out).unwrap());
    let low: f64 = out[1000..].iter().map(|x| f64::from(x[0]).powi(2)).sum();
    support::without_heap(|| {
        rt.set_engine_parameter(gain, 250000).unwrap();
        rt.render(&mut out).unwrap();
    });
    let high: f64 = out[1000..].iter().map(|x| f64::from(x[0]).powi(2)).sum();
    assert!(
        (10. * (high / low).log10() - 6.).abs() < 0.02,
        "{low} {high}"
    );
    assert_eq!(rt.engine_parameter(gain), Ok(250000));
}

#[test]
fn normalized_cutoff_routes_address_ladder_and_daft_without_audio_heap() {
    for ladder in [true, false] {
        for post in [false, true] {
            for mixed in [false, true] {
                let render = |cutoff, depth| {
                    let processor = if ladder {
                        ir::Processor::LadderLP4(ir::LadderLP4 {
                            address: None,
                            gain: 0.,
                            cutoff,
                            resonance: 0.1,
                            cutoff_path: true,
                            taps: 4,
                            high_quality: false,
                        })
                    } else {
                        ir::Processor::Daft(ir::Daft {
                            address: None,
                            gain: 0.,
                            cutoff,
                            resonance: 0.1,
                            highpass: false,
                        })
                    };
                    let mut processors = vec![
                        ir::Processor::Gain(ir::Gain::UNITY),
                        ir::Processor::Filter(ir::Filter {
                            kind: ir::FilterKind::LowPass { poles: 4 },
                            cutoff: ir::Frequency::Hertz(20000.),
                            resonance: ir::Resonance::Q(0.7),
                        }),
                    ];
                    if mixed {
                        processors.push(ir::Processor::Mix {
                            count: 1,
                            address: ir::SlotAddress {
                                group: 0,
                                slot: 5,
                                generic: -1,
                            },
                            dry: 0.,
                            wet: 1.,
                            bypass: false,
                        });
                    }
                    let index = processors.len();
                    processors.push(processor);
                    let mut chains = vec![
                        ir::Chain {
                            scope: ir::Scope::Voice,
                            pre_amplitude: vec![],
                            post_amplitude: vec![]
                        };
                        13
                    ];
                    chains.push(ir::Chain {
                        scope: ir::Scope::Voice,
                        pre_amplitude: if post { vec![] } else { processors.clone() },
                        post_amplitude: if post { processors } else { vec![] },
                    });
                    let instrument = ir::Instrument {
                        assets: vec![asset("native cutoff")],
                        chains,
                        modulators: vec![ir::Modulator {
                            scope: ir::Scope::Voice,
                            source: ir::ModulationSource::Constant,
                        }],
                        routes: vec![ir::Route {
                            source: ir::ModulatorRef(0),
                            target: ir::Target::Processor {
                                chain: ir::ChainRef(13),
                                index,
                                parameter: ir::ProcessorParameter::Cutoff,
                            },
                            depth: ir::Depth::Normalized(depth),
                            invert: false,
                            shape: None,
                            smoothing: ir::Time::Seconds(0.),
                            scale: None,
                        }],
                        zones: vec![ir::Zone {
                            chain: Some(ir::ChainRef(13)),
                            routes: vec![ir::RouteRef(0)],
                            pitch: ir::KeyTracking::Fixed,
                            velocity: ir::VelocityResponse::None,
                            ..ir::Zone::new(ir::AssetRef(0))
                        }],
                        ..Default::default()
                    };
                    let pcm = Pcm::new(
                        48000,
                        (0..4096)
                            .map(|i| [0.01 * (i as f32 * std::f32::consts::TAU / 24.).sin(); 2])
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    )
                    .unwrap();
                    let plan = lower(&instrument, 48000, vec![pcm], no_behaviors).unwrap();
                    let mut runtime = Runtime::new(plan, limits()).unwrap();
                    let mut out = [[0.; 2]; 2048];
                    support::without_heap(|| {
                        runtime.trigger(input(60), 60, 1.).unwrap();
                        for frames in out.chunks_mut(7) {
                            runtime.render(frames).unwrap();
                        }
                        runtime.note_off(input(60), None).unwrap();
                    });
                    out
                };
                let dry = render(0.5, 0.);
                let wet = render(0.5, 0.25);
                assert_eq!(
                    wet,
                    render(0.75, 0.),
                    "normalized depth must adjust the saved knob before conversion"
                );
                let energy = |x: &[[f32; 2]]| {
                    x[1024..]
                        .iter()
                        .flatten()
                        .map(|v| f64::from(*v).powi(2))
                        .sum::<f64>()
                };
                assert!(
                    10. * (energy(&wet) / energy(&dry)).log10() > 3.,
                    "opening the cutoff must pass more of the 2 kHz tone"
                );
            }
        }
    }
}

#[test]
fn normalized_q_gain_routes_address_native_knobs_without_audio_heap() {
    for ladder in [true, false] {
        for parameter in [
            ir::ProcessorParameter::Resonance,
            ir::ProcessorParameter::Gain,
        ] {
            let render = |value, depth| {
                let (gain, resonance) = if parameter == ir::ProcessorParameter::Gain {
                    (value, 0.1)
                } else {
                    (0., value)
                };
                let processor = if ladder {
                    ir::Processor::LadderLP4(ir::LadderLP4 {
                        address: None,
                        gain,
                        cutoff: 0.5,
                        resonance,
                        cutoff_path: true,
                        taps: 4,
                        high_quality: false,
                    })
                } else {
                    ir::Processor::Daft(ir::Daft {
                        address: None,
                        gain,
                        cutoff: 0.5,
                        resonance,
                        highpass: false,
                    })
                };
                let instrument = ir::Instrument {
                    assets: vec![asset("native cutoff")],
                    chains: vec![ir::Chain {
                        scope: ir::Scope::Voice,
                        pre_amplitude: vec![ir::Processor::Gain(ir::Gain::Linear(1.)), processor],
                        post_amplitude: vec![],
                    }],
                    modulators: vec![ir::Modulator {
                        scope: ir::Scope::Voice,
                        source: ir::ModulationSource::Constant,
                    }],
                    routes: vec![ir::Route {
                        source: ir::ModulatorRef(0),
                        target: ir::Target::Processor {
                            chain: ir::ChainRef(0),
                            index: 1,
                            parameter,
                        },
                        depth: ir::Depth::Normalized(depth),
                        invert: false,
                        shape: None,
                        smoothing: ir::Time::Seconds(0.),
                        scale: None,
                    }],
                    zones: vec![ir::Zone {
                        chain: Some(ir::ChainRef(0)),
                        routes: vec![ir::RouteRef(0)],
                        pitch: ir::KeyTracking::Fixed,
                        velocity: ir::VelocityResponse::None,
                        ..ir::Zone::new(ir::AssetRef(0))
                    }],
                    ..Default::default()
                };
                let pcm = Pcm::new(
                    48000,
                    (0..4096)
                        .map(|i| [0.01 * (i as f32 * std::f32::consts::TAU / 24.).sin(); 2])
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                )
                .unwrap();
                let plan = lower(&instrument, 48000, vec![pcm], no_behaviors).unwrap();
                let mut runtime = Runtime::new(plan, limits()).unwrap();
                let mut out = [[0.; 2]; 2048];
                support::without_heap(|| {
                    runtime.trigger(input(60), 60, 1.).unwrap();
                    for frames in out.chunks_mut(7) {
                        runtime.render(frames).unwrap();
                    }
                    runtime.note_off(input(60), None).unwrap();
                });
                out
            };
            let dry = render(0.1, 0.);
            let wet = render(0.1, 0.4);
            assert_eq!(
                wet,
                render(0.5, 0.),
                "depth must add to the native knob before conversion"
            );
            assert_ne!(wet, dry, "Q/Gain must change audio");
            // Every enabled route saturates, Daft gain included: the native driver clamps routed
            // sources to [0, 1] (FUN_1408fa270, daft_route_clamp_kemu.json).
            assert_eq!(
                render(0.9, 0.4),
                render(1., 0.),
                "normalized knobs saturate"
            );
            if ladder && parameter == ir::ProcessorParameter::Gain {
                assert_eq!(
                    render(-0.25, 0.),
                    render(0., 0.),
                    "enabled zero-depth Gain clamps saved signed gain, as v1 does"
                );
            }
        }
    }
}

/// A velocity route of `depth` to a 0..=`max` gain lane, its intensity scaled
/// by velocity (scale depth 1, `flags`); the control's base is 0.
fn scaled_wide_lane(depth: f64, flags: u8, max: f64) -> ir::Instrument {
    ir::Instrument {
        assets: vec![asset("scaled generic target")],
        controls: vec![ir::Control {
            key: "custom/gain/scaled".into(),
            label: "level".into(),
            value: ir::ControlValue::Continuous {
                min: 0.,
                max,
                default: 0.,
                unit: ir::ControlUnit::None,
            },
            automation: ir::Automation::None,
        }],
        chains: vec![ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Gain(ir::Gain::UNITY)],
            post_amplitude: vec![],
        }],
        processor_controls: vec![ir::ProcessorControl {
            control: ir::ControlRef(0),
            chain: ir::ChainRef(0),
            index: 0,
            parameter: ir::ProcessorParameter::Gain,
            ramp: ir::Time::ZERO,
        }],
        modulators: vec![
            ir::Modulator {
                scope: ir::Scope::Voice,
                source: ir::ModulationSource::Velocity,
            },
            ir::Modulator {
                scope: ir::Scope::Voice,
                source: ir::ModulationSource::Velocity,
            },
        ],
        routes: vec![ir::Route {
            source: ir::ModulatorRef(0),
            target: ir::Target::Control(ir::ControlRef(0)),
            depth: ir::Depth::Normalized(depth),
            invert: false,
            shape: None,
            smoothing: ir::Time::ZERO,
            scale: Some(ir::RouteScale {
                source: ir::ModulatorRef(1),
                shape: None,
                law: ir::ScaleLaw::KontaktIntensity {
                    depth: 1.,
                    flags,
                    unit: 1.,
                },
                lag_ms: 0.,
            }),
        }],
        zones: vec![ir::Zone {
            chain: Some(ir::ChainRef(0)),
            routes: vec![ir::RouteRef(0)],
            keys: ir::KeyRange { low: 60, high: 60 },
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        ..Default::default()
    }
}

fn scaled_wide_lane_out(depth: f64, flags: u8, velocity: f64) -> [f32; 2] {
    let prepared = lower(&scaled_wide_lane(depth, flags, 2.), 48000, vec![constant(0.1)], no_behaviors)
        .expect("a normalized intensity within 0..=1 is admissible on any lane span");
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    rt.trigger(input(60), 60, velocity).unwrap();
    let mut out = [[0.; 2]; 256];
    rt.render(&mut out).unwrap();
    out[128]
}

#[test]
fn an_intensity_scaled_route_keeps_its_normalized_intensity_on_a_wide_processor_lane() {
    // The lane spans 0..=2, so the bound depth doubles; the intensity unit must too.
    // Flag 0x04 combines in normalized intensity: 1 - (1 - 0.25) * (1 - 0.5) = 0.625,
    // times the span 2 and the route's velocity 0.5: 0.1 * 0.625.
    let [l, r] = scaled_wide_lane_out(0.25, 0x14, 0.5);
    assert!((l - 0.0625).abs() < 1e-6 && (r - 0.0625).abs() < 1e-6, "{l} {r}");
    // A full scaling source leaves the route at its saved depth: 0.1 * (0 + 0.75 * 2).
    assert_eq!(scaled_wide_lane_out(0.75, 0x10, 1.), [0.15; 2]);
}

#[test]
fn an_intensity_scaled_route_on_a_zero_width_lane_still_loads() {
    lower(&scaled_wide_lane(0.75, 0x10, 0.), 48000, vec![constant(0.1)], no_behaviors)
        .expect("a route with no depth left is a no-op, not an error");
}

#[test]
fn arbitrary_registered_control_target_drives_its_real_processor_per_voice_without_heap() {
    let mut instrument = ir::Instrument {
        assets: vec![asset("generic target")],
        controls: vec![ir::Control {
            key: "custom/gain/no-native-id".into(),
            label: "level".into(),
            value: ir::ControlValue::Continuous {
                min: 0.,
                max: 2.,
                default: 1.,
                unit: ir::ControlUnit::None,
            },
            automation: ir::Automation::None,
        }],
        chains: vec![ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Gain(ir::Gain::UNITY)],
            post_amplitude: vec![],
        }],
        processor_controls: vec![ir::ProcessorControl {
            control: ir::ControlRef(0),
            chain: ir::ChainRef(0),
            index: 0,
            parameter: ir::ProcessorParameter::Gain,
            ramp: ir::Time::ZERO,
        }],
        modulators: vec![ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Velocity,
        }],
        routes: vec![ir::Route {
            source: ir::ModulatorRef(0),
            target: ir::Target::Control(ir::ControlRef(0)),
            depth: ir::Depth::Normalized(0.25),
            invert: false,
            shape: None,
            smoothing: ir::Time::ZERO,
            scale: None,
        }],
        zones: vec![ir::Zone {
            chain: Some(ir::ChainRef(0)),
            routes: vec![ir::RouteRef(0)],
            keys: ir::KeyRange { low: 60, high: 60 },
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        ..Default::default()
    };
    for (processor_target, threads, block) in
        [false, true].into_iter().flat_map(|processor_target| {
            [1, 2, 4].into_iter().flat_map(move |threads| {
                [1, 7, 64, 137].map(|block| (processor_target, threads, block))
            })
        })
    {
        instrument.routes[0].target = if processor_target {
            ir::Target::Processor {
                chain: ir::ChainRef(0),
                index: 0,
                parameter: ir::ProcessorParameter::Gain,
            }
        } else {
            ir::Target::Control(ir::ControlRef(0))
        };
        let prepared = lower(&instrument, 48000, vec![constant(0.1)], no_behaviors)
            .expect("registered processor parameter must have a generic route consumer");
        let published: Vec<_> = prepared.parameter_registry().descriptors().collect();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].name, "level");
        assert_eq!(published[0].range, [0., 2.]);
        assert_eq!(published[0].default, 1.);
        assert_eq!(
            published[0].address.scope,
            sampler_core::ParameterScope::Voice
        );
        assert_eq!(published[0].display.group, "chain 0 processor 0");
        let mut rt = Runtime::new(
            prepared,
            Limits {
                notes: 128,
                families: 128,
                expressions: 128,
                voices: 128,
                ..limits()
            },
        )
        .unwrap()
        .with_threads(sampler_core::Threads::Fixed(threads));
        let voice = |id| Input {
            protocol: Protocol::Clap,
            external_id: Some(id),
            ..input(60)
        };
        let mut first = [[0.; 2]; 256];
        support::without_heap(|| {
            rt.trigger(voice(1), 60, 1.).unwrap();
            for chunk in first.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
        });
        assert_eq!(first[128], [0.15; 2]);
        let mut second = [[0.; 2]; 256];
        support::without_heap(|| {
            // Independent voice contribution must not overwrite the first's target.
            rt.trigger(voice(2), 60, 0.5).unwrap();
            for chunk in second.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
        });
        assert_eq!(second[128], [0.275; 2]);
        let id = sampler_core::lower::ir_control_id("custom/gain/no-native-id");
        let active = rt.active_plan();
        assert_eq!(
            rt.control_value(active, id),
            Ok(sampler_core::ControlValue::Real(1.)),
            "voice modulation must not overwrite the shared editor/script base"
        );
        let mut edited = [[0.; 2]; 256];
        support::without_heap(|| {
            rt.edit_controls(
                active,
                None,
                &[sampler_core::ControlWrite {
                    id,
                    value: sampler_core::ControlValue::Real(0.5),
                }],
            )
            .unwrap();
            for chunk in edited.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
        });
        let expected = 0.1f32 + 0.1f32 * 0.75;
        assert_eq!(
            edited[128], [expected; 2],
            "a base edit must invalidate consumers while retaining each voice's held offset"
        );
        assert_eq!(
            rt.control_base_value(active, id),
            Ok(sampler_core::ControlValue::Real(0.5))
        );
        let mut many = [[0.; 2]; 256];
        support::without_heap(|| {
            for id in 3..=128 {
                rt.trigger(voice(id), 60, 1.).unwrap();
            }
            for chunk in many.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
        });
        let expected_many = (3..=128).fold(expected, |sum, _| sum + 0.1f32);
        assert_eq!(many[128], [expected_many; 2]);
        if threads > 1 {
            assert!(
                rt.parallel_blocks() > 0,
                "worker projection must actually execute"
            );
        }
    }
}

#[test]
fn generic_voice_filter_parameters_never_share_another_voices_coefficients() {
    let zone = |key, routes| ir::Zone {
        chain: Some(ir::ChainRef(0)),
        routes,
        keys: ir::KeyRange {
            low: key,
            high: key,
        },
        pitch: ir::KeyTracking::Fixed,
        velocity: ir::VelocityResponse::None,
        ..ir::Zone::new(ir::AssetRef(0))
    };
    let instrument = ir::Instrument {
        assets: vec![asset("per-voice filter coefficients")],
        controls: vec![ir::Control {
            key: "filter/cutoff".into(),
            label: "Cutoff".into(),
            value: ir::ControlValue::Continuous {
                min: 500.,
                max: 9500.,
                default: 500.,
                unit: ir::ControlUnit::Hertz,
            },
            automation: ir::Automation::None,
        }],
        chains: vec![ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Filter(ir::Filter {
                kind: ir::FilterKind::LowPass { poles: 2 },
                cutoff: ir::Frequency::Hertz(500.),
                resonance: ir::Resonance::Q(1.),
            })],
            post_amplitude: vec![],
        }],
        processor_controls: vec![ir::ProcessorControl {
            control: ir::ControlRef(0),
            chain: ir::ChainRef(0),
            index: 0,
            parameter: ir::ProcessorParameter::Cutoff,
            ramp: ir::Time::ZERO,
        }],
        modulators: vec![ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Velocity,
        }],
        routes: vec![ir::Route {
            source: ir::ModulatorRef(0),
            target: ir::Target::Control(ir::ControlRef(0)),
            depth: ir::Depth::Normalized(1.),
            invert: false,
            shape: None,
            smoothing: ir::Time::ZERO,
            scale: None,
        }],
        zones: vec![zone(60, vec![ir::RouteRef(0)]), zone(61, vec![])],
        ..Default::default()
    };
    let frames: Vec<_> = (0..4800)
        .map(|n| [0.01 * (std::f32::consts::TAU * n as f32 / 16.).sin(); 2])
        .collect();
    let runtime = |threads| {
        let pcm = Pcm::new(48000, frames.clone().into_boxed_slice()).unwrap();
        Runtime::new(
            lower(&instrument, 48000, vec![pcm], no_behaviors).unwrap(),
            Limits {
                notes: 128,
                families: 128,
                expressions: 128,
                voices: 128,
                ..limits()
            },
        )
        .unwrap()
        .with_threads(sampler_core::Threads::Fixed(threads))
    };
    for threads in [1, 2, 4] {
        let mut expected = [[0f32; 2]; 256];
        for id in 1..=128 {
            let (key, velocity) = match id % 3 {
                0 => (61, 1.),
                1 => (60, 1.),
                _ => (60, 0.1),
            };
            let input = Input {
                external_id: Some(id),
                ..input(key)
            };
            let mut rt = runtime(1);
            let mut separate = [[0.; 2]; 256];
            support::without_heap(|| {
                rt.trigger(input, key, velocity).unwrap();
                rt.render(&mut separate).unwrap();
            });
            for (sum, voice) in expected.iter_mut().zip(separate) {
                for c in 0..2 {
                    sum[c] += voice[c];
                }
            }
        }
        let mut combined = runtime(threads);
        let mut actual = [[0.; 2]; 256];
        support::without_heap(|| {
            for id in 1..=128 {
                let (key, velocity) = match id % 3 {
                    0 => (61, 1.),
                    1 => (60, 1.),
                    _ => (60, 0.1),
                };
                combined
                    .trigger(
                        Input {
                            external_id: Some(id),
                            ..input(key)
                        },
                        key,
                        velocity,
                    )
                    .unwrap();
            }
            combined.render(&mut actual).unwrap();
        });
        let mismatch = actual
            .iter()
            .zip(expected)
            .enumerate()
            .find(|(_, (a, b))| **a != *b);
        assert!(
            mismatch.is_none(),
            "each voice keeps its cutoff, including the unmodulated sibling, threads={threads}, first mismatch={mismatch:?}"
        );
        if threads > 1 {
            assert!(combined.parallel_blocks() > 0);
        }
    }
}

#[test]
fn registered_peak_gain_keeps_a_flat_band_and_drives_real_voice_audio() {
    /// Centre gain in true dB per displayed dB of the native legacy EQ.
    const GAIN_SCALE: f64 = 2.006_866_648_250_223;
    let instrument = ir::Instrument {
        source_indices: ir::SourceIndices {
            control_aliases: vec![ir::SourceControlAlias {
                control: ir::ControlRef(0),
                parameter: "ENGINE_PAR_GAIN2".into(),
                address: ir::SlotAddress {
                    group: 0,
                    slot: 5,
                    generic: -1,
                },
            }],
            ..Default::default()
        },
        assets: vec![asset("EQ owner witness")],
        controls: vec![ir::Control {
            key: "eq/physical-slot-5/band-2/gain".into(),
            label: "Band 2 Gain".into(),
            value: ir::ControlValue::Continuous {
                min: -18.,
                max: 18.,
                default: 0.,
                unit: ir::ControlUnit::Decibels,
            },
            automation: ir::Automation::None,
        }],
        chains: vec![ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Eq(ir::fx::eq::Config::band(ir::fx::eq::Band {
                law: ir::fx::eq::Law::KontaktLegacy,
                frequency: sampler_core::KontaktLegacyEqSettings::frequency_knob(1000.),
                q: sampler_core::KontaktLegacyEqSettings::bandwidth_knob(1.),
                gain_db: 0.,
                ..ir::fx::eq::Band::new(ir::fx::eq::Kind::Peak, 0., 0., 0.)
            }))],
            post_amplitude: vec![],
        }],
        processor_controls: vec![ir::ProcessorControl {
            control: ir::ControlRef(0),
            chain: ir::ChainRef(0),
            index: 0,
            parameter: ir::ProcessorParameter::Gain,
            ramp: ir::Time::ZERO,
        }],
        modulators: vec![ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Velocity,
        }],
        routes: vec![ir::Route {
            source: ir::ModulatorRef(0),
            target: ir::Target::Control(ir::ControlRef(0)),
            depth: ir::Depth::Normalized(1. / 3.),
            invert: false,
            shape: None,
            smoothing: ir::Time::ZERO,
            scale: None,
        }],
        zones: vec![ir::Zone {
            chain: Some(ir::ChainRef(0)),
            routes: vec![ir::RouteRef(0)],
            keys: ir::KeyRange { low: 60, high: 60 },
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        ..Default::default()
    };
    let mut phantom = instrument.clone();
    phantom.processor_controls.clear();
    assert!(
        phantom.validate().is_err(),
        "an alias cannot publish an unbound mirror"
    );
    let mut layer_alias = instrument.clone();
    layer_alias.source_indices.control_aliases[0].address.slot = -1;
    assert!(
        layer_alias.validate().is_err(),
        "module aliases cannot shadow layer parameter dispatch"
    );
    let mut duplicate = instrument.clone();
    duplicate
        .source_indices
        .control_aliases
        .push(duplicate.source_indices.control_aliases[0].clone());
    let pcm = Pcm::new(48000, vec![[0.; 2]; 4800].into_boxed_slice()).unwrap();
    assert!(
        lower(&duplicate, 48000, vec![pcm], no_behaviors).is_err(),
        "native address aliases cannot conflict"
    );
    let frames: Vec<_> = (0..4800)
        .map(|n| [0.01 * (std::f32::consts::TAU * n as f32 / 48.).sin(); 2])
        .collect();
    let pcm = Pcm::new(48000, frames.clone().into_boxed_slice()).unwrap();
    let prepared = lower(&instrument, 48000, vec![pcm], no_behaviors)
        .expect("a flat peak band must retain its real gain owner before modulation");
    let native = sampler_core::EngineParameterAddress {
        parameter: sampler_core::engine_parameter_id("ENGINE_PAR_GAIN2").unwrap(),
        group: 0,
        slot: 5,
        generic: -1,
    };
    assert!(
        prepared
            .parameter_registry()
            .resolve_native(native)
            .is_some(),
        "native alias must resolve to the real EQ gain owner"
    );
    let descriptor = prepared.parameter_registry().descriptors().next().unwrap();
    assert_eq!(
        (descriptor.unit, descriptor.range, descriptor.default),
        (sampler_core::ParameterUnit::Decibels, [-18., 18.], 0.)
    );
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    let mut output = [[0.; 2]; 4096];
    support::without_heap(|| {
        rt.trigger(input(60), 60, 1.).unwrap();
        rt.render(&mut output).unwrap();
    });
    let power = |frames: &[[f32; 2]]| frames.iter().map(|f| f64::from(f[0]).powi(2)).sum::<f64>();
    let gain_db = 10. * (power(&output[1024..]) / power(&frames[1024..4096])).log10();
    assert!(
        (gain_db - 12. * GAIN_SCALE).abs() < 0.4,
        "center-frequency gain: {gain_db} dB"
    );
    let id = sampler_core::lower::ir_control_id("eq/physical-slot-5/band-2/gain");
    assert_eq!(
        rt.control_base_value(rt.active_plan(), id),
        Ok(sampler_core::ControlValue::Real(0.))
    );
    let mut edited = [[0.; 2]; 512];
    support::without_heap(|| {
        rt.set_engine_parameter(native, 666_667).unwrap();
        rt.render(&mut edited).unwrap();
    });
    let edited_db = 10. * (power(&edited[128..]) / power(&frames[4224..4608])).log10();
    assert!(
        // The native wrapper slews 1% of the distance per 32 frames toward the +36 dB target, so
        // 512 frames only start the climb from the 24 dB the velocity route held.
        edited_db > 12. * GAIN_SCALE + 0.5 && edited_db < 18. * GAIN_SCALE,
        "native base plus held modulation clamps at +18 dB: {edited_db}"
    );
    let Ok(sampler_core::ControlValue::Real(base)) = rt.control_base_value(rt.active_plan(), id)
    else {
        panic!("real EQ base");
    };
    assert!((base - 6.000012).abs() < 1e-12);
}

#[test]
fn registered_peak_frequency_and_width_use_normalized_knob_domains() {
    let controls = ["frequency", "bandwidth"]
        .map(|name| ir::Control {
            key: format!("eq/{name}"),
            label: name.into(),
            value: ir::ControlValue::Continuous {
                min: 0.,
                max: 1.,
                default: if name == "frequency" { 0.3 } else { 0.2 },
                unit: ir::ControlUnit::Percent,
            },
            automation: ir::Automation::None,
        })
        .to_vec();
    let instrument = ir::Instrument {
        assets: vec![asset("EQ knob domains")],
        controls,
        chains: vec![ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Eq(ir::fx::eq::Config::band(ir::fx::eq::Band {
                law: ir::fx::eq::Law::KontaktLegacy,
                frequency: 0.3,
                q: 0.2,
                gain_db: 12.,
                ..ir::fx::eq::Band::new(ir::fx::eq::Kind::Peak, 0., 0., 0.)
            }))],
            post_amplitude: vec![],
        }],
        processor_controls: [
            ir::ProcessorParameter::Cutoff,
            ir::ProcessorParameter::Resonance,
        ]
        .into_iter()
        .enumerate()
        .map(|(control, parameter)| ir::ProcessorControl {
            control: ir::ControlRef(control),
            chain: ir::ChainRef(0),
            index: 0,
            parameter,
            ramp: ir::Time::ZERO,
        })
        .collect(),
        source_indices: ir::SourceIndices {
            control_aliases: ["ENGINE_PAR_FREQ2", "ENGINE_PAR_BW2"]
                .into_iter()
                .enumerate()
                .map(|(control, parameter)| ir::SourceControlAlias {
                    control: ir::ControlRef(control),
                    parameter: parameter.into(),
                    address: ir::SlotAddress {
                        group: 0,
                        slot: 5,
                        generic: -1,
                    },
                })
                .collect(),
            ..Default::default()
        },
        modulators: vec![ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Velocity,
        }],
        routes: [0.4, 0.2]
            .into_iter()
            .enumerate()
            .map(|(control, depth)| ir::Route {
                source: ir::ModulatorRef(0),
                target: ir::Target::Control(ir::ControlRef(control)),
                depth: ir::Depth::Normalized(depth),
                invert: false,
                shape: None,
                smoothing: ir::Time::ZERO,
                scale: None,
            })
            .collect(),
        zones: vec![ir::Zone {
            keys: ir::KeyRange { low: 60, high: 60 },
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            chain: Some(ir::ChainRef(0)),
            routes: vec![ir::RouteRef(0), ir::RouteRef(1)],
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        ..Default::default()
    };
    let hz = sampler_core::KontaktLegacyEqSettings::frequency_hz(0.7, 48000);
    let frames: Vec<_> = (0..10000)
        .map(|n| [0.01 * (std::f32::consts::TAU * hz * n as f32 / 48000.).sin(); 2])
        .collect();
    let pcm = Pcm::new(48000, frames.clone().into_boxed_slice()).unwrap();
    let prepared = lower(&instrument, 48000, vec![pcm.clone()], no_behaviors)
        .expect("both normalized EQ knobs bind actual processor lanes");
    for descriptor in prepared.parameter_registry().descriptors() {
        assert_eq!(
            (descriptor.range, descriptor.unit),
            ([0., 1.], sampler_core::ParameterUnit::Percent)
        );
    }
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    let mut out = [[0.; 2]; 4096];
    support::without_heap(|| {
        rt.trigger(input(60), 60, 1.).unwrap();
        rt.render(&mut out).unwrap();
    });
    let power = |f: &[[f32; 2]]| f.iter().map(|x| f64::from(x[0]).powi(2)).sum::<f64>();
    let boosted_db = 10. * (power(&out[1024..]) / power(&frames[1024..4096])).log10();
    assert!(
        (boosted_db - 12. * 2.006_866_648_250_223).abs() < 0.3,
        "frequency moved in the native knob domain: {boosted_db}"
    );
    let mut edited = [[0.; 2]; 1024];
    support::without_heap(|| {
        for name in ["ENGINE_PAR_FREQ2", "ENGINE_PAR_BW2"] {
            rt.set_engine_parameter(
                sampler_core::EngineParameterAddress {
                    parameter: sampler_core::engine_parameter_id(name).unwrap(),
                    group: 0,
                    slot: 5,
                    generic: -1,
                },
                0,
            )
            .unwrap();
        }
        rt.render(&mut edited).unwrap();
    });
    let edited_db = 10. * (power(&edited[512..]) / power(&frames[4608..5120])).log10();
    assert!(
        edited_db < boosted_db - 5.,
        "native base edits move the same held knobs (slewing at the native cadence): {edited_db}"
    );

    let mut width_outputs = [[[0.; 2]; 1024]; 2];
    support::without_heap(|| {
        rt.set_engine_parameter(
            sampler_core::EngineParameterAddress {
                parameter: sampler_core::engine_parameter_id("ENGINE_PAR_FREQ2").unwrap(),
                group: 0,
                slot: 5,
                generic: -1,
            },
            400000,
        )
        .unwrap();
        for (n, width) in [0, 800000].into_iter().enumerate() {
            rt.set_engine_parameter(
                sampler_core::EngineParameterAddress {
                    parameter: sampler_core::engine_parameter_id("ENGINE_PAR_BW2").unwrap(),
                    group: 0,
                    slot: 5,
                    generic: -1,
                },
                width,
            )
            .unwrap();
            rt.render(&mut width_outputs[n]).unwrap();
        }
    });
    let width_db =
        10. * (power(&width_outputs[1][512..]) / power(&width_outputs[0][512..])).log10();
    assert!(
        width_db > 2.,
        "wider bell boosts the off-center sine: {width_db}"
    );

    let mut runtimes: Vec<_> = (0..3)
        .map(|_| {
            Runtime::new(
                lower(&instrument, 48000, vec![pcm.clone()], no_behaviors).unwrap(),
                limits(),
            )
            .unwrap()
        })
        .collect();
    let mut separate = [[[0.; 2]; 2048]; 2];
    let mut mixed = [[0.; 2]; 2048];
    support::without_heap(|| {
        for (n, velocity) in [0.25, 0.75].into_iter().enumerate() {
            runtimes[n].trigger(input(60), 60, velocity).unwrap();
            runtimes[n].render(&mut separate[n]).unwrap();
            runtimes[2].trigger(input(60), 60, velocity).unwrap();
        }
        runtimes[2].render(&mut mixed).unwrap();
    });
    for ((actual, first), second) in mixed.iter().zip(&separate[0]).zip(&separate[1]) {
        for c in 0..2 {
            assert!(
                (actual[c] - first[c] - second[c]).abs() < 2e-8,
                "EQ knob coefficients belong to each voice"
            );
        }
    }
}

#[test]
fn compressor_descriptors_have_live_typed_physical_lanes() {
    use sampler_core::{ParameterRole, ParameterUnit};
    let mut i = instrument();
    i.sequences.clear();
    i.zones = vec![ir::Zone {
        chain: Some(ir::ChainRef(0)),
        pitch: ir::KeyTracking::Fixed,
        velocity: ir::VelocityResponse::None,
        ..ir::Zone::new(ir::AssetRef(0))
    }];
    i.assets.truncate(1);
    i.chains = vec![ir::Chain {
        scope: ir::Scope::Voice,
        pre_amplitude: vec![],
        post_amplitude: vec![ir::Processor::Dynamics(ir::fx::dynamics::Config::generic(
            -12.,
            4.,
            ir::Time::Seconds(0.01),
            ir::Time::Seconds(0.1),
        ))],
    }];
    i.register_compressor_controls();
    i.register_compressor_controls(); // Re-registration preserves identities.
    assert_eq!(i.controls.len(), 4);
    let plan = lower(&i, 48000, vec![constant(0.8)], no_behaviors).unwrap();
    let lanes: Vec<_> = plan.parameter_registry().descriptors().cloned().collect();
    assert_eq!(lanes.len(), 4);
    for (lane, role, unit, default) in [
        (
            &lanes[0],
            ParameterRole::Threshold,
            ParameterUnit::Decibels,
            -12.,
        ),
        (&lanes[1], ParameterRole::Ratio, ParameterUnit::Linear, 4.),
        (
            &lanes[2],
            ParameterRole::Attack,
            ParameterUnit::Seconds,
            0.01,
        ),
        (
            &lanes[3],
            ParameterRole::Release,
            ParameterUnit::Seconds,
            0.1,
        ),
    ] {
        assert_eq!(lane.role, role);
        assert_eq!(lane.unit, unit);
        assert_eq!(lane.default, default);
    }
    // Each physical edit changes the envelope, including release after a quiet section.
    let render = |edited: Option<usize>| {
        let pcm = Pcm::new(
            48000,
            (0..24000)
                .map(|n| [if n < 12000 { 0.8 } else { 0.08 }; 2])
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
        .unwrap();
        let plan = lower(&i, 48000, vec![pcm], no_behaviors).unwrap();
        let mut rt = Runtime::new(plan, limits()).unwrap();
        let mut out = vec![[0.; 2]; 24000];
        support::without_heap(|| {
            rt.trigger(input(60), 60, 1.).unwrap();
            for block in out[..1024].chunks_mut(37) {
                rt.render(block).unwrap();
            }
            if let Some(n) = edited {
                rt.edit_controls(
                    rt.active_plan(),
                    None,
                    &[sampler_core::ControlWrite {
                        id: lanes[n].control,
                        value: sampler_core::ControlValue::Real([0., 1., 0.2, 1.][n]),
                    }],
                )
                .unwrap();
            }
            for block in out[1024..].chunks_mut(37) {
                rt.render(block).unwrap();
            }
        });
        assert_eq!(rt.stats().nonfinite_frames, 0);
        out
    };
    let baseline = render(None);
    for n in 0..4 {
        let edited = render(Some(n));
        assert_eq!(baseline[..1024], edited[..1024]);
        let difference: f64 = baseline
            .iter()
            .flatten()
            .zip(edited.iter().flatten())
            .map(|(a, b)| f64::from(a - b).powi(2))
            .sum();
        assert!(
            difference > 1e-3,
            "compressor lane {n} is inert: {difference}"
        );
    }
}

#[test]
fn authored_zone_getters_keep_physical_group_holes_after_lowering() {
    use sampler_core::ZoneParameter;
    let mut instrument = ir::Instrument {
        assets: vec![asset("tone")],
        groups: vec![ir::Group::default()],
        zones: vec![ir::Zone {
            group: Some(ir::GroupRef(0)),
            keys: ir::KeyRange { low: 12, high: 94 },
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        ..Default::default()
    };
    instrument.source_indices.groups = vec![None; 8];
    instrument.source_indices.groups[7] = Some(ir::GroupRef(0));
    instrument.source_indices.zones = vec![None; 73];
    instrument.source_indices.zones[72] = Some(ir::ZoneRef(0));
    instrument.source_indices.zone_catalog = (0..73).map(|i| ir::SourceZone {
        id: (i == 72).then_some(73),
        home: ir::SourceZoneHome { group: 7, low_key: 12, high_key: 94 },
    }).collect();
    let plan = lower(&instrument, 48000, vec![constant(1.)], no_behaviors).unwrap();
    assert_eq!(
        plan.source_zone_parameter(73, ZoneParameter::Group),
        Some(7)
    );
    assert_eq!(
        plan.source_zone_parameter(73, ZoneParameter::LowKey),
        Some(12)
    );
    assert_eq!(
        plan.source_zone_parameter(73, ZoneParameter::HighKey),
        Some(94)
    );
    assert_eq!(plan.source_zone_parameter(1, ZoneParameter::Group), None);
}


#[test]
fn saved_digital_fade_plays_native_f32_growth_and_terminal_step_without_heap() {
    // Shared modulation points are still on its 64-frame grid; the saved
    // source fade must sample the native 32-frame recurrence at those points.
    for rate in [44100, 48000, 96000] {
        for fade_ms in [0., f32::from_bits(0x3f24ca3c), 10.] {
            let ticks = (fade_ms * (rate as f32 / 32.) * 0.001) as usize;
            let mut value = 0.3f32;
            let factor = if ticks == 0 {
                1.
            } else {
                (1. + 1. / f64::from(0.3f32)).powf(1. / ticks as f64) as f32
            };
            let mut gains = vec![];
            for _ in 0..ticks {
                gains.push(value - 0.3);
                value = (value * factor).clamp(0., 1.);
            }
            gains.push(1.);
            for block in [1, 7, 32, 61, 256] {
                let instrument = ir::Instrument {
                    assets: vec![asset("a")],
                    zones: vec![ir::Zone {
                        keys: ir::KeyRange { low: 60, high: 60 },
                        pitch: ir::KeyTracking::Fixed,
                        velocity: ir::VelocityResponse::None,
                        routes: vec![ir::RouteRef(0)],
                        ..ir::Zone::new(ir::AssetRef(0))
                    }],
                    modulators: vec![ir::Modulator {
                        scope: ir::Scope::Voice,
                        source: ir::ModulationSource::Lfo(ir::Lfo {
                            shape: ir::LfoShape::DigitalSine {
                                level: -f64::from(0.3f32),
                                fade_ms,
                            },
                            rate: ir::Frequency::Hertz(1e-10),
                            delay: ir::Time::ZERO,
                            fade_in: ir::Time::ZERO,
                            phase: 0.25,
                            retrigger: true,
                        }),
                    }],
                    routes: vec![ir::Route::new(
                        ir::ModulatorRef(0),
                        ir::Target::Amplitude,
                        ir::Depth::Normalized(1.),
                    )],
                    ..Default::default()
                };
                let plan = lower(
                    &instrument,
                    rate,
                    vec![Pcm::new(rate, vec![[0.5; 2]; 4800].into_boxed_slice()).unwrap()],
                    no_behaviors,
                )
                .unwrap();
                let mut rt = Runtime::new(plan, limits()).unwrap();
                for _ in 0..2 {
                    let mut audio = [[0.; 2]; 1024];
                    support::without_heap(|| {
                        rt.trigger(input(60), 60, 1.).unwrap();
                        for chunk in audio.chunks_mut(block) {
                            rt.render(chunk).unwrap();
                        }
                        rt.note_off(input(60), None).unwrap();
                        rt.render(&mut [[0.; 2]; 128]).unwrap();
                    });
                    assert_eq!(rt.voice_count(), 0);
                    for (i, frame) in audio.iter().enumerate() {
                        let begin = i / 64 * 64;
                        let at = |frame: usize| {
                            0.5 * ((-0.3f32 * gains[(frame / 32).min(ticks)]) + 1.) * 0.5
                        };
                        let a = at(begin);
                        let b = at(begin + 64);
                        let expected = a + (b - a) * ((i % 64 + 1) as f32 / 64.);
                        assert!(
                            frame.iter().all(|v| (*v - expected).abs() < 1e-6),
                            "rate={rate} fade={fade_ms} block={block} frame={i} actual={frame:?} native={expected}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn saved_digital_fade_rejects_unproven_clock_state_at_ir_boundary() {
    let valid = ir::Lfo {
        shape: ir::LfoShape::DigitalSine {
            level: -0.3,
            fade_ms: 10.,
        },
        rate: ir::Frequency::Hertz(1.),
        phase: 0.25,
        delay: ir::Time::ZERO,
        fade_in: ir::Time::ZERO,
        retrigger: true,
    };
    let instrument = |lfo| ir::Instrument {
        modulators: vec![ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Lfo(lfo),
        }],
        ..Default::default()
    };
    assert!(instrument(valid).validate().is_ok());
    let mut shared = instrument(valid);
    shared.modulators[0].scope = ir::Scope::Master;
    assert!(shared.validate().is_err());
    for invalid in [
        ir::Lfo {
            delay: ir::Time::Milliseconds(1.),
            ..valid
        },
        ir::Lfo {
            fade_in: ir::Time::Milliseconds(1.),
            ..valid
        },
        ir::Lfo {
            retrigger: false,
            ..valid
        },
        ir::Lfo {
            rate: ir::Frequency::Beats(1.),
            ..valid
        },
        ir::Lfo {
            shape: ir::LfoShape::DigitalSine {
                level: -0.3,
                fade_ms: f32::NAN,
            },
            ..valid
        },
        ir::Lfo {
            shape: ir::LfoShape::DigitalSine {
                level: -0.3,
                fade_ms: 5001.,
            },
            ..valid
        },
    ] {
        let ir = instrument(invalid);
        assert!(ir.validate().is_err());
        assert!(lower(&ir, 48000, vec![], no_behaviors).is_err());
    }
}

/// A routed (non-amplitude) Kontakt envelope's stages are engine lanes at its
/// saved slot; a voice reads them when it starts (ANALOG STRINGS ENV_Filter 88/2).
#[test]
fn modulation_envelope_stages_are_engine_lanes_read_at_voice_start() {
    let mut ir = ir::Instrument {
        assets: vec![asset("mod-env")],
        groups: vec![ir::Group::default()],
        zones: vec![ir::Zone {
            group: Some(ir::GroupRef(0)),
            routes: vec![ir::RouteRef(0)],
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        modulators: vec![ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Envelope(ir::Envelope {
                attack: ir::Time::ZERO,
                decay: ir::Time::Milliseconds(1.),
                sustain: 0.,
                ..Default::default()
            }),
        }],
        routes: vec![ir::Route {
            source: ir::ModulatorRef(0),
            target: ir::Target::Amplitude,
            depth: ir::Depth::Gain(ir::Gain::Decibels(-40.)),
            invert: false,
            shape: None,
            smoothing: ir::Time::ZERO,
            scale: None,
        }],
        ..Default::default()
    };
    ir.source_indices.modulators.push(ir::SourceModulator {
        group: 0, slot: 2, external: false, name: "ENV_Filter".into(),
        runtime: Some(ir::ModulatorRef(0)), settings: None,
    });
    ir.source_indices.ahdsrs.push(ir::SourceAhdsr {
        group: 0, slot: 2, attack_ms: 0., attack_curve: 0., hold_ms: 0., decay_ms: 1.,
        sustain: 0., release_ms: 0., ahd_only: false, native_amplitude: false,
    });
    let sustain = sampler_core::EngineParameterAddress {
        parameter: sampler_core::engine_parameter_id("ENGINE_PAR_SUSTAIN").unwrap(),
        group: 0, slot: 2, generic: -1,
    };
    let level = |write: Option<i32>| {
        let mut rt = Runtime::new(lower(&ir, 48000, vec![constant(1.)], no_behaviors).unwrap(), limits()).unwrap();
        if let Some(value) = write {
            rt.set_engine_parameter(sustain, value).unwrap();
        }
        rt.trigger(input(60), 60, 1.).unwrap();
        let mut out = [[0.; 2]; 512];
        rt.render(&mut out).unwrap();
        out[511][0]
    };
    let (authored, held) = (level(None), level(Some(1_000_000)));
    // The envelope sits at sustain after 1 ms: 0 applies none of the route's
    // -40 dB, sustain 1 all of it.
    assert!((authored / held).log10() * 20. > 30., "authored {authored}, sustain 1 {held}");
}

/// A native Multi LFO with a saved fade starts silent and reaches full depth
/// on the native fade law (ANALOG STRINGS LFO_P2, 98.8 ms).
#[test]
fn native_multi_lfo_fade_runs_the_saved_fade_law() {
    let wave = ir::NativeWave::Multi { weights: [1., 0., 0., 0., 0.], width: 0.5, normalize: true, digital: false };
    let level = |shape: ir::LfoShape| {
        let ir = ir::Instrument {
            assets: vec![asset("multi-fade")],
            groups: vec![ir::Group::default()],
            zones: vec![ir::Zone {
                group: Some(ir::GroupRef(0)),
                routes: vec![ir::RouteRef(0)],
                pitch: ir::KeyTracking::Fixed,
                velocity: ir::VelocityResponse::None,
                ..ir::Zone::new(ir::AssetRef(0))
            }],
            modulators: vec![ir::Modulator {
                scope: ir::Scope::Voice,
                source: ir::ModulationSource::Lfo(ir::Lfo {
                    shape,
                    rate: ir::Frequency::Hertz(0.01),
                    delay: ir::Time::ZERO,
                    fade_in: ir::Time::ZERO,
                    phase: 0.25,
                    retrigger: true,
                }),
            }],
            routes: vec![ir::Route {
                source: ir::ModulatorRef(0),
                target: ir::Target::Amplitude,
                depth: ir::Depth::Gain(ir::Gain::Decibels(-20.)),
                invert: false,
                shape: None,
                smoothing: ir::Time::ZERO,
                scale: None,
            }],
            ..Default::default()
        };
        let pcm = Pcm::new(48000, vec![[1.; 2]; 12000].into_boxed_slice()).unwrap();
        let mut rt = Runtime::new(lower(&ir, 48000, vec![pcm], no_behaviors).unwrap(), limits()).unwrap();
        rt.trigger(input(60), 60, 1.).unwrap();
        let mut out = vec![[0.; 2]; 9600];
        for block in out.chunks_mut(128) {
            rt.render(block).unwrap();
        }
        // Depth in dB away from the unmodulated level.
        let db = |x: f32| (20. * x.log10()).abs();
        (db(out[64][0]), db(out[9599][0]))
    };
    let (plain_start, plain_end) = level(ir::LfoShape::Native(wave));
    let (faded_start, faded_end) = level(ir::LfoShape::NativeFaded { wave, fade_ms: 100. });
    assert!(plain_start > 19. && (plain_start - plain_end).abs() < 0.1, "{plain_start} {plain_end}");
    assert!(faded_start < 1., "the fade starts at no depth: {faded_start} dB");
    assert!((faded_end - plain_end).abs() < 0.2, "full depth by 200 ms: {faded_end} vs {plain_end}");
}

fn native_primary_fixture() -> ir::Instrument {
    let mut ir = ir::Instrument {
        assets: vec![asset("native-primary")],
        groups: vec![ir::Group::default()],
        zones: vec![ir::Zone {
            group: Some(ir::GroupRef(0)),
            amplitude: Some(ir::ModulatorRef(0)),
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        modulators: vec![ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Envelope(ir::Envelope {
                attack: ir::Time::Milliseconds(125.01293),
                attack_shape: {
                    let b = f64::from((-20_000f64.ln()).exp() as f32);
                    ir::Curve::Exponential(((1. + b) / b).ln())
                },
                release_shape: ir::Curve::Exponential((3.0f64 / 43.).ln()),
                release: ir::Time::Milliseconds(250.0013),
                ..Default::default()
            }),
        }],
        ..Default::default()
    };
    ir.source_indices.modulators.push(ir::SourceModulator {
        group: 0, slot: 7, external: false, name: "primary".into(),
        runtime: Some(ir::ModulatorRef(0)), settings: None,
    });
    ir.source_indices.ahdsrs.push(ir::SourceAhdsr {
        group: 0, slot: 7, attack_ms: 125.01293, attack_curve: -1.,
        hold_ms: 0., decay_ms: 0., sustain: 1., release_ms: 250.0013,
        ahd_only: false, native_amplitude: true,
    });
    ir
}

#[test]
fn lowered_native_primary_runs_the_pinned_evaluator_in_production_voice() {
    use sampler_core::v1_voice_controls::{Ahdsr, ControlDescription, ControlPlan, ControlState};
    for processed in [false, true] {
        let mut ir = native_primary_fixture();
        if processed {
            ir.chains.push(ir::Chain {
                scope: ir::Scope::Voice,
                post_amplitude: vec![ir::Processor::Gain(ir::Gain::Linear(1.))],
                pre_amplitude: vec![]
            });
            ir.zones[0].chain = Some(ir::ChainRef(0));
        }
        let plan = ControlPlan::prepare(ControlDescription {
            amplitude: Ahdsr::from(&ir.source_indices.ahdsrs[0]),
            native_amplitude: true, ..Default::default()
        }, 48000.).unwrap();
        let mut expected = ControlState::new(&plan);
        let mut rt = Runtime::new(lower(&ir, 48000, vec![constant(1.)], no_behaviors).unwrap(), limits()).unwrap();
        let mut output = [[0.; 2]; 128];
        let mut amp = [0.; 128];
        let mut positions = [0; 128];
        for reuse in 0..2 {
            expected = if reuse == 0 { expected } else { ControlState::new(&plan) };
            support::without_heap(|| { rt.trigger(input(60), 60, 1.).unwrap(); });
            for (block, n) in [1, 7, 31, 32, 33, 64, 17, 3].into_iter().enumerate() {
                output.fill([0.; 2]);
                if block == 5 {
                    expected.release(&plan);
                    support::without_heap(|| { rt.note_off(input(60), None).unwrap(); });
                }
                expected.render(&plan, 120., 1., &mut amp[..n], None, None, &mut positions[..n]).unwrap();
                support::without_heap(|| rt.render(&mut output[..n]).unwrap());
                for (i, (frame, value)) in output[..n].iter().zip(&amp).enumerate() {
                    assert_eq!(frame.map(f32::to_bits), [value.to_bits(); 2], "processed={processed} reuse={reuse} block={block} frame={i}");
                }
            }
            rt.panic();
        }
    }
}


#[test]
fn native_primary_requires_unique_physical_identity_and_rejects_bad_coefficients() {
    let render = |ir: &ir::Instrument| {
        let mut rt = Runtime::new(lower(ir, 48000, vec![constant(1.)], no_behaviors).unwrap(), limits()).unwrap();
        rt.trigger(input(60), 60, 1.).unwrap();
        let mut out = [[0.; 2]; 64];
        rt.render(&mut out).unwrap();
        out
    };
    let mut generic = native_primary_fixture();
    generic.source_indices.ahdsrs.clear();
    let expected = render(&generic);
    for case in 0..4 {
        let mut ir = native_primary_fixture();
        match case {
            0 => ir.source_indices.ahdsrs[0].slot = 8,
            1 => ir.source_indices.modulators[0].external = true,
            2 => ir.source_indices.modulators.push(ir.source_indices.modulators[0].clone()),
            _ => ir.source_indices.ahdsrs.push(ir.source_indices.ahdsrs[0]),
        }
        if case == 2 {
            assert!(lower(&ir, 48000, vec![constant(1.)], no_behaviors).is_err(), "duplicate physical owner is invalid");
        } else {
            assert_eq!(render(&ir), expected, "case={case}");
        }
    }
    let mut malformed = native_primary_fixture();
    malformed.source_indices.ahdsrs[0].attack_curve = f32::NAN;
    assert!(lower(&malformed, 48000, vec![constant(1.)], no_behaviors).is_err());
}

// V1's eight-frame affine kernel rounds at fragment boundaries. Match the existing 64-frame control grid.
fn render_v1_on_core_grid(state: &mut sampler_core::v1_voice_controls::ControlState,
    plan: &sampler_core::v1_voice_controls::ControlPlan, now: u64,
    amplitude: &mut [f32], mut flex: Option<&mut [f32]>) {
    let mut offset = 0;
    while offset < amplitude.len() {
        let n = (64 - ((now as usize + offset) % 64)).min(amplitude.len() - offset);
        let mut positions = [0;64];
        state.render(plan,120.,1.,&mut amplitude[offset..offset+n],
            flex.as_deref_mut().map(|f| &mut f[offset..offset+n]),None,&mut positions[..n]).unwrap();
        offset += n;
    }
}

#[test]
fn lowered_whole_voice_uses_v1_ordinary_amplitude_clock() {
    use sampler_core::v1_voice_controls::{Ahdsr, ControlDescription, ControlPlan, ControlState};
    let mut ir = native_primary_fixture();
    ir.source_indices.ahdsrs[0].native_amplitude = false;
    let plan = ControlPlan::prepare(ControlDescription {
        amplitude: Ahdsr::from(&ir.source_indices.ahdsrs[0]),
        native_amplitude: false, ..Default::default()
    }, 48000.).unwrap();
    let mut oracle = ControlState::new(&plan);
    let mut rt = Runtime::new(lower(&ir, 48000, vec![constant(1.)], no_behaviors).unwrap(), limits()).unwrap();
    support::without_heap(|| { rt.trigger(input(60), 60, 1.).unwrap(); });
    let mut output = [[0.; 2]; 64];
    let mut amp = [0.; 64];
    for (block, n) in [1, 7, 31, 32, 33, 64, 17, 3].into_iter().enumerate() {
        if block == 5 {
            oracle.release(&plan);
            support::without_heap(|| { rt.note_off(input(60), None).unwrap(); });
        }
        render_v1_on_core_grid(&mut oracle,&plan,rt.now(),&mut amp[..n],None);
        support::without_heap(|| rt.render(&mut output[..n]).unwrap());
        for (i, (frame, value)) in output[..n].iter().zip(&amp).enumerate() {
            assert_eq!(frame.map(f32::to_bits), [value.to_bits(); 2], "whole voice block={block} frame={i}");
        }
    }
}


fn original_volume_target(intensity: f32) -> ir::kontakt::ModulationTarget {
    ir::kontakt::ModulationTarget {
        param: "volume".into(),
        intensity,
        lag_ms: 0,
        name: String::new(),
        slot: None,
        invert: false,
        shaper: None,
        unknown_i16: 0,
        flags: 0,
    }
}

#[test]
fn lowered_whole_voice_flex_is_bit_exact_through_dsp_release_and_reuse() {
    use sampler_core::v1_voice_controls::{
        Ahdsr, ControlDescription, ControlPlan, ControlState, Flex,
    };
    let points = vec![
        ir::kontakt::FlexPoint {
            time_ms: 12.5,
            level: 0.8,
            curve: 0.25,
        },
        ir::kontakt::FlexPoint {
            time_ms: 2.5,
            level: 0.3,
            curve: 0.75,
        },
        ir::kontakt::FlexPoint {
            time_ms: 3.0,
            level: 0.0,
            curve: 0.5,
        },
    ];
    for (processed, primary) in [(false,true),(true,true),(false,false),(true,false)] {
        let mut ir = native_primary_fixture();
        ir.source_indices.ahdsrs[0].native_amplitude = false;
        ir.modulators.push(ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Breakpoints(ir::Breakpoints {
                points: points
                    .iter()
                    .map(|p| ir::Breakpoint {
                        time: ir::Time::Milliseconds(f64::from(p.time_ms)),
                        level: f64::from(p.level),
                        shape: { let c = f64::from(p.curve) - 0.5; if c.abs() < 1e-4 { ir::Curve::Linear } else { ir::Curve::Exponential(-17. * c) } },
                    })
                    .collect(),
                sustain: Some(1),
                loop_range: None,
                flex_release: false,
            }),
        });
        ir.routes.push(ir::Route::new(
            ir::ModulatorRef(1),
            ir::Target::Amplitude,
            ir::Depth::Normalized(1.),
        ));
        ir.zones[0].routes.push(ir::RouteRef(0));
        ir.source_indices.modulators.push(ir::SourceModulator {
            group: 0,
            slot: 9,
            external: false,
            name: "flex".into(),
            runtime: Some(ir::ModulatorRef(1)),
            settings: Some(ir::kontakt::Modulation {
                version: 1,
                source: ir::kontakt::ModulationSource::Internal {
                    flags: [0; 4],
                    unknown_id: 0,
                    source: ir::kontakt::InternalSource::Flex {
                        points: points.clone(),
                        sustain: 1,
                        unknown_index: 0,
                        unknown_tail: vec![],
                    },
                },
                targets: vec![original_volume_target(1.)],
            }),
        });
        if processed {
            ir.chains.push(ir::Chain {
                scope: ir::Scope::Voice,
                pre_amplitude: vec![],
                post_amplitude: vec![ir::Processor::Gain(ir::Gain::Linear(1.))],
            });
            ir.zones[0].chain = Some(ir::ChainRef(0));
        }
        let amplitude = if primary { Ahdsr::from(&ir.source_indices.ahdsrs[0]) } else { Ahdsr::UNITY };
        if !primary {
            ir.modulators[0].source = ir::ModulationSource::Envelope(ir::Envelope {
                release: ir::Time::Milliseconds(3.), release_shape: ir::Curve::Step, ..Default::default()
            });
            ir.source_indices.modulators.remove(0);
            ir.source_indices.ahdsrs.clear();
        }
        let plan = ControlPlan::prepare(
            ControlDescription {
                amplitude,
                native_amplitude: false,
                flex: Some(Flex::from_kontakt(&points, 1).unwrap()),
                ..Default::default()
            },
            48000.,
        )
        .unwrap();
        let mut rt = Runtime::new(
            lower(&ir, 48000, vec![constant(1.)], no_behaviors).unwrap(),
            limits(),
        )
        .unwrap();
        for reuse in 0..2 {
            let mut oracle = ControlState::new(&plan);
            support::without_heap(|| {
                rt.trigger(input(60), 60, 1.).unwrap();
            });
            for (block, n) in [1, 7, 31, 32, 33, 64, 17, 3, 64, 64, 64]
                .into_iter()
                .enumerate()
            {
                if block == 5 {
                    oracle.release(&plan);
                    support::without_heap(|| {
                        rt.note_off(input(60), None).unwrap();
                    });
                }
                let mut out = [[0.; 2]; 64];
                let mut amp = [0.; 64];
                let mut flex = [0.; 64];
                render_v1_on_core_grid(&mut oracle,&plan,rt.now(),&mut amp[..n],Some(&mut flex[..n]));
                support::without_heap(|| rt.render(&mut out[..n]).unwrap());
                for i in 0..n {
                    assert_eq!(
                        out[i].map(f32::to_bits),
                        [(amp[i] * flex[i]).to_bits(); 2],
                        "processed={processed} primary={primary} reuse={reuse} block={block} frame={i}"
                    );
                }
            }
            support::without_heap(|| rt.panic());
        }
        assert_eq!(rt.stats().v1_control_starts, 2);
        assert!(rt.stats().v1_control_blocks > 0);
    }
}

#[test]
fn lowered_whole_voice_external_uses_current_depth_and_full_resolution_cc() {
    let mut ir = native_primary_fixture();
    ir.source_indices.ahdsrs[0].attack_ms = 0.;
    ir.source_indices.ahdsrs[0].release_ms = 0.;
    ir.source_indices.ahdsrs[0].native_amplitude = false;
    ir.modulators[0].source = ir::ModulationSource::Envelope(Default::default());
    ir.modulators.push(ir::Modulator {
        scope: ir::Scope::Voice,
        source: ir::ModulationSource::Controller(11),
    });
    ir.routes.push(ir::Route::new(
        ir::ModulatorRef(1),
        ir::Target::Amplitude,
        ir::Depth::Normalized(f64::from(0.4f32)),
    ));
    ir.zones[0].routes.push(ir::RouteRef(0));
    ir.source_indices.modulators.push(ir::SourceModulator {
        group: 0,
        slot: 3,
        external: true,
        name: "expression".into(),
        runtime: Some(ir::ModulatorRef(1)),
        settings: Some(ir::kontakt::Modulation {
            version: 1,
            source: ir::kontakt::ModulationSource::External {
                source: ir::kontakt::ExternalSource::MidiCc(11),
                unknown_id: 0,
                unknown_source_data: vec![],
                unknown_tail: vec![],
            },
            targets: vec![original_volume_target(0.7)],
        }),
    });
    for raw in [0, u32::MAX, 0x2345_6789] {
        let mut rt = Runtime::new(
            lower(&ir, 48000, vec![constant(1.)], no_behaviors).unwrap(),
            limits(),
        )
        .unwrap();
        let performance = rt.performance(0).unwrap();
        let mut out = [[0.; 2]; 64];
        support::without_heap(|| {
            rt.set_controller(performance, 11, raw).unwrap();
            rt.trigger(input(60), 60, 1.).unwrap();
            rt.render(&mut out).unwrap();
        });
        let value = (f64::from(raw) / f64::from(u32::MAX)) as f32;
        let expected = 1. - 0.4f32 * (1. - value);
        assert!(
            out.iter()
                .all(|f| f.map(f32::to_bits) == [expected.to_bits(); 2]),
            "raw={raw} first={:?}",
            out[0]
        );
        assert_eq!(rt.stats().v1_control_starts, 1);
    }
    // An unrepresentable live depth cannot be substituted with the saved 0.7.
    ir.routes[0].depth = ir::Depth::Normalized(0.40000000001);
    let mut rt = Runtime::new(
        lower(&ir, 48000, vec![constant(1.)], no_behaviors).unwrap(),
        limits(),
    )
    .unwrap();
    rt.trigger(input(60), 60, 1.).unwrap();
    rt.render(&mut [[0.; 2]; 64]).unwrap();
    assert_eq!(rt.stats().v1_control_starts, 0);
}


#[test]
fn lowered_whole_voice_state_survives_pool_growth_on_serial_and_parallel_paths() {
    for threads in [1,2] {
        let mut ir = native_primary_fixture();
        ir.source_indices.ahdsrs[0].native_amplitude = false;
        let make = |voices| {
            let mut sizes = limits(); sizes.voices = voices;
            sizes.notes = 64; sizes.families = 64; sizes.expressions = 64;
            let plan = lower(&ir,48000,vec![constant(1.)],no_behaviors).unwrap();
            let (rt,control) = Runtime::with_plan_updates(plan,sizes,2,1).unwrap();
            (rt.with_threads(sampler_core::Threads::Fixed(threads)),control)
        };
        let (mut reference,_reference_control) = make(64);
        let (mut actual,mut control) = make(1);
        support::without_heap(|| {
            reference.trigger(input(60),60,1.).unwrap(); actual.trigger(input(60),60,1.).unwrap();
        });
        for block in 0..8 {
            if block == 2 { assert_eq!(control.grow_voices(64),Ok(64)); }
            if block == 3 {
                support::without_heap(|| {
                    for id in 1..64 {
                        let note = Input { external_id: Some(id), ..input(60) };
                        reference.trigger(note,60,1.).unwrap(); actual.trigger(note,60,1.).unwrap();
                    }
                });
            }
            if block == 4 {
                support::without_heap(|| {
                    reference.note_off(input(60),None).unwrap(); actual.note_off(input(60),None).unwrap();
                });
            }
            let mut expected = [[0.;2];37]; let mut output = [[0.;2];37];
            support::without_heap(|| { reference.render(&mut expected).unwrap(); actual.render(&mut output).unwrap(); });
            assert_eq!(output,expected,"threads={threads} block={block}");
        }
        assert_eq!(actual.stats().voice_growths,1);
        assert_eq!(actual.stats().v1_control_starts,64);
        if threads == 2 { assert!(actual.parallel_blocks() > 0); }
        assert!(actual.stats().v1_control_blocks > 0);
    }
}

fn controlled_native_external_fixture() -> ir::Instrument {
    let mut ir = native_primary_fixture();
    ir.source_indices.ahdsrs[0].attack_ms = 0.;
    ir.source_indices.ahdsrs[0].release_ms = 0.;
    ir.source_indices.ahdsrs[0].native_amplitude = false;
    ir.modulators[0].source = ir::ModulationSource::Envelope(Default::default());
    ir.modulators.push(ir::Modulator {
        scope: ir::Scope::Voice,
        source: ir::ModulationSource::Controller(11),
    });
    ir.routes.push(ir::Route::new(
        ir::ModulatorRef(1),
        ir::Target::Amplitude,
        ir::Depth::Normalized(f64::from(0.4f32)),
    ));
    ir.zones[0].routes.push(ir::RouteRef(0));
    ir.source_indices.modulators.push(ir::SourceModulator {
        group: 0,
        slot: 3,
        external: true,
        name: "expression".into(),
        runtime: Some(ir::ModulatorRef(1)),
        settings: Some(ir::kontakt::Modulation {
            version: 1,
            source: ir::kontakt::ModulationSource::External {
                source: ir::kontakt::ExternalSource::MidiCc(11),
                unknown_id: 0,
                unknown_source_data: vec![],
                unknown_tail: vec![],
            },
            targets: vec![original_volume_target(0.7)],
        }),
    });
    ir.controls = ["external-depth", "external-bypass"].into_iter().enumerate().map(|(i,key)| ir::Control {
        key: key.into(), label: key.into(),
        value: ir::ControlValue::Continuous { min: 0., max: 1., default: if i == 0 { f64::from(0.4f32) } else { 0. }, unit: ir::ControlUnit::None },
        automation: ir::Automation::None,
    }).collect();
    ir.source_indices.route_controls.push((ir::RouteRef(0), ir::ControlRef(0), ir::ControlRef(1)));
    ir
}

/// Lowering precomputes which route controls a zone uses (it was a per-alias
/// routes x zones scan, ~90 s on ANALOG STRINGS); the binding rule is unchanged.
#[test]
fn source_alias_binds_only_controls_of_routes_a_zone_uses() {
    let mut used = controlled_native_external_fixture();
    used.source_indices.control_aliases.push(ir::SourceControlAlias {
        control: ir::ControlRef(0),
        parameter: "ENGINE_PAR_GAIN2".into(),
        address: ir::SlotAddress { group: 0, slot: 5, generic: -1 },
    });
    let mut unused = used.clone();
    unused.zones[0].routes.clear();
    let depth = sampler_core::lower::ir_control_id("external-depth");
    let bindings = |ir: &ir::Instrument| {
        lower(ir, 48000, vec![constant(1.)], no_behaviors).unwrap()
            .engine_parameter_bindings().iter().filter(|b| b.control == depth).count()
    };
    assert_eq!(bindings(&used), 1);
    assert_eq!(bindings(&unused), 0);
}

#[test]
fn native_external_reads_live_depth_and_bypass_without_resetting_held_source() {
    use sampler_core::{ControlValue,ControlWrite};
    let ir = controlled_native_external_fixture();
    for threads in [1,2] {
        let mut rt = Runtime::new(lower(&ir,48000,vec![constant(1.)],no_behaviors).unwrap(),limits()).unwrap()
            .with_threads(sampler_core::Threads::Fixed(threads));
        let plan = rt.active_plan();
        let performance = rt.performance(0).unwrap();
        support::without_heap(|| rt.set_controller(performance,11,0).unwrap());
        let mut note = None;
        support::without_heap(|| { note = Some(rt.trigger(input(60),60,1.).unwrap()); });
        let note = note.unwrap();
        let mut out = [[0.;2];128];
        support::without_heap(|| rt.render(&mut out).unwrap());
        assert_eq!(rt.stats().v1_control_starts,1);
        let depth = sampler_core::lower::ir_control_id("external-depth");
        let bypass = sampler_core::lower::ir_control_id("external-bypass");
        // At a control-grid boundary the current route depth supersedes saved0.7 and init0.4.
        support::without_heap(|| { rt.edit_controls(plan,None,&[ControlWrite{id:depth,value:ControlValue::Real(0.75)}]).unwrap(); });
        support::without_heap(|| rt.render(&mut out).unwrap());
        assert_eq!(out[127],[0.25;2]);
        // Bypass masks the destination while input/lag state follows the held controller.
        support::without_heap(|| {
            rt.edit_controls(plan,None,&[ControlWrite{id:bypass,value:ControlValue::Real(1.)}]).unwrap();
            rt.set_controller(performance,11,u32::MAX).unwrap();
            rt.render(&mut out).unwrap();
        });
        assert_eq!(out[127],[1.;2]);
        support::without_heap(|| {
            rt.edit_controls(plan,None,&[ControlWrite{id:bypass,value:ControlValue::Real(0.)}]).unwrap();
            rt.render(&mut out).unwrap();
        });
        assert_eq!(out[127],[1.;2]);
        assert!(rt.note(note).is_ok());
        assert_eq!(rt.stats().v1_control_starts,1,"updates must keep the same native source state");
    }
}

#[test]
fn initialized_envelope_overlay_declines_stale_raw_native_coefficients() {
    for native in [false,true] {
        let mut ir = native_primary_fixture();
        ir.source_indices.ahdsrs[0].native_amplitude = native;
        let ir::ModulationSource::Envelope(envelope) = &mut ir.modulators[0].source else { unreachable!() };
        envelope.attack = ir::Time::Milliseconds(2.);
        envelope.attack_shape = ir::Curve::Step;
        let mut generic = ir.clone(); generic.source_indices.ahdsrs.clear();
        let make = |i:&ir::Instrument| Runtime::new(lower(i,48000,vec![constant(1.)],no_behaviors).unwrap(),limits()).unwrap();
        let mut actual = make(&ir); let mut expected = make(&generic);
        support::without_heap(|| {
            actual.trigger(input(60),60,1.).unwrap(); expected.trigger(input(60),60,1.).unwrap();
        });
        for _ in 0..5 {
            let mut a = [[0.;2];37]; let mut b = [[0.;2];37];
            support::without_heap(|| {actual.render(&mut a).unwrap();expected.render(&mut b).unwrap();});
            assert_eq!(a,b,"an initialized Step envelope must keep the current owner");
        }
        assert_eq!(actual.stats().v1_control_starts,0);
    }
}

#[test]
fn native_control_state_keeps_physical_plan_ownership_across_replacement() {
    use sampler_core::{ControlValue,ControlWrite};
    let old = controlled_native_external_fixture();
    let mut new = old.clone();
    let ir::ControlValue::Continuous{default,..} = &mut new.controls[0].value else {unreachable!()};
    *default = f64::from(0.8f32);
    new.routes[0].depth = ir::Depth::Normalized(f64::from(0.8f32));
    let old_prepared = lower(&old,48000,vec![constant(1.)],no_behaviors).unwrap();
    let new_prepared = lower(&new,48000,vec![constant(0.5)],no_behaviors).unwrap();
    let (mut rt,mut worker) = Runtime::with_plan_updates(old_prepared,limits(),2,1).unwrap();
    let old_plan = rt.active_plan();
    let mut old_note = None;
    let mut new_note = None;
    let mut out = [[0.;2];128];
    support::without_heap(|| {
        rt.set_controller(rt.performance(0).unwrap(),11,0).unwrap();
        old_note = Some(rt.trigger(Input{external_id:Some(1),..input(60)},60,1.).unwrap());
        rt.render(&mut out).unwrap();
    });
    let request = worker.submit(Box::new(new_prepared)).unwrap();
    support::without_heap(|| { assert_eq!(rt.poll_plan_update(),Ok(Some(request))); });
    let new_plan = rt.active_plan();
    assert_ne!(old_plan,new_plan);
    support::without_heap(|| {
        new_note = Some(rt.trigger(Input{external_id:Some(2),..input(60)},60,1.).unwrap());
        rt.render(&mut out).unwrap();
    });
    assert_eq!(rt.note_plan(old_note.unwrap()),Ok(old_plan));
    assert_eq!(rt.note_plan(new_note.unwrap()),Ok(new_plan));
    assert!((out[127][0]-0.7).abs()<1e-6);
    support::without_heap(|| {
        rt.edit_controls(new_plan,None,&[ControlWrite{id:sampler_core::lower::ir_control_id("external-depth"),value:ControlValue::Real(0.2)}]).unwrap();
        rt.render(&mut out).unwrap();
    });
    assert!((out[127][0]-1.).abs()<1e-6,"new generation depth cannot retarget the held old source");
    support::without_heap(|| {
        rt.edit_controls(old_plan,None,&[ControlWrite{id:sampler_core::lower::ir_control_id("external-bypass"),value:ControlValue::Real(1.)}]).unwrap();
        rt.render(&mut out).unwrap();
    });
    assert!((out[127][0]-1.4).abs()<1e-6,"old physical control cannot alter the replacement source");
    assert_eq!(rt.stats().v1_control_starts,2);
}

#[test]
fn native_external_bypass_keeps_lag_state_running_until_resume() {
    use sampler_core::{ControlValue, ControlWrite};
    let mut ir = controlled_native_external_fixture();
    // Keep the current IR projection and original physical descriptor aligned;
    // native admission checks the route's compiled smoothing against saved lag.
    ir.routes[0].smoothing = ir::Time::Milliseconds(1000.);
    ir.source_indices.modulators[1].settings.as_mut().unwrap().targets[0].lag_ms = 1000;
    let make = || Runtime::new(lower(&ir,48000,vec![constant(1.)],no_behaviors).unwrap(),limits()).unwrap();
    let mut masked = make();
    let mut reference = make();
    let plan = masked.active_plan();
    let bypass = sampler_core::lower::ir_control_id("external-bypass");
    let mut actual = [[0.;2];128];
    let mut expected = [[0.;2];128];
    support::without_heap(|| {
        for rt in [&mut masked,&mut reference] {
            rt.set_controller(rt.performance(0).unwrap(),11,0).unwrap();
            rt.trigger(input(60),60,1.).unwrap();
        }
        masked.render(&mut actual).unwrap();
        reference.render(&mut expected).unwrap();
        masked.edit_controls(plan,None,&[ControlWrite{id:bypass,value:ControlValue::Real(1.)}]).unwrap();
        for rt in [&mut masked,&mut reference] { rt.set_controller(rt.performance(0).unwrap(),11,u32::MAX).unwrap(); }
        for _ in 0..2 {
            masked.render(&mut actual).unwrap();
            reference.render(&mut expected).unwrap();
        }
        masked.edit_controls(plan,None,&[ControlWrite{id:bypass,value:ControlValue::Real(0.)}]).unwrap();
        masked.render(&mut actual).unwrap();
        reference.render(&mut expected).unwrap();
    });
    assert_eq!(actual[127],expected[127],"resume must read the continuously lagged source, not reset to current CC");
    assert!(actual[127][0] < 0.7,"one-second source lag must still be progressing");
    assert_eq!(masked.stats().v1_control_starts,1);
}

#[test]
fn native_external_indices_follow_a_replaced_physical_control_schema() {
    use sampler_core::{ControlDefinition,ControlDomain,ControlId,ControlValue};
    let ir = controlled_native_external_fixture();
    let plan = lower(&ir,48000,vec![constant(1.)],no_behaviors).unwrap();
    let mut controls = plan.controls().to_vec();
    assert!(controls.iter().all(|c| c.id != ControlId(0)));
    controls.push(ControlDefinition{id:ControlId(0),domain:ControlDomain::Real{min:0.,max:1.},default:ControlValue::Real(0.9)});
    let plan = plan.with_controls(controls).unwrap();
    let mut rt = Runtime::new(plan,limits()).unwrap();
    let mut out = [[0.;2];128];
    support::without_heap(|| {
        rt.set_controller(rt.performance(0).unwrap(),11,0).unwrap();
        rt.trigger(input(60),60,1.).unwrap();
        rt.render(&mut out).unwrap();
    });
    assert_eq!(out[127],[(1.-0.4f32);2],"native depth/bypass must use remapped IDs, never old dense indices");
    assert_eq!(rt.stats().v1_control_starts,1);
}

fn controlled_native_flex_fixture() -> ir::Instrument {
    let mut ir = native_primary_fixture();
    ir.source_indices.ahdsrs[0].attack_ms = 0.;
    ir.modulators[0].source = ir::ModulationSource::Envelope(ir::Envelope {
        release: ir::Time::Milliseconds(250.0013),
        release_shape: ir::Curve::Exponential((3.0f64 / 43.).ln()),
        ..Default::default()
    });
    let points = vec![
        ir::kontakt::FlexPoint{time_ms:12.5,level:0.5,curve:0.5},
        ir::kontakt::FlexPoint{time_ms:3.,level:0.,curve:0.5},
    ];
    ir.modulators.push(ir::Modulator{scope:ir::Scope::Voice,source:ir::ModulationSource::Breakpoints(ir::Breakpoints{
        points:points.iter().map(|p|ir::Breakpoint{time:ir::Time::Milliseconds(f64::from(p.time_ms)),level:f64::from(p.level),shape:ir::Curve::Linear}).collect(),sustain:Some(0),loop_range:None,flex_release:false,
    })});
    ir.routes.push(ir::Route::new(ir::ModulatorRef(1),ir::Target::Amplitude,ir::Depth::Normalized(1.)));
    ir.zones[0].routes.push(ir::RouteRef(0));
    ir.source_indices.modulators.push(ir::SourceModulator{group:0,slot:9,external:false,name:"flex".into(),runtime:Some(ir::ModulatorRef(1)),settings:Some(ir::kontakt::Modulation{
        version:1,source:ir::kontakt::ModulationSource::Internal{flags:[0;4],unknown_id:0,source:ir::kontakt::InternalSource::Flex{points,sustain:0,unknown_index:0,unknown_tail:vec![]}},targets:vec![original_volume_target(1.)],
    })});
    ir.controls = ["flex-depth","flex-bypass"].into_iter().enumerate().map(|(i,key)|ir::Control{key:key.into(),label:key.into(),value:ir::ControlValue::Continuous{min:0.,max:1.,default:if i==0{1.}else{0.},unit:ir::ControlUnit::None},automation:ir::Automation::None}).collect();
    ir.source_indices.route_controls.push((ir::RouteRef(0),ir::ControlRef(0),ir::ControlRef(1)));
    ir
}

#[test]
fn native_flex_live_depth_and_bypass_keep_clock_and_primary_release_ownership() {
    use sampler_core::{ControlValue,ControlWrite};
    for processed in [false,true] {
        let mut ir = controlled_native_flex_fixture();
        if processed {
            ir.chains.push(ir::Chain{scope:ir::Scope::Voice,pre_amplitude:vec![],post_amplitude:vec![ir::Processor::Gain(ir::Gain::Linear(1.))]});
            ir.zones[0].chain=Some(ir::ChainRef(0));
        }
        let mut primary = ir.clone();
        primary.zones[0].routes.clear(); primary.routes.clear();primary.modulators.pop();primary.source_indices.modulators.pop();primary.source_indices.route_controls.clear();primary.controls.clear();
        let make=|i:&ir::Instrument|Runtime::new(lower(i,48000,vec![constant(1.)],no_behaviors).unwrap(),limits()).unwrap();
        let mut actual=make(&ir);let mut full=make(&ir);let mut dry=make(&primary);
        let plan=actual.active_plan();
        let depth=sampler_core::lower::ir_control_id("flex-depth");let bypass=sampler_core::lower::ir_control_id("flex-bypass");
        let mut a=[[0.;2];128];let mut f=[[0.;2];128];let mut d=[[0.;2];128];
        support::without_heap(||{
            for rt in [&mut actual,&mut full,&mut dry]{rt.trigger(input(60),60,1.).unwrap();}
            actual.render(&mut a).unwrap();full.render(&mut f).unwrap();dry.render(&mut d).unwrap();
        });
        assert_eq!(a,f);assert_eq!(actual.stats().v1_control_starts,1);
        support::without_heap(||{
            actual.edit_controls(plan,None,&[ControlWrite{id:depth,value:ControlValue::Real(0.)}]).unwrap();
            actual.render(&mut a).unwrap();full.render(&mut f).unwrap();dry.render(&mut d).unwrap();
        });
        assert_eq!(a,d,"zero depth must leave the primary audible");
        support::without_heap(||{
            actual.edit_controls(plan,None,&[ControlWrite{id:depth,value:ControlValue::Real(0.25)}]).unwrap();
            actual.render(&mut a).unwrap();full.render(&mut f).unwrap();dry.render(&mut d).unwrap();
        });
        assert_eq!(a[127],[(1.-0.25*(1.-f64::from(f[127][0]))) as f32;2]);
        support::without_heap(||{
            actual.edit_controls(plan,None,&[ControlWrite{id:depth,value:ControlValue::Real(1.)}]).unwrap();
            actual.render(&mut a).unwrap();full.render(&mut f).unwrap();dry.render(&mut d).unwrap();
        });
        assert_eq!(a,f,"the masked clock must continue without resetting its phase");
        support::without_heap(||{
            actual.edit_controls(plan,None,&[ControlWrite{id:bypass,value:ControlValue::Real(1.)}]).unwrap();
            for rt in [&mut actual,&mut full,&mut dry]{rt.note_off(input(60),None).unwrap();}
            for _ in 0..4{
                actual.render(&mut a).unwrap();full.render(&mut f).unwrap();dry.render(&mut d).unwrap();
            }
        });
        assert_eq!(a,d,"a completed masked Flex must not cut the primary's independent release");
        assert_eq!(actual.voice_count(),1);
        assert_eq!(actual.stats().v1_control_starts,1);
    }
}

#[test]
fn mutable_flex_without_a_raw_primary_keeps_the_shared_synthetic_gate_owner() {
    let mut ir=controlled_native_flex_fixture();
    ir.source_indices.ahdsrs.clear();ir.source_indices.modulators.remove(0);
    ir.modulators[0].source=ir::ModulationSource::Envelope(ir::Envelope{release:ir::Time::Milliseconds(3.),release_shape:ir::Curve::Step,..Default::default()});
    let mut rt=Runtime::new(lower(&ir,48000,vec![constant(1.)],no_behaviors).unwrap(),limits()).unwrap();
    support::without_heap(||{rt.trigger(input(60),60,1.).unwrap();rt.render(&mut [[0.;2];128]).unwrap();});
    assert_eq!(rt.stats().v1_control_starts,0,"the unity native primary cannot replace a mutable synthetic gate");
}

#[test]
fn initialized_flex_route_transforms_keep_the_current_source_consumer() {
    for transform in 0..3 {
        let mut ir = controlled_native_flex_fixture();
        match transform {
            0 => ir.routes[0].invert = true,
            1 => {
                ir.shapes.push(ir::Shape { points: vec![(0.,0.2),(1.,0.9)] });
                ir.routes[0].shape = Some(ir::ShapeRef(0));
            }
            _ => ir.routes[0].smoothing = ir::Time::Milliseconds(1000.),
        }
        // Drop only the original Flex descriptor in the reference. Its current
        // source, transform and independently prepared native primary remain.
        let mut current = ir.clone();
        current.source_indices.modulators.pop();
        let make = |i:&ir::Instrument| Runtime::new(lower(i,48000,vec![constant(1.)],no_behaviors).unwrap(),limits()).unwrap();
        let mut actual = make(&ir); let mut expected = make(&current);
        let mut nonzero = false;
        support::without_heap(|| {
            actual.trigger(input(60),60,1.).unwrap(); expected.trigger(input(60),60,1.).unwrap();
        });
        for block in 0..24 {
            if block == 16 {
                support::without_heap(|| {actual.note_off(input(60),None).unwrap();expected.note_off(input(60),None).unwrap();});
            }
            let mut a = [[0.;2];37]; let mut b = [[0.;2];37];
            support::without_heap(|| {actual.render(&mut a).unwrap();expected.render(&mut b).unwrap();});
            assert_eq!(a,b,"initialized Flex transform {transform} cannot be replaced by saved unity multiplication");
            nonzero |= a.iter().any(|frame| frame[0] != 0.);
        }
        assert!(nonzero);
        assert_eq!(actual.stats().v1_control_starts,0,"transformed Flex must keep its current route owner");
    }
}

#[test]
fn post_prepare_ahdsr_timing_curve_and_sustain_writes_use_future_controlled_owner() {
    use sampler_core::{EngineParameterAddress,engine_parameter_id};
    let ir=native_primary_fixture();let mut generic=ir.clone();generic.source_indices.ahdsrs.clear();
    for (parameter,value) in [("ENGINE_PAR_ATTACK",0),("ENGINE_PAR_ATK_CURVE",500000),("ENGINE_PAR_SUSTAIN",625000)] {
        let make=|i:&ir::Instrument|Runtime::new(lower(i,48000,vec![constant(1.)],no_behaviors).unwrap(),limits()).unwrap();
        let mut actual=make(&ir);let mut expected=make(&generic);
        let address=EngineParameterAddress{parameter:engine_parameter_id(parameter).unwrap(),group:0,slot:7,generic:-1};
        support::without_heap(||{
            actual.set_engine_parameter(address,value).unwrap();expected.set_engine_parameter(address,value).unwrap();
            actual.trigger(input(60),60,1.).unwrap();expected.trigger(input(60),60,1.).unwrap();
        });
        assert_eq!(actual.engine_parameter(address),expected.engine_parameter(address));
        for block in 0..8 {
            if block==4 {support::without_heap(||{actual.note_off(input(60),None).unwrap();expected.note_off(input(60),None).unwrap();});}
            let mut a=[[0.;2];37];let mut b=[[0.;2];37];
            support::without_heap(||{actual.render(&mut a).unwrap();expected.render(&mut b).unwrap();});
            assert_eq!(a,b,"{parameter} must not be overwritten by saved native coefficients");
        }
        assert_eq!(actual.stats().v1_control_starts,0);
    }
}

fn allocation_fixture() -> ir::Instrument {
    let limit = || ir::VoiceLimit {
        voices: 16,
        kill: ir::Kill::Oldest,
        prefer_released: false,
        fade: ir::Time::Seconds(8.0 / 48000.0),
        exclusive_set: None,
    };
    ir::Instrument {
        assets: ["a", "b", "c"].map(asset).to_vec(),
        groups: (0..3)
            .map(|i| ir::Group {
                voice_limit: Some(i),
                ..Default::default()
            })
            .collect(),
        voice_limits: (0..3).map(|_| limit()).collect(),
        zones: (0..3)
            .map(|i| ir::Zone {
                keys: ir::KeyRange {
                    low: 60 + i as u8,
                    high: 60 + i as u8,
                },
                pitch: ir::KeyTracking::Fixed,
                velocity: ir::VelocityResponse::None,
                group: Some(ir::GroupRef(i)),
                ..ir::Zone::new(ir::AssetRef(i))
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn m1_exclusive_voice_domains_cut_other_domains_with_fade_and_plan_isolation() {
    let mut ir = allocation_fixture();
    ir.voice_limits[0].exclusive_set = Some(0);
    ir.voice_limits[1].exclusive_set = Some(0);
    let prepare = || {
        lower(
            &ir,
            48000,
            vec![constant(0.1), constant(0.2), constant(0.3)],
            no_behaviors,
        )
        .unwrap()
    };
    let (mut rt, mut control) = Runtime::with_plan_updates(prepare(), limits(), 2, 1).unwrap();
    control.submit(Box::new(prepare())).unwrap();
    support::without_heap(|| {
        rt.trigger(input(60), 60, 1.).unwrap(); // old instrument retains its voice
        rt.poll_plan_update().unwrap();
        rt.trigger(input(60), 60, 1.).unwrap();
        rt.trigger(input(60), 60, 1.).unwrap(); // same domain does not exclude itself
        rt.trigger(input(62), 62, 1.).unwrap(); // unrelated exclusive set
        rt.trigger(input(61), 61, 1.).unwrap(); // cuts both new-plan domain-0 voices
        assert_eq!(rt.stolen_voices(), 2);
        let mut out = [[0.; 2]; 9];
        rt.render(&mut out).unwrap();
        assert_eq!(rt.voice_count(), 3);
        assert!((out[8][0] - 0.6).abs() < 1e-6, "{:?}", out[8]);
    });
}

#[test]
fn m1_independent_source_budget_spans_voice_groups_without_limiting_other_sources() {
    let mut ir = allocation_fixture();
    ir.voice_limits.push(ir::VoiceLimit {
        voices: 1,
        kill: ir::Kill::Oldest,
        prefer_released: false,
        fade: ir::Time::Seconds(8.0 / 48000.0),
        exclusive_set: None,
    });
    ir.groups[0].source_voice_limit = Some(3);
    ir.groups[1].source_voice_limit = Some(3);
    let plan = lower(
        &ir,
        48000,
        vec![constant(0.1), constant(0.2), constant(0.3)],
        no_behaviors,
    )
    .unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    support::without_heap(|| {
        rt.trigger(input(60), 60, 1.).unwrap();
        rt.trigger(input(62), 62, 1.).unwrap();
        rt.trigger(input(61), 61, 1.).unwrap();
        assert_eq!(rt.stolen_voices(), 1);
        let mut out = [[0.; 2]; 9];
        rt.render(&mut out).unwrap();
        assert_eq!(rt.voice_count(), 2);
        assert!((out[8][0] - 0.5).abs() < 1e-6);
    });
}

#[test]
fn m1_domain_counts_track_fades_growth_retirement_and_slot_reuse() {
    use sampler_core::{AllocationCounts, Error};
    let mut ir = allocation_fixture();
    ir.voice_limits[0].voices = 1;
    let prepare = || {
        lower(
            &ir,
            48000,
            vec![constant(0.1), constant(0.2), constant(0.3)],
            no_behaviors,
        )
        .unwrap()
    };
    let (mut rt, mut control) = Runtime::with_plan_updates(prepare(), limits(), 2, 1).unwrap();
    let old = rt.active_plan();
    let instrument = rt.allocation_domain(old, None).unwrap();
    let group = rt.allocation_domain(old, Some(0)).unwrap();
    let a = rt.trigger(input(60), 60, 1.).unwrap();
    let b = rt.trigger(input(60), 60, 1.).unwrap();
    assert_eq!(
        rt.allocation_counts(group),
        Ok(AllocationCounts { live: 1, fading: 1 })
    );
    assert_eq!(
        rt.allocation_counts(instrument),
        Ok(AllocationCounts { live: 1, fading: 1 })
    );
    control.grow_voices(64).unwrap();
    control.submit(Box::new(prepare())).unwrap();
    support::without_heap(|| {
        rt.render(&mut [[0.; 2]; 9]).unwrap();
        assert_eq!(rt.voice_capacity(), 64);
        assert_eq!(
            rt.allocation_counts(group),
            Ok(AllocationCounts { live: 1, fading: 0 })
        );
        rt.poll_plan_update().unwrap();
        let active = rt.active_plan();
        let other = rt.allocation_domain(active, Some(0)).unwrap();
        assert_ne!(group, other);
        rt.trigger(input(60), 60, 1.).unwrap();
        assert_eq!(
            rt.allocation_counts(other),
            Ok(AllocationCounts { live: 1, fading: 0 })
        );
        assert_eq!(
            rt.allocation_counts(group),
            Ok(AllocationCounts { live: 1, fading: 0 })
        );
        rt.key_up(a, None).unwrap();
        rt.key_up(b, None).unwrap();
        rt.render(&mut [[0.; 2]; 9]).unwrap();
        rt.flush_ended(|_| true);
        rt.collect_retired_plans();
        assert_eq!(rt.allocation_counts(group), Err(Error::StaleHandle));
        rt.trigger(input(60), 60, 1.).unwrap(); // reuse ended voice slots
        rt.render(&mut [[0.; 2]; 9]).unwrap();
        assert_eq!(
            rt.allocation_counts(other),
            Ok(AllocationCounts { live: 1, fading: 0 })
        );
    });
    drop(control.retired());
}

#[test]
fn m1_domain_membership_preserves_existing_victim_modes() {
    // This freezes the existing law, including Any=quietest. It is not a native
    // Any/Pref.Rel parity assertion; those modes still require the native probe.
    for (kill, surviving_level) in [
        (ir::Kill::Any, 0.5),
        (ir::Kill::Oldest, 0.5),
        (ir::Kill::Newest, 0.4),
        (ir::Kill::Highest, 0.4),
        (ir::Kill::Lowest, 0.5),
    ] {
        let mut ir = allocation_fixture();
        for zone in &mut ir.zones {
            zone.group = Some(ir::GroupRef(0));
        }
        ir.voice_limits[0].voices = 2;
        ir.voice_limits[0].kill = kill;
        let plan = lower(
            &ir,
            48000,
            vec![constant(0.1), constant(0.2), constant(0.3)],
            no_behaviors,
        )
        .unwrap();
        let mut rt = Runtime::new(plan, limits()).unwrap();
        support::without_heap(|| {
            for key in [60, 61, 62] {
                rt.trigger(input(key), key, 1.).unwrap();
            }
            let mut out = [[0.; 2]; 9];
            rt.render(&mut out).unwrap();
            assert_eq!(rt.voice_count(), 2);
            assert!(
                (out[8][0] - surviving_level).abs() < 1e-6,
                "{kill:?}: {:?}",
                out[8]
            );
        });
    }
}

#[test]
fn m1_reconfiguring_limits_rejects_stale_domain_bindings() {
    use sampler_core::{Error, Kill, VoiceLimit};
    let ir = allocation_fixture();
    let prepare = || {
        lower(
            &ir,
            48000,
            vec![constant(0.1), constant(0.2), constant(0.3)],
            no_behaviors,
        )
        .unwrap()
    };
    let limit = VoiceLimit {
        voices: 1,
        kill: Kill::Oldest,
        prefer_released: false,
        fade: 8,
    };
    let source = prepare()
        .with_source_voice_limits(vec![Some(2), None, None])
        .unwrap();
    assert!(matches!(
        source.with_voice_limits(None, vec![limit], vec![Some(0); 3]),
        Err(Error::InvalidInput)
    ));
    let exclusive = prepare()
        .with_exclusive_sets(vec![Some(0), Some(0), None])
        .unwrap();
    assert!(matches!(
        exclusive.with_voice_limits(None, vec![limit], vec![Some(0); 3]),
        Err(Error::InvalidInput)
    ));
}

/// IR lowering of the router's new forms: an LFO rate route, a lagged signed
/// shaped intensity assignment and a chained second one all reach audio.
#[test]
fn rate_routes_and_chained_intensity_assignments_lower_and_render() {
    let lfo = ir::Lfo {
        shape: ir::LfoShape::Sine,
        rate: ir::Frequency::Hertz(2.),
        delay: ir::Time::ZERO,
        fade_in: ir::Time::ZERO,
        phase: 0.,
        retrigger: true,
    };
    let voice = |source| ir::Modulator { scope: ir::Scope::Voice, source };
    let law = |depth| ir::ScaleLaw::KontaktIntensity { depth, flags: 0x12, unit: 1. };
    let mut instrument = ir::Instrument {
        assets: vec![asset("a")],
        zones: vec![ir::Zone {
            keys: ir::KeyRange { low: 60, high: 60 },
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            routes: vec![ir::RouteRef(0), ir::RouteRef(1)],
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        modulators: vec![
            voice(ir::ModulationSource::Lfo(lfo)),
            voice(ir::ModulationSource::Constant),
            voice(ir::ModulationSource::Key),
        ],
        routes: vec![
            ir::Route::new(ir::ModulatorRef(1), ir::Target::SourceRate(ir::ModulatorRef(0)), ir::Depth::Normalized(0.1)),
            ir::Route {
                scale: Some(ir::RouteScale {
                    source: ir::ModulatorRef(1),
                    shape: None,
                    law: law(-0.5),
                    lag_ms: 10.,
                }),
                ..ir::Route::new(ir::ModulatorRef(2), ir::Target::Amplitude, ir::Depth::Normalized(0.8))
            },
        ],
        ..Default::default()
    };
    instrument.source_indices.chained_scales.push((
        ir::RouteRef(1),
        ir::RouteScale { source: ir::ModulatorRef(1), shape: None, law: law(-0.25), lag_ms: 0. },
    ));
    instrument.validate().unwrap();
    let plan = lower(&instrument, 48000, vec![constant(0.5)], no_behaviors).unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    let mut audio = [[0.; 2]; 512];
    support::without_heap(|| {
        rt.trigger(input(60), 60, 1.).unwrap();
        rt.render(&mut audio).unwrap();
    });
    // 0.8 * (1 + 0.5*0) * (1 + 0.25*0): Constant is 1, so the lane stays 0.8.
    let key = 60. / 127.;
    let expected = 0.5 * (1. - 0.8 * (1. - key));
    assert!((f64::from(audio[500][0]) - expected).abs() < 1e-3, "{}", audio[500][0]);
}

/// A route reaches a script-addressed effect module's knob: the lane moves by
/// `depth x span x source` (additive, FUN_140873040's remaining-ID template), so
/// the routed render equals a module saved at the moved point.
#[test]
fn routes_move_scripted_effect_lanes_additively() {
    let address = ir::SlotAddress { group: 0, slot: 7, generic: -1 };
    let build = |shape: f32, routed: bool| {
        let mut i = ir::Instrument {
            assets: vec![asset("a")],
            zones: vec![ir::Zone {
                keys: ir::KeyRange { low: 60, high: 60 },
                pitch: ir::KeyTracking::Fixed,
                velocity: ir::VelocityResponse::None,
                chain: Some(ir::ChainRef(0)),
                routes: if routed { vec![ir::RouteRef(0)] } else { vec![] },
                ..ir::Zone::new(ir::AssetRef(0))
            }],
            chains: vec![ir::Chain {
                scope: ir::Scope::Voice,
                pre_amplitude: vec![ir::Processor::Color(ir::fx::color::Config::classic(shape).at(address))],
                post_amplitude: vec![],
            }],
            modulators: vec![ir::Modulator { scope: ir::Scope::Voice, source: ir::ModulationSource::Key }],
            ..Default::default()
        };
        if routed {
            i.routes.push(ir::Route::new(
                ir::ModulatorRef(0),
                ir::Target::Processor {
                    chain: ir::ChainRef(0),
                    index: 0,
                    parameter: ir::ProcessorParameter::Shape,
                },
                ir::Depth::Normalized(0.25),
            ));
        }
        i
    };
    let render = |i: &ir::Instrument| {
        let plan = lower(i, 48000, vec![constant(0.5)], no_behaviors).unwrap();
        let mut rt = Runtime::new(plan, limits()).unwrap();
        let mut audio = [[0.; 2]; 2048];
        support::without_heap(|| {
            rt.trigger(input(60), 60, 1.).unwrap();
            for chunk in audio.chunks_mut(64) {
                rt.render(chunk).unwrap();
            }
        });
        audio[2000][0]
    };
    // Shape spans -1..=1, so 0.25 of the range times the key (60/127).
    let moved = -0.5 + 0.25 * 2. * (60. / 127.);
    let routed = render(&build(-0.5, true));
    let saved = render(&build(moved as f32, false));
    let unmoved = render(&build(-0.5, false));
    assert!((routed - saved).abs() < 1e-4, "{routed} vs {saved}");
    assert!((routed - unmoved).abs() > 1e-3, "{routed} vs {unmoved}");
}

#[test]
fn compressor_editors_use_each_laws_physical_domain_without_invalidating_loads() {
    use ir::fx::dynamics::{Config, Law, Link, Native};
    let native = Native { threshold_db: -12.0, ratio: 0.2 };
    for (law, expected_controls) in [
        (Law::KontaktClassic(native), 4),
        (Law::KontaktEnhanced(native), 4),
        (Law::KontaktPro(native), 4),
        (Law::KontaktFeedback { native: Native { threshold_db: 1.7782794, ratio: 4.8 }, high_quality: true }, 4),
        (Law::KontaktLimiter(Native { threshold_db: 0.0, ratio: 0.0 }), 2),
        (Law::KontaktHilbert(Native { threshold_db: 0.5, ratio: 0.8 }, ir::fx::dynamics::Key::Internal), 0),
    ] {
        let mut i = instrument();
        i.sequences.clear();
        i.zones = vec![ir::Zone { chain: Some(ir::ChainRef(0)),
            pitch: ir::KeyTracking::Fixed, velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(0)) }];
        i.assets.truncate(1);
        i.chains = vec![ir::Chain { scope: ir::Scope::Voice,
            pre_amplitude: vec![], post_amplitude: vec![ir::Processor::Dynamics(Config {
                address: None, law, attack: ir::Time::Seconds(0.01),
                release: ir::Time::Seconds(0.1), link: Link::Max,
                makeup: ir::Gain::UNITY, mix: 1.0,
            })] }];
        i.register_compressor_controls();
        i.register_compressor_controls();
        assert_eq!(i.controls.len(), expected_controls, "{law:?}");
        if matches!(law, Law::KontaktFeedback { .. } | Law::KontaktLimiter(_)) {
            assert_eq!(i.controls[0].label, "Input gain");
            assert_eq!(i.controls[0].key, "compressor:0:0:Threshold");
        }
        let plan = lower(&i, 48000, vec![constant(0.8)], no_behaviors).unwrap();
        let descriptors: Vec<_> = plan.parameter_registry().descriptors().collect();
        assert_eq!(descriptors.len(), expected_controls, "{law:?}");
        if matches!(law, Law::KontaktFeedback { .. }) {
            assert_eq!(descriptors[0].unit, sampler_core::ParameterUnit::Linear);
            assert_eq!(descriptors[0].default, 1.7782794);
            assert_eq!(descriptors[1].default, 4.8);
        }
        let limits = Limits::for_plan(&plan, 4, 4);
        Runtime::new(plan, limits).unwrap();
    }
}
