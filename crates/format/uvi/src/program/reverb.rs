//! UVI's SparkVerb onto `ir::Reverb`. Natively a feedback-delay network of
//! 8 or 16 lines (Quality); here its laws drive the one fdn. From
//! UVIWorkstation's SparkVerb engine (parameter ids 0x142805d88..de0, setters
//! 0x141320bb0..0x141321010, audio 0x141679c90):
//! - the lines' mean length is the room's crossing time, `RoomSize/340` s
//!   (0x14167f190, 1/340 at 0x141e88a40);
//! - each pass loses `600·length/(DecayTime·RoomSize)` dB (0x14167b8c0,
//!   −600 at 0x14202c3a8), so the tail falls 60 dB in `DecayTime·RoomSize/10` s,
//!   DecayLow times that below FreqLow; HiCut swaps the high band's decay
//!   for a first-order low-pass at FreqHigh in the loop (0x141649220);
//! - the input is scaled by `√((1 − m)/(1 + m))`, `m` the lines' mean power
//!   gain per pass (+0x440, set at the end of 0x14167b8c0), and enters every
//!   line at `1/√2` per channel; each line leaves at `±√(2/N)` (0x14167dbd0,
//!   0x14167c0e0). A centred impulse's tail then carries `4m/(1 + m)` per
//!   channel;
//! - the lines swing by ModDepth cents at `ModRate/16` cycles per line length
//!   (0x14167d1a0, 2π·1200 at 0x14202c390);
//! - Mix is linear, or equal-power when MixMode is 1; Rolloff is a one-pole
//!   on the output.

use kontra_format::Notes;
use kontra_ir as ir;
use roxmltree::Node;

use super::{flag, num};

const SPEED_OF_SOUND: f64 = 340.0;
/// The fdn's mean line length at size 1, s, and its damping and diffusion
/// knobs at 1 (kontra-dsp-reverb).
const LINES_S: f64 = 0.051_36;
const FDN_DAMP: f64 = 0.85;
const FDN_DIFFUSION: f64 = 0.7;
/// The fdn's tail energy per channel over `m/(1 − m)` for a centred unit
/// impulse (kontra-dsp-reverb tests/fdn.rs, within 1 dB).
const FDN_ENERGY: f64 = 0.84;
/// The rate the loop low-pass's corner is given at.
const RATE: f64 = 48_000.0;

/// The SparkVerb at `node`. Freeze, Shape, a low cut in the loop and the
/// high band's own decay are reported when set.
pub(super) fn spark_verb(node: Node, notes: &mut Notes) -> Result<ir::Effect, String> {
    let room = num(node, "RoomSize", 20.0)?.max(1.0);
    let seconds = num(node, "DecayTime", 1.0)?.max(0.01) * room / 10.0;
    let crossing = room / SPEED_OF_SOUND;
    // At the fdn's size for the same mean length, its loop power per pass is
    // SparkVerb's.
    let m = 10f64.powf(-6.0 * crossing / seconds);
    let level = 2.0 * ((1.0 - m) / ((1.0 + m) * FDN_ENERGY)).sqrt();
    let mut report = |feature: &str| notes.add(ir::Impact::Audible, feature, "");
    for name in ["Freeze", "Shape"] {
        if num(node, name, 0.0)? != 0.0 {
            report(&format!("sparkverb {name}"));
        }
    }
    let low_shelf_db = if flag(node, "LowCut") {
        report("sparkverb loop low cut");
        0.0
    } else {
        // The bass tail's energy scales with its decay time.
        10.0 * num(node, "DecayLow", 1.0)?.max(1e-3).log10()
    };
    let damping = if flag(node, "HiCut") {
        let pole = (-std::f64::consts::TAU * num(node, "FreqHigh", 12_000.0)? / RATE).exp();
        pole / FDN_DAMP
    } else {
        if num(node, "DecayHigh", 1.0)? != 1.0 {
            report("sparkverb high decay");
        }
        0.0
    };
    let diffusion = if flag(node, "DiffusionOnOff") {
        num(node, "Diffusion", 0.618)? / FDN_DIFFUSION
    } else {
        0.0
    };
    let hz = num(node, "ModRate", 1.0)?.max(0.0) / 16.0 / crossing;
    let swing_ms = if hz > 0.0 {
        let ratio = num(node, "ModDepth", 0.0)?.max(0.0) * std::f64::consts::LN_2 / 1200.0;
        1000.0 * ratio / (std::f64::consts::TAU * hz)
    } else {
        0.0
    };
    let mix = num(node, "Mix", 1.0)?.clamp(0.0, 1.0);
    let (wet, dry) = if num(node, "MixMode", 0.0)? == 1.0 {
        (mix.sqrt(), (1.0 - mix).sqrt())
    } else {
        (mix, 1.0 - mix)
    };
    Ok(ir::Effect {
        kind: ir::EffectKind::Reverb(ir::Reverb {
            seconds: seconds as f32,
            size: (crossing / LINES_S) as f32,
            size_curve: [1.0, 0.0],
            damping: damping.clamp(0.0, 1.0) as f32,
            diffusion: diffusion.clamp(0.0, 1.0) as f32,
            predelay_ms: num(node, "PreDelay", 0.0)?.max(0.0) as f32,
            high_cut_hz: num(node, "Rolloff", 20_000.0)? as f32,
            low_shelf_db: low_shelf_db as f32,
            stereo: num(node, "Width", 1.0)?.clamp(0.0, 1.0) as f32,
            modulation_ms: swing_ms as f32,
            modulation_hz: hz as f32,
        }),
        bypass: false,
        output: (wet * level) as f32,
        dry: dry as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_corpus_setting_loads_its_decay_room_and_level() {
        // Augmented Orchestra's most common SparkVerb (620 instances).
        let xml = r#"<SparkVerb ModDepth="5.9121876" ModRate="1" Diffusion="0.61799997"
            Width="1" RoomSize="20" DecayTime="2.2388823" DecayLow="0.16929972"
            DecayHigh="0.70764023" FreqLow="250" FreqHigh="12000" Shape="0" Mix="1"
            HiCut="1" LowCut="0" Rolloff="20000" DiffusionOnOff="1" PreDelay="0"
            MixMode="1"/>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let e = spark_verb(doc.root_element(), &mut Notes::default()).unwrap();
        let ir::EffectKind::Reverb(r) = e.kind else {
            panic!("{e:?}")
        };
        let near = |a: f32, b: f32, tolerance: f32| (a - b).abs() < tolerance;
        // 2.24 × 20 m / 10: 4.5 s; a 20 m room's 59 ms crossing.
        assert!(near(r.seconds, 4.478, 0.01), "{}", r.seconds);
        assert!(near(r.size, 1.145, 0.01), "{}", r.size);
        // Bass dies 6× faster: −7.7 dB of its tail.
        assert!(near(r.low_shelf_db, -7.71, 0.01), "{}", r.low_shelf_db);
        // A one-pole at 12 kHz in the loop.
        assert!(near(r.damping, 0.245, 0.01), "{}", r.damping);
        // 5.9 cents at 1.06 Hz: a 0.51 ms swing.
        assert!(near(r.modulation_hz, 1.0625, 0.001), "{}", r.modulation_hz);
        assert!(near(r.modulation_ms, 0.511, 0.005), "{}", r.modulation_ms);
        // m = 10^(−0.176/2.24) = 0.834: 2·√(0.166/(1.834·0.84)) = 0.66.
        assert!(near(e.output, 0.657, 0.005), "{}", e.output);
        assert_eq!(e.dry, 0.0);
    }
}
