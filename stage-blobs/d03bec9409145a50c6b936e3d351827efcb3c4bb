//! Native catalog fields bound to the controls actually read by the DSP.
use crate::{InsertNode, script::parameters};
use sampler_core::{EngineParameterAddress, EngineParameterBinding, EngineParameterLaw};
use sampler_ir as ir;

const PREFIX: &str = "uvi:insert:";

// Only exact native fields with direct DSP lanes belong here. In particular,
// ThreeBandShelves' relative-stage approximation is not a native gain control.
fn field(kind: &str, name: &str) -> Option<(ir::ProcessorParameter, &'static str)> {
    match (kind, name) {
        ("OnePole", "Freq") => Some((ir::ProcessorParameter::Cutoff, "ENGINE_PAR_CUTOFF")),
        ("Gain", "Volume") => Some((ir::ProcessorParameter::Gain, "ENGINE_PAR_VOLUME")),
        _ => None,
    }
}

fn key(node: usize, kind: &str, name: &str) -> String {
    format!("{PREFIX}{node}:{kind}:{name}")
}

pub(crate) fn binding(node: usize, kind: &str, name: &str) -> Option<EngineParameterBinding> {
    let (_, engine) = field(kind, name)?;
    let p = parameters::definitions(kind)
        .iter()
        .find(|p| p.name == name)?;
    Some(EngineParameterBinding {
        address: EngineParameterAddress {
            parameter: sampler_core::engine_parameter_id(engine)?,
            group: -1,
            slot: -1,
            generic: i32::try_from(node).ok()?,
        },
        control: sampler_core::lower::ir_control_id(&key(node, kind, name)),
        // This is service normalization, not a second DSP law. DSP lanes hold
        // native Hz or linear amplitude directly.
        law: if p.unit == "Hz" {
            EngineParameterLaw::Exponential {
                low: p.min,
                high: p.max,
            }
        } else {
            EngineParameterLaw::Linear {
                low: p.min,
                high: p.max,
            }
        },
    })
}

pub(crate) fn bindings(instrument: &ir::Instrument) -> Vec<EngineParameterBinding> {
    instrument
        .controls
        .iter()
        .filter_map(|c| {
            let mut parts = c.key.strip_prefix(PREFIX)?.split(':');
            let node = parts.next()?.parse().ok()?;
            let kind = parts.next()?;
            let name = parts.next()?;
            if parts.next().is_some() {
                return None;
            }
            binding(node, kind, name)
        })
        .collect()
}

pub(crate) fn register(
    instrument: &mut ir::Instrument,
    doc: &roxmltree::Document,
    inserts: &[InsertNode],
) {
    for insert in inserts.iter().filter(|i| i.count == 1) {
        let Some(node) = u32::try_from(insert.node)
            .ok()
            .and_then(|id| doc.get_node(roxmltree::NodeId::new(id)))
        else {
            continue;
        };
        let kind = node.tag_name().name();
        // The translator reports nonzero OnePole key tracking as unmodeled;
        // do not replace its static approximation with an unrelated raw lane.
        if kind == "OnePole"
            && node
                .attribute("KeyTracking")
                .is_some_and(|v| v.parse::<f64>().unwrap_or(0.) != 0.)
        {
            continue;
        }
        for p in parameters::definitions(kind) {
            let Some((parameter, _)) = field(kind, p.name) else {
                continue;
            };
            let default = node
                .attribute(p.name)
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(p.default);
            if !default.is_finite() || default < p.min || default > p.max {
                continue;
            }
            let control = ir::ControlRef(instrument.controls.len());
            instrument.controls.push(ir::Control {
                key: key(insert.node, kind, p.name),
                label: p.name.into(),
                value: ir::ControlValue::Continuous {
                    min: p.min,
                    max: p.max,
                    default,
                    unit: if p.unit == "Hz" {
                        ir::ControlUnit::Hertz
                    } else {
                        ir::ControlUnit::None
                    },
                },
                automation: ir::Automation::None,
            });
            instrument.processor_controls.push(ir::ProcessorControl {
                control,
                chain: insert.chain,
                index: insert.first,
                parameter,
                ramp: ir::Time::Seconds(0.),
            });
        }
    }
}

pub(crate) fn initialize(instrument: &mut ir::Instrument, overrides: &[(usize, String, String)]) {
    for (node, name, value) in overrides {
        for c in &mut instrument.controls {
            let Some(mut parts) = c.key.strip_prefix(PREFIX).map(|s| s.split(':')) else {
                continue;
            };
            if parts.next().and_then(|n| n.parse().ok()) != Some(*node) {
                continue;
            }
            parts.next();
            if parts.next() != Some(name.as_str()) {
                continue;
            }
            if let (
                ir::ControlValue::Continuous {
                    min, max, default, ..
                },
                Ok(v),
            ) = (&mut c.value, value.parse::<f64>())
            {
                if v.is_finite() && v >= *min && v <= *max {
                    *default = v;
                }
            }
        }
    }
}
