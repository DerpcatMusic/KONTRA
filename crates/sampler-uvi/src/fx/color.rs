//! UVI `color` effects onto `sampler_ir::fx::color`.
//!
//! Mapped (inserts.rs): `WaveShaper` rectifier modes 6/7 as a color rectifier; GarageFuzz,
//! SmoothOverdrive and StraightFuzz (with the HIIR oversampler) as the color chain's drive curve;
//! FormantCrusher as its biquad + decimator; DiodeClipper as its solver shaper; ToneStack (26 models); Exciter (Tape/Tube, HIIR).
//! Everything else in the family is reported here, with what is missing, never
//! mapped to the nearest curve.

/// Why a drive, lo-fi, amp, cabinet or utility element is not mapped.
pub(crate) fn unproved(element: &str) -> Option<&'static str> {
    Some(match element {
        "Redux" | "Redux2" => {
            "kernel at 0x1411e4de0 dispatches to 8 specialised bit/rate variants (0x1411e59f0..0x1411eb6d0) keyed by mode and flags; variant bodies and parameter conversion not transcribed"
        }
        "BigMuffTone" => {
            "tone filter at 0x141256270 is a runtime-built biquad from unreferenced double constants at 0x141e6e870..0x141e6e898; coefficients not transcribed"
        }
        "Drive" | "TSDrive"
        | "DoubleDrive" | "UviDrive" | "Buzz" | "Retro" | "Vinyl" => {
            "per-class kernel located but its saturation law, oversampling and filter stages are not transcribed; no Kontakt/UVI oracle to prove a transcription"
        }
        "JCMAmp" | "BoxModeler" | "Cabinet" => {
            "amp, tone-stack or cabinet topology kernel located but not transcribed; cabinet impulse data is not in the executable"
        }
        "Widener" => "kernel 0x1412cff90 located; stereo widening law not transcribed",
        _ => return None,
    })
}
