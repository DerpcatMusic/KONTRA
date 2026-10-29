//! A recalled project's effect slots, put back in the plan before the
//! scripts run.

use kontra_ir as ir;

use crate::Plan;

impl Plan {
    /// Loads slot `slot` of `at` with type `ty` before the scripts run, as a
    /// recalled project left it. False when there is no such slot or type.
    pub fn load_type(&mut self, at: ir::RackAt, slot: u8, ty: u16) -> bool {
        let Some(effect) = self
            .slot_shapes
            .types
            .get(usize::from(ty))
            .map(|t| t.effect)
        else {
            return false;
        };
        let Some(s) = self.slot_mut(at, slot) else {
            return false;
        };
        *s = effect;
        self.loaded.retain(|&(a, s, _)| (a, s) != (at, slot));
        self.loaded.push((at, slot, ty));
        true
    }

    /// Loads the library response `name` into the convolution at slot
    /// `slot` of `at` before the scripts run, prepared as the response it
    /// replaces, as a recalled project left it. False when it cannot.
    /// Decodes at `rate`: not real-time.
    pub fn load_response(&mut self, at: ir::RackAt, slot: u8, name: &str, rate: u32) -> bool {
        let Some(Some(ir::Effect {
            kind: ir::EffectKind::Convolution { impulse },
            ..
        })) = self.slot_mut(at, slot).map(|s| *s)
        else {
            return false;
        };
        let Some(&(_, law)) = self.impulse_laws.get(usize::from(impulse)) else {
            return false;
        };
        let found = (self.responses.iter()).find(|(n, _)| n.eq_ignore_ascii_case(name));
        let Some(r) =
            found.and_then(|(_, sample)| kontra_effects::response(sample, &law, rate).ok())
        else {
            return false;
        };
        let Ok(impulse) = u16::try_from(self.impulses.len()) else {
            return false;
        };
        self.impulses.push(Some(r));
        self.impulse_laws.push((name.to_lowercase(), law));
        if let Some(Some(e)) = self.slot_mut(at, slot) {
            e.kind = ir::EffectKind::Convolution { impulse };
        }
        true
    }

    fn slot_mut(&mut self, at: ir::RackAt, slot: u8) -> Option<&mut Option<ir::Effect>> {
        let rack = match at {
            ir::RackAt::Group(g) => self.groups.get_mut(g as usize).map(|g| &mut g.inserts),
            ir::RackAt::Inserts => Some(&mut self.inserts),
            ir::RackAt::Main => Some(&mut self.main),
            ir::RackAt::Sends => Some(&mut self.sends),
            ir::RackAt::Bus(b) => self
                .buses
                .get_mut(usize::from(b) + 1)
                .map(|b| &mut b.inserts),
        };
        rack?.get_mut(usize::from(slot))
    }
}
