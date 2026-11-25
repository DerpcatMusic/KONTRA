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
            "Kontakt [ref]",
            unit,
        ),
        n(
            "common::CUBE",
            &common::CUBE,
            "Kontakt [ref]",
            unit,
        ),
        n(
            "common::SQUARE",
            &common::SQUARE,
            "measured only: probe mod_par_laws.lua",
            unit,
        ),
        n(
            "common::SIGNED_CUBE",
            &common::SIGNED_CUBE,
            "measured only: probe mod_par_laws.lua",
            unit,
        ),
        n(
            "common::ATTACK_CURVE",
            &common::ATTACK_CURVE,
            "Kontakt [ref]",
            [-1.0, 1.0],
        ),
        n(
            "common::STAGE_15S",
            &common::STAGE_15S,
            "Kontakt [ref]",
            unit,
        ),
        n(
            "common::STAGE_25S",
            &common::STAGE_25S,
            "Kontakt [ref]",
            unit,
        ),
        n(
            "common::STAGE_10S",
            &common::STAGE_10S,
            "Kontakt [ref]",
            unit,
        ),
        n(
            "common::LFO_FADE_IN",
            &common::LFO_FADE_IN,
            "Kontakt [ref]",
            unit,
        ),
        n(
            "common::SQUARE_ROOT",
            &common::SQUARE_ROOT,
            "UVI [ref]",
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
            "[ref]",
            unit,
        ),
        n(
            "kontakt::VOICE_VOLUME",
            &kontakt::VOICE_VOLUME,
            "[ref]",
            unit,
        ),
        n(
            "kontakt::FILTER_NOTE_HZ",
            &kontakt::FILTER_NOTE_HZ,
            "[ref]",
            [0.0, 160.0],
        ),
        n(
            "kontakt::PITCH_INDEX",
            &kontakt::PITCH_INDEX,
            "[ref]",
            [1200.0, 1900.0],
        ),
        n(
            "kontakt::VERSATILE_NOTE_HZ",
            &kontakt::VERSATILE_NOTE_HZ,
            "[ref]",
            [20.0, 131.0],
        ),
        n(
            "kontakt::UNIT_FIFTH",
            &kontakt::UNIT_FIFTH,
            "[ref]",
            unit,
        ),
        n(
            "kontakt::REPLIKA_LOW_CUT",
            &kontakt::REPLIKA_LOW_CUT,
            "measured only: kemu run, k-replika-binding.md",
            unit,
        ),
        n(
            "kontakt::VERSATILE_GAIN",
            &kontakt::VERSATILE_GAIN,
            "measured only: filt-multi-versatile.md",
            unit,
        ),
        n(
            "kontakt::CONVOLUTION_PREDELAY",
            &kontakt::CONVOLUTION_PREDELAY,
            "[ref]",
            unit,
        ),
        n(
            "kontakt::FLEX_SHAPE",
            &kontakt::FLEX_SHAPE,
            "[ref]",
            unit,
        ),
        n(
            "kontakt::EQ_PEAK_DB",
            &kontakt::EQ_PEAK_DB,
            "[ref]",
            [-18.0, 18.0],
        ),
        n(
            "kontakt::LADDER_GAIN",
            &kontakt::LADDER_GAIN,
            "[ref]",
            [-1.0, 1.0],
        ),
        n(
            "kontakt::LEGACY_RESONANCE_Q",
            &kontakt::LEGACY_RESONANCE_Q,
            "measured only: LP4 peaks filt-legacy.md",
            unit,
        ),
        n(
            "kontakt::LEGACY_RESONANCE_LOSS[2]",
            &kontakt::LEGACY_RESONANCE_LOSS[2],
            "measured only: LP4 peaks filt-legacy.md",
            unit,
        ),
        n(
            "kontakt::LEGACY_CUTOFF_HZ",
            &kontakt::LEGACY_CUTOFF_HZ,
            "[ref]",
            unit,
        ),
        n(
            "kontakt::LEGACY_REVERB_HIGH_CUT_HZ",
            &kontakt::LEGACY_REVERB_HIGH_CUT_HZ,
            "[ref]",
            unit,
        ),
        n(
            "kontakt::FEEDBACK_THRESHOLD",
            &kontakt::FEEDBACK_THRESHOLD,
            "[ref]",
            [2.0, 20.0],
        ),
        n(
            "kontakt::FEEDBACK_KNEE",
            &kontakt::FEEDBACK_KNEE,
            "[ref]",
            [1.0, 20.0],
        ),
        n(
            "kontakt::modulation_shaper(0.5 bend)",
            &kontakt::modulation_shaper(&[(0.0, 0.0, 0.5), (1.0, 1.0, 0.0)]),
            "[ref]",
            unit,
        ),
        n("uvi::KEY_SOURCE", &uvi::KEY_SOURCE, "[ref]", unit),
        n(
            "uvi::LOWER_KEY_FOLLOW",
            &uvi::LOWER_KEY_FOLLOW,
            "[ref]",
            unit,
        ),
        n(
            "uvi::ENVELOPE_CURVE",
            &uvi::ENVELOPE_CURVE,
            "[ref]",
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
            "[ref]",
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
