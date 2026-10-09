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
    instrument.chains[0].pre_amplitude[0] = ir::Processor::LadderLP4(ir::LadderLP4 {
        address: None, gain: 0., cutoff: 0., resonance: 0., record_version: 0x92,
    });
    let pcm = Pcm::new(48000, (0..4096)
        .map(|i| [(i as f32 * std::f32::consts::TAU / 8.).sin() * 0.001; 2])
        .collect::<Vec<_>>().into_boxed_slice()).unwrap();
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

#[test]
fn physical_ladder_lanes_keep_signed_gain_and_drive_audio_without_heap() {
    use sampler_core::{EngineParameterAddress, engine_parameter_id};
    let address = ir::SlotAddress { group: 4, slot: 3, generic: -1 };
    let instrument = ir::Instrument {
        assets: vec![asset("tone")],
        chains: vec![ir::Chain { scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::LadderLP4(ir::LadderLP4 {
                address: Some(address), gain: -0.25, cutoff: 1., resonance: 0., record_version: 0x92 })],
            post_amplitude: vec![] }],
        zones: vec![ir::Zone { keys: ir::KeyRange { low: 60, high: 60 },
            pitch: ir::KeyTracking::Fixed, velocity: ir::VelocityResponse::None,
            chain: Some(ir::ChainRef(0)), ..ir::Zone::new(ir::AssetRef(0)) }], ..Default::default() };
    let pcm = Pcm::new(48000, (0..10000).map(|i|
        [(i as f32 * std::f32::consts::TAU / 100.).sin() * 0.0001; 2]).collect::<Vec<_>>().into_boxed_slice()).unwrap();
    let plan = lower(&instrument, 48000, vec![pcm], no_behaviors).unwrap();
    assert_eq!(plan.engine_parameter_bindings().len(), 3);
    let gain = EngineParameterAddress { parameter: engine_parameter_id("ENGINE_PAR_GAIN").unwrap(),
        group: 4, slot: 3, generic: -1 };
    let mut rt = Runtime::new(plan, limits()).unwrap();
    assert_eq!(rt.engine_parameter(gain), Ok(-250000));
    let mut out = [[0.; 2]; 2000];
    rt.trigger(input(60), 60, 1.).unwrap();
    support::without_heap(|| rt.render(&mut out).unwrap());
    let low: f64 = out[1000..].iter().map(|x| f64::from(x[0]).powi(2)).sum();
    support::without_heap(|| { rt.set_engine_parameter(gain, 250000).unwrap(); rt.render(&mut out).unwrap(); });
    let high: f64 = out[1000..].iter().map(|x| f64::from(x[0]).powi(2)).sum();
    assert!((10. * (high / low).log10() - 6.).abs() < 0.02, "{low} {high}");
    assert_eq!(rt.engine_parameter(gain), Ok(250000));
}

#[test]
fn normalized_cutoff_routes_address_ladder_and_daft_without_audio_heap() {
    for ladder in [true, false] {
        let render = |cutoff, depth| {
            let processor = if ladder {
                ir::Processor::LadderLP4(ir::LadderLP4 { address: None, gain: 0.,
                    cutoff, resonance: 0.1, record_version: 0x92 })
            } else {
                ir::Processor::Daft(ir::Daft { gain: 0., cutoff, resonance: 0.1, highpass: false })
            };
            let instrument = ir::Instrument {
                assets: vec![asset("native cutoff")],
                chains: vec![ir::Chain { scope: ir::Scope::Voice,
                    pre_amplitude: vec![ir::Processor::Gain(ir::Gain::Linear(1.)), processor],
                    post_amplitude: vec![] }],
                modulators: vec![ir::Modulator { scope: ir::Scope::Voice, source: ir::ModulationSource::Constant }],
                routes: vec![ir::Route { source: ir::ModulatorRef(0),
                    target: ir::Target::Processor { chain: ir::ChainRef(0), index: 1, parameter: ir::ProcessorParameter::Cutoff },
                    depth: ir::Depth::Normalized(depth), invert: false, shape: None, smoothing: ir::Time::Seconds(0.), scale: None }],
                zones: vec![ir::Zone { chain: Some(ir::ChainRef(0)), routes: vec![ir::RouteRef(0)],
                    pitch: ir::KeyTracking::Fixed, velocity: ir::VelocityResponse::None,
                    ..ir::Zone::new(ir::AssetRef(0)) }], ..Default::default() };
            let pcm = Pcm::new(48000, (0..4096).map(|i|
                [0.01 * (i as f32 * std::f32::consts::TAU / 24.).sin(); 2]).collect::<Vec<_>>().into_boxed_slice()).unwrap();
            let plan = lower(&instrument, 48000, vec![pcm], no_behaviors).unwrap();
            let mut runtime = Runtime::new(plan, limits()).unwrap();
            let mut out = [[0.; 2]; 2048];
            support::without_heap(|| {
                runtime.trigger(input(60), 60, 1.).unwrap();
                for frames in out.chunks_mut(7) { runtime.render(frames).unwrap(); }
                runtime.note_off(input(60), None).unwrap();
            });
            out
        };
        let dry = render(0.5, 0.);
        let wet = render(0.5, 0.25);
        assert_eq!(wet, render(0.75, 0.), "normalized depth must adjust the saved knob before conversion");
        let energy = |x: &[[f32; 2]]| x[1024..].iter().flatten().map(|v| f64::from(*v).powi(2)).sum::<f64>();
        assert!(10. * (energy(&wet) / energy(&dry)).log10() > 3., "opening the cutoff must pass more of the 2 kHz tone");
    }
}

#[test]
fn normalized_q_gain_routes_address_native_knobs_without_audio_heap() {
    for ladder in [true, false] {
      for parameter in [ir::ProcessorParameter::Resonance, ir::ProcessorParameter::Gain] {
        let render = |value, depth| {
            let (gain, resonance) = if parameter == ir::ProcessorParameter::Gain { (value, 0.1) } else { (0., value) };
            let processor = if ladder {
                ir::Processor::LadderLP4(ir::LadderLP4 { address: None, gain,
                    cutoff: 0.5, resonance, record_version: 0x92 })
            } else {
                ir::Processor::Daft(ir::Daft { gain, cutoff: 0.5, resonance, highpass: false })
            };
            let instrument = ir::Instrument {
                assets: vec![asset("native cutoff")],
                chains: vec![ir::Chain { scope: ir::Scope::Voice,
                    pre_amplitude: vec![ir::Processor::Gain(ir::Gain::Linear(1.)), processor],
                    post_amplitude: vec![] }],
                modulators: vec![ir::Modulator { scope: ir::Scope::Voice, source: ir::ModulationSource::Constant }],
                routes: vec![ir::Route { source: ir::ModulatorRef(0),
                    target: ir::Target::Processor { chain: ir::ChainRef(0), index: 1, parameter },
                    depth: ir::Depth::Normalized(depth), invert: false, shape: None, smoothing: ir::Time::Seconds(0.), scale: None }],
                zones: vec![ir::Zone { chain: Some(ir::ChainRef(0)), routes: vec![ir::RouteRef(0)],
                    pitch: ir::KeyTracking::Fixed, velocity: ir::VelocityResponse::None,
                    ..ir::Zone::new(ir::AssetRef(0)) }], ..Default::default() };
            let pcm = Pcm::new(48000, (0..4096).map(|i|
                [0.01 * (i as f32 * std::f32::consts::TAU / 24.).sin(); 2]).collect::<Vec<_>>().into_boxed_slice()).unwrap();
            let plan = lower(&instrument, 48000, vec![pcm], no_behaviors).unwrap();
            let mut runtime = Runtime::new(plan, limits()).unwrap();
            let mut out = [[0.; 2]; 2048];
            support::without_heap(|| {
                runtime.trigger(input(60), 60, 1.).unwrap();
                for frames in out.chunks_mut(7) { runtime.render(frames).unwrap(); }
                runtime.note_off(input(60), None).unwrap();
            });
            out
        };
        let dry = render(0.1, 0.);
        let wet = render(0.1, 0.4);
        assert_eq!(wet, render(0.5, 0.), "depth must add to the native knob before conversion");
        assert_ne!(wet, dry, "Q/Gain must change audio");
        assert_eq!(render(0.9, 0.4), render(1., 0.), "normalized knobs saturate");
        if ladder && parameter == ir::ProcessorParameter::Gain {
            assert_eq!(render(-0.25, 0.), render(0., 0.), "enabled zero-depth Gain clamps saved signed gain, as v1 does");
        }
      }
    }
}

#[test]
fn arbitrary_registered_control_target_drives_its_real_processor_per_voice_without_heap() {
    let mut instrument = ir::Instrument {
        assets: vec![asset("generic target")],
        controls: vec![ir::Control { key: "custom/gain/no-native-id".into(), label: "level".into(),
            value: ir::ControlValue::Continuous { min: 0., max: 2., default: 1., unit: ir::ControlUnit::None },
            automation: ir::Automation::None }],
        chains: vec![ir::Chain { scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Gain(ir::Gain::UNITY)], post_amplitude: vec![] }],
        processor_controls: vec![ir::ProcessorControl { control: ir::ControlRef(0), chain: ir::ChainRef(0),
            index: 0, parameter: ir::ProcessorParameter::Gain, ramp: ir::Time::ZERO }],
        modulators: vec![ir::Modulator { scope: ir::Scope::Voice, source: ir::ModulationSource::Velocity }],
        routes: vec![ir::Route { source: ir::ModulatorRef(0), target: ir::Target::Control(ir::ControlRef(0)),
            depth: ir::Depth::Normalized(0.25), invert: false, shape: None, smoothing: ir::Time::ZERO, scale: None }],
        zones: vec![ir::Zone { chain: Some(ir::ChainRef(0)), routes: vec![ir::RouteRef(0)],
            keys: ir::KeyRange { low: 60, high: 60 }, pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None, ..ir::Zone::new(ir::AssetRef(0)) }],
        ..Default::default()
    };
    for (processor_target, threads, block) in [false, true].into_iter().flat_map(|processor_target|
        [1, 2, 4].into_iter().flat_map(move |threads| [1, 7, 64, 137].map(|block| (processor_target, threads, block)))) {
        instrument.routes[0].target = if processor_target {
            ir::Target::Processor { chain: ir::ChainRef(0), index: 0, parameter: ir::ProcessorParameter::Gain }
        } else { ir::Target::Control(ir::ControlRef(0)) };
        let prepared = lower(&instrument, 48000, vec![constant(0.1)], no_behaviors)
            .expect("registered processor parameter must have a generic route consumer");
        let published: Vec<_> = prepared.parameter_registry().descriptors().collect();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].name, "level");
        assert_eq!(published[0].range, [0., 2.]);
        assert_eq!(published[0].default, 1.);
        assert_eq!(published[0].address.scope, sampler_core::ParameterScope::Voice);
        assert_eq!(published[0].display.group, "chain 0 processor 0");
        let mut rt = Runtime::new(prepared, Limits { notes: 128, families: 128, expressions: 128, voices: 128, ..limits() }).unwrap().with_threads(sampler_core::Threads::Fixed(threads));
        let voice = |id| Input { protocol: Protocol::Clap, external_id: Some(id), ..input(60) };
        let mut first = [[0.; 2]; 256];
        support::without_heap(|| {
            rt.trigger(voice(1), 60, 1.).unwrap();
            for chunk in first.chunks_mut(block) { rt.render(chunk).unwrap(); }
        });
        assert_eq!(first[128], [0.15; 2]);
        let mut second = [[0.; 2]; 256];
        support::without_heap(|| {
            // Independent voice contribution must not overwrite the first's target.
            rt.trigger(voice(2), 60, 0.5).unwrap();
            for chunk in second.chunks_mut(block) { rt.render(chunk).unwrap(); }
        });
        assert_eq!(second[128], [0.275; 2]);
        let id = sampler_core::lower::ir_control_id("custom/gain/no-native-id");
        let active = rt.active_plan();
        assert_eq!(rt.control_value(active, id), Ok(sampler_core::ControlValue::Real(1.)),
            "voice modulation must not overwrite the shared editor/script base");
        let mut edited = [[0.; 2]; 256];
        support::without_heap(|| {
            rt.edit_controls(active, None, &[sampler_core::ControlWrite {
                id, value: sampler_core::ControlValue::Real(0.5),
            }]).unwrap();
            for chunk in edited.chunks_mut(block) { rt.render(chunk).unwrap(); }
        });
        let expected = 0.1f32 + 0.1f32 * 0.75;
        assert_eq!(edited[128], [expected; 2],
            "a base edit must invalidate consumers while retaining each voice's held offset");
        assert_eq!(rt.control_base_value(active, id), Ok(sampler_core::ControlValue::Real(0.5)));
        let mut many = [[0.; 2]; 256];
        support::without_heap(|| {
            for id in 3..=128 { rt.trigger(voice(id), 60, 1.).unwrap(); }
            for chunk in many.chunks_mut(block) { rt.render(chunk).unwrap(); }
        });
        let expected_many = (3..=128).fold(expected, |sum, _| sum + 0.1f32);
        assert_eq!(many[128], [expected_many; 2]);
        if threads > 1 { assert!(rt.parallel_blocks() > 0, "worker projection must actually execute"); }
    }
}


#[test]
fn generic_voice_filter_parameters_never_share_another_voices_coefficients() {
    let zone = |key, routes| ir::Zone { chain: Some(ir::ChainRef(0)), routes,
        keys: ir::KeyRange { low: key, high: key }, pitch: ir::KeyTracking::Fixed,
        velocity: ir::VelocityResponse::None, ..ir::Zone::new(ir::AssetRef(0)) };
    let instrument = ir::Instrument {
        assets: vec![asset("per-voice filter coefficients")],
        controls: vec![ir::Control { key: "filter/cutoff".into(), label: "Cutoff".into(),
            value: ir::ControlValue::Continuous { min: 500., max: 9500., default: 500., unit: ir::ControlUnit::Hertz },
            automation: ir::Automation::None }],
        chains: vec![ir::Chain { scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Filter(ir::Filter { kind: ir::FilterKind::LowPass { poles: 2 },
                cutoff: ir::Frequency::Hertz(500.), resonance: ir::Resonance::Q(1.) })], post_amplitude: vec![] }],
        processor_controls: vec![ir::ProcessorControl { control: ir::ControlRef(0), chain: ir::ChainRef(0),
            index: 0, parameter: ir::ProcessorParameter::Cutoff, ramp: ir::Time::ZERO }],
        modulators: vec![ir::Modulator { scope: ir::Scope::Voice, source: ir::ModulationSource::Velocity }],
        routes: vec![ir::Route { source: ir::ModulatorRef(0), target: ir::Target::Control(ir::ControlRef(0)),
            depth: ir::Depth::Normalized(1.), invert: false, shape: None, smoothing: ir::Time::ZERO, scale: None }],
        zones: vec![zone(60, vec![ir::RouteRef(0)]), zone(61, vec![])],
        ..Default::default()
    };
    let frames: Vec<_> = (0..4800).map(|n| [0.01 * (std::f32::consts::TAU * n as f32 / 16.).sin(); 2]).collect();
    let runtime = |threads| {
        let pcm = Pcm::new(48000, frames.clone().into_boxed_slice()).unwrap();
        Runtime::new(lower(&instrument, 48000, vec![pcm], no_behaviors).unwrap(),
            Limits { notes: 128, families: 128, expressions: 128, voices: 128, ..limits() }).unwrap()
            .with_threads(sampler_core::Threads::Fixed(threads))
    };
    for threads in [1, 2, 4] {
        let mut expected = [[0f32; 2]; 256];
        for id in 1..=128 {
            let (key, velocity) = match id % 3 { 0 => (61, 1.), 1 => (60, 1.), _ => (60, 0.1) };
            let input = Input { external_id: Some(id), ..input(key) };
            let mut rt = runtime(1);
            let mut separate = [[0.; 2]; 256];
            support::without_heap(|| { rt.trigger(input, key, velocity).unwrap(); rt.render(&mut separate).unwrap(); });
            for (sum, voice) in expected.iter_mut().zip(separate) { for c in 0..2 { sum[c] += voice[c]; } }
        }
        let mut combined = runtime(threads);
        let mut actual = [[0.; 2]; 256];
        support::without_heap(|| {
            for id in 1..=128 {
                let (key, velocity) = match id % 3 { 0 => (61, 1.), 1 => (60, 1.), _ => (60, 0.1) };
                combined.trigger(Input { external_id: Some(id), ..input(key) }, key, velocity).unwrap();
            }
            combined.render(&mut actual).unwrap();
        });
        let mismatch = actual.iter().zip(expected).enumerate().find(|(_, (a, b))| **a != *b);
        assert!(mismatch.is_none(), "each voice keeps its cutoff, including the unmodulated sibling, threads={threads}, first mismatch={mismatch:?}");
        if threads > 1 { assert!(combined.parallel_blocks() > 0); }
    }
}


#[test]
fn registered_peak_gain_keeps_a_flat_band_and_drives_real_voice_audio() {
    let instrument = ir::Instrument {
        source_indices: ir::SourceIndices { control_aliases: vec![ir::SourceControlAlias {
            control: ir::ControlRef(0), parameter: "ENGINE_PAR_GAIN2".into(),
            address: ir::SlotAddress { group: 0, slot: 5, generic: -1 },
        }], ..Default::default() },
        assets: vec![asset("EQ owner witness")],
        controls: vec![ir::Control { key: "eq/physical-slot-5/band-2/gain".into(), label: "Band 2 Gain".into(),
            value: ir::ControlValue::Continuous { min: -18., max: 18., default: 0., unit: ir::ControlUnit::Decibels },
            automation: ir::Automation::None }],
        chains: vec![ir::Chain { scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Filter(ir::Filter {
                kind: ir::FilterKind::Peak { gain: ir::Gain::Decibels(0.) },
                cutoff: ir::Frequency::Hertz(1000.), resonance: ir::Resonance::Q(1.),
            })], post_amplitude: vec![] }],
        processor_controls: vec![ir::ProcessorControl { control: ir::ControlRef(0), chain: ir::ChainRef(0),
            index: 0, parameter: ir::ProcessorParameter::Gain, ramp: ir::Time::ZERO }],
        modulators: vec![ir::Modulator { scope: ir::Scope::Voice, source: ir::ModulationSource::Velocity }],
        routes: vec![ir::Route { source: ir::ModulatorRef(0), target: ir::Target::Control(ir::ControlRef(0)),
            depth: ir::Depth::Normalized(1./3.), invert: false, shape: None, smoothing: ir::Time::ZERO, scale: None }],
        zones: vec![ir::Zone { chain: Some(ir::ChainRef(0)), routes: vec![ir::RouteRef(0)],
            keys: ir::KeyRange { low: 60, high: 60 }, pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None, ..ir::Zone::new(ir::AssetRef(0)) }],
        ..Default::default()
    };
    let mut phantom = instrument.clone();
    phantom.processor_controls.clear();
    assert!(phantom.validate().is_err(), "an alias cannot publish an unbound mirror");
    let mut layer_alias = instrument.clone();
    layer_alias.source_indices.control_aliases[0].address.slot = -1;
    assert!(layer_alias.validate().is_err(), "module aliases cannot shadow layer parameter dispatch");
    let mut duplicate = instrument.clone();
    duplicate.source_indices.control_aliases.push(duplicate.source_indices.control_aliases[0].clone());
    let pcm = Pcm::new(48000, vec![[0.; 2]; 4800].into_boxed_slice()).unwrap();
    assert!(lower(&duplicate, 48000, vec![pcm], no_behaviors).is_err(), "native address aliases cannot conflict");
    let frames: Vec<_> = (0..4800).map(|n| [0.01 * (std::f32::consts::TAU * n as f32 / 48.).sin(); 2]).collect();
    let pcm = Pcm::new(48000, frames.clone().into_boxed_slice()).unwrap();
    let prepared = lower(&instrument, 48000, vec![pcm], no_behaviors)
        .expect("a flat peak band must retain its real gain owner before modulation");
    let native = sampler_core::EngineParameterAddress {
        parameter: sampler_core::engine_parameter_id("ENGINE_PAR_GAIN2").unwrap(), group: 0, slot: 5, generic: -1,
    };
    assert!(prepared.parameter_registry().resolve_native(native).is_some(), "native alias must resolve to the real EQ gain owner");
    let descriptor = prepared.parameter_registry().descriptors().next().unwrap();
    assert_eq!((descriptor.unit, descriptor.range, descriptor.default),
        (sampler_core::ParameterUnit::Decibels, [-18., 18.], 0.));
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    let mut output = [[0.; 2]; 4096];
    support::without_heap(|| { rt.trigger(input(60), 60, 1.).unwrap(); rt.render(&mut output).unwrap(); });
    let power = |frames: &[[f32; 2]]| frames.iter().map(|f| f64::from(f[0]).powi(2)).sum::<f64>();
    let gain_db = 10. * (power(&output[1024..]) / power(&frames[1024..4096])).log10();
    assert!((gain_db - 12.).abs() < 0.02, "center-frequency gain: {gain_db} dB");
    let id = sampler_core::lower::ir_control_id("eq/physical-slot-5/band-2/gain");
    assert_eq!(rt.control_base_value(rt.active_plan(), id), Ok(sampler_core::ControlValue::Real(0.)));
    let mut edited = [[0.; 2]; 512];
    support::without_heap(|| {
        rt.set_engine_parameter(native, 666_667).unwrap();
        rt.render(&mut edited).unwrap();
    });
    let edited_db = 10. * (power(&edited[128..]) / power(&frames[4224..4608])).log10();
    assert!((edited_db - 18.).abs() < 0.02, "native base plus held modulation clamps at +18 dB: {edited_db}");
    let Ok(sampler_core::ControlValue::Real(base)) = rt.control_base_value(rt.active_plan(), id) else { panic!("real EQ base"); };
    assert!((base - 6.000012).abs() < 1e-12);
}

#[test]
fn registered_peak_frequency_and_width_use_normalized_knob_domains() {
    let controls = ["frequency", "bandwidth"].map(|name| ir::Control {
        key: format!("eq/{name}"), label: name.into(),
        value: ir::ControlValue::Continuous { min: 0., max: 1., default: if name == "frequency" { 0.3 } else { 0.2 }, unit: ir::ControlUnit::Percent },
        automation: ir::Automation::None,
    }).to_vec();
    let instrument = ir::Instrument {
        assets: vec![asset("EQ knob domains")], controls,
        chains: vec![ir::Chain { scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Filter(ir::Filter { kind: ir::FilterKind::Peak { gain: ir::Gain::Decibels(12.) },
                cutoff: ir::Frequency::Hertz(20. * 10f64.powf(0.9)), resonance: ir::Resonance::Q(1. / (2. * (std::f64::consts::LN_2 * 0.5 * 0.84).sinh())) })], post_amplitude: vec![] }],
        processor_controls: [ir::ProcessorParameter::Cutoff, ir::ProcessorParameter::Resonance].into_iter().enumerate()
            .map(|(control, parameter)| ir::ProcessorControl { control: ir::ControlRef(control), chain: ir::ChainRef(0), index: 0, parameter, ramp: ir::Time::ZERO }).collect(),
        source_indices: ir::SourceIndices { control_aliases: ["ENGINE_PAR_FREQ2", "ENGINE_PAR_BW2"].into_iter().enumerate()
            .map(|(control, parameter)| ir::SourceControlAlias { control: ir::ControlRef(control), parameter: parameter.into(), address: ir::SlotAddress { group: 0, slot: 5, generic: -1 } }).collect(), ..Default::default() },
        modulators: vec![ir::Modulator { scope: ir::Scope::Voice, source: ir::ModulationSource::Velocity }],
        routes: [0.4, 0.2].into_iter().enumerate().map(|(control, depth)| ir::Route { source: ir::ModulatorRef(0),
            target: ir::Target::Control(ir::ControlRef(control)), depth: ir::Depth::Normalized(depth), invert: false,
            shape: None, smoothing: ir::Time::ZERO, scale: None }).collect(),
        zones: vec![ir::Zone { keys: ir::KeyRange { low: 60, high: 60 }, pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None, chain: Some(ir::ChainRef(0)), routes: vec![ir::RouteRef(0), ir::RouteRef(1)], ..ir::Zone::new(ir::AssetRef(0)) }],
        ..Default::default()
    };
    let hz = 20f32 * 10f32.powf(3. * 0.7);
    let frames: Vec<_> = (0..10000).map(|n| [0.01 * (std::f32::consts::TAU * hz * n as f32 / 48000.).sin(); 2]).collect();
    let pcm = Pcm::new(48000, frames.clone().into_boxed_slice()).unwrap();
    let prepared = lower(&instrument, 48000, vec![pcm.clone()], no_behaviors).expect("both normalized EQ knobs bind actual processor lanes");
    for descriptor in prepared.parameter_registry().descriptors() {
        assert_eq!((descriptor.range, descriptor.unit), ([0., 1.], sampler_core::ParameterUnit::Percent));
    }
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    let mut out = [[0.; 2]; 4096];
    support::without_heap(|| { rt.trigger(input(60), 60, 1.).unwrap(); rt.render(&mut out).unwrap(); });
    let power = |f: &[[f32; 2]]| f.iter().map(|x| f64::from(x[0]).powi(2)).sum::<f64>();
    let boosted_db = 10. * (power(&out[1024..]) / power(&frames[1024..4096])).log10();
    assert!((boosted_db - 12.).abs() < 0.03, "frequency moved in log-knob domain: {boosted_db}");
    let mut edited = [[0.; 2]; 1024];
    support::without_heap(|| {
        for name in ["ENGINE_PAR_FREQ2", "ENGINE_PAR_BW2"] {
            rt.set_engine_parameter(sampler_core::EngineParameterAddress { parameter: sampler_core::engine_parameter_id(name).unwrap(), group: 0, slot: 5, generic: -1 }, 0).unwrap();
        }
        rt.render(&mut edited).unwrap();
    });
    let edited_db = 10. * (power(&edited[512..]) / power(&frames[4608..5120])).log10();
    assert!(edited_db < 1., "native base edits move the same held knobs: {edited_db}");

    let mut width_outputs = [[[0.; 2]; 1024]; 2];
    support::without_heap(|| {
        rt.set_engine_parameter(sampler_core::EngineParameterAddress {
            parameter: sampler_core::engine_parameter_id("ENGINE_PAR_FREQ2").unwrap(), group: 0, slot: 5, generic: -1 }, 400000).unwrap();
        for (n, width) in [0, 800000].into_iter().enumerate() {
            rt.set_engine_parameter(sampler_core::EngineParameterAddress {
                parameter: sampler_core::engine_parameter_id("ENGINE_PAR_BW2").unwrap(), group: 0, slot: 5, generic: -1 }, width).unwrap();
            rt.render(&mut width_outputs[n]).unwrap();
        }
    });
    let width_db = 10. * (power(&width_outputs[1][512..]) / power(&width_outputs[0][512..])).log10();
    assert!(width_db > 2., "wider bell boosts the off-center sine: {width_db}");

    let mut runtimes: Vec<_> = (0..3).map(|_| Runtime::new(
        lower(&instrument, 48000, vec![pcm.clone()], no_behaviors).unwrap(), limits()).unwrap()).collect();
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
            assert!((actual[c] - first[c] - second[c]).abs() < 2e-8, "EQ knob coefficients belong to each voice");
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
        post_amplitude: vec![ir::Processor::Compressor(ir::Compressor {
            threshold_db: -12.,
            ratio: 4.,
            attack: ir::Time::Seconds(0.01),
            release: ir::Time::Seconds(0.1),
            makeup: ir::Gain::Linear(1.),
            link: true,
        })],
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
                attack_shape: ir::Curve::Exponential(12.),
                release: ir::Time::Milliseconds(250.0013),
                ..Default::default()
            }),
        }],
        ..Default::default()
    };
    ir.source_indices.modulators.push(ir::SourceModulator {
        group: 0, slot: 7, external: false, name: "primary".into(),
        runtime: Some(ir::ModulatorRef(0)),
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
    generic.source_indices.ahdsrs[0].native_amplitude = false;
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
