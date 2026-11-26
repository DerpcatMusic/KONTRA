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

    /// Builds a fresh [`ir::SlotType`] from `model`, deriving the slot dry
    /// level from [`Self::mix`] (`1.0` for `WetDry`, `0.0` otherwise).
    #[must_use]
    pub fn slot_type_model(&'static self, model: ir::KnobModel) -> ir::SlotType {
        let dry = match self.mix {
            MixMode::WetDry => 1.0,
            MixMode::InPlace | MixMode::OwnMix => 0.0,
        };
        let effect = (self.build)(&model.saved).map(|kind| ir::Effect {
            kind,
            bypass: false,
            output: 1.0,
            dry,
        });
        ir::SlotType {
            id: self.script_type,
            subtype: false,
            reads: 0,
            kind: self.script_type,
            effect,
            laws: [].into(),
            model: Some(model),
        }
    }

    /// Builds a fresh [`ir::SlotType`] from `saved` knob positions.
    #[must_use]
    pub fn slot_type(&'static self, saved: &[f32]) -> ir::SlotType {
        self.slot_type_model(self.knob_model(saved))
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
    pub const RINGMOD: ModelId = ModelId(B + 19);
    pub const STEREO_TUNE: ModelId = ModelId(B + 20);
    pub const CRY_WAH: ModelId = ModelId(B + 21);
    pub const TWIN_DELAY: ModelId = ModelId(B + 22);
    pub const PLATE_REVERB: ModelId = ModelId(B + 23);
    pub const ROTATOR: ModelId = ModelId(B + 24);
    pub const GATER: ModelId = ModelId(B + 25);
    pub const BEAT_MASHER: ModelId = ModelId(B + 26);
    pub const BEAT_SLICER: ModelId = ModelId(B + 27);
    pub const REVERSE_GRAIN: ModelId = ModelId(B + 28);
    pub const TRANSPOSE_STRETCH: ModelId = ModelId(B + 29);
    pub const TWANG: ModelId = ModelId(B + 30);
    pub const CABINET: ModelId = ModelId(B + 31);
    pub const JUMP: ModelId = ModelId(B + 32);
    pub const VAN51: ModelId = ModelId(B + 33);
    pub const AC_BOX: ModelId = ModelId(B + 34);
    pub const HOT_SOLO: ModelId = ModelId(B + 35);
    pub const CAT: ModelId = ModelId(B + 36);
    pub const BASS_PRO: ModelId = ModelId(B + 37);
    pub const SKREAMER_DELUXE: ModelId = ModelId(B + 38);
    pub const KOLOR: ModelId = ModelId(B + 39);
    pub const DYX: ModelId = ModelId(B + 40);
    pub const CORE_CELL: ModelId = ModelId(B + 41);
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
    &crate::rotator::MODULE,
    &crate::mangling::beat_masher::MODULE,
    &crate::mangling::gater::MODULE,
    &crate::mangling::beat_slicer::MODULE,
    &crate::mangling::reverse_grain::MODULE,
    &crate::mangling::transpose_stretch::MODULE,
    &crate::ringmod::MODULE,
    &crate::stereo_tune::MODULE,
    &crate::cry_wah::MODULE,
    &crate::twin_delay::MODULE,
    &crate::plate_reverb::MODULE,
    &crate::amps::twang::MODULE,
    &crate::cabinet::MODULE,
    &crate::amps::jump::MODULE,
    &crate::amps::van51::MODULE,
    &crate::amps::ac_box::MODULE,
    &crate::amps::hot_solo::MODULE,
    &crate::cat::MODULE,
    &crate::amps::bass_pro::MODULE,
    &crate::skreamer_deluxe::MODULE,
    &crate::kolor::MODULE,
    &crate::dyx::MODULE,
    &crate::core_cell::MODULE,
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

/// `$NI_SYNC_UNIT_*` note values in quarter-note beats (0 = free, < 0 = unused).
const SYNC_UNIT_BEATS: [f32; 22] = [
    0.0,
    -1.0,
    -1.0,
    -1.0,
    -1.0,
    4.0,
    8.0 / 3.0,
    2.0,
    4.0 / 3.0,
    1.0,
    2.0 / 3.0,
    -1.0,
    0.5,
    1.0 / 3.0,
    -1.0,
    0.25,
    1.0 / 6.0,
    0.125,
    1.0 / 12.0,
    0.0625,
    1.0 / 24.0,
    1.0 / 64.0,
];

/// Quarter-note beats for a `$NI_SYNC_UNIT_*` index (`None` when free or invalid).
#[must_use]
pub fn sync_beats(unit: i32) -> Option<f32> {
    let b = *SYNC_UNIT_BEATS.get(usize::try_from(unit).ok()?)?;
    (b > 0.0).then_some(b)
}

/// Nearest `$NI_SYNC_UNIT_*` index for a saved quarter-note beat count (0 = free).
#[must_use]
pub fn sync_unit(beats: f32) -> i32 {
    if beats <= 1e-9 {
        return 0;
    }
    SYNC_UNIT_BEATS
        .iter()
        .copied()
        .enumerate()
        .filter(|&(_, b)| b > 0.0)
        .min_by(|&(_, a), &(_, b)| (a - beats).abs().total_cmp(&(b - beats).abs()))
        .map_or(0, |(i, _)| i as i32)
}

/// Reads a finite little-endian `f32` at byte offset `at`.
#[must_use]
pub fn f32_le(bytes: &[u8], at: usize) -> Option<f32> {
    let raw: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
    Some(f32::from_le_bytes(raw)).filter(|v| v.is_finite())
}

/// Reads a little-endian `i32` at byte offset `at`.
#[must_use]
pub fn i32_le(bytes: &[u8], at: usize) -> Option<i32> {
    let raw: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
    Some(i32::from_le_bytes(raw))
}

/// Normalized `0..=1` KSP knob law.
pub const UNIT_LAW: ir::Law = ir::Law::Linear { lo: 0.0, hi: 1.0 };

/// Raw integer-valued KSP switch/menu knob law (`0..=1e6`).
pub const RAW_LAW: ir::Law = ir::Law::Linear { lo: 0.0, hi: 1e6 };

/// Declarative KSP knob binding row targeting `Knob(slot)`.
#[must_use]
pub const fn knob_row(
    name: &'static str,
    slot: u8,
    law: ir::Law,
) -> (&'static str, ir::KnobTarget, ir::Law) {
    (name, ir::KnobTarget::Slot(ir::SlotKnob::Knob(slot)), law)
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
