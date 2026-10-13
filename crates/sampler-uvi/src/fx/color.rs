//! UVI `color` effects onto `sampler_ir::fx::color`.
//!
//! Mapped (inserts.rs): `WaveShaper` rectifier modes 6/7 as a color rectifier.
//! Everything else in the family is reported here, with what is missing, never
//! mapped to the nearest curve.

/// Why a drive, lo-fi, amp, cabinet or utility element is not mapped.
pub(crate) fn unproved(element: &str) -> Option<&'static str> {
    Some(match element {
        "Redux" | "Redux2" | "FormantCrusher" => {
            "crusher/decimator public parameter conversion is not established"
        }
        "Drive" | "Exciter" | "GarageFuzz" | "StraightFuzz" | "SmoothOverdrive" | "TSDrive"
        | "DiodeClipper" | "DoubleDrive" | "UviDrive" | "Maximizer" | "Buzz" | "Retro"
        | "Vinyl" => "saturation law is not extracted from the binary",
        "BigMuffTone" | "ToneStack" | "JCMAmp" | "BoxModeler" | "Cabinet" => {
            "amp, tone-stack or cabinet topology is not extracted from the binary"
        }
        "Widener" => "stereo widening law is not extracted from the binary",
        _ => return None,
    })
}
