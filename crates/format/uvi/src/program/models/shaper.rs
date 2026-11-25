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

use super::{Attr, UviModule, ids, nothing, num};

/// WaveShaper: `[InputGain, Amount, Mix, OutputGain, Mode, PreFreq, PostFreq]`.
pub static WAVE_SHAPER: UviModule = UviModule {
    id: ids::WAVE_SHAPER,
    tag: "WaveShaper",
    attrs: &[
        Attr::num("InputGain", 0.0),
        Attr::num("Amount", 0.0),
        Attr::num("Mix", 1.0),
        Attr::num("OutputGain", 0.0),
        Attr::num("Mode", 0.0),
        Attr::num("PreFreq", 20_000.0),
        Attr::num("PostFreq", 20.0),
    ],
    report: wave_shaper_report,
    build_effect: |p, current| {
        let [input_db, amount, mix, output_db, mode, pre_hz, post_hz] = *p else {
            return None;
        };
        if mode.fract() != 0.0 || !(0.0..=3.0).contains(&mode) {
            return None;
        }
        let input = DB_GAIN.eval_unclamped(input_db);
        let out_gain = DB_GAIN.eval_unclamped(output_db);
        let d = WAVESHAPER_DRIVE.eval(amount);
        let (curve, scale) = match mode as u8 {
            0 => (ir::DriveCurve::Sine(d * FRAC_PI_2 * input), 1.0),
            1 => (ir::DriveCurve::Fold(d * FRAC_PI_2 * input), 1.0 / FRAC_PI_2),
            2 => (ir::DriveCurve::Tanh(d * input), 1.0 / (2.0 * d).tanh()),
            3 => (ir::DriveCurve::Atan(d * input), 1.0 / (2.0 * d).atan()),
            _ => return None,
        };
        let mix = mix.clamp(0.0, 1.0);
        // ponytail: UVI's post filter is a one-pole high-pass; the engine's is
        // a second-order Butterworth, within 1 dB above twice the corner.
        let shaper = ir::Shaper {
            pre_lowpass_hz: Some(pre_hz),
            curve,
            lowpass_hz: None,
            highpass_hz: Some(post_hz),
            sections: ir::Sections::NONE,
        };
        Some(ir::Effect {
            kind: ir::EffectKind::Shaper(shaper),
            bypass: current.bypass,
            output: mix * out_gain * scale,
            dry: 1.0 - mix,
        })
    },
};

/// DiodeClipper: `[OutputGain, Drive, Tone, HighPass, Asymmetry]`.
pub static DIODE_CLIPPER: UviModule = UviModule {
    id: ids::DIODE_CLIPPER,
    tag: "DiodeClipper",
    attrs: &[
        Attr::num("OutputGain", 0.0),
        Attr::num("Drive", 0.0),
        Attr::num("Tone", 20_000.0),
        Attr::num("HighPass", 1.0),
        Attr::num("Asymmetry", 0.0),
    ],
    report: nothing,
    build_effect: |p, current| {
        let [out_db, drive_db, tone_hz, coupling_hz, asymmetry] = *p else {
            return None;
        };
        let clipper = ir::Clipper {
            drive: DB_GAIN.eval_unclamped(drive_db),
            limit: 4.5,
            tone_hz,
            coupling_hz,
            saturation: 5.5462e-6,
            slope: 22.075_055,
            asymmetry: CLIPPER_ASYMMETRY.eval_unclamped(asymmetry),
            dc_hz: 1.0,
        };
        Some(ir::Effect {
            kind: ir::EffectKind::Clipper(clipper),
            bypass: current.bypass,
            output: DB_GAIN.eval_unclamped(out_db),
            dry: 0.0,
        })
    },
};

/// A mode not carried, or a `Knee`, is reported.
fn wave_shaper_report(node: Node, notes: &mut Notes) -> Result<(), String> {
    let mode = num(node, "Mode", 0.0)?;
    if mode.fract() != 0.0 || !(0.0..=3.0).contains(&mode) {
        notes.add(ir::Impact::Audible, "WaveShaper mode", mode);
    } else if num(node, "Knee", 0.0)? != 0.0 {
        notes.add(ir::Impact::Audible, "WaveShaper Knee", "");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::UviModule;
    use super::*;

    fn shaper(attrs: &str) -> Option<ir::Effect> {
        let xml = format!("<WaveShaper {attrs}/>");
        let doc = roxmltree::Document::parse(&xml).ok()?;
        UviModule::load(doc.root_element(), &mut Notes::default()).ok()?
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
