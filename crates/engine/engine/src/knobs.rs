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
        if let ir::KnobTarget::Slot(knob) = target {
            return self.set_slot_knob(knob, y, group, slot, generic);
        }
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
        if let ir::KnobTarget::Slot(knob) = target {
            let law = self.slot_law(knob, group, slot, generic)?;
            let y = *law
                .field
                .field_mut(&mut self.effect_mut(group, slot, generic)?.kind)?;
            let x = invert(&self.plan.slot_laws.get(law.index)?.law, y);
            let law = &self.script()?.knobs.get(usize::from(n))?.1;
            return Some((invert(law, x) * 1e6).round() as i32);
        }
        let y = *self.knob_mut(target, group, slot, generic)?;
        let law = &self.script()?.knobs.get(usize::from(n))?.1;
        Some((invert(law, y) * 1e6).round() as i32)
    }

    /// Sets every field slot knob `knob` moves to its law at `x`.
    fn set_slot_knob(
        &mut self,
        knob: ir::SlotKnob,
        x: f32,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> bool {
        let mut hit = false;
        for index in 0..self.plan.slot_laws.len() {
            let Some(at) = self.slot_at(index, knob, group, slot, generic) else {
                continue;
            };
            let Some(l) = self.plan.slot_laws.get(at) else {
                continue;
            };
            let (field, y) = (l.field, l.law.eval(x));
            let effect = self.effect_mut(group, slot, generic);
            if let Some(v) = effect.and_then(|e| field.field_mut(&mut e.kind)) {
                *v = y;
                hit = true;
            }
        }
        if hit {
            self.refresh_all(None);
        }
        hit
    }

    /// The first law of slot knob `knob` at the addressed slot.
    fn slot_law(&self, knob: ir::SlotKnob, group: i32, slot: i32, generic: i32) -> Option<Found> {
        (0..self.plan.slot_laws.len()).find_map(|i| {
            let index = self.slot_at(i, knob, group, slot, generic)?;
            let field = self.plan.slot_laws.get(index)?.field;
            Some(Found { index, field })
        })
    }

    /// `index` when that slot law is knob `knob` of the addressed slot: a
    /// group's insert (`generic` −1) or an instrument rack slot.
    fn slot_at(
        &self,
        index: usize,
        knob: ir::SlotKnob,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> Option<usize> {
        let l = self.plan.slot_laws.get(index)?;
        // The addressing of `effect_mut`.
        let at = match (u32::try_from(group), generic) {
            (Ok(g), -1) => ir::RackAt::Group(g),
            (Err(_), 0) => ir::RackAt::Sends,
            (Err(_), 1) => ir::RackAt::Inserts,
            (Err(_), 2) => ir::RackAt::Main,
            (Err(_), b) => {
                ir::RackAt::Bus(u8::try_from(b.checked_sub(crate::params::BUS_OFFSET)?).ok()?)
            }
            _ => return None,
        };
        (l.at == at && i32::from(l.slot) == slot && l.knob == knob).then_some(index)
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
            ir::KnobTarget::Slot(_) => None,
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

/// A slot law found for a knob: its index in the plan and the field it sets.
struct Found {
    index: usize,
    field: ir::EffectParam,
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
