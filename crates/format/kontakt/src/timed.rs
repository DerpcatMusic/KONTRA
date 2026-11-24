//! Timed modulation translation and its law provenance.

use kontra_format::Notes;
use kontra_ir as ir;
use ni_file::kontakt::objects::{InternalModParams, Modulator, ShaperCurve, StepMod};

use crate::laws;

/// The bypass flag among a modulator's flags.
const BYPASS: usize = 1;
/// The retrigger flag among a modulator's flags (`+0xab8`, read only by LFO & StepMod).
const RETRIGGER: usize = 2;

fn clock_of(p: &InternalModParams) -> ir::Clock {
    if p.unknown_flags.get(RETRIGGER).is_some_and(|&f| f != 0) {
        ir::Clock::Voice
    } else {
        ir::Clock::Free
    }
}

/// Deduplicated stepped transfer table for a looping StepMod target.
fn step_mod_transfer(
    shapers: &mut Vec<ir::Transfer>,
    lfo_source: ir::Source,
    step: &StepMod,
    curve: Option<&ShaperCurve>,
    signed: bool,
) -> Option<ir::Source> {
    let table = laws::step_mod_table(step, curve, signed)?;
    if let Some(i) = shapers
        .iter()
        .position(|t| t.source == lfo_source && t.stepped && t.smooth_s == 0.0 && t.table == table)
    {
        return u8::try_from(i).ok().map(ir::Source::Shaped);
    }
    let n = u8::try_from(shapers.len()).ok()?;
    shapers.push(ir::Transfer {
        source: lfo_source,
        table,
        smooth_s: 0.0,
        stepped: true,
    });
    Some(ir::Source::Shaped(n))
}

/// Deduplicated unipolar `0..=1` one-shot StepMod breakpoints envelope.
fn step_mod_oneshot_env(
    envelopes: &mut Vec<ir::Breakpoints>,
    step: &StepMod,
    curve: Option<&ShaperCurve>,
) -> Option<ir::Source> {
    let env = laws::step_mod_oneshot(step, curve)?;
    if let Some(i) = envelopes.iter().position(|e| e == &env) {
        return u8::try_from(i).ok().map(ir::Source::Envelope);
    }
    let n = u8::try_from(envelopes.len()).ok()?;
    envelopes.push(env);
    Some(ir::Source::Envelope(n))
}

/// Deduplicated one-shot StepMod source: keeps `ir::Breakpoints` strictly in
/// `0..=1` and maps signed targets onto `-1..=1` through a linear transfer.
fn step_mod_envelope(
    out: &mut ir::Group,
    step: &StepMod,
    curve: Option<&ShaperCurve>,
    signed: bool,
) -> Option<ir::Source> {
    let base = step_mod_oneshot_env(&mut out.envelopes, step, curve)?;
    if !signed {
        return Some(base);
    }
    let table = vec![-1.0, 1.0];
    if let Some(i) = out
        .shapers
        .iter()
        .position(|t| t.source == base && !t.stepped && t.smooth_s == 0.0 && t.table == table)
    {
        return u8::try_from(i).ok().map(ir::Source::Shaped);
    }
    let n = u8::try_from(out.shapers.len()).ok()?;
    out.shapers.push(ir::Transfer {
        source: base,
        table,
        smooth_s: 0.0,
        stepped: false,
    });
    Some(ir::Source::Shaped(n))
}

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
/// `ir::ModRoute::AMP_ENVELOPE` and routes any secondary targets here).
pub(crate) fn timed(
    out: &mut ir::Group,
    p: &InternalModParams,
    group_index: usize,
    slot: u16,
    skip_target: Option<usize>,
    notes: &mut Notes,
) -> Option<ir::Source> {
    let report = |notes: &mut Notes, what: &str| -> Option<ir::Source> {
        for (i, t) in p.targets.iter().enumerate() {
            if skip_target == Some(i) {
                continue;
            }
            let feature = format!("internal modulator {what} -> {}", t.param);
            notes.add(ir::Impact::Audible, &feature, &p.name);
        }
        None
    };
    let mut envelope = |e: ir::Breakpoints| {
        let n = u8::try_from(out.envelopes.len()).ok()?;
        out.envelopes.push(e);
        Some(ir::Source::Envelope(n))
    };
    let mut step_mod_oneshot_kind = false;
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
            // DBD pitch routes scale by `9.0` instead of `3.0`,
            // i.e., 3× standard pitch travel (±36 st) on the normalized `0..=1` shape.
            (
                envelope(result),
                "DBD envelope",
                3.0 * laws::PITCH_SEMITONES,
            )
        }
        Modulator::Lfo(l) => {
            let clock = clock_of(p);
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
        Modulator::StepMod(s) => {
            let clock = clock_of(p);
            if s.single_trigger {
                if clock != ir::Clock::Voice {
                    return report(notes, "step modulator (synced or free one-shot)");
                }
                let Some(base_source) = step_mod_oneshot_env(&mut out.envelopes, s, None) else {
                    return report(notes, "step modulator (synced or free one-shot)");
                };
                step_mod_oneshot_kind = true;
                trace_timed(
                    "kontakt.step_mod.v1",
                    group_index,
                    slot,
                    "step_mod",
                    &(s, clock),
                    &"oneshot",
                );
                // Return the canonical unipolar envelope source so knob-only and
                // reused-envelope slots always register in `timed_slots`.
                (Some(base_source), "step modulator", laws::PITCH_SEMITONES)
            } else {
                let result = laws::step_mod_lfo(s, clock);
                trace_timed(
                    "kontakt.step_mod.v1",
                    group_index,
                    slot,
                    "step_mod",
                    &(s, clock),
                    &result,
                );
                let (Some(lfo), Ok(n)) = (result, u8::try_from(out.lfos.len())) else {
                    return report(notes, "step modulator");
                };
                out.lfos.push(lfo);
                (
                    Some(ir::Source::Lfo(n)),
                    "step modulator",
                    laws::PITCH_SEMITONES,
                )
            }
        }
        _ => return None,
    };
    let Some(raw_source) = raw_source else {
        return report(notes, what);
    };
    let flex = what == "flex envelope";
    let amp = out.mods.iter().any(|m| {
        matches!(
            m.route,
            ir::ModRoute {
                target: ir::ParamAddr::AmpEnvelope,
                ..
            }
        )
    });
    for (target, t) in p.targets.iter().enumerate() {
        if skip_target == Some(target) {
            continue;
        }
        // Another modulator's knob: `knob_routes`, once every slot is translated.
        if knob_route(t) {
            continue;
        }
        let source = if let Modulator::StepMod(s) = &p.modulator {
            let signed = crate::external::signed_route(&t.param);
            let curve = t
                .shaper
                .as_ref()
                .filter(|sh| sh.enabled)
                .map(|sh| &sh.curve);
            let built = if step_mod_oneshot_kind {
                step_mod_envelope(out, s, curve, signed)
            } else {
                step_mod_transfer(&mut out.shapers, raw_source, s, curve, signed)
            };
            let Some(step_source) = built else {
                let to = if t.slot.is_some() { "module " } else { "" };
                let feature = format!("internal modulator {what} -> {to}{}", t.param);
                notes.add(ir::Impact::Audible, &feature, &p.name);
                continue;
            };
            step_source
        } else {
            match t.shaper.as_ref().filter(|s| s.enabled) {
                Some(shaper) => {
                    let signed = matches!(raw_source, ir::Source::Lfo(_))
                        && crate::external::signed_route(&t.param);
                    let result = crate::external::shaped(
                        &mut out.shapers,
                        raw_source,
                        &shaper.curve,
                        signed,
                    );
                    kontra_law::record(|| kontra_law::Application {
                        id: kontra_law::LawId("kontakt.mod_shaper.v1"),
                        location: format!(
                            "groups/{group_index}/routes/{slot}/targets/{target}/shaper"
                        ),
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
            }
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
                ir::ModRoute::new(source, ir::ParamAddr::Volume, t.signed_intensity())
                    .law(ir::RouteLaw::Gain(ir::Curve::Depth { power: 1 }))
                    .smooth(tau_ms)
            }
            ("pitch", None) => {
                crate::external::record_lag(
                    group_index,
                    usize::from(slot),
                    target,
                    t.lag_ms,
                    tau_ms,
                );
                ir::ModRoute::new(source, ir::ParamAddr::Pitch, t.signed_intensity())
                    .span(pitch_semitones)
                    .smooth(tau_ms)
            }
            ("pan", None) => ir::ModRoute::new(source, ir::ParamAddr::Pan, t.signed_intensity()),
            ("filterCutoff", Some(insert)) => ir::ModRoute::new(
                source,
                ir::ParamAddr::FilterCutoff { insert },
                t.signed_intensity(),
            ),
            ("filterQ", Some(insert)) => ir::ModRoute::new(
                source,
                ir::ParamAddr::FilterResonance { insert },
                t.signed_intensity(),
            ),
            (param, None) if let Some(value) = crate::wavetable::target(param) => {
                ir::ModRoute::new(
                    source,
                    ir::ParamAddr::Wavetable(value),
                    t.signed_intensity(),
                )
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
            && matches!(
                kind,
                ir::ModRoute {
                    target: ir::ParamAddr::Volume,
                    ..
                }
            )
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
            route: kind,
        });
    }
    Some(raw_source)
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
            let envelopes_before = out.envelopes.len();
            let source = if let Modulator::StepMod(s) = &p.modulator {
                let signed = crate::external::signed_route(&t.param);
                let curve = t
                    .shaper
                    .as_ref()
                    .filter(|sh| sh.enabled)
                    .map(|sh| &sh.curve);
                let built = if s.single_trigger {
                    step_mod_envelope(out, s, curve, signed)
                } else {
                    step_mod_transfer(&mut out.shapers, raw_source, s, curve, signed)
                };
                match built {
                    Some(src) => src,
                    None => {
                        notes.add(
                            ir::Impact::Audible,
                            "internal modulator -> module knob",
                            &p.name,
                        );
                        continue;
                    }
                }
            } else {
                match t.shaper.as_ref().filter(|s| s.enabled) {
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
                }
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
                        route: kind,
                    });
                }
                None => {
                    out.shapers.truncate(shapers_before);
                    out.envelopes.truncate(envelopes_before);
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
            group.mods[0].route,
            ir::ModRoute {
                target: ir::ParamAddr::LfoParam {
                    param: ir::LfoParam::Rate,
                    ..
                },
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
            table_open: false,
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
        match group.mods[0].route {
            ir::ModRoute {
                target: ir::ParamAddr::Pitch,
                source,
                span: semitones,
                smooth_ms: tau_ms,
                ..
            } => {
                assert_eq!(source, ir::Source::Shaped(0));
                assert!((semitones - 36.0).abs() < 1e-5);
                assert!(tau_ms > 20.0, "{tau_ms}");
            }
            other => panic!("expected pitch route, got {other:?}"),
        }
        match group.mods[1].route {
            ir::ModRoute {
                target: ir::ParamAddr::Volume,
                smooth_ms: tau_ms,
                ..
            } => assert_eq!(tau_ms, 0.0),
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
            g2.mods[0].route,
            ir::ModRoute {
                target: ir::ParamAddr::FilterCutoff { insert: 2 },
                law: ir::RouteLaw::Linear,
                ..
            }
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

    #[test]
    fn step_mod_looping_free_synced_shaper_and_polarity() {
        let step_free = StepMod {
            version: 0x81,
            snap_to_12: true,
            num_steps: 4,
            single_trigger: false,
            steps: vec![-1.0, 0.0, 0.5, 1.0, 0.99],
            frequency: 8.0, // 8 steps/s -> 2.0 Hz ramp over 4 steps
            legacy_note_value: None,
            frequency_sync: Some(ni_file::kontakt::objects::PackedRecord {
                values: [-1.0, 0.0, 0.0],
                flag: 0,
            }),
        };
        let invert = ModShaper {
            enabled: true,
            curve: ShaperCurve::Breakpoints(vec![
                Breakpoint {
                    x: 0.0,
                    y: 1.0,
                    curve: 0.0,
                },
                Breakpoint {
                    x: 1.0,
                    y: 0.0,
                    curve: 0.0,
                },
            ]),
        };
        let mut group = ir::Group::DEFAULT;
        let mut notes = Notes::default();
        let p = InternalModParams {
            name: "STEP_LOOP".into(),
            targets: vec![
                target("pitch", None, 0, None),
                target("volume", None, 0, None),
                target("pan", None, 0, Some(invert)),
            ],
            modulator: Modulator::StepMod(step_free),
            unknown_flags: [0, 0, 1, 0], // retrigger -> Voice clock
            unknown_id: 42,
        };
        timed(&mut group, &p, 0, 0, None, &mut notes);
        assert!(notes.finish().is_empty());
        assert_eq!(group.lfos.len(), 1);
        assert_eq!(group.lfos[0].wave, ir::Wave::Ramp);
        assert_eq!(group.lfos[0].rate, ir::Rate::Hz(2.0));
        assert_eq!(group.lfos[0].clock, ir::Clock::Voice);
        assert_eq!(group.shapers.len(), 3);
        // Pitch (signed): [-1, 0, 0.5, 1, 1] (trailing sentinel copy)
        assert!(group.shapers[0].stepped);
        assert_eq!(group.shapers[0].table, vec![-1.0, 0.0, 0.5, 1.0, 1.0]);
        assert!((group.shapers[0].apply(0.0) - (-1.0)).abs() < 1e-6);
        assert!((group.shapers[0].apply(0.24) - (-1.0)).abs() < 1e-6);
        assert!((group.shapers[0].apply(0.25) - 0.0).abs() < 1e-6);
        assert!((group.shapers[0].apply(0.75) - 1.0).abs() < 1e-6);
        assert!((group.shapers[0].apply(0.999) - 1.0).abs() < 1e-6);
        // Volume (unipolar): (s + 1) / 2 -> [0, 0.5, 0.75, 1, 1]
        assert_eq!(group.shapers[1].table, vec![0.0, 0.5, 0.75, 1.0, 1.0]);
        // Pan (signed + inverted shaper): [1, 0, -0.5, -1, -1]
        for (a, b) in group.shapers[2]
            .table
            .iter()
            .zip([1.0f32, 0.0, -0.5, -1.0, -1.0])
        {
            assert!((a - b).abs() < 1e-4, "{a} vs {b}");
        }

        // Synced looping StepMod (v0x80): 1/16 note (0.25 beats) * 4 steps = 1.0 beat/cycle.
        let step_sync = StepMod {
            version: 0x80,
            snap_to_12: false,
            num_steps: 4,
            single_trigger: false,
            steps: vec![0.0, 1.0, 0.0, -1.0],
            frequency: 2.0,
            legacy_note_value: Some(0.25),
            frequency_sync: None,
        };
        let mut g2 = ir::Group::DEFAULT;
        let mut n2 = Notes::default();
        let p2 = InternalModParams {
            name: "STEP_SYNC".into(),
            targets: vec![target("pitch", None, 0, None)],
            modulator: Modulator::StepMod(step_sync),
            unknown_flags: [0; 4], // Free clock
            unknown_id: 0,
        };
        timed(&mut g2, &p2, 0, 0, None, &mut n2);
        assert!(n2.finish().is_empty());
        assert_eq!(g2.lfos[0].clock, ir::Clock::Free);
        assert_eq!(
            g2.lfos[0].rate,
            ir::Rate::Steps {
                beats: 1.0,
                count: 2.0
            }
        );
    }

    #[test]
    fn step_mod_oneshot_holds_final_step_and_diagnoses_synced_oneshot() {
        let oneshot = StepMod {
            version: 0x81,
            snap_to_12: false,
            num_steps: 3,
            single_trigger: true,
            steps: vec![-0.5, 0.25, 1.0],
            frequency: 4.0, // 250 ms/step
            legacy_note_value: None,
            frequency_sync: Some(ni_file::kontakt::objects::PackedRecord {
                values: [-1.0, 0.0, 0.0],
                flag: 0,
            }),
        };
        let mut group = ir::Group::DEFAULT;
        let mut notes = Notes::default();
        let p = InternalModParams {
            name: "STEP_ONCE".into(),
            targets: vec![target("pitch", None, 0, None)],
            modulator: Modulator::StepMod(oneshot.clone()),
            unknown_flags: [0, 0, 1, 0], // Voice clock
            unknown_id: 0,
        };
        let reg = timed(&mut group, &p, 0, 0, None, &mut notes);
        assert!(notes.finish().is_empty());
        assert!(group.lfos.is_empty());
        assert_eq!(reg, Some(ir::Source::Envelope(0)));
        assert_eq!(group.envelopes.len(), 1);
        // Breakpoints stay strictly within 0..=1: (-0.5 + 1)/2 = 0.25, (0.25 + 1)/2 = 0.625, 1.0.
        assert!((group.envelopes[0].start - 0.25).abs() < 1e-6);
        assert_eq!(group.envelopes[0].sustain, None);
        assert_eq!(group.envelopes[0].points.len(), 4);
        assert!((group.envelopes[0].points[0].ms - 250.0).abs() < 1e-4);
        assert!((group.envelopes[0].points[3].level - 1.0).abs() < 1e-6);
        // Signed pitch target maps 0..=1 -> -1..=1 through a linear transfer.
        assert_eq!(group.mods[0].route.source, ir::Source::Shaped(0));
        assert_eq!(group.shapers[0].source, ir::Source::Envelope(0));
        assert!((group.shapers[0].apply(0.25) - (-0.5)).abs() < 1e-6);
        assert!((group.shapers[0].apply(0.625) - 0.25).abs() < 1e-6);
        assert!((group.shapers[0].apply(1.0) - 1.0).abs() < 1e-6);

        // Knob-only & reused-envelope one-shot StepMods still return their source
        // explicitly so `knob_routes` never drops them.
        let mut g_knob = ir::Group::DEFAULT;
        let mut n_knob = Notes::default();
        let knob_only = InternalModParams {
            name: "STEP_KNOB_ONLY".into(),
            targets: vec![target("frequency", Some(0), 0, None)],
            modulator: Modulator::StepMod(oneshot.clone()),
            unknown_flags: [0, 0, 1, 0],
            unknown_id: 0,
        };
        let reused_knob = InternalModParams {
            name: "STEP_REUSED_KNOB".into(),
            targets: vec![target("frequency", Some(0), 0, None)],
            modulator: Modulator::StepMod(oneshot.clone()),
            unknown_flags: [0, 0, 1, 0],
            unknown_id: 0,
        };
        let src1 = timed(&mut g_knob, &knob_only, 0, 1, None, &mut n_knob);
        let src2 = timed(&mut g_knob, &reused_knob, 0, 2, None, &mut n_knob);
        assert_eq!(src1, Some(ir::Source::Envelope(0)));
        assert_eq!(src2, Some(ir::Source::Envelope(0)));
        let slots = vec![
            (0usize, ir::Source::Lfo(0)),
            (1usize, src1.expect("knob-only source")),
            (2usize, src2.expect("reused source")),
        ];
        knob_routes(
            &mut g_knob,
            &[(1, knob_only), (2, reused_knob)],
            &slots,
            &mut n_knob,
        );
        assert!(n_knob.finish().is_empty());
        assert_eq!(g_knob.mods.len(), 2); // 2 StepMod -> LFO frequency routes

        // Synced one-shot emits an honest Audible diagnostic.
        let mut synced = oneshot;
        synced.frequency_sync = Some(ni_file::kontakt::objects::PackedRecord {
            values: [0.25, 0.0, 0.0],
            flag: 0,
        });
        let mut g2 = ir::Group::DEFAULT;
        let mut n2 = Notes::default();
        let p2 = InternalModParams {
            name: "STEP_SYNC_ONCE".into(),
            targets: vec![target("pitch", None, 0, None)],
            modulator: Modulator::StepMod(synced),
            unknown_flags: [0, 0, 1, 0],
            unknown_id: 0,
        };
        assert_eq!(timed(&mut g2, &p2, 0, 0, None, &mut n2), None);
        assert!(g2.mods.is_empty());
        let diags = n2.finish();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].impact, ir::Impact::Audible);
    }
}
