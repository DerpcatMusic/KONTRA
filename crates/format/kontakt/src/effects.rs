//! Effect racks: where Kontakt keeps them and what each slot holds. The
//! engine carries the linear stereo modules; every other active slot is reported.
//!
//! A program holds up to three 8-slot racks (0x3a: insert, send, main, in
//! that order) and up to 16 buses (0x45, each with a rack); a group keeps an
//! insert rack. A slot's first child is the effect, its chunk id the module.

use ni_file::kontakt::objects::{BParFX, BParamArrayBParFX8, Group, InsertBus, Program};
use ni_file::kontakt::{Chunk, StructuredObject};

use crate::translate::Notes;
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
fn rack(array: &BParamArrayBParFX8, at: &str, notes: &mut Notes) -> Vec<Option<ir::Effect>> {
    let mut out: Vec<Option<ir::Effect>> = array
        .items
        .iter()
        .enumerate()
        .map(|(slot, chunk)| {
            let chunk = chunk.as_ref()?;
            let effect = slot_effect(chunk);
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
    out
}

/// A slot's effect and module id. The saved records (reader layouts in
/// ni-file's effect_parameters.rs): Gainer a linear gain; Inverter phase and
/// swap bytes; Stereo Modeller signed spread (offset from 100% width), signed
/// pan and a pseudo-stereo byte.
fn slot_effect(chunk: &Chunk) -> Option<(ir::Effect, u16, i32)> {
    let fx = BParFX::try_from(chunk).ok()?;
    let params = fx.params().ok()?;
    let effect = fx.effect()?;
    let public = StructuredObject::try_from(effect).ok()?.public_data;
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
        0x18 => kind_id.and_then(|k| eq(&public, k)),
        _ => None,
    };
    // The legacy delay, chorus and flanger, the Gainer and Convolution mix
    // their slot's dry level back in; other modules' stored dry is unused
    // (trims store 1 there). v2 sampler-kontakt effects/mod.rs, measured on Una Corda's trims.
    let parallel = matches!(effect.id, 0x10..=0x12 | 0x16);
    // An EQ (Filter kinds 22..=24) has no Output control: ANALOG STRINGS
    // stores 0 on one that plays in Kontakt (v2 sampler-kontakt effects/mod.rs, corpus of
    // 12,918 EQ slots).
    let eq = effect.id == 0x18 && kind_id.is_some_and(|k| (22..=24).contains(&k));
    let effect_out = ir::Effect {
        kind: kind.unwrap_or(if parallel {
            ir::EffectKind::OtherWet
        } else {
            ir::EffectKind::Other
        }),
        bypass: params.bypass,
        output: if eq { 1.0 } else { params.output_gain },
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
fn eq(public: &[u8], kind: i32) -> Option<ir::EffectKind> {
    let count = usize::try_from(kind - 21)
        .ok()
        .filter(|n| (1..=3).contains(n))?;
    let tail = public.get(public.len().checked_sub(count * 12)?..)?;
    let mut bands = [ir::Bell::FLAT; 3];
    for (band, raw) in bands.iter_mut().zip(tail.as_chunks::<12>().0) {
        let f = |n: usize| {
            let bytes = raw.get(n..n + 4)?.try_into().ok()?;
            Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
        };
        *band = crate::laws::eq_band(f(0)?, f(4)?, f(8)?)?;
    }
    Some(ir::EffectKind::Eq(bands))
}

pub(crate) fn group(group: &Group, index: usize, notes: &mut Notes) -> Vec<Option<ir::Effect>> {
    match group.insert_fx() {
        Ok(array) => rack(&array, &format!("group {index} insert"), notes),
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

/// Reports every program rack; returns the instrument insert rack and the
/// instrument buses (Kontakt's 0..=15), each with its rack.
pub(crate) fn program(
    program: &Program,
    notes: &mut Notes,
) -> (Vec<Option<ir::Effect>>, Vec<ir::Bus>) {
    let names = ["instrument insert", "instrument send", "instrument main"];
    let mut racks = 0;
    let mut inserts = Vec::new();
    let mut buses: Vec<ir::Bus> = Vec::new();
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
                    inserts: Vec::new(),
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
                let effects = rack(&array, &at, notes);
                match (child.id, buses.last_mut()) {
                    (BUS, Some(bus)) => bus.inserts = effects,
                    _ if racks == 1 => inserts = effects,
                    _ => {}
                }
            }
            Err(e) => notes.add(
                ir::Impact::Audible,
                "effect rack unreadable",
                format!("{at}: {e}"),
            ),
        }
    }
    (inserts, buses)
}
