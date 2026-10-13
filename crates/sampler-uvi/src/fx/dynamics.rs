//! UVI `dynamics` effects onto sampler_ir::fx::dynamics.
//!
//! Maximizer runs its native law (Workstation 4.0.9). No other UVI dynamics
//! law is recovered from the binary, so every other mapped effect
//! runs the generic kernel and is reported `UnknownLaw`; effects whose
//! structure the generic kernel cannot express are reported `NotModeled`.
use crate::{Translation, number};
use roxmltree::Node;
use sampler_ir::{self as ir, fx::dynamics as dyn_fx};

fn db(db: f64) -> ir::Gain {
    ir::Gain::Linear(10f64.powf(db / 20.0))
}

fn report(t: &mut Translation, at: &str, feature: &str, reason: ir::Reason) {
    t.ir.unsupported.push(ir::Unsupported {
        location: at.into(),
        feature: feature.into(),
        value: String::new(),
        reason,
    });
}

/// Insert elements this module handles.
pub(crate) fn handles(kind: &str) -> bool {
    matches!(
        kind,
        "CompExp"
            | "BrickwallLimiter"
            | "FeedbackCompressor"
            | "FETCompressor"
            | "FeedforwardCompressor"
            | "ThreeBandComp"
            | "ThreeBandLimiter"
            | "UVIMastering"
            | "LowpassGateFX"
            | "Compressor"
            | "Gate"
            | "Limiter"
            | "SoftLimiter"
            | "Maximizer"
    )
}

/// Translate one dynamics insert; the caller marks it used.
pub(crate) fn translate(
    t: &mut Translation,
    node: Node,
    at: &str,
    out: &mut Vec<ir::Processor>,
) -> Result<(), String> {
    let kind = node.tag_name().name();
    let ms = |name: &str, d: f64| -> Result<ir::Time, String> {
        Ok(ir::Time::Milliseconds(number(node, name, d)?.max(0.0)))
    };
    match kind {
        "CompExp" => {
            // Falcon manual: threshold dB, ratio, attack/release ms, manual makeup dB.
            for (name, why, off) in [
                ("AutoMakeUp", "CompExp auto makeup", 0.0),
                ("GateRatio", "CompExp gate (expander)", 1.0),
            ] {
                let v = number(node, name, off)?;
                if v != off
                    && (name != "GateRatio" || number(node, "GateThreshold", -130.0)? > -130.0)
                {
                    t.unsupported(at, why, v);
                }
            }
            let mut c = dyn_fx::Config::generic(
                number(node, "CompThreshold", 0.0)?,
                number(node, "CompRatio", 10.0)?.max(1.0),
                ms("CompAttack", 10.0)?,
                ms("CompRelease", 100.0)?,
            );
            c.makeup = db(number(node, "MakeUpGain", 0.0)?);
            c.mix = number(node, "Mix", 1.0)?.clamp(0.0, 1.0);
            report(t, at, "CompExp compressor law (generic kernel)", ir::Reason::UnknownLaw);
            out.push(ir::Processor::Dynamics(c));
        }
        "BrickwallLimiter" => {
            // Margin is the ceiling below the threshold; both in dB.
            let mut c = dyn_fx::Config::generic(
                number(node, "Threshold", 0.0)? + number(node, "Margin", 0.0)?,
                100.0,
                ir::Time::Milliseconds(0.0),
                ms("Release", 100.0)?,
            );
            if let dyn_fx::Law::Generic(g) = &mut c.law {
                g.lookahead = ir::Time::Milliseconds(2.0);
            }
            c.mix = number(node, "Mix", 1.0)?.clamp(0.0, 1.0);
            report(
                t,
                at,
                "BrickwallLimiter law (generic look-ahead limiter)",
                ir::Reason::UnknownLaw,
            );
            out.push(ir::Processor::Dynamics(c));
        }
        "FeedbackCompressor" => {
            let mut c = dyn_fx::Config::generic(
                number(node, "Threshold", -10.0)?,
                number(node, "Ratio", 4.0)?.max(1.0),
                ms("Attack", 0.0)?,
                ms("Release", 100.0)?,
            );
            if let dyn_fx::Law::Generic(g) = &mut c.law {
                g.knee_db = number(node, "Knee", 10.0)?.max(0.0);
                g.range_db = number(node, "DynamicRange", 100.0)?;
                g.topology = dyn_fx::Topology::Feedback;
            }
            c.mix = number(node, "Mix", 1.0)?.clamp(0.0, 1.0);
            let makeup = number(node, "Makeup", 0.5)?;
            if makeup != 0.0 {
                t.unsupported(at, "FeedbackCompressor normalised makeup", makeup);
            }
            out.push(ir::Processor::Gain(db(number(node, "InputGain", 0.0)?)));
            out.push(ir::Processor::Dynamics(c));
            out.push(ir::Processor::Gain(db(number(node, "OutputGain", 0.0)?)));
            report(
                t,
                at,
                "FeedbackCompressor law (generic feedback kernel)",
                ir::Reason::UnknownLaw,
            );
        }
        "Maximizer" => {
            // Native law (Workstation 4.0.9, process 0x141665620); XML units
            // go straight into the core.
            let m = dyn_fx::Maximizer {
                threshold_db: number(node, "Threshold", -6.0)?,
                attack_ms: number(node, "Attack", 0.0)?,
                lookahead_ms: number(node, "Lookahead", 2.0)?,
                release_ms: number(node, "Release", 10.0)?,
                ceiling_db: number(node, "Ceiling", -0.1)?,
                knee_db: number(node, "Knee", 0.0)?,
                release_blend: number(node, "ReleaseBlend", 0.1)?,
                slew_ms: number(node, "SlewRate", 20.0)?,
                mode: number(node, "CeilingMode", 0.0)?.clamp(0.0, 2.0) as u8,
                true_peak: number(node, "TruePeak", 0.0)? > 0.5,
            };
            let mut c = dyn_fx::Config::generic(m.threshold_db, 1.0, ir::Time::Milliseconds(0.0), ms("Release", 10.0)?);
            c.law = dyn_fx::Law::UviMaximizer(m);
            out.push(ir::Processor::Dynamics(c));
        }
        _ => report(t, at, &format!("{kind} dynamics effect"), ir::Reason::NotModeled),
    }
    Ok(())
}
