//! UVI's SparkVerb onto its own network, `ir::Network::Hadamard`. From
//! UVIWorkstation's SparkVerb engine (parameter ids 0x142805d88..de0, setters
//! 0x141320bb0..0x141321010, audio 0x141679c90):
//! - the lines' lengths are geometric from the room's crossing time,
//!   `RoomSize/340` s (0x14167f190, 1/340 at 0x141e88a40), spread by Shape;
//! - each pass loses `600·length/(DecayTime·RoomSize)` dB (0x14167b8c0,
//!   −600 at 0x14202c3a8), so the mid band falls 60 dB in
//!   `DecayTime·RoomSize/10` s, DecayLow and DecayHigh times that below
//!   FreqLow and above FreqHigh; LowCut and HiCut swap a band's decay for a
//!   first-order cut in the loop (0x141649410, 0x141649220);
//! - the lines swing by ModDepth cents at `ModRate/16` cycles per line length
//!   (0x14167d1a0, 2π·1200 at 0x14202c390): for a line one crossing long,
//!   the swing and rate below;
//! - Mix is linear, or equal-power when MixMode is 1; Rolloff is a one-pole
//!   on the output.

use kontra_format::Notes;
use kontra_ir as ir;
use kontra_law::common::EQUAL_POWER;
use roxmltree::Node;

use super::{flag, num};

const SPEED_OF_SOUND: f64 = 340.0;

/// The SparkVerb at `node`. Quality is log2 of the lines (+0x4c, flag 0x80
/// reallocates); Mode picks the lines' read interpolation (+0x50 → each
/// line's +0x48, 0x14167d8b0; 0x1416485a0: 0 none, 1 linear, 2 cubic), and
/// only linear is carried. Density and a Mode other than 1 are reported.
pub(super) fn spark_verb(node: Node, notes: &mut Notes) -> Result<ir::Effect, String> {
    let room = num(node, "RoomSize", 20.0)?.max(1.0);
    let seconds = num(node, "DecayTime", 1.0)?.max(0.01) * room / 10.0;
    let crossing = room / SPEED_OF_SOUND;
    let quality = num(node, "Quality", 3.0)?;
    if node.attribute("Density").is_some() || num(node, "Mode", 1.0)? != 1.0 {
        notes.add(ir::Impact::Audible, "sparkverb density or mode", "");
    }
    let hz = num(node, "ModRate", 1.0)?.max(0.0) / 16.0 / crossing;
    let swing_ms = if hz > 0.0 {
        let ratio = num(node, "ModDepth", 4.0)?.max(0.0) * std::f64::consts::LN_2 / 1200.0;
        1000.0 * ratio / (std::f64::consts::TAU * hz)
    } else {
        0.0
    };
    let mix = num(node, "Mix", 1.0)?.clamp(0.0, 1.0);
    let (wet, dry) = if num(node, "MixMode", 0.0)? == 1.0 {
        (EQUAL_POWER.eval_f64(mix), EQUAL_POWER.eval_f64(1.0 - mix))
    } else {
        (mix, 1.0 - mix)
    };
    let law = ir::HadamardNetwork {
        lines: 1 << (quality.clamp(1.0, 4.0) as u8),
        shape: num(node, "Shape", 0.0)? as f32,
        diffusers: flag(node, "DiffusionOnOff"),
        diffuser_ms: num(node, "DiffusionStart", 5.0)?.max(0.1) as f32,
        decay_low: num(node, "DecayLow", 1.0)? as f32,
        decay_high: num(node, "DecayHigh", 1.0)? as f32,
        low_hz: num(node, "FreqLow", 250.0)? as f32,
        high_hz: num(node, "FreqHigh", 12_000.0)? as f32,
        low_cut: flag(node, "LowCut"),
        high_cut: flag(node, "HiCut"),
        freeze: flag(node, "Freeze"),
    };
    Ok(ir::Effect {
        kind: ir::EffectKind::Reverb(ir::Reverb {
            seconds: seconds as f32,
            size: crossing as f32,
            network: Some(ir::Network::Hadamard(law)),
            damping: 0.0,
            diffusion: num(node, "Diffusion", 0.618)?.clamp(0.0, 1.0) as f32,
            predelay_ms: num(node, "PreDelay", 0.0)?.max(0.0) as f32,
            high_cut_hz: num(node, "Rolloff", 20_000.0)? as f32,
            low_shelf_db: 0.0,
            stereo: num(node, "Width", 1.0)?.clamp(0.0, 1.0) as f32,
            modulation_ms: swing_ms as f32,
            modulation_hz: hz as f32,
        }),
        bypass: false,
        output: wet as f32,
        dry: dry as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_corpus_setting_loads_its_decay_room_and_level() {
        // Augmented Orchestra's insert-3 SparkVerb (620 instances).
        let xml = r#"<SparkVerb ModDepth="5.9121876" ModRate="1" Diffusion="0.61799997"
            DiffusionStart="10" Width="1" RoomSize="20" DecayTime="2.2388823"
            DecayLow="0.16929972" DecayHigh="0.70764023" FreqLow="250" FreqHigh="12000"
            Shape="0" Mix="1" Quality="3" Mode="1" HiCut="1" LowCut="0" Rolloff="20000"
            DiffusionOnOff="1" PreDelay="0" MixMode="1" SparkVerbVersion="1"/>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let mut notes = Notes::default();
        let e = spark_verb(doc.root_element(), &mut notes).unwrap();
        assert!(notes.finish().is_empty());
        let ir::EffectKind::Reverb(r) = e.kind else {
            panic!("{e:?}")
        };
        let near = |a: f32, b: f32, tolerance: f32| (a - b).abs() < tolerance;
        // 2.24 × 20 m / 10: 4.5 s; a 20 m room's 59 ms crossing.
        assert!(near(r.seconds, 4.478, 0.01), "{}", r.seconds);
        assert!(near(r.size, 0.0588, 0.0001), "{}", r.size);
        let Some(ir::Network::Hadamard(law)) = r.network else {
            panic!("{r:?}")
        };
        assert!(law.high_cut && !law.low_cut && law.diffusers, "{law:?}");
        assert!(near(law.decay_low, 0.169, 0.001), "{law:?}");
        assert_eq!((law.lines, law.diffuser_ms), (8, 10.0));
        // 5.9 cents at 1.06 Hz: a 0.51 ms swing.
        assert!(near(r.modulation_hz, 1.0625, 0.001), "{}", r.modulation_hz);
        assert!(near(r.modulation_ms, 0.511, 0.005), "{}", r.modulation_ms);
        assert_eq!(e.output, 1.0);
        assert_eq!(e.dry, 0.0);
    }

    #[test]
    fn quality_four_runs_sixteen_lines_into_its_rolloff() {
        // Augmented Orchestra's insert-0 SparkVerb (620 instances).
        let xml = r#"<SparkVerb ModDepth="0.60000002" ModRate="1" Diffusion="0.61799997"
            DiffusionStart="5" Width="1" RoomSize="20" DecayTime="0.63095725"
            DecayLow="3.1622777" DecayHigh="3.1622777" FreqLow="250" FreqHigh="11780.971"
            Shape="0" Mix="1" Quality="4" Mode="1" HiCut="1" LowCut="0" Rolloff="6871.1567"
            DiffusionOnOff="1" PreDelay="0" MixMode="0" SparkVerbVersion="1"/>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let mut notes = Notes::default();
        let e = spark_verb(doc.root_element(), &mut notes).unwrap();
        assert!(notes.finish().is_empty());
        let ir::EffectKind::Reverb(r) = e.kind else {
            panic!("{e:?}")
        };
        let Some(ir::Network::Hadamard(law)) = r.network else {
            panic!("{r:?}")
        };
        assert_eq!(law.lines, 16);
        assert!((r.high_cut_hz - 6871.16).abs() < 0.01, "{}", r.high_cut_hz);
    }
}
