//! UVI `color` effects onto `sampler_ir::fx::color`.
//!
//! Mapped (inserts.rs): `WaveShaper` rectifier modes 6/7 as a color rectifier; GarageFuzz,
//! SmoothOverdrive and StraightFuzz (with the HIIR oversampler) as the color chain's drive curve.
//! Everything else in the family is reported here, with what is missing, never
//! mapped to the nearest curve.

/// Why a drive, lo-fi, amp, cabinet or utility element is not mapped.
pub(crate) fn unproved(element: &str) -> Option<&'static str> {
    Some(match element {
        "Redux" | "Redux2" => {
            "kernel at [ref] dispatches to 8 specialised bit/rate variants ([ref]..[ref]) keyed by mode and flags; variant bodies and parameter conversion not transcribed"
        }
        "FormantCrusher" => {
            "kernel at [ref] chains a biquad bank and a 0x1038-byte per-channel formant stage; stage laws not transcribed"
        }
        "Exciter" => "kernel [ref] runs an oversampled (up to 2^n) stage via [ref] with an FIR resampler; shaper laws not transcribed",
        "BigMuffTone" => {
            "tone filter at [ref] is a runtime-built biquad from unreferenced double constants at [ref]..[ref]; coefficients not transcribed"
        }
        "Drive" | "TSDrive" | "DiodeClipper"
        | "DoubleDrive" | "UviDrive" | "Buzz" | "Retro" | "Vinyl" => {
            "per-class kernel located but its saturation law, oversampling and filter stages are not transcribed; no Kontakt/UVI oracle to prove a transcription"
        }
        "ToneStack" | "JCMAmp" | "BoxModeler" | "Cabinet" => {
            "amp, tone-stack or cabinet topology kernel located but not transcribed; cabinet impulse data is not in the executable"
        }
        "Widener" => "kernel [ref] located; stereo widening law not transcribed",
        _ => return None,
    })
}
