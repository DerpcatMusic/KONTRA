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

/// Module names by chunk id (Kontakt's `BParFX*` classes).
const MODULES: &[(u16, &str)] = &[
    (0x10, "Delay (legacy)"),
    (0x11, "Chorus (legacy)"),
    (0x12, "Flanger (legacy)"),
    (0x13, "Gainer"),
    (0x14, "Phaser (legacy)"),
    (0x15, "Reverb (legacy)"),
    (0x16, "Convolution"),
    (0x17, "Send Levels"),
    (0x18, "Filter"),
    (0x19, "Compressor"),
    (0x1a, "Inverter"),
    (0x1b, "DYX"),
    (0x1c, "Limiter"),
    (0x1d, "Saturation"),
    (0x1e, "Distortion"),
    (0x1f, "Stereo Modeller"),
    (0x20, "Lo-Fi"),
    (0x21, "Skreamer"),
    (0x22, "Rotator"),
    (0x23, "Twang"),
    (0x24, "Cabinet"),
    (0x42, "Tape Saturator"),
    (0x43, "Transient Master"),
    (0x44, "Solid G-EQ"),
    (0x46, "Solid Bus Comp"),
    (0x4c, "Feedback Compressor"),
    (0x4d, "Jump"),
    (0x52, "Van51"),
    (0x53, "AC Box"),
    (0x54, "Hot Solo"),
    (0x55, "Cat"),
    (0x56, "DStortion"),
    (0x57, "Plate Reverb"),
    (0x58, "Cry Wah"),
    (0x59, "Reverb"),
    (0x5a, "Replika"),
    (0x5b, "Phasis"),
    (0x5c, "Flair"),
    (0x5d, "Choral"),
    (0x5e, "Core Cell"),
    (0x5f, "Hilbert Limiter"),
    (0x60, "Supercharger"),
    (0x61, "Bass Pro"),
    (0x63, "Psyche Delay"),
    (0x64, "Ring Modulator"),
    (0x65, "Raum"),
    (0x66, "Bite"),
    (0x68, "Freak"),
    (0x69, "Vibrato Chorus"),
    (0x6a, "Wow / Flutter"),
    (0x6b, "Twin Delay"),
    (0x70, "Stereo Tune"),
    (0x71, "Beat Masher"),
    (0x72, "Gater"),
    (0x73, "Beat Slicer"),
    (0x74, "Reverse Grain"),
    (0x75, "Transpose Stretch"),
    (0x78, "Skreamer Deluxe"),
    (0x79, "Kolor"),
];

fn name(id: u16) -> String {
    MODULES
        .iter()
        .find(|(m, _)| *m == id)
        .map_or_else(|| format!("unknown {id:#04x}"), |(_, n)| (*n).into())
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
    mut laws: Option<(ir::RackAt, &mut Vec<ir::SlotLaw>)>,
) -> Vec<Option<ir::Effect>> {
    let mut out: Vec<Option<ir::Effect>> = array
        .items
        .iter()
        .enumerate()
        .map(|(slot, chunk)| {
            let chunk = chunk.as_ref()?;
            let mut knobs = Vec::new();
            let effect = slot_effect(
                chunk,
                impulses,
                &format!("{at} slot {slot}"),
                notes,
                &mut knobs,
            );
            if let (Some((rack_at, laws)), Ok(slot)) = (laws.as_mut(), u8::try_from(slot)) {
                laws.extend(knobs.into_iter().map(|(knob, field, law)| ir::SlotLaw {
                    at: *rack_at,
                    slot,
                    knob,
                    field,
                    law,
                }));
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
                let (impact, state) = if effect.bypass {
                    (ir::Impact::Cosmetic, "bypassed effect")
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
        !e.bypass && matches!(e.kind, ir::EffectKind::Shaper(_) | ir::EffectKind::Crush(_))
    };
    let last_drive = out.iter().rposition(|e| e.as_ref().is_some_and(drive));
    for (slot, e) in out.iter().enumerate() {
        let Some(e) = e.as_ref().filter(|e| !e.bypass) else {
            continue;
        };
        let lost = match e.kind {
            ir::EffectKind::Sends(_) => Some("group send levels"),
            _ if last_drive.is_none_or(|d| slot > d) => None,
            ir::EffectKind::Convolution { .. } => Some("group convolution before a drive"),
            ir::EffectKind::Reverb(_) => Some("group reverb before a drive"),
            ir::EffectKind::Compressor(_) => Some("group compressor before a drive"),
            ir::EffectKind::Limiter(_) => Some("group limiter before a drive"),
            ir::EffectKind::Delay(_) => Some("group delay before a drive"),
            ir::EffectKind::Phaser(_) => Some("group phaser before a drive"),
            ir::EffectKind::Transient(_) => Some("group transient master before a drive"),
            _ => None,
        };
        if let Some(feature) = lost {
            notes.add(ir::Impact::Audible, feature, format!("{at} slot {slot}"));
        }
    }
}

/// A slot's effect and module id. The saved records (reader layouts in
/// ni-file's effect_parameters.rs): Gainer a linear gain; Inverter phase and
/// swap bytes; Stereo Modeller signed spread (offset from 100% width), signed
/// pan and a pseudo-stereo byte.
fn slot_effect(
    chunk: &Chunk,
    impulses: &mut Impulses,
    at: &str,
    notes: &mut Notes,
    knobs: &mut Vec<crate::script_knobs::SlotKnob>,
) -> Option<(ir::Effect, u16, i32)> {
    let fx = BParFX::try_from(chunk).ok()?;
    let params = fx.params().ok()?;
    let effect = fx.effect()?;
    let object = StructuredObject::try_from(effect).ok()?;
    let public = object.public_data;
    let f32_at = |n: usize| {
        let bytes = public.get(n..n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    let byte = |n: usize| public.get(n).map(|&b| b != 0);
    let kind_id = public
        .get(..4)
        .and_then(|b| b.try_into().ok())
        .map(i32::from_le_bytes);
    let kind = match effect.id {
        0x13 => f32_at(0).map(ir::EffectKind::Gain),
        0x1a => byte(0)
            .zip(byte(1))
            .map(|(phase, swap)| ir::EffectKind::Invert { phase, swap }),
        // ponytail: pseudo stereo (a short delay on the right) plays as plain width.
        0x1f => f32_at(0)
            .zip(f32_at(4))
            .map(|(spread, pan)| ir::EffectKind::Stereo {
                width: (spread.clamp(-1.0, 1.0) + 1.0) * 0.5,
                pan: pan.clamp(-1.0, 1.0),
            }),
        0x18 => kind_id.and_then(|k| eq(&public, k, knobs).or_else(|| filter(&public, k, knobs))),
        // Mode (i32), threshold, ratio, attack, release, then the link byte.
        0x19 => kind_id
            .zip(f32_at(4).zip(f32_at(8)))
            .zip(f32_at(12).zip(f32_at(16)))
            .zip(byte(20))
            .map(|(((mode, (t, r)), (a, rel)), link)| {
                *knobs = crate::script_knobs::compressor_knobs(mode, [t, r, a, rel], link);
                ir::EffectKind::Compressor(crate::laws::compressor(mode, t, r, a, rel, link))
            }),
        0x1c => f32_at(0)
            .zip(f32_at(4))
            .map(|(gain, release)| ir::EffectKind::Limiter(crate::laws::limiter(gain, release))),
        0x5f if object.version == 0x51 => {
            f32_at(0)
                .zip(f32_at(4))
                .zip(f32_at(8))
                .map(|((drive, release), ceiling)| {
                    ir::EffectKind::Limiter(crate::laws::transparent_limiter(
                        drive, release, ceiling,
                    ))
                })
        }
        // Six floats, the link byte, then (0x11 on) the side-chain byte.
        0x46 if (0x10..=0x12).contains(&object.version) => {
            let impact = if params.bypass {
                ir::Impact::Cosmetic
            } else {
                ir::Impact::Audible
            };
            if object.version >= 0x11 && byte(25) == Some(true) {
                notes.add(impact, "Solid Bus Comp side chain", at.to_owned());
            }
            (0..6)
                .map(|i| f32_at(4 * i))
                .collect::<Option<Vec<_>>>()
                .and_then(|v| v.try_into().ok())
                .zip(byte(24))
                .map(|(knobs, link)| {
                    let (c, auto) = crate::laws::bus_compressor(knobs, link);
                    if auto {
                        notes.add(impact, "Solid Bus Comp auto release", at.to_owned());
                    }
                    ir::EffectKind::Compressor(c)
                })
        }
        0x43 | 0x4c => {
            crate::dynamics::effect(effect.id, object.version, &public).map(|(kind, lost)| {
                if let Some(feature) = lost {
                    let impact = if params.bypass {
                        ir::Impact::Cosmetic
                    } else {
                        ir::Impact::Audible
                    };
                    notes.add(impact, feature, at.to_owned());
                }
                kind
            })
        }
        0x17 => sends(&public),
        0x44 => solid_eq(object.version, &public, at, notes),
        0x16 => impulses
            .convolution(object.version, &public, at, notes)
            .map(|impulse| ir::EffectKind::Convolution { impulse }),
        // The room type (i32), then nine floats.
        0x59 => (0..9)
            .map(|i| f32_at(4 + 4 * i))
            .collect::<Option<Vec<_>>>()
            .and_then(|v| v.try_into().ok())
            .zip(kind_id)
            .map(|(v, kind)| ir::EffectKind::Reverb(crate::laws::reverb(kind, v))),
        0x10..=0x12 | 0x14 => crate::delays::legacy(effect.id, object.version, &public),
        0x15 => {
            if let Some(saved) = crate::legacy_reverb::saved(object.version, &public) {
                *knobs = crate::script_knobs::legacy_reverb_knobs(saved);
            }
            crate::legacy_reverb::effect(object.version, &public)
        }
        id => crate::drive::effect(id, object.version, &public, at, notes),
    };
    // The legacy delay, chorus and flanger, the Gainer and Convolution mix
    // their slot's dry level back in; other modules' stored dry is unused
    // (trims store 1 there). v2 sampler-kontakt effects/mod.rs, measured on Una Corda's trims.
    // The reverbs run fully wet; Return and Dry carry their mix (v2 effects/space.rs).
    let parallel = matches!(effect.id, 0x10..=0x12 | 0x15 | 0x16 | 0x59);
    // An EQ (Filter kinds 22..=24) has no Output control: ANALOG STRINGS
    // stores 0 on one that plays in Kontakt (v2 sampler-kontakt effects/mod.rs, corpus of
    // 12,918 EQ slots).
    let eq = matches!(
        kind,
        Some(ir::EffectKind::Eq(_) | ir::EffectKind::Filter(_))
    );
    let effect_out = ir::Effect {
        kind: kind.unwrap_or(if parallel {
            ir::EffectKind::OtherWet
        } else {
            ir::EffectKind::Other
        }),
        bypass: params.bypass,
        output: if eq {
            1.0
        } else if effect.id == 0x15 {
            params.output_gain * crate::legacy_reverb::WET
        } else {
            params.output_gain
        },
        dry: if parallel || effect.id == 0x13 {
            params.dry_level
        } else {
            0.0
        },
    };
    Some((effect_out, effect.id, kind_id.unwrap_or(-1)))
}

/// The 1/2/3-band EQ (Filter types 22..=24): the record ends with its bands,
/// each Hz, octaves and dB (layouts in v2 sampler-kontakt effects/mod.rs,
/// reader 0x140d00db0).
fn eq(
    public: &[u8],
    kind: i32,
    knobs: &mut Vec<crate::script_knobs::SlotKnob>,
) -> Option<ir::EffectKind> {
    let count = usize::try_from(kind.checked_sub(21)?)
        .ok()
        .filter(|n| (1..=3).contains(n))?;
    let tail = public.get(public.len().checked_sub(count * 12)?..)?;
    let mut bands = [ir::Bell::FLAT; 4];
    let mut saved = Vec::new();
    for (band, raw) in bands.iter_mut().zip(tail.as_chunks::<12>().0) {
        let f = |n: usize| {
            let bytes = raw.get(n..n + 4)?.try_into().ok()?;
            Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
        };
        *band = crate::laws::eq_band(f(0)?, f(4)?, f(8)?)?;
        saved.push([f(0)?, f(4)?, f(8)?]);
    }
    *knobs = crate::script_knobs::eq_knobs(&saved);
    Some(ir::EffectKind::Eq(bands))
}

/// The Solid G-EQ (0x44) record (reader 0x140d029f0): LF gain, Hz, bell
/// flag; LMF and HMF gain, Hz, Q; HF gain, Hz, bell flag (knobs 0..=1, flags
/// a byte each); v0x11 adds high- and low-pass Hz and their switches.
fn solid_eq(version: u16, public: &[u8], at: &str, notes: &mut Notes) -> Option<ir::EffectKind> {
    let f = |n: usize| {
        let bytes = public.get(n..n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    let flag = |n: usize| public.get(n).map(|&b| b != 0);
    let bell = |on: bool, shelf| if on { ir::Shape::Peak } else { shelf };
    // ponytail: v0x11's pass filters are reported, not run (no corpus slot uses them yet).
    if version > 0x10 && (flag(50)? || flag(51)?) {
        notes.add(
            ir::Impact::Audible,
            "Solid G-EQ pass filters",
            at.to_string(),
        );
    }
    Some(ir::EffectKind::Eq(crate::laws::solid_eq(
        [f(0)?, f(4)?],
        bell(flag(8)?, ir::Shape::LowShelf),
        [f(9)?, f(13)?, f(17)?],
        [f(21)?, f(25)?, f(29)?],
        [f(33)?, f(37)?],
        bell(flag(41)?, ir::Shape::HighShelf),
    )))
}

/// A filter: the kind, repeated (newer records put a flag byte before a
/// repeat), a leading gain for Ladder and Daft (the SV Par/Ser bandwidth,
/// unused), then cutoff and resonance, all 0..=1 (v2 sampler-kontakt
/// effects/mod.rs, reader 0x140d00db0).
fn filter(
    public: &[u8],
    kind: i32,
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
    let (gain, at) = if matches!(kind, 30..=41 | 63..=65 | 70 | 71) {
        (f(0)?, 4)
    } else {
        (0.0, 0)
    };
    let (cutoff, resonance) = (f(at)?, f(at + 4)?);
    let filter = crate::laws::filter(kind, cutoff, resonance, gain)?;
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

pub(crate) fn group(
    group: &Group,
    index: usize,
    impulses: &mut Impulses,
    notes: &mut Notes,
    laws: &mut Vec<ir::SlotLaw>,
) -> Vec<Option<ir::Effect>> {
    let rack_at = u32::try_from(index)
        .ok()
        .map(|g| (ir::RackAt::Group(g), laws));
    match group.insert_fx() {
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

/// The program's racks: instrument insert, send and main, then the
/// instrument buses (Kontakt's 0..=15), each with its rack.
pub(crate) struct Racks {
    pub inserts: Vec<Option<ir::Effect>>,
    pub sends: Vec<Option<ir::Effect>>,
    pub main: Vec<Option<ir::Effect>>,
    pub buses: Vec<ir::Bus>,
    /// The slot laws of these racks.
    pub laws: Vec<ir::SlotLaw>,
}

pub(crate) fn program(program: &Program, impulses: &mut Impulses, notes: &mut Notes) -> Racks {
    let names = ["instrument insert", "instrument send", "instrument main"];
    let mut racks = 0;
    let mut instrument = [Vec::new(), Vec::new(), Vec::new()];
    let mut buses: Vec<ir::Bus> = Vec::new();
    let mut laws = Vec::new();
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
