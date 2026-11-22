//! Timed modulation translation and its law provenance.

use kontra_format::Notes;
use kontra_ir as ir;
use ni_file::kontakt::objects::{InternalModParams, Modulator};

use crate::laws;

/// The bypass flag among a modulator's flags.
const BYPASS: usize = 1;

/// Records the selected law and its input and output when tracing is enabled.
fn trace_timed(
    id: &'static str,
    group: usize,
    slot: u16,
    field: &str,
    input: &impl std::fmt::Debug,
    output: &impl std::fmt::Debug,
) {
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId(id),
        location: format!("groups/{group}/routes/{slot}/{field}"),
        input: format!("{input:?}"),
        output: format!("{output:?}"),
    });
}

/// Routes a timed envelope or LFO and reports unmapped targets and cutoff slots.
/// `skip_target`, when set, omits one target index (used when the primary
/// amplitude AHDSR already handles its first `volume` target via
/// `ir::ModKind::Envelope` and routes any secondary targets here).
pub(crate) fn timed(
    out: &mut ir::Group,
    p: &InternalModParams,
    group_index: usize,
    slot: u16,
    skip_target: Option<usize>,
    notes: &mut Notes,
) {
    // The modulator's flags: [router open, bypass, retrigger, unknown].
    const RETRIGGER: usize = 2;
    let report = |notes: &mut Notes, what: &str| {
        for (i, t) in p.targets.iter().enumerate() {
            if skip_target == Some(i) {
                continue;
            }
            let feature = format!("internal modulator {what} -> {}", t.param);
            notes.add(ir::Impact::Audible, &feature, &p.name);
        }
    };
    let mut envelope = |e: ir::Breakpoints| {
        let n = u8::try_from(out.envelopes.len()).ok()?;
        out.envelopes.push(e);
        Some(ir::Source::Envelope(n))
    };
    let (raw_source, what, pitch_semitones) = match &p.modulator {
        Modulator::Flex(e) => {
            if e.unknown_version_word().is_some_and(|w| w & 0xFF00 != 0) {
                notes.add(
                    ir::Impact::Audible,
                    "flex envelope release-adjustment bypass",
                    &p.name,
                );
            }
            let result = laws::flex(e);
            trace_timed(
                "kontakt.flex_envelope.v1",
                group_index,
                slot,
                "envelope",
                e,
                &result,
            );
            (envelope(result), "flex envelope", laws::PITCH_SEMITONES)
        }
        Modulator::Ahdsr(e) => {
            let result = laws::ahdsr(e);
            trace_timed(
                "kontakt.breakpoint_ahdsr.v1",
                group_index,
                slot,
                "envelope",
                e,
                &result,
            );
            (envelope(result), "AHDSR", laws::PITCH_SEMITONES)
        }
        Modulator::Dbd(e) => {
            let result = laws::dbd(e);
            trace_timed(
                "kontakt.dbd_envelope.v1",
                group_index,
                slot,
                "envelope",
                e,
                &result,
            );
            // DBD pitch routes scale by `9.0` instead of `3.0` (`0140870000.c:3972`),
            // i.e., 3× standard pitch travel (±36 st) on the normalized `0..=1` shape.
            (
                envelope(result),
                "DBD envelope",
                3.0 * laws::PITCH_SEMITONES,
            )
        }
        Modulator::Lfo(l) => {
            let clock = if p.unknown_flags.get(RETRIGGER).is_some_and(|&f| f != 0) {
                ir::Clock::Voice
            } else {
                ir::Clock::Free
            };
            let result = laws::lfo(l, clock);
            trace_timed(
                "kontakt.lfo.v1",
                group_index,
                slot,
                "lfo",
                &(l, clock),
                &result,
            );
            let (Some(lfo), Ok(n)) = (result, u8::try_from(out.lfos.len())) else {
                return report(notes, "LFO (synced fade or unknown wave)");
            };
            out.lfos.push(lfo);
            (Some(ir::Source::Lfo(n)), "LFO", laws::PITCH_SEMITONES)
        }
        _ => return,
    };
    let Some(raw_source) = raw_source else {
        return report(notes, what);
    };
    let flex = what == "flex envelope";
    let amp = out
        .mods
        .iter()
        .any(|m| matches!(m.kind, ir::ModKind::Envelope));
    for (target, t) in p.targets.iter().enumerate() {
        if skip_target == Some(target) {
            continue;
        }
        // Another modulator's knob: `knob_routes`, once every slot is translated.
        if knob_route(t) {
            continue;
        }
        let source = match t.shaper.as_ref().filter(|s| s.enabled) {
            Some(shaper) => {
                let signed = matches!(raw_source, ir::Source::Lfo(_))
                    && crate::external::signed_route(&t.param);
                let result =
                    crate::external::shaped(&mut out.shapers, raw_source, &shaper.curve, signed);
                kontra_law::record(|| kontra_law::Application {
                    id: kontra_law::LawId("kontakt.mod_shaper.v1"),
                    location: format!("groups/{group_index}/routes/{slot}/targets/{target}/shaper"),
                    input: format!("source={raw_source:?}, curve={:?}", shaper.curve),
                    output: format!(
                        "source={result:?}, transfer={:?}",
                        result.and_then(|id| match id {
                            ir::Source::Shaped(i) => out.shapers.get(usize::from(i)),
                            _ => None,
                        })
                    ),
                });
                let Some(shaped_source) = result else {
                    let to = if t.slot.is_some() { "module " } else { "" };
                    let feature = format!("internal modulator {what} -> {to}{}", t.param);
                    notes.add(ir::Impact::Audible, &feature, &p.name);
                    continue;
                };
                shaped_source
            }
            None => raw_source,
        };
        let tau_ms = laws::route_tau_ms(t.lag_ms);
        let kind = match (t.param.as_str(), t.slot) {
            // An LFO's volume route is linear in its unipolar swing (v2
            // sampler-core v1_voice_controls volume target; the range conversion is not yet isolated).
            ("volume", None) => {
                crate::external::record_lag(
                    group_index,
                    usize::from(slot),
                    target,
                    t.lag_ms,
                    tau_ms,
                );
                ir::ModKind::Volume {
                    source,
                    intensity: t.signed_intensity(),
                    curve: ir::Curve::Depth { power: 1 },
                    tau_ms,
                }
            }
            ("pitch", None) => {
                crate::external::record_lag(
                    group_index,
                    usize::from(slot),
                    target,
                    t.lag_ms,
                    tau_ms,
                );
                ir::ModKind::Pitch {
                    source,
                    semitones: pitch_semitones,
                    depth: t.signed_intensity(),
                    tau_ms,
                }
            }
            ("pan", None) => ir::ModKind::Pan {
                source,
                amount: t.signed_intensity(),
            },
            ("filterCutoff", Some(insert)) => ir::ModKind::Cutoff {
                source,
                insert,
                depth: t.signed_intensity(),
            },
            ("filterQ", Some(insert)) => ir::ModKind::Resonance {
                source,
                insert,
                depth: t.signed_intensity(),
            },
            (param, None) if let Some(value) = crate::wavetable::target(param) => {
                ir::ModKind::Wavetable {
                    source,
                    value,
                    depth: t.signed_intensity(),
                }
            }
            (param, Some(insert))
                if crate::provenance::eq_band_index(
                    param,
                    group_index,
                    usize::from(slot),
                    target,
                )
                .is_some() =>
            {
                let result = laws::eq_gain(source, insert, param, t.signed_intensity());
                kontra_law::record(|| kontra_law::Application {
                    id: kontra_law::LawId("kontakt.eq_gain_route.v1"),
                    location: format!("groups/{group_index}/routes/{slot}/targets/{target}"),
                    input: format!("{:?}", (source, insert, param, t.signed_intensity())),
                    output: format!("{result:?}"),
                });
                match result {
                    Some(kind) => kind,
                    None => continue,
                }
            }
            (param, Some(insert))
                if let Some(kind) =
                    crate::drive::route(source, insert, param, t.signed_intensity()) =>
            {
                kind
            }
            _ => {
                let to = if t.slot.is_some() { "module " } else { "" };
                let feature = format!("internal modulator {what} -> {to}{}", t.param);
                notes.add(ir::Impact::Audible, &feature, &p.name);
                continue;
            }
        };
        if t.lag_ms < 0 {
            notes.add(ir::Impact::Audible, "negative modulation lag", &p.name);
        }
        let bypassed = p.unknown_flags.get(BYPASS).is_some_and(|&f| f != 0);
        if flex
            && !amp
            && !bypassed
            && matches!(kind, ir::ModKind::Volume { .. })
            && let Some(e) = out.envelopes.last()
        {
            out.envelope = laws::flex_gate(e);
            trace_timed(
                "kontakt.flex_gate.v1",
                group_index,
                slot,
                "amp_envelope",
                e,
                &out.envelope,
            );
        }
        out.mods.push(ir::Mod {
            name: p.name.clone(),
            slot,
            target: u16::try_from(target).unwrap_or(u16::MAX),
            bypass: bypassed,
            kind,
        });
    }
}

/// Whether a target is another modulator's `intensity` or `frequency`.
pub(crate) fn knob_route(t: &ni_file::kontakt::objects::ModTarget) -> bool {
    matches!(t.param.as_str(), "intensity" | "frequency") && t.slot.is_some()
}

/// Each slot's routes to another modulator's knob, now that every slot's source is known.
pub(crate) fn knob_routes(
    out: &mut ir::Group,
    knobs: &[(usize, InternalModParams)],
    timed_slots: &[(usize, ir::Source)],
    notes: &mut Notes,
) {
    for (n, p) in knobs {
        let Some(&(_, raw_source)) = timed_slots.iter().find(|(s, _)| s == n) else {
            continue;
        };
        for (target, t) in p.targets.iter().enumerate().filter(|(_, t)| knob_route(t)) {
            let shapers_before = out.shapers.len();
            let source = match t.shaper.as_ref().filter(|s| s.enabled) {
                Some(shaper) => {
                    let signed = matches!(raw_source, ir::Source::Lfo(_))
                        && crate::external::signed_route(&t.param);
                    match crate::external::shaped(
                        &mut out.shapers,
                        raw_source,
                        &shaper.curve,
                        signed,
                    ) {
                        Some(s) => s,
                        None => {
                            notes.add(
                                ir::Impact::Audible,
                                "internal modulator -> module knob",
                                &p.name,
                            );
                            continue;
                        }
                    }
                }
                None => raw_source,
            };
            let at = t.slot.unwrap_or(u8::MAX);
            match crate::external::modulator_route(source, &t.param, at, t, timed_slots) {
                Some(kind) => {
                    if t.lag_ms < 0 {
                        notes.add(ir::Impact::Audible, "negative modulation lag", &p.name);
                    }
                    out.mods.push(ir::Mod {
                        name: p.name.clone(),
                        slot: u16::try_from(*n).unwrap_or(u16::MAX),
                        target: u16::try_from(target).unwrap_or(u16::MAX),
                        bypass: p.unknown_flags.get(BYPASS).is_some_and(|&f| f != 0),
                        kind,
                    });
                }
                None => {
                    out.shapers.truncate(shapers_before);
                    notes.add(
                        ir::Impact::Audible,
                        "internal modulator -> module knob",
                        &p.name,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ni_file::kontakt::objects::{
        Breakpoint, EnvelopeAhdsr, EnvelopeDbd, ModShaper, ModTarget, ShaperCurve,
    };

    #[test]
    fn rejected_knob_route_does_not_exhaust_shaper_slots() {
        let curve = ShaperCurve::Breakpoints(vec![
            Breakpoint {
                x: 0.0,
                y: 0.0,
                curve: 0.0,
            },
            Breakpoint {
                x: 1.0,
                y: 1.0,
                curve: 0.0,
            },
        ]);
        let shape = Some(ModShaper {
            enabled: true,
            curve: curve.clone(),
        });
        let mut group = ir::Group::DEFAULT;
        for _ in 0..255 {
            assert!(
                crate::external::shaped(&mut group.shapers, ir::Source::Constant, &curve, false)
                    .is_some()
            );
        }
        let p = InternalModParams {
            name: "test".into(),
            targets: vec![
                target("frequency", Some(9), 0, shape.clone()),
                target("frequency", Some(1), 0, shape),
            ],
            modulator: Modulator::Dbd(EnvelopeDbd {
                version: 0x12,
                values: [0.0; 3],
                extra: None,
                records: None,
                flag: Some(0),
            }),
            unknown_flags: [0; 4],
            unknown_id: 0,
        };
        let mut notes = Notes::default();
        knob_routes(
            &mut group,
            &[(0, p)],
            &[(0, ir::Source::Envelope(0)), (1, ir::Source::Lfo(0))],
            &mut notes,
        );
        assert_eq!(group.shapers.len(), 256);
        assert_eq!(group.mods.len(), 1);
        assert!(matches!(
            group.mods[0].kind,
            ir::ModKind::LfoRate {
                source: ir::Source::Shaped(255),
                ..
            }
        ));
        assert!(
            notes
                .finish()
                .iter()
                .any(|d| d.feature == "internal modulator -> module knob")
        );
    }

    fn target(param: &str, slot: Option<u8>, lag_ms: i16, shaper: Option<ModShaper>) -> ModTarget {
        ModTarget {
            param: param.into(),
            intensity: 0.5,
            lag_ms,
            name: String::new(),
            slot,
            invert: false,
            shaper,
            unknown_i16: -1,
            unknown_flags: 0,
        }
    }

    #[test]
    fn timed_applies_shaper_lag_dbd_pitch_and_skip_target() {
        let flat_mid = ModShaper {
            enabled: true,
            curve: ShaperCurve::Breakpoints(vec![
                Breakpoint {
                    x: 0.0,
                    y: 0.25,
                    curve: 0.0,
                },
                Breakpoint {
                    x: 1.0,
                    y: 0.25,
                    curve: 0.0,
                },
            ]),
        };
        let mut group = ir::Group::DEFAULT;
        let mut notes = Notes::default();
        let p = InternalModParams {
            name: "DBD_MOD".into(),
            targets: vec![
                target("pitch", None, 100, Some(flat_mid)),
                target("volume", None, -10, None),
            ],
            modulator: Modulator::Dbd(EnvelopeDbd {
                version: 0x12,
                values: [50.0, 0.2, 100.0],
                extra: None,
                records: None,
                flag: Some(0),
            }),
            unknown_flags: [0; 4],
            unknown_id: 0,
        };
        timed(&mut group, &p, 0, 0, None, &mut notes);
        assert_eq!(group.envelopes.len(), 1);
        assert_eq!(group.shapers.len(), 1);
        assert_eq!(group.mods.len(), 2);
        match group.mods[0].kind {
            ir::ModKind::Pitch {
                source,
                semitones,
                tau_ms,
                ..
            } => {
                assert_eq!(source, ir::Source::Shaped(0));
                assert!((semitones - 36.0).abs() < 1e-5);
                assert!(tau_ms > 20.0, "{tau_ms}");
            }
            other => panic!("expected pitch route, got {other:?}"),
        }
        match group.mods[1].kind {
            ir::ModKind::Volume { tau_ms, .. } => assert_eq!(tau_ms, 0.0),
            other => panic!("expected volume route, got {other:?}"),
        }
        let diags = notes.finish();
        assert!(diags.iter().any(|d| d.feature == "negative modulation lag"));

        // Primary amp AHDSR secondary targets: skip_target = Some(0) routes only target 1.
        let mut g2 = ir::Group::DEFAULT;
        let mut notes2 = Notes::default();
        let ahdsr = InternalModParams {
            name: "AMP_ENV".into(),
            targets: vec![
                target("volume", None, 0, None),
                target("filterCutoff", Some(2), 0, None),
            ],
            modulator: Modulator::Ahdsr(EnvelopeAhdsr {
                attack_curve: 0.0,
                attack_ms: 10.0,
                hold_ms: 0.0,
                decay_ms: 50.0,
                sustain: 0.5,
                release_ms: 100.0,
                unknown_flag: 0,
                unknown_tail: Vec::new(),
            }),
            unknown_flags: [0, 1, 0, 0], // bypass set
            unknown_id: 0,
        };
        timed(&mut g2, &ahdsr, 0, 0, Some(0), &mut notes2);
        assert_eq!(g2.mods.len(), 1);
        assert!(g2.mods[0].bypass);
        assert!(matches!(
            g2.mods[0].kind,
            ir::ModKind::Cutoff { insert: 2, .. }
        ));
    }

    #[test]
    fn bypassed_flex_does_not_overwrite_amp_envelope_and_high_byte_is_diagnosed() {
        let flex = ni_file::kontakt::objects::EnvelopeFlex {
            points: vec![
                ni_file::kontakt::objects::FlexPoint {
                    time_ms: 0.0,
                    level: 0.0,
                    curve: 0.0,
                },
                ni_file::kontakt::objects::FlexPoint {
                    time_ms: 50.0,
                    level: 1.0,
                    curve: 0.0,
                },
                ni_file::kontakt::objects::FlexPoint {
                    time_ms: 200.0,
                    level: 0.0,
                    curve: 0.0,
                },
            ],
            sustain: 1,
            unknown_index: 0,
            // v0x12 tail (15 bytes): high byte 0x01 in leading u16 triggers diagnostic.
            unknown_tail: vec![0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        };
        let mut group = ir::Group::DEFAULT;
        let original_env = group.envelope;
        let mut notes = Notes::default();
        let p = InternalModParams {
            name: "FLEX_BYPASSED".into(),
            targets: vec![target("volume", None, 0, None)],
            modulator: Modulator::Flex(flex),
            unknown_flags: [0, 1, 0, 0], // bypassed
            unknown_id: 0,
        };
        timed(&mut group, &p, 0, 0, None, &mut notes);
        assert_eq!(group.envelope, original_env);
        assert!(
            notes
                .finish()
                .iter()
                .any(|d| d.feature == "flex envelope release-adjustment bypass")
        );
    }
}
