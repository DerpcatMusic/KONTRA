mod support;
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

/// An AHD (one-shot) envelope on a looped release zone lowers, ignores the
/// gate and ends its voice after the decay; the loop never sounds past it.
#[test]
fn one_shot_envelope_bounds_a_looped_release_zone() {
    let mut ir = instrument();
    ir.zones.truncate(1);
    ir.zones[0].selection = None;
    ir.sequences.clear();
    let mut release = ir.zones[0].clone();
    release.asset = ir::AssetRef(3);
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
    // Bend anywhere else is a bipolar voice modulation source.
    assert!(lower(&ir, 48000, vec![constant(0.5)], no_behaviors).is_ok());
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

/// A 4-pole filter lowers as two cascaded 2-pole sections: it attenuates a
/// tone well above the cutoff more than one 2-pole section does.
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
    let (two, four) = (tone(2), tone(4));
    assert!(two.is_finite() && four.is_finite());
    assert!(four < two * 0.5, "two {two}, four {four}");
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
    rt.set_engine_offsets(&[sampler_core::EngineParameterOffset { address, offset: 0.1 }]).unwrap();
    rt.render(&mut out).unwrap();
    assert!((out[32][0] / before - 0.35).abs() < 0.001);
    assert_eq!(rt.engine_parameter(address), Ok(250000));
    rt.set_engine_offsets(&[]).unwrap();
    rt.render(&mut out).unwrap();
    assert!((out[32][0] / before - 0.25).abs() < 0.001);
    instrument.chains[0].pre_amplitude[0] = ir::Processor::Gainer {
        gain: ir::Gain::UNITY,
        dry: 0.,
    };
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
            pre_amplitude: vec![ir::Processor::StereoModeller {
                width: 0.5,
                pan: 0.,
                pseudo: false,
            }],
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
        ir::Processor::StereoModeller {
            width: 0.5,
            pan: 0.0,
            pseudo: true,
        },
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
