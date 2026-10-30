//! Every named law, for inspection: its name, where its maths was confirmed, and the input
//! range worth sampling.

use crate::{Law, common, kontakt, uvi};

/// A named law and its provenance.
#[derive(Clone, Debug)]
pub struct Named {
    /// The constant's path under `kontra_law`.
    pub name: &'static str,
    /// The law.
    pub law: Law,
    /// The native address it was confirmed at, or why it has none.
    pub source: &'static str,
    /// The input range worth sampling, in the law's source units.
    pub domain: [f32; 2],
}

/// Not found in the vendor binary.
const UNVERIFIED: &str = "unverified";

/// Every named law. Data-dependent families (`kontakt::modulation_shaper`,
/// `uvi::fade_edge`) appear at a representative parameter.
#[must_use]
pub fn catalog() -> Vec<Named> {
    let n = |name, law: &Law, source, domain| Named {
        name,
        law: law.clone(),
        source,
        domain,
    };
    let unit = [0.0, 1.0];
    vec![
        n(
            "common::BIPOLAR",
            &common::BIPOLAR,
            "Kontakt 0x14071a840",
            unit,
        ),
        n(
            "common::CUBE",
            &common::CUBE,
            "Kontakt 0x140ae48d0, 0x140b05f26",
            unit,
        ),
        n(
            "common::SQUARE",
            &common::SQUARE,
            "Kontakt probe mod_par_laws.lua",
            unit,
        ),
        n(
            "common::SIGNED_CUBE",
            &common::SIGNED_CUBE,
            "Kontakt probe mod_par_laws.lua",
            unit,
        ),
        n(
            "common::ATTACK_CURVE",
            &common::ATTACK_CURVE,
            "Kontakt 0x140ae4958",
            [-1.0, 1.0],
        ),
        n(
            "common::STAGE_15S",
            &common::STAGE_15S,
            "Kontakt 0x1407b85a0",
            unit,
        ),
        n(
            "common::STAGE_25S",
            &common::STAGE_25S,
            "Kontakt 0x1407b85e0",
            unit,
        ),
        n(
            "common::STAGE_10S",
            &common::STAGE_10S,
            "Kontakt probe mod_par_laws.lua",
            unit,
        ),
        n(
            "common::LFO_FADE_IN",
            &common::LFO_FADE_IN,
            "Kontakt 0x140b06c90",
            unit,
        ),
        n(
            "common::SQUARE_ROOT",
            &common::SQUARE_ROOT,
            "UVI 0x14138ab10",
            unit,
        ),
        n(
            "common::DB_GAIN",
            &common::DB_GAIN,
            "definition",
            [-60.0, 12.0],
        ),
        n(
            "common::MILLIBEL_GAIN",
            &common::MILLIBEL_GAIN,
            "definition",
            [-60_000.0, 12_000.0],
        ),
        n(
            "kontakt::CONTROLLER_VOLUME",
            &kontakt::CONTROLLER_VOLUME,
            "0x140873120",
            unit,
        ),
        n(
            "kontakt::VOICE_VOLUME",
            &kontakt::VOICE_VOLUME,
            "0x140873120",
            unit,
        ),
        n(
            "kontakt::FILTER_NOTE_HZ",
            &kontakt::FILTER_NOTE_HZ,
            "0x140b04ee0",
            [0.0, 160.0],
        ),
        n(
            "kontakt::PITCH_INDEX",
            &kontakt::PITCH_INDEX,
            "0x140aaf220",
            [1200.0, 1900.0],
        ),
        n(
            "kontakt::VERSATILE_NOTE_HZ",
            &kontakt::VERSATILE_NOTE_HZ,
            "0x14095db50",
            [20.0, 131.0],
        ),
        n(
            "kontakt::UNIT_FIFTH",
            &kontakt::UNIT_FIFTH,
            "0x140b05f5c",
            unit,
        ),
        n(
            "kontakt::REPLIKA_LOW_CUT",
            &kontakt::REPLIKA_LOW_CUT,
            UNVERIFIED,
            unit,
        ),
        n(
            "kontakt::VERSATILE_GAIN",
            &kontakt::VERSATILE_GAIN,
            UNVERIFIED,
            unit,
        ),
        n(
            "kontakt::CONVOLUTION_PREDELAY",
            &kontakt::CONVOLUTION_PREDELAY,
            "0x1407b68c0",
            unit,
        ),
        n(
            "kontakt::FLEX_SHAPE",
            &kontakt::FLEX_SHAPE,
            "0x140ae5a10",
            unit,
        ),
        n(
            "kontakt::EQ_PEAK_DB",
            &kontakt::EQ_PEAK_DB,
            "0x1409591c0",
            [-18.0, 18.0],
        ),
        n(
            "kontakt::LADDER_GAIN",
            &kontakt::LADDER_GAIN,
            "0x140b05460",
            [-1.0, 1.0],
        ),
        n(
            "kontakt::LEGACY_RESONANCE_Q",
            &kontakt::LEGACY_RESONANCE_Q,
            UNVERIFIED,
            unit,
        ),
        n(
            "kontakt::LEGACY_RESONANCE_LOSS[2]",
            &kontakt::LEGACY_RESONANCE_LOSS[2],
            UNVERIFIED,
            unit,
        ),
        n(
            "kontakt::LEGACY_CUTOFF_HZ",
            &kontakt::LEGACY_CUTOFF_HZ,
            "0x14095c9c0",
            unit,
        ),
        n(
            "kontakt::LEGACY_REVERB_HIGH_CUT_HZ",
            &kontakt::LEGACY_REVERB_HIGH_CUT_HZ,
            "0x14095b2d0",
            unit,
        ),
        n(
            "kontakt::FEEDBACK_THRESHOLD",
            &kontakt::FEEDBACK_THRESHOLD,
            "0x141a91d10",
            [2.0, 20.0],
        ),
        n(
            "kontakt::FEEDBACK_KNEE",
            &kontakt::FEEDBACK_KNEE,
            "0x141a91d10",
            [1.0, 20.0],
        ),
        n(
            "kontakt::modulation_shaper(0.5 bend)",
            &kontakt::modulation_shaper(&[(0.0, 0.0, 0.5), (1.0, 1.0, 0.0)]),
            "0x14087c7f0",
            unit,
        ),
        n("uvi::KEY_SOURCE", &uvi::KEY_SOURCE, "0x14138ab10", unit),
        n(
            "uvi::LOWER_KEY_FOLLOW",
            &uvi::LOWER_KEY_FOLLOW,
            "0x14138ab10",
            unit,
        ),
        n(
            "uvi::ENVELOPE_CURVE",
            &uvi::ENVELOPE_CURVE,
            "0x1410f1220",
            [-1.0, 1.0],
        ),
        n(
            "uvi::WAVESHAPER_DRIVE",
            &uvi::WAVESHAPER_DRIVE,
            UNVERIFIED,
            unit,
        ),
        n(
            "uvi::CLIPPER_ASYMMETRY",
            &uvi::CLIPPER_ASYMMETRY,
            UNVERIFIED,
            [-1.0, 1.0],
        ),
        n(
            "uvi::fade_edge(2)",
            &uvi::fade_edge(2.0),
            "0x140f28cba",
            unit,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::catalog;

    #[test]
    fn names_are_unique_and_laws_finite_over_their_domain() {
        let all = catalog();
        for (i, a) in all.iter().enumerate() {
            assert!(
                all.iter().skip(i + 1).all(|b| b.name != a.name),
                "{}",
                a.name
            );
            let [lo, hi] = a.domain;
            for k in 0..=8 {
                let x = lo + (hi - lo) * k as f32 / 8.0;
                assert!(a.law.eval_unclamped(x).is_finite(), "{} at {x}", a.name);
            }
        }
    }
}
