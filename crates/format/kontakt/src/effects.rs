//! Effect racks: where Kontakt keeps them and what each slot holds. The
//! engine carries the linear stereo modules; every other active slot is reported.
//!
//! A program holds up to three 8-slot racks (0x3a: insert, send, main, in
//! that order) and up to 16 buses (0x45, each with a rack); a group keeps an
//! insert rack. A slot's first child is the effect, its chunk id the module.

use ni_file::kontakt::objects::{BParFX, BParamArrayBParFX8, Group, InsertBus, Program};
use ni_file::kontakt::{Chunk, StructuredObject};

use crate::impulse::Impulses;
use kontra_format::Notes;
use kontra_ir as ir;

const RACK: u16 = 0x3a;
const BUS: u16 = 0x45;

/// One line per Kontakt `BParFX*` module: chunk id, display name,
/// `$EFFECT_TYPE_*` readback (−1 when unnamed), whether the slot mixes its
/// dry level in parallel (`OtherWet` when uncarried), and how its saved
/// record maps to an effect. Any unlisted module passes at its output gain
/// and is reported.
struct Fx {
    id: u16,
    name: &'static str,
    script_type: i32,
    parallel: bool,
    map: Map,
}

enum Map {
    /// A hand-read record.
    Read(fn(&mut Slot) -> Option<ir::EffectKind>),
    /// A modelled module (06-FORMATS.md §2): the row reads its saved knob
    /// positions as the module's [`ir::KnobModel`], and the module's own
    /// `build` makes the effect, as a script's live move rebuilds it.
    Model(fn(&mut Slot) -> Option<ir::KnobModel>),
}

const fn fx(id: u16, name: &'static str, script_type: i32, parallel: bool, map: Map) -> Fx {
    Fx {
        id,
        name,
        script_type,
        parallel,
        map,
    }
}

/// A modelled row: its script type is the module's.
const fn model(
    module: &Module,
    name: &'static str,
    parallel: bool,
    read: fn(&mut Slot) -> Option<ir::KnobModel>,
) -> Fx {
    fx(
        module.module_id as u16,
        name,
        module.script_type,
        parallel,
        Map::Model(read),
    )
}

use Map::Read;
use kontra_kontakt_effects::module::{KNOBS, MixMode, Module};

const ID_LOFI: ir::ModelId = ir::ModelId(ir::ModelId::KONTAKT_BASE + 52);
const ID_HILBERT_LIMITER: ir::ModelId = ir::ModelId(ir::ModelId::KONTAKT_BASE + 53);
const ID_LIMITER: ir::ModelId = ir::ModelId(ir::ModelId::KONTAKT_BASE + 54);
const ID_INVERTER: ir::ModelId = ir::ModelId(ir::ModelId::KONTAKT_BASE + 55);
const ID_GAINER: ir::ModelId = ir::ModelId(ir::ModelId::KONTAKT_BASE + 56);
const ID_SATURATION: ir::ModelId = ir::ModelId(ir::ModelId::KONTAKT_BASE + 57);
const ID_CONVOLUTION: ir::ModelId = ir::ModelId(ir::ModelId::KONTAKT_BASE + 58);
const ID_STEREO: ir::ModelId = ir::ModelId(ir::ModelId::KONTAKT_BASE + 59);
const ID_FILTER: ir::ModelId = ir::ModelId(ir::ModelId::KONTAKT_BASE + 60);

struct LofiModel;
impl ir::EffectModel for LofiModel {
    fn build(&self, params: &[f32]) -> Option<ir::EffectKind> {
        let &[noise, color, bits, freq] = params else {
            return None;
        };
        Some(crate::drive::lofi(bits, freq, noise, color))
    }

    fn build_effect(&self, params: &[f32], current: &ir::Effect) -> Option<ir::Effect> {
        let &[noise, color, ..] = params else {
            return None;
        };
        let mut kind = current.kind.clone();
        if let ir::EffectKind::Crush(c) = &mut kind {
            c.noise = if noise >= 0.001 { noise * 0.075 } else { 0.0 };
            c.noise_tone = 1.0 - 0.9 * color.clamp(0.0, 1.0);
        } else {
            kind = self.build(params)?;
        }
        Some(ir::Effect { kind, ..*current })
    }
}
static LOFI_EFFECT_MODEL: LofiModel = LofiModel;

struct SaturationModel;
impl ir::EffectModel for SaturationModel {
    fn build(&self, params: &[f32]) -> Option<ir::EffectKind> {
        let &[algo, shape] = params else {
            return None;
        };
        crate::drive::saturation_mode(algo.round() as i32, shape)
    }

    fn build_effect(&self, params: &[f32], current: &ir::Effect) -> Option<ir::Effect> {
        let &[algo, saved_shape] = params else {
            return None;
        };
        let shape = match &current.kind {
            ir::EffectKind::Shaper(s) => match s.curve {
                ir::DriveCurve::Soft(x) => x,
                ir::DriveCurve::Blend { t, .. } => 2.0 * t - 1.0,
                _ => saved_shape,
            },
            _ => saved_shape,
        };
        let kind = crate::drive::saturation_mode(algo.round() as i32, shape)?;
        Some(ir::Effect { kind, ..*current })
    }
}
static SATURATION_EFFECT_MODEL: SaturationModel = SaturationModel;

/// Builds Stereo Modeller (`0x1f`) as `EffectKind::Stereo` when pseudo is off,
/// or as a zero-feedback Haas delay (`10 * width^3` ms on the right channel)
/// when `$ENGINE_PAR_STEREO_PSEUDO` is enabled.
fn build_stereo(width: f32, pan: f32, pseudo: bool) -> ir::EffectKind {
    let width = width.clamp(0.0, 1.0);
    let pan = pan.clamp(-1.0, 1.0);
    if !pseudo {
        return ir::EffectKind::Stereo { width, pan };
    }
    let delay_ms = 10.0 * width.powi(3);
    let half = delay_ms * 0.5;
    let l_gain = (1.0 - pan.max(0.0)).clamp(0.0, 1.0);
    let r_gain = (1.0 + pan.min(0.0)).clamp(0.0, 1.0);
    ir::EffectKind::Delay(ir::Delay {
        time: ir::DelayTime::Ms(half + 0.05),
        depth_ms: 0.0,
        second_head: false,
        input: [[1.0, 0.0], [0.0, 1.0]],
        feedback: [[0.0, 0.0], [0.0, 0.0]],
        output: [[l_gain, 0.0], [0.0, r_gain]],
        damping_hz: f32::INFINITY,
        low_cut_hz: 0.0,
        steep_low_cut: false,
        body: std::borrow::Cow::Borrowed(&[]),
        triangle_s: None,
        shape: std::borrow::Cow::Borrowed(&[]),
        noise: [0.0; 2],
        high_shelf: None,
        spread_ms: half,
        rate: ir::Rate::Hz(0.0),
        phase: 0.0,
        direct: 0.0,
    })
}

struct StereoModel;
impl ir::EffectModel for StereoModel {
    fn build(&self, params: &[f32]) -> Option<ir::EffectKind> {
        let &[pseudo, width, pan] = params else {
            return None;
        };
        Some(build_stereo(width, pan, pseudo >= 0.5))
    }

    fn build_effect(&self, params: &[f32], current: &ir::Effect) -> Option<ir::Effect> {
        let &[pseudo, saved_width, saved_pan] = params else {
            return None;
        };
        let (width, pan) = match &current.kind {
            ir::EffectKind::Stereo { width, pan } => (*width, *pan),
            _ => (saved_width, saved_pan),
        };
        let kind = build_stereo(width, pan, pseudo >= 0.5);
        Some(ir::Effect { kind, ..*current })
    }
}
static STEREO_EFFECT_MODEL: StereoModel = StereoModel;

pub(crate) static LOFI_MODULE: Module = Module {
    id: ID_LOFI,
    module_id: 0x20,
    versions: (0, u32::MAX),
    name: "Lo-Fi",
    script_type: 8,
    mix: MixMode::InPlace,
    knobs: KNOBS.split_at(2).0,
    build: |p| {
        let &[noise, color, bits, freq] = p else {
            return None;
        };
        Some(crate::drive::lofi(bits, freq, noise, color))
    },
};

pub(crate) static HILBERT_LIMITER_MODULE: Module = Module {
    id: ID_HILBERT_LIMITER,
    module_id: 0x5f,
    versions: (0x51, 0x51),
    name: "Hilbert Limiter",
    script_type: 45,
    mix: MixMode::InPlace,
    knobs: KNOBS.split_at(3).0,
    build: |p| {
        let &[threshold, release, ceiling] = p else {
            return None;
        };
        Some(ir::EffectKind::Limiter(crate::laws::transparent_limiter(
            threshold, release, ceiling,
        )))
    },
};

pub(crate) static LIMITER_MODULE: Module = Module {
    id: ID_LIMITER,
    module_id: 0x1c,
    versions: (0, u32::MAX),
    name: "Limiter",
    script_type: 3,
    mix: MixMode::InPlace,
    knobs: &[],
    build: |p| {
        let &[gain, release] = p else {
            return None;
        };
        Some(ir::EffectKind::Limiter(crate::laws::limiter(gain, release)))
    },
};

pub(crate) static INVERTER_MODULE: Module = Module {
    id: ID_INVERTER,
    module_id: 0x1a,
    versions: (0, u32::MAX),
    name: "Inverter",
    script_type: 4,
    mix: MixMode::InPlace,
    knobs: KNOBS.split_at(2).0,
    build: |p| {
        let &[phase, swap] = p else {
            return None;
        };
        Some(ir::EffectKind::Invert {
            phase: phase >= 0.5,
            swap: swap >= 0.5,
        })
    },
};

pub(crate) static GAINER_MODULE: Module = Module {
    id: ID_GAINER,
    module_id: 0x13,
    versions: (0, u32::MAX),
    name: "Gainer",
    script_type: 20,
    mix: MixMode::InPlace,
    knobs: &[],
    build: |p| p.first().copied().map(ir::EffectKind::Gain),
};

pub(crate) static SATURATION_MODULE: Module = Module {
    id: ID_SATURATION,
    module_id: 0x1d,
    versions: (0, u32::MAX),
    name: "Saturation",
    script_type: 6,
    mix: MixMode::InPlace,
    knobs: KNOBS.split_at(1).0,
    build: |p| {
        let &[algo, shape] = p else {
            return None;
        };
        crate::drive::saturation_mode(algo.round() as i32, shape)
    },
};

pub(crate) static CONVOLUTION_MODULE: Module = Module {
    id: ID_CONVOLUTION,
    module_id: 0x16,
    versions: (0, u32::MAX),
    name: "Convolution",
    script_type: 19,
    mix: MixMode::WetDry,
    knobs: &[],
    build: |_| None,
};

pub(crate) static STEREO_MODULE: Module = Module {
    id: ID_STEREO,
    module_id: 0x1f,
    versions: (0, u32::MAX),
    name: "Stereo Modeller",
    script_type: 9,
    mix: MixMode::InPlace,
    knobs: KNOBS.split_at(1).0,
    build: |p| {
        let &[pseudo, width, pan] = p else {
            return None;
        };
        Some(build_stereo(width, pan, pseudo >= 0.5))
    },
};

pub(crate) static FILTER_MODULE: Module = Module {
    id: ID_FILTER,
    module_id: 0x18,
    versions: (0, u32::MAX),
    name: "Filter",
    script_type: 7,
    mix: MixMode::InPlace,
    knobs: &[],
    build: |_| None,
};

/// KSP `$ENGINE_PAR_*` rows for the newly-wired live controls in this module:
/// Inverter (`PHASE_INVERT`, `LR_SWAP`), Saturation (`SHAPE_TYPE`),
/// Stereo Modeller (`STEREO_PSEUDO`), Lo-Fi (`NOISELEVEL`, `NOISECOLOR`),
/// and Transparent/Hilbert Limiter (`TRANSLIM_THRESHOLD`, `TRANSLIM_RELEASE`,
/// `TRANSLIM_CEILING`).
pub(crate) fn live_rows() -> Vec<(&'static str, ir::KnobTarget, ir::Law)> {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let raw = || ir::Law::Linear { lo: 0.0, hi: 1e6 };
    let cube = ir::Law::Table((0..=64u8).map(|n| (f32::from(n) / 64.0).powi(3)).collect());
    vec![
        ("$ENGINE_PAR_PHASE_INVERT", Slot(Knob(0)), raw()),
        ("$ENGINE_PAR_LR_SWAP", Slot(Knob(1)), raw()),
        ("$ENGINE_PAR_SHAPE_TYPE", Slot(Knob(0)), raw()),
        ("$ENGINE_PAR_STEREO_PSEUDO", Slot(Knob(0)), raw()),
        ("$ENGINE_PAR_NOISELEVEL", Slot(Knob(0)), cube),
        ("$ENGINE_PAR_NOISECOLOR", Slot(Knob(1)), unit()),
        ("$ENGINE_PAR_TRANSLIM_THRESHOLD", Slot(Knob(0)), unit()),
        ("$ENGINE_PAR_TRANSLIM_RELEASE", Slot(Knob(1)), unit()),
        ("$ENGINE_PAR_TRANSLIM_CEILING", Slot(Knob(2)), unit()),
    ]
}

// `parity-board.py` matches `|s|` in `MODULES` rows to identify modelled entries.
#[allow(clippy::redundant_closure)]
#[rustfmt::skip]
const MODULES: &[Fx] = &[
    fx(0x10, "Delay (legacy)", 18, true, Read(legacy_delay)),
    model(&crate::delays::CHORUS, "Chorus (legacy)", true, |s| crate::delays::saved(s.version, s.public).map(|p: [f32; 4]| crate::delays::CHORUS.knob_model(&p))),
    model(&crate::delays::FLANGER, "Flanger (legacy)", true, |s| crate::delays::saved(s.version, s.public).map(|p: [f32; 6]| crate::delays::FLANGER.knob_model(&p))),
    model(&crate::effects::GAINER_MODULE, "Gainer", false, |s| gainer_model(s)),
    fx(0x14, "Phaser (legacy)", 14, true, Read(legacy_delay)),
    fx(0x15, "Reverb (legacy)", 17, true, Read(legacy_reverb)),
    model(&crate::effects::CONVOLUTION_MODULE, "Convolution", true, |s| convolution_model(s)),
    fx(0x17, "Send Levels", 11, false, Read(|s| sends(s.public))),
    model(&crate::effects::FILTER_MODULE, "Filter", false, |s| filter_model(s)),
    fx(0x19, "Compressor", 2, false, Read(compressor)),
    model(&crate::effects::INVERTER_MODULE, "Inverter", false, |s| inverter_model(s)),
    model(&crate::dyx::MODULE, "DYX", false, |s| crate::dyx::saved(s.version, s.public).map(crate::dyx::model)),
    model(&crate::effects::LIMITER_MODULE, "Limiter", false, |s| limiter_model(s)),
    model(&crate::effects::SATURATION_MODULE, "Saturation", false, |s| saturation_model(s)),
    fx(0x1e, "Distortion", 10, false, Read(|s| crate::drive::distortion(s.i32_at(0)?, s.f32_at(4)?, s.f32_at(8)?))),
    model(&crate::effects::STEREO_MODULE, "Stereo Modeller", false, |s| stereo_model(s)),
    model(&crate::effects::LOFI_MODULE, "Lo-Fi", false, |s| lofi_model(s)),
    model(&crate::skreamer::MODULE, "Skreamer", false, |s| crate::skreamer::model(s.public)),
    model(&crate::rotator::MODULE, "Rotator", false, |s| crate::rotator::saved(s.version, s.public).map(crate::rotator::model)),
    model(&crate::twang::MODULE, "Twang", false, |s| crate::twang::saved(s.version, s.public).map(crate::twang::model)),
    model(&crate::cabinet::MODULE, "Cabinet", false, |s| crate::cabinet::saved(s.version, s.public).map(crate::cabinet::model)),
    model(&crate::tape::MODULE, "Tape Saturator", false, |s| crate::tape::model(s.public)),
    fx(0x43, "Transient Master", 27, false, Read(dynamics)),
    model(&crate::solid_knobs::SOLID_EQ_MODULE, "Solid G-EQ", false, |s| solid_eq(s.version, s.public, s.bypass, s.at, s.notes)),
    fx(0x46, "Solid Bus Comp", 29, false, Read(bus_compressor)),
    model(&crate::dynamics::FEEDBACK_MODULE, "Feedback Compressor", false, |s| feedback_compressor(s)),
    model(&crate::jump::MODULE, "Jump", false, |s| crate::jump::saved(s.version, s.public).map(crate::jump::model)),
    model(&crate::van51::MODULE, "Van51", false, |s| crate::van51::saved(s.version, s.public).map(crate::van51::model)),
    model(&crate::ac_box::MODULE, "AC Box", false, |s| crate::ac_box::saved(s.version, s.public).map(crate::ac_box::model)),
    model(&crate::hot_solo::MODULE, "Hot Solo", false, |s| crate::hot_solo::saved(s.version, s.public).map(crate::hot_solo::model)),
    model(&crate::cat::MODULE, "Cat", false, |s| crate::cat::saved(s.version, s.public).map(crate::cat::model)),
    model(&crate::dstortion::MODULE, "DStortion", false, |s| {
        if s.version != 0x10 { return None }
        if crate::dstortion::mono(s.public) { s.lost("DStortion mono") }
        crate::dstortion::model(s.public)
    }),
    model(&crate::plate_reverb::MODULE, "Plate Reverb", true, |s| crate::plate_reverb::saved(s.version, s.public).map(crate::plate_reverb::model)),
    model(&crate::cry_wah::MODULE, "Cry Wah", false, |s| crate::cry_wah::saved(s.version, s.public).map(crate::cry_wah::model)),
    fx(0x59, "Reverb", 39, true, Read(reverb)),
    model(&crate::replika::MODULE, "Replika", true, |s| replika_model(s)),
    model(&crate::phasis::MODULE, "Phasis", false, |s| crate::phasis::saved(s.version, s.public).map(crate::phasis::model_from)),
    model(&crate::flair::MODULE, "Flair", false, |s| crate::flair::saved(s.version, s.public).map(crate::flair::model_from)),
    model(&crate::choral::MODULE, "Choral", false, |s| crate::choral::saved(s.version, s.public).map(crate::choral::model_from)),
    model(&crate::core_cell::MODULE, "Core Cell", false, |s| crate::core_cell::saved(s.version, s.public).map(crate::core_cell::model)),
    model(&crate::effects::HILBERT_LIMITER_MODULE, "Hilbert Limiter", false, |s| transparent_limiter_model(s)),
    model(&crate::supergt::MODULE, "Supercharger", false, |s| if s.version == 0x51 { crate::supergt::model(s.public) } else { None }),
    model(&crate::bass_pro::MODULE, "Bass Pro", false, |s| crate::bass_pro::saved(s.version, s.public).map(crate::bass_pro::model)),
    model(&crate::psyche::MODULE, "Psyche Delay", true, |s| crate::psyche::saved(s.version, s.public).map(crate::psyche::model)),
    model(&crate::ringmod::MODULE, "Ring Modulator", false, |s| crate::ringmod::saved(s.version, s.public).map(crate::ringmod::model)),
    model(&crate::raum::MODULE, "Raum", true, |s| {
        let (p, mix, lost) = crate::raum::saved(s.version, s.public)?;
        s.mix = Some(mix);
        lost.into_iter().for_each(|feature| s.lost(feature));
        Some(crate::raum::model_from(p))
    }),
    model(&crate::bite::MODULE, "Bite", false, |s| crate::bite::saved(s.public).map(crate::bite::model)),
    model(&crate::freak::MODULE, "Freak", false, |s| crate::freak::saved(s.public).map(crate::freak::model)),
    model(&crate::vibrato::MODULE, "Vibrato Chorus", false, |s| crate::vibrato::saved(s.version, s.public).map(crate::vibrato::model)),
    model(&crate::wowflutter::MODULE, "Wow / Flutter", false, |s| {
        let (p, mix) = crate::wowflutter::saved(s.version, s.public)?;
        s.mix = Some(mix.clamp(0.0, 1.0));
        Some(crate::wowflutter::model_from(p))
    }),
    model(&crate::twin_delay::MODULE, "Twin Delay", true, |s| crate::twin_delay::saved(s.version, s.public).map(crate::twin_delay::model)),
    model(&crate::stereo_tune::MODULE, "Stereo Tune", false, |s| crate::stereo_tune::saved(s.version, s.public).map(crate::stereo_tune::model)),
    model(&crate::mangling::beat_masher::MODULE, "Beat Masher", false, |s| crate::mangling::beat_masher::saved(s.version, s.public).map(crate::mangling::beat_masher::model)),
    model(&crate::mangling::gater::MODULE, "Gater", false, |s| crate::mangling::gater::saved(s.version, s.public).map(crate::mangling::gater::model)),
    model(&crate::mangling::beat_slicer::MODULE, "Beat Slicer", false, |s| crate::mangling::beat_slicer::saved(s.version, s.public).map(crate::mangling::beat_slicer::model)),
    model(&crate::mangling::reverse_grain::MODULE, "Reverse Grain", false, |s| crate::mangling::reverse_grain::saved(s.version, s.public).map(crate::mangling::reverse_grain::model)),
    model(&crate::mangling::transpose_stretch::MODULE, "Transpose Stretch", false, |s| crate::mangling::transpose_stretch::saved(s.version, s.public).map(crate::mangling::transpose_stretch::model)),
    model(&crate::skreamer_deluxe::MODULE, "Skreamer Deluxe", false, |s| crate::skreamer_deluxe::saved(s.version, s.public).map(crate::skreamer_deluxe::model)),
    model(&crate::kolor::MODULE, "Kolor", false, |s| crate::kolor::saved(s.version, s.public).map(crate::kolor::model)),
];

fn lookup(id: u16) -> Option<&'static Fx> {
    MODULES.iter().find(|m| m.id == id)
}

fn name(id: u16) -> String {
    lookup(id).map_or_else(|| format!("unknown {id:#04x}"), |m| m.name.into())
}

/// The effect type a script reads for saved module `id`.
fn effect_type(id: u16) -> Option<i32> {
    let ty = lookup(id)?.script_type;
    (ty >= 0).then_some(ty)
}

/// Whether module `id` mixes its slot dry level in parallel.
fn is_parallel(id: u16) -> bool {
    lookup(id).is_some_and(|m| m.parallel)
}

/// A rack's slots as effects. Gainer, Inverter and Stereo Modeller carry
/// over; every other module passes audio at its output gain and is reported
/// (bypassed ones as cosmetic: a script may switch them on).
/// A rack's effects by slot. Slot laws go to `laws` when the rack is one a
/// script addresses by `rack_at`.
fn rack(
    array: &BParamArrayBParFX8,
    at: &str,
    impulses: &mut Impulses,
    notes: &mut Notes,
    mut laws: Option<(ir::RackAt, &mut Slots)>,
) -> Vec<Option<ir::Effect>> {
    let mut out: Vec<Option<ir::Effect>> = array
        .items
        .iter()
        .enumerate()
        .map(|(slot, chunk)| {
            let chunk = chunk.as_ref()?;
            let (mut knobs, mut model, mut held) = (Vec::new(), None, Vec::new());
            let effect = slot_effect(
                chunk,
                impulses,
                &format!("{at} slot {slot}"),
                notes,
                (&mut knobs, &mut model, &mut held),
            );
            if let (Some((rack_at, laws)), Ok(slot)) = (laws.as_mut(), u8::try_from(slot)) {
                (laws.held).extend(held.into_iter().map(|(k, v)| (*rack_at, slot, k, v)));
                laws.models.extend(model.map(|m| (*rack_at, slot, m)));
                laws.laws
                    .extend(knobs.into_iter().map(|(knob, field, law)| ir::SlotLaw {
                        at: *rack_at,
                        slot,
                        knob,
                        field,
                        law,
                    }));
                if let Some(ty) = effect.as_ref().and_then(|(_, m, _)| effect_type(*m)) {
                    laws.types.push((*rack_at, slot, ty));
                }
                if let Some((_, 0x18, kind)) = &effect {
                    laws.subtypes.push((*rack_at, slot, *kind));
                    let switches = crate::laws::switches(*kind);
                    kontra_law::record(|| kontra_law::Application {
                        id: kontra_law::LawId("kontakt.filter_switches.v1"),
                        location: format!("{rack_at:?}/slot/{slot}/switches"),
                        input: format!("{kind:?}"),
                        output: format!("{switches:?}"),
                    });
                    if switches {
                        laws.shape_slots.push((*rack_at, slot));
                    }
                }
            }
            if effect.is_none() {
                notes.add(
                    ir::Impact::Audible,
                    "effect slot unreadable",
                    format!("{at} slot {slot}"),
                );
            }
            let (effect, module, kind) = effect?;
            if matches!(
                effect.kind,
                ir::EffectKind::Other | ir::EffectKind::OtherWet
            ) {
                // A parallel one whose wet output is 0 plays its dry level
                // alone in Kontakt too (Areia's empty convolutions).
                let silent = effect.kind == ir::EffectKind::OtherWet && effect.output <= 0.0;
                let (impact, state) = if effect.bypass {
                    (ir::Impact::Cosmetic, "bypassed effect")
                } else if silent {
                    (ir::Impact::Cosmetic, "silent effect")
                } else {
                    (ir::Impact::Audible, "effect")
                };
                let kind = if module == 0x18 {
                    format!(" type {kind}")
                } else {
                    String::new()
                };
                notes.add(
                    impact,
                    &format!("{state} {}{kind}", name(module)),
                    format!("{at} slot {slot}"),
                );
            }
            Some(effect)
        })
        .collect();
    while out.last().is_some_and(Option::is_none) {
        out.pop();
    }
    if at.starts_with("group") {
        group_wet(&out, at, notes);
    }
    out
}

/// A group rack runs its wet slots on the group's sum, except one at or
/// before a drive effect (which runs on each voice): that one keeps only its
/// dry level. Group sends feed nothing.
// ponytail: a filter or EQ a route moves also runs per voice (engine
// voice_rack.rs); a wet slot before one is not reported.
fn group_wet(out: &[Option<ir::Effect>], at: &str, notes: &mut Notes) {
    let drive = |e: &ir::Effect| {
        !e.bypass
            && matches!(
                e.kind,
                ir::EffectKind::Shaper(_) | ir::EffectKind::Strip(_) | ir::EffectKind::Crush(_)
            )
    };
    let last_drive = out.iter().rposition(|e| e.as_ref().is_some_and(drive));
    for (slot, e) in out.iter().enumerate() {
        let Some(e) = e.as_ref().filter(|e| !e.bypass) else {
            continue;
        };
        let lost = match &e.kind {
            ir::EffectKind::Sends(_) => Some("group send levels"),
            _ if last_drive.is_none_or(|d| slot > d) => None,
            ir::EffectKind::Convolution { .. } => Some("group convolution before a drive"),
            ir::EffectKind::Reverb(_) => Some("group reverb before a drive"),
            ir::EffectKind::Compressor(_) => Some("group compressor before a drive"),
            ir::EffectKind::Limiter(_) => Some("group limiter before a drive"),
            ir::EffectKind::Delay(_) | ir::EffectKind::Shifter(_) => {
                Some("group delay before a drive")
            }
            ir::EffectKind::Phaser(_) => Some("group phaser before a drive"),
            ir::EffectKind::Cascade(_) => Some("group 3x2 Versatile before a drive"),
            ir::EffectKind::Transient(_) => Some("group transient master before a drive"),
            ir::EffectKind::RingMod(_) => Some("group Freak before a drive"),
            _ => None,
        };
        if let Some(feature) = lost {
            notes.add(ir::Impact::Audible, feature, format!("{at} slot {slot}"));
        }
    }
}

/// A saved slot as its row reads it.
struct Slot<'a, 'i> {
    id: u16,
    version: u16,
    public: &'a [u8],
    /// The record's leading i32: the Filter's type, the Compressor's mode,
    /// the Reverb's room.
    kind_id: Option<i32>,
    bypass: bool,
    at: &'a str,
    notes: &'a mut Notes,
    impulses: &'a mut Impulses<'i>,
    knobs: &'a mut Vec<crate::script_knobs::SlotKnob>,
    model: &'a mut Option<ir::KnobModel>,
    /// Wow/Flutter's or Raum's MIX, which splits the slot's output into
    /// wet and dry; read once by its row.
    mix: Option<f32>,
    /// Pre-built effect for modules whose state (e.g. an impulse response or
    /// polymorphic filter/EQ slot) cannot be reconstructed from floats alone.
    built: Option<ir::EffectKind>,
}

impl Slot<'_, '_> {
    fn f32_at(&self, n: usize) -> Option<f32> {
        let bytes = self.public.get(n..n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    }

    fn i32_at(&self, n: usize) -> Option<i32> {
        Some(i32::from_le_bytes(
            self.public.get(n..n + 4)?.try_into().ok()?,
        ))
    }

    fn byte(&self, n: usize) -> Option<bool> {
        self.public.get(n).map(|&b| b != 0)
    }

    /// Reports a saved feature the effect does not carry: audible unless
    /// the slot is bypassed (a script may switch it on).
    fn lost(&mut self, feature: &str) {
        let impact = if self.bypass {
            ir::Impact::Cosmetic
        } else {
            ir::Impact::Audible
        };
        self.notes.add(impact, feature, self.at.to_owned());
    }
}

/// A slot's effect and module id. The saved records: reader layouts in
/// ni-file's effect_parameters.rs; each module's row in [`MODULES`].
fn slot_effect(
    chunk: &Chunk,
    impulses: &mut Impulses,
    at: &str,
    notes: &mut Notes,
    (knobs, model, held): (
        &mut Vec<crate::script_knobs::SlotKnob>,
        &mut Option<ir::KnobModel>,
        &mut Vec<(u32, i32)>,
    ),
) -> Option<(ir::Effect, u16, i32)> {
    let fx = BParFX::try_from(chunk).ok()?;
    let params = fx.params().ok()?;
    let effect = fx.effect()?;
    let object = StructuredObject::try_from(effect).ok()?;
    let public = object.public_data;
    *held = crate::held::saved(effect.id, object.version, &public);
    let kind_id = public
        .get(..4)
        .and_then(|b| b.try_into().ok())
        .map(i32::from_le_bytes);
    let mut slot = Slot {
        id: effect.id,
        version: object.version,
        public: &public,
        kind_id,
        bypass: params.bypass,
        at,
        notes,
        impulses,
        knobs,
        model,
        mix: None,
        built: None,
    };
    let kind = match lookup(effect.id).map(|m| &m.map) {
        None => None,
        Some(Map::Read(read)) => read(&mut slot),
        Some(Map::Model(read)) => read(&mut slot).and_then(|m| {
            let kind = slot.built.take().or_else(|| m.build(&m.saved));
            *slot.model = Some(m);
            kind
        }),
    };
    let parallel = is_parallel(effect.id);
    // Legacy 1/2/3-band EQ (Filter 0x18 kinds 22..=24) has no Output control:
    // ANALOG STRINGS stores 0 on one that plays in Kontakt (v2 sampler-kontakt
    // effects/mod.rs, corpus of 12,918 EQ slots). Solid G-EQ (0x44) and other
    // filters have an Output gain (`filters_eq.tsv:4, 13, 28, 32, 35`), so keep
    // their `output_gain` (falling back to 1.0 only when a legacy 0x18 filter
    // stores exact 0.0).
    let legacy_eq = effect.id == 0x18
        && (matches!(kind_id, Some(22..=24))
            || (matches!(
                kind,
                Some(ir::EffectKind::Filter(_) | ir::EffectKind::Clocked(_))
            ) && params.output_gain == 0.0));
    let wow_mix = slot.mix.filter(|_| effect.id == 0x6a);
    let raum_mix = slot
        .mix
        .filter(|_| effect.id == 0x65)
        .map(crate::raum::mix_gains);
    let effect_out = ir::Effect {
        kind: kind.unwrap_or(if parallel {
            ir::EffectKind::OtherWet
        } else {
            ir::EffectKind::Other
        }),
        bypass: params.bypass,
        output: if legacy_eq {
            1.0
        } else if let Some((_, wet)) = raum_mix {
            params.output_gain * wet
        } else if let Some(mix) = wow_mix {
            params.output_gain * mix
        } else {
            params.output_gain
        },
        dry: if let Some((dry, _)) = raum_mix {
            params.output_gain * dry
        } else if parallel || effect.id == 0x13 {
            params.dry_level
        } else if effect.id == 0x60 && object.version == 0x51 {
            // Its model's last knob is MIX, read once by its row.
            (model.as_ref().and_then(|m| m.saved.get(9)))
                .map_or(0.0, |&mix| crate::supergt::dry(mix, params.output_gain))
        } else if let Some(mix) = wow_mix {
            params.output_gain * (1.0 - mix)
        } else {
            0.0
        },
    };
    Some((effect_out, effect.id, kind_id.unwrap_or(-1)))
}

fn gainer_model(s: &mut Slot) -> Option<ir::KnobModel> {
    let gain = s.f32_at(0)?;
    Some(GAINER_MODULE.knob_model(&[gain]))
}

fn inverter_model(s: &mut Slot) -> Option<ir::KnobModel> {
    let (phase, swap) = s.byte(0).zip(s.byte(1))?;
    Some(INVERTER_MODULE.knob_model(&[f32::from(u8::from(phase)), f32::from(u8::from(swap))]))
}

fn convolution_model(s: &mut Slot) -> Option<ir::KnobModel> {
    s.built = convolution(s);
    s.built
        .is_some()
        .then(|| CONVOLUTION_MODULE.knob_model(&[]))
}

fn filter_model(s: &mut Slot) -> Option<ir::KnobModel> {
    s.built = s.kind_id.and_then(|k| {
        eq(s.public, k, s.at, s.knobs).or_else(|| filter(s.public, k, s.at, s.knobs))
    });
    s.built.is_some().then(|| FILTER_MODULE.knob_model(&[]))
}

/// Stereo Modeller (`0x1f`): spread, pan, and the pseudo-stereo byte (which
/// runs a right-channel Haas decorrelation delay of `10 * width^3` ms).
fn stereo_model(s: &mut Slot) -> Option<ir::KnobModel> {
    let (spread, pan) = s.f32_at(0).zip(s.f32_at(4))?;
    let pseudo = f32::from(u8::from(s.byte(8).unwrap_or(false)));
    let width = (spread.clamp(-1.0, 1.0) + 1.0) * 0.5;
    ir::register_model(ID_STEREO, &STEREO_EFFECT_MODEL);
    Some(ir::KnobModel {
        id: ID_STEREO,
        knobs: STEREO_MODULE.knobs.into(),
        saved: vec![pseudo, width, pan.clamp(-1.0, 1.0)].into(),
    })
}

/// Mode (i32), threshold, ratio, attack, release, then the link byte.
fn compressor(s: &mut Slot) -> Option<ir::EffectKind> {
    let (((mode, (t, r)), (a, rel)), link) = s
        .kind_id
        .zip(s.f32_at(4).zip(s.f32_at(8)))
        .zip(s.f32_at(12).zip(s.f32_at(16)))
        .zip(s.byte(20))?;
    *s.knobs = crate::script_knobs::compressor_knobs(mode, [t, r, a, rel], link);
    let value = crate::laws::compressor(mode, t, r, a, rel, link);
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId("kontakt.compressor.v1"),
        location: format!("{}/compressor", s.at),
        input: format!("{:?}", (mode, t, r, a, rel, link)),
        output: format!("{value:?}"),
    });
    Some(ir::EffectKind::Compressor(value))
}

fn limiter_model(s: &mut Slot) -> Option<ir::KnobModel> {
    let (gain, release) = s.f32_at(0).zip(s.f32_at(4))?;
    let value = crate::laws::limiter(gain, release);
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId("kontakt.limiter.v1"),
        location: format!("{}/limiter", s.at),
        input: format!("{:?}", (gain, release)),
        output: format!("{value:?}"),
    });
    Some(LIMITER_MODULE.knob_model(&[gain, release]))
}

fn transparent_limiter_model(s: &mut Slot) -> Option<ir::KnobModel> {
    if s.version != 0x51 {
        return None;
    }
    let ((drive, release), ceiling) = s.f32_at(0).zip(s.f32_at(4)).zip(s.f32_at(8))?;
    let value = crate::laws::transparent_limiter(drive, release, ceiling);
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId("kontakt.transparent_limiter.v1"),
        location: format!("{}/limiter", s.at),
        input: format!("{:?}", (drive, release, ceiling)),
        output: format!("{value:?}"),
    });
    Some(HILBERT_LIMITER_MODULE.knob_model(&[drive, release, ceiling]))
}

/// Six floats, the link byte, then (0x11 on) the side-chain byte and (0x12)
/// an extra float at offset 26 (KT-23).
fn bus_compressor(s: &mut Slot) -> Option<ir::EffectKind> {
    if !(0x10..=0x12).contains(&s.version) {
        return None;
    }
    if s.version >= 0x11 && s.byte(25) == Some(true) {
        s.lost("Solid Bus Comp side chain");
    }
    if s.version >= 0x12 && s.f32_at(26).is_some_and(|v| v.abs() > 1e-6) {
        s.lost("Solid Bus Comp extra parameter");
    }
    let (saved, link) = (0..6)
        .map(|i| s.f32_at(4 * i))
        .collect::<Option<Vec<_>>>()
        .and_then(|v| v.try_into().ok())
        .zip(s.byte(24))?;
    *s.model = Some(crate::solid_knobs::bus_model(saved, link));
    let (c, auto) = crate::laws::bus_compressor(saved, link);
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId("kontakt.bus_compressor.v1"),
        location: format!("{}/compressor", s.at),
        input: format!("{:?}", (saved, link)),
        output: format!("{:?}", (&c, auto)),
    });
    if auto {
        s.lost("Solid Bus Comp auto release");
    }
    Some(ir::EffectKind::Compressor(c))
}

fn feedback_compressor(s: &mut Slot) -> Option<ir::KnobModel> {
    if s.version != 0x10 {
        return None;
    }
    if let Some(v) = crate::dynamics::feedback_extra_float(s.public)
        && v.abs() > 1e-6
    {
        s.lost("Feedback Compressor extra parameter");
    }
    crate::dynamics::feedback_model(s.public)
}

fn replika_model(s: &mut Slot) -> Option<ir::KnobModel> {
    let r = crate::replika::saved(s.version, s.public)?;
    crate::replika::lost(r).for_each(|feature| s.lost(feature));
    *s.knobs = crate::replika::extra_knobs(r);
    s.built = crate::replika::delay(r).map(ir::EffectKind::Delay);
    Some(crate::replika::model(r))
}

/// Transient Master.
fn dynamics(s: &mut Slot) -> Option<ir::EffectKind> {
    let (kind, lost) = crate::dynamics::effect(s.id, s.version, s.public)?;
    if let Some(feature) = lost {
        s.lost(feature);
    }
    Some(kind)
}

fn convolution(s: &mut Slot) -> Option<ir::EffectKind> {
    s.impulses
        .convolution(s.version, s.public, s.at, s.notes)
        .map(|impulse| ir::EffectKind::Convolution { impulse })
}

/// The room type (i32), then nine floats.
fn reverb(s: &mut Slot) -> Option<ir::EffectKind> {
    let v = (0..9)
        .map(|i| s.f32_at(4 + 4 * i))
        .collect::<Option<Vec<_>>>()
        .and_then(|v| v.try_into().ok())?;
    Some(ir::EffectKind::Reverb(crate::reverb::reverb(s.kind_id?, v)))
}

/// Delay and Phaser (legacy); the Delay's knobs move.
fn legacy_delay(s: &mut Slot) -> Option<ir::EffectKind> {
    if let Some(saved) = crate::delays::saved(s.version, s.public).filter(|_| s.id == 0x10) {
        *s.knobs = crate::script_knobs::delay_knobs(saved);
    }
    crate::delays::legacy(s.id, s.version, s.public)
}

fn legacy_reverb(s: &mut Slot) -> Option<ir::EffectKind> {
    if let Some(saved) = crate::legacy_reverb::saved(s.version, s.public) {
        *s.knobs = crate::script_knobs::legacy_reverb_knobs(saved);
    }
    crate::legacy_reverb::effect(s.version, s.public)
}

/// Shape −1..=1 and the algorithm (0 Classic, 1 Enhanced, 2 Drums).
fn saturation_model(s: &mut Slot) -> Option<ir::KnobModel> {
    // Version 0x70 of the same id is the Surround Panner.
    if s.version == 0x70 {
        return None;
    }
    let (shape, algorithm) = (s.f32_at(0)?, s.i32_at(4)?);
    if !(0..=2).contains(&algorithm) {
        return None;
    }
    ir::register_model(ID_SATURATION, &SATURATION_EFFECT_MODEL);
    Some(ir::KnobModel {
        id: ID_SATURATION,
        knobs: SATURATION_MODULE.knobs.into(),
        saved: vec![algorithm as f32, shape].into(),
    })
}

/// Bits, frequency, a flag, noise and colour.
fn lofi_model(s: &mut Slot) -> Option<ir::KnobModel> {
    if s.byte(8) == Some(true) {
        s.lost("Lo-Fi flag (law unknown)");
    }
    let (bits, freq, noise, color) = (s.f32_at(0)?, s.f32_at(4)?, s.f32_at(9)?, s.f32_at(13)?);
    ir::register_model(ID_LOFI, &LOFI_EFFECT_MODEL);
    Some(ir::KnobModel {
        id: ID_LOFI,
        knobs: LOFI_MODULE.knobs.into(),
        saved: vec![noise, color, bits, freq].into(),
    })
}

/// The 1/2/3-band EQ (Filter types 22..=24): the record ends with its bands,
/// each Hz, octaves and dB (layouts in v2 sampler-kontakt effects/mod.rs,
/// reader [ref]).
fn eq(
    public: &[u8],
    kind: i32,
    at: &str,
    knobs: &mut Vec<crate::script_knobs::SlotKnob>,
) -> Option<ir::EffectKind> {
    let count = usize::try_from(kind.checked_sub(21)?)
        .ok()
        .filter(|n| (1..=3).contains(n))?;
    let tail = public.get(public.len().checked_sub(count * 12)?..)?;
    let mut bands = [ir::Bell::FLAT; 4];
    let mut saved = Vec::new();
    for (index, (band, raw)) in bands.iter_mut().zip(tail.as_chunks::<12>().0).enumerate() {
        let f = |n: usize| {
            let bytes = raw.get(n..n + 4)?.try_into().ok()?;
            Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
        };
        let values = [f(0)?, f(4)?, f(8)?];
        let [hz, octaves, db] = values;
        let result = crate::laws::eq_band(hz, octaves, db);
        kontra_law::record(|| kontra_law::Application {
            id: kontra_law::LawId("kontakt.eq_band.v1"),
            location: format!("{at}/eq/{index}"),
            input: format!("{values:?}"),
            output: format!("{result:?}"),
        });
        *band = result?;
        saved.push(values);
    }
    *knobs = crate::script_knobs::eq_knobs(&saved);
    Some(ir::EffectKind::Eq(bands))
}

/// The Solid G-EQ (0x44) record (reader [ref]): LF gain, Hz, bell
/// flag; LMF and HMF gain, Hz, Q; HF gain, Hz, bell flag (knobs 0..=1, flags
/// a byte each); v0x11 adds high- and low-pass Hz and their switches.
fn solid_eq(
    version: u16,
    public: &[u8],
    bypass: bool,
    at: &str,
    notes: &mut Notes,
) -> Option<ir::KnobModel> {
    let f = |n: usize| {
        let bytes = public.get(n..n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    let flag = |n: usize| public.get(n).map(|&b| b != 0);
    let bell = |on: bool, shelf| if on { ir::Shape::Peak } else { shelf };
    // ponytail: v0x11's pass filters are reported, not run (no corpus slot uses them yet).
    if version > 0x10 && (flag(50)? || flag(51)?) {
        let impact = if bypass {
            ir::Impact::Cosmetic
        } else {
            ir::Impact::Audible
        };
        notes.add(impact, "Solid G-EQ pass filters", at.to_string());
    }
    let saved = [0, 4, 9, 13, 17, 21, 25, 29, 33, 37].map(f);
    let [lg, lh, mg, mh, mq, hg, hh, hq, fg, fh] = saved;
    let saved = [lg?, lh?, mg?, mh?, mq?, hg?, hh?, hq?, fg?, fh?];
    let bells = [flag(8)?, flag(41)?];
    let shapes = [
        bell(bells[0], ir::Shape::LowShelf),
        bell(bells[1], ir::Shape::HighShelf),
    ];
    let [lg, lh, mg, mh, mq, hg, hh, hq, fg, fh] = saved;
    let [lf, hf] = shapes;
    let bands = crate::laws::solid_eq([lg, lh], lf, [mg, mh, mq], [hg, hh, hq], [fg, fh], hf);
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId("kontakt.solid_eq.v1"),
        location: format!("{at}/solid_eq"),
        input: format!("saved={saved:?}, shapes={shapes:?}"),
        output: format!("{bands:?}"),
    });
    Some(crate::solid_knobs::solid_model(saved, bells))
}

/// A filter: the kind, repeated (newer records put a flag byte before a
/// repeat), a leading gain for Ladder and Daft (the SV Par/Ser bandwidth,
/// unused), then cutoff and resonance, all 0..=1 (v2 sampler-kontakt
/// effects/mod.rs, reader [ref]).
fn filter(
    public: &[u8],
    kind: i32,
    at: &str,
    knobs: &mut Vec<crate::script_knobs::SlotKnob>,
) -> Option<ir::EffectKind> {
    let k = kind.to_le_bytes();
    let mut rest = public;
    loop {
        if let Some(r) = rest.strip_prefix(&k) {
            rest = r;
        } else if let Some(r) = rest.get(1..).and_then(|r| r.strip_prefix(&k)) {
            rest = r;
        } else {
            break;
        }
    }
    let f = |n: usize| {
        let bytes = rest.get(n..n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    // Formant I and II: talk, sharp, size (v2 sampler-kontakt
    // effects/filter.rs, its response checked against Kontakt within 0.1 dB).
    if let 90 | 91 = kind {
        return Some(ir::EffectKind::Formant(ir::Formant {
            talk: f(0)?,
            sharp: f(4)?,
            size: f(8)?,
            set: if kind == 91 {
                ir::VowelSet::Second
            } else {
                ir::VowelSet::First
            },
        }));
    }
    if let Some(c) = crate::clocked::clocked(kind, f(0)?, f(4)?) {
        *knobs = crate::clocked::knobs();
        return Some(ir::EffectKind::Clocked(c));
    }
    if kind == 19 {
        let mut saved = crate::versatile::DEFAULT;
        for (n, v) in saved.iter_mut().enumerate() {
            *v = f(n * 4)?;
        }
        *knobs = crate::versatile::knobs();
        return Some(ir::EffectKind::Cascade(crate::versatile::cascade(saved)));
    }
    let (gain, offset) = if matches!(kind, 30..=41 | 63..=65 | 70 | 71) {
        (f(0)?, 4)
    } else {
        (0.0, 0)
    };
    let (cutoff, resonance) = (f(offset)?, f(offset + 4)?);
    let result = crate::laws::filter(kind, cutoff, resonance, gain);
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId("kontakt.filter.v1"),
        location: format!("{at}/filter"),
        input: format!("kind={kind}, cutoff={cutoff:?}, resonance={resonance:?}, gain={gain:?}"),
        output: format!("{result:?}"),
    });
    let filter = result?;
    *knobs = crate::script_knobs::filter_knobs(kind, cutoff, resonance, gain);
    Some(ir::EffectKind::Filter(filter))
}

/// Send Levels: a counted list of send levels, then one of outputs (v2
/// sampler-kontakt effects/mod.rs reader 0x17).
fn sends(public: &[u8]) -> Option<ir::EffectKind> {
    let count = usize::try_from(u32::from_le_bytes(public.get(..4)?.try_into().ok()?)).ok()?;
    let mut levels = [0.0; 8];
    for (n, level) in levels.iter_mut().enumerate().take(count) {
        let at = 4 + n * 4;
        let value = f32::from_le_bytes(public.get(at..at + 4)?.try_into().ok()?);
        *level = if value.is_finite() { value } else { 0.0 };
    }
    Some(ir::EffectKind::Sends(levels))
}

/// Group `index`'s insert rack: `saved` (a snapshot's) or the group's own.
pub(crate) fn group(
    group: &Group,
    saved: Option<BParamArrayBParFX8>,
    index: usize,
    impulses: &mut Impulses,
    notes: &mut Notes,
    laws: &mut Slots,
) -> Vec<Option<ir::Effect>> {
    let rack_at = u32::try_from(index)
        .ok()
        .map(|g| (ir::RackAt::Group(g), laws));
    match saved.map_or_else(|| group.insert_fx(), Ok) {
        Ok(array) => rack(
            &array,
            &format!("group {index} insert"),
            impulses,
            notes,
            rack_at,
        ),
        Err(e) => {
            notes.add(
                ir::Impact::Audible,
                "effect rack unreadable",
                format!("group {index}: {e}"),
            );
            Vec::new()
        }
    }
}

/// Puts `saved` racks (a snapshot's) in place of the program's own, kind
/// by kind. Kontakt names a rack by its place among its id's chunks
/// ([`program`]: the first `RACK` the insert rack, then send and main; the
/// n-th `BUS` bus n), so the n-th saved one replaces the n-th own one; own
/// ones the snapshot lacks stay, saved ones the program lacks are added
/// after the last, where that place is theirs.
pub(crate) fn replace_racks(children: &mut Vec<Chunk>, saved: Vec<Chunk>) {
    let (racks, buses): (Vec<Chunk>, Vec<Chunk>) = saved.into_iter().partition(|c| c.id == RACK);
    for (id, saved) in [(RACK, racks), (BUS, buses)] {
        let mut saved = saved.into_iter();
        for own in children.iter_mut().filter(|c| c.id == id) {
            match saved.next() {
                Some(chunk) => *own = chunk,
                None => break,
            }
        }
        children.extend(saved);
    }
}

/// The program's racks: instrument insert, send and main, then the
/// instrument buses (Kontakt's 0..=15), each with its rack.
pub(crate) struct Racks {
    pub inserts: Vec<Option<ir::Effect>>,
    pub sends: Vec<Option<ir::Effect>>,
    pub main: Vec<Option<ir::Effect>>,
    pub buses: Vec<ir::Bus>,
    /// The slot laws and shapes of these racks.
    pub laws: Slots,
}

/// What a script may change in the racks' slots: knob laws and filter shapes.
#[derive(Default)]
pub(crate) struct Slots {
    pub laws: Vec<ir::SlotLaw>,
    /// Filter slots that may switch to [`crate::laws::filter_shapes`].
    pub shape_slots: Vec<(ir::RackAt, u8)>,
    /// Filter slots and the subtype each was saved with.
    pub subtypes: Vec<(ir::RackAt, u8, i32)>,
    /// Each saved slot's effect type, as a script reads it.
    pub types: Vec<(ir::RackAt, u8, i32)>,
    /// Slots whose knobs interact, rebuilt together.
    pub models: Vec<(ir::RackAt, u8, ir::KnobModel)>,
    /// Saved values of controls no processor plays (held key, raw).
    pub held: Vec<(ir::RackAt, u8, u32, i32)>,
}

pub(crate) fn program(program: &Program, impulses: &mut Impulses, notes: &mut Notes) -> Racks {
    let names = ["instrument insert", "instrument send", "instrument main"];
    let mut racks = 0;
    let mut instrument = [Vec::new(), Vec::new(), Vec::new()];
    let mut buses: Vec<ir::Bus> = Vec::new();
    let mut bus_invert: Vec<(bool, bool)> = Vec::new();
    let mut laws = Slots::default();
    for child in &program.0.children {
        let (at, array) = match child.id {
            RACK => {
                racks += 1;
                let at = names
                    .get(racks - 1)
                    .map_or_else(|| format!("rack {racks}"), |n| (*n).into());
                (at, BParamArrayBParFX8::try_from(child))
            }
            BUS => {
                let at = format!("bus {}", buses.len());
                let bus = InsertBus::try_from(child);
                let params = bus.as_ref().ok().and_then(|b| b.params().ok());
                // KT-22: v0x12 carries two trailing flag bytes: phase invert
                // (`unknown_tail[0]`) and L/R swap (`unknown_tail[1]`), applied
                // after the bus's insert rack and before volume/pan. A bus routed
                // to a host output (`output != -1`) skips instrument racks in the
                // reference host and is still reported because multi-out stems
                // route through main here.
                let (mut phase_invert, mut swap_lr) = (false, false);
                if let Some(p) = &params {
                    if p.output != -1 {
                        notes.add(
                            ir::Impact::Audible,
                            "instrument bus output routing",
                            format!("{at} to output {}", p.output),
                        );
                    }
                    phase_invert = p.unknown_tail.first().is_some_and(|&b| b != 0);
                    swap_lr = p.unknown_tail.get(1).is_some_and(|&b| b != 0);
                    if let Some(rest) = p.unknown_tail.get(2..)
                        && rest.iter().any(|&b| b != 0)
                    {
                        notes.add(
                            ir::Impact::Audible,
                            "instrument bus extra flags",
                            format!("{at}: {rest:02x?}"),
                        );
                    }
                }
                bus_invert.push((phase_invert, swap_lr));
                buses.push(ir::Bus {
                    name: params
                        .as_ref()
                        .map_or_else(|| at.clone(), |p| p.name.clone()),
                    volume_db: params
                        .as_ref()
                        .map_or(0.0, |p| crate::translate::db(p.volume)),
                    pan: params.as_ref().map_or(0.0, |p| p.pan.clamp(-1.0, 1.0)),
                    to_main: true,
                    ..ir::Bus::new("")
                });
                let array = bus.and_then(|bus| {
                    let rack = bus
                        .0
                        .find_first(RACK)
                        .ok_or(ni_file::Error::Static("bus without a rack"))?;
                    BParamArrayBParFX8::try_from(rack)
                });
                (at, array)
            }
            _ => continue,
        };
        match array {
            Ok(array) => {
                let to = match (child.id, racks) {
                    (BUS, _) => u8::try_from(buses.len().saturating_sub(1))
                        .ok()
                        .map(ir::RackAt::Bus),
                    (_, 1) => Some(ir::RackAt::Inserts),
                    (_, 2) => Some(ir::RackAt::Sends),
                    (_, 3) => Some(ir::RackAt::Main),
                    _ => None,
                };
                let to = to.map(|at| (at, &mut laws));
                let effects = rack(&array, &at, impulses, notes, to);
                match (child.id, buses.last_mut()) {
                    (BUS, Some(bus)) => {
                        bus.inserts = effects;
                        if let Some(&(phase, swap)) = bus_invert.last()
                            && (phase || swap)
                        {
                            // Keep Kontakt's 8 script-addressable insert slots (0..8)
                            // untouched and run the bus's post-rack phase/swap stage after them.
                            if bus.inserts.len() < 8 {
                                bus.inserts.resize(8, None);
                            }
                            bus.inserts.push(Some(ir::Effect {
                                kind: ir::EffectKind::Invert { phase, swap },
                                bypass: false,
                                output: 1.0,
                                dry: 0.0,
                            }));
                        }
                    }
                    _ => {
                        if let Some(rack) = instrument.get_mut(racks - 1) {
                            *rack = effects;
                        }
                    }
                }
            }
            Err(e) => notes.add(
                ir::Impact::Audible,
                "effect rack unreadable",
                format!("{at}: {e}"),
            ),
        }
    }
    let [inserts, sends, main] = instrument;
    Racks {
        inserts,
        sends,
        main,
        buses,
        laws,
    }
}

#[cfg(test)]
mod tests {
    /// Every modelled descriptor names the chunk and KSP type the loader
    /// dispatches it by.
    #[test]
    fn module_descriptors_match_the_loader_table() {
        let all = kontra_kontakt_effects::module::ALL.iter().copied().chain([
            &crate::solid_knobs::SOLID_EQ_MODULE,
            &crate::solid_knobs::SOLID_BUS_MODULE,
        ]);
        for m in all {
            let meta = u16::try_from(m.module_id)
                .ok()
                .and_then(super::lookup)
                .unwrap_or_else(|| panic!("{}: no loader entry {:#04x}", m.name, m.module_id));
            assert_eq!(
                (meta.script_type, m.module_id),
                (m.script_type, u32::from(meta.id)),
                "{} vs loader {}",
                m.name,
                meta.name
            );
        }
    }

    use super::*;

    #[test]
    fn a_snapshots_racks_replace_the_programs_by_kind_and_add_what_it_lacks() {
        let chunk = |id, tag: u8| Chunk {
            id,
            data: vec![tag],
        };
        let tags = |c: &[Chunk], id| -> Vec<u8> {
            c.iter()
                .filter(|c| c.id == id)
                .filter_map(|c| c.data.first().copied())
                .collect()
        };
        // The program has an insert rack and one bus; the snapshot an
        // insert, a send and two buses.
        let mut children = vec![chunk(1, 0), chunk(RACK, 10), chunk(BUS, 20)];
        let saved = vec![
            chunk(RACK, 11),
            chunk(RACK, 12),
            chunk(BUS, 21),
            chunk(BUS, 22),
        ];
        replace_racks(&mut children, saved);
        assert_eq!(tags(&children, RACK), [11, 12]);
        assert_eq!(tags(&children, BUS), [21, 22]);
        // A snapshot with fewer racks keeps the program's others.
        let mut children = vec![chunk(RACK, 10), chunk(RACK, 13), chunk(BUS, 20)];
        replace_racks(&mut children, vec![chunk(RACK, 11)]);
        assert_eq!(tags(&children, RACK), [11, 13]);
        assert_eq!(tags(&children, BUS), [20]);
    }

    fn fx_slot(
        effect_type: u32,
        bypass: bool,
        output_gain: f32,
        dry_level: f32,
        child_id: u16,
        child_version: u16,
        child_public: Vec<u8>,
    ) -> Chunk {
        let mut child_body = vec![0];
        child_body.extend(child_version.to_le_bytes());
        child_body.extend(child_public);
        let child = Chunk {
            id: child_id,
            data: child_body,
        };
        let mut child_bytes = Vec::new();
        child.write(&mut child_bytes).expect("write child chunk");

        let mut private = effect_type.to_le_bytes().to_vec();
        private.push(0); // byte 4: unknown_flag
        private.extend(0u32.to_le_bytes()); // bytes 5..9: reserved
        private.push(u8::from(bypass)); // byte 9: bypass
        private.extend(output_gain.to_le_bytes());
        private.extend(dry_level.to_le_bytes());
        private.extend((-1i32).to_le_bytes());

        let mut data = vec![1]; // structured object
        data.extend(0x50u16.to_le_bytes());
        data.extend((private.len() as u32).to_le_bytes());
        data.extend(private);
        data.extend(0u32.to_le_bytes()); // public_data len
        data.extend((child_bytes.len() as u32).to_le_bytes());
        data.extend(child_bytes);
        Chunk { id: 0x25, data }
    }

    #[test]
    fn legacy_phaser_preserves_dry_and_solid_eq_preserves_output_gain() {
        let mut resolver =
            crate::samples::Resolver::new(std::path::Path::new("."), None).expect("resolver");
        let mut impulses = crate::impulse::Impulses::new(
            std::collections::HashMap::new(),
            &mut resolver,
            std::path::Path::new("."),
            None,
        );
        let mut notes = Notes::default();
        let mut knobs = Vec::new();
        let mut model = None;
        let mut held = Vec::new();

        // Legacy Phaser (0x14) must keep its dry_level (parallel wet/dry mix).
        let phaser_pub: Vec<u8> = [0.5f32, 0.3, 1.0, 0.25, -1.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let fx = fx_slot(18, false, 0.8, 0.6, 0x14, 0x50, phaser_pub);
        let (e, _, _) = slot_effect(
            &fx,
            &mut impulses,
            "slot",
            &mut notes,
            (&mut knobs, &mut model, &mut held),
        )
        .expect("phaser");
        assert!(matches!(e.kind, ir::EffectKind::Phaser(_)));
        assert_eq!((e.output, e.dry), (0.8, 0.6));

        // Solid G-EQ (0x44) must preserve its BParFX output_gain rather than clobbering to 1.0.
        let mut seq_pub = Vec::new();
        seq_pub.extend(0.5f32.to_le_bytes()); // LF gain
        seq_pub.extend(0.5f32.to_le_bytes()); // LF freq
        seq_pub.push(0); // LF bell
        for _ in 0..8 {
            seq_pub.extend(0.5f32.to_le_bytes());
        }
        seq_pub.push(0); // HF bell
        let fx = fx_slot(29, false, 0.42, 0.0, 0x44, 0x10, seq_pub);
        let (e, _, _) = slot_effect(
            &fx,
            &mut impulses,
            "slot",
            &mut notes,
            (&mut knobs, &mut model, &mut held),
        )
        .expect("solid eq");
        assert!(matches!(e.kind, ir::EffectKind::Eq(_)));
        assert_eq!(e.output, 0.42);
    }

    #[test]
    fn saved_phasis_flair_choral_raum_and_wowflutter_decode_with_models() {
        let mut resolver =
            crate::samples::Resolver::new(std::path::Path::new("."), None).expect("resolver");
        let mut impulses = crate::impulse::Impulses::new(
            std::collections::HashMap::new(),
            &mut resolver,
            std::path::Path::new("."),
            None,
        );
        let mut notes = Notes::default();
        let mut knobs = Vec::new();
        let mut held = Vec::new();

        // Phasis (0x5b v0x51, 52 bytes)
        let mut phasis_pub = Vec::new();
        for v in [0.4f32, 0.5, 0.5, 0.2, 0.8, 0.5, 1.0, 0.5, 0.3] {
            phasis_pub.extend(v.to_le_bytes());
        }
        phasis_pub.extend([0u8, 0, 1]);
        for v in [-1.0f32, 0.0, 0.0] {
            phasis_pub.extend(v.to_le_bytes());
        }
        phasis_pub.push(0);
        let mut model = None;
        let (e, _, _) = slot_effect(
            &fx_slot(41, false, 1.0, 0.0, 0x5b, 0x51, phasis_pub),
            &mut impulses,
            "phasis",
            &mut notes,
            (&mut knobs, &mut model, &mut held),
        )
        .expect("phasis");
        assert!(matches!(e.kind, ir::EffectKind::Notches(_)));
        assert!(model.is_some());

        // Flair (0x5c v0x51, 66 bytes)
        let mut flair_pub = Vec::new();
        for v in [1.0f32, 0.5, 0.36, 0.0, 0.5, 0.0, 0.4, 0.0, 0.0, 0.0] {
            flair_pub.extend(v.to_le_bytes());
        }
        flair_pub.push(0);
        flair_pub.extend(0i32.to_le_bytes());
        flair_pub.extend(0i32.to_le_bytes());
        flair_pub.extend(0i32.to_le_bytes());
        for v in [-1.0f32, 0.0, 0.0] {
            flair_pub.extend(v.to_le_bytes());
        }
        flair_pub.push(0);
        let mut model = None;
        let (e, _, _) = slot_effect(
            &fx_slot(42, false, 1.0, 0.0, 0x5c, 0x51, flair_pub),
            &mut impulses,
            "flair",
            &mut notes,
            (&mut knobs, &mut model, &mut held),
        )
        .expect("flair");
        assert!(matches!(e.kind, ir::EffectKind::CombBank(_)));
        assert!(model.is_some());

        // Choral (0x5d v0x51, 34 bytes)
        let mut choral_pub = Vec::new();
        for v in [0.5f32, 0.2, 0.3, 0.4, 0.5, 0.1, 0.9] {
            choral_pub.extend(v.to_le_bytes());
        }
        choral_pub.extend([0u8, 0]);
        choral_pub.extend(1i32.to_le_bytes());
        let mut model = None;
        let (e, _, _) = slot_effect(
            &fx_slot(43, false, 1.0, 0.0, 0x5d, 0x51, choral_pub),
            &mut impulses,
            "choral",
            &mut notes,
            (&mut knobs, &mut model, &mut held),
        )
        .expect("choral");
        assert!(matches!(e.kind, ir::EffectKind::Ensemble(_)));
        assert!(model.is_some());

        // Raum (0x65 v0x10, 67 bytes)
        let mut raum_pub = 1i32.to_le_bytes().to_vec();
        for v in [
            0.1f32, 0.2, 0.0, 0.8, 0.5, 0.8, 0.5, 0.3, 0.5, 0.5, 0.3, 0.0,
        ] {
            raum_pub.extend(v.to_le_bytes());
        }
        raum_pub.extend([0u8, 0]);
        for v in [-1.0f32, 0.0, 0.0] {
            raum_pub.extend(v.to_le_bytes());
        }
        raum_pub.push(0);
        let mut model = None;
        let (e, _, _) = slot_effect(
            &fx_slot(51, false, 0.7, 0.9, 0x65, 0x10, raum_pub),
            &mut impulses,
            "raum",
            &mut notes,
            (&mut knobs, &mut model, &mut held),
        )
        .expect("raum");
        assert!(matches!(e.kind, ir::EffectKind::Reverb(_)));
        // Raum mix (offset 20 = 0.5) follows binary [ref]: (dry=1, wet=1) * output_gain (0.7).
        assert_eq!((e.output, e.dry), (0.7, 0.7));
        assert!(model.is_some());

        // Wow/Flutter (0x6a v0x10, 33 bytes)
        let mut wow_pub = Vec::new();
        for v in [0.5f32, 0.3, 0.0, 0.5, 0.5, 0.3, 0.0] {
            wow_pub.extend(v.to_le_bytes());
        }
        wow_pub.push(0);
        wow_pub.extend(0.25f32.to_le_bytes()); // mix = 0.25
        let mut model = None;
        let (e, _, _) = slot_effect(
            &fx_slot(56, false, 1.0, 0.0, 0x6a, 0x10, wow_pub),
            &mut impulses,
            "wow",
            &mut notes,
            (&mut knobs, &mut model, &mut held),
        )
        .expect("wowflutter");
        assert!(matches!(e.kind, ir::EffectKind::Delay(_)));
        assert_eq!((e.output, e.dry), (0.25, 0.75));
        assert!(model.is_some());

        // Stereo Modeller (0x1f) with pseudo stereo enabled builds a live Haas delay
        // with zero loss diagnostics and a live KnobModel.
        let mut stereo_pub = Vec::new();
        stereo_pub.extend(0.5f32.to_le_bytes()); // spread -> width = 0.75
        stereo_pub.extend(0.0f32.to_le_bytes()); // pan
        stereo_pub.push(1); // pseudo = true
        let mut pseudo_notes = Notes::default();
        let mut model = None;
        let (e, _, _) = slot_effect(
            &fx_slot(19, false, 1.0, 0.0, 0x1f, 0x10, stereo_pub),
            &mut impulses,
            "stereo",
            &mut pseudo_notes,
            (&mut knobs, &mut model, &mut held),
        )
        .expect("stereo");
        assert!(matches!(e.kind, ir::EffectKind::Delay(d) if d.spread_ms > 2.0));
        assert!(model.is_some());
        assert!(pseudo_notes.finish().is_empty());
    }

    #[test]
    fn kt22_insert_bus_phase_invert_and_swap_lr_append_invert_effect() {
        use ni_file::kontakt::StructuredObject;
        use ni_file::kontakt::objects::Program;

        // Build a minimal Program containing one BInsertBus (chunk 0x4a v0x12)
        // with an empty 8-slot rack (chunk 0x3a) inside it.
        let make_program = |output: i32, tail: &[u8]| -> Program {
            // InsertBusParams wire layout (insert_bus.rs): name_len(u32), utf16, volume(f32),
            // pan(f32), output(i32), unknown_tail([phase_invert, swap_lr, ...]).
            let mut bus_pub = Vec::new();
            bus_pub.extend(4u32.to_le_bytes());
            for ch in "Bus1".encode_utf16() {
                bus_pub.extend(ch.to_le_bytes());
            }
            bus_pub.extend(1.0f32.to_le_bytes()); // volume
            bus_pub.extend(0.0f32.to_le_bytes()); // pan
            bus_pub.extend(output.to_le_bytes());
            bus_pub.extend_from_slice(tail);

            let empty_rack = Chunk {
                id: RACK,
                data: {
                    let mut d = vec![0u8];
                    d.extend(0x10u16.to_le_bytes());
                    d.extend([0u8; 8]); // 8 empty slots
                    d
                },
            };
            let mut rack_bytes = Vec::new();
            empty_rack.write(&mut rack_bytes).expect("write rack");
            let mut bus_data = vec![1u8]; // structured
            bus_data.extend(0x12u16.to_le_bytes());
            bus_data.extend(0u32.to_le_bytes()); // private len
            bus_data.extend((bus_pub.len() as u32).to_le_bytes());
            bus_data.extend(bus_pub);
            bus_data.extend((rack_bytes.len() as u32).to_le_bytes());
            bus_data.extend(rack_bytes);
            let bus = Chunk {
                id: BUS,
                data: bus_data,
            };
            Program(StructuredObject {
                version: 0x10,
                private_data: Vec::new(),
                public_data: Vec::new(),
                children: vec![bus],
            })
        };

        let mut resolver =
            crate::samples::Resolver::new(std::path::Path::new("."), None).expect("resolver");
        let mut impulses = crate::impulse::Impulses::new(
            std::collections::HashMap::new(),
            &mut resolver,
            std::path::Path::new("."),
            None,
        );

        // Phase invert + L/R swap enabled, output = -1 (default): no Audible diagnostic,
        // slot 8 carries EffectKind::Invert { phase: true, swap: true }.
        let mut notes = Notes::default();
        let r = program(&make_program(-1, &[1, 1, 0, 0]), &mut impulses, &mut notes);
        assert!(
            notes.finish().is_empty(),
            "phase/swap should not emit a loss diagnostic"
        );
        assert_eq!(r.buses.len(), 1);
        let inv = r.buses[0]
            .inserts
            .get(8)
            .and_then(Option::as_ref)
            .expect("invert effect appended at slot 8");
        assert_eq!(
            inv.kind,
            ir::EffectKind::Invert {
                phase: true,
                swap: true
            }
        );

        // Non-default output routing still diagnoses cleanly.
        let mut notes2 = Notes::default();
        let _ = program(&make_program(2, &[0, 0, 0, 0]), &mut impulses, &mut notes2);
        let diags = notes2.finish();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].impact, ir::Impact::Audible);
        assert_eq!(diags[0].feature, "instrument bus output routing");
    }

    fn render_slot(
        kind: ir::EffectKind,
        output: f32,
        dry: f32,
        input: &[[f32; 2]],
    ) -> Vec<[f32; 2]> {
        use kontra_effects::processors::{Dsp, Responses, processor};
        let slot = ir::Effect {
            kind,
            bypass: false,
            output,
            dry,
        };
        let mut out = input.to_vec();
        match processor(Some(&slot), 48_000, &Responses(&[], &[])) {
            Dsp::Wet(mut unit, _) => {
                for chunk in out.chunks_mut(256) {
                    let mut wet = chunk.to_vec();
                    unit.process(&mut wet);
                    for (dst, w) in chunk.iter_mut().zip(wet.iter()) {
                        dst[0] = dst[0] * dry + w[0] * output;
                        dst[1] = dst[1] * dry + w[1] * output;
                    }
                }
            }
            Dsp::None | Dsp::Eq(_) | Dsp::Bells(_) | Dsp::Filter(_) => {
                let [[ll, lr], [rl, rr]] = kontra_effects::matrix::matrix(&slot);
                for f in &mut out {
                    let (l, r) = (f[0], f[1]);
                    f[0] = ll * l + lr * r;
                    f[1] = rl * l + rr * r;
                }
            }
        }
        out
    }

    fn sine_48k(hz: f64, amp: f32, frames: usize) -> Vec<[f32; 2]> {
        (0..frames)
            .map(|i| {
                let s = amp * (std::f64::consts::TAU * hz * i as f64 / 48_000.0).sin() as f32;
                [s, s]
            })
            .collect()
    }

    fn rms_db_from(buf: &[[f32; 2]], start: usize) -> f64 {
        let s = &buf[start..];
        let ms = s
            .iter()
            .map(|f| f64::from(f[0]) * f64::from(f[0]))
            .sum::<f64>()
            / s.len().max(1) as f64;
        10.0 * ms.max(1e-18).log10()
    }

    /// Replika (0x5a) audio oracle gate against `docs/re/engine/fx-measured/replika-modes.md`:
    /// Modern `1.2 * FEEDBACK` echo gains (-18.42, -5.35, -0.35 dB at 0.1, 0.45, 0.8),
    /// LOWCUT `10 * 100^x` Hz, and Tape/Vintage body rolloff.
    #[test]
    fn replika_matches_kontakt_oracle_echo_gains_and_bodies() {
        let mut imp = vec![[0.0f32; 2]; 16_000];
        imp[0] = [1.0, 1.0];
        for (fb, want_db) in [(0.1_f32, -18.42_f64), (0.45, -5.35), (0.8, -0.35)] {
            let r = crate::replika::Replika {
                ms: 50.0,
                feedback: fb,
                high_cut: 1.0,
                low_cut: 0.0,
                offset: 0.0,
                saturation: 0.0,
                ..crate::replika::INIT
            };
            let out = render_slot(
                ir::EffectKind::Delay(crate::replika::delay(r).unwrap()),
                1.0,
                0.0,
                &imp,
            );
            let peak_near = |c: usize| -> f32 {
                out[c.saturating_sub(6)..(c + 6).min(out.len())]
                    .iter()
                    .map(|f| f[0].abs())
                    .fold(0.0, f32::max)
            };
            let e1 = peak_near(2400);
            let e2 = peak_near(4800);
            let ratio_db = 20.0 * (f64::from(e2) / f64::from(e1)).log10();
            assert!(
                (ratio_db - want_db).abs() < 0.25,
                "Replika fb={fb}: got {ratio_db:.2} dB, want {want_db:.2} dB"
            );
        }
        // Tape (TAPEAGE 1) rolls off 8 kHz ~17.5 dB vs Vintage (QUALITY 1) ~3.2 dB.
        let s8k = sine_48k(8000.0, 0.1, 12_000);
        let in_db = rms_db_from(&s8k, 6000);
        let tape = crate::replika::Replika {
            mode: 2,
            ms: 20.0,
            feedback: 0.0,
            high_cut: 1.0,
            tape_age: 1.0,
            flutter: 0.0,
            offset: 0.0,
            saturation: 0.0,
            ..crate::replika::INIT
        };
        let vintage = crate::replika::Replika {
            mode: 3,
            quality: 1.0,
            ..tape
        };
        let tape_db = rms_db_from(
            &render_slot(
                ir::EffectKind::Delay(crate::replika::delay(tape).unwrap()),
                1.0,
                0.0,
                &s8k,
            ),
            6000,
        ) - in_db;
        let vint_db = rms_db_from(
            &render_slot(
                ir::EffectKind::Delay(crate::replika::delay(vintage).unwrap()),
                1.0,
                0.0,
                &s8k,
            ),
            6000,
        ) - in_db;
        assert!(
            (tape_db - (-17.5)).abs() < 0.8 && (vint_db - (-3.2)).abs() < 0.8,
            "Replika 8kHz bodies: tape={tape_db:.2} dB, vintage={vint_db:.2} dB"
        );
    }

    /// Distortion (0x1e) against `docs/re/engine/fx-measured/distortion-family.md`:
    /// Tube static peaks + 36.8 Hz Butterworth DC blocker + Trans drive.
    #[test]
    fn distortion_matches_kontakt_oracle_tube_and_dc_blocker() {
        // 36.8 Hz Butterworth DC blocker: -3.0 dB at 36.8 Hz, ~0 dB at 500 Hz.
        let low = sine_48k(36.8, 0.05, 16_384);
        let mid = sine_48k(500.0, 0.05, 16_384);
        let tube0 = crate::drive::distortion(0, 0.0, 0.0).unwrap();
        let g_low = rms_db_from(&render_slot(tube0.clone(), 1.0, 0.0, &low), 8192)
            - rms_db_from(&low, 8192);
        let g_mid =
            rms_db_from(&render_slot(tube0, 1.0, 0.0, &mid), 8192) - rms_db_from(&mid, 8192);
        assert!(
            (g_low - g_mid - (-3.01)).abs() < 0.35,
            "Distortion 36.8Hz HPF: {g_low:.2} vs {g_mid:.2}"
        );
        // Tube DRIVE 0.5 asymmetric static curve (+0.5 -> +0.509, -0.5 -> -0.496;
        // DC blocker removes the positive cycle mean, preserving peak-to-peak ~1.005).
        let s1k = sine_48k(1000.0, 0.5, 8192);
        let out_t = render_slot(
            crate::drive::distortion(0, 0.5, 0.0).unwrap(),
            1.0,
            0.0,
            &s1k,
        );
        let pk_pos = out_t[4096..]
            .iter()
            .map(|f| f[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let pk_neg = out_t[4096..]
            .iter()
            .map(|f| f[0])
            .fold(f32::INFINITY, f32::min);
        let p2p = pk_pos - pk_neg;
        assert!(
            (p2p - 1.005).abs() < 0.05,
            "Tube DRIVE=0.5 +/-0.5 p2p: {p2p:.3} (pos={pk_pos:.3}, neg={pk_neg:.3}) vs 1.005"
        );
    }

    /// Saturation (0x1d) Classic, Enhanced, and Drums against
    /// `docs/re/kontakt-bindings/saturation-curves.tsv`.
    #[test]
    fn saturation_all_three_modes_match_kontakt_oracle_curves() {
        let s = sine_48k(200.0, 0.696, 8192);
        let peak = |algo: i32, knob_u: f32| -> f32 {
            let saved_shape = 2.0 * knob_u - 1.0;
            let out = render_slot(
                crate::drive::saturation_mode(algo, saved_shape).unwrap(),
                1.0,
                0.0,
                &s,
            );
            out[4096..].iter().map(|f| f[0].abs()).fold(0.0, f32::max)
        };
        // Enhanced (1): knob 0 -> 0.696 (identity), 0.75 -> ~0.806, 1.0 -> ~0.735.
        assert!((peak(1, 0.0) - 0.696).abs() < 0.02);
        assert!((peak(1, 0.75) - 0.806).abs() < 0.02);
        assert!((peak(1, 1.0) - 0.735).abs() < 0.02);
        // Drums (2): knob 0 -> 0.696, 0.75 -> ~0.634, 1.0 -> ~0.578.
        assert!((peak(2, 0.0) - 0.696).abs() < 0.02);
        assert!((peak(2, 0.75) - 0.634).abs() < 0.02);
        assert!((peak(2, 1.0) - 0.578).abs() < 0.02);
    }

    /// Limiter (0x1c), Hilbert Limiter (0x5f), Lo-Fi (0x20), Raum (0x65),
    /// Wow/Flutter (0x6a), and Solid Bus Comp (0x46) audio oracle proofs.
    #[test]
    fn limiters_lofi_raum_wowflutter_and_bus_comp_match_oracle_laws() {
        let loud = sine_48k(1000.0, 0.9, 12_000);

        // 1. Legacy Limiter: +12 dB input gain brickwall-limits peak <= 1.0.
        let lim = render_slot(
            ir::EffectKind::Limiter(crate::laws::limiter(12.0, 100.0)),
            1.0,
            0.0,
            &loud,
        );
        let lim_pk = lim[4096..]
            .iter()
            .map(|f| f[0].abs())
            .fold(0.0f32, f32::max);
        assert!(lim_pk <= 1.02 && lim_pk > 0.85, "Limiter peak: {lim_pk:.3}");

        // 2. Hilbert / Transparent Limiter: ceiling at 0.5 (-20.15 dB -> ~0.098).
        let tlim = render_slot(
            ir::EffectKind::Limiter(crate::laws::transparent_limiter(0.5, 0.5, 0.5)),
            1.0,
            0.0,
            &loud,
        );
        let tlim_pk = tlim[4096..]
            .iter()
            .map(|f| f[0].abs())
            .fold(0.0f32, f32::max);
        let tlim_db = 20.0 * f64::from(tlim_pk).log10();
        assert!(
            (tlim_db - (-20.15)).abs() < 0.6,
            "Hilbert Limiter ceiling @0.5: {tlim_db:.2} dB vs -20.15 dB"
        );

        // 3. Lo-Fi: noise level raises floor on silence; noise color darkens it.
        let silence = vec![[0.0f32; 2]; 8192];
        let lofi_hi = render_slot(crate::drive::lofi(1.0, 1.0, 0.5, 0.0), 1.0, 0.0, &silence);
        let lofi_lo = render_slot(crate::drive::lofi(1.0, 1.0, 0.5, 1.0), 1.0, 0.0, &silence);
        assert!(rms_db_from(&lofi_hi, 1024) > rms_db_from(&lofi_lo, 1024) + 3.0);

        // 4. Raum: predelay=0.5 delays impulse by 80 ms (3840 frames), freeze sustains tail.
        let mut imp = vec![[0.0f32; 2]; 24_000];
        imp[0] = [1.0, 1.0];
        let mut rp = crate::raum::INIT;
        rp[5] = 0.5; // 80 ms predelay
        let raum_out = render_slot(
            ir::EffectKind::Reverb(crate::raum::raum(rp)),
            1.0,
            0.0,
            &imp,
        );
        let early = raum_out[..3500]
            .iter()
            .map(|f| f[0].abs())
            .fold(0.0f32, f32::max);
        let after = raum_out[3800..8000]
            .iter()
            .map(|f| f[0].abs())
            .fold(0.0f32, f32::max);
        assert!(
            early < 1e-4 && after > 1e-2,
            "Raum 80ms predelay: early={early}, after={after}"
        );

        // 5. Wow/Flutter: gate=0 injects hiss on silence, gate=1 suppresses it.
        let mut wf = crate::wowflutter::INIT;
        wf[2] = 0.8; // scrape
        wf[7] = 0.0; // gate off
        let wf_open = render_slot(
            ir::EffectKind::Delay(crate::wowflutter::wow_flutter(wf)),
            1.0,
            0.0,
            &silence,
        );
        wf[7] = 1.0; // gate on
        let wf_gated = render_slot(
            ir::EffectKind::Delay(crate::wowflutter::wow_flutter(wf)),
            1.0,
            0.0,
            &silence,
        );
        assert!(rms_db_from(&wf_open, 1024) > rms_db_from(&wf_gated, 1024) + 20.0);

        // 6. Solid Bus Comp: 10:1 ratio compresses a 20 dB level step by ~18 dB.
        let (bc, _) = crate::laws::bus_compressor([0.0, 0.2, 0.0, 0.0, 1.0, 1.0], true);
        let quiet = sine_48k(1000.0, 0.05, 12_000);
        let in_step = rms_db_from(&loud, 6000) - rms_db_from(&quiet, 6000);
        let out_step = rms_db_from(
            &render_slot(ir::EffectKind::Compressor(bc), 1.0, 0.0, &loud),
            6000,
        ) - rms_db_from(
            &render_slot(ir::EffectKind::Compressor(bc), 1.0, 0.0, &quiet),
            6000,
        );
        assert!(
            out_step < in_step * 0.35,
            "Solid Bus Comp 10:1 compression: in={in_step:.1} -> out={out_step:.1} dB"
        );
    }
}
