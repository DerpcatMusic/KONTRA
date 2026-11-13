//! A Kontakt wavetable group (source mode 9) as the model's wavetable.

use kontra_format::Notes;
use kontra_ir::{self as ir, FormShaper, Side, Warp, WarpShape};
use ni_file::kontakt::objects::WavetableSource;

/// The source's table, forms and start phase. Its modulation oscillator
/// and inharmonic partials are not played yet: each leaves a note.
pub(crate) fn wavetable(source: &WavetableSource, group: &str, notes: &mut Notes) -> ir::Wavetable {
    let mut warp = |form: u32, amount: f32| {
        let kind = form.checked_sub(1);
        let shape = kind.and_then(shape);
        if shape.is_none() {
            notes.add(
                ir::Impact::Audible,
                "wavetable phase form",
                format!("{group}: {form}"),
            );
        }
        Warp {
            shape: shape.unwrap_or(WarpShape::Off),
            shaper: kind.map_or(FormShaper::Off, shaper),
            amount,
        }
    };
    let warps = [
        warp(source.form_type, source.form1),
        warp(source.form2_type, source.form2),
    ];
    for (on, feature) in [
        (source.mod_type != 0, "wavetable modulation"),
        (source.inharmonic_enabled, "wavetable inharmonic"),
    ] {
        if on {
            notes.add(ir::Impact::Audible, feature, group);
        }
    }
    ir::Wavetable {
        position: source.position,
        phase: source.phase,
        phase_random: source.phase_random.clamp(0.0, 1.0),
        warps,
    }
}

/// The wavetable value a modulation target moves: Kontakt adds the routes
/// to it in its 0..=1 units and clamps the sum every 32 frames
/// (`fminf`/`fmaxf` at 0x140b52133..0x140b521b4, targets +0x10260/4/74).
pub(crate) fn target(param: &str) -> Option<ir::TableValue> {
    Some(match param {
        "wavetablePosition" => ir::TableValue::Position,
        "warpFactor" => ir::TableValue::Warp(0),
        "warpFactor2" => ir::TableValue::Warp(1),
        _ => return None,
    })
}

/// The warp and shaper of each `$NI_WT_FORM_*` value, a script's form mode:
/// the KSP values are the kinds below (LINEAR 0, SATURATE 18, WRAP 27).
#[must_use]
pub fn warp_modes() -> Vec<Option<(WarpShape, FormShaper)>> {
    (0..=33).map(|k| shape(k).map(|s| (s, shaper(k)))).collect()
}

/// Kontakt's phase forms, numbered as its phase routine (`0x140567aa0`)
/// switches on them: one less than the saved form.
fn shape(kind: u32) -> Option<WarpShape> {
    let skew = |reach, side| WarpShape::Skew { reach, side };
    Some(match kind {
        0 => WarpShape::Off,
        1..=3 | 24..=26 => WarpShape::Sync,
        18..=23 | 27 => WarpShape::Off,
        4 => WarpShape::Cosine,
        5 => WarpShape::Arcsine,
        6 => WarpShape::CosineArcsine,
        7 => WarpShape::Pulse,
        8 => skew(0.9, Side::Minus),
        9 => skew(0.9, Side::Plus),
        10 => skew(0.9, Side::Both),
        11 => WarpShape::Flip,
        12 => WarpShape::Mirror,
        13 => WarpShape::Quantize,
        14 => skew(0.98, Side::Minus),
        15 => skew(0.98, Side::Plus),
        16 => skew(0.98, Side::Both),
        17 => WarpShape::Seesaw,
        28 => WarpShape::Exp,
        29 => WarpShape::Log,
        30 => WarpShape::LogExp,
        31 => WarpShape::Bend { side: Side::Minus },
        32 => WarpShape::Bend { side: Side::Plus },
        33 => WarpShape::Bend { side: Side::Both },
        _ => return None,
    })
}

/// What each form does to the level read, numbered as Kontakt's shaper
/// (`0x140ce6eb0`) switches on them, after both phase warps.
fn shaper(kind: u32) -> FormShaper {
    match kind {
        2 => FormShaper::Edges,
        3 => FormShaper::Window,
        18 => FormShaper::Saturate,
        19 => FormShaper::Fold,
        20 => FormShaper::Blinds(2),
        21 => FormShaper::Blinds(4),
        22 => FormShaper::Blinds(6),
        23 => FormShaper::Blinds(8),
        24 => FormShaper::Arch,
        25 => FormShaper::Peak,
        26 => FormShaper::Fall,
        27 => FormShaper::Wrap,
        _ => FormShaper::Off,
    }
}
