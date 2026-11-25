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
/// record maps to an effect. A module not listed, or listed with
/// [`Map::Unread`], passes at its output gain and is reported.
struct Fx {
    id: u16,
    name: &'static str,
    script_type: i32,
    parallel: bool,
    map: Map,
}

enum Map {
    /// Not carried yet.
    Unread,
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

use Map::{Read, Unread};
use kontra_kontakt_effects::module::Module;

#[rustfmt::skip]
const MODULES: &[Fx] = &[
    fx(0x10, "Delay (legacy)", 18, true, Read(legacy_delay)),
    fx(0x11, "Chorus (legacy)", 16, true, Read(legacy_delay)),
    fx(0x12, "Flanger (legacy)", 15, true, Read(legacy_delay)),
    fx(0x13, "Gainer", 20, false, Read(|s| s.f32_at(0).map(ir::EffectKind::Gain))),
    fx(0x14, "Phaser (legacy)", 14, true, Read(legacy_delay)),
    fx(0x15, "Reverb (legacy)", 17, true, Read(legacy_reverb)),
    fx(0x16, "Convolution", 19, true, Read(convolution)),
    fx(0x17, "Send Levels", 11, false, Read(|s| sends(s.public))),
    fx(0x18, "Filter", 7, false, Read(|s| s.kind_id.and_then(|k| eq(s.public, k, s.at, s.knobs).or_else(|| filter(s.public, k, s.at, s.knobs))))),
    fx(0x19, "Compressor", 2, false, Read(compressor)),
    fx(0x1a, "Inverter", 4, false, Read(|s| s.byte(0).zip(s.byte(1)).map(|(phase, swap)| ir::EffectKind::Invert { phase, swap }))),
    fx(0x1b, "DYX", -1, false, Unread),
    fx(0x1c, "Limiter", 3, false, Read(limiter)),
    fx(0x1d, "Saturation", 6, false, Read(saturation)),
    fx(0x1e, "Distortion", 10, false, Read(|s| crate::drive::distortion(s.i32_at(0)?, s.f32_at(4)?, s.f32_at(8)?))),
    fx(0x1f, "Stereo Modeller", 9, false, Read(stereo)),
    fx(0x20, "Lo-Fi", 8, false, Read(lofi)),
    model(&crate::skreamer::MODULE, "Skreamer", false, |s| crate::skreamer::model(s.public)),
    fx(0x22, "Rotator", 22, false, Unread),
    fx(0x23, "Twang", 23, false, Unread),
    fx(0x24, "Cabinet", 24, false, Unread),
    model(&crate::tape::MODULE, "Tape Saturator", false, |s| crate::tape::model(s.public)),
    fx(0x43, "Transient Master", 27, false, Read(dynamics)),
    fx(0x44, "Solid G-EQ", 28, false, Read(|s| solid_eq(s.version, s.public, s.bypass, s.at, s.notes, s.model))),
    fx(0x46, "Solid Bus Comp", 29, false, Read(bus_compressor)),
    fx(0x4c, "Feedback Compressor", 30, false, Read(dynamics)),
    fx(0x4d, "Jump", 31, false, Unread),
    fx(0x52, "Van51", 32, false, Unread),
    fx(0x53, "AC Box", 33, false, Unread),
    fx(0x54, "Hot Solo", 34, false, Unread),
    fx(0x55, "Cat", 35, false, Unread),
    model(&crate::dstortion::MODULE, "DStortion", false, |s| {
        if s.version != 0x10 { return None }
        if crate::dstortion::mono(s.public) { s.lost("DStortion mono") }
        crate::dstortion::model(s.public)
    }),
    fx(0x57, "Plate Reverb", 38, true, Unread),
    fx(0x58, "Cry Wah", 37, false, Unread),
    fx(0x59, "Reverb", 39, true, Read(reverb)),
    fx(0x5a, "Replika", 40, true, Read(replika)),
    model(&crate::phasis::MODULE, "Phasis", false, |s| crate::phasis::saved(s.version, s.public).map(crate::phasis::model_from)),
    model(&crate::flair::MODULE, "Flair", false, |s| crate::flair::saved(s.version, s.public).map(crate::flair::model_from)),
    model(&crate::choral::MODULE, "Choral", false, |s| crate::choral::saved(s.version, s.public).map(crate::choral::model_from)),
    fx(0x5e, "Core Cell", -1, false, Unread),
    fx(0x5f, "Hilbert Limiter", -1, false, Read(transparent_limiter)),
    model(&crate::supergt::MODULE, "Supercharger", false, |s| if s.version == 0x51 { crate::supergt::model(s.public) } else { None }),
    fx(0x61, "Bass Pro", 47, false, Unread),
    model(&crate::psyche::MODULE, "Psyche Delay", true, |s| {
        let p = crate::psyche::saved(s.version, s.public)?;
        if p.beats.is_some() { s.lost("Psyche Delay tempo sync (unmeasured)") }
        Some(crate::psyche::model(p))
    }),
    fx(0x64, "Ring Modulator", 50, false, Unread),
    model(&crate::raum::MODULE, "Raum", true, |s| {
        let (p, _mix, lost) = crate::raum::saved(s.version, s.public)?;
        lost.into_iter().for_each(|feature| s.lost(feature));
        Some(crate::raum::model_from(p))
    }),
    model(&crate::bite::MODULE, "Bite", false, |s| crate::bite::saved(s.public).map(crate::bite::model)),
    model(&crate::freak::MODULE, "Freak", false, |s| crate::freak::saved(s.public).map(crate::freak::model)),
    model(&crate::vibrato::MODULE, "Vibrato Chorus", false, |s| crate::vibrato::saved(s.version, s.public).map(crate::vibrato::model)),
    model(&crate::wowflutter::MODULE, "Wow / Flutter", false, |s| crate::wowflutter::saved(s.version, s.public).map(|(p, _mix)| crate::wowflutter::model_from(p))),
    fx(0x6b, "Twin Delay", 57, true, Unread),
    fx(0x70, "Stereo Tune", 61, false, Unread),
    fx(0x71, "Beat Masher", 62, false, Unread),
    fx(0x72, "Gater", 63, false, Unread),
    fx(0x73, "Beat Slicer", 64, false, Unread),
    fx(0x74, "Reverse Grain", 65, false, Unread),
    fx(0x75, "Transpose Stretch", 66, false, Unread),
    fx(0x78, "Skreamer Deluxe", 69, false, Unread),
    fx(0x79, "Kolor", 70, false, Unread),
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
            ir::EffectKind::Clocked(_) => Some("group Phaser or Vowel filter before a drive"),
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
    };
    let kind = match lookup(effect.id).map(|m| &m.map) {
        None | Some(Map::Unread) => None,
        Some(Map::Read(read)) => read(&mut slot),
        Some(Map::Model(read)) => read(&mut slot).and_then(|m| {
            let kind = m.build(&m.saved);
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
    let wow_mix = (effect.id == 0x6a)
        .then(|| crate::wowflutter::saved(object.version, &public).map(|(_, m)| m.clamp(0.0, 1.0)))
        .flatten();
    let raum_mix = (effect.id == 0x65)
        .then(|| {
            crate::raum::saved(object.version, &public).map(|(_, m, _)| crate::raum::mix_gains(m))
        })
        .flatten();
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
            crate::supergt::mix_dry(&public, params.output_gain)
        } else if let Some(mix) = wow_mix {
            params.output_gain * (1.0 - mix)
        } else {
            0.0
        },
    };
    Some((effect_out, effect.id, kind_id.unwrap_or(-1)))
}

/// Pseudo stereo (`BParFXStereoSpread +0x10`, `0140920000.c:5423-5436`)
/// adds a short decorrelation delay on the right; reported when enabled
/// rather than silently playing plain stereo width. Spread is signed (offset
/// from 100% width), then pan, then the pseudo-stereo byte.
fn stereo(s: &mut Slot) -> Option<ir::EffectKind> {
    let (spread, pan) = s.f32_at(0).zip(s.f32_at(4))?;
    if s.byte(8) == Some(true) {
        s.lost("Stereo Modeller pseudo stereo");
    }
    Some(ir::EffectKind::Stereo {
        width: (spread.clamp(-1.0, 1.0) + 1.0) * 0.5,
        pan: pan.clamp(-1.0, 1.0),
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

fn limiter(s: &mut Slot) -> Option<ir::EffectKind> {
    let (gain, release) = s.f32_at(0).zip(s.f32_at(4))?;
    let value = crate::laws::limiter(gain, release);
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId("kontakt.limiter.v1"),
        location: format!("{}/limiter", s.at),
        input: format!("{:?}", (gain, release)),
        output: format!("{value:?}"),
    });
    Some(ir::EffectKind::Limiter(value))
}

fn transparent_limiter(s: &mut Slot) -> Option<ir::EffectKind> {
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
    Some(ir::EffectKind::Limiter(value))
}

/// Six floats, the link byte, then (0x11 on) the side-chain byte.
fn bus_compressor(s: &mut Slot) -> Option<ir::EffectKind> {
    if !(0x10..=0x12).contains(&s.version) {
        return None;
    }
    if s.version >= 0x11 && s.byte(25) == Some(true) {
        s.lost("Solid Bus Comp side chain");
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

/// Transient Master and Feedback Compressor.
fn dynamics(s: &mut Slot) -> Option<ir::EffectKind> {
    if s.id == 0x4c && s.version == 0x10 {
        *s.model = crate::dynamics::feedback_model(s.public);
    }
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

/// Delay, Chorus, Flanger and Phaser (legacy); the Delay's knobs move.
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

fn replika(s: &mut Slot) -> Option<ir::EffectKind> {
    let r = crate::replika::saved(s.version, s.public)?;
    for lost in crate::replika::lost(r) {
        s.lost(lost);
    }
    *s.knobs = crate::replika::knobs(r);
    crate::replika::delay(r).map(ir::EffectKind::Delay)
}

/// Shape −1..=1 and the algorithm (0 Classic, 1 Enhanced, 2 Drums).
fn saturation(s: &mut Slot) -> Option<ir::EffectKind> {
    // Version 0x70 of the same id is the Surround Panner.
    if s.version == 0x70 {
        return None;
    }
    let (shape, algorithm) = (s.f32_at(0)?, s.i32_at(4)?);
    if algorithm != 0 {
        // ponytail: Enhanced and Drums are a table and a peak follower (v2
        // sampler-core dsp/color/saturator.rs); none in the corpus yet.
        s.lost("Saturation Enhanced or Drums");
        return None;
    }
    Some(crate::drive::saturation(shape))
}

/// Bits, frequency, a flag, noise and colour.
fn lofi(s: &mut Slot) -> Option<ir::EffectKind> {
    if s.byte(8) == Some(true) {
        s.lost("Lo-Fi flag (law unknown)");
    }
    Some(crate::drive::lofi(
        s.f32_at(0)?,
        s.f32_at(4)?,
        s.f32_at(9)?,
        s.f32_at(13)?,
    ))
}

/// The 1/2/3-band EQ (Filter types 22..=24): the record ends with its bands,
/// each Hz, octaves and dB (layouts in v2 sampler-kontakt effects/mod.rs,
/// reader 0x140d00db0).
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

/// The Solid G-EQ (0x44) record (reader 0x140d029f0): LF gain, Hz, bell
/// flag; LMF and HMF gain, Hz, Q; HF gain, Hz, bell flag (knobs 0..=1, flags
/// a byte each); v0x11 adds high- and low-pass Hz and their switches.
fn solid_eq(
    version: u16,
    public: &[u8],
    bypass: bool,
    at: &str,
    notes: &mut Notes,
    model: &mut Option<ir::KnobModel>,
) -> Option<ir::EffectKind> {
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
    *model = Some(crate::solid_knobs::solid_model(saved, bells));
    let [lg, lh, mg, mh, mq, hg, hh, hq, fg, fh] = saved;
    let [lf, hf] = shapes;
    let bands = crate::laws::solid_eq([lg, lh], lf, [mg, mh, mq], [hg, hh, hq], [fg, fh], hf);
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId("kontakt.solid_eq.v1"),
        location: format!("{at}/solid_eq"),
        input: format!("saved={saved:?}, shapes={shapes:?}"),
        output: format!("{bands:?}"),
    });
    Some(ir::EffectKind::Eq(bands))
}

/// A filter: the kind, repeated (newer records put a flag byte before a
/// repeat), a leading gain for Ladder and Daft (the SV Par/Ser bandwidth,
/// unused), then cutoff and resonance, all 0..=1 (v2 sampler-kontakt
/// effects/mod.rs, reader 0x140d00db0).
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
                // KT-22: a bus routed to its own output skips the instrument
                // racks; the tail holds phase invert, L/R swap and newer
                // fields. Neither is modelled yet: every bus plays on main.
                if let Some(p) = &params {
                    if p.output != -1 {
                        notes.add(
                            ir::Impact::Audible,
                            "instrument bus output routing",
                            format!("{at} to output {}", p.output),
                        );
                    }
                    if p.unknown_tail.iter().any(|&b| b != 0) {
                        notes.add(
                            ir::Impact::Audible,
                            "instrument bus phase/swap flags",
                            format!("{at}: {:02x?}", p.unknown_tail),
                        );
                    }
                }
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
                    (BUS, Some(bus)) => bus.inserts = effects,
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
        // Raum mix (offset 20 = 0.5) follows binary 0x141982250: (dry=1, wet=1) * output_gain (0.7).
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

        // Stereo Modeller (0x1f) with pseudo stereo enabled emits an Audible diagnostic.
        let mut stereo_pub = Vec::new();
        stereo_pub.extend(0.5f32.to_le_bytes()); // spread
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
        assert!(matches!(e.kind, ir::EffectKind::Stereo { .. }));
        let diags = pseudo_notes.finish();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].impact, ir::Impact::Audible);
        assert_eq!(diags[0].feature, "Stereo Modeller pseudo stereo");
    }
}
