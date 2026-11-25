//! UVI's Flanger onto `ir::Delay` and its Phasor onto `ir::Phaser`.
//!
//! The Flanger (audio method [ref], state [ref]): one sine LFO
//! (the 257-point table [ref] that [ref] fills, rising first)
//! sweeps both channels' heads over `0.02·(DelayTime + Depth·(lfo + 1)/2)`
//! seconds (0.02 at [ref]); each line takes `x + Feedback·head`, and
//! the output is `x + Mix·head`. `Speed` is Hz, or quarter notes per cycle
//! when `SyncToHost` (`tempo/60/Speed`, [ref]).
//!
//! The Phasor (its filter's audio method [ref], parameters bound by
//! [ref]): `Order` pairs of first-order allpasses
//! `y = a·x + s, s' = x − a·y`, all at `a = (1 − c)/(1 + c)`
//! with `c = rate/(f·Order)`, `f = MaxFreq·(MinFreq/MaxFreq)^t`
//! and `t` the LFO in 0..=1; the loop takes `x + Feedback·y₋₁`, and the
//! output is `(x + Depth·y)/√(1 + Depth²)`. `LFOSHape` 0 is the triangle
//! table, 1 the sine, both rising first; `Spread` puts each
//! channel `Spread/(2·channels)` of a cycle ahead. `Speed`
//! stops at 10 Hz.
//!
//! The engine's allpass is the same section with the sign of `a` flipped,
//! which an even count of them cancels; its corner `w = π·hz/rate` is
//! `1/c`, so `hz = Order·f/π`. Its LFOs fall first: a free-running LFO
//! starts half a cycle off, which no note can hear.

use kontra_format::Notes;
use kontra_ir as ir;
use roxmltree::Node;

use super::{Attr, UviModule, ids, nothing, num};

/// Flanger: `[DelayTime, Depth, Feedback, Speed, Mix, SyncToHost]`.
pub static FLANGER: UviModule = UviModule {
    id: ids::FLANGER,
    tag: "Flanger",
    attrs: &[
        Attr::num("DelayTime", 0.0),
        Attr::num("Depth", 0.5),
        Attr::num("Feedback", 0.0),
        Attr::num("Speed", 1.0),
        Attr::num("Mix", 1.0),
        Attr::flag("SyncToHost"),
    ],
    report: nothing,
    build_effect: |p, current| {
        let [time, depth, feedback, speed, mix, synced] = *p else {
            return None;
        };
        let depth = depth.clamp(0.0, 1.0);
        let time = time.clamp(0.0, 1.0);
        let speed = speed.max(1e-3);
        let rate = if synced > 0.5 {
            ir::Rate::Beats(speed)
        } else {
            ir::Rate::Hz(speed)
        };
        let delay = ir::Delay {
            direct: 0.0,
            time: ir::DelayTime::Ms(20.0 * (time + depth / 2.0)),
            depth_ms: 10.0 * depth,
            second_head: false,
            input: ir::IDENTITY,
            feedback: [[feedback, 0.0], [0.0, feedback]],
            output: ir::IDENTITY,
            damping_hz: f32::INFINITY,
            low_cut_hz: 0.0,
            steep_low_cut: false,
            body: std::borrow::Cow::Borrowed(&[]),
            triangle_s: None,
            shape: std::borrow::Cow::Borrowed(&[]),
            noise: [0.0; 2],
            high_shelf: None,
            spread_ms: 0.0,
            rate,
            phase: 0.0,
        };
        Some(ir::Effect {
            kind: ir::EffectKind::Delay(delay),
            bypass: current.bypass,
            output: mix.clamp(0.0, 1.0),
            dry: 1.0,
        })
    },
};

/// Phasor: `[MinFreq, MaxFreq, Feedback, Speed, Spread, Depth, Order, LFOSHape, SyncToHost]`.
pub static PHASOR: UviModule = UviModule {
    id: ids::PHASOR,
    tag: "Phasor",
    attrs: &[
        Attr::num("MinFreq", 200.0),
        Attr::num("MaxFreq", 3000.0),
        Attr::num("Feedback", 0.0),
        Attr::num("Speed", 1.0),
        Attr::num("Spread", 1.0),
        Attr::num("Depth", 1.0),
        Attr::num("Order", 4.0),
        Attr::num("LFOSHape", 0.0),
        Attr::flag("SyncToHost"),
    ],
    report: phasor_report,
    build_effect: |p, current| {
        let [
            low,
            high,
            feedback,
            speed,
            spread,
            depth,
            order,
            shape,
            synced,
        ] = *p
        else {
            return None;
        };
        let order = order.round().clamp(1.0, 12.0);
        let low = low.max(1.0);
        let high = high.max(1.0);
        let speed = speed.max(1e-3);
        let rate = if synced > 0.5 {
            ir::Rate::Beats(speed)
        } else {
            ir::Rate::Hz(speed.min(10.0))
        };
        let hz = order / std::f32::consts::PI * (low * high).sqrt();
        let phaser = ir::Phaser {
            stages: 2 * order as u8,
            triangle: shape == 0.0,
            hz,
            spread_hz: hz,
            exponent: 0.5 * (high / low).ln(),
            feedback,
            rate,
            phase: spread.clamp(0.0, 1.0) / 4.0,
        };
        let depth = depth.clamp(0.0, 1.0);
        let norm = (1.0 + depth * depth).sqrt();
        Some(ir::Effect {
            kind: ir::EffectKind::Phaser(phaser),
            bypass: current.bypass,
            output: depth / norm,
            dry: 1.0 / norm,
        })
    },
};

/// A Phasor LFO shape past the sine is reported.
fn phasor_report(node: Node, notes: &mut Notes) -> Result<(), String> {
    let shape = num(node, "LFOSHape", 0.0)?;
    if shape > 1.0 {
        notes.add(ir::Impact::Audible, "Phasor LFO shape", shape);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::load_one;
    use super::*;
    use roxmltree::Node;

    fn node<T>(xml: &str, f: impl FnOnce(Node) -> T) -> Option<T> {
        let doc = roxmltree::Document::parse(xml).ok()?;
        Some(f(doc.root_element()))
    }

    #[test]
    fn a_flanger_keeps_its_input_and_sweeps_from_its_delay_time() {
        let xml = r#"<Flanger SyncToHost="0" Speed="0.4" Feedback="0.4" DelayTime="0.2" Depth="0.5" Mix="0.9"/>"#;
        let Some(Ok(e)) = node(xml, |n| load_one(n, &mut Notes::default())) else {
            panic!("not carried");
        };
        let ir::EffectKind::Delay(d) = e.kind else {
            panic!("not a delay");
        };
        assert_eq!(d.time, ir::DelayTime::Ms(9.0));
        assert_eq!(d.depth_ms, 5.0);
        assert_eq!((e.output, e.dry), (0.9, 1.0));
    }

    #[test]
    fn a_phasor_runs_two_allpasses_per_order_between_its_frequencies() {
        let xml = r#"<Phasor SyncToHost="0" Speed="0.5" MinFreq="200" MaxFreq="3000" Feedback="0.4" Depth="1" Spread="1" Order="5" LFOSHape="0"/>"#;
        let Some(Ok(e)) = node(xml, |n| load_one(n, &mut Notes::default())) else {
            panic!("not carried");
        };
        let ir::EffectKind::Phaser(p) = e.kind else {
            panic!("not a phaser");
        };
        assert_eq!((p.stages, p.triangle, p.phase), (10, true, 0.25));
        // At the LFO's extremes the corner is Order·f/π at each frequency.
        let corner = |lfo: f32| p.hz + p.spread_hz * ((p.exponent * lfo).exp() - 1.0);
        let want = |f: f32| 5.0 * f / std::f32::consts::PI;
        assert!((corner(1.0) / want(3000.0) - 1.0).abs() < 1e-5);
        assert!((corner(-1.0) / want(200.0) - 1.0).abs() < 1e-5);
        assert!((e.output - e.dry).abs() < 1e-6);
    }
}
