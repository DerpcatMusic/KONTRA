//! UVI's WaveShaper onto `ir::Shaper` and its DiodeClipper onto
//! `ir::Clipper`. The laws are UVIWorkstation's
//! WaveShaper (parameters registered by 0x14126e2b0, the audio method
//! 0x141271fd0 behind vtable 0x141e71108):
//! - the wet path takes `InputGain` (dB), a one-pole low-pass at `PreFreq`
//!   (pole `e^(−2π·f/rate)`, 0x141641550), the curve, then a one-pole
//!   high-pass at `PostFreq` (`x − lowpass(x)`);
//! - the drive is `d = 2^(8·Amount)` (0x141cfab88, 0x141cf0408);
//! - `Mode` picks the curve: 0 `sin(d·π/2·x)`, 1 `asin(sin(d·π/2·x))/(π/2)`,
//!   2 `tanh(d·x)/tanh(2d)`, 3 `atan(d·x)/atan(2d)` (12 modes, the
//!   rest unused in the corpus);
//! - `out = (1 − Mix)·input + Mix·OutputGain·wet`.
//!
//! `Knee` (`10^(Knee/10)`, a power on `|x|` around the curve) is 0 in every
//! corpus program and reported when set. UVI runs the curve at `2^Oversampling`
//! times the rate; the engine always runs it at twice.
//!
//! The DiodeClipper (audio method 0x1412584e0, the circuit 0x141258040,
//! constants at 0x141e6ec40..68): `Drive` dB into a 4.5 V `tanh` limit, a
//! `HighPass` Hz coupling capacitor into a `Tone` Hz low-pass shunted by a
//! diode pair (`5.5462e-6`, `22.075`/V, the reverse side `10^Asymmetry`
//! steeper), a 1 Hz DC blocker (0x141257ee0), then `OutputGain` dB.

use std::f32::consts::FRAC_PI_2;

use kontra_format::Notes;
use kontra_ir as ir;
use kontra_law::common::DB_GAIN;
use kontra_law::uvi::{CLIPPER_ASYMMETRY, WAVESHAPER_DRIVE};
use roxmltree::Node;

use super::{Params, num};

/// The WaveShaper at `node`; `None` (reported) for a mode not carried.
pub(super) fn wave_shaper(p: Params, notes: &mut Notes) -> Result<Option<ir::Effect>, String> {
    let gain = |name| Ok::<_, String>(gain_of(p.num(name, 0.0)?));
    let input = gain("InputGain")?;
    let d = WAVESHAPER_DRIVE.eval(p.num("Amount", 0.0)? as f32);
    let mode = p.num("Mode", 0.0)?;
    let (curve, scale) = match mode as u8 {
        _ if mode.fract() != 0.0 => return unsupported(notes, mode),
        0 => (ir::DriveCurve::Sine(d * FRAC_PI_2 * input), 1.0),
        1 => (ir::DriveCurve::Fold(d * FRAC_PI_2 * input), 1.0 / FRAC_PI_2),
        2 => (ir::DriveCurve::Tanh(d * input), 1.0 / (2.0 * d).tanh()),
        3 => (ir::DriveCurve::Atan(d * input), 1.0 / (2.0 * d).atan()),
        _ => return unsupported(notes, mode),
    };
    if p.num("Knee", 0.0)? != 0.0 {
        notes.add(ir::Impact::Audible, "WaveShaper Knee", "");
    }
    let mix = p.num("Mix", 1.0)?.clamp(0.0, 1.0) as f32;
    // ponytail: UVI's post filter is a one-pole high-pass; the engine's is
    // a second-order Butterworth, within 1 dB above twice the corner.
    let shaper = ir::Shaper {
        pre_lowpass_hz: Some(p.num("PreFreq", 20_000.0)? as f32),
        curve,
        lowpass_hz: None,
        highpass_hz: Some(p.num("PostFreq", 20.0)? as f32),
    };
    Ok(Some(ir::Effect {
        kind: ir::EffectKind::Shaper(shaper),
        bypass: false,
        output: mix * gain("OutputGain")? * scale,
        dry: 1.0 - mix,
    }))
}

/// The DiodeClipper at `node`.
pub(super) fn diode_clipper(node: Node) -> Result<ir::Effect, String> {
    let clipper = ir::Clipper {
        drive: gain_of(num(node, "Drive", 0.0)?),
        limit: 4.5,
        tone_hz: num(node, "Tone", 20_000.0)? as f32,
        coupling_hz: num(node, "HighPass", 1.0)? as f32,
        saturation: 5.5462e-6,
        slope: 22.075_055,
        asymmetry: CLIPPER_ASYMMETRY.eval_unclamped(num(node, "Asymmetry", 0.0)? as f32),
        dc_hz: 1.0,
    };
    Ok(ir::Effect {
        kind: ir::EffectKind::Clipper(clipper),
        bypass: false,
        output: gain_of(num(node, "OutputGain", 0.0)?),
        dry: 0.0,
    })
}

/// Linear gain of `dB`.
fn gain_of(db: f64) -> f32 {
    DB_GAIN.eval_unclamped(db as f32)
}

fn unsupported(notes: &mut Notes, mode: f64) -> Result<Option<ir::Effect>, String> {
    notes.add(ir::Impact::Audible, "WaveShaper mode", mode);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shaper(attrs: &str) -> Option<ir::Effect> {
        let xml = format!("<WaveShaper {attrs}/>");
        let doc = roxmltree::Document::parse(&xml).ok()?;
        wave_shaper(Params::of(doc.root_element()), &mut Notes::default()).ok()?
    }

    #[test]
    fn tanh_is_normalized_at_twice_full_scale_and_mixed_with_the_input() {
        let Some(e) =
            shaper(r#"Amount="0.25" Mode="2" Mix="0.5" OutputGain="-6.0206" InputGain="0""#)
        else {
            panic!("not carried");
        };
        let ir::EffectKind::Shaper(s) = e.kind else {
            panic!("not a shaper");
        };
        assert_eq!(s.curve, ir::DriveCurve::Tanh(4.0));
        assert!((e.output - 0.25 / 8f32.tanh()).abs() < 1e-4, "{}", e.output);
        assert_eq!(e.dry, 0.5);
        assert_eq!(s.pre_lowpass_hz, Some(20_000.0));
        assert_eq!(s.highpass_hz, Some(20.0));
    }

    #[test]
    fn a_mode_not_carried_passes_its_input() {
        assert!(shaper(r#"Mode="7""#).is_none());
    }
}
