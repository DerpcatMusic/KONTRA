//! A Kontakt wavetable group (source mode 9) as the model's wavetable.

use kontra_format::Notes;
use kontra_ir::{self as ir, Side, Warp, WarpShape};
use ni_file::kontakt::objects::WavetableSource;

/// The source's table and warps. Its modulation oscillator, inharmonic
/// partials and random start phase are not played yet: each leaves a note.
pub(crate) fn wavetable(source: &WavetableSource, group: &str, notes: &mut Notes) -> ir::Wavetable {
    let mut warp = |form: u32, amount: f32| {
        let shape = form.checked_sub(1).and_then(shape);
        if shape.is_none() {
            notes.add(
                ir::Impact::Audible,
                "wavetable phase form",
                format!("{group}: {form}"),
            );
        }
        Warp {
            shape: shape.unwrap_or(WarpShape::Off),
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
        (source.phase_random != 0.0, "wavetable random phase"),
    ] {
        if on {
            notes.add(ir::Impact::Audible, feature, group);
        }
    }
    ir::Wavetable {
        position: source.position,
        phase: source.phase,
        warps,
    }
}

/// Kontakt's phase forms, numbered as its phase routine (`0x140567aa0`)
/// switches on them: one less than the saved form.
fn shape(kind: u32) -> Option<WarpShape> {
    let skew = |reach, side| WarpShape::Skew { reach, side };
    Some(match kind {
        0 => WarpShape::Off,
        1..=3 | 24..=26 => WarpShape::Sync,
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
