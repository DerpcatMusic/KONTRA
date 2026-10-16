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

// Phase D: public heterogeneous schema, authoritative homogeneous runtime cells.
fn phase_d_definition(id: u128, domain: ControlDomain, default: ControlValue) -> ControlDefinition {
    ControlDefinition {
        id: ControlId(id),
        domain,
        default,
    }
}
fn phase_d_schema() -> Vec<ControlDefinition> {
    vec![
        phase_d_definition(
            10,
            ControlDomain::Integer {
                min: i64::MIN,
                max: i64::MAX,
            },
            ControlValue::Integer(i64::MAX),
        ),
        phase_d_definition(
            20,
            ControlDomain::Real {
                min: -f64::MAX,
                max: f64::MAX,
            },
            ControlValue::Real(-0.),
        ),
        phase_d_definition(30, ControlDomain::Toggle, ControlValue::Toggle(true)),
    ]
}

#[test]
fn phase_d_all_domains_keep_bits_and_late_refusal_before_valid_snapshot_retry_without_heap() {
    let p = source_plan().with_controls(phase_d_schema()).unwrap();
    let mut rt = Runtime::new(p, limits()).unwrap();
    let owner = rt.active_plan();
    let writes = [
        ControlWrite {
            id: ControlId(10),
            value: ControlValue::Integer(i64::MIN),
        },
        ControlWrite {
            id: ControlId(20),
            value: ControlValue::Real(f64::from_bits(1)),
        },
        ControlWrite {
            id: ControlId(30),
            value: ControlValue::Toggle(false),
        },
    ];
    let mut snapshot = [writes[0]; 3];
    let mut state = ScriptStateBuffer {
        values: writes
            .iter()
            .map(|w| ScriptStateEntry {
                address: ScriptStateAddress::Control(w.id),
                value: ScriptStateValue::Control(w.value),
            })
            .collect(),
        ..Default::default()
    };
    support::without_heap(|| {
        rt.capture_controls(owner, &mut snapshot).unwrap();
        assert!(
            matches!(snapshot[1].value,ControlValue::Real(value) if value.to_bits()==(-0.0f64).to_bits())
        );
        for (row, wrong) in [
            (0, ControlValue::Real(0.)),
            (0, ControlValue::Toggle(false)),
            (1, ControlValue::Integer(0)),
            (1, ControlValue::Toggle(false)),
            (1, ControlValue::Real(f64::NAN)),
            (1, ControlValue::Real(f64::INFINITY)),
            (2, ControlValue::Integer(0)),
            (2, ControlValue::Real(0.)),
        ] {
            let mut invalid = writes;
            invalid[row].value = wrong;
            assert_eq!(
                rt.edit_controls(owner, Some(0), &invalid),
                Err(Error::InvalidInput)
            );
            assert_eq!(rt.control_revision(owner), Ok(0));
            assert_eq!(
                rt.control_value(owner, ControlId(10)),
                Ok(ControlValue::Integer(i64::MAX))
            );
            assert!(
                matches!(rt.control_value(owner,ControlId(20)),Ok(ControlValue::Real(v)) if v.to_bits()==(-0.0f64).to_bits())
            );
            assert_eq!(
                rt.control_value(owner, ControlId(30)),
                Ok(ControlValue::Toggle(true))
            );
            // The ordinary snapshot path resolves the same typed row operation.
            state.values[row].value = ScriptStateValue::Control(wrong);
            assert_eq!(
                rt.restore_script_state(owner, Some(0), &mut state),
                Err(Error::InvalidInput)
            );
            assert_eq!(rt.control_revision(owner), Ok(0));
            assert_eq!(
                rt.control_value(owner, ControlId(10)),
                Ok(ControlValue::Integer(i64::MAX))
            );
            assert!(
                matches!(rt.control_value(owner,ControlId(20)),Ok(ControlValue::Real(v)) if v.to_bits()==(-0.0f64).to_bits())
            );
            assert_eq!(
                rt.control_value(owner, ControlId(30)),
                Ok(ControlValue::Toggle(true))
            );
            state.values[row].value = ScriptStateValue::Control(writes[row].value);
        }
        assert_eq!(
            rt.restore_script_state(owner, Some(0), &mut state),
            Ok((3, 1))
        );
        rt.capture_controls(owner, &mut snapshot).unwrap();
        assert_eq!(snapshot, writes);
        assert!(matches!(snapshot[1].value,ControlValue::Real(value) if value.to_bits()==1));
        assert_eq!(rt.recall_controls(owner, Some(1), &snapshot), Ok(2));
        assert_eq!(
            rt.control_base_value(owner, ControlId(10)),
            Ok(ControlValue::Integer(i64::MIN))
        );
    });
}

const D_DEPTH: ControlId = ControlId(100);
const D_BYPASS: ControlId = ControlId(200);
fn phase_d_live_schema(extra: bool) -> Vec<ControlDefinition> {
    let mut defs = vec![
        phase_d_definition(
            100,
            ControlDomain::Real { min: -1., max: 1. },
            ControlValue::Real(0.125),
        ),
        phase_d_definition(
            200,
            ControlDomain::Real { min: 0., max: 1. },
            ControlValue::Real(0.),
        ),
    ];
    if extra {
        defs.extend([
            phase_d_definition(
                25,
                ControlDomain::Integer { min: 0, max: 10 },
                ControlValue::Integer(9),
            ),
            phase_d_definition(
                50,
                ControlDomain::Real { min: 0., max: 1. },
                ControlValue::Real(0.875),
            ),
            phase_d_definition(150, ControlDomain::Toggle, ControlValue::Toggle(true)),
        ]);
    }
    defs.reverse();
    defs
}
fn phase_d_live_plan(target: ModTarget, replaced: bool) -> Prepared {
    let mut p = source_plan()
        .with_controls(phase_d_live_schema(!replaced))
        .unwrap()
        .with_voice_chains(
            vec![VoiceChain::new(vec![Processor::Gain(1.)], vec![], 0).unwrap()],
            vec![Some(0)],
        )
        .unwrap()
        .with_voice_modulation(
            vec![ModProgram {
                controls: vec![(0, [D_DEPTH, D_BYPASS])],
                sources: vec![ModSource::Constant],
                routes: vec![ModRoute::new(0, target, 0.125)],
                ..Default::default()
            }],
            vec![Some(0)],
            vec![0],
        )
        .unwrap();
    if replaced {
        // Insert another Real lane, plus non-Real lanes. Both physical and logical
        // positions change; a second substitution uses the immediately prior map.
        p = p.with_controls(phase_d_live_schema(true)).unwrap();
        let reversed = p.controls().iter().rev().copied().collect();
        p = p.with_controls(reversed).unwrap();
    }
    p
}

/// This additionally catches the original loop_routes remapping omission.
#[test]
fn phase_d_live_real_handles_follow_replacements_for_pan_and_both_loop_lanes_without_heap() {
    for target in [ModTarget::Pan, ModTarget::LoopStart, ModTarget::LoopLength] {
        for threads in [1, 2] {
            let p = phase_d_live_plan(target, true);
            let cap = Limits::for_plan(&p, 20, 20);
            let mut actual = Runtime::new(p, cap)
                .unwrap()
                .with_threads(Threads::Fixed(threads));
            let mut expected = Runtime::new(phase_d_live_plan(target, false), cap)
                .unwrap()
                .with_threads(Threads::Fixed(threads));
            let mut a = [[0.; 2]; 137];
            let mut b = a;
            let mut nonzero = false;
            support::without_heap(|| {
                for id in 0..16 {
                    actual.trigger(input(id), 60, 0.75).unwrap();
                    expected.trigger(input(id), 60, 0.75).unwrap();
                }
                for round in 0..24 {
                    if round == 6 || round == 12 || round == 18 {
                        let w = if round == 12 {
                            ControlWrite {
                                id: D_BYPASS,
                                value: ControlValue::Real(1.),
                            }
                        } else if round == 18 {
                            ControlWrite {
                                id: D_BYPASS,
                                value: ControlValue::Real(0.),
                            }
                        } else {
                            ControlWrite {
                                id: D_DEPTH,
                                value: ControlValue::Real(-0.25),
                            }
                        };
                        actual
                            .edit_controls(actual.active_plan(), None, &[w])
                            .unwrap();
                        expected
                            .edit_controls(expected.active_plan(), None, &[w])
                            .unwrap();
                    }
                    let n = [1, 7, 64, 137][round % 4];
                    actual.render(&mut a[..n]).unwrap();
                    expected.render(&mut b[..n]).unwrap();
                    assert_eq!(a[..n], b[..n], "{target:?} round {round}");
                    nonzero |= a[..n].iter().flatten().any(|v| *v != 0.);
                }
            });
            assert!(nonzero);
            assert_eq!(actual.stats().nonfinite_frames, 0);
        }
    }
}

fn phase_d_engine_address(group: i32) -> EngineParameterAddress {
    EngineParameterAddress {
        parameter: engine_parameter_id("ENGINE_PAR_GAIN").unwrap(),
        group,
        slot: 3,
        generic: -1,
    }
}

/// Non-Real bindings are historically accepted by with_engine_parameters. Their
/// offsets still advance the revision and restart the same DSP ramp, but never
/// turn their integer/toggle values into fabricated Real values.
#[test]
fn phase_d_engine_offsets_preserve_non_real_admission_and_last_real_binding_law() {
    let defs = vec![
        phase_d_definition(
            10,
            ControlDomain::Integer { min: -9, max: 9 },
            ControlValue::Integer(7),
        ),
        phase_d_definition(
            20,
            ControlDomain::Real { min: -1., max: 1. },
            ControlValue::Real(-0.25),
        ),
        phase_d_definition(30, ControlDomain::Toggle, ControlValue::Toggle(true)),
    ];
    let bindings = vec![
        EngineParameterBinding {
            address: phase_d_engine_address(0),
            control: ControlId(10),
            law: EngineParameterLaw::Linear { low: 0., high: 1. },
        },
        EngineParameterBinding {
            address: phase_d_engine_address(1),
            control: ControlId(20),
            law: EngineParameterLaw::Linear { low: -1., high: 1. },
        },
        EngineParameterBinding {
            address: phase_d_engine_address(2),
            control: ControlId(20),
            law: EngineParameterLaw::SignedNormalized,
        },
        EngineParameterBinding {
            address: phase_d_engine_address(3),
            control: ControlId(30),
            law: EngineParameterLaw::Linear { low: 0., high: 1. },
        },
    ];
    let p = source_plan()
        .with_controls(defs)
        .unwrap()
        .with_engine_parameters(bindings, vec![])
        .unwrap();
    let mut rt = Runtime::new(p, limits()).unwrap();
    let owner = rt.active_plan();
    let offsets = [
        EngineParameterOffset {
            address: phase_d_engine_address(0),
            offset: 0.1,
        },
        EngineParameterOffset {
            address: phase_d_engine_address(1),
            offset: 0.2,
        },
        EngineParameterOffset {
            address: phase_d_engine_address(2),
            offset: -0.125,
        },
        EngineParameterOffset {
            address: phase_d_engine_address(3),
            offset: 0.3,
        },
    ];
    support::without_heap(|| {
        rt.set_engine_offsets(&offsets).unwrap();
        assert_eq!(rt.control_revision(owner), Ok(1));
        assert_eq!(
            rt.control_value(owner, ControlId(10)),
            Ok(ControlValue::Integer(7))
        );
        assert_eq!(
            rt.control_value(owner, ControlId(30)),
            Ok(ControlValue::Toggle(true))
        );
        let law = EngineParameterLaw::SignedNormalized;
        let expected = law.decode(law.encode(-0.25) - 125000).clamp(-1., 1.);
        assert_eq!(
            rt.control_value(owner, ControlId(20)),
            Ok(ControlValue::Real(expected))
        );
        assert_eq!(
            rt.control_base_value(owner, ControlId(20)),
            Ok(ControlValue::Real(-0.25))
        );
        let mut bad = offsets;
        bad[3].offset = f32::NAN;
        assert_eq!(rt.set_engine_offsets(&bad), Err(Error::InvalidInput));
        assert_eq!(rt.control_revision(owner), Ok(1));
        rt.set_engine_offsets(&[]).unwrap();
        assert_eq!(
            rt.control_value(owner, ControlId(20)),
            Ok(ControlValue::Real(-0.25))
        );
        assert_eq!(
            rt.control_value(owner, ControlId(10)),
            Ok(ControlValue::Integer(7))
        );
        assert_eq!(
            rt.control_value(owner, ControlId(30)),
            Ok(ControlValue::Toggle(true))
        );
    });
}

fn phase_d_wave_plan(replaced: bool) -> Prepared {
    use sampler_ir as ir;
    let data = (0..4096)
        .map(|i| [(i as f32 * 0.013).sin() * 0.1; 2])
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let mut zone = ir::Zone::new(ir::AssetRef(0));
    zone.group = Some(ir::GroupRef(0));
    zone.velocity = ir::VelocityResponse::None;
    let instrument = ir::Instrument {
        assets: vec![ir::Asset {
            location: ir::AssetLocation::Path("typed-wave.wav".into()),
            residency: Default::default(),
            encoding: ir::Encoding::Wav,
            root_key: None,
            loops: vec![],
        }],
        groups: vec![ir::Group {
            wavetable: Some(Wavetable::default()),
            ..Default::default()
        }],
        zones: vec![zone],
        ..Default::default()
    };
    let p = lower::lower(
        &instrument,
        48000,
        vec![Pcm::new(48000, data).unwrap()],
        |_, p| Ok(p),
    )
    .unwrap();
    if replaced {
        let mut defs = p.controls().to_vec();
        defs.push(phase_d_definition(
            1,
            ControlDomain::Real { min: 0., max: 1. },
            ControlValue::Real(0.9375),
        ));
        defs.push(phase_d_definition(
            2,
            ControlDomain::Toggle,
            ControlValue::Toggle(true),
        ));
        defs.reverse();
        let p = p.with_controls(defs).unwrap();
        let defs = p.controls().iter().rev().copied().collect();
        p.with_controls(defs).unwrap()
    } else {
        p
    }
}

#[test]
fn phase_d_wavetable_real_handles_survive_replacement_and_old_new_generations_without_heap() {
    let p = phase_d_wave_plan(true);
    let cap = Limits::for_plan(&p, 16, 16);
    let (mut actual, mut ac) = Runtime::with_plan_updates(p, cap, 2, 1).unwrap();
    let (mut expected, mut ec) =
        Runtime::with_plan_updates(phase_d_wave_plan(false), cap, 2, 1).unwrap();
    let wave = EngineParameterAddress {
        parameter: engine_parameter_id("ENGINE_PAR_WT_POSITION").unwrap(),
        group: 0,
        slot: -1,
        generic: -1,
    };
    // Use actual lowered addresses; the schema is public and remains unchanged.
    let wave_a = actual
        .engine_parameter_bindings(actual.active_plan())
        .unwrap()
        .iter()
        .find(|b| b.address.parameter == wave.parameter)
        .unwrap()
        .address;
    let wave_e = expected
        .engine_parameter_bindings(expected.active_plan())
        .unwrap()
        .iter()
        .find(|b| b.address.parameter == wave.parameter)
        .unwrap()
        .address;
    let mut a = [[0.; 2]; 64];
    let mut b = a;
    support::without_heap(|| {
        actual.trigger(input(1), 60, 1.).unwrap();
        expected.trigger(input(1), 60, 1.).unwrap();
        actual.set_engine_parameter(wave_a, 750000).unwrap();
        expected.set_engine_parameter(wave_e, 750000).unwrap();
        actual.render(&mut a).unwrap();
        expected.render(&mut b).unwrap();
        assert_eq!(a, b);
    });
    let old_a = actual.active_plan();
    let old_e = expected.active_plan();
    ac.submit(Box::new(phase_d_wave_plan(false))).unwrap();
    ec.submit(Box::new(phase_d_wave_plan(true))).unwrap();
    support::without_heap(|| {
        actual.poll_plan_update().unwrap();
        expected.poll_plan_update().unwrap();
        assert_ne!(actual.active_plan(), old_a);
        assert_ne!(expected.active_plan(), old_e);
        actual.trigger(input(2), 60, 1.).unwrap();
        expected.trigger(input(2), 60, 1.).unwrap();
        for _ in 0..12 {
            actual.render(&mut a).unwrap();
            expected.render(&mut b).unwrap();
            assert_eq!(a, b);
        }
        let ca = actual
            .engine_parameter_bindings(old_a)
            .unwrap()
            .iter()
            .find(|b| b.address == wave_a)
            .unwrap()
            .control;
        let ce = expected
            .engine_parameter_bindings(old_e)
            .unwrap()
            .iter()
            .find(|b| b.address == wave_e)
            .unwrap()
            .control;
        assert_eq!(
            actual.control_value(old_a, ca),
            expected.control_value(old_e, ce)
        );
        assert_eq!(
            actual.control_value(old_a, ca),
            Ok(ControlValue::Real(0.75))
        );
        actual.panic();
        expected.panic();
        actual.flush_ended(|_| true);
        expected.flush_ended(|_| true);
        actual.collect_retired_plans();
        expected.collect_retired_plans();
    });
    drop(ac.retired().unwrap());
    drop(ec.retired().unwrap());
}

#[test]
fn phase_d_wavetable_admission_rejects_wrong_domains_missing_ids_and_inexact_real_bounds() {
    for change in [
        None,
        Some((
            ControlDomain::Integer { min: 0, max: 1 },
            ControlValue::Integer(0),
        )),
        Some((ControlDomain::Toggle, ControlValue::Toggle(false))),
        Some((
            ControlDomain::Real { min: 0., max: 2. },
            ControlValue::Real(0.),
        )),
    ] {
        let p = phase_d_wave_plan(true);
        let position = engine_parameter_id("ENGINE_PAR_WT_POSITION").unwrap();
        let id = p
            .engine_parameter_bindings()
            .iter()
            .find(|b| b.address.parameter == position)
            .unwrap()
            .control;
        let mut defs = p.controls().to_vec();
        if let Some((domain, default)) = change {
            let c = defs.iter_mut().find(|c| c.id == id).unwrap();
            c.domain = domain;
            c.default = default;
        } else {
            defs.retain(|c| c.id != id);
        }
        assert!(matches!(p.with_controls(defs), Err(Error::InvalidInput)));
    }
}

#[test]
fn phase_d_host_automation_reaches_the_same_real_owner_after_schema_replacement_without_heap() {
    let p = phase_d_live_plan(ModTarget::Pan, true)
        .with_script_instances(vec![vec![]])
        .unwrap()
        .with_widgets(vec![WidgetDefinition {
            id: D_DEPTH,
            source_slot: 0,
            ui_id: 1,
            instance: ScriptInstanceId(0),
            storage: WidgetStorage::Control(D_DEPTH),
            drop: None,
            menu: None,
            program: None,
            stage: 0,
        }])
        .unwrap()
        .with_automation_bindings(vec![AutomationBinding {
            source: AutomationSource::HostParameter(7),
            source_slot: 0,
            ui_id: 1,
            low: 0.,
            high: 1.,
            soft_takeover: false,
        }])
        .unwrap();
    let mut actual = Runtime::new(p, limits()).unwrap();
    let mut expected = Runtime::new(phase_d_live_plan(ModTarget::Pan, false), limits()).unwrap();
    let context = ControlContext {
        performance: actual.performance(0).unwrap(),
        origin: ChannelAddress {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
        },
        channels: 1,
    };
    let mut a = [[0.; 2]; 128];
    let mut b = a;
    support::without_heap(|| {
        actual.trigger(input(1), 60, 1.).unwrap();
        expected.trigger(input(1), 60, 1.).unwrap();
        actual.dispatch_host_parameter(context, 7, 0.75).unwrap();
        expected
            .edit_controls(
                expected.active_plan(),
                None,
                &[ControlWrite {
                    id: D_DEPTH,
                    value: ControlValue::Real(0.5),
                }],
            )
            .unwrap();
        assert_eq!(
            actual.control_base_value(actual.active_plan(), D_DEPTH),
            Ok(ControlValue::Real(0.5))
        );
        for _ in 0..4 {
            actual.render(&mut a).unwrap();
            expected.render(&mut b).unwrap();
            assert_eq!(a, b);
        }
    });
}

#[test]
fn phase_d_typed_control_preparation_allocation_receipt() {
    let mut prepared = None;
    let preparation = support::allocated_bytes(|| {
        let defs = (0..1024u128)
            .map(|i| match i % 3 {
                0 => phase_d_definition(
                    i,
                    ControlDomain::Integer {
                        min: i64::MIN,
                        max: i64::MAX,
                    },
                    ControlValue::Integer(i as i64),
                ),
                1 => phase_d_definition(
                    i,
                    ControlDomain::Real {
                        min: -f64::MAX,
                        max: f64::MAX,
                    },
                    ControlValue::Real(-0.),
                ),
                _ => phase_d_definition(i, ControlDomain::Toggle, ControlValue::Toggle(true)),
            })
            .collect();
        prepared = Some(source_plan().with_controls(defs).unwrap());
    });
    let mut runtime = None;
    let runtime_bytes = support::allocated_bytes(|| {
        runtime = Some(Runtime::new(prepared.take().unwrap(), limits()).unwrap());
    });
    eprintln!(
        "D 1024-control fixture requested bytes: preparation={preparation} runtime={runtime_bytes}"
    );
    assert!(runtime.is_some());
}
