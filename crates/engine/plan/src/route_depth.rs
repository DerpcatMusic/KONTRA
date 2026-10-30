//! A modulation route's signed depth, as scripts read and write it: an EQ
//! gain or effect route's depth over its span, a plain route's own depth.

use kontra_ir as ir;

/// A route's signed depth; an EQ gain or effect route's is its depth over
/// its span.
#[must_use]
pub fn signed(kind: &ir::ModKind) -> Option<f32> {
    match *kind {
        ir::ModKind::EqGain {
            depth_db, span_db, ..
        } => Some(depth_db / span_db),
        ir::ModKind::EffectParam { depth, span, .. } => Some(depth / span),
        ir::ModKind::LfoPhase { depth, .. } | ir::ModKind::LoopShift { depth, .. } => Some(depth),
        ref other => depth(&mut other.clone()).map(|d| *d),
    }
}

/// Sets a route's signed depth. Returns whether the route has one.
pub fn set_signed(kind: &mut ir::ModKind, d: f32) -> bool {
    if let ir::ModKind::EqGain {
        depth_db, span_db, ..
    } = kind
    {
        *depth_db = d * *span_db;
        return true;
    }
    if let ir::ModKind::EffectParam { depth, span, .. } = kind {
        *depth = d * *span;
        return true;
    }
    // Bipolar (measured, probes/mod_par_laws.lua on ANALOG's Phase_P1).
    if let ir::ModKind::LfoPhase { depth, .. } = kind {
        *depth = d;
        return true;
    }
    // Signed; scripts set it bipolar and cubed (v1 audits/MODULATION.md:
    // ANALOG's 751086 lands on the native 0.1266360730).
    if let ir::ModKind::LoopShift { depth, .. } = kind {
        *depth = d;
        return true;
    }
    depth(kind).map(|v| *v = d).is_some()
}

/// The depth field of a route that has a plain one.
pub fn depth(kind: &mut ir::ModKind) -> Option<&mut f32> {
    match kind {
        ir::ModKind::Volume { intensity, .. }
        | ir::ModKind::Attack { intensity, .. }
        | ir::ModKind::Release { intensity, .. } => Some(intensity),
        ir::ModKind::Pitch { depth, .. }
        | ir::ModKind::Cutoff { depth, .. }
        | ir::ModKind::Resonance { depth, .. }
        | ir::ModKind::SampleStart { depth, .. }
        | ir::ModKind::Intensity { depth, .. }
        | ir::ModKind::LfoRate { depth, .. }
        | ir::ModKind::LfoHz { depth, .. } => Some(depth),
        ir::ModKind::Pan { amount, .. } => Some(amount),
        ir::ModKind::Envelope
        | ir::ModKind::EqGain { .. }
        | ir::ModKind::EffectParam { .. }
        | ir::ModKind::Mix { .. }
        | ir::ModKind::Send { .. }
        | ir::ModKind::LfoPhase { .. }
        | ir::ModKind::LoopShift { .. } => None,
    }
}

/// `a` to `b` by `t`, for the controls a route moves.
#[must_use]
pub fn lerp(a: ir::EffectKind, b: ir::EffectKind, t: f32) -> ir::EffectKind {
    let mix = |x: f32, y: f32| x + (y - x) * t;
    match (a, b) {
        (ir::EffectKind::Shaper(mut s), ir::EffectKind::Shaper(e)) => {
            let mut to = e.curve;
            if std::mem::discriminant(&s.curve) == std::mem::discriminant(&to) {
                let x = s.curve.amount_mut();
                *x = mix(*x, *to.amount_mut());
            }
            ir::EffectKind::Shaper(s)
        }
        (ir::EffectKind::Crush(mut c), ir::EffectKind::Crush(e)) => {
            (c.rate, c.bits) = (mix(c.rate, e.rate), mix(c.bits, e.bits));
            ir::EffectKind::Crush(c)
        }
        (a, _) => a,
    }
}

/// Whether a route moves insert `n`'s controls.
#[must_use]
pub fn moved(mods: &[ir::Mod], n: usize) -> bool {
    mods.iter().any(|m| match m.kind {
        ir::ModKind::Cutoff { insert, .. }
        | ir::ModKind::Resonance { insert, .. }
        | ir::ModKind::EqGain { insert, .. }
        | ir::ModKind::EffectParam { insert, .. } => usize::from(insert) == n,
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::{set_signed, signed};
    use kontra_ir as ir;

    #[test]
    fn an_eq_gain_depth_is_its_gain_over_its_span() {
        // A depth past twice the limit survives a read and write back.
        let mut kind = ir::ModKind::EqGain {
            source: ir::Source::Velocity,
            insert: 0,
            band: 0,
            depth_db: -48.0,
            limit_db: 6.0,
            span_db: 72.0,
        };
        let d = signed(&kind).unwrap();
        assert!(set_signed(&mut kind, d));
        assert!(
            matches!(kind, ir::ModKind::EqGain { depth_db, .. } if (depth_db + 48.0).abs() < 1e-4)
        );
        assert!(set_signed(&mut kind, -0.25));
        assert!(matches!(kind, ir::ModKind::EqGain { depth_db, .. } if depth_db == -18.0));
        assert_eq!(signed(&kind), Some(-0.25));
    }

    #[test]
    fn a_start_phase_route_depth_is_signed() {
        let mut kind = ir::ModKind::LfoPhase {
            source: ir::Source::Constant,
            lfo: 0,
            depth: -1.0,
        };
        assert_eq!(signed(&kind), Some(-1.0));
        assert!(set_signed(&mut kind, 0.5));
        assert!(matches!(kind, ir::ModKind::LfoPhase { depth, .. } if depth == 0.5));
    }

    #[test]
    fn a_loop_route_depth_is_signed() {
        let mut kind = ir::ModKind::LoopShift {
            source: ir::Source::Constant,
            length: false,
            depth: 0.0,
        };
        // ANALOG STRINGS writes 751086; Kontakt holds 0.1266360730 (v1 audit).
        assert!(set_signed(&mut kind, (2.0f32 * 0.751_086 - 1.0).powi(3)));
        let ir::ModKind::LoopShift { depth, .. } = kind else {
            panic!("a loop route");
        };
        assert!((depth - 0.126_636_07).abs() < 1e-6, "{depth}");
    }

    #[test]
    fn an_effect_route_depth_is_its_depth_over_its_span() {
        let mut kind = ir::ModKind::EffectParam {
            source: ir::Source::Velocity,
            insert: 0,
            param: ir::EffectParam::CrushBits,
            depth: -8.0,
            span: 16.0,
            unipolar: false,
        };
        assert_eq!(signed(&kind), Some(-0.5));
        assert!(set_signed(&mut kind, -0.25));
        assert!(matches!(kind, ir::ModKind::EffectParam { depth, .. } if depth == -4.0));
    }
}
