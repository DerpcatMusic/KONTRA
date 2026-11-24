//! External modulators (velocity, controllers, pitch bend, key, scripts)
//! as routes on a group.

use kontra_format::Notes;
use kontra_ir as ir;
use ni_file::kontakt::objects::{ExternalModArray32, Group, ModSource, ModTarget, ShaperCurve};

use crate::laws;

const EXTERNAL_MODS: u16 = 0x3c;

/// Adds `group`'s external modulators to `out` as routes. `timed_slots` are
/// the internal modulator slots that made an LFO or envelope, with the source
/// each made.
pub(crate) fn external(
    group: &Group,
    group_index: usize,
    timed_slots: &[(usize, ir::Source)],
    release_counter: ir::Source,
    out: &mut ir::Group,
    notes: &mut Notes,
) -> Result<(), ni_file::Error> {
    let slot = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
    if let Some(chunk) = group.0.find_first(EXTERNAL_MODS) {
        for (ext_idx, m) in ExternalModArray32::try_from(chunk)?.slots()? {
            let p = m.params()?;
            let n = ext_idx.saturating_add(usize::from(laws::EXTERNAL_MOD_BASE));
            out.routes.push(ir::Routes {
                slot: slot(n),
                name: p.name.clone(),
                names: p.targets.iter().map(|t| t.name.clone()).collect(),
                depths: p.targets.iter().map(|t| t.signed_intensity()).collect(),
            });
            // The loader reads a source only for category 1 (FUN_140d09620,
            // +0x538); an unassigned slot has nothing to modulate with.
            if p.source == ModSource::Unassigned {
                notes.add(
                    ir::Impact::Cosmetic,
                    "unassigned external modulator",
                    &p.name,
                );
                continue;
            }
            let source = match p.source {
                ModSource::Velocity => Some(ir::Source::Velocity),
                ModSource::MidiCc(cc) => Some(match p.cc_start() {
                    Some(start) => ir::Source::ControllerFrom { cc, start },
                    None => ir::Source::Controller(cc),
                }),
                ModSource::PitchBend => Some(ir::Source::PitchBend),
                // Kontakt's aftertouch sources: a note's own pressure, or
                // its channel's (MPE: the same).
                ModSource::PolyAftertouch | ModSource::MonoAftertouch => Some(ir::Source::Pressure),
                ModSource::KeyPosition => Some(ir::Source::Key),
                ModSource::ReleaseVelocity => Some(ir::Source::ReleaseVelocity),
                ModSource::Constant => Some(ir::Source::Constant),
                // KSP set_event_par_arr(id, $EVENT_PAR_MOD_VALUE_ID, v, index).
                ModSource::Script(index) => u16::try_from(index).ok().map(ir::Source::Script),
                ModSource::ReleaseTriggerCounter => Some(release_counter),
                // "A random value between 0 and 1 (-1 and 1) for each received
                // note" (Kontakt manual, external sources).
                ModSource::RandomUnipolar => Some(ir::Source::Random { bipolar: false }),
                ModSource::RandomBipolar => Some(ir::Source::Random { bipolar: true }),
                // Per-zone sample-position envelope (BZEnvMod, BParExternalMod
                // category 2); matches each zone's BZoneLevelEnv by ext_mod_idx.
                ModSource::ZoneEnvelope => u8::try_from(ext_idx).ok().map(ir::Source::ZoneEnvelope),
                ModSource::PolyExpression(_) | ModSource::Unassigned => None,
            };
            for (target, t) in p.targets.iter().enumerate() {
                // The loop consumer is off for a streamed sample (its preload
                // budget, sample +0x177c, gates FUN_140d20250): nothing moves.
                if matches!(t.param.as_str(), "loopStart" | "loopLength") && out.stream.is_some() {
                    continue;
                }
                let raw = match source {
                    Some(ir::Source::Script(id)) if signed_route(&t.param) => {
                        Some(ir::Source::ScriptBipolar(id))
                    }
                    other => other,
                };
                let source = match (raw, t.shaper.as_ref().filter(|s| s.enabled)) {
                    (Some(s), Some(shaper)) => {
                        let bipolar = s.is_bipolar() || matches!(s, ir::Source::Key);
                        let signed = bipolar && signed_route(&t.param);
                        let result = shaped(&mut out.shapers, s, &shaper.curve, signed);
                        kontra_law::record(|| kontra_law::Application {
                            id: kontra_law::LawId("kontakt.mod_shaper.v1"),
                            location: format!(
                                "groups/{group_index}/routes/{n}/targets/{target}/shaper"
                            ),
                            input: format!("source={s:?}, curve={:?}", shaper.curve),
                            output: format!(
                                "source={result:?}, transfer={:?}",
                                result.and_then(|id| match id {
                                    ir::Source::Shaped(i) => out.shapers.get(usize::from(i)),
                                    _ => None,
                                })
                            ),
                        });
                        result
                    }
                    // Unshaped KeyPosition on signed targets produces C3-centered
                    // octaves `(key - 60) / 12` (`0140840000.c:9341`): a 2-point
                    // linear transfer mapping `key / 127` in `0..=1` to
                    // `-5.0..=(67.0 / 12.0)`. Unipolar targets keep `key / 127`.
                    (Some(ir::Source::Key), None) if signed_route(&t.param) => {
                        key_bipolar(&mut out.shapers)
                    }
                    _ => raw,
                };
                if matches!(
                    t.param.as_str(),
                    "ahdsr_attack"
                        | "ahdsr_hold"
                        | "ahdsr_decay"
                        | "ahdsr_sustain"
                        | "ahdsr_release"
                ) && let (Some(source), Some(at)) = (source, t.slot)
                {
                    let is_amp = out.mods.iter().any(|m| {
                        m.route.target == ir::ParamAddr::AmpEnvelope && m.slot == u16::from(at)
                    });
                    let timed_env = staged_envelope(out, timed_slots, at, &t.param);
                    if is_amp || timed_env.is_some() {
                        let intensity = t.signed_intensity();
                        let raw_source = raw.unwrap_or(source);
                        let scale = laws::time_scale(raw_source, p.simulate_k2_env_time());
                        if t.param != "ahdsr_sustain" {
                            kontra_law::record(|| kontra_law::Application {
                                id: kontra_law::LawId("kontakt.time_scale.v1"),
                                location: format!(
                                    "groups/{group_index}/routes/{n}/targets/{target}/time_scale"
                                ),
                                input: format!(
                                    "source={raw_source:?}, simulate_k2={}",
                                    p.simulate_k2_env_time()
                                ),
                                output: format!("{scale:?}"),
                            });
                        }
                        if t.lag_ms < 0 {
                            notes.add(ir::Impact::Audible, "negative modulation lag", &p.name);
                        }
                        let bypass = p.bypass();
                        if is_amp
                            && let Some(kind) =
                                ahdsr_route(&t.param, source, raw_source, intensity, scale, None)
                        {
                            out.mods.push(ir::Mod {
                                name: p.name.clone(),
                                slot: slot(n),
                                target: slot(target),
                                bypass,
                                route: kind,
                            });
                        }
                        if let Some(env) = timed_env
                            && let Some(kind) = ahdsr_route(
                                &t.param,
                                source,
                                raw_source,
                                intensity,
                                scale,
                                Some(env),
                            )
                        {
                            out.mods.push(ir::Mod {
                                name: p.name.clone(),
                                slot: slot(n),
                                target: slot(target),
                                bypass,
                                route: kind,
                            });
                        }
                        continue;
                    }
                }
                let kind = match (source, t.param.as_str(), t.slot) {
                    (Some(source), "volume", None) => Some(
                        ir::ModRoute::new(source, ir::ParamAddr::Volume, t.signed_intensity())
                            .law(ir::RouteLaw::Gain(laws::volume(raw.unwrap_or(source))))
                            .smooth(laws::route_tau_ms(t.lag_ms)),
                    ),
                    (Some(source), "pan", None) => Some(ir::ModRoute::new(
                        source,
                        ir::ParamAddr::Pan,
                        t.signed_intensity(),
                    )),
                    (Some(source), "pitch", None) => Some(
                        ir::ModRoute::new(source, ir::ParamAddr::Pitch, t.signed_intensity())
                            .span(laws::PITCH_SEMITONES)
                            .smooth(laws::route_tau_ms(t.lag_ms)),
                    ),
                    // Targets 7 and 8 (FUN_140a6d410); the loop consumer
                    // FUN_14087b9c0 adds `(int)(frames · x)` to both bounds,
                    // or to the end, every 32 frames.
                    (Some(source), param @ ("loopStart" | "loopLength"), None)
                        if !matches!(source, ir::Source::HeldTime { .. }) =>
                    {
                        Some(ir::ModRoute::new(
                            source,
                            ir::ParamAddr::LoopShift {
                                length: param == "loopLength",
                            },
                            t.signed_intensity(),
                        ))
                    }
                    (Some(source), param, None)
                        if let Some(value) = crate::wavetable::target(param) =>
                    {
                        Some(ir::ModRoute::new(
                            source,
                            ir::ParamAddr::Wavetable(value),
                            t.signed_intensity(),
                        ))
                    }
                    (Some(source), "playPos", None) => Some(ir::ModRoute::new(
                        source,
                        ir::ParamAddr::SampleStart,
                        t.signed_intensity(),
                    )),
                    (Some(source), "filterCutoff", Some(insert)) => Some(ir::ModRoute::new(
                        source,
                        ir::ParamAddr::FilterCutoff { insert },
                        t.signed_intensity(),
                    )),
                    (Some(source), "filterQ", Some(insert)) => Some(ir::ModRoute::new(
                        source,
                        ir::ParamAddr::FilterResonance { insert },
                        t.signed_intensity(),
                    )),
                    (Some(source), param, Some(insert))
                        if crate::provenance::eq_band_index(param, group_index, n, target)
                            .is_some() =>
                    {
                        let result = laws::eq_gain(source, insert, param, t.signed_intensity());
                        kontra_law::record(|| kontra_law::Application {
                            id: kontra_law::LawId("kontakt.eq_gain_route.v1"),
                            location: format!("groups/{group_index}/routes/{n}/targets/{target}"),
                            input: format!(
                                "source={source:?}, insert={insert}, param={param}, intensity={:?}",
                                t.signed_intensity()
                            ),
                            output: format!("{result:?}"),
                        });
                        result
                    }
                    // The target table (FUN_140a6d410) names it 0xb4; the law
                    // compiler (FUN_140873040) adds `E·intensity` in knob
                    // units, one cycle.
                    (Some(source), "startPhase", Some(at))
                        if !matches!(source, ir::Source::HeldTime { .. }) =>
                    {
                        timed_slots.iter().find_map(|&(n, of)| match of {
                            ir::Source::Lfo(lfo) if n == usize::from(at) => {
                                Some(ir::ModRoute::new(
                                    source,
                                    ir::ParamAddr::LfoParam {
                                        lfo,
                                        param: ir::LfoParam::Phase,
                                    },
                                    t.signed_intensity(),
                                ))
                            }
                            _ => None,
                        })
                    }
                    (Some(source), param @ ("frequency" | "intensity"), Some(at))
                        if !matches!(source, ir::Source::HeldTime { .. }) =>
                    {
                        let result = modulator_route(source, param, at, t, timed_slots);
                        kontra_law::record(|| kontra_law::Application {
                            id: kontra_law::LawId("kontakt.modulator_route.v1"),
                            location: format!("groups/{group_index}/routes/{n}/targets/{target}"),
                            input: format!("source={source:?}, param={param}, slot={at}"),
                            output: format!("{result:?}"),
                        });
                        result
                    }
                    (Some(source), param, Some(insert)) => {
                        crate::drive::route(source, insert, param, t.signed_intensity())
                    }
                    _ => None,
                };
                if let Some(kind) = kind {
                    match &kind {
                        ir::ModRoute {
                            target: ir::ParamAddr::Volume,
                            source,
                            smooth_ms: tau_ms,
                            law: ir::RouteLaw::Gain(curve),
                            ..
                        } => {
                            kontra_law::record(|| kontra_law::Application {
                                id: kontra_law::LawId("kontakt.volume_source_curve.v1"),
                                location: format!(
                                    "groups/{group_index}/routes/{n}/targets/{target}/curve"
                                ),
                                input: format!("source={:?}", raw.unwrap_or(*source)),
                                output: format!("{curve:?}"),
                            });
                            record_lag(group_index, n, target, t.lag_ms, *tau_ms);
                        }
                        ir::ModRoute {
                            target: ir::ParamAddr::Pitch,
                            smooth_ms: tau_ms,
                            ..
                        } => record_lag(group_index, n, target, t.lag_ms, *tau_ms),
                        _ => {}
                    }
                    if t.lag_ms < 0 {
                        notes.add(ir::Impact::Audible, "negative modulation lag", &p.name);
                    }
                    out.mods.push(ir::Mod {
                        name: p.name.clone(),
                        slot: slot(n),
                        target: slot(target),
                        bypass: p.bypass(),
                        route: kind,
                    });
                } else {
                    let to = if t.slot.is_some() { "module " } else { "" };
                    let feature = format!("external modulator {:?} -> {to}{}", p.source, t.param);
                    notes.add(ir::Impact::Audible, &feature, &p.name);
                }
            }
        }
    }
    Ok(())
}

fn staged_envelope(
    out: &ir::Group,
    slots: &[(usize, ir::Source)],
    at: u8,
    param: &str,
) -> Option<u8> {
    let needs_stages = matches!(param, "ahdsr_hold" | "ahdsr_decay" | "ahdsr_sustain");
    slots.iter().find_map(|&(slot, source)| match source {
        ir::Source::Envelope(e)
            if slot == usize::from(at)
                && (!needs_stages
                    || out
                        .envelopes
                        .get(usize::from(e))
                        .is_some_and(|env| env.stages)) =>
        {
            Some(e)
        }
        _ => None,
    })
}

/// A route to another modulator's knob, by the target's law whatever modulates it (FUN_140873040):
/// an LFO's `frequency` (0x1a) moves by `signal·intensity`; `intensity` (0x19) scales by
/// `1 − (1 − signal)·intensity` (0x144f1daa0; flag 4, a screen form, is unused).
pub(crate) fn modulator_route(
    source: ir::Source,
    param: &str,
    at: u8,
    t: &ModTarget,
    timed_slots: &[(usize, ir::Source)],
) -> Option<ir::ModRoute> {
    let (_, of) = timed_slots.iter().find(|&&(n, _)| n == usize::from(at))?;
    match (param, *of) {
        ("frequency", ir::Source::Lfo(lfo)) => {
            Some(laws::lfo_rate(source, lfo, t.signed_intensity()))
        }
        ("intensity", of) if t.unknown_flags & 4 == 0 => Some(ir::ModRoute::new(
            source,
            ir::ParamAddr::RouteIntensity { of },
            t.signed_intensity(),
        )),
        _ => None,
    }
}

/// Builds an AHDSR stage route for `stage` (`ahdsr_{attack,hold,decay,sustain,release}`).
fn ahdsr_route(
    stage: &str,
    source: ir::Source,
    raw_source: ir::Source,
    intensity: f32,
    scale: ir::TimeScale,
    envelope: Option<u8>,
) -> Option<ir::ModRoute> {
    Some(match stage {
        "ahdsr_attack" => ir::ModRoute::new(
            source,
            ir::ParamAddr::EnvelopeStage {
                envelope,
                stage: ir::EnvStage::Attack,
            },
            intensity,
        )
        .law(ir::RouteLaw::Time(scale)),
        "ahdsr_hold" => ir::ModRoute::new(
            source,
            ir::ParamAddr::EnvelopeStage {
                envelope,
                stage: ir::EnvStage::Hold,
            },
            intensity,
        )
        .law(ir::RouteLaw::Time(scale)),
        "ahdsr_decay" => ir::ModRoute::new(
            source,
            ir::ParamAddr::EnvelopeStage {
                envelope,
                stage: ir::EnvStage::Decay,
            },
            intensity,
        )
        .law(ir::RouteLaw::Time(scale)),
        // Sustain scales knob `u` by `m^p` (`p = 3` for external source types
        // 7..=13 via `fcubic`, `p = 1` for types 1..=5 / ZoneEnvelope), and
        // Kontakt sustain level is `S = u³`, so `S' = (u·m^p)³ = S·m^(3p)`
        // (`0x140873120` / `0x140876753`).
        "ahdsr_sustain" => ir::ModRoute::new(
            source,
            ir::ParamAddr::EnvelopeStage {
                envelope,
                stage: ir::EnvStage::Sustain,
            },
            intensity,
        )
        .law(ir::RouteLaw::Gain(ir::Curve::Depth {
            power: 3 * laws::source_power(raw_source),
        })),
        "ahdsr_release" => ir::ModRoute::new(
            source,
            ir::ParamAddr::EnvelopeStage {
                envelope,
                stage: ir::EnvStage::Release,
            },
            intensity,
        )
        .law(ir::RouteLaw::Time(scale)),
        _ => return None,
    })
}

/// Deduplicated 2-point linear transfer mapping `ir::Source::Key` (`key / 127`)
/// to Kontakt's C3-centered octaves `(key - 60) / 12` (`0140840000.c:9341`).
fn key_bipolar(shapers: &mut Vec<ir::Transfer>) -> Option<ir::Source> {
    let table = [-5.0_f32, 67.0 / 12.0];
    if let Some(i) = shapers.iter().position(|t| {
        t.source == ir::Source::Key
            && !t.stepped
            && t.smooth_s == 0.0
            && t.table.as_slice() == table
    }) {
        return u8::try_from(i).ok().map(ir::Source::Shaped);
    }
    let n = u8::try_from(shapers.len()).ok()?;
    shapers.push(ir::Transfer {
        source: ir::Source::Key,
        table: table.to_vec(),
        smooth_s: 0.0,
        stepped: false,
    });
    Some(ir::Source::Shaped(n))
}

/// Whether a Kontakt target reads its source signed (-1..=1): every target
/// but the gains and times the engine reads on 0..=1.
pub(crate) fn signed_route(param: &str) -> bool {
    !matches!(
        param,
        "volume"
            | "ahdsr_attack"
            | "ahdsr_hold"
            | "ahdsr_decay"
            | "ahdsr_sustain"
            | "ahdsr_release"
            | "intensity"
            | "filterQ"
            | "distortionIntensity"
    )
}

/// `source` through a modulation shaper, sampled at 128 even steps. Kontakt
/// draws every shaper on 0..=1; a bipolar source's shaper keeps its source's
/// polarity (centre 0.5 = rest), so on a `signed` route the table becomes
/// `2y - 1`, the domain the IR reads it in.
pub(crate) fn shaped(
    shapers: &mut Vec<ir::Transfer>,
    source: ir::Source,
    curve: &ShaperCurve,
    signed: bool,
) -> Option<ir::Source> {
    let n = u8::try_from(shapers.len()).ok()?;
    let law = match curve {
        ShaperCurve::Breakpoints(points) if !points.is_empty() => {
            let points: Vec<_> = points.iter().map(|p| (p.x, p.y, p.curve)).collect();
            Some(kontra_law::kontakt::modulation_shaper(&points))
        }
        _ => None,
    };
    shapers.push(ir::Transfer {
        stepped: false,
        source,
        table: (0..128)
            .map(|k| {
                let x = k as f32 / 127.0;
                let y = law
                    .as_ref()
                    .map_or_else(|| curve.evaluate(x), |law| law.eval(x));
                if signed { 2.0 * y - 1.0 } else { y }
            })
            .collect(),
        smooth_s: 0.0,
    });
    Some(ir::Source::Shaped(n))
}

pub(crate) fn record_lag(group: usize, route: usize, target: usize, lag: i16, tau: f32) {
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId("kontakt.route_tau_ms.v1"),
        location: format!("groups/{group}/routes/{route}/targets/{target}/tau_ms"),
        input: format!("lag_ms={lag}"),
        output: format!("{tau:?}"),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ni_file::kontakt::objects::Breakpoint;

    #[test]
    fn stage_targets_only_accept_staged_envelopes() {
        let mut group = ir::Group::DEFAULT;
        group.envelopes = vec![
            ir::Breakpoints {
                start: 0.0,
                points: vec![],
                sustain: None,
                loop_from: None,
                stages: false,
            },
            ir::Breakpoints {
                start: 0.0,
                points: vec![],
                sustain: None,
                loop_from: None,
                stages: true,
            },
        ];
        let slots = [(4, ir::Source::Envelope(0)), (5, ir::Source::Envelope(1))];
        for stage in ["ahdsr_hold", "ahdsr_decay", "ahdsr_sustain"] {
            assert_eq!(staged_envelope(&group, &slots, 4, stage), None);
            assert_eq!(staged_envelope(&group, &slots, 5, stage), Some(1));
        }
        for stage in ["ahdsr_attack", "ahdsr_release"] {
            assert_eq!(staged_envelope(&group, &slots, 4, stage), Some(0));
            assert_eq!(staged_envelope(&group, &slots, 5, stage), Some(1));
        }
    }

    #[test]
    fn curved_crossfade_keeps_both_layers_near_the_native_midpoint() {
        let point = |x, y, curve| Breakpoint { x, y, curve };
        for (a, b, native) in [(1.0, 0.0, 0.663), (0.0, 1.0, 0.664)] {
            let curve = ShaperCurve::Breakpoints(vec![
                point(0.332_677_16, a, -0.084_459_46),
                point(0.667_322_8, b, 0.0),
            ]);
            let mut shapers = Vec::new();
            shaped(&mut shapers, ir::Source::Controller(1), &curve, false).unwrap();
            let actual = shapers[0].apply(64.0 / 127.0);
            assert!((actual - native).abs() < 0.035, "{actual} vs {native}");
        }
    }

    #[test]
    fn a_shaped_pitch_bend_stays_bipolar_on_a_signed_route() {
        let flat = ShaperCurve::Breakpoints(vec![
            Breakpoint {
                x: 0.0,
                y: 0.5,
                curve: 0.0,
            },
            Breakpoint {
                x: 1.0,
                y: 0.5,
                curve: 0.0,
            },
        ]);
        let mut shapers = Vec::new();
        shaped(
            &mut shapers,
            ir::Source::PitchBend,
            &flat,
            signed_route("playPos"),
        )
        .unwrap();
        shaped(
            &mut shapers,
            ir::Source::PitchBend,
            &flat,
            signed_route("volume"),
        )
        .unwrap();
        assert!(shapers[0].apply(0.5).abs() < 1e-6, "{:?}", shapers[0].table);
        assert!((shapers[1].apply(0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn key_position_unshaped_is_c3_centered_octaves_and_deduplicated() {
        let mut shapers = Vec::new();
        let a = key_bipolar(&mut shapers).unwrap();
        let b = key_bipolar(&mut shapers).unwrap();
        assert_eq!(a, b);
        assert_eq!(shapers.len(), 1);
        let t = &shapers[0];
        assert_eq!(t.source, ir::Source::Key);
        for (key, expected) in [
            (0u8, -5.0f32),
            (48, -1.0),
            (60, 0.0),
            (72, 1.0),
            (127, 67.0 / 12.0),
        ] {
            let actual = t.apply(f32::from(key) / 127.0);
            assert!((actual - expected).abs() < 1e-5, "key {key}: {actual}");
        }
        assert!(!signed_route("ahdsr_hold"));
        assert!(!signed_route("ahdsr_decay"));
        assert!(!signed_route("ahdsr_sustain"));
        assert!(signed_route("pan"));
        assert!(signed_route("pitch"));
    }

    #[test]
    fn laws_cover_inverted_env_depth_pressure_power_time_scale_and_dbd() {
        let mut env = ir::Envelope::GATE;
        laws::envelope_depth(&mut env, -0.6, false);
        assert!((env.floor - 1.0).abs() < 1e-6);
        assert!((env.depth - -0.6).abs() < 1e-6);
        laws::envelope_depth(&mut env, -0.6, true);
        assert!((env.floor - 0.6).abs() < 1e-6);
        assert!((env.depth - -0.6).abs() < 1e-6);

        assert_eq!(
            laws::volume(ir::Source::Pressure),
            ir::Curve::Depth { power: 1 }
        );
        assert_eq!(
            laws::volume(ir::Source::ReleaseVelocity),
            ir::Curve::Depth { power: 3 }
        );
        for cubed in [
            ir::Source::Key,
            ir::Source::Velocity,
            ir::Source::ReleaseVelocity,
            ir::Source::HeldTime { span_ms: 1000 },
            ir::Source::Constant,
            ir::Source::Random { bipolar: false },
            ir::Source::Random { bipolar: true },
        ] {
            assert_eq!(
                laws::time_scale(cubed, false),
                ir::TimeScale::LogKnob {
                    floor_ms: 2.0,
                    power: 3
                },
                "KT-01: modern AHDSR time must apply fcubic (power 3) for {cubed:?}"
            );
            assert_eq!(
                laws::time_scale(cubed, true),
                ir::TimeScale::Linear { power: 3 }
            );
            let sustain = ahdsr_route(
                "ahdsr_sustain",
                ir::Source::Shaped(0),
                cubed,
                1.0,
                laws::time_scale(cubed, false),
                None,
            )
            .expect("cubed sustain route");
            assert_eq!(
                sustain.law,
                ir::RouteLaw::Gain(ir::Curve::Depth { power: 9 }),
                "KT-01: cubed source {cubed:?} -> Sustain must compose to power 9"
            );
        }
        for plain in [
            ir::Source::PitchBend,
            ir::Source::Pressure,
            ir::Source::Controller(1),
            ir::Source::ControllerFrom { cc: 11, start: 127 },
            ir::Source::Script(0),
            ir::Source::ZoneEnvelope(0),
        ] {
            assert_eq!(
                laws::time_scale(plain, false),
                ir::TimeScale::LogKnob {
                    floor_ms: 2.0,
                    power: 1
                },
                "KT-01: modern AHDSR time stays power 1 for {plain:?}"
            );
            assert_eq!(
                laws::time_scale(plain, true),
                ir::TimeScale::Linear { power: 3 }
            );
            let sustain = ahdsr_route(
                "ahdsr_sustain",
                plain,
                plain,
                1.0,
                laws::time_scale(plain, false),
                None,
            )
            .expect("plain sustain route");
            assert_eq!(
                sustain.law,
                ir::RouteLaw::Gain(ir::Curve::Depth { power: 3 }),
                "KT-01: plain source {plain:?} -> Sustain stays power 3"
            );
        }

        let dbd = ni_file::kontakt::objects::EnvelopeDbd {
            version: 0x12,
            values: [100.0, 0.1, 200.0],
            extra: None,
            records: None,
            flag: Some(0),
        };
        let bp = laws::dbd(&dbd);
        assert!((bp.start - 1.0).abs() < 1e-6);
        assert_eq!(bp.points.len(), 2);
        assert!((bp.points[0].ms - 100.0).abs() < 1e-6);
        assert!((bp.points[0].level - 0.3).abs() < 1e-6);
        assert!((bp.points[1].ms - 200.0).abs() < 1e-6);
        assert!((bp.points[1].level - 0.0).abs() < 1e-6);
        assert_eq!(bp.sustain, None);

        let flagged = ni_file::kontakt::objects::EnvelopeDbd {
            flag: Some(1),
            ..dbd
        };
        let bp = laws::dbd(&flagged);
        assert_eq!(bp.start, 0.0);
        assert!(bp.points.is_empty());
    }
}
