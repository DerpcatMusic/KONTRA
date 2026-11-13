//! Inserts the program's buses and chains run that are not a parallel path:
//! an `XpanderFilter` on the explicit ladder (the keygroup's filter), a
//! `Drive`, a `CombFilter`, and the wet share of a `FeedbackMachine`
//! (its chain is laid like an effect rack's; see `Translator::lay`).
//!
//! ponytail, unverified (no UVI render oracle): `Drive` takes the WaveShaper's
//! drive law and `tanh`, normalised to unity at full scale; `CombFilter` is
//! one feedback comb at `Freq` Hz with `Q` as its loop gain (a one-voice
//! [`ir::CombBank`]); a `FeedbackMachine`'s own delay (milliseconds in every
//! corpus program) is dropped, and its loop is reported when `Feedback` is
//! above 0.

use kontra_format::Notes;
use kontra_ir as ir;
use kontra_law::uvi::WAVESHAPER_DRIVE;
use roxmltree::Node;

use super::super::{Library, Translator, filters, num};
use super::effect;

/// The line a comb at the lowest `Freq` needs, Hz.
const COMB_FLOOR_HZ: f32 = 20.0;
/// The most a comb's loop gain reaches.
const COMB_FEEDBACK_MAX: f32 = 0.999;

/// The bus insert at `node` this module carries; `None` for any other.
pub(super) fn bus_effect(node: Node, notes: &mut Notes) -> Result<Option<ir::Effect>, String> {
    Ok(if filters::on_bus(node) {
        Some(filters::effect(node, 0))
    } else if node.has_tag_name("Drive") {
        drive(node, notes)?
    } else if node.has_tag_name("CombFilter") {
        Some(comb(node, notes)?)
    } else {
        None
    })
}

fn drive(node: Node, notes: &mut Notes) -> Result<Option<ir::Effect>, String> {
    if num(node, "Mode", 0.0)? != 0.0 {
        notes.add(ir::Impact::Audible, "Drive mode", "");
        return Ok(None);
    }
    let d = WAVESHAPER_DRIVE.eval(num(node, "DriveAmount", 0.0)?.clamp(0.0, 1.0) as f32);
    let shaper = ir::Shaper {
        pre_lowpass_hz: None,
        curve: ir::DriveCurve::Tanh(d),
        lowpass_hz: None,
        highpass_hz: None,
        sections: ir::Sections::NONE,
    };
    // Unity at full scale, so a hard drive adds colour, not level.
    let scale = 1.0 / d.tanh().max(f32::EPSILON);
    Ok(Some(effect(ir::EffectKind::Shaper(shaper), scale, 0.0)))
}

fn comb(node: Node, notes: &mut Notes) -> Result<ir::Effect, String> {
    if num(node, "Mode", 0.0)? != 0.0 {
        notes.add(ir::Impact::Audible, "CombFilter mode", "");
    }
    if node.attribute("KeyTracking").is_some_and(|k| k != "0") {
        notes.add(ir::Impact::Audible, "CombFilter key tracking", "");
    }
    let root_hz = (num(node, "Freq", 440.0)? as f32).max(COMB_FLOOR_HZ);
    let bank = ir::CombBank {
        root_hz,
        ratio: [1.0; 4],
        detune: [1.0; 4],
        lowest_hz: COMB_FLOOR_HZ,
        feedback: (num(node, "Q", 0.0)? as f32).clamp(0.0, COMB_FEEDBACK_MAX),
        voices: 0.0,
        damping: 0.0,
        depth: 0.0,
        rate: ir::Rate::Hz(0.1),
        offset: 0.0,
        polarity: ir::Polarity::Normal,
        mode: ir::CombMode::Standard,
        scan_depth: 0.0,
        scan_shape: ir::ScanShape::Triangle,
        width: 0.0,
        mix: 1.0,
    };
    Ok(effect(ir::EffectKind::CombBank(bank), 1.0, 0.0))
}

impl<L: Library> Translator<'_, '_, L> {
    /// A FeedbackMachine's wet share (`Mix`); its loop is reported.
    pub(super) fn feedback_machine(&mut self, node: Node) -> Result<f64, String> {
        if num(node, "Feedback", 0.0)? != 0.0 {
            self.notes
                .add(ir::Impact::Audible, "FeedbackMachine feedback", "");
        }
        Ok(num(node, "Mix", 1.0)?.clamp(0.0, 1.0))
    }
}
