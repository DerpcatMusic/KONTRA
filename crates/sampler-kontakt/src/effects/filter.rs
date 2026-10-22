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

use super::{Lowering, Notes, Site, product};
use sampler_ir::fx::filter::{Config, Fidelity, Gap, KontaktFx, Response, Topology, Variant};
use sampler_ir::{Processor, Reason, SlotAddress};

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
            let (fidelity, caveat) = if matches!(id, 2..=4 | 6..=12)
                && !(0.0..=1.0).contains(&cutoff) {
                (Partial, "legacy cutoff coefficient saturation and raw slew state verified in native setters/wrapper; transition boundary phase for out-of-range saved targets is not yet calibrated")
            } else { (fidelity, caveat) };
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

    use crate::effects::*;
    use crate::effects::test_support::*;

    /// The three KONTAKTFX record layouts decode to the core's knobs in native order.
    #[test]
    fn kontaktfx_filter_records_reach_their_cores() {
        use sampler_ir::fx::filter::{KontaktFx, Variant};
        let record = |kind: i32, head: &[u8], tail: usize, knobs: &[f32]| {
            let mut public = head.to_vec();
            public.extend(knobs.iter().flat_map(|v| v.to_le_bytes()));
            public.extend((0..tail).flat_map(|_| kind.to_le_bytes()));
            slot(0x18, public, 1.)
        };
        let k = |kind: i32, n: usize| (0..n).flat_map(|_| kind.to_le_bytes()).collect::<Vec<u8>>();
        let simple = [k(120, 1), vec![1], k(120, 4)].concat();
        let monark = [k(141, 2), 2i32.to_le_bytes().to_vec()].concat();
        for (fx, expected) in [
            (record(120, &simple, 0, &[0.7, 0.3]), KontaktFx::SimpleLpHp { lp: 0.7, hp: 0.3 }),
            (record(130, &k(130, 3), 2, &[0.5, 0.25, 0.75, 0.125, 0.0]), KontaktFx::DualSkf([0.25, 0.125, 0.5, 0.75, 0.5])),
            (
                record(141, &monark, 3, &[0.5, 0.25, 2.5, 1.0, 0.0]),
                KontaktFx::Monark { mode: 1, variant: 2, knobs: [0.5, 0.25, 0.5, 0.5, 1.0] },
            ),
        ] {
            let built = chain(std::slice::from_ref(&fx), Scope::Voice);
            assert!(
                built.processors.iter().any(|p| matches!(p, sampler_ir::Processor::FilterBank(c) if c.variant == Variant::Kontakt(expected))),
                "{expected:?}: {:?}",
                built.processors
            );
        }
    }

    #[test]
    fn ladder_and_daft_cutoff_routes_retain_the_authored_physical_slot() {
        for kind in [33i32, 70, 71] {
            let mut public = kind.to_le_bytes().to_vec();
            if kind == 33 {
                public.push(0);
            }
            public.extend(kind.to_le_bytes());
            for value in [0.2f32, 0.5, 0.3] {
                public.extend(value.to_le_bytes());
            }
            let slot = crate::effects::Slot {
                slot: 5,
                module: 0x18,
                version: 0x92,
                bypass: false,
                output_gain: 1.,
                dry_level: 0.,
                output_set: false,
                public,
            };
            for dynamic in [None, Some((7, -1))] {
                let chain = crate::effects::chain_with(
                    std::slice::from_ref(&slot),
                    crate::effects::Scope::Voice,
                    None,
                    dynamic,
                    (7, -1),
                    &[],
                );
                let index = usize::from(dynamic.is_some());
                assert_eq!(
                    chain.filter_slots,
                    vec![(5, index)],
                    "native filter {kind} loses its addressed cutoff consumer"
                );
                assert!(matches!(
                    chain.processors[index],
                    sampler_ir::Processor::LadderLP4(_) | sampler_ir::Processor::Daft(_)
                ));
            }
        }
    }

    #[test]
    fn native_ladder_lp4_import_preserves_signed_gain_and_record_version() {
        let mut public = 33i32.to_le_bytes().to_vec();
        public.push(0);
        public.extend(33i32.to_le_bytes());
        for value in [-0.25f32, 0.5, 0.3] {
            public.extend(value.to_le_bytes());
        }
        let slot = crate::effects::Slot {
            slot: 3,
            module: 0x18,
            version: 0x92,
            bypass: false,
            output_gain: 1.,
            dry_level: 0.,
            output_set: false,
            public,
        };
        let chain = crate::effects::chain_with(
            &[slot],
            crate::effects::Scope::Voice,
            None,
            Some((7, -1)),
            (7, -1),
            &[],
        );
        assert!(only_law_notes(&chain.notes), "{:?}", chain.notes);
        assert!(chain.processors.iter().any(|p| matches!(p,
            sampler_ir::Processor::LadderLP4(d) if d.gain == -0.25
                && d.cutoff == 0.5 && (d.resonance - 0.3).abs() < 1e-7
                && d.record_version == 0x92)));
    }

    #[test]
    fn morphology_ladder_lp1_record_selects_the_first_stage() {
        // A saved v0x95 type-30 record: kind repeats, then gain 0, cutoff 1.0, resonance 0.
        let hex = "1e000000001e0000001e0000001e000000001e00000000000000" .to_owned()
            + "0000803f00000000";
        let public: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let slot = crate::effects::Slot {
            slot: 2,
            module: 0x18,
            version: 0x95,
            bypass: false,
            output_gain: 1.,
            dry_level: 0.,
            output_set: false,
            public,
        };
        let chain = crate::effects::chain_with(&[slot], crate::effects::Scope::Voice, None, Some((7, -1)), (7, -1), &[]);
        assert!(only_law_notes(&chain.notes), "{:?}", chain.notes);
        assert!(chain.processors.iter().any(|p| matches!(p,
            sampler_ir::Processor::LadderLP4(d) if d.taps == 1 && d.cutoff == 1.0
                && d.resonance == 0.0 && d.record_version == 0x95)));
    }

    #[test]
    fn adaptive_resonance_records_become_native_ar_modes() {
        for (kind, mode) in [(100i32, 0u8), (106, 6), (108, 8)] {
            let mut public = [kind.to_le_bytes(), kind.to_le_bytes()].concat();
            for value in [0.5135f32, 0.25] {
                public.extend(value.to_le_bytes());
            }
            let slot = crate::effects::Slot {
                slot: 4,
                module: 0x18,
                version: 0x92,
                bypass: false,
                output_gain: 1.,
                dry_level: 0.,
                output_set: false,
                public,
            };
            let chain = crate::effects::chain_with(&[slot], crate::effects::Scope::Voice, None, None, (7, -1), &[]);
            assert!(chain.notes.is_empty(), "{:?}", chain.notes);
            assert_eq!(chain.filter_slots, vec![(4, 0)]);
            assert!(matches!(chain.processors[0],
                sampler_ir::Processor::Ar(d) if d.mode == mode && d.cutoff == f64::from(0.5135f32)
                    && d.resonance == 0.25));
        }
    }

    #[test]
    fn legacy_filters_keep_the_authored_slot_and_their_stored_knobs() {
        // Saved types 2 (LP1) and 3 (HP1): the cascade core takes the stored knobs.
        for (kind, cutoff, poles, response) in [
            (2i32, 0.5996094f32, 1, sampler_ir::fx::filter::Response::LowPass),
            (3, 0., 1, sampler_ir::fx::filter::Response::HighPass),
        ] {
            let mut payload = kind.to_le_bytes().repeat(2);
            for value in [cutoff, 0.] {
                payload.extend(value.to_le_bytes());
            }
            for scope in [Scope::Voice, Scope::Bus] {
                let mut saved = slot(0x18, payload.clone(), 1.);
                saved.slot = 5;
                let out = chain(&[saved], scope);
                assert!(
                    !out.notes.iter().any(|(_, _, _, r)| *r == sampler_ir::Reason::NotModeled),
                    "{kind}: {:?}",
                    out.notes
                );
                assert_eq!(out.filter_slots, [(5, 0)]);
                let sampler_ir::Processor::FilterBank(c) = out.processors[0] else {
                    panic!("missing filter")
                };
                assert_eq!(c.topology, sampler_ir::fx::filter::Topology::Cascade);
                assert_eq!((c.response, c.poles), (response, poles));
                assert_eq!(c.cutoff, f64::from(cutoff));
            }
        }
        let sampler_ir::Processor::FilterBank(c) = filter(2, 0.4, 0.7, &[]).unwrap().0[0] else {
            panic!("missing filter")
        };
        assert_eq!((c.cutoff, c.resonance), (f64::from(0.4f32), f64::from(0.7f32)));
    }

    #[test]
    fn sv_filters_are_native_cascade_classes() {
        use sampler_ir::fx::filter::{Topology, Variant};
        let sampler_ir::Processor::FilterBank(c) = filter(52, 0.293, 0.0, &[]).unwrap().0[0] else {
            panic!()
        };
        assert_eq!((c.topology, c.variant, c.cutoff as f32), (Topology::Cascade, Variant::Sv, 0.293));
        let sampler_ir::Processor::FilterBank(c) = filter(65, 0.5, 0.5, &[0.3]).unwrap().0[0] else {
            panic!()
        };
        assert_eq!(c.variant, Variant::MultiSv { mode: 2, bandwidth: 0.3 });
        // Legacy LP4 is a cascade-core class; type 1 is not a filter type.
        assert!(filter(8, 0.5, 0.5, &[]).is_ok());
        assert!(filter(1, 0.5, 0.5, &[]).is_err());
    }

    #[test]
    fn formant_native_parallel_impulse_and_saved_sharp_size_mapping() {
        use sampler_ir as ir;
        for (talk, sharp, size, expected) in [
            (0.0f32, 0.5, 0.5, 0.01663771558f32),
            (0.0, 0.0, 1.0, 0.0361916923f32),
        ] {
            let mut payload = 90i32.to_le_bytes().repeat(2);
            for value in [talk, sharp, size] { payload.extend(value.to_le_bytes()); }
            let built = chain(&[slot(0x18, payload, 1.0)], Scope::Voice);
            assert!(built.notes.is_empty(), "{:?}", built.notes);
            let mut instrument = ir::Instrument::default();
            instrument.assets.push(ir::Asset { residency: Default::default(), location: ir::AssetLocation::Path("probe".into()), encoding: ir::Encoding::Wav, root_key: None, loops: Vec::new() });
            instrument.chains.push(ir::Chain { scope: ir::Scope::Voice, pre_amplitude: built.processors, post_amplitude: vec![] });
            let mut zone = ir::Zone::new(ir::AssetRef(0));
            zone.keys = ir::KeyRange { low: 60, high: 60 };
            zone.chain = Some(ir::ChainRef(0));
            zone.velocity = ir::VelocityResponse::None;
            instrument.zones.push(zone);
            let mut samples = vec![[0.0; 2]; 64]; samples[0] = [1.0, -0.5];
            let pcm = sampler_core::Pcm::new(48000, samples.into_boxed_slice()).unwrap();
            let plan = sampler_core::lower::lower(&instrument, 48000, vec![pcm], |_, plan| Ok(plan)).unwrap();
            let limits = sampler_core::Limits::for_plan(&plan, 4, 4);
            let mut rt = sampler_core::Runtime::new(plan, limits).unwrap();
            rt.trigger(sampler_core::Input { protocol: sampler_core::Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: None }, 60, 1.0).unwrap();
            let mut output = [[0.0; 2]; 8]; rt.render(&mut output).unwrap();
            assert!((output[0][0] - expected).abs() < 1e-7, "{:?} expected {expected}", output[0]);
            assert!((output[0][1] + expected * 0.5).abs() < 1e-7);
        }
    }

    #[test]
    fn v1_formant_slot_is_an_executable_processor_in_both_scopes() {
        let mut payload = 90i32.to_le_bytes().repeat(2);
        for value in [0.25f32, 0.5, 0.5] {
            payload.extend(value.to_le_bytes());
        }
        for scope in [Scope::Voice, Scope::Bus] {
            let out = chain(&[slot(0x18, payload.clone(), 1.)], scope);
            assert!(!out.processors.is_empty(), "v1 executes Formant I");
            assert!(
                !out.notes
                    .iter()
                    .any(|(_, _, _, reason)| *reason == sampler_ir::Reason::NotModeled)
            );
        }
    }

    #[test]
    fn ladder_records_with_flag_bytes_parse_to_their_cutoff() {
        // Conflux (v0x92): kind, flag, kind, then leading, cutoff, resonance.
        // Morphology (v0x95): kind, flag, kind, kind, kind, flag, kind, floats.
        let k = 33i32.to_le_bytes();
        let floats: Vec<u8> = [0.0f32, 0.4, 0.25]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect();
        for layout in [&[&k[..], &[0], &k][..], &[&k, &[0], &k, &k, &k, &[0], &k]] {
            let mut bytes = layout.concat();
            bytes.extend(&floats);
            let built = chain(&[slot(0x18, bytes, 1.0)], Scope::Voice);
            let note = &built.notes[1].2;
            assert_eq!(*note, "33 cutoff 0.4 resonance 0.25", "{:?}", built.notes);
        }
    }

    #[test]
    fn daft_filter_slot_keeps_its_normalized_controls() {
        let mut bytes = Vec::new();
        for _ in 0..2 {
            bytes.extend(71i32.to_le_bytes());
        }
        for x in [0.25f32, 0.5, 0.75] {
            bytes.extend(x.to_le_bytes());
        }
        let built = chain(&[slot(0x18, bytes, 1.0)], Scope::Voice);
        let [sampler_ir::Processor::Daft(d), ..] = built.processors[..] else {
            panic!("{:?} {:?}", built.processors, built.notes)
        };
        assert_eq!(
            (d.gain, d.cutoff, d.resonance, d.highpass),
            (0.25, 0.5, 0.75, true)
        );
    }
}

/// Report a filter that is configured but not fully proved, so a stand-in never passes silently.
fn note_fidelity(notes: &mut Notes, kind: i32, mapped: &Mapped) {
    if mapped.fidelity != sampler_ir::fx::filter::Fidelity::Exact {
        notes.push((
            "filter law".into(),
            format!("{} ({:?}): {}", name(kind).unwrap_or("?"), mapped.fidelity, mapped.caveat),
            sampler_ir::Reason::UnknownLaw,
        ));
    }
}

/// A Filter slot as IR processors, or why its type cannot be configured.
fn filter(
    kind: i32,
    cutoff: f32,
    resonance: f32,
    extra: &[f32],
) -> Result<(Vec<sampler_ir::Processor>, Mapped), Gap> {
    let mapped = map(kind, cutoff, resonance, extra)?;
    // The native kernels (Ladder, AR, Daft, Formant I) are admitted by their own arms, which
    // check the record version, the leading Gain value and the knob ranges.
    if !matches!(mapped.config.topology, Topology::Svf | Topology::FormantII | Topology::Cascade) {
        return Err(Gap {
            name: name(kind).unwrap_or("filter"),
            reason: "the stored record does not meet the native kernel's admission",
        });
    }
    let processors = mapped.config.processors(None)?;
    Ok((processors, mapped))
}


/// Formant I (saved 90) and Formant II (saved 91, the second table of the same core).
pub(super) fn slot_formant(
    b: &mut Lowering,
    s: &Site,
    notes: &mut Notes,
    kind: i32,
    cutoff: f32,
    resonance: f32,
    extra: &[f32],
) -> bool {
    b.flush();
    // Kontakt 8 Filter Reference: Formant I. FUN_1409a80a0 maps
    // Talk/Sharp to core 0/2; FUN_140aeffe0 maps saved Size to core 1.
    // Formant II is the same core on its second table.
    let mut modelled = true;
    let knobs = extra.first().map(|&size| [cutoff, resonance, size]);
    if let Some(knobs) = knobs.filter(|v| v.iter().all(|x| x.is_finite() && (0.0..=1.0).contains(x))) {
        // Formant II with script or modulation lanes takes the live path below on the
        // second table; the static one keeps the filter-bank mapping and its fidelity.
        if kind == 91 && s.dynamic.is_none() {
            match filter(91, knobs[0], knobs[1], &knobs[2..]) {
                Ok((processors, mapped)) => {
                    b.out.processors.extend(processors);
                    note_fidelity(notes, 91, &mapped);
                    b.combined = s.gain;
                }
                Err(gap) => {
                    notes.push((gap.name.into(), gap.reason.into(), Reason::NotModeled));
                    modelled = false;
                }
            }
        } else if let Some((group, generic)) = s.dynamic {
            b.out.lane_slots.push((s.fx.slot, b.out.processors.len()));
            b.out.processors.push(Processor::LiveFormant {
                knobs,
                second: kind == 91,
                address: SlotAddress { group, slot: s.fx.slot as i32, generic },
            });
            b.combined = s.gain;
        } else {
            b.out.processors.push(Processor::Formant(knobs));
            b.combined = s.gain;
        }
    } else {
        notes.push((
            "Formant parameters".into(),
            "missing Size or outside normalized range".into(),
            Reason::InvalidValue,
        ));
        modelled = false;
    }
    modelled
}

/// Ladder filters (saved 30..=41): the native Ladder rows. Gain is signed
/// normalized (12 dB per unit); the native kernel retains the record-version cutoff clock.
pub(super) fn slot_ladder(
    b: &mut Lowering,
    s: &Site,
    notes: &mut Notes,
    kind: i32,
    cutoff: f32,
    resonance: f32,
    extra: &[f32],
) -> bool {
    // Kontakt 8 Filter Reference: Ladder filters. FUN_140cf4c80
    // maps saved 30..=41 to the 12 rows in FUN_140b00b80.
    let values = [extra[0], cutoff, resonance];
    if values.iter().all(|v| v.is_finite())
        && (-1.0..=1.0).contains(&values[0])
        && values[1..].iter().all(|v| (0.0..=1.0).contains(v))
    {
        b.out.filter_slots.push((s.fx.slot, b.out.processors.len() + b.filters.len()));
        let mapped = template(kind).expect("ladder rows are in the table");
        let mut config = mapped.config;
        config.drive = f64::from(values[0]);
        config.cutoff = f64::from(values[1]);
        config.resonance = f64::from(values[2]);
        config.binding = Some(sampler_ir::fx::filter::Binding {
            address: sampler_ir::SlotAddress {
                group: s.physical.0,
                slot: s.fx.slot as i32,
                generic: s.physical.1,
            },
            record_version: s.fx.version,
        });
        b.filters.extend(config.processors(None).expect("ladder rows configure"));
        note_fidelity(notes, kind, &mapped);
        b.combined = product(s.gain, b.combined);
        true
    } else {
        notes.push((
            "Ladder LP4 parameters".into(),
            "non-finite or outside native range".into(),
            Reason::InvalidValue,
        ));
        false
    }
}

/// Adaptive-resonance family: saved 100..=108 are modes 0..=8 of the native
/// FilterDJ (docs/audit-2026-10-10/w15-ar-native.md). The kernel is checked against
/// original-instruction vectors for all nine modes; host recording acceptance still
/// requires calibrated paired captures.
pub(super) fn slot_ar(b: &mut Lowering, s: &Site, notes: &mut Notes, kind: i32, cutoff: f32, resonance: f32) -> bool {
    b.out.filter_slots.push((s.fx.slot, b.out.processors.len() + b.filters.len()));
    let mapped = template(kind).expect("AR rows are in the table");
    let mut config = mapped.config;
    config.cutoff = f64::from(cutoff);
    config.resonance = f64::from(resonance);
    b.filters.extend(config.processors(None).expect("AR rows configure"));
    note_fidelity(notes, kind, &mapped);
    b.combined = product(s.gain, b.combined);
    true
}

/// Daft (stored 70 low pass, 71 high pass): DSP_SYSTEM_INVENTORY "Daft parameter
/// laws and scheduling". The leading value is the Gain control; modulation adds
/// to the saved normalized knob.
// ponytail: unverified - 70/71 as Daft rests on v1's stored-ID table.
pub(super) fn slot_daft(b: &mut Lowering, s: &Site, notes: &mut Notes, kind: i32, cutoff: f32, resonance: f32, extra: &[f32]) -> bool {
    b.out.filter_slots.push((s.fx.slot, b.out.processors.len() + b.filters.len()));
    let mapped = template(kind).expect("Daft rows are in the table");
    let mut config = mapped.config;
    config.drive = f64::from(extra[0]).clamp(0.0, 1.0);
    config.cutoff = f64::from(cutoff).clamp(0.0, 1.0);
    config.resonance = f64::from(resonance).clamp(0.0, 1.0);
    b.filters.extend(config.processors(None).expect("Daft rows configure"));
    note_fidelity(notes, kind, &mapped);
    b.combined = product(s.gain, b.combined);
    true
}

/// Every other stored filter type, through the shared `map` table.
pub(super) fn slot_filter(b: &mut Lowering, s: &Site, notes: &mut Notes, kind: i32, cutoff: f32, resonance: f32, extra: &[f32]) -> bool {
    match filter(
        kind,
        cutoff,
        resonance,
        match kind {
            63..=65 => &extra[..extra.len().min(1)],
            19 | 120 | 130 | 140..=143 => extra,
            _ => &[],
        },
    ) {
        Ok((processors, mapped)) => {
            b.out.filter_slots.push((s.fx.slot, b.out.processors.len() + b.filters.len()));
            b.filters.extend(processors);
            note_fidelity(notes, kind, &mapped);
            b.combined = product(s.gain, b.combined);
            true
        }
        Err(gap) => {
            notes.push((
                format!("filter type ({}: {})", gap.name, gap.reason),
                format!("{kind} cutoff {cutoff} resonance {resonance}"),
                Reason::NotModeled,
            ));
            false
        }
    }
}
