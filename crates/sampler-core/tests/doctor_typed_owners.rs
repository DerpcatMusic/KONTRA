//! Typed filter owners preserve public addressed targets, pure refusal and PCM.
use sampler_core::*;
mod support;

fn source_plan() -> Prepared {
    let pcm = (0..4096)
        .map(|i| [(i as f32 * 0.071).sin() * 0.125, (i as f32 * 0.037).cos() * -0.1])
        .collect::<Vec<_>>();
    Prepared::new(48_000, vec![Pcm::new(48_000, pcm).unwrap()], vec![Region {
        sample: 0, key_low: 60, key_high: 60, root_key: None,
        velocity_low: 0., velocity_high: 1., gain: 0.75,
        envelope: Envelope::new(7, 3, 31, 0.6, 19).unwrap(),
        playback: Playback {
            loop_range: Some(Loop { start: 0, end: 4096, passes: None,
                shape: LoopShape::Wrap, mode: LoopMode::Continuous }),
            ..Playback::default()
        },
    }], 1).unwrap().with_velocity_curves(vec![VelocityCurve::Constant]).unwrap()
}
fn svf(cutoff_hz: Parameter) -> Processor {
    Processor::StateVariable(StateVariableFilter {
        mode: SvfMode::LowPass, cutoff_hz, q: Parameter::Constant(0.7),
    })
}
fn native() -> Processor {
    Processor::Filter(FilterSettings::Ladder(LadderSettings {
        cutoff: Parameter::Constant(0.4), resonance: Parameter::Constant(0.2),
        gain: Parameter::Constant(0.), record_version: 99, taps: 4,
    }))
}
fn voice_stages() -> Vec<Processor> {
    vec![native(), svf(Parameter::Constant(700.)), native(),
        svf(Parameter::Expression { source: ExpressionSource::Pressure, low: 400., high: 4000. }),
        svf(Parameter::Constant(2600.))]
}
fn with_chain(plan: Prepared, stages: Vec<Processor>) -> Prepared {
    plan.with_voice_chains(vec![VoiceChain::new(vec![], stages, 0).unwrap()], vec![Some(0)]).unwrap()
}
fn program(target: ModTarget) -> ModProgram {
    ModProgram { sources: vec![ModSource::Constant], routes: vec![ModRoute::new(0, target, 6.)],
        ..Default::default() }
}
fn with_target(plan: Prepared, target: ModTarget) -> Result<Prepared, Error> {
    plan.with_voice_modulation(vec![program(target)], vec![Some(0)], vec![0])
}

/// Interleaving must not renumber the public address space. Wrong-kind and
/// out-of-range targets fail on the control side before a runtime exists.
#[test]
fn interleaved_filter_target_admission_keeps_public_addresses() {
    for index in 0..=5 {
        let is_svf = [1, 3, 4].contains(&index);
        let is_native = [0, 2].contains(&index);
        for (target, accepted) in [
            (ModTarget::ProcessorCutoff(index), is_svf),
            (ModTarget::ProcessorResonance(index), is_svf),
            (ModTarget::ProcessorNativeCutoff(index), is_native),
            (ModTarget::ProcessorNativeResonance(index), is_native),
            (ModTarget::ProcessorNativeGain(index), is_native),
        ] {
            let result = with_target(with_chain(source_plan(), voice_stages()), target);
            if accepted { assert!(result.is_ok(), "{target:?}"); }
            else { assert!(matches!(result, Err(Error::InvalidInput)), "{target:?}"); }
        }
    }
}

#[test]
fn interleaved_topology_replacement_rejects_wrong_kind_and_removed_targets() {
    for target in [ModTarget::ProcessorCutoff(3), ModTarget::ProcessorResonance(3)] {
        let initial = with_target(with_chain(source_plan(), voice_stages()), target).unwrap();
        let mut replaced = voice_stages();
        replaced[3] = native();
        assert!(matches!(initial.with_voice_chains(
            vec![VoiceChain::new(vec![], replaced, 0).unwrap()], vec![Some(0)]),
            Err(Error::InvalidInput)));
    }
    let initial = with_target(with_chain(source_plan(), voice_stages()),
        ModTarget::ProcessorNativeCutoff(2)).unwrap();
    let mut replaced = voice_stages();
    replaced[2] = svf(Parameter::Constant(900.));
    assert!(matches!(initial.with_voice_chains(
        vec![VoiceChain::new(vec![], replaced, 0).unwrap()], vec![Some(0)]),
        Err(Error::InvalidInput)));
    let initial = with_target(with_chain(source_plan(), voice_stages()),
        ModTarget::ProcessorCutoff(4)).unwrap();
    assert!(matches!(initial.with_voice_chains(vec![], vec![None]), Err(Error::InvalidInput)));
}

fn plan(traced: bool) -> Prepared {
    let p = with_target(with_chain(source_plan(), voice_stages()),
        ModTarget::ProcessorCutoff(4)).unwrap()
        .with_buses(vec![Bus {
            processors: vec![native(), svf(Parameter::Constant(1100.)), native(),
                svf(Parameter::Constant(3100.))],
            sends: vec![BusSend { bus: None, gain: 1. }], tail_frames: 0,
        }], vec![Some(0)]).unwrap();
    if traced { p.with_signal_trace(8192).unwrap() } else { p }
}
fn limits() -> Limits {
    Limits { notes: 4, channels: 1, performances: 1, families: 4,
        expressions: 4, voices: 4, decisions: 4, commands: 4,
        behaviors: 0, behavior_fuel: 0, behavior_cells: 0, note_cells: 0 }
}
fn input(id: i32) -> Input {
    Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60,
        external_id: Some(id) }
}

/// Voice and bus registries have independent dense indices. Tracing must select
/// each authored SVF design, preserve PCM, and reuse expression slots without
/// callback allocation/free during trigger, expression edits, release or render.
#[test]
fn interleaved_voice_bus_trace_and_expression_reuse_preserve_pcm_without_heap() {
    let mut actual = Runtime::new(plan(true), limits()).unwrap();
    let mut expected = Runtime::new(plan(false), limits()).unwrap();
    let reader = actual.signal_trace_reader().unwrap();
    let cutoffs = |kind| reader.graph.nodes.iter()
        .filter(|node| node.kind == kind && node.processor == "state_variable_filter")
        .map(|node| node.parameters.iter().find(|p| p.name == "cutoff_hz").unwrap().initial)
        .collect::<Vec<_>>();
    assert_eq!(cutoffs("group_fx_post"), [700., 400., 2600.]);
    assert_eq!(cutoffs("bus_fx"), [1100., 3100.]);
    let mut a = [[0.; 2]; 128];
    let mut b = a;
    let mut nonzero = false;
    let mut previous_a = None;
    let mut previous_b = None;
    for cycle in 0..4 {
        let (mut current_a, mut current_b) = (None, None);
        support::without_heap(|| {
            let note = actual.trigger(input(cycle), 60, 1.).unwrap();
            let expression = actual.expression_id(note).unwrap();
            current_a = Some(expression);
            actual.set_expression(expression, Expression {
                pressure: if cycle % 2 == 0 { 0 } else { u32::MAX }, ..Default::default()
            }).unwrap();
            let note = expected.trigger(input(cycle), 60, 1.).unwrap();
            let expression = expected.expression_id(note).unwrap();
            current_b = Some(expression);
            expected.set_expression(expression, Expression {
                pressure: if cycle % 2 == 0 { 0 } else { u32::MAX }, ..Default::default()
            }).unwrap();
        });
        if let Some(id) = previous_a {
            assert!(matches!(actual.expression(id), Err(Error::StaleHandle)));
        }
        if let Some(id) = previous_b {
            assert!(matches!(expected.expression(id), Err(Error::StaleHandle)));
        }
        for n in [1, 7, 64, 17, 128, 63] {
            support::without_heap(|| {
                actual.render(&mut a[..n]).unwrap();
                expected.render(&mut b[..n]).unwrap();
            });
            assert_eq!(a[..n], b[..n]);
            nonzero |= a[..n].iter().flatten().any(|v| *v != 0.);
        }
        support::without_heap(|| {
            actual.note_off(input(cycle), None).unwrap();
            expected.note_off(input(cycle), None).unwrap();
            actual.render(&mut a).unwrap();
            expected.render(&mut b).unwrap();
            actual.flush_ended(|_| true);
            expected.flush_ended(|_| true);
        });
        assert_eq!(a, b);
        assert_eq!(actual.voice_count(), 0);
        assert_eq!(expected.voice_count(), 0);
        previous_a = current_a;
        previous_b = current_b;
    }
    assert!(nonzero);
    for runtime in [&actual, &expected] {
        assert_eq!(runtime.stats().nonfinite_frames, 0);
        assert_eq!(runtime.stats().stream_underruns, 0);
    }
    assert_eq!(reader.dropped(), 0);
}

/// Run the same public fixture on baseline and candidate under root's lock to
/// record total control-side requested bytes. This is a receipt, not a guessed
/// ABI assertion; callback allocation/free is checked separately above.
#[test]
fn typed_filter_owner_preparation_allocation_receipt() {
    let mut prepared = None;
    let prepared_bytes = support::allocated_bytes(|| prepared = Some(plan(false)));
    let mut runtime = None;
    let runtime_bytes = support::allocated_bytes(|| {
        runtime = Some(Runtime::new(prepared.take().unwrap(), limits()).unwrap());
    });
    eprintln!("typed owner fixture requested bytes: preparation={prepared_bytes} runtime={runtime_bytes}");
    assert!(runtime.is_some());
}


/// A summed bus has no unique expression owner. Dense preparation must retain
/// this refusal even when a native address precedes the expressive SVF.
#[test]
fn interleaved_bus_rejects_expression_owned_svf_before_publication() {
    let result = source_plan().with_buses(vec![Bus {
        processors: vec![native(), svf(Parameter::Expression {
            source: ExpressionSource::Pressure, low: 400., high: 4000.,
        })],
        sends: vec![BusSend { bus: None, gain: 1. }], tail_frames: 0,
    }], vec![Some(0)]);
    assert!(matches!(result, Err(Error::InvalidInput)));
}
