//! Script knobs: a 0..=1 setting through the translator's law into an
//! effect control or an LFO setting (`EnginePar::Knob`).

use kontra_ir as ir;

use crate::{Engine, EnginePar};

impl Engine {
    /// `set_engine_par` of a knob. False when `par` is not one, or the slot
    /// has no such control.
    pub(crate) fn set_knob(
        &mut self,
        par: EnginePar,
        value: i32,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> bool {
        let EnginePar::Knob(n) = par else {
            return false;
        };
        let x = value.clamp(0, 1_000_000) as f32 / 1e6;
        let Some((target, y)) = self
            .script()
            .and_then(|s| s.knobs.get(usize::from(n)))
            .map(|(t, law)| (*t, law.eval(x)))
        else {
            return false;
        };
        let Some(at) = self.knob_mut(target, group, slot, generic) else {
            return false;
        };
        *at = y;
        self.refresh_all(None);
        true
    }

    /// `get_engine_par` of knob `n`: the setting whose law gives the value
    /// the control holds.
    pub(crate) fn knob(&mut self, n: u16, group: i32, slot: i32, generic: i32) -> Option<i32> {
        let target = self.script()?.knobs.get(usize::from(n))?.0;
        let y = *self.knob_mut(target, group, slot, generic)?;
        let law = &self.script()?.knobs.get(usize::from(n))?.1;
        Some((invert(law, y) * 1e6).round() as i32)
    }

    fn knob_mut(
        &mut self,
        target: ir::KnobTarget,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> Option<&mut f32> {
        match target {
            ir::KnobTarget::Effect(p) => {
                p.field_mut(&mut self.effect_mut(group, slot, generic)?.kind)
            }
            ir::KnobTarget::LfoRate | ir::KnobTarget::LfoWeight(_) => {
                let lfo = self
                    .groups
                    .get_mut(usize::try_from(group).ok()?)?
                    .lfo_mut(slot)?;
                match (target, &mut lfo.rate, &mut lfo.wave) {
                    // ponytail: a synced LFO's knob is a note count whose law
                    // is not read; its writes stay unanswered.
                    (ir::KnobTarget::LfoRate, ir::Rate::Hz(hz), _) => Some(hz),
                    (ir::KnobTarget::LfoWeight(w), _, ir::Wave::Multi { weights, .. }) => {
                        weights.get_mut(usize::from(w))
                    }
                    _ => None,
                }
            }
        }
    }
}

/// The 0..=1 setting `law` maps to `y`, by bisection; the laws are monotonic.
fn invert(law: &ir::Law, y: f32) -> f32 {
    let rising = law.eval(1.0) >= law.eval(0.0);
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..32 {
        let mid = 0.5 * (lo + hi);
        if (law.eval(mid) < y) == rising {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}
