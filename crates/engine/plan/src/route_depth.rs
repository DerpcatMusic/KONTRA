//! A modulation route's signed depth, as scripts read and write it: an EQ
//! gain or effect route's depth over its span, a plain route's own depth.

use kontra_ir as ir;

/// A route's signed depth; an EQ gain or effect route's is its depth over
/// its span.
#[must_use]
pub fn signed(r: &ir::ModRoute) -> Option<f32> {
    match r.target {
        ir::ParamAddr::EqBandGain { .. } | ir::ParamAddr::EffectParam { .. } => {
            Some(r.depth / r.span)
        }
        _ => has_depth(r).then_some(r.depth),
    }
}

/// Sets a route's signed depth. Returns whether the route has one.
pub fn set_signed(r: &mut ir::ModRoute, d: f32) -> bool {
    match r.target {
        ir::ParamAddr::EqBandGain { .. } | ir::ParamAddr::EffectParam { .. } => {
            r.depth = d * r.span;
            true
        }
        // Start phase: bipolar (measured, probes/mod_par_laws.lua on
        // ANALOG's Phase_P1). Loop shift: signed; scripts set it bipolar and
        // cubed (v1 audits/MODULATION.md: ANALOG's 751086 lands on the
        // native 0.1266360730).
        _ if has_depth(r) => {
            r.depth = d;
            true
        }
        _ => false,
    }
}

/// Whether scripts read and write the route's depth: not the amplitude
/// envelope, a Hz-range cutoff, a zone mix cell or a send, which have none.
#[must_use]
pub fn has_depth(r: &ir::ModRoute) -> bool {
    !matches!(
        (r.target, r.law),
        (ir::ParamAddr::AmpEnvelope, _)
            | (ir::ParamAddr::FilterCutoff { .. }, ir::RouteLaw::Range(_))
            | (
                ir::ParamAddr::ZoneMix { .. } | ir::ParamAddr::BusSend { .. },
                _
            )
    )
}

/// `a` to `b` by `t`, for the controls a route moves.
#[must_use]
pub fn lerp(a: &ir::EffectKind, b: &ir::EffectKind, t: f32) -> ir::EffectKind {
    let mix = |x: f32, y: f32| x + (y - x) * t;
    match (a, b) {
        (ir::EffectKind::Shaper(s), ir::EffectKind::Shaper(e)) => {
            let mut s = s.clone();
            let mut to = e.curve.clone();
            if std::mem::discriminant(&s.curve) == std::mem::discriminant(&to) {
                let x = s.curve.amount_mut();
                *x = mix(*x, *to.amount_mut());
            }
            ir::EffectKind::Shaper(s)
        }
        // A strip's curve moves as a shaper's; its compressor takes the new
        // settings at once.
        (ir::EffectKind::Strip(s), ir::EffectKind::Strip(e)) => {
            let mut s = s.clone();
            let mut to = e.shaper.curve.clone();
            if std::mem::discriminant(&s.shaper.curve) == std::mem::discriminant(&to) {
                let x = s.shaper.curve.amount_mut();
                *x = mix(*x, *to.amount_mut());
            }
            s.compressor = e.compressor;
            ir::EffectKind::Strip(s)
        }
        (ir::EffectKind::Crush(c), ir::EffectKind::Crush(e)) => {
            let mut c = *c;
            (c.rate, c.bits) = (mix(c.rate, e.rate), mix(c.bits, e.bits));
            ir::EffectKind::Crush(c)
        }
        (a, _) => a.clone(),
    }
}

/// Whether a route moves insert `n`'s controls.
#[must_use]
pub fn moved(mods: &[ir::Mod], n: usize) -> bool {
    mods.iter().any(|m| match m.route.target {
        ir::ParamAddr::FilterCutoff { insert }
        | ir::ParamAddr::FilterResonance { insert }
        | ir::ParamAddr::EqBandGain { insert, .. }
        | ir::ParamAddr::EffectParam { insert, .. } => usize::from(insert) == n,
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
        let mut r = ir::ModRoute::new(
            ir::Source::Velocity,
            ir::ParamAddr::EqBandGain { insert: 0, band: 0 },
            -48.0,
        )
        .span(72.0)
        .law(ir::RouteLaw::Clamp(6.0));
        let d = signed(&r).unwrap();
        assert!(set_signed(&mut r, d));
        assert!((r.depth + 48.0).abs() < 1e-4);
        assert!(set_signed(&mut r, -0.25));
        assert_eq!(r.depth, -18.0);
        assert_eq!(signed(&r), Some(-0.25));
    }

    #[test]
    fn a_start_phase_route_depth_is_signed() {
        let phase = ir::ParamAddr::LfoParam {
            lfo: 0,
            param: ir::LfoParam::Phase,
        };
        let mut r = ir::ModRoute::new(ir::Source::Constant, phase, -1.0);
        assert_eq!(signed(&r), Some(-1.0));
        assert!(set_signed(&mut r, 0.5));
        assert_eq!(r.depth, 0.5);
    }

    #[test]
    fn a_loop_route_depth_is_signed() {
        let mut r = ir::ModRoute::new(
            ir::Source::Constant,
            ir::ParamAddr::LoopShift { length: false },
            0.0,
        );
        // ANALOG STRINGS writes 751086; Kontakt holds 0.1266360730 (v1 audit).
        assert!(set_signed(&mut r, (2.0f32 * 0.751_086 - 1.0).powi(3)));
        assert!((r.depth - 0.126_636_07).abs() < 1e-6, "{}", r.depth);
    }

    #[test]
    fn an_effect_route_depth_is_its_depth_over_its_span() {
        let mut r = ir::ModRoute::new(
            ir::Source::Velocity,
            ir::ParamAddr::EffectParam {
                insert: 0,
                param: ir::EffectParam::CrushBits,
            },
            -8.0,
        )
        .span(16.0);
        assert_eq!(signed(&r), Some(-0.5));
        assert!(set_signed(&mut r, -0.25));
        assert_eq!(r.depth, -4.0);
    }

    #[test]
    fn routes_without_a_depth_refuse_one() {
        for mut r in [
            ir::ModRoute::AMP_ENVELOPE,
            ir::ModRoute::new(
                ir::Source::Velocity,
                ir::ParamAddr::BusSend { send: 0 },
                1.0,
            ),
            ir::ModRoute::new(
                ir::Source::Velocity,
                ir::ParamAddr::FilterCutoff { insert: 0 },
                1.0,
            )
            .law(ir::RouteLaw::Range([20.0, 2000.0])),
        ] {
            assert_eq!(signed(&r), None);
            assert!(!set_signed(&mut r, 0.5));
        }
    }
}
