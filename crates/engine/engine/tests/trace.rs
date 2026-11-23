//! The engine's bounded trace of voices and route evaluations.
#![cfg(feature = "trace")]

mod common;

use std::sync::Arc;

use kontra_engine::ir::{self};
use kontra_engine::{Engine, Plan, Timed, Trigger};
use kontra_stream::{Store, StoreConfig};

use common::*;

#[cfg(feature = "trace")]
#[test]
fn bounded_trace_captures_voice_start_steal_and_end_without_changing_audio() {
    use kontra_engine::{TraceEvent, VoiceEvent};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let plan = Arc::new(Plan::new(vec![zone(&mut store, 0.5)], 1));
    let mut plain = Engine::new(Arc::clone(&plan), store.client(), RATE);
    let mut traced = Engine::new_traced(plan, store.client(), RATE, 128);
    for input in [
        &[on(60, 1.0)][..],
        &[on(62, 1.0)][..],
        &[off(62)][..],
        &[][..],
        &[][..],
        &[][..],
    ] {
        let mut a = [[0.0; 2]; 128];
        let mut b = [[0.0; 2]; 128];
        plain.process(&mut a, input);
        traced.process(&mut b, input);
        assert_eq!(a, b, "tracing changed the rendered samples");
    }
    let transitions: Vec<_> = traced
        .trace_events()
        .iter()
        .filter_map(|event| match event {
            TraceEvent::Voice { event, .. } => Some(*event),
            _ => None,
        })
        .collect();
    assert!(transitions.contains(&VoiceEvent::Start), "{transitions:?}");
    assert!(transitions.contains(&VoiceEvent::Steal), "{transitions:?}");
    assert!(transitions.contains(&VoiceEvent::End), "{transitions:?}");
    assert_eq!(traced.trace_dropped(), 0);
}

#[cfg(feature = "trace")]
#[test]
fn note_on_route_trace_keeps_the_controller_value_at_start() {
    use kontra_engine::TraceEvent;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut z = zone(&mut store, 0.5);
    z.start_mod = 1000;
    let mut plan = Plan::new(vec![z], 8);
    plan.groups[0].mods = vec![ir::Mod {
        name: "start".into(),
        slot: 3,
        target: 0,
        bypass: false,
        route: ir::ModRoute::new(ir::Source::Controller(1), ir::ParamAddr::SampleStart, 1.0),
    }];
    let mut engine = Engine::new_traced(Arc::new(plan), store.client(), RATE, 128);
    level(&mut engine, &[cc(1, 64), on(60, 1.0)]);
    level(&mut engine, &[cc(1, 127)]);
    let source = engine
        .trace_events()
        .iter()
        .find_map(|event| match event {
            TraceEvent::RouteOnset {
                mod_slot: 3,
                source,
                ..
            } => *source,
            _ => None,
        })
        .unwrap();
    assert!((source - 64.0 / 127.0).abs() < 0.001, "{source}");
}

#[cfg(feature = "trace")]
#[test]
fn release_route_trace_uses_the_key_held_time() {
    use kontra_engine::TraceEvent;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut z = zone(&mut store, 0.5);
    z.trigger = Trigger::Release;
    z.start_mod = 1000;
    let mut plan = Plan::new(vec![z], 8);
    plan.groups[0].mods = vec![ir::Mod {
        name: "held".into(),
        slot: 7,
        target: 0,
        bypass: false,
        route: ir::ModRoute::new(
            ir::Source::HeldTime { span_ms: 1000 },
            ir::ParamAddr::SampleStart,
            1.0,
        ),
    }];
    let mut engine = Engine::new_traced(Arc::new(plan), store.client(), RATE, 128);
    let mut output = vec![[0.0; 2]; 24_000];
    engine.process(&mut output, &[on(60, 1.0)]);
    level(&mut engine, &[off(60)]);
    let source = engine
        .trace_events()
        .iter()
        .find_map(|event| match event {
            TraceEvent::RouteOnset {
                mod_slot: 7,
                source,
                ..
            } => *source,
            _ => None,
        })
        .unwrap();
    assert!((source - 0.5).abs() < 0.001, "{source}");
}

/// A shaper over the release-trigger counter (Pacific and Dolce move their
/// release samples' start this way) reads the held time like a bare route.
#[cfg(feature = "trace")]
#[test]
fn shaped_release_counter_reads_the_key_held_time() {
    use kontra_engine::TraceEvent;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut z = zone(&mut store, 0.5);
    z.trigger = Trigger::Release;
    z.start_mod = 1000;
    let mut plan = Plan::new(vec![z], 8);
    plan.groups[0].shapers = vec![ir::Transfer {
        source: ir::Source::HeldTime { span_ms: 1000 },
        table: vec![0.2, 1.0],
        ..Default::default()
    }];
    plan.groups[0].mods = vec![ir::Mod {
        name: "held".into(),
        slot: 7,
        target: 0,
        bypass: false,
        route: ir::ModRoute::new(ir::Source::Shaped(0), ir::ParamAddr::SampleStart, 1.0),
    }];
    let mut engine = Engine::new_traced(Arc::new(plan), store.client(), RATE, 128);
    let mut output = vec![[0.0; 2]; 24_000];
    engine.process(&mut output, &[on(60, 1.0)]);
    level(&mut engine, &[off(60)]);
    let source = engine
        .trace_events()
        .iter()
        .find_map(|event| match event {
            TraceEvent::RouteOnset {
                mod_slot: 7,
                source,
                ..
            } => *source,
            _ => None,
        })
        .expect("the shaped counter has a value at the release voice's start");
    // Held 0.5 s of a 1 s span: counter 0.5, through 0.2..1.0 is 0.6.
    assert!((source - 0.6).abs() < 0.01, "{source}");
}

/// A shaper keeps its source's polarity: pitch bend at rest through a flat
/// 0.5 table (Pacific's release start route) is 0, not half the depth.
#[cfg(feature = "trace")]
#[test]
fn voice_snapshot_reports_the_playing_envelope_and_pitch() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    plan.groups[0].envelope.release = RATE;
    let mut engine = Engine::new_traced(Arc::new(plan), store.client(), RATE, 128);
    level(&mut engine, &[on(60, 1.0)]);
    let before = engine.inspect_voices().next().unwrap();
    assert_eq!(before.envelope_stage, "sustain");
    assert!((before.step - 1.0).abs() < 0.0001);
    level(&mut engine, &[off(60)]);
    let after = engine.inspect_voices().next().unwrap();
    assert_eq!(after.envelope_stage, "release");
    assert!(after.last_peak > 0.0);
}

#[cfg(feature = "trace")]
#[test]
fn controller_route_trace_is_the_value_used_and_preserves_audio() {
    use kontra_engine::RouteUnit;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    plan.groups[0].mods = vec![
        ir::Mod {
            name: "volume".into(),
            slot: 1,
            target: 0,
            bypass: false,
            route: ir::ModRoute::new(ir::Source::Controller(1), ir::ParamAddr::Volume, 1.0)
                .law(ir::RouteLaw::Gain(ir::Curve::Depth { power: 1 })),
        },
        ir::Mod {
            name: "pan".into(),
            slot: 2,
            target: 0,
            bypass: false,
            route: ir::ModRoute::new(ir::Source::Controller(1), ir::ParamAddr::Pan, 0.5),
        },
        ir::Mod {
            name: "pitch".into(),
            slot: 3,
            target: 0,
            bypass: false,
            route: ir::ModRoute::new(ir::Source::Controller(1), ir::ParamAddr::Pitch, 1.0)
                .span(12.0),
        },
    ];
    let plan = Arc::new(plan);
    let mut normal = Engine::new(Arc::clone(&plan), store.client(), RATE);
    let mut traced = Engine::new_traced(plan, store.client(), RATE, 128);
    for events in [&[on(60, 1.0)][..], &[cc(1, 64)][..]] {
        let mut a = [[0.0; 2]; 256];
        let mut b = [[0.0; 2]; 256];
        let input: Vec<_> = events
            .iter()
            .map(|e| Timed {
                at: normal.now(),
                body: e.body,
            })
            .collect();
        normal.process(&mut a, &input);
        traced.process(&mut b, &input);
        assert_eq!(a, b, "observer changed the audio");
    }
    let routes: Vec<_> = traced
        .trace_route_values()
        .filter(|v| v.controller == 1 && !v.timed_refresh)
        .collect();
    let input = 64.0 / 127.0;
    assert!(
        routes
            .iter()
            .any(|v| matches!(v.unit, RouteUnit::GainMultiplier)
                && (v.value - input).abs() < 1e-5
                && v.direct_controller_source)
    );
    assert!(routes.iter().any(|v| matches!(v.unit, RouteUnit::Pan)
        && (v.value - input * 0.5).abs() < 1e-5
        && v.direct_controller_source));
    assert!(routes.iter().any(|v| matches!(v.unit, RouteUnit::Semitones)
        && (v.value - input * 12.0).abs() < 1e-5
        && v.direct_controller_source));
    assert_eq!(traced.trace_dropped(), 0);
}

#[cfg(feature = "trace")]
#[test]
fn timed_lfo_route_retains_all_ccs_received_before_the_span() {
    use kontra_engine::RouteUnit;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    plan.groups[0].lfos.push(ir::Lfo {
        wave: ir::Wave::Sine,
        rate: ir::Rate::Hz(2.0),
        phase: 0.0,
        fade_ms: 0.0,
        clock: ir::Clock::Voice,
    });
    plan.groups[0].mods = vec![ir::Mod {
        name: "rate".into(),
        slot: 9,
        target: 0,
        bypass: false,
        route: ir::ModRoute::new(
            ir::Source::Controller(1),
            ir::ParamAddr::LfoParam {
                lfo: 0,
                param: ir::LfoParam::Hz,
            },
            1.0,
        )
        .law(ir::RouteLaw::Range([1.0, 5.0])),
    }];
    plan.groups[0].mods.push(ir::Mod {
        name: "rate11".into(),
        slot: 10,
        target: 0,
        bypass: false,
        route: ir::ModRoute::new(
            ir::Source::Controller(11),
            ir::ParamAddr::LfoParam {
                lfo: 0,
                param: ir::LfoParam::Hz,
            },
            0.5,
        )
        .law(ir::RouteLaw::Range([1.0, 5.0])),
    });
    let mut engine = Engine::new_traced(Arc::new(plan), store.client(), RATE, 128);
    level(&mut engine, &[on(60, 1.0)]);
    level(&mut engine, &[cc(1, 64), cc(11, 127)]);
    let routes: Vec<_> = engine
        .trace_route_values()
        .filter(|v| matches!(v.unit, RouteUnit::LfoHertz))
        .collect();
    assert!(routes.iter().any(|v| v.controller == 1
        && v.timed_refresh
        && v.direct_controller_source
        && (v.value - (64.0 / 127.0 * 4.0)).abs() < 1e-5));
    assert!(routes.iter().any(|v| v.controller == 11
        && v.timed_refresh
        && v.direct_controller_source
        && v.mod_slot == 10
        && (v.value - 2.0).abs() < 1e-5));
}

#[cfg(feature = "trace")]
#[test]
fn onset_route_trace_records_sample_and_envelope_values() {
    use kontra_engine::RouteUnit;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut z = zone(&mut store, 0.5);
    z.start_mod = 1000;
    let mut plan = Plan::new(vec![z], 8);
    plan.groups[0].envelope.attack = 4800;
    plan.groups[0].mods = vec![
        ir::Mod {
            name: "offset".into(),
            slot: 4,
            target: 0,
            bypass: false,
            route: ir::ModRoute::new(ir::Source::Controller(1), ir::ParamAddr::SampleStart, 0.5),
        },
        ir::Mod {
            name: "attack".into(),
            slot: 5,
            target: 0,
            bypass: false,
            route: ir::ModRoute::new(
                ir::Source::Controller(1),
                ir::ParamAddr::EnvelopeStage {
                    envelope: None,
                    stage: ir::EnvStage::Attack,
                },
                1.0,
            )
            .law(ir::RouteLaw::Time(ir::TimeScale::Linear { power: 1 })),
        },
    ];
    let mut engine = Engine::new_traced(Arc::new(plan), store.client(), RATE, 128);
    level(&mut engine, &[cc(1, 64), on(60, 1.0)]);
    let routes: Vec<_> = engine
        .trace_route_values()
        .filter(|v| v.note_onset && v.controller == 1)
        .collect();
    let x = 64.0 / 127.0;
    assert!(
        routes
            .iter()
            .any(|v| matches!(v.unit, RouteUnit::SampleStartFraction)
                && (v.value - x * 0.5).abs() < 1e-5
                && v.voice_slot != usize::MAX)
    );
    assert!(
        routes
            .iter()
            .any(|v| matches!(v.unit, RouteUnit::EnvelopeFrames)
                && (v.value - (4800.0 * x).round()).abs() < 1.0
                && v.voice_slot != usize::MAX)
    );
    assert_eq!(engine.trace_dropped(), 0);
}

#[cfg(feature = "trace")]
#[test]
fn script_controller_callback_links_indirect_route_evaluations() {
    use kontra_engine::{
        Action, Callbacks, Controls, EnginePars, Op, Program, RouteUnit, ScriptPlan,
    };
    use kontra_vm::Returns;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    plan.groups[0].mods = vec![ir::Mod {
        name: "velocity".into(),
        slot: 2,
        target: 0,
        bypass: false,
        route: ir::ModRoute::new(ir::Source::Velocity, ir::ParamAddr::Volume, 1.0)
            .law(ir::RouteLaw::Gain(ir::Curve::Depth { power: 1 })),
    }];
    plan.scripts.push(ScriptPlan {
        program: Program {
            code: vec![
                Op::Int(0),
                Op::Int(-6000),
                Op::Host {
                    action: Action::GroupVolume,
                    ints: 2,
                    texts: 0,
                    returns: Returns::Nothing,
                },
                Op::Exit,
            ],
            max_stack: 4,
            ..Program::default()
        },
        callbacks: Callbacks {
            controller: Some(0),
            ..Callbacks::default()
        },
        coroutines: 4,
        data: vec![],
        text_data: vec![],
        source_folder: None,
        library_folder: None,
        rc_folder: None,
        factory_folder: None,
        restore: vec![],
        restore_texts: vec![],
        controls: Controls::default(),
        engine_pars: EnginePars::default(),
        view: None,
        links: vec![],
        reverb_laws: ir::ReverbControls::from_fn(|_| ir::Law::Linear { lo: 0.0, hi: 1.0 }),
        reverb_networks: vec![],
        lfo_units: vec![],
        warp_modes: vec![],
        mod_kinds: vec![],
        knobs: vec![],
        foreign: None,
        files: None,
        arrays: Vec::new(),
        hears_own_releases: false,
        held_release_ends_children: false,
    });
    let mut engine = Engine::new_traced(Arc::new(plan), store.client(), RATE, 256);
    level(&mut engine, &[on(60, 1.0)]);
    engine.clear_trace();
    level(&mut engine, &[cc(1, 64)]);
    assert!(engine.trace_route_values().any(|v| v.controller == 1
        && v.script_callback.is_some()
        && !v.direct_controller_source
        && !v.controller_can_affect
        && matches!(v.unit, RouteUnit::GainMultiplier)));
}
