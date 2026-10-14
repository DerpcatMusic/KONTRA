//! Kontakt `filter` effects onto sampler_ir::fx::filter.
//!
//! The saved type ids are measured: each `$FILTER_TYPE_*` constant set through
//! `set_engine_par(EFFECT_SUBTYPE)` in real Kontakt 8 and read back from the saved
//! BParFXFilter (docs/re/kontakt-bindings/filter-type-ids.tsv, filter_subtypes.tsv: every id
//! below is accepted, every other id in 0..=140 is stored as 0).
//!
//! The native class behind each id (RTTI): BFilter1Pole{LP,HP}, BFilter2Pole{LP,HP,BP},
//! BFilter4Pole{LP,HP,BP,BR}, BFilter6PoleLP, BFilterPhaser, BFilterPro52, BFilterLadder,
//! BFilterSV1/2/4/6, BFilterMultiSV, BFilterDaft, BFilterFormant, BFilterDJ,
//! BFilterSimpleLP/HP/LPHP, BFilterDualSKF, BFilterBlueMonark, BFilterLadder_legacy,
//! BFilterVowelA/B, BFilterVersatile.

use sampler_ir::fx::filter::{Config, Fidelity, Gap, KontaktFx, Response, Topology, Variant};

/// How a saved type is realised.
enum Build {
    /// A configuration; `Fidelity` and the caveat say how far it is proved.
    Config(Config, Fidelity, &'static str),
    /// Handled by another translation arm (the family named).
    Elsewhere(&'static str),
}

const fn cascade(response: Response, poles: u8) -> Config {
    Config::new(Topology::Cascade, response, poles)
}

const fn variant(response: Response, poles: u8, variant: Variant) -> Config {
    let mut c = Config::new(Topology::Cascade, response, poles);
    c.variant = variant;
    c
}

const fn native(topology: Topology, response: Response, poles: u8) -> Config {
    Config::new(topology, response, poles)
}

use Fidelity::{Exact, Partial};
use Response::{BandPass as Bp, HighPass as Hp, LowPass as Lp, Notch as Nt, Peak as Pk};

const SV: &str = "FilterSV1/2/4/6 setters, 32-frame coefficient ramp and process (0x140b05d80.., 0x140b02280..) run by the cascade core match the native classes under kemu to 1e-6 and the Kontakt measurements (rows 1..56) at res <= .5; self-oscillating rows (res .9) are level-dependent and unmeasured here";
const MULTI_SV: &str = "FilterMultiSV (0x140b05d50, 0x140b01d00) matches the native class under kemu to 1e-6; the bandwidth is the record's leading value (read into +0x24), its ENGINE_PAR id is unprobed";
const LEGACY: &str = "BFilter class law and process run by the cascade core (dsp/filter/cascade.rs) match the native classes under kemu to 2e-5 and the Kontakt LP4/LP6 measurements; control clock 0x140ae9f90 reproduced except its alternative branch (global 0x144fd4c54) and offset slew (+0x1f8), both unwritten";
const STAGES: &str = "setter and process (Pro-53 FUN_14095d940/FUN_1409099a0, Legacy Ladder FUN_14095d630/FUN_140aef200, Vowel FUN_14095dd00/FUN_14095e250/FUN_140914a20) run by the cascade core match the native code (kemu) to 2e-5; scalar summation order only (the SSE path for >= 32-frame calls differs in rounding); clock as LEGACY";
const LADDER: &str = "core FUN_140b00be2/FUN_140b06180 checked (cutoff law and table, pole polynomial, resonance, feedback correction, clip, stages, tap weights and compensation, 2 ms ramp); the gain law and the HQ (2x resampled) mode are unproved";
const DAFT: &str = "saved controls and ramp scheduling extracted; native audio core and resampler not recovered, saturation and interpolation are stand-ins (CORE-FILTERS-029..031)";
const AR: &str = "FilterDJ equation and 32-frame clock match extracted instruction vectors; no host oracle (CORE-FILTERS-019)";
const KFX: &str = "KONTAKTFX cores (dsp/filter/kontaktfx.rs: FilterDJExtRange + FilterLadderExtRange, Dual SKF, BlueMonark) bit-exact against the native classes under kemu at 48 kHz; record layouts and knob order from the BFilter readers; stereo interleave and the Simple LP/HP alternate smoother (u8, rate 1/(p2 1500 + 150)) unprobed";
const SIMPLE: KontaktFx = KontaktFx::SimpleLpHp { lp: 0.0, hp: 0.0 };
const fn monark(mode: u8) -> KontaktFx {
    KontaktFx::Monark { mode, variant: 0, knobs: [0.0; 5] }
}
const FORMANT_II: &str = "table rows 5..=9 and the +5 row offset are extracted from the shared core; type 91 is measured ($FILTER_TYPE_FORMANT_2)";

fn build(id: i32) -> Option<(&'static str, Build)> {
    use Build::{Config as C, Elsewhere};
    Some(match id {
        2 => ("Legacy LP1", C(cascade(Lp, 1), Exact, LEGACY)),
        3 => ("Legacy HP1", C(cascade(Hp, 1), Exact, LEGACY)),
        4 => ("Legacy BP2", C(cascade(Bp, 2), Exact, LEGACY)),
        6 => ("Legacy LP2", C(cascade(Lp, 2), Exact, LEGACY)),
        7 => ("Legacy HP2", C(cascade(Hp, 2), Exact, LEGACY)),
        8 => ("Legacy LP4", C(cascade(Lp, 4), Exact, LEGACY)),
        9 => ("Legacy HP4", C(cascade(Hp, 4), Exact, LEGACY)),
        10 => ("Legacy BP4", C(cascade(Bp, 4), Exact, LEGACY)),
        11 => ("Legacy BR4", C(cascade(Nt, 4), Exact, LEGACY)),
        12 => ("Legacy LP6", C(cascade(Lp, 6), Exact, LEGACY)),
        13 => ("Phaser", Elsewhere("Processor::Phaser arm")),
        14 => ("Vowel A", C(variant(Bp, 4, Variant::VowelA), Exact, STAGES)),
        15 => ("Vowel B", C(variant(Bp, 4, Variant::VowelB), Exact, STAGES)),
        16 => ("Pro-53", C(variant(Lp, 4, Variant::Pro52), Exact, STAGES)),
        17 => ("Legacy Ladder", C(variant(Lp, 4, Variant::Ladder), Exact, STAGES)),
        19 => ("3x2 Versatile", C(variant(Lp, 4, Variant::Versatile(VERSATILE_UNSET)), Exact, STAGES)),
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
        50 => ("SV LP1", C(variant(Lp, 1, Variant::Sv), Exact, SV)),
        51 => ("SV HP1", C(variant(Hp, 1, Variant::Sv), Exact, SV)),
        52 => ("SV LP2", C(variant(Lp, 2, Variant::Sv), Exact, SV)),
        53 => ("SV BP2", C(variant(Bp, 2, Variant::Sv), Exact, SV)),
        54 => ("SV HP2", C(variant(Hp, 2, Variant::Sv), Exact, SV)),
        55 => ("SV LP4", C(variant(Lp, 4, Variant::Sv), Exact, SV)),
        56 => ("SV BP4", C(variant(Bp, 4, Variant::Sv), Exact, SV)),
        57 => ("SV HP4", C(variant(Hp, 4, Variant::Sv), Exact, SV)),
        58 => ("SV Notch 4", C(variant(Nt, 4, Variant::Sv), Exact, SV)),
        59 => ("SV LP6", C(variant(Lp, 6, Variant::Sv), Exact, SV)),
        60 => ("SV BP6", C(variant(Bp, 6, Variant::Sv), Exact, SV)),
        61 => ("SV HP6", C(variant(Hp, 6, Variant::Sv), Exact, SV)),
        62 => ("SV Notch 6", C(variant(Nt, 6, Variant::Sv), Exact, SV)),
        63 => ("SV Par LP/HP", C(variant(Lp, 4, Variant::MultiSv { mode: 0, bandwidth: 0.0 }), Exact, MULTI_SV)),
        64 => ("SV Par BP/BP", C(variant(Bp, 4, Variant::MultiSv { mode: 1, bandwidth: 0.0 }), Exact, MULTI_SV)),
        65 => ("SV Ser LP/HP", C(variant(Bp, 4, Variant::MultiSv { mode: 2, bandwidth: 0.0 }), Exact, MULTI_SV)),
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
        120 => ("Simple LP/HP", C(variant(Lp, 2, Variant::Kontakt(SIMPLE)), Exact, KFX)),
        130 => ("Dual SKF", C(variant(Bp, 4, Variant::Kontakt(KontaktFx::DualSkf([0.0; 5]))), Exact, KFX)),
        140 => ("Monark LP1", C(variant(Lp, 1, Variant::Kontakt(monark(2))), Exact, KFX)),
        141 => ("Monark LP2", C(variant(Lp, 2, Variant::Kontakt(monark(1))), Exact, KFX)),
        142 => ("Monark LP4", C(variant(Lp, 4, Variant::Kontakt(monark(0))), Exact, KFX)),
        143 => ("Monark BP", C(variant(Bp, 2, Variant::Kontakt(monark(3))), Exact, KFX)),
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
/// Placeholder knobs that [`map`] replaces with the record's.
const VERSATILE_UNSET: sampler_ir::fx::filter::Versatile =
    sampler_ir::fx::filter::Versatile { shift: [0.0; 2], resonance: [0.0; 2], types: [0.0; 3], wet: [0.0; 3], gain: 0.0 };

/// `extra` is the SV bandwidth (63..=65), Formant II's size, or Versatile's 11 knobs in object order (reader 0x140d00db0, offsets
/// 0x38..0x60): SHIFTB, SHIFTC, RESB, RESC, TYPEA..C, BYPA..C, FILTER_GAIN.
pub(crate) fn map(id: i32, cutoff: f32, resonance: f32, extra: &[f32]) -> Result<Mapped, Gap> {
    match build(id) {
        None => Err(Gap { name: "filter type", reason: "type id not in the Kontakt 8 filter table" }),
        Some((name, Build::Elsewhere(_))) => {
            Err(Gap { name, reason: "belongs to another translation arm" })
        }
        Some((_, Build::Config(mut config, fidelity, caveat))) => {
            config.cutoff = f64::from(cutoff);
            config.resonance = f64::from(resonance);
            let lead = extra.first().copied().filter(|_| id != 19);
            config.extra = lead.map_or(0.0, f64::from);
            match &mut config.variant {
                Variant::MultiSv { bandwidth, .. } => *bandwidth = lead.unwrap_or(0.0).clamp(0.0, 1.0),
                Variant::Versatile(v) => {
                    let &[sb, sc, rb, rc, ta, tb, tc, wa, wb, wc, gain] = extra else {
                        return Err(Gap { name: "3x2 Versatile", reason: "record without the 11 Versatile knobs" });
                    };
                    *v = sampler_ir::fx::filter::Versatile {
                        shift: [sb, sc],
                        resonance: [rb, rc],
                        types: [ta, tb, tc],
                        wet: [wa, wb, wc],
                        gain,
                    };
                }
                // Record order a, b, then `extra` (Monark's variant last); see the reader.
                Variant::Kontakt(k) => {
                    config.extra = 0.0;
                    let (a, b) = (cutoff, resonance);
                    let get = |i: usize| extra.get(i).copied().ok_or(Gap { name: "KONTAKTFX filter", reason: "record without its knobs" });
                    *k = match *k {
                        KontaktFx::SimpleLpHp { .. } => KontaktFx::SimpleLpHp { lp: a, hp: b },
                        // record e is the stored 2v - 1 drive
                        KontaktFx::DualSkf(_) => KontaktFx::DualSkf([b, get(1)?, a, get(0)?, (get(2)? + 1.0) * 0.5]),
                        // record c is the stored 3v + 1 input gain, e the stored 2v - 1 modulation
                        KontaktFx::Monark { mode, .. } => KontaktFx::Monark {
                            mode,
                            variant: get(3)?.clamp(0.0, 2.0) as u8,
                            knobs: [a, b, (get(0)? - 1.0) / 3.0, (get(2)? + 1.0) * 0.5, get(1)?],
                        },
                    };
                }
                _ => {}
            }
            Ok(Mapped { config, fidelity, caveat })
        }
    }
}

/// Every saved type with its name and how it is realised, for the coverage report.
#[cfg(test)]
pub(crate) fn coverage() -> Vec<(i32, &'static str, String)> {
    (0..=143)
        .filter_map(|id| {
            let (name, b) = build(id)?;
            let status = match b {
                Build::Config(_, f, caveat) => format!("{f:?}: {caveat}"),
                Build::Elsewhere(who) => format!("elsewhere: {who}"),
            };
            Some((id, name, status))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_ir::Processor;

    #[test]
    fn legacy_types_are_cascade_classes() {
        for (id, response, poles) in [
            (2, Lp, 1), (3, Hp, 1), (4, Bp, 2), (6, Lp, 2), (7, Hp, 2),
            (8, Lp, 4), (9, Hp, 4), (10, Bp, 4), (11, Nt, 4), (12, Lp, 6),
        ] {
            let m = map(id, 0.5, 0.3, &[]).unwrap();
            assert_eq!(m.fidelity, Exact, "{id}");
            assert_eq!((m.config.topology, m.config.response, m.config.poles), (Topology::Cascade, response, poles));
            assert!(matches!(m.config.processors(None).unwrap()[..], [Processor::FilterBank(_)]));
        }
    }

    #[test]
    fn versatile_takes_its_eleven_knobs_in_object_order() {
        let knobs: Vec<f32> = (0..11).map(|i| i as f32 / 10.0).collect();
        let m = map(19, 0.5, 0.5, &knobs).unwrap();
        let Variant::Versatile(v) = m.config.variant else { panic!() };
        let flat: Vec<f32> = v.shift.iter().chain(&v.resonance).chain(&v.types).chain(&v.wet).chain([&v.gain]).copied().collect();
        assert_eq!(flat, knobs);
        assert!(map(19, 0.5, 0.5, &knobs[..10]).is_err());
    }

    #[test]
    fn every_table_entry_builds_or_says_why_not() {
        for (id, name, status) in coverage() {
            if status.starts_with("gap") || status.starts_with("elsewhere") {
                assert!(map(id, 0.5, 0.5, &[0.5; 11]).is_err(), "{id} {name}");
                continue;
            }
            let m = map(id, 0.5, 0.5, &[0.5; 11]).unwrap();
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
}
