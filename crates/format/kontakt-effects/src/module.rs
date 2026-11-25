//! Declarative single-encoding effect module descriptor.
//!
//! Each Kontakt modelled effect defines one static [`Module`] containing its
//! [`ir::ModelId`], binary module id, supported version range, name, script
//! type id, mix mode, knob layout, and single pure `build(&[f32]) ->
//! Option<ir::EffectKind>` function. Both load-time decoding and audio-thread
//! script knob updates evaluate the exact same [`Module::build`] via
//! [`ir::EffectModel`].

use kontra_ir as ir;

/// How a module mixes wet and dry audio at the slot boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixMode {
    /// Processes the slot signal in place (`output = 1`, `dry = 0`).
    InPlace,
    /// Runs wet over dry (`output` and `dry` controlled by the slot).
    WetDry,
    /// Blends dry/wet inside the processor itself (`dry = 0`).
    OwnMix,
}

/// Declarative descriptor for one modelled Kontakt effect.
pub struct Module {
    /// Stable serializable [`ir::ModelId`] in the Kontakt band (`0..64`).
    pub id: ir::ModelId,
    /// Binary Kontakt module identifier (e.g. `0x42` for Tape Saturator).
    pub module_id: u32,
    /// Inclusive supported binary record version range.
    pub versions: (u32, u32),
    /// Human-readable module name.
    pub name: &'static str,
    /// KSP `$EFFECT_TYPE_*` identifier.
    pub script_type: i32,
    /// Slot wet/dry mixing convention.
    pub mix: MixMode,
    /// Knobs in the order [`Self::build`] reads their positions.
    pub knobs: &'static [ir::SlotKnob],
    /// Single pure builder shared by load-time decoding and live script moves.
    pub build: fn(&[f32]) -> Option<ir::EffectKind>,
}

impl Module {
    /// Registers this module in `kontra-ir`'s wait-free model table (idempotent).
    #[inline]
    pub fn register(&'static self) {
        ir::register_model(self.id, self);
    }

    /// Evaluates [`Self::build`] on `params`.
    #[inline]
    #[must_use]
    pub fn build_kind(&self, params: &[f32]) -> Option<ir::EffectKind> {
        (self.build)(params)
    }

    /// Builds a serializable [`ir::KnobModel`] bound to [`Self::id`] with
    /// `saved` positions, registering the module if needed.
    #[must_use]
    pub fn knob_model(&'static self, saved: &[f32]) -> ir::KnobModel {
        self.register();
        ir::KnobModel {
            id: self.id,
            knobs: self.knobs.into(),
            saved: saved.into(),
        }
    }
}

impl ir::EffectModel for Module {
    #[inline]
    fn build(&self, params: &[f32]) -> Option<ir::EffectKind> {
        (self.build)(params)
    }

    /// As the default, except Supercharger GT's slot dry level follows its
    /// live MIX with the law it loads by ([`crate::supergt::dry`]).
    fn build_effect(&self, params: &[f32], current: &ir::Effect) -> Option<ir::Effect> {
        let kind = (self.build)(params)?;
        let dry = if self.id == ids::SUPERGT {
            crate::supergt::dry(*params.get(9)?, current.output)
        } else {
            current.dry
        };
        Some(ir::Effect {
            kind,
            bypass: current.bypass,
            output: current.output,
            dry,
        })
    }
}

/// Model IDs for all Kontakt effect modules (`ModelId::KONTAKT_BASE + 0..64`).
pub mod ids {
    use kontra_ir::ModelId;

    const B: u16 = ModelId::KONTAKT_BASE;

    pub const TAPE: ModelId = ModelId(B);
    pub const SKREAMER: ModelId = ModelId(B + 1);
    pub const DSTORTION: ModelId = ModelId(B + 2);
    pub const SUPERGT: ModelId = ModelId(B + 3);
    pub const FEEDBACK_COMP: ModelId = ModelId(B + 4);
    pub const PHASIS: ModelId = ModelId(B + 5);
    pub const FLAIR: ModelId = ModelId(B + 6);
    pub const CHORAL: ModelId = ModelId(B + 7);
    pub const BITE: ModelId = ModelId(B + 8);
    pub const FREAK: ModelId = ModelId(B + 9);
    pub const PSYCHE: ModelId = ModelId(B + 10);
    pub const VIBRATO: ModelId = ModelId(B + 11);
    pub const SOLID_EQ: ModelId = ModelId(B + 12);
    pub const SOLID_BUS_COMP: ModelId = ModelId(B + 13);
    pub const RAUM: ModelId = ModelId(B + 14);
    pub const WOWFLUTTER: ModelId = ModelId(B + 15);
    pub const LEGACY_CHORUS: ModelId = ModelId(B + 16);
    pub const LEGACY_FLANGER: ModelId = ModelId(B + 17);
    pub const REPLIKA: ModelId = ModelId(B + 18);
}

/// All modules defined in `kontra-kontakt-effects`.
pub static ALL: &[&Module] = &[
    &crate::tape::MODULE,
    &crate::skreamer::MODULE,
    &crate::dstortion::MODULE,
    &crate::supergt::MODULE,
    &crate::dynamics::FEEDBACK_MODULE,
    &crate::phasis::MODULE,
    &crate::flair::MODULE,
    &crate::choral::MODULE,
    &crate::bite::MODULE,
    &crate::freak::MODULE,
    &crate::psyche::MODULE,
    &crate::vibrato::MODULE,
    &crate::delays::CHORUS,
    &crate::delays::FLANGER,
];

/// Standard `Knob(0..N)` slice helper for static [`Module`] tables.
pub const KNOBS: [ir::SlotKnob; ir::KnobModel::MAX] = [
    ir::SlotKnob::Knob(0),
    ir::SlotKnob::Knob(1),
    ir::SlotKnob::Knob(2),
    ir::SlotKnob::Knob(3),
    ir::SlotKnob::Knob(4),
    ir::SlotKnob::Knob(5),
    ir::SlotKnob::Knob(6),
    ir::SlotKnob::Knob(7),
    ir::SlotKnob::Knob(8),
    ir::SlotKnob::Knob(9),
    ir::SlotKnob::Knob(10),
    ir::SlotKnob::Knob(11),
    ir::SlotKnob::Knob(12),
    ir::SlotKnob::Knob(13),
    ir::SlotKnob::Knob(14),
    ir::SlotKnob::Knob(15),
    ir::SlotKnob::Knob(16),
    ir::SlotKnob::Knob(17),
    ir::SlotKnob::Knob(18),
    ir::SlotKnob::Knob(19),
    ir::SlotKnob::Knob(20),
    ir::SlotKnob::Knob(21),
    ir::SlotKnob::Knob(22),
    ir::SlotKnob::Knob(23),
];

/// Registers every module in `kontra-kontakt-effects`.
pub fn register_all() {
    for module in ALL {
        module.register();
    }
}

#[cfg(test)]
mod tests {
    use kontra_ir::EffectModel;

    /// A script moving Supercharger GT's MIX from 1 to 0 brings the dry
    /// input back, as the load-time law would have.
    #[test]
    fn supergt_live_mix_moves_the_slot_dry_level() {
        let current = kontra_ir::Effect {
            kind: kontra_ir::EffectKind::Other,
            bypass: false,
            output: 0.8,
            dry: 0.0,
        };
        let mut p = [0.5f32; 10];
        p[3] = 0.0;
        p[4] = 0.0;
        p[8] = 0.0;
        p[9] = 0.0;
        let e = crate::supergt::MODULE.build_effect(&p, &current).unwrap();
        assert!((e.dry - 0.8).abs() < 1e-6, "dry {}", e.dry);
        p[9] = 1.0;
        let e = crate::supergt::MODULE.build_effect(&p, &current).unwrap();
        assert!(e.dry.abs() < 1e-6);
        // Other modules keep the slot's dry level.
        let tape = crate::tape::MODULE.build_effect(&[0.5; 4], &current);
        assert!(tape.is_none_or(|e| e.dry == 0.0));
    }
}
