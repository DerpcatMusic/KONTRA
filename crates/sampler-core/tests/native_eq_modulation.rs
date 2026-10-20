use sampler_core::{Input, Limits, Pcm, Protocol, Runtime};
use sampler_ir as ir;

fn render(
    parameter: ir::ProcessorParameter,
    depths: &[f64],
    target_control: bool,
) -> Vec<[f32; 2]> {
    let fields = [
        ir::ProcessorParameter::Cutoff,
        ir::ProcessorParameter::Resonance,
        ir::ProcessorParameter::Gain,
    ];
    let selected = fields.iter().position(|p| *p == parameter).unwrap();
    let mut instrument = ir::Instrument {
        assets: vec![ir::Asset {
            location: ir::AssetLocation::Path("authored-flat-eq".into()),
            encoding: ir::Encoding::Wav,
            root_key: None,
            loops: vec![],
        }],
        chains: vec![ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::KontaktLegacyEq(ir::KontaktLegacyEq {
                frequency: 0.5,
                bandwidth: 0.5,
                gain_db: 0.,
            })],
            post_amplitude: vec![],
        }],
        modulators: vec![ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Constant,
        }],
        ..Default::default()
    };
    // All three are scalar live owners in both renders. Binding a control alone
    // must not be mistaken for an audio modulation feed.
    for (index, field) in fields.into_iter().enumerate() {
        let gain = field == ir::ProcessorParameter::Gain;
        instrument.controls.push(ir::Control {
            key: format!("flat-eq-{index}"),
            label: "EQ".into(),
            value: ir::ControlValue::Continuous {
                min: if gain { -18. } else { 0. },
                max: if gain { 18. } else { 1. },
                default: if gain { 0. } else { 0.5 },
                unit: ir::ControlUnit::None,
            },
            automation: ir::Automation::None,
        });
        instrument.processor_controls.push(ir::ProcessorControl {
            control: ir::ControlRef(index),
            chain: ir::ChainRef(0),
            index: 0,
            parameter: field,
            ramp: ir::Time::ZERO,
        });
    }
    for &depth in depths {
        instrument.routes.push(ir::Route {
            source: ir::ModulatorRef(0),
            target: if target_control {
                ir::Target::Control(ir::ControlRef(selected))
            } else {
                ir::Target::Processor {
                    chain: ir::ChainRef(0),
                    index: 0,
                    parameter,
                }
            },
            depth: ir::Depth::Normalized(depth),
            invert: false,
            shape: None,
            smoothing: ir::Time::ZERO,
            scale: None,
        });
    }
    let mut zone = ir::Zone::new(ir::AssetRef(0));
    zone.chain = Some(ir::ChainRef(0));
    zone.pitch = ir::KeyTracking::Fixed;
    zone.velocity = ir::VelocityResponse::None;
    zone.routes = (0..depths.len()).map(ir::RouteRef).collect();
    instrument.zones.push(zone);
    let pcm = Pcm::new(
        48000,
        (0..2048)
            .map(|i| {
                let x = ((i * 7 + 3) % 31) as f32 * 0.03125 - 15. * 0.03125;
                [x, -x]
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
    .unwrap();
    let plan = sampler_core::lower::lower(&instrument, 48000, vec![pcm], |_, p| Ok(p)).unwrap();
    let limits = Limits::for_plan(&plan, 4, 4);
    let mut runtime = Runtime::new(plan, limits).unwrap();
    runtime
        .trigger(
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
    let mut output = vec![[0.; 2]; 1024];
    runtime.render(&mut output).unwrap();
    output
}

#[test]
fn lowered_zero_and_cancelled_eq_routes_suppress_flat_bypass() {
    for parameter in [
        ir::ProcessorParameter::Cutoff,
        ir::ProcessorParameter::Resonance,
        ir::ProcessorParameter::Gain,
    ] {
        let scalar_alias = render(parameter, &[], false);
        assert!(scalar_alias.iter().any(|f| f[0] != 0.));
        for target_control in [false, true] {
            let zero = render(parameter, &[0.], target_control);
            let cancelled = render(parameter, &[0.125, -0.125], target_control);
            assert_ne!(
                zero, scalar_alias,
                "projected {parameter:?} must run the binary32 recurrence"
            );
            assert_eq!(
                zero, cancelled,
                "route cancellation retains modulation presence"
            );
            assert!(zero.iter().flatten().all(|v| v.is_finite()));
        }
    }
}
