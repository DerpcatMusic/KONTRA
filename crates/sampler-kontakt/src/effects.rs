//! Kontakt effect racks: where they live in a program, and module names.
//!
//! A program holds up to three 8-slot racks as `BParamArray<BParFX,8>`
//! children (0x3a: insert, send, main, in that order) and up to 16 instrument
//! buses (0x45, each with its own rack). Groups keep an insert rack in their
//! private data. Each slot's first child is the effect object, whose chunk
//! ID names the module.

use ni_file::kontakt::{
    Chunk, StructuredObject,
    objects::{BParFX, BParamArrayBParFX8, InsertBus, Program},
};

const RACK: u16 = 0x3a;
const BUS: u16 = 0x45;

/// Module names by serialization ID (Kontakt's `BParFX*` classes).
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
    (0x1d, "Surround Panner"),
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
];

pub(crate) fn module_name(id: u16) -> String {
    MODULES
        .iter()
        .find(|(m, _)| *m == id)
        .map_or_else(|| format!("unknown effect {id:#04x}"), |(_, n)| (*n).into())
}

/// One occupied rack slot.
pub(crate) struct Slot {
    pub slot: usize,
    pub module: u16,
    pub version: u16,
    pub bypass: bool,
    pub output_gain: f32,
    pub dry_level: f32,
    pub public: Vec<u8>,
    pub private: Vec<u8>,
}

pub(crate) fn rack(array: &BParamArrayBParFX8) -> Vec<Slot> {
    array
        .items
        .iter()
        .enumerate()
        .filter_map(|(slot, chunk)| {
            let fx = BParFX::try_from(chunk.as_ref()?).ok()?;
            let state = fx.params().ok()?;
            let effect: &Chunk = fx.effect()?;
            let object = StructuredObject::try_from(effect).ok();
            Some(Slot {
                slot,
                module: effect.id,
                version: object.as_ref().map_or(0, |o| o.version),
                bypass: state.bypass,
                output_gain: state.output_gain,
                dry_level: state.dry_level,
                public: object
                    .as_ref()
                    .map_or_else(Vec::new, |o| o.public_data.clone()),
                private: object.map_or_else(Vec::new, |o| o.private_data),
            })
        })
        .collect()
}

/// The program's racks with their locations: instrument insert, send and
/// main, then each bus.
pub(crate) fn program_racks(program: &Program) -> Vec<(String, Vec<Slot>)> {
    let names = ["instrument insert", "instrument send", "instrument main"];
    let mut out = Vec::new();
    let mut racks = 0;
    let mut buses = 0;
    for child in &program.0.children {
        match child.id {
            RACK => {
                let name = names
                    .get(racks)
                    .map_or_else(|| format!("rack {racks}"), |n| (*n).into());
                racks += 1;
                if let Ok(array) = BParamArrayBParFX8::try_from(child) {
                    out.push((name, rack(&array)));
                }
            }
            BUS => {
                let index = buses;
                buses += 1;
                if let Ok(bus) = InsertBus::try_from(child)
                    && let Some(array) = bus
                        .0
                        .find_first(RACK)
                        .and_then(|c| BParamArrayBParFX8::try_from(c).ok())
                {
                    out.push((format!("bus {index}"), rack(&array)));
                }
            }
            _ => {}
        }
    }
    out
}
