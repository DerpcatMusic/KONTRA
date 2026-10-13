//! UVI `Arpeggiator` onto the pattern-arpeggiator block
//! (`sampler_core::blocks::pattern_arp`). Only the binary-proved bindings of
//! UVI Workstation 4.0.9 are mapped (registry 0x14110c900, constructor
//! 0x14110e690, scheduler 0x141110c90); every open item is reported and the
//! arpeggiator is withheld rather than approximated. Attribute defaults are the
//! public catalogue's (`parameters.tsv`), except the traced native defaults
//! Resolution 22, StepCCId -1 and StepLength 1 (doc-only defaults are inlined).
//!
//! The IR has no event-processor slot yet, so this returns the block
//! configuration for the engine to host; nothing here is DSP.

use crate::{number, path};
use roxmltree::Node;
use sampler_core::blocks::pattern_arp::{PATTERN_STEPS, PatternArp, PatternStep, PatternTrigger};
use sampler_ir as ir;

/// `Resolution` index to quarter notes (selector 0x140dc84e0, identical switch
/// in 0x141110c90). The native values are float32.
const RESOLUTION: [(f64, f64); 30] = [
    (128.0, 1.0),
    (64.0, 1.0),
    (32.0, 1.0),
    (28.0, 1.0),
    (24.0, 1.0),
    (20.0, 1.0),
    (16.0, 1.0),
    (12.0, 1.0),
    (8.0, 1.0),
    (6.0, 1.0),
    (4.0, 1.0),
    (3.0, 1.0),
    (8.0, 3.0),
    (2.0, 1.0),
    (3.0, 2.0),
    (4.0, 3.0),
    (1.0, 1.0),
    (3.0, 4.0),
    (2.0, 3.0),
    (1.0, 2.0),
    (3.0, 8.0),
    (1.0, 3.0),
    (1.0, 4.0),
    (3.0, 16.0),
    (1.0, 6.0),
    (1.0, 8.0),
    (3.0, 32.0),
    (1.0, 12.0),
    (1.0, 16.0),
    (1.0, 24.0),
];

pub struct Arpeggiator {
    /// `None` when an open item makes the native behavior unknowable.
    pub arp: Option<PatternArp>,
    pub unsupported: Vec<ir::Unsupported>,
}

/// Quarter-note length of a `Resolution` index, `None` outside 0..=29 (the
/// native selector yields zero there).
pub fn resolution_beats(index: i64) -> Option<f64> {
    let (n, d) = *usize::try_from(index).ok().and_then(|i| RESOLUTION.get(i))?;
    Some(f64::from((n / d) as f32))
}

pub fn pattern_arp(node: Node) -> Result<Arpeggiator, String> {
    let get = |name: &str, default: f64| number(node, name, default);
    let mut unsupported = Vec::new();
    let mut open = |feature: &str, value: String, reason: ir::Reason| {
        unsupported.push(ir::Unsupported { location: path(node), feature: feature.into(), value, reason });
    };
    let mut withheld = false;
    let mode = get("Mode", 0.0)?;
    if matches!(mode as i64, 24 | 26 | 27) || !(0.0..=27.0).contains(&mode) || mode.fract() != 0.0 {
        let why = match mode as i64 {
            24 | 26 => "sort key of the (value, index) list unrecovered",
            27 => "weighted selector draw distribution unrecovered",
            _ => "outside the traced modes 0..=27",
        };
        open("Arpeggiator Mode", format!("{mode} ({why})"), ir::Reason::NotModeled);
        withheld = true;
    }
    let trigger = match get("TriggerMode", 1.0)? as i64 {
        0 => PatternTrigger::Note,
        1 => PatternTrigger::Legato,
        other => {
            open(
                "Arpeggiator TriggerMode",
                format!("{other} (Song Position: transport boundary conversion open)"),
                ir::Reason::NotModeled,
            );
            withheld = true;
            PatternTrigger::Legato
        }
    };
    if get("Bypass", 0.0)? != 0.0 {
        let why = "stored Bypass to the inherited predicate and the pass-through law untraced";
        open("Arpeggiator Bypass", why.into(), ir::Reason::NotModeled);
        withheld = true;
    }
    if let Some(version) = node.attribute("ArpeggiatorVersion") {
        let why = "version to migration-argument binding untraced (Resolution remap, OctaveMode)";
        open("Arpeggiator ArpeggiatorVersion", format!("{version} ({why})"), ir::Reason::NotModeled);
        withheld = true;
    }
    if get("Record", 0.0)? != 0.0 {
        open("Arpeggiator Record", "recording cursor and reference-key policy open".into(), ir::Reason::NotModeled);
    }
    let resolution = get("Resolution", 22.0)?;
    let beats = if resolution.fract() == 0.0 { resolution_beats(resolution as i64) } else { None };
    if beats.is_none() {
        open("Arpeggiator Resolution", resolution.to_string(), ir::Reason::InvalidValue);
        withheld = true;
    }
    let num_steps = get("NumSteps", 16.0)?;
    if !(1.0..=PATTERN_STEPS as f64).contains(&num_steps) || num_steps.fract() != 0.0 {
        open("Arpeggiator NumSteps", num_steps.to_string(), ir::Reason::InvalidValue);
        withheld = true;
    }
    let mut steps = [PatternStep::default(); PATTERN_STEPS];
    for (k, step) in steps.iter_mut().enumerate() {
        let state = get(&format!("Step{k}State"), 0.0)?;
        if !(0.0..=3.0).contains(&state) || state.fract() != 0.0 {
            open("Arpeggiator StepState", format!("Step{k}State={state}"), ir::Reason::InvalidValue);
            withheld = true;
        }
        *step = PatternStep {
            state: state as u8,
            size: get(&format!("Step{k}Size"), 1.0)? as f32,
            level: get(&format!("Step{k}Level"), 1.0)? as f32,
            offset: get(&format!("Step{k}Offset"), 0.0)? as i32,
            cc: get(&format!("Step{k}CC"), 0.0)? as i32,
        };
    }
    let arp = PatternArp {
        mode: mode as u8,
        octave: get("Octave", 0.0)? as i32,
        octave_mode: get("OctaveMode", 1.0)? as u8,
        num_strike: get("NumStrike", 1.0)? as u32,
        repeat_bottom: get("RepeatBottomNote", 0.0)? != 0.0,
        repeat_top: get("RepeatTopNote", 0.0)? != 0.0,
        resolution: beats.unwrap_or(0.0),
        step_length: get("StepLength", 1.0)? as f32,
        groove: get("GrooveAmount", 0.0)?,
        velocity_blend: get("ArpVelocityBlend", 0.5)? as f32,
        num_steps: num_steps as u32,
        step_cc_id: get("StepCCId", -1.0)? as i32,
        trigger,
        hold: get("Hold", 0.0)? != 0.0,
        bypass: false,
        steps,
    };
    if !arp.valid() {
        withheld = true;
        open("Arpeggiator", "attribute outside the block's domain".into(), ir::Reason::InvalidValue);
    }
    Ok(Arpeggiator { arp: (!withheld).then_some(arp), unsupported })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arp(attrs: &str) -> Arpeggiator {
        let xml = format!(r#"<Arpeggiator Name="A" {attrs}/>"#);
        let doc = roxmltree::Document::parse(&xml).unwrap();
        pattern_arp(doc.root_element()).unwrap()
    }

    /// Selector 0x140dc84e0: index to quarter notes; default 22 is a sixteenth.
    #[test]
    fn resolution_index_selects_the_traced_quarter_note_length() {
        assert_eq!(resolution_beats(0), Some(128.0));
        assert_eq!(resolution_beats(16), Some(1.0));
        assert_eq!(resolution_beats(22), Some(0.25));
        assert_eq!(resolution_beats(12), Some((8.0f32 / 3.0) as f64));
        assert_eq!(resolution_beats(29), Some((1.0f32 / 24.0) as f64));
        assert_eq!(resolution_beats(30), None);
        assert_eq!(arp("").arp.unwrap().resolution, 0.25);
    }

    /// Named attributes land on the block fields (registry 0x14110c900).
    #[test]
    fn named_attributes_configure_the_block() {
        let a = arp(
            r#"Mode="12" Octave="-2" OctaveMode="0" NumStrike="3" RepeatBottomNote="1" RepeatTopNote="1"
               Resolution="19" StepLength="0.5" ArpVelocityBlend="1" GrooveAmount="0.4" NumSteps="4"
               StepCCId="74" TriggerMode="0" Step2State="3" Step2Size="0.25" Step2Level="0.5"
               Step2Offset="-7" Step2CC="21""#,
        );
        assert!(a.unsupported.is_empty(), "{:?}", a.unsupported);
        let a = a.arp.unwrap();
        assert_eq!((a.mode, a.octave, a.octave_mode, a.num_strike), (12, -2, 0, 3));
        assert!(a.repeat_bottom && a.repeat_top);
        assert_eq!((a.resolution, a.step_length, a.velocity_blend), (0.5, 0.5, 1.0));
        assert_eq!((a.groove, a.num_steps, a.step_cc_id, a.trigger), (0.4, 4, 74, PatternTrigger::Note));
        assert_eq!(
            a.steps[2],
            PatternStep { state: 3, size: 0.25, level: 0.5, offset: -7, cc: 21 }
        );
        assert_eq!(a.steps[3], PatternStep::default());
    }

    /// Open items withhold the arpeggiator with a precise report instead of a guess.
    #[test]
    fn open_modes_and_joins_are_reported_and_withheld() {
        for (attrs, feature) in [
            (r#"Mode="24""#, "Arpeggiator Mode"),
            (r#"Mode="26""#, "Arpeggiator Mode"),
            (r#"Mode="27""#, "Arpeggiator Mode"),
            (r#"TriggerMode="2""#, "Arpeggiator TriggerMode"),
            (r#"Bypass="1""#, "Arpeggiator Bypass"),
            (r#"ArpeggiatorVersion="0""#, "Arpeggiator ArpeggiatorVersion"),
            (r#"Resolution="30""#, "Arpeggiator Resolution"),
        ] {
            let a = arp(attrs);
            assert!(a.arp.is_none(), "{attrs}");
            assert!(a.unsupported.iter().any(|u| u.feature == feature), "{attrs}: {:?}", a.unsupported);
        }
    }
}
