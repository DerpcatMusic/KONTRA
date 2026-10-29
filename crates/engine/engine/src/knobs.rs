//! Script knobs: a 0..=1 setting through the translator's law into an
//! effect control or an LFO setting (`EnginePar::Knob`).

use kontra_ir as ir;

use std::sync::Arc;

use crate::{Engine, EnginePar, Plan};

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
        let n = self.row(n, group, slot);
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
        *at = if target == ir::KnobTarget::LfoSteps {
            y.round().max(1.0)
        } else {
            y
        };
        self.refresh_all(None);
        true
    }

    /// `get_engine_par` of knob `n`: the setting whose law gives the value
    /// the control holds.
    pub(crate) fn knob(&mut self, n: u16, group: i32, slot: i32, generic: i32) -> Option<i32> {
        let n = self.row(n, group, slot);
        let target = self.script()?.knobs.get(usize::from(n))?.0;
        if let ir::KnobTarget::Slot(knob) = target {
            let x = self.slot_x(knob, group, slot, generic)?;
            let law = &self.script()?.knobs.get(usize::from(n))?.1;
            return Some((invert(law, x) * 1e6).round() as i32);
        }
        let y = *self.knob_mut(target, group, slot, generic)?;
        let law = &self.script()?.knobs.get(usize::from(n))?.1;
        Some((invert(law, y) * 1e6).round() as i32)
    }

    /// Row `n`, or the [`ir::KnobTarget::LfoSteps`] row after it when `n`
    /// sets the rate of a stepped LFO.
    fn row(&mut self, n: u16, group: i32, slot: i32) -> u16 {
        let next = n.saturating_add(1);
        let target = |e: &Self, k: u16| e.script()?.knobs.get(usize::from(k)).map(|r| r.0);
        if target(self, n) != Some(ir::KnobTarget::LfoRate)
            || target(self, next) != Some(ir::KnobTarget::LfoSteps)
        {
            return n;
        }
        let lfo = usize::try_from(group)
            .ok()
            .and_then(|g| self.groups.get_mut(g)?.lfo_mut(slot));
        match lfo.map(|l| l.rate) {
            Some(ir::Rate::Steps { .. }) => next,
            _ => n,
        }
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
        let plan = Arc::clone(&self.plan);
        let Some(at) = rack_at(group, generic) else {
            return false;
        };
        let mut hit = false;
        for (field, law) in laws(&plan, self.shape_on(&plan, at, slot), knob, at, slot) {
            let effect = self.effect_mut(group, slot, generic);
            if let Some(v) = effect.and_then(|e| field.field_mut(&mut e.kind)) {
                *v = law.eval(x);
                hit = true;
            }
        }
        if hit {
            self.refresh_all(None);
        }
        hit
    }

    /// The 0..=1 setting slot knob `knob` holds: its first field back
    /// through its law.
    fn slot_x(&mut self, knob: ir::SlotKnob, group: i32, slot: i32, generic: i32) -> Option<f32> {
        let plan = Arc::clone(&self.plan);
        let at = rack_at(group, generic)?;
        let (field, law) = laws(&plan, self.shape_on(&plan, at, slot), knob, at, slot).next()?;
        let y = *field.field_mut(&mut self.effect_mut(group, slot, generic)?.kind)?;
        Some(invert(law, y))
    }

    /// The shape the addressed slot last switched to, when it brings laws.
    fn shape_on<'p>(&self, plan: &'p Plan, at: ir::RackAt, slot: i32) -> Option<&'p ir::SlotShape> {
        let mut on = plan.slot_shapes.iter().zip(&self.behaviour.shapes);
        on.find(|(s, on)| **on && s.at == at && i32::from(s.slot) == slot)
            .map(|(s, _)| s)
            .filter(|s| !s.laws.is_empty())
    }

    /// `set_engine_par` of a filter slot's shape `id`; false when the slot
    /// has no such shape. The resonance keeps its setting through the new
    /// shape's laws.
    pub(crate) fn set_shape(&mut self, id: i32, group: i32, slot: i32, generic: i32) -> bool {
        let plan = Arc::clone(&self.plan);
        let Some(at) = rack_at(group, generic) else {
            return false;
        };
        let here = |s: &ir::SlotShape| s.at == at && i32::from(s.slot) == slot;
        let shapes = &plan.slot_shapes;
        let Some(n) = shapes.iter().position(|s| here(s) && s.id == id) else {
            return false;
        };
        let resonance = self.slot_x(ir::SlotKnob::Resonance, group, slot, generic);
        let Some(ir::EffectKind::Filter(f)) =
            self.effect_mut(group, slot, generic).map(|e| &mut e.kind)
        else {
            return false;
        };
        if let Some(shape) = shapes.get(n) {
            shape.apply(f);
        }
        for (k, (on, s)) in self.behaviour.shapes.iter_mut().zip(shapes).enumerate() {
            if here(s) {
                *on = k == n;
            }
        }
        if let Some(x) = resonance {
            self.set_slot_knob(ir::SlotKnob::Resonance, x, group, slot, generic);
        }
        self.refresh_all(None);
        true
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
            ir::KnobTarget::LfoRate
            | ir::KnobTarget::LfoSteps
            | ir::KnobTarget::LfoWidth
            | ir::KnobTarget::LfoWeight(_) => {
                let lfo = self
                    .groups
                    .get_mut(usize::try_from(group).ok()?)?
                    .lfo_mut(slot)?;
                match (target, &mut lfo.rate, &mut lfo.wave) {
                    (ir::KnobTarget::LfoRate, ir::Rate::Hz(hz), _) => Some(hz),
                    (ir::KnobTarget::LfoSteps, ir::Rate::Steps { count, .. }, _) => Some(count),
                    (
                        ir::KnobTarget::LfoWidth,
                        _,
                        ir::Wave::Square { width } | ir::Wave::Multi { width, .. },
                    ) => Some(width),
                    (ir::KnobTarget::LfoWeight(w), _, ir::Wave::Multi { weights, .. }) => {
                        weights.get_mut(usize::from(w))
                    }
                    _ => None,
                }
            }
        }
    }
}

/// The rack `group` and `generic` address, as `effect_mut` reads them.
fn rack_at(group: i32, generic: i32) -> Option<ir::RackAt> {
    Some(match (u32::try_from(group), generic) {
        (Ok(g), -1) => ir::RackAt::Group(g),
        (Err(_), 0) => ir::RackAt::Sends,
        (Err(_), 1) => ir::RackAt::Inserts,
        (Err(_), 2) => ir::RackAt::Main,
        (Err(_), b) => {
            ir::RackAt::Bus(u8::try_from(b.checked_sub(crate::params::BUS_OFFSET)?).ok()?)
        }
        _ => return None,
    })
}

/// The fields slot knob `knob` of slot `slot` in `at` moves, with their
/// laws: `shape`'s when it has them, else the slot's.
fn laws<'p>(
    plan: &'p Plan,
    shape: Option<&'p ir::SlotShape>,
    knob: ir::SlotKnob,
    at: ir::RackAt,
    slot: i32,
) -> impl Iterator<Item = (ir::EffectParam, &'p ir::Law)> {
    let own = shape.into_iter().flat_map(|s| s.laws.iter());
    let own = own.filter(move |l| l.0 == knob);
    let has = own.clone().next().is_some();
    let slots = plan
        .slot_laws
        .iter()
        .filter(move |l| !has && l.at == at && i32::from(l.slot) == slot && l.knob == knob);
    let own = own.map(|(_, f, l)| (*f, l));
    own.chain(slots.map(|l| (l.field, &l.law)))
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
