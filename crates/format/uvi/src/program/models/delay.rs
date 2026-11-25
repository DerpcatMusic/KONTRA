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

use std::f64::consts::FRAC_PI_4;

use kontra_format::Notes;
use kontra_ir as ir;
use kontra_law::common::SQUARE_ROOT;
use roxmltree::Node;

use super::{Attr, DUAL_DELAY_MAX_S, UviModule, ids, num};

/// DualDelay: `[DelayTime, HighCut, LowCut, ModRate, Mix, Feedback, FeedbackRatio,
/// InputWidth, InputRotation, Rotation, OutputWidth, OutputRotation, SyncToHost]`.
pub static DUAL_DELAY: UviModule = UviModule {
    id: ids::DUAL_DELAY,
    tag: "DualDelay",
    attrs: &[
        Attr::num("DelayTime", 0.5),
        Attr::num("HighCut", 20_000.0),
        Attr::num("LowCut", 0.0),
        Attr::num("ModRate", 1.0),
        Attr::num("Mix", 1.0),
        Attr::num("Feedback", 0.3),
        Attr::num("FeedbackRatio", 0.0),
        Attr::num("InputWidth", 1.0),
        Attr::num("InputRotation", 0.0),
        Attr::num("Rotation", 0.0),
        Attr::num("OutputWidth", 1.0),
        Attr::num("OutputRotation", 0.0),
        Attr::flag("SyncToHost"),
    ],
    report: dual_delay_report,
    build_effect: |p, current| {
        let [
            time,
            high_cut,
            low_cut,
            mod_rate,
            mix,
            feedback,
            ratio,
            in_width,
            in_rot,
            rot_deg,
            out_width,
            out_rot,
            synced,
        ] = *p
        else {
            return None;
        };
        let rot = |rad: f32| -> ir::Matrix {
            let (s, c) = rad.sin_cos();
            [[c, -s], [s, c]]
        };
        let wid = |w: f32| -> ir::Matrix {
            let (same, other) = ((1.0 + w) / 2.0, (1.0 - w) / 2.0);
            [[same, other], [other, same]]
        };
        let mul = |a: ir::Matrix, b: ir::Matrix| -> ir::Matrix {
            let [[b00, b01], [b10, b11]] = b;
            a.map(|[x, y]| [x * b00 + y * b10, x * b01 + y * b11])
        };
        let ratio = ratio.clamp(-1.0, 1.0);
        let [fb0, fb1] = if ratio <= 0.0 {
            [(1.0 + ratio) * feedback, feedback]
        } else {
            [feedback, (1.0 - ratio) * feedback]
        };
        let quarter = FRAC_PI_4 as f32;
        let input = mul(rot(in_rot * quarter), wid(in_width));
        let output = mul(wid(out_width), rot(out_rot * quarter));
        let fb_rot = rot(rot_deg.to_radians());
        let scale = |[top, bottom]: ir::Matrix| [top.map(|x| x * fb0), bottom.map(|x| x * fb1)];
        let time = f64::from(time).max(0.0);
        let mix = mix.clamp(0.0, 1.0);
        Some(ir::Effect {
            kind: ir::EffectKind::Delay(ir::Delay {
                direct: 0.0,
                time: if synced > 0.5 {
                    ir::DelayTime::Beats(time as f32)
                } else {
                    ir::DelayTime::Ms((time.min(DUAL_DELAY_MAX_S) * 1000.0) as f32)
                },
                depth_ms: 0.0,
                second_head: false,
                input: scale(input),
                feedback: scale(fb_rot),
                output,
                damping_hz: high_cut,
                low_cut_hz: low_cut.max(0.0),
                steep_low_cut: false,
                body: std::borrow::Cow::Borrowed(&[]),
                triangle_s: None,
                shape: std::borrow::Cow::Borrowed(&[]),
                noise: [0.0; 2],
                high_shelf: None,
                spread_ms: 0.0,
                rate: ir::Rate::Hz(mod_rate),
                phase: 0.0,
            }),
            bypass: current.bypass,
            output: SQUARE_ROOT.eval(mix),
            dry: SQUARE_ROOT.eval(1.0 - mix),
        })
    },
};

/// Parameters every corpus program leaves at rest (DelayRatio, ModDepth,
/// PeakGain) are reported when set.
fn dual_delay_report(node: Node, notes: &mut Notes) -> Result<(), String> {
    for (name, rest) in [("DelayRatio", 0.0), ("ModDepth", 0.0), ("PeakGain", 0.0)] {
        if num(node, name, rest)? != rest {
            notes.add(ir::Impact::Audible, &format!("dual delay {name}"), "");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::load_one;
    use super::*;

    fn delay(attributes: &str) -> (ir::Effect, ir::Delay) {
        let xml = format!("<DualDelay {attributes}/>");
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let e = load_one(doc.root_element(), &mut Notes::default()).unwrap();
        let ir::EffectKind::Delay(d) = e.kind.clone() else {
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
