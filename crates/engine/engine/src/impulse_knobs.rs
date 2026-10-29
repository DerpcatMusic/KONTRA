//! Script knobs on how a convolution slot's response is prepared
//! ([`ir::KnobTarget::Impulse`]): the slot plays its response prepared
//! again. In `on init` a change waits for the racks' rebuild, so a slot
//! decodes once however many settings its init writes; later the host
//! prepares it off the audio thread, as a `load_ir_sample` after init.

use std::sync::Arc;

use kontra_ir as ir;

use crate::Engine;
use crate::script::Kind;

/// A slot `on init` re-prepared: slot, generic, its response's index in
/// the plan's responses and how it is prepared.
pub(crate) type InitImpulse = (i32, i32, u16, ir::Impulse);

impl Engine {
    /// `set_engine_par` of impulse knob `knob` at `y`. False when the slot
    /// plays no response the plan can prepare again.
    pub(crate) fn set_impulse_knob(
        &mut self,
        knob: ir::ImpulseKnob,
        y: f32,
        slot: i32,
        generic: i32,
    ) -> bool {
        let Some((Some(found), mut law)) = self.playing(slot, generic) else {
            return false;
        };
        let at = knob.field_mut(&mut law);
        if *at == y {
            return true;
        }
        *at = y;
        if self.running.kind == Kind::Init {
            self.init_impulses.retain(|p| (p.0, p.1) != (slot, generic));
            self.init_impulses.push((slot, generic, found, law));
            return true;
        }
        self.request_response(slot, generic, found, law).is_some()
    }

    /// The setting impulse knob `knob` holds at the slot.
    pub(crate) fn impulse_knob(
        &mut self,
        knob: ir::ImpulseKnob,
        slot: i32,
        generic: i32,
    ) -> Option<f32> {
        let (_, mut law) = self.playing(slot, generic)?;
        Some(*knob.field_mut(&mut law))
    }

    /// The convolution at `slot` of the `generic` rack: its response's index
    /// in the plan's responses (`None` when they do not hold it) and how it
    /// is prepared, as `on init` last left it.
    pub(crate) fn playing(
        &mut self,
        slot: i32,
        generic: i32,
    ) -> Option<(Option<u16>, ir::Impulse)> {
        let pending = self
            .init_impulses
            .iter()
            .find(|p| (p.0, p.1) == (slot, generic));
        if let Some(&(_, _, found, law)) = pending {
            return Some((Some(found), law));
        }
        let &ir::Effect {
            kind: ir::EffectKind::Convolution { impulse },
            ..
        } = self.effect_mut(-1, slot, generic).map(|e| &*e)?
        else {
            return None;
        };
        let plan = Arc::clone(&self.plan);
        let at = usize::from(impulse);
        let (name, law) = match at.checked_sub(plan.impulses.len()) {
            None => plan.impulse_laws.get(at).map(|(n, law)| (n.as_str(), *law)),
            Some(j) => self.loaded.get(j).map(|(n, law, _)| (n.as_str(), *law)),
        }?;
        let found = (plan.responses.iter()).position(|(n, _)| n.eq_ignore_ascii_case(name));
        Some((found.and_then(|f| u16::try_from(f).ok()), law))
    }

    /// Decodes each response `on init` re-prepared into its slot. Not
    /// real-time.
    pub(crate) fn prepare_init_impulses(&mut self) {
        let plan = Arc::clone(&self.plan);
        for (slot, generic, found, law) in std::mem::take(&mut self.init_impulses) {
            let Some((name, sample)) = plan.responses.get(usize::from(found)) else {
                continue;
            };
            let known = (self.loaded.iter()).position(|(n, l, _)| n == name && *l == law);
            let j = match known {
                Some(j) => j,
                None => match crate::response(sample, &law, self.rate) {
                    Ok(r) => {
                        self.loaded.push((name.clone(), law, r));
                        self.loaded.len() - 1
                    }
                    Err(_) => continue,
                },
            };
            let Ok(impulse) = u16::try_from(plan.impulses.len() + j) else {
                continue;
            };
            if let Some(e) = self.effect_mut(-1, slot, generic) {
                e.kind = ir::EffectKind::Convolution { impulse };
            }
        }
        // Room for loads after init, which must not allocate.
        self.loaded.reserve(crate::fx_types::REQUESTS);
    }
}
