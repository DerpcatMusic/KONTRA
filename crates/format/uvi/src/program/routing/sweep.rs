//! UVI's Flanger onto `ir::Delay` and its Phasor onto `ir::Phaser`.
//!
//! The Flanger (audio method 0x1412eeec0, state 0x1412ef6d0): one sine LFO
//! (the 257-point table 0x1428393a0 that 0x14163c3a0 fills, rising first)
//! sweeps both channels' heads over `0.02·(DelayTime + Depth·(lfo + 1)/2)`
//! seconds (0.02 at 0x141cfa9e0); each line takes `x + Feedback·head`, and
//! the output is `x + Mix·head`. `Speed` is Hz, or quarter notes per cycle
//! when `SyncToHost` (`tempo/60/Speed`, 0x141e29edc).
//!
//! The Phasor (its filter's audio method 0x14166c120, parameters bound by
//! 0x1412f2d70): `Order` pairs of first-order allpasses
//! `y = a·x + s, s' = x − a·y` (0x14166e2a0), all at `a = (1 − c)/(1 + c)`
//! (0x14164c7e0) with `c = rate/(f·Order)`, `f = MaxFreq·(MinFreq/MaxFreq)^t`
//! and `t` the LFO in 0..=1; the loop takes `x + Feedback·y₋₁`, and the
//! output is `(x + Depth·y)/√(1 + Depth²)`. `LFOSHape` 0 is the triangle
//! table (0x142839bc0), 1 the sine, both rising first; `Spread` puts each
//! channel `Spread/(2·channels)` of a cycle ahead (0x14166d2f0). `Speed`
//! stops at 10 Hz (0x141e7df9c).
//!
//! The engine's allpass is the same section with the sign of `a` flipped,
//! which an even count of them cancels; its corner `w = π·hz/rate` is
//! `1/c`, so `hz = Order·f/π`. Its LFOs fall first: a free-running LFO
//! starts half a cycle off, which no note can hear.

use kontra_format::Notes;
use kontra_ir as ir;

use super::{Params, flag};

/// The Flanger at `node`.
pub(super) fn flanger(p: Params) -> Result<ir::Effect, String> {
    let depth = p.num("Depth", 0.5)?.clamp(0.0, 1.0) as f32;
    let time = p.num("DelayTime", 0.0)?.clamp(0.0, 1.0) as f32;
    let feedback = p.num("Feedback", 0.0)? as f32;
    let delay = ir::Delay {
        time: ir::DelayTime::Ms(20.0 * (time + depth / 2.0)),
        depth_ms: 10.0 * depth,
        second_head: false,
        input: ir::IDENTITY,
        feedback: [[feedback, 0.0], [0.0, feedback]],
        output: ir::IDENTITY,
        damping_hz: f32::INFINITY,
        low_cut_hz: 0.0,
        steep_low_cut: false,
        high_shelf: None,
        spread_ms: 0.0,
        sync: ir::DelaySync::FREE,
        rate: rate(p, f32::INFINITY)?,
        phase: 0.0,
    };
    Ok(ir::Effect {
        kind: ir::EffectKind::Delay(delay),
        bypass: false,
        output: p.num("Mix", 1.0)?.clamp(0.0, 1.0) as f32,
        dry: 1.0,
    })
}

/// The Phasor at `node`.
pub(super) fn phasor(p: Params, notes: &mut Notes) -> Result<ir::Effect, String> {
    let order = p.num("Order", 4.0)?.round().clamp(1.0, 12.0) as f32;
    let low = p.num("MinFreq", 200.0)?.max(1.0) as f32;
    let high = p.num("MaxFreq", 3000.0)?.max(1.0) as f32;
    let shape = p.num("LFOSHape", 0.0)?;
    if shape > 1.0 {
        notes.add(ir::Impact::Audible, "Phasor LFO shape", shape);
    }
    let hz = order / std::f32::consts::PI * (low * high).sqrt();
    let phaser = ir::Phaser {
        stages: 2 * order as u8,
        triangle: shape == 0.0,
        hz,
        spread_hz: hz,
        exponent: 0.5 * (high / low).ln(),
        feedback: p.num("Feedback", 0.0)? as f32,
        rate: rate(p, 10.0)?,
        phase: p.num("Spread", 1.0)?.clamp(0.0, 1.0) as f32 / 4.0,
    };
    let depth = p.num("Depth", 1.0)?.clamp(0.0, 1.0) as f32;
    let norm = (1.0 + depth * depth).sqrt();
    Ok(ir::Effect {
        kind: ir::EffectKind::Phaser(phaser),
        bypass: false,
        output: depth / norm,
        dry: 1.0 / norm,
    })
}

/// `Speed` in Hz up to `max_hz`, or quarter notes per cycle when synced.
fn rate(p: Params, max_hz: f32) -> Result<ir::Rate, String> {
    let speed = p.num("Speed", 1.0)?.max(1e-3) as f32;
    Ok(if flag(p.node, "SyncToHost") {
        ir::Rate::Beats(speed)
    } else {
        ir::Rate::Hz(speed.min(max_hz))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use roxmltree::Node;

    fn node<T>(xml: &str, f: impl FnOnce(Node) -> T) -> Option<T> {
        let doc = roxmltree::Document::parse(xml).ok()?;
        Some(f(doc.root_element()))
    }

    #[test]
    fn a_flanger_keeps_its_input_and_sweeps_from_its_delay_time() {
        let xml = r#"<Flanger SyncToHost="0" Speed="0.4" Feedback="0.4" DelayTime="0.2" Depth="0.5" Mix="0.9"/>"#;
        let Some(Ok(e)) = node(xml, |n| flanger(Params::of(n))) else {
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
        let Some(Ok(e)) = node(xml, |n| phasor(Params::of(n), &mut Notes::default())) else {
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
