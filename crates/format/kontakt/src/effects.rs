//! Effect racks: where Kontakt keeps them and what each slot holds. The
//! engine runs no effects yet, so every active slot is reported.
//!
//! A program holds up to three 8-slot racks (0x3a: insert, send, main, in
//! that order) and up to 16 buses (0x45, each with a rack); a group keeps an
//! insert rack. A slot's first child is the effect, its chunk id the module.

use ni_file::kontakt::objects::{BParFX, BParamArrayBParFX8, Group, InsertBus, Program};

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

fn module(id: u16) -> String {
    MODULES
        .iter()
        .find(|(m, _)| *m == id)
        .map_or_else(|| format!("unknown {id:#04x}"), |(_, n)| (*n).into())
}

/// Reports a rack's slots: active ones as audible gaps, bypassed ones as
/// cosmetic (a script may switch them on).
fn rack(array: &BParamArrayBParFX8, at: &str, notes: &mut Notes) {
    for (slot, chunk) in array.items.iter().enumerate() {
        let Some(chunk) = chunk else { continue };
        let fx = BParFX::try_from(chunk);
        let (Ok(params), Some(effect)) = (
            fx.as_ref()
                .map_err(|_| ())
                .and_then(|f| f.params().map_err(|_| ())),
            fx.as_ref().ok().and_then(BParFX::effect),
        ) else {
            notes.add(
                ir::Impact::Audible,
                "effect slot unreadable",
                format!("{at} slot {slot}"),
            );
            continue;
        };
        let (impact, state) = if params.bypass {
            (ir::Impact::Cosmetic, "bypassed effect")
        } else {
            (ir::Impact::Audible, "effect")
        };
        let detail = format!("{at} slot {slot}");
        notes.add(impact, &format!("{state} {}", module(effect.id)), detail);
    }
}

pub(crate) fn group(group: &Group, index: usize, notes: &mut Notes) {
    match group.insert_fx() {
        Ok(array) => rack(&array, &format!("group {index} insert"), notes),
        Err(e) => notes.add(
            ir::Impact::Audible,
            "effect rack unreadable",
            format!("group {index}: {e}"),
        ),
    }
}

pub(crate) fn program(program: &Program, notes: &mut Notes) {
    let names = ["instrument insert", "instrument send", "instrument main"];
    let (mut racks, mut buses) = (0, 0);
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
                buses += 1;
                let array = InsertBus::try_from(child).and_then(|bus| {
                    let rack = bus
                        .0
                        .find_first(RACK)
                        .ok_or(ni_file::Error::Static("bus without a rack"))?;
                    BParamArrayBParFX8::try_from(rack)
                });
                (format!("bus {}", buses - 1), array)
            }
            _ => continue,
        };
        match array {
            Ok(array) => rack(&array, &at, notes),
            Err(e) => notes.add(
                ir::Impact::Audible,
                "effect rack unreadable",
                format!("{at}: {e}"),
            ),
        }
    }
}
