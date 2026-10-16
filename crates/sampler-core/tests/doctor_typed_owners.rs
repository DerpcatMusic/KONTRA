//! Typed filter owners preserve public addressed targets, pure refusal and PCM.
use sampler_core::*;
mod support;

fn source_plan() -> Prepared {
    let pcm = (0..4096)
        .map(|i| [(i as f32 * 0.071).sin() * 0.125, (i as f32 * 0.037).cos() * -0.1])
        .collect::<Vec<_>>();
    Prepared::new(48_000, vec![Pcm::new(48_000, pcm.into_boxed_slice()).unwrap()], vec![Region {
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

// Phase C additions: exercise moved batch owners versus traced scalar views.
const C_NOTES: usize = 64;
fn phase_c_chain(scalar: bool, revision: bool) -> VoiceChain {
    let mut band = EqBand::rbj(sampler_ir::fx::eq::Kind::Peak,
        f64::from_bits(0x3fe0_0000_0000_0001), 0.3456789012345,
        if revision { -3.1234567898 } else { 9.1234567898 });
    band.law = sampler_ir::fx::eq::Law::KontaktLegacy;
    band.enabled = Parameter::Constant(f64::from_bits(0x3fe0_0000_0000_0001));
    let mut pre = vec![Processor::Gain(0.8), Processor::Eq(band),
        Processor::Color(ColorSettings {
            curve: CurveSettings::Classic(Parameter::Constant(0.3)),
            gainer: Some(GainerSettings { gain: Parameter::Constant(0.7), dry: 0.2 }),
            ..ColorSettings::IDENTITY
        }), svf(Parameter::Constant(if revision { 1700. } else { 900. }))];
    if scalar {
        // A valid scalar-only operation after the admitted Color/native EQ
        // requires preparation to retain all earlier owners and f64 constants.
        pre.push(Processor::ControlMatrix([Parameter::Constant(1.), Parameter::Constant(0.),
            Parameter::Constant(0.), Parameter::Constant(1.)]));
    }
    VoiceChain::new(pre, vec![Processor::StereoMatrix([[0.7, -0.1], [0.2, 0.8]])], 31).unwrap()
}
fn phase_c_plan(traced: bool, revision: bool) -> Prepared {
    let pcm = (0..4096).map(|i| [(i as f32 * 0.071).sin() * 0.01,
        (i as f32 * 0.037).cos() * -0.008]).collect::<Vec<_>>().into_boxed_slice();
    let regions = (0..3).map(|layer| Region {
        sample: 0, key_low: 60, key_high: 60, root_key: None,
        velocity_low: 0., velocity_high: 1., gain: 0.03 + layer as f32 * 0.01,
        envelope: Envelope::new(layer, 3, 31, 0.6, 19 + layer * 3).unwrap(),
        playback: Playback {
            loop_range: Some(Loop { start: 0, end: 4096, passes: None,
                shape: LoopShape::Wrap, mode: LoopMode::Continuous }), ..Playback::default()
        },
    }).collect();
    // Logical chain 0 is scalar. Dense batch 0 belongs to logical chain 1;
    // revision adds a distinct empty batch owner, shifting the retained key.
    let mut chains = vec![phase_c_chain(true, revision)];
    if revision { chains.push(VoiceChain::new(vec![], vec![], 0).unwrap()); }
    let batch = chains.len(); chains.push(phase_c_chain(false, revision));
    let p = Prepared::new(48_000, vec![Pcm::new(48_000, pcm).unwrap()], regions, 3).unwrap()
        .with_velocity_curves(vec![VelocityCurve::Constant; 3]).unwrap()
        .with_voice_chains(chains, vec![Some(batch), Some(batch), Some(0)]).unwrap();
    if traced { p.with_signal_trace(4096).unwrap() } else { p }
}

#[test]
fn phase_c_native_batch_constants_retain_exact_f64_trace_values() {
    let rt = Runtime::new(phase_c_plan(true, false),
        Limits::for_plan(&phase_c_plan(false, false), 4, 12)).unwrap();
    let reader = rt.signal_trace_reader().unwrap();
    let expected = [f64::from_bits(0x3fe0_0000_0000_0001), 0.3456789012345,
        9.1234567898, f64::from_bits(0x3fe0_0000_0000_0001)].map(f64::to_bits);
    let eqs = reader.graph.nodes.iter().filter(|node| node.processor == "eq").collect::<Vec<_>>();
    assert_eq!(eqs.len(), 3);
    for node in eqs {
        assert_eq!(node.parameters.iter().map(|p| p.initial.to_bits()).collect::<Vec<_>>(), expected);
    }
}

/// Tracing forces the scalar operation view. Compare it with real lane/worker
/// runs across scalar/batch chain holes, old/new generations, unequal releases,
/// one-voice runs and recycled slots. Heap guards cover the calling audio thread;
/// root retains the existing all-thread allocator/worker-witness matrix as well.
#[test]
fn phase_c_retained_batch_keys_match_scalar_pcm_across_plan_and_voice_lifecycles() {
    for threads in [1, 2] {
        let p = phase_c_plan(false, false);
        let limits = Limits::for_plan(&p, C_NOTES * 2 + 4, (C_NOTES * 2 + 4) * 3);
        let (actual, mut ac) = Runtime::with_plan_updates(p, limits, 2, 1).unwrap();
        let mut actual = actual.with_threads(Threads::Fixed(threads));
        let (mut expected, mut ec) = Runtime::with_plan_updates(phase_c_plan(true, false), limits, 2, 1).unwrap();
        support::without_heap(|| {
            for id in 0..C_NOTES as i32 {
                actual.trigger(input(id), 60, 0.75).unwrap();
                expected.trigger(input(id), 60, 0.75).unwrap();
            }
        });
        let mut a = [[0.; 2]; 64]; let mut b = a;
        for round in 0..16 {
            if round == 3 {
                let ar = ac.submit(Box::new(phase_c_plan(false, true))).unwrap();
                let er = ec.submit(Box::new(phase_c_plan(true, true))).unwrap();
                support::without_heap(|| {
                    assert_eq!(actual.poll_plan_update(), Ok(Some(ar)));
                    assert_eq!(expected.poll_plan_update(), Ok(Some(er)));
                    for id in C_NOTES as i32..(C_NOTES * 2) as i32 {
                        actual.trigger(input(id), 60, 0.65).unwrap();
                        expected.trigger(input(id), 60, 0.65).unwrap();
                    }
                });
            }
            support::without_heap(|| {
                if round == 6 {
                    for id in (0..(C_NOTES * 2) as i32).step_by(2) {
                        actual.note_off(input(id), None).unwrap(); expected.note_off(input(id), None).unwrap();
                    }
                }
                if round == 10 {
                    for id in (1..(C_NOTES * 2) as i32).step_by(2) {
                        actual.note_off(input(id), None).unwrap(); expected.note_off(input(id), None).unwrap();
                    }
                }
                let len = [1, 7, 64, 17, 63, 32][round % 6];
                actual.render(&mut a[..len]).unwrap(); expected.render(&mut b[..len]).unwrap();
                assert_eq!(a[..len], b[..len]);
                actual.flush_ended(|_| true); expected.flush_ended(|_| true);
            });
            assert_eq!(actual.voice_count(), expected.voice_count());
            assert_eq!(actual.note_count(), expected.note_count());
        }
        assert_eq!(actual.voice_count(), 0);
        if threads > 1 { assert!(actual.parallel_blocks() > 0); }
        support::without_heap(|| {
            assert_eq!(actual.collect_retired_plans(), 1);
            assert_eq!(expected.collect_retired_plans(), 1);
        });
        drop(ac.retired().unwrap()); drop(ec.retired().unwrap());
        support::without_heap(|| {
            actual.trigger(input(1000), 60, 0.5).unwrap(); expected.trigger(input(1000), 60, 0.5).unwrap();
            actual.render(&mut a).unwrap(); expected.render(&mut b).unwrap();
        });
        assert_eq!(a, b);
        for rt in [&actual, &expected] { assert_eq!(rt.stats().nonfinite_frames, 0); assert_eq!(rt.stats().stream_underruns, 0); }
    }
}

#[test]
fn phase_c_single_eligible_voice_uses_scalar_view_through_slot_reuse() {
    let make = |traced| {
        let p = source_plan().with_voice_chains(vec![phase_c_chain(false, false)], vec![Some(0)]).unwrap();
        if traced { p.with_signal_trace(4096).unwrap() } else { p }
    };
    let mut actual = Runtime::new(make(false), limits()).unwrap();
    let mut expected = Runtime::new(make(true), limits()).unwrap();
    let mut a = [[0.; 2]; 128]; let mut b = a;
    for id in 0..4 {
        support::without_heap(|| {
            actual.trigger(input(id), 60, 0.75).unwrap(); expected.trigger(input(id), 60, 0.75).unwrap();
        });
        assert_eq!(actual.voice_count(), 1);
        for len in [1, 7, 64, 63] {
            support::without_heap(|| {
                actual.render(&mut a[..len]).unwrap(); expected.render(&mut b[..len]).unwrap();
            });
            assert_eq!(a[..len], b[..len]);
        }
        support::without_heap(|| {
            actual.note_off(input(id), None).unwrap(); expected.note_off(input(id), None).unwrap();
            actual.render(&mut a).unwrap(); expected.render(&mut b).unwrap();
            actual.flush_ended(|_| true); expected.flush_ended(|_| true);
        });
        assert_eq!(a, b);
        assert_eq!(actual.voice_count(), 0);
        assert_eq!(expected.voice_count(), 0);
    }
}
