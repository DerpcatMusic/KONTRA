//! Kontakt `filter` effects onto sampler_ir::fx::filter.
//!
//! The saved type ids and names come from the Kontakt 8 binary: the KSP constant registration
//! order ($FILTER_TYPE_*, file offset 0x4e7c288) matches the ids accepted by the probe
//! (`filter_subtypes.tsv`) and the GUI-verified ids 3 (Legacy HP1), 13 (Phaser), 19 (3x2
//! Versatile), 22..24 (EQ), 30..41 (Ladder), 52, 54, 55, 57 (SV), 70/71 (Daft), 90, 100..108.
//! Ids marked "inferred" are placed by that order only; 5 and 18 are the two gaps in it.
//!
//! The native class behind each id (RTTI): BFilter1Pole{LP,HP}, BFilter2Pole{LP,HP,BP},
//! BFilter4Pole{LP,HP,BP,BR}, BFilter6PoleLP, BFilterPhaser, BFilterPro52, BFilterLadder,
//! BFilterSV1/2/4/6, BFilterMultiSV, BFilterDaft, BFilterFormant, BFilterDJ,
//! BFilterSimpleLP/HP/LPHP, BFilterDualSKF, BFilterBlueMonark, BFilterLadder_legacy,
//! BFilterVowelA/B, BFilterVersatile.

use sampler_ir::fx::filter::{
    Config, CutoffLaw, Fidelity, Gap, LEGACY_CUTOFF_OCTAVES, ResonanceLaw, Response, Topology,
};

/// How a saved type is realised.
enum Build {
    /// A configuration; `Fidelity` and the caveat say how far it is proved.
    Config(Config, Fidelity, &'static str),
    /// Handled by another translation arm (the family named).
    Elsewhere(&'static str),
    /// Not configurable yet, and why.
    Gap(&'static str),
}

const fn svf(response: Response, poles: u8) -> Config {
    let mut c = Config::new(Topology::Svf, response, poles);
    c.cutoff_law = CutoffLaw::Sv;
    c.resonance_law = ResonanceLaw::Sv;
    c
}

const fn legacy(response: Response, poles: u8) -> Config {
    let mut c = Config::new(Topology::Svf, response, poles);
    c.cutoff_law = CutoffLaw::Legacy;
    c.resonance_law = ResonanceLaw::Legacy;
    c
}

const fn legacy_one(response: Response) -> Config {
    let mut c = legacy(response, 1);
    c.cutoff_law = CutoffLaw::LegacyOnePole;
    c
}

const fn native(topology: Topology, response: Response, poles: u8) -> Config {
    Config::new(topology, response, poles)
}

use Fidelity::{Exact, Generic, Partial};
use Response::{BandPass as Bp, HighPass as Hp, LowPass as Lp, Notch as Nt, Peak as Pk};

const SV_LP2: &str = "response measured against Kontakt 8 (cutoff, Q, -3 dB points); native recurrence, control clock and the -6 dB passband loss at full resonance are not reproduced (CORE-FILTERS-065..072)";
const SV: &str = "cutoff 25*800^x and Q law measured on SV LP2 only; this member shares them unmeasured, cascaded sections share one Q, and the passband loss at high resonance is not reproduced";
const SV_ONE: &str = "one-pole section with the SV LP2 cutoff law, unmeasured; Q is unused";
const LEGACY: &str = "pinned v1 stand-in: legacy cutoff/Q laws unmeasured, native BFilter1Pole/2Pole kernel not extracted";
const LEGACY_ONE: &str = "BFilter1PoleLP/HP setCutoff extracted (bilinear one-pole at the native warped pole, rate independent); native cutoff modulation is a 32-frame slewed update here run on the shared clock, and the dry/accumulate output mode is not modelled";
const LADDER: &str = "saved row/morphology and compensation tables extracted; cutoff, gain, resonance setters, control clock and nonlinear kernel unproved (audit CORE-FILTERS-045..054)";
const DAFT: &str = "saved controls and ramp scheduling extracted; native audio core and resampler not recovered, saturation and interpolation are stand-ins (CORE-FILTERS-029..031)";
const AR: &str = "FilterDJ equation and 32-frame clock match extracted instruction vectors; no host oracle (CORE-FILTERS-019)";
const FORMANT_II: &str = "table rows 5..=9 and the +5 row offset are extracted from the shared core; binding of type 91 to that offset is inferred";
const BUTTERWORTH: &str = "legacy 4- and 6-pole filters measure as Butterworth, which needs a different Q per cascaded section; the IR cascades identical sections only";

fn build(id: i32) -> Option<(&'static str, Build)> {
    use Build::{Config as C, Elsewhere, Gap};
    Some(match id {
        2 => ("Legacy LP1", C(legacy_one(Lp), Partial, LEGACY_ONE)),
        3 => ("Legacy HP1", C(legacy_one(Hp), Partial, LEGACY_ONE)),
        4 => ("Legacy BP2", C(legacy(Bp, 2), Generic, LEGACY)),
        6 => ("Legacy LP2", C(legacy(Lp, 2), Generic, LEGACY)),
        7 => ("Legacy HP2", C(legacy(Hp, 2), Generic, LEGACY)),
        8 => ("Legacy LP4", Gap(BUTTERWORTH)),
        9 => ("Legacy HP4", Gap(BUTTERWORTH)),
        10 => ("Legacy BP4", Gap(BUTTERWORTH)),
        11 => ("Legacy BR4", Gap(BUTTERWORTH)),
        12 => ("Legacy LP6", Gap(BUTTERWORTH)),
        13 => ("Phaser", Elsewhere("Processor::Phaser arm")),
        14 => ("Vowel A", Gap("BFilterVowelA core not extracted (id inferred)")),
        15 => ("Vowel B", Gap("BFilterVowelB core not extracted (id inferred)")),
        16 => ("Pro-53", Gap("BFilterPro52 core not extracted; passband +3.3 dB measured only (id inferred)")),
        17 => ("Legacy Ladder", Gap("BFilterLadder_legacy core not extracted (id inferred)")),
        19 => ("3x2 Versatile", Gap("Filter3x2 module and BFilterVersatile core not extracted")),
        22 => ("EQ 1 band", Elsewhere("eq family")),
        23 => ("EQ 2 band", Elsewhere("eq family")),
        24 => ("EQ 3 band", Elsewhere("eq family")),
        30 => ("Ladder LP1", C(native(Topology::Ladder, Lp, 1), Partial, LADDER)),
        31 => ("Ladder LP2", C(native(Topology::Ladder, Lp, 2), Partial, LADDER)),
        32 => ("Ladder LP3", C(native(Topology::Ladder, Lp, 3), Partial, LADDER)),
        33 => ("Ladder LP4", C(native(Topology::Ladder, Lp, 4), Partial, LADDER)),
        34 => ("Ladder HP1", C(native(Topology::Ladder, Hp, 1), Partial, LADDER)),
        35 => ("Ladder HP2", C(native(Topology::Ladder, Hp, 2), Partial, LADDER)),
        36 => ("Ladder HP3", C(native(Topology::Ladder, Hp, 3), Partial, LADDER)),
        37 => ("Ladder HP4", C(native(Topology::Ladder, Hp, 4), Partial, LADDER)),
        38 => ("Ladder BP2", C(native(Topology::Ladder, Bp, 2), Partial, LADDER)),
        39 => ("Ladder BP4", C(native(Topology::Ladder, Bp, 4), Partial, LADDER)),
        40 => ("Ladder Peak", C(native(Topology::Ladder, Pk, 4), Partial, LADDER)),
        41 => ("Ladder Notch", C(native(Topology::Ladder, Nt, 4), Partial, LADDER)),
        50 => ("SV LP1", C(svf(Lp, 1), Partial, SV_ONE)),
        51 => ("SV HP1", C(svf(Hp, 1), Partial, SV_ONE)),
        52 => ("SV LP2", C(svf(Lp, 2), Partial, SV_LP2)),
        53 => ("SV BP2", C(svf(Bp, 2), Partial, SV)),
        54 => ("SV HP2", C(svf(Hp, 2), Partial, SV)),
        55 => ("SV LP4", C(svf(Lp, 4), Partial, SV)),
        56 => ("SV BP4", C(svf(Bp, 4), Partial, SV)),
        57 => ("SV HP4", C(svf(Hp, 4), Partial, SV)),
        58 => ("SV Notch 4", C(svf(Nt, 4), Partial, SV)),
        59 => ("SV LP6", C(svf(Lp, 6), Partial, SV)),
        60 => ("SV BP6", C(svf(Bp, 6), Partial, SV)),
        61 => ("SV HP6", C(svf(Hp, 6), Partial, SV)),
        62 => ("SV Notch 6", C(svf(Nt, 6), Partial, SV)),
        63 => ("SV Par LP/HP", Gap("BFilterMultiSV topology and its extra controls not extracted (id inferred)")),
        64 => ("SV Par BP/BP", Gap("BFilterMultiSV topology and its extra controls not extracted (id inferred)")),
        65 => ("SV Ser LP/HP", Gap("BFilterMultiSV topology and its extra controls not extracted (id inferred)")),
        70 => ("Daft LP", C(native(Topology::Daft, Lp, 2), Partial, DAFT)),
        71 => ("Daft HP", C(native(Topology::Daft, Hp, 2), Partial, DAFT)),
        90 => ("Formant I", C(native(Topology::Formant, Lp, 0), Exact, "")),
        91 => ("Formant II", C(native(Topology::FormantII, Lp, 0), Partial, FORMANT_II)),
        100 => ("AR LP2", C(native(Topology::Ar, Lp, 2), Exact, AR)),
        101 => ("AR BP2", C(native(Topology::Ar, Bp, 2), Exact, AR)),
        102 => ("AR HP2", C(native(Topology::Ar, Hp, 2), Exact, AR)),
        103 => ("AR LP4", C(native(Topology::Ar, Lp, 4), Exact, AR)),
        104 => ("AR BP4", C(native(Topology::Ar, Bp, 4), Exact, AR)),
        105 => ("AR HP4", C(native(Topology::Ar, Hp, 4), Exact, AR)),
        106 => ("AR LP2/4", C(native(Topology::Ar, Lp, 0), Exact, AR)),
        107 => ("AR BP2/4", C(native(Topology::Ar, Bp, 0), Exact, AR)),
        108 => ("AR HP2/4", C(native(Topology::Ar, Hp, 0), Exact, AR)),
        _ => return None,
    })
}

/// The native name of a saved filter type.
pub(crate) fn name(id: i32) -> Option<&'static str> {
    build(id).map(|(name, _)| name)
}

/// A mapped filter: its configuration and how far it is proved.
pub(crate) struct Mapped {
    pub config: Config,
    pub fidelity: Fidelity,
    pub caveat: &'static str,
}

/// The configuration template of a saved type (zero knobs), for the arms that own a native
/// kernel's binding and ranges.
pub(crate) fn template(id: i32) -> Option<Mapped> {
    match build(id)? {
        (_, Build::Config(config, fidelity, caveat)) => Some(Mapped { config, fidelity, caveat }),
        _ => None,
    }
}

/// A saved type with its stored cutoff, resonance and optional third knob (Formant Size).
pub(crate) fn map(id: i32, cutoff: f32, resonance: f32, extra: Option<f32>) -> Result<Mapped, Gap> {
    match build(id) {
        None => Err(Gap { name: "filter type", reason: "type id not in the Kontakt 8 filter table" }),
        Some((name, Build::Gap(reason))) => Err(Gap { name, reason }),
        Some((name, Build::Elsewhere(_))) => {
            Err(Gap { name, reason: "belongs to another translation arm" })
        }
        Some((_, Build::Config(mut config, fidelity, caveat))) => {
            config.cutoff = f64::from(cutoff);
            config.resonance = f64::from(resonance);
            config.extra = extra.map_or(0.0, f64::from);
            Ok(Mapped { config, fidelity, caveat })
        }
    }
}

/// Octaves one unit of an addressed cutoff modulation moves, by the type's cutoff law.
pub(crate) fn cutoff_octaves(id: i32) -> Option<f64> {
    match build(id)? {
        (_, Build::Config(c, ..)) => match c.cutoff_law {
            CutoffLaw::Legacy | CutoffLaw::LegacyOnePole => Some(LEGACY_CUTOFF_OCTAVES),
            CutoffLaw::Sv => Some(800f64.log2()),
            _ => None,
        },
        _ => None,
    }
}

/// Every saved type with its name and how it is realised, for the coverage report.
#[cfg(test)]
pub(crate) fn coverage() -> Vec<(i32, &'static str, String)> {
    (0..=108)
        .filter_map(|id| {
            let (name, b) = build(id)?;
            let status = match b {
                Build::Config(_, f, caveat) => format!("{f:?}: {caveat}"),
                Build::Elsewhere(who) => format!("elsewhere: {who}"),
                Build::Gap(reason) => format!("gap: {reason}"),
            };
            Some((id, name, status))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_ir::{Filter, FilterKind, Processor};

    #[test]
    fn sv_lp2_keeps_its_measured_laws() {
        let m = map(52, 0.293, 0.0, None).unwrap();
        let [Processor::Filter(Filter { kind, .. })] = m.config.processors(None).unwrap()[..]
        else {
            panic!("one filter")
        };
        assert_eq!(kind, FilterKind::LowPass { poles: 2 });
        assert_eq!(m.fidelity, Partial);
        assert!((m.config.hertz(None).unwrap() - 25.0 * 800f64.powf(0.293f32 as f64)).abs() < 1e-6);
    }

    #[test]
    fn legacy_one_pole_types_use_the_extracted_warp() {
        for (id, high) in [(2, false), (3, true)] {
            let m = map(id, 0.5, 0.3, None).unwrap();
            assert_eq!(m.fidelity, Partial);
            let [Processor::Filter(f)] = m.config.processors(None).unwrap()[..] else { panic!() };
            assert_eq!(
                f.kind,
                if high { FilterKind::HighPass { poles: 1 } } else { FilterKind::LowPass { poles: 1 } }
            );
        }
    }

    #[test]
    fn every_table_entry_builds_or_says_why_not() {
        for (id, name, status) in coverage() {
            if status.starts_with("gap") || status.starts_with("elsewhere") {
                assert!(map(id, 0.5, 0.5, Some(0.5)).is_err(), "{id} {name}");
                continue;
            }
            let m = map(id, 0.5, 0.5, Some(0.5)).unwrap();
            assert!(!m.config.processors(None).unwrap().is_empty(), "{id} {name}");
        }
    }

    #[test]
    fn ids_are_the_ladder_and_ar_orders_the_arms_expect() {
        for id in 30..=41 {
            let m = template(id).unwrap().config;
            assert_eq!(
                sampler_ir::fx::filter::ladder_taps(m.response, m.poles),
                Some((id - 29) as u8)
            );
        }
        for id in 100..=108 {
            let Processor::Ar(a) = template(id).unwrap().config.processors(None).unwrap()[0] else {
                panic!()
            };
            assert_eq!(i32::from(a.mode), id - 100);
        }
    }

    #[test]
    fn cutoff_octaves_follow_the_law() {
        assert_eq!(cutoff_octaves(2), Some(8.96));
        assert!((cutoff_octaves(55).unwrap() - 9.6439).abs() < 1e-3);
        assert_eq!(cutoff_octaves(33), None);
    }
}
