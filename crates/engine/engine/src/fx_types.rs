//! Effects a script loads into a slot by type (`$ENGINE_PAR_EFFECT_TYPE`, or
//! an `EFFECT_SUBTYPE` of another kind: [`ir::SlotType`]). In `on init` the
//! slot changes at once; the racks are built after it. Later the processor
//! is built off the audio thread: the engine queues an [`EffectRequest`], the
//! host builds it ([`Prepared::new`]) and hands it to [`Engine::install`],
//! which swaps it in and only then completes the async id the script waits on.
//! A library response a script loads into a convolution after `on init`
//! (`load_ir_sample`) takes the same path, decoded where it is built.

use kontra_ir as ir;

use crate::queue::Work;
use crate::racks::{self, Dsp, Rack, Responses};
use crate::script::Kind;
use crate::{Engine, Plan, Response};

/// Requests the engine holds until the host drains them; more answer as
/// not loaded.
pub(crate) const REQUESTS: usize = 16;
/// Slots that may hold a loaded type at once.
pub(crate) const LOADED: usize = 256;

/// Slot `slot` of `at` to load with type `ty` of the plan's
/// [`ir::SlotShapes::types`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectRequest {
    pub at: ir::RackAt,
    pub slot: u8,
    /// What a project saves to load the slot again ([`Plan::load_type`]);
    /// `None` for a response, or for the slot's own effect built again.
    pub ty: Option<u16>,
    pub effect: Option<ir::Effect>,
    /// Instead of a type, a library response for the slot's convolution:
    /// its index in [`Plan::responses`] and how to prepare it.
    pub response: Option<(u16, ir::Impulse)>,
    script: u8,
    id: i32,
}

/// A request with its processor built.
pub struct Prepared {
    request: EffectRequest,
    dsp: Dsp,
    /// A requested response, decoded: its name in the plan, how it was
    /// prepared, the response.
    response: Option<(String, ir::Impulse, Response)>,
}

impl Prepared {
    /// Builds the processor `request` loads, at `rate`, decoding a response
    /// from `plan`'s. Not real-time.
    #[must_use]
    pub fn new(request: EffectRequest, plan: Option<&Plan>, rate: u32) -> Self {
        let response = request.response.and_then(|(index, law)| {
            let (name, sample) = plan?.responses.get(usize::from(index))?;
            Some((name.clone(), law, crate::response(sample, &law, rate).ok()?))
        });
        let dsp = match (&response, request.response) {
            (Some((_, _, r)), _) => racks::convolver(r),
            (None, Some(_)) => Dsp::None,
            // Routed, so a script's later knob writes retune it.
            (None, None) => racks::processor(request.effect, true, rate, &Responses(&[], &[])),
        };
        Self {
            request,
            dsp,
            response,
        }
    }

    #[must_use]
    pub fn request(&self) -> &EffectRequest {
        &self.request
    }

    /// The plan's name of the response it decoded; `None` when it asked for
    /// none or the decode failed (the slot then keeps what it plays).
    #[must_use]
    pub fn response_name(&self) -> Option<&str> {
        self.response.as_ref().map(|(name, ..)| name.as_str())
    }
}

impl Engine {
    /// `set_engine_par` of an effect type, or of a subtype of another kind:
    /// the answer the script gets (−1 in `on init`, else the async id), or
    /// `None` when the type is unknown or the slot cannot change now.
    pub(crate) fn load_type(
        &mut self,
        id: i32,
        subtype: bool,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> Option<i32> {
        let types = &self.plan.slot_shapes.types;
        let ty = types
            .iter()
            .position(|t| t.id == id && t.subtype == subtype)?;
        let (ty, effect) = (u16::try_from(ty).ok()?, types.get(ty)?.effect);
        let (at, n) = (
            crate::knobs::rack_at(group, generic)?,
            u8::try_from(slot).ok()?,
        );
        // A send slot returns only what it makes: its dry level starts at 0
        // (Kontakt 8.13: a loaded reverb's, measured).
        let effect = effect.map(|e| match at {
            ir::RackAt::Sends => ir::Effect { dry: 0.0, ..e },
            _ => e,
        });
        let init = self.running.kind == Kind::Init;
        // Loading the type a slot holds keeps it as it is (Kontakt 8.13: a
        // reverb's size stays through a reverb load; measured). A subtype
        // is held when the slot reads it; a type, by its kind of effect.
        let kind = |e: &Option<ir::Effect>| e.map(|e| std::mem::discriminant(&e.kind));
        let holds = kind(self.slot_mut(group, slot, generic)?);
        let same = match subtype {
            true => self.subtype(group, slot, generic) == Some(id),
            false => holds.is_some() && holds == kind(&effect),
        };
        if same {
            return Some(if init { -1 } else { self.start_async() });
        }
        if init {
            *self.slot_mut(group, slot, generic)? = effect;
            self.mark_loaded(at, n, ty);
            return Some(-1);
        }
        self.queue(at, n, Some(ty), effect, None)
    }

    /// `set_engine_par` of a reverb's network, entry `n` of the script
    /// plan's. In `on init` the slot changes at once; later the reverb is
    /// built again off the audio thread with its controls as they are, and
    /// its tail starts over. False when the slot holds no reverb or there is
    /// no such network.
    pub(crate) fn set_reverb_network(
        &mut self,
        n: i32,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> bool {
        let network = usize::try_from(n)
            .ok()
            .and_then(|n| self.script()?.reverb_networks.get(n).copied());
        let init = self.running.kind == Kind::Init;
        let Some(slot_now) = self.effect_mut(group, slot, generic) else {
            return false;
        };
        let (Some(network), ir::EffectKind::Reverb(r)) = (network, &mut slot_now.kind) else {
            return false;
        };
        if r.network == Some(network) {
            return true;
        }
        if init {
            r.network = Some(network);
            return true;
        }
        let mut effect = *slot_now;
        if let ir::EffectKind::Reverb(r) = &mut effect.kind {
            r.network = Some(network);
        }
        let at = crate::knobs::rack_at(group, generic);
        let n = u8::try_from(slot).ok();
        at.zip(n)
            .and_then(|(at, n)| self.queue(at, n, None, Some(effect), None))
            .is_some()
    }

    /// Queues the plan's response `index`, prepared by `law`, for the
    /// convolution at `slot` of the `generic` rack.
    pub(crate) fn request_response(
        &mut self,
        slot: i32,
        generic: i32,
        index: u16,
        law: ir::Impulse,
    ) -> Option<i32> {
        let at = crate::knobs::rack_at(-1, generic)?;
        self.queue(at, u8::try_from(slot).ok()?, None, None, Some((index, law)))
    }

    /// Queues a request: its async id, or `None` when the queue is full or
    /// the slot is a group's.
    fn queue(
        &mut self,
        at: ir::RackAt,
        slot: u8,
        ty: Option<u16>,
        effect: Option<ir::Effect>,
        response: Option<(u16, ir::Impulse)>,
    ) -> Option<i32> {
        // ponytail: group inserts run per voice and per chain; they change in
        // `on init` only.
        if matches!(at, ir::RackAt::Group(_)) || self.requests.len() >= REQUESTS {
            return None;
        }
        let (script, id) = (self.running.slot, self.next_async_id());
        self.requests.push(EffectRequest {
            at,
            slot,
            ty,
            effect,
            response,
            script,
            id,
        });
        Some(id)
    }

    /// Hands each queued request to `f`. Audio thread, between blocks.
    pub fn drain_requests(&mut self, mut f: impl FnMut(EffectRequest)) {
        for r in self.requests.drain(..) {
            f(r);
        }
    }

    /// Puts `prepared` in its slot and completes the async id its script
    /// waits on; one that cannot go in leaves the slot as it was. What it
    /// replaces comes back, to be dropped off the audio thread. No
    /// allocation.
    pub fn install(&mut self, prepared: Prepared) -> Prepared {
        let Prepared {
            request: r,
            dsp,
            mut response,
        } = prepared;
        let effect = match r.response {
            None => Some(r.effect),
            Some(_) => self.retarget(r.at, r.slot, &mut response).map(Some),
        };
        let swapped = match (effect, self.rack_slot(r.at, r.slot)) {
            (Some(effect), Some((rack, n))) => rack.install(n, effect, dsp),
            _ => Err(dsp),
        };
        if let (true, Some(ty)) = (swapped.is_ok(), r.ty) {
            self.mark_loaded(r.at, r.slot, ty);
        }
        let (effect, dsp) = swapped.unwrap_or_else(|dsp| (r.effect, dsp));
        let work = Work::Async {
            slot: r.script,
            id: r.id,
        };
        if !self.queue.push(self.now, work) {
            self.stats.events_refused += 1;
        }
        Prepared {
            request: EffectRequest { effect, ..r },
            dsp,
            response,
        }
    }

    /// The convolution at `slot` of `at` playing `response`, which moves
    /// into the loaded responses unless an equal one is there; `None` when
    /// the slot is no convolution or there is no room. No allocation.
    fn retarget(
        &mut self,
        at: ir::RackAt,
        slot: u8,
        response: &mut Option<(String, ir::Impulse, Response)>,
    ) -> Option<ir::Effect> {
        let (rack, n) = self.rack_slot(at, slot)?;
        let current = (*rack.slots.get(n)?)?;
        let ir::EffectKind::Convolution { .. } = current.kind else {
            return None;
        };
        let (name, law, _) = response.as_ref()?;
        let known = (self.loaded.iter()).position(|(m, l, _)| m == name && l == law);
        let j = match known {
            Some(j) => j,
            None if self.loaded.len() < self.loaded.capacity() => {
                self.loaded.push(response.take()?);
                self.loaded.len() - 1
            }
            None => return None,
        };
        let impulse = u16::try_from(self.plan.impulses.len() + j).ok()?;
        Some(ir::Effect {
            kind: ir::EffectKind::Convolution { impulse },
            ..current
        })
    }

    /// The type slot `slot` of `at` was last loaded with.
    pub(crate) fn loaded_type(&self, at: ir::RackAt, slot: i32) -> Option<u16> {
        let here = |&&(a, s, _): &&(ir::RackAt, u8, u16)| a == at && i32::from(s) == slot;
        self.behaviour.loaded.iter().find(here).map(|l| l.2)
    }

    /// `get_engine_par($ENGINE_PAR_EFFECT_SUBTYPE)`: the shape the slot last
    /// switched to, else what its loaded type reads, else its saved subtype.
    pub(crate) fn subtype(&self, group: i32, slot: i32, generic: i32) -> Option<i32> {
        let (plan, at) = (&self.plan, crate::knobs::rack_at(group, generic)?);
        let shapes = &plan.slot_shapes;
        let switched = crate::knobs::shape_slot(plan, at, slot)
            .and_then(|n| self.behaviour.shapes.get(n).copied().flatten())
            .and_then(|on| shapes.shapes.get(usize::from(on)));
        if let Some(shape) = switched {
            return Some(shape.id);
        }
        if let Some(ty) = self.loaded_type(at, slot) {
            return shapes.types.get(usize::from(ty)).map(|t| t.reads);
        }
        // ponytail: a linear scan, run only by scripts that read it.
        let saved = shapes
            .subtypes
            .iter()
            .find(|s| s.0 == at && i32::from(s.1) == slot);
        saved.map(|s| s.2)
    }

    fn mark_loaded(&mut self, at: ir::RackAt, slot: u8, ty: u16) {
        // A load replaces the unit a shape switch changed.
        if let Some(n) = crate::knobs::shape_slot(&self.plan, at, i32::from(slot))
            && let Some(on) = self.behaviour.shapes.get_mut(n)
        {
            *on = None;
        }
        let loaded = &mut self.behaviour.loaded;
        loaded.retain(|&(a, s, _)| (a, s) != (at, slot));
        if loaded.len() < loaded.capacity() {
            loaded.push((at, slot, ty));
        }
    }

    /// The rack and index slot `slot` of `at` addresses outside the groups.
    fn rack_slot(&mut self, at: ir::RackAt, slot: u8) -> Option<(&mut Rack, usize)> {
        let n = usize::from(slot);
        match at {
            ir::RackAt::Group(_) => None,
            ir::RackAt::Inserts => Some((&mut self.rack, n)),
            ir::RackAt::Main => Some((&mut self.main, n)),
            ir::RackAt::Sends => Some((self.sends.get_mut(n)?, 0)),
            ir::RackAt::Bus(b) => Some((&mut self.buses.get_mut(usize::from(b) + 1)?.rack, n)),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_prepared_effect_crosses_threads() {
        fn send<T: Send>() {}
        send::<super::Prepared>();
    }
}
