//! UVI's DualDelay onto `ir::Delay`. The laws are UVIWorkstation's DualDelay
//! engine: parameter setters 0x141237c50..0x141237f20 (ids from 0x1427f9a70)
//! and the audio method 0x14165c960. Per block:
//! - the time is `DelayTime` seconds, or synced quarter notes, at most 5 s;
//! - `line = fb·(rotate(Rotation°)·heads + In·input)`, where `In` rotates by
//!   `InputRotation·45°` after an M/S width of `InputWidth`, and `fb` is
//!   Feedback, split by FeedbackRatio (the side it is negative on keeps all);
//! - each line runs a one-pole high-pass at LowCut (0x141641420) and a
//!   one-pole low-pass at HighCut (0x141641310, `e^(−2π·f/rate)`);
//! - `out = √Mix·Out·heads + √(1 − Mix)·input`, where `Out` rotates by
//!   `OutputRotation·45°` and then applies an M/S width of `OutputWidth`.
//!
//! Rotations are `[[cos, −sin], [sin, cos]]` (0x14165dd70 over sincos
//! 0x14199de20); the width `R(45°)·diag(1, w)·R(−45°)` (0x14165dea0,
//! 0x14165e0b0, constants 0x14283ac30/40) is the mid/side matrix below.

use kontra_format::Notes;
use kontra_ir as ir;
use kontra_law::common::SQUARE_ROOT;
use roxmltree::Node;

use super::{flag, num};

/// The engine's longest line, seconds (0x141f8f628).
const MAX_S: f64 = 5.0;

/// The DualDelay at `node`. Parameters every corpus program leaves at rest
/// (DelayRatio, ModDepth, PeakGain) are reported when set.
pub(super) fn dual_delay(node: Node, notes: &mut Notes) -> Result<ir::Effect, String> {
    let time = num(node, "DelayTime", 0.5)?.max(0.0);
    let feedback = num(node, "Feedback", 0.3)? as f32;
    let ratio = num(node, "FeedbackRatio", 0.0)?.clamp(-1.0, 1.0) as f32;
    let [fb0, fb1] = if ratio <= 0.0 {
        [(1.0 + ratio) * feedback, feedback]
    } else {
        [feedback, (1.0 - ratio) * feedback]
    };
    let angle = |name: &str, unit: f64| -> Result<f32, String> {
        Ok((num(node, name, 0.0)? * unit) as f32)
    };
    let quarter = std::f64::consts::FRAC_PI_4;
    let input = mul(
        rotation(angle("InputRotation", quarter)?),
        width(num(node, "InputWidth", 1.0)? as f32),
    );
    let output = mul(
        width(num(node, "OutputWidth", 1.0)? as f32),
        rotation(angle("OutputRotation", quarter)?),
    );
    let feedback = rotation(angle("Rotation", 1f64.to_radians())?);
    let scale = |[top, bottom]: ir::Matrix| [top.map(|x| x * fb0), bottom.map(|x| x * fb1)];
    for (name, rest) in [("DelayRatio", 0.0), ("ModDepth", 0.0), ("PeakGain", 0.0)] {
        if num(node, name, rest)? != rest {
            notes.add(ir::Impact::Audible, &format!("dual delay {name}"), "");
        }
    }
    let mix = num(node, "Mix", 1.0)?.clamp(0.0, 1.0) as f32;
    Ok(ir::Effect {
        kind: ir::EffectKind::Delay(ir::Delay {
            time: if flag(node, "SyncToHost") {
                ir::DelayTime::Beats(time as f32)
            } else {
                ir::DelayTime::Ms((time.min(MAX_S) * 1000.0) as f32)
            },
            depth_ms: 0.0,
            second_head: false,
            input: scale(input),
            feedback: scale(feedback),
            output,
            damping_hz: num(node, "HighCut", 20_000.0)? as f32,
            low_cut_hz: num(node, "LowCut", 0.0)?.max(0.0) as f32,
            steep_low_cut: false,
            high_shelf: None,
            spread_ms: 0.0,
            rate: ir::Rate::Hz(num(node, "ModRate", 1.0)? as f32),
            phase: 0.0,
        }),
        bypass: false,
        output: SQUARE_ROOT.eval(mix),
        dry: SQUARE_ROOT.eval(1.0 - mix),
    })
}

fn rotation(radians: f32) -> ir::Matrix {
    let (sin, cos) = radians.sin_cos();
    [[cos, -sin], [sin, cos]]
}

/// Mid kept, side scaled by `w`.
fn width(w: f32) -> ir::Matrix {
    let (same, other) = ((1.0 + w) / 2.0, (1.0 - w) / 2.0);
    [[same, other], [other, same]]
}

fn mul(a: ir::Matrix, b: ir::Matrix) -> ir::Matrix {
    let [[b00, b01], [b10, b11]] = b;
    a.map(|[x, y]| [x * b00 + y * b10, x * b01 + y * b11])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delay(attributes: &str) -> (ir::Effect, ir::Delay) {
        let xml = format!("<DualDelay {attributes}/>");
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let e = dual_delay(doc.root_element(), &mut Notes::default()).unwrap();
        let ir::EffectKind::Delay(d) = e.kind else {
            panic!("{:?}", e.kind)
        };
        (e, d)
    }

    fn near(a: ir::Matrix, b: ir::Matrix) -> bool {
        a.iter()
            .flatten()
            .zip(b.iter().flatten())
            .all(|(x, y)| (x - y).abs() < 1e-6)
    }

    #[test]
    fn the_corpus_setting_ping_pongs_a_centred_source_from_the_right() {
        // Augmented Orchestra's most common DualDelay (248 programs).
        let (e, d) = delay(
            r#"SyncToHost="1" DelayTime="0.5" LowCut="27.361252" HighCut="2630"
               Feedback="0.41065004" Mix="1" Rotation="90" InputWidth="1" OutputWidth="1"
               InputRotation="1" OutputRotation="0""#,
        );
        assert_eq!(d.time, ir::DelayTime::Beats(0.5));
        assert_eq!((e.output, e.dry), (1.0, 0.0));
        assert_eq!((d.damping_hz, d.low_cut_hz), (2630.0, 27.361252));
        let (g, h) = (0.41065004, 0.41065004 * std::f32::consts::FRAC_1_SQRT_2);
        // A mono input (L = R) enters only the right line, at fb·√2.
        assert!(near(d.input, [[h, -h], [h, h]]), "{:?}", d.input);
        // The right head returns, negated, into the left line, and the left
        // one into the right.
        assert!(near(d.feedback, [[0.0, -g], [g, 0.0]]), "{:?}", d.feedback);
        assert!(near(d.output, ir::IDENTITY));
    }

    #[test]
    fn widths_scale_the_side_and_the_feedback_ratio_trims_one_line() {
        let (e, d) = delay(
            r#"DelayTime="0.25" Feedback="0.5" FeedbackRatio="0.5" Mix="0.25"
               InputWidth="0.5" OutputWidth="0" Rotation="0" InputRotation="0""#,
        );
        assert_eq!(d.time, ir::DelayTime::Ms(250.0));
        assert_eq!((e.output, e.dry), (0.5, 0.75f32.sqrt()));
        assert!(
            near(d.input, [[0.375, 0.125], [0.0625, 0.1875]]),
            "{:?}",
            d.input
        );
        assert!(near(d.feedback, [[0.5, 0.0], [0.0, 0.25]]));
        assert!(near(d.output, [[0.5, 0.5], [0.5, 0.5]]));
    }
}
