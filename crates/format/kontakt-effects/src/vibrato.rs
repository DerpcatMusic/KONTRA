//! Vibrato/Chorus (module `0x69`, saved v`0x10`) onto [`ir::Scanner`].
//!
//! Wave-digital LC ladder (0.5 H, 4 nF sections, 1 nF last) with 18 sections
//! (DEPTH I..III) or 36 sections (IV..VI), 7.1 kHz Butterworth split, and
//! 16-station rotary scanner pickup.

use kontra_ir as ir;

use crate::module::{MixMode, ids, unit};

const TAPS: [[u8; 9]; 6] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8],
    [0, 1, 2, 4, 6, 8, 10, 11, 12],
    [0, 1, 3, 6, 9, 12, 15, 17, 18],
    [0, 1, 3, 7, 12, 17, 21, 23, 24],
    [0, 1, 4, 9, 15, 21, 26, 29, 30],
    [0, 2, 6, 12, 20, 28, 30, 34, 36],
];
const SHUNT_OHM: [f32; 6] = [206e3, 189e3, 213e3, 198e3, 192e3, f32::INFINITY];

crate::define_effect! {
    struct Vibrato,
    init: INIT,
    id: ids::VIBRATO,
    module_id: 0x69,
    versions: (0, u32::MAX),
    name: "Vibrato/Chorus",
    script_type: 55,
    mix: MixMode::OwnMix,
    saved: 0x10,
    build: |v| ir::EffectKind::Scanner(scanner(v)),
    fields: [
        (depth: i32 = 0,   "$ENGINE_PAR_VC_DEPTH"),
        (color: i32 = 0,   "$ENGINE_PAR_VC_COLOR"),
        (blend: f32 = 0.0, "$ENGINE_PAR_VC_BLEND"),
        (rate:  f32 = 0.6, "$ENGINE_PAR_VC_RATE"),
        (width: f32 = 0.0, "$ENGINE_PAR_VC_WIDTH"),
        (mix:   f32 = 1.0, "$ENGINE_PAR_VC_MIX"),
    ]
}

fn load_ohm(color: i32) -> f32 {
    const P: f32 = 1e-6;
    let u = match color {
        1 => 0.366_666 * (1.0 - P),
        2 => 0.366_666 * P + 0.633_333,
        _ => 0.5,
    };
    15_000.0 * 10f32.powf(4.5 * u - 2.25)
}

#[must_use]
pub fn scanner(v: Vibrato) -> ir::Scanner {
    let d = usize::try_from(v.depth.clamp(0, 5)).unwrap_or(0);
    ir::Scanner {
        sections: if d < 3 { 18 } else { 36 },
        henry: 0.5,
        farad: 4e-9,
        last_farad: 1e-9,
        load_ohm: load_ohm(v.color),
        shunt_ohm: SHUNT_OHM,
        taps: TAPS.get(d).copied().unwrap_or(TAPS[0]),
        hz: 6.0 * 2f32.powf(5.0 * unit(v.rate) - 3.0),
        lead: 0.5 * unit(v.width),
        split_hz: 7100.0,
        high: 0.5,
        blend: unit(v.blend),
        mix: unit(v.mix),
    }
}
