//! Effects a script loads into a slot by type (`$ENGINE_PAR_EFFECT_TYPE`, or
//! an `EFFECT_SUBTYPE` of another kind: [`ir::SlotType`]). In `on init` the
//! slot changes at once; the racks are built after it. Later the processor
//! is built off the audio thread: the engine queues an [`EffectRequest`], the
//! host builds it ([`Prepared::new`]) and hands it to [`Engine::install`],
//! which swaps it in and only then completes the async id the script waits on.
//! A library response a script loads into a convolution after `on init`
//! (`load_ir_sample`) takes the same path, decoded where it is built.

use kontra_ir as ir;

use crate::queue::Exit;
use crate::racks::{self, Dsp, Rack, Responses};
use crate::script::Kind;
use crate::{Engine, Plan, Response};

/// Requests the engine holds until the host drains them; more answer as
/// not loaded.
pub(crate) const REQUESTS: usize = 16;

/// Slot `slot` of `at` to load with type `ty` of the plan's
/// [`ir::SlotShapes::types`].
#[derive(Clone, Debug, PartialEq)]
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
    epoch: u64,
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
            (None, None) => racks::processor(request.effect.as_ref(), rate, &Responses(&[], &[])),
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
        let target = types.get(ty)?;
        let (ty, target_kind, effect) =
            (u16::try_from(ty).ok()?, target.kind, target.effect.clone());
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
        let init = self.building || self.running.kind == Kind::Init;
        // Loading the type a slot holds keeps it as it is (Kontakt 8.13: a
        // reverb's size stays through a reverb load; measured). Compare the
        // stored effect-type / subtype ID rather than the `ir::EffectKind`
        // discriminant so distinct modules sharing an IR family always swap.
        let _ = self.slot_mut(group, slot, generic)?;
        let same = match subtype {
            true => self.subtype(group, slot, generic) == Some(id),
            false => self.effect_type(group, slot, generic) == Some(target_kind),
        };
        if same {
            return Some(if init {
                -1
            } else {
                self.start_async(Exit::Done)
            });
        }
        if init {
            if group < 0 {
                self.wanted.retain(|p| (p.0, p.1) != (slot, generic));
            }
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
            .and_then(|n| self.script()?.reverb_networks.get(n).cloned());
        let init = self.building || self.running.kind == Kind::Init;
        let Some(slot_now) = self.effect_mut(group, slot, generic) else {
            return false;
        };
        let (Some(network), ir::EffectKind::Reverb(r)) = (network, &mut slot_now.kind) else {
            return false;
        };
        if r.network.as_ref() == Some(&network) {
            return true;
        }
        if init {
            r.network = Some(network);
            return true;
        }
        let mut effect = slot_now.clone();
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
            epoch: self.recall_epoch,
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
            request: mut r,
            dsp,
            mut response,
        } = prepared;
        if r.epoch != self.recall_epoch {
            return Prepared {
                request: r,
                dsp,
                response,
            };
        }
        let effect = match r.response {
            None => Some(r.effect.take()),
            Some(_) => self.retarget(r.at, r.slot, &mut response).map(Some),
        };
        let swapped = match (effect, self.rack_slot(r.at, r.slot)) {
            (Some(effect), Some((rack, n))) => rack.install(n, effect, dsp),
            (effect, _) => {
                r.effect = effect.flatten();
                Err(dsp)
            }
        };
        if let (true, Some(ty)) = (swapped.is_ok(), r.ty) {
            self.mark_loaded(r.at, r.slot, ty);
        }
        let exit = Exit::from(swapped.is_ok());
        let (effect, dsp) = swapped.unwrap_or_else(|dsp| (r.effect, dsp));
        self.queue_async(r.script, r.id, exit);
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
        let current = rack.slots.get(n)?.as_ref()?;
        let ir::EffectKind::Convolution { .. } = current.kind else {
            return None;
        };
        let (bypass, output, dry) = (current.bypass, current.output, current.dry);
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
            bypass,
            output,
            dry,
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

    /// `get_engine_par($ENGINE_PAR_EFFECT_TYPE)`: the type last loaded into
    /// the slot, else the one it was saved with, else for an empty slot
    /// the type that loads nothing. `None` for an effect of no known type.
    pub(crate) fn effect_type(&mut self, group: i32, slot: i32, generic: i32) -> Option<i32> {
        let at = crate::knobs::rack_at(group, generic)?;
        let types = &self.plan.slot_shapes.types;
        if let Some(ty) = self.loaded_type(at, slot) {
            return types.get(usize::from(ty)).map(|t| t.kind);
        }
        let saved = (self.plan.slot_shapes.saved_types.iter())
            .find(|s| s.0 == at && i32::from(s.1) == slot)
            .map(|s| s.2);
        let none = (types.iter())
            .find(|t| !t.subtype && t.effect.is_none())
            .map(|t| t.kind);
        match self.slot_mut(group, slot, generic)? {
            Some(_) => saved,
            None => none,
        }
    }

    fn mark_loaded(&mut self, at: ir::RackAt, slot: u8, ty: u16) {
        // A load replaces the unit a shape switch changed.
        if let Some(n) = crate::knobs::shape_slot(&self.plan, at, i32::from(slot))
            && let Some(on) = self.behaviour.shapes.get_mut(n)
        {
            *on = None;
        }
        // The new unit's controls start at its own values, not the old one's.
        (self.behaviour.held).retain(|&(a, s, ..)| (a, s) != (at, slot));
        let shapes = &self.plan.slot_shapes;
        if let Some(t) = shapes.types.get(usize::from(ty)).filter(|t| !t.subtype) {
            let held = &mut self.behaviour.held;
            for &(_, name, value) in shapes.fresh.iter().filter(|f| f.0 == t.id) {
                if held.len() < held.capacity() {
                    held.push((at, slot, name, value));
                }
            }
        }
        let loaded = &mut self.behaviour.loaded;
        loaded.retain(|&(a, s, _)| (a, s) != (at, slot));
        if loaded.len() < loaded.capacity() {
            loaded.push((at, slot, ty));
        }
        // Its knobs start where the new unit's are.
        let knobs = &mut self.behaviour.knobs;
        knobs.retain(|&(a, s, _)| (a, s) != (at, slot));
        if let Some(model) = self.plan.knob_model(at, slot, Some(ty))
            && knobs.len() < knobs.capacity()
        {
            knobs.push((at, slot, kontra_behaviour::positions(model)));
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
    use std::sync::Arc;

    use kontra_ir as ir;
    use kontra_stream::{Store, StoreConfig};

    use super::Prepared;
    use crate::{Engine, Plan};

    #[test]
    fn a_prepared_effect_crosses_threads() {
        fn send<T: Send>() {}
        send::<Prepared>();
    }

    #[test]
    fn init_load_type_clears_pending_wanted_impulse_on_kind_change() {
        let store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut plan = Plan::new(Vec::new(), 8);
        plan.inserts = vec![Some(ir::Effect {
            kind: ir::EffectKind::Convolution { impulse: 0 },
            bypass: false,
            output: 1.0,
            dry: 0.0,
        })];
        plan.slot_shapes.types.push(ir::SlotType {
            id: 42,
            subtype: false,
            kind: 42,
            reads: 0,
            effect: Some(ir::Effect {
                kind: ir::EffectKind::Gain(1.0),
                bypass: false,
                output: 1.0,
                dry: 0.0,
            }),
            laws: Arc::from([]),
            model: None,
        });
        let mut engine = Engine::new(Arc::new(plan), store.client(), 48_000);
        engine.building = true;
        engine.wanted.push((0, 1, 0, ir::Impulse::PLAIN));
        let res = engine.load_type(42, false, -1, 0, 1);
        assert_eq!(res, Some(-1));
        assert!(
            engine.wanted.is_empty(),
            "changing slot kind in init must clear pending impulse entry"
        );
    }

    #[test]
    fn recall_discards_queued_and_stale_in_flight_effect_requests() {
        let store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut plan = Plan::new(Vec::new(), 8);
        plan.inserts = vec![Some(ir::Effect {
            kind: ir::EffectKind::Gain(1.0),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        })];
        plan.slot_shapes.types.push(ir::SlotType {
            id: 7,
            subtype: false,
            kind: 7,
            reads: 0,
            effect: None,
            laws: Arc::from([]),
            model: None,
        });
        let plan = Arc::new(plan);
        let mut engine = Engine::new(Arc::clone(&plan), store.client(), 48_000);
        // Queue two requests after init: drain the first (in-flight), leave the second queued.
        assert!(engine.load_type(7, false, -1, 0, 1).is_some());
        let mut drained = Vec::new();
        engine.drain_requests(|r| drained.push(r));
        assert_eq!(drained.len(), 1);
        assert!(engine.load_type(7, false, -1, 0, 1).is_some());

        let prepared = Prepared::new(drained.pop().unwrap(), Some(&plan), 48_000);
        let _ = engine.recall(Box::new([]));

        // Queued pre-recall request was discarded by recall.
        let mut after_recall = Vec::new();
        engine.drain_requests(|r| after_recall.push(r));
        assert!(after_recall.is_empty());

        // Installing the stale in-flight Prepared must not overwrite the recalled slot.
        let _returned = engine.install(prepared);
        assert!(
            engine.rack.slots[0].is_some(),
            "stale pre-recall install must not clobber slot after recall"
        );
    }

    #[test]
    fn kt02_effect_type_swap_compares_type_id_not_ir_discriminant() {
        let store = Store::new(StoreConfig::DEFAULT).unwrap();
        let make_reverb = |predelay_ms| ir::Effect {
            kind: ir::EffectKind::Reverb(ir::Reverb {
                seconds: 2.0,
                size: 1.0,
                network: None,
                damping: 0.5,
                diffusion: 0.8,
                predelay_ms,
                high_cut_hz: 10_000.0,
                low_shelf_db: 0.0,
                stereo: 1.0,
                modulation_ms: 0.5,
                modulation_hz: 0.5,
            }),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        };
        let reverb_a = make_reverb(10.0);
        let reverb_b = make_reverb(80.0);
        let eq_a = ir::Effect {
            kind: ir::EffectKind::Eq([ir::Bell::FLAT; 4]),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        };
        let mut eq_b_bands = [ir::Bell::FLAT; 4];
        eq_b_bands[0].gain_db = 3.0;
        let eq_b = ir::Effect {
            kind: ir::EffectKind::Eq(eq_b_bands),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        };
        let slot_ty = |id, effect| ir::SlotType {
            id,
            subtype: false,
            kind: id,
            reads: 0,
            effect: Some(effect),
            laws: Arc::from([]),
            model: None,
        };

        let mut plan = Plan::new(Vec::new(), 8);
        plan.inserts = vec![Some(reverb_a.clone())];
        plan.slot_shapes.saved_types = vec![(ir::RackAt::Inserts, 0, 39)];
        plan.slot_shapes.types = vec![
            slot_ty(39, reverb_a),
            slot_ty(51, reverb_b.clone()),
            slot_ty(24, eq_a),
            slot_ty(28, eq_b.clone()),
        ];
        let plan = Arc::new(plan);
        let mut engine = Engine::new(Arc::clone(&plan), store.client(), 48_000);

        // Reloading the same effect type ID (39 -> 39) keeps the slot as-is.
        assert!(engine.load_type(39, false, -1, 0, 1).is_some());
        let mut drained = Vec::new();
        engine.drain_requests(|r| drained.push(r));
        assert!(
            drained.is_empty(),
            "same effect type ID must not queue a reload"
        );

        // Swapping 39 -> 51: both share EffectKind::Reverb,
        // but differ in effect type ID, so this MUST queue a swap.
        assert!(engine.load_type(51, false, -1, 0, 1).is_some());
        engine.drain_requests(|r| drained.push(r));
        assert_eq!(
            drained.len(),
            1,
            "KT-02: swapping 39 -> 51 must not no-op on EffectKind discriminant"
        );
        let req = drained.pop().expect("queued type 51 swap");
        assert_eq!(req.ty, Some(1));
        assert_eq!(req.effect, Some(reverb_b));
        let _ = engine.install(Prepared::new(req, Some(&plan), 48_000));
        assert_eq!(engine.effect_type(-1, 0, 1), Some(51));

        // Reloading type 51 after install must now no-op.
        assert!(engine.load_type(51, false, -1, 0, 1).is_some());
        engine.drain_requests(|r| drained.push(r));
        assert!(drained.is_empty(), "same loaded type 51 must no-op");

        // In `on init`, swapping between two colliding Eq types (24 -> 28)
        // must replace the slot immediately.
        engine.building = true;
        assert_eq!(engine.load_type(24, false, -1, 0, 1), Some(-1));
        assert_eq!(engine.effect_type(-1, 0, 1), Some(24));
        assert_eq!(engine.load_type(28, false, -1, 0, 1), Some(-1));
        assert_eq!(engine.effect_type(-1, 0, 1), Some(28));
        assert_eq!(engine.rack.slots[0], Some(eq_b));
    }
}
