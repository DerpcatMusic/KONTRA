//! Authored signal and KSP only. These exercise the shared physical parameter
//! consumer; they do not establish native audio equivalence for moving knobs.
use sampler_core::*;
use sampler_ir as ir;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

const RATE: u32 = 48_000;
// Bright's coefficient reaches the native min(1) clamp at the all-.5 preset
// at 48 kHz. Start below that plateau so this knob's PCM test is observable.
const KNOBS: [f32; 5] = [0.5, 0.5, 0.5, 0.0, 0.5];
const NAMES: [&str; 5] = ["SK_TONE", "SK_DRIVE", "SK_BASS", "SK_BRIGHT", "SK_MIX"];

fn address(parameter: &str, group: i32, slot: i32) -> EngineParameterAddress {
    EngineParameterAddress {
        parameter: engine_parameter_id(&format!("ENGINE_PAR_{parameter}")).unwrap(),
        group,
        slot,
        generic: -1,
    }
}

fn plan(parameter: &str, addressed: bool) -> Prepared {
    configured_plan(parameter, addressed, KNOBS, None)
}

fn configured_plan(
    parameter: &str,
    addressed: bool,
    left_knobs: [f32; 5],
    modulation: Option<ir::ProcessorParameter>,
) -> Prepared {
    let mut instrument = ir::Instrument::default();
    for i in 0..2 {
        instrument.assets.push(ir::Asset {
            location: ir::AssetLocation::Path(format!("authored-signal-{i}")),
            encoding: ir::Encoding::Wav,
            residency: Default::default(),
            root_key: None,
            loops: vec![],
        });
        let config = ir::fx::color::Config::skreamer(if i == 0 { left_knobs } else { KNOBS });
        let config = if addressed {
            config.at(ir::SlotAddress {
                group: i as i32,
                slot: 3 + i as i32 * 2,
                generic: -1,
            })
        } else {
            config
        };
        instrument.chains.push(ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Color(config)],
            post_amplitude: vec![],
        });
        let mut zone = ir::Zone::new(ir::AssetRef(i));
        zone.chain = Some(ir::ChainRef(i));
        zone.velocity = ir::VelocityResponse::None;
        zone.pitch = ir::KeyTracking::Fixed;
        instrument.zones.push(zone);
    }
    if let Some(parameter) = modulation {
        instrument.modulators.push(ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Controller(11),
        });
        instrument.routes.push(ir::Route::new(
            ir::ModulatorRef(0),
            ir::Target::Processor {
                chain: ir::ChainRef(0),
                index: 0,
                parameter,
            },
            ir::Depth::Normalized(0.25),
        ));
        instrument.zones[0].routes.push(ir::RouteRef(0));
    }
    let pcm = (0..2)
        .map(|channel| {
            let frames = (0..8192)
                .map(|i| {
                    let angle = std::f64::consts::TAU * i as f64 / f64::from(RATE);
                    let mut frame = [0.; 2];
                    frame[channel] =
                        (0.35 * (angle * 200.).sin() + 0.1 * (angle * 2100.).sin()) as f32;
                    frame
                })
                .collect::<Vec<_>>();
            Pcm::new(RATE, frames.into_boxed_slice()).unwrap()
        })
        .collect();
    let prepared = sampler_core::lower::lower(&instrument, RATE, pcm, |_, p| Ok(p)).unwrap();
    let source = format!(
        r#"on init
        make_perfview
        declare ui_knob $change(0,1000000,1)
        declare ui_knob $seen(0,1000000,1)
        declare ui_knob $other(0,1000000,1)
        declare ui_knob $calls(0,100,1)
    end on
    on ui_control($change)
        set_engine_par($ENGINE_PAR_{parameter},$change,0,3,-1)
        $seen := get_engine_par($ENGINE_PAR_{parameter},0,3,-1)
        $other := get_engine_par($ENGINE_PAR_{parameter},1,5,-1)
        inc($calls)
    end on"#
    );
    sampler_ksp::compile(&source, RATE, sampler_ksp::Limits::LIBRARY, &[])
        .unwrap()
        .bind(prepared)
        .unwrap()
}

#[test]
fn skreamer_held_voice_modulation_matches_the_static_knob_and_preserves_the_other_effect() {
    for (index, role) in [
        ir::ProcessorParameter::Tone,
        ir::ProcessorParameter::Drive,
        ir::ProcessorParameter::Bass,
        ir::ProcessorParameter::Bright,
        ir::ProcessorParameter::Mix,
    ]
    .into_iter()
    .enumerate()
    {
        let mut changed = KNOBS;
        changed[index] += 0.25;
        let make = |p: Prepared| {
            let limits = Limits::for_plan(&p, 4, 4);
            Runtime::new(p, limits).unwrap()
        };
        let mut held = make(configured_plan(NAMES[index], true, KNOBS, Some(role)));
        let mut fixed = make(configured_plan(NAMES[index], false, changed, None));
        let mut baseline = make(configured_plan(NAMES[index], false, KNOBS, None));
        let mut output = [[0.; 2]; 2048];
        let mut expected = output;
        let mut unchanged = output;
        support::without_heap(|| {
            held.set_controller(held.performance(0).unwrap(), 11, u32::MAX)
                .unwrap();
            start(&mut held);
            start(&mut fixed);
            start(&mut baseline);
            held.render(&mut output).unwrap();
            fixed.render(&mut expected).unwrap();
            baseline.render(&mut unchanged).unwrap();
        });
        assert_eq!(
            output, expected,
            "{role:?}: held modulation must use the static native knob law"
        );
        assert!(
            output
                .iter()
                .zip(&unchanged)
                .any(|(a, b)| (a[0] - b[0]).abs() > 1e-5),
            "{role:?}: modulation did not reach the DSP"
        );
        assert!(
            output
                .iter()
                .zip(&unchanged)
                .all(|(a, b)| a[1].to_bits() == b[1].to_bits()),
            "{role:?}: modulation leaked to another effect"
        );
    }
}

fn runtime(parameter: &str, addressed: bool) -> Runtime {
    let plan = plan(parameter, addressed);
    let limits = Limits::for_plan(&plan, 4, 4);
    Runtime::new(plan, limits).unwrap()
}

fn context(rt: &Runtime) -> ControlContext {
    ControlContext {
        performance: rt.performance(0).unwrap(),
        origin: ChannelAddress {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
        },
        channels: 1,
    }
}

fn start(rt: &mut Runtime) {
    rt.trigger(
        Input {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
            key: 60,
            external_id: None,
        },
        60,
        1.,
    )
    .unwrap();
}

fn invoke(rt: &mut Runtime, control: ControlId, value: i64) {
    rt.invoke_control(
        context(rt),
        rt.active_plan(),
        None,
        ControlWrite {
            id: control,
            value: ControlValue::Integer(value),
        },
    )
    .unwrap();
    rt.flush_behaviors(|_, _, _| true);
}

#[test]
fn skreamer_authored_ui_callback_reaches_only_its_physical_effect_and_changes_pcm_without_heap() {
    for (index, parameter) in NAMES.into_iter().enumerate() {
        let target = address(parameter, 0, 3);
        let other = address(parameter, 1, 5);
        let saved = (f64::from(KNOBS[index]) * 1_000_000.).round() as i32;
        let change = sampler_ksp::derived_control_id(0, "$change");
        let observed = [
            ("$seen", 900000),
            ("$other", i64::from(saved)),
            ("$calls", 1),
        ]
        .map(|(name, value)| (sampler_ksp::derived_control_id(0, name), value));
        let mut live = runtime(parameter, true);
        let mut fixed = runtime(parameter, false);
        let mut reference = runtime(parameter, true);
        let mut before = [[0.; 2]; 2048];
        let mut static_before = before;
        let mut ref_before = before;
        support::without_heap(|| {
            start(&mut live);
            start(&mut fixed);
            start(&mut reference);
            live.render(&mut before).unwrap();
            fixed.render(&mut static_before).unwrap();
            reference.render(&mut ref_before).unwrap();
        });
        assert_eq!(
            before, static_before,
            "held live {parameter} must equal static DSP"
        );
        assert_eq!(before, ref_before);
        let mut after = [[0.; 2]; 2048];
        let mut unchanged = after;
        support::without_heap(|| {
            assert_eq!(live.engine_parameter(target), Ok(saved));
            assert_eq!(
                live.set_engine_parameter(EngineParameterAddress { slot: 4, ..target }, 900000),
                Err(Error::InvalidInput)
            );
            invoke(&mut live, change, 900000);
            assert_eq!(live.engine_parameter(target), Ok(900000));
            assert_eq!(live.engine_parameter(other), Ok(saved));
            let owner = live.active_plan();
            for (control, value) in observed {
                assert_eq!(
                    live.control_value(owner, control),
                    Ok(ControlValue::Integer(value))
                );
            }
            let mut reached = false;
            while let Some(outcome) = live.take_engine_parameter_outcome() {
                assert_eq!(outcome.result, Ok(()));
                reached |= outcome.write && outcome.address == Some(target);
            }
            assert!(
                reached,
                "the authored callback must execute its engine write"
            );
            // A stale revision rejects the complete callback before any DSP write.
            assert_eq!(
                live.invoke_control(
                    context(&live),
                    owner,
                    Some(0),
                    ControlWrite {
                        id: change,
                        value: ControlValue::Integer(100000),
                    }
                ),
                Err(Error::RevisionConflict)
            );
            assert_eq!(live.engine_parameter(target), Ok(900000));
            live.render(&mut after).unwrap();
            reference.render(&mut unchanged).unwrap();
        });
        assert!(after.iter().flatten().all(|v| v.is_finite()));
        let delta = after
            .iter()
            .zip(&unchanged)
            .map(|(a, b)| f64::from(a[0] - b[0]).powi(2))
            .sum::<f64>()
            / after.len() as f64;
        assert!(
            delta.sqrt() > 1e-5,
            "{parameter} is still constant: RMS difference {}",
            delta.sqrt()
        );
        eprintln!(
            "{parameter}: authored callback changed-left RMS difference={:.9}; other-effect output bit-identical",
            delta.sqrt()
        );
        assert!(
            after
                .iter()
                .zip(&unchanged)
                .all(|(a, b)| a[1].to_bits() == b[1].to_bits()),
            "{parameter} altered the other physical effect"
        );
        assert!(live.take_fault().is_none());
    }
}

#[test]
fn skreamer_replacement_rejects_old_callback_generation_without_editing_new_parameter_state() {
    let first = plan("SK_DRIVE", true);
    let limits = Limits::for_plan(&first, 4, 4);
    let (mut rt, mut plans) = Runtime::with_plan_updates(first, limits, 2, 1).unwrap();
    let old = rt.active_plan();
    let target = address("SK_DRIVE", 0, 3);
    let change = sampler_ksp::derived_control_id(0, "$change");
    plans.submit(Box::new(plan("SK_DRIVE", true))).unwrap();
    support::without_heap(|| {
        rt.poll_plan_update().unwrap();
        assert_ne!(old, rt.active_plan());
        rt.collect_retired_plans();
        assert_eq!(
            rt.invoke_control(
                context(&rt),
                old,
                None,
                ControlWrite {
                    id: change,
                    value: ControlValue::Integer(1000000),
                }
            ),
            Err(Error::StaleHandle)
        );
        assert_eq!(rt.engine_parameter(target), Ok(500000));
    });
}
