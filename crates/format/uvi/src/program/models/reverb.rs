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
use kontra_law::common::SQUARE_ROOT;
use roxmltree::Node;

use super::{Attr, SPEED_OF_SOUND, UviModule, ids, num};

/// SparkVerb: the 21 parameters above, in knob order.
pub static SPARK_VERB: UviModule = UviModule {
    id: ids::SPARK_VERB,
    tag: "SparkVerb",
    attrs: &[
        Attr::num("RoomSize", 20.0),
        Attr::num("DecayTime", 1.0),
        Attr::num("PreDelay", 0.0),
        Attr::num("Rolloff", 20_000.0),
        Attr::num("Width", 1.0),
        Attr::num("Mix", 1.0),
        Attr::num("MixMode", 0.0),
        Attr::num("Diffusion", 0.618),
        Attr::num("ModRate", 1.0),
        Attr::num("ModDepth", 4.0),
        Attr::num("Quality", 3.0),
        Attr::num("Shape", 0.0),
        Attr::flag("DiffusionOnOff"),
        Attr::num("DiffusionStart", 5.0),
        Attr::num("DecayLow", 1.0),
        Attr::num("DecayHigh", 1.0),
        Attr::num("FreqLow", 250.0),
        Attr::num("FreqHigh", 12_000.0),
        Attr::flag("LowCut"),
        Attr::flag("HiCut"),
        Attr::flag("Freeze"),
    ],
    report: spark_verb_report,
    build_effect: |p, current| {
        let [
            room,
            decay,
            predelay,
            rolloff,
            width,
            mix,
            mix_mode,
            diffusion,
            mod_rate,
            mod_depth,
            quality,
            shape,
            diff_on,
            diff_start,
            decay_low,
            decay_high,
            freq_low,
            freq_high,
            low_cut,
            hi_cut,
            freeze,
        ] = *p
        else {
            return None;
        };
        let room = f64::from(room).max(1.0);
        let seconds = f64::from(decay).max(0.01) * room / 10.0;
        let crossing = room / SPEED_OF_SOUND;
        let hz = f64::from(mod_rate).max(0.0) / 16.0 / crossing;
        let swing_ms = if hz > 0.0 {
            let ratio = f64::from(mod_depth).max(0.0) * std::f64::consts::LN_2 / 1200.0;
            1000.0 * ratio / (std::f64::consts::TAU * hz)
        } else {
            0.0
        };
        let mix = f64::from(mix).clamp(0.0, 1.0);
        let (wet, dry) = if (mix_mode - 1.0).abs() < 0.5 {
            (SQUARE_ROOT.eval_f64(mix), SQUARE_ROOT.eval_f64(1.0 - mix))
        } else {
            (mix, 1.0 - mix)
        };
        let law = ir::HadamardNetwork {
            lines: 1 << (f64::from(quality).clamp(1.0, 4.0) as u8),
            shape,
            diffusers: diff_on > 0.5,
            diffuser_ms: diff_start.max(0.1),
            decay_low,
            decay_high,
            low_hz: freq_low,
            high_hz: freq_high,
            low_cut: low_cut > 0.5,
            high_cut: hi_cut > 0.5,
            freeze: freeze > 0.5,
        };
        Some(ir::Effect {
            kind: ir::EffectKind::Reverb(ir::Reverb {
                seconds: seconds as f32,
                size: crossing as f32,
                network: Some(ir::Network::Hadamard(law)),
                damping: 0.0,
                diffusion: diffusion.clamp(0.0, 1.0),
                predelay_ms: predelay.max(0.0),
                high_cut_hz: rolloff,
                low_shelf_db: 0.0,
                stereo: width.clamp(0.0, 1.0),
                modulation_ms: swing_ms as f32,
                modulation_hz: hz as f32,
            }),
            bypass: current.bypass,
            output: wet as f32,
            dry: dry as f32,
        })
    },
};

/// Quality is log2 of the lines (+0x4c, flag 0x80 reallocates); Mode picks
/// the lines' read interpolation (+0x50 → each line's +0x48, 0x14167d8b0;
/// 0x1416485a0: 0 none, 1 linear, 2 cubic), and only linear is carried.
/// Density and a Mode other than 1 are reported.
fn spark_verb_report(node: Node, notes: &mut Notes) -> Result<(), String> {
    if node.attribute("Density").is_some() || num(node, "Mode", 1.0)? != 1.0 {
        notes.add(ir::Impact::Audible, "sparkverb density or mode", "");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::load_one;
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
        let e = load_one(doc.root_element(), &mut notes).unwrap();
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
        let e = load_one(doc.root_element(), &mut notes).unwrap();
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
