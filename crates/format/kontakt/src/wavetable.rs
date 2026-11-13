//! A Kontakt wavetable group (source mode 9) as the model's wavetable.

use kontra_format::Notes;
use kontra_ir::{self as ir, FormShaper, Side, Warp, WarpShape};
use kontra_law::kontakt::{INHARMONIC_RANGE, MOD_FM_HZ, MOD_TUNE_RANGE};
use ni_file::kontakt::objects::WavetableSource;

/// The source's table, forms, start phase, inharmonic stretch and mod
/// oscillator. The oscillator plays a sine whatever its saved wave.
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
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId("kontakt.wavetable.v1"),
        location: format!("groups/{group}/wavetable"),
        input: format!(
            "mod type={} wave={} amount={} tune={}, inharmonic={}:{}",
            source.mod_type,
            source.mod_wave,
            source.mod_amount,
            source.mod_tune,
            source.inharmonic_enabled,
            source.inharmonic
        ),
        output: String::new(),
    });
    // The reader's names are swapped: `mod_wave` holds the type and
    // `mod_type` the wave (probes saved with type FM1/FM2 and wave sine read
    // wave=1/2, type=0); the tune is in octaves (+5 st reads 0.41666).
    let kind = mod_kind(source.mod_wave);
    if kind.is_none() {
        notes.add(ir::Impact::Audible, "wavetable modulation", group);
    }
    ir::Wavetable {
        position: source.position,
        phase: source.phase,
        phase_random: source.phase_random.clamp(0.0, 1.0),
        gain: LEVEL,
        inharmonic: ir::Inharmonic {
            on: source.inharmonic_enabled,
            amount: source.inharmonic.clamp(0.0, 1.0),
            range: INHARMONIC_RANGE,
        },
        modosc: ir::ModOsc {
            kind: kind.unwrap_or(ir::ModOscKind::Off),
            amount: source.mod_amount.clamp(0.0, 1.0),
            tune: ((12.0 * source.mod_tune + 36.0) / 72.0).clamp(0.0, 1.0),
            tune_range: MOD_TUNE_RANGE,
            fm_hz: MOD_FM_HZ,
        },
        warps,
    }
}

/// Kontakt plays a wavetable group 6.9 dB below the same cycles in sampler
/// mode, whatever their level: measured on an oracle probe, a 0.5 saw and a
/// 0.25 sine, each −6.9 dB against ours at unity.
const LEVEL: f32 = 0.45;

/// The wavetable value a modulation target moves: Kontakt adds the routes
/// to it in its 0..=1 units and clamps the sum every 32 frames
/// (`fminf`/`fmaxf` at 0x140b52133..0x140b521b4, targets +0x10260/4/74).
pub(crate) fn target(param: &str) -> Option<ir::TableValue> {
    Some(match param {
        "wavetablePosition" => ir::TableValue::Position,
        "warpFactor" => ir::TableValue::Warp(0),
        "warpFactor2" => ir::TableValue::Warp(1),
        "wavetableInharmonic" => ir::TableValue::Inharmonic,
        "wavetableModAmount" => ir::TableValue::ModAmount,
        "wavetableModFrequency" => ir::TableValue::ModTune,
        _ => return None,
    })
}

/// The mod oscillator kind of each `$NI_WT_MOD_TYPE_*` value (OFF 0, FM1 1,
/// FM2 2, FM3 3, RM 4, MIX 5, PM1 6, PM2 7, PM3 8, BLEND 9), a script's
/// `$ENGINE_PAR_WT_MOD_TYPE` and the saved type alike. Every type but BLEND
/// (a guess) renders as Kontakt does at amount ¼ on a sine probe
/// (scratch/laneDSP/b8 mo2_case*, partials within 1 dB; FM3's pitch pull
/// 303 Hz vs 307): FM1 moves the table's frequency, FM2 sounds the
/// oscillator moved by the table, FM3 and PM3 feed the table back, PM1 and
/// PM2 (without forms) move its phase, RM scales by 1 − A.
#[must_use]
pub fn mod_kinds() -> Vec<Option<ir::ModOscKind>> {
    (0..10).map(mod_kind).collect()
}

fn mod_kind(kind: u32) -> Option<ir::ModOscKind> {
    use ir::ModOscKind as K;
    Some(match kind {
        0 => K::Off,
        1 => K::Fm,
        2 => K::FmBy,
        3 => K::FmFeedback,
        4 => K::Ring,
        5 => K::Mix,
        6 | 7 => K::Pm,
        8 => K::PmFeedback,
        9 => K::Blend,
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
