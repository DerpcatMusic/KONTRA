//! External modulators (velocity, controllers, pitch bend, key, scripts)
//! as routes on a group.

use kontra_format::Notes;
use kontra_ir as ir;
use ni_file::kontakt::objects::{ExternalModArray32, Group, ModSource, ShaperCurve};

use crate::laws;

const EXTERNAL_MODS: u16 = 0x3c;

/// Adds `group`'s external modulators to `out` as routes; filter inserts a
/// route moves land in `modulated`. `timed_slots` are the internal
/// modulator slots that made an LFO or envelope, with the source each made.
pub(crate) fn external(
    group: &Group,
    group_index: usize,
    timed_slots: &[(usize, ir::Source)],
    release_counter: ir::Source,
    out: &mut ir::Group,
    notes: &mut Notes,
    modulated: &mut Vec<u8>,
) -> Result<(), ni_file::Error> {
    let slot = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
    if let Some(chunk) = group.0.find_first(EXTERNAL_MODS) {
        for (n, m) in ExternalModArray32::try_from(chunk)?.slots()? {
            let p = m.params()?;
            let n = n.saturating_add(usize::from(laws::EXTERNAL_MOD_BASE));
            out.routes.push(ir::Routes {
                slot: slot(n),
                name: p.name.clone(),
                names: p.targets.iter().map(|t| t.name.clone()).collect(),
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
                ModSource::KeyPosition => Some(ir::Source::Key),
                ModSource::Constant => Some(ir::Source::Constant),
                // KSP set_event_par_arr(id, $EVENT_PAR_MOD_VALUE_ID, v, index).
                ModSource::Script(index) => u16::try_from(index).ok().map(ir::Source::Script),
                ModSource::ReleaseTriggerCounter => Some(release_counter),
                _ => None,
            };
            for (target, t) in p.targets.iter().enumerate() {
                // The loop consumer is off for a streamed sample (its preload
                // budget, sample +0x177c, gates FUN_140d20250): nothing moves.
                if matches!(t.param.as_str(), "loopStart" | "loopLength") && out.stream.is_some() {
                    continue;
                }
                let raw = source;
                let source = match (raw, t.shaper.as_ref().filter(|s| s.enabled)) {
                    (Some(s), Some(shaper)) => {
                        let result = shaped(&mut out.shapers, s, &shaper.curve);
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
                    _ => raw,
                };
                let kind = match (source, t.param.as_str(), t.slot) {
                    (Some(source), "volume", None) => Some(ir::ModKind::Volume {
                        source,
                        intensity: t.signed_intensity(),
                        curve: laws::volume(raw.unwrap_or(source)),
                        tau_ms: laws::route_tau_ms(t.lag_ms),
                    }),
                    (Some(source), "pitch", None) => Some(ir::ModKind::Pitch {
                        source,
                        semitones: laws::PITCH_SEMITONES,
                        depth: t.signed_intensity(),
                        tau_ms: laws::route_tau_ms(t.lag_ms),
                    }),
                    // Targets 7 and 8 (FUN_140a6d410); the loop consumer
                    // FUN_14087b9c0 adds `(int)(frames · x)` to both bounds,
                    // or to the end, every 32 frames.
                    (Some(source), param @ ("loopStart" | "loopLength"), None)
                        if !matches!(source, ir::Source::HeldTime { .. }) =>
                    {
                        Some(ir::ModKind::LoopShift {
                            source,
                            length: param == "loopLength",
                            depth: t.signed_intensity(),
                        })
                    }
                    (Some(source), "playPos", None) => Some(ir::ModKind::SampleStart {
                        source,
                        depth: t.signed_intensity(),
                    }),
                    (Some(source), "filterCutoff", Some(insert)) => Some(ir::ModKind::Cutoff {
                        source,
                        insert,
                        depth: t.signed_intensity(),
                    }),
                    (Some(source), "filterQ", Some(insert)) => Some(ir::ModKind::Resonance {
                        source,
                        insert,
                        depth: t.signed_intensity(),
                    }),
                    (Some(source), param, Some(insert)) if laws::eq_band_index(param).is_some() => {
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
                    // Only the amplitude envelope's times: another AHDSR is
                    // a breakpoint source with fixed times.
                    (Some(source), stage @ ("ahdsr_attack" | "ahdsr_release"), Some(at))
                        if out.mods.iter().any(|m| {
                            m.kind == ir::ModKind::Envelope && m.slot == u16::from(at)
                        }) =>
                    {
                        let intensity = t.signed_intensity();
                        let floor_ms = laws::time_floor_ms(&p.unknown_source_data);
                        Some(if stage == "ahdsr_attack" {
                            ir::ModKind::Attack {
                                source,
                                intensity,
                                floor_ms,
                            }
                        } else {
                            ir::ModKind::Release {
                                source,
                                intensity,
                                floor_ms,
                            }
                        })
                    }
                    // The target table (FUN_140a6d410) names it 0xb4; the law
                    // compiler (FUN_140873040) adds `E·intensity` in knob
                    // units, one cycle.
                    (Some(source), "startPhase", Some(at))
                        if !matches!(source, ir::Source::HeldTime { .. }) =>
                    {
                        timed_slots.iter().find_map(|&(n, of)| match of {
                            ir::Source::Lfo(lfo) if n == usize::from(at) => {
                                Some(ir::ModKind::LfoPhase {
                                    source,
                                    lfo,
                                    depth: t.signed_intensity(),
                                })
                            }
                            _ => None,
                        })
                    }
                    // Target 0x1a is in the additive family (FUN_140873040):
                    // the knob moves by `signal·intensity`.
                    (Some(source), "frequency", Some(at))
                        if !matches!(source, ir::Source::HeldTime { .. }) =>
                    {
                        timed_slots.iter().find_map(|&(n, of)| match of {
                            ir::Source::Lfo(lfo) if n == usize::from(at) => {
                                let result = laws::lfo_rate(source, lfo, t.signed_intensity());
                                kontra_law::record(|| kontra_law::Application {
                                    id: kontra_law::LawId("kontakt.lfo_rate_route.v1"),
                                    location: format!(
                                        "groups/{group_index}/routes/{n}/targets/{target}"
                                    ),
                                    input: format!(
                                        "source={source:?}, lfo={lfo}, intensity={:?}",
                                        t.signed_intensity()
                                    ),
                                    output: format!("{result:?}"),
                                });
                                Some(result)
                            }
                            _ => None,
                        })
                    }
                    // Target 0x19 compiles (FUN_140873040, 0x144f1daa0) to
                    // `$in * (1.0 - (1.0 - $signal) * $intensity)`; flag 4
                    // picks a screen form (0x144f1daf0) no library uses yet.
                    (Some(source), "intensity", Some(at))
                        if t.unknown_flags & 4 == 0
                            && !matches!(source, ir::Source::HeldTime { .. }) =>
                    {
                        timed_slots
                            .iter()
                            .find(|&&(n, _)| n == usize::from(at))
                            .map(|&(_, of)| ir::ModKind::Intensity {
                                source,
                                of,
                                depth: t.signed_intensity(),
                            })
                    }
                    (Some(source), param, Some(insert)) => {
                        crate::drive::route(source, insert, param, t.signed_intensity())
                    }
                    _ => None,
                };
                if let Some(kind) = kind {
                    match &kind {
                        ir::ModKind::Volume {
                            source,
                            curve,
                            tau_ms,
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
                        ir::ModKind::Pitch { tau_ms, .. } => {
                            record_lag(group_index, n, target, t.lag_ms, *tau_ms)
                        }
                        _ => {}
                    }
                    out.mods.push(ir::Mod {
                        name: p.name.clone(),
                        slot: slot(n),
                        target: slot(target),
                        bypass: false,
                        kind,
                    });
                } else {
                    if t.param == "filterCutoff" {
                        modulated.extend(t.slot);
                    }
                    let to = if t.slot.is_some() { "module " } else { "" };
                    let feature = format!("external modulator {:?} -> {to}{}", p.source, t.param);
                    notes.add(ir::Impact::Audible, &feature, &p.name);
                }
            }
        }
    }
    Ok(())
}

/// `source` through a modulation shaper, sampled at 128 even steps.
fn shaped(
    shapers: &mut Vec<ir::Transfer>,
    source: ir::Source,
    curve: &ShaperCurve,
) -> Option<ir::Source> {
    let n = u8::try_from(shapers.len()).ok()?;
    // V2's measured Kontakt shaper bow: sign is geometric, so a falling
    // segment reverses the exponential's direction. See KONTAKT_REFERENCE.
    let law = match curve {
        ShaperCurve::Breakpoints(points) if !points.is_empty() => {
            let points = points
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let direction = points
                        .get(i + 1)
                        .map_or(1.0, |next| if next.y < p.y { -1.0 } else { 1.0 });
                    (p.x, p.y, 17.0 * p.curve * direction)
                })
                .collect();
            Some(kontra_law::Law::Breakpoints(points))
        }
        _ => None,
    };
    shapers.push(ir::Transfer {
        source,
        table: (0..128)
            .map(|k| {
                let x = k as f32 / 127.0;
                law.as_ref()
                    .map_or_else(|| curve.evaluate(x), |law| law.eval(x))
            })
            .collect(),
    });
    Some(ir::Source::Shaped(n))
}

fn record_lag(group: usize, route: usize, target: usize, lag: i16, tau: f32) {
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
    fn curved_crossfade_keeps_both_layers_near_the_native_midpoint() {
        let point = |x, y, curve| Breakpoint { x, y, curve };
        for (a, b, native) in [(1.0, 0.0, 0.663), (0.0, 1.0, 0.664)] {
            let curve = ShaperCurve::Breakpoints(vec![
                point(0.332_677_16, a, -0.084_459_46),
                point(0.667_322_8, b, 0.0),
            ]);
            let mut shapers = Vec::new();
            shaped(&mut shapers, ir::Source::Controller(1), &curve).unwrap();
            let actual = shapers[0].apply(64.0 / 127.0);
            assert!((actual - native).abs() < 0.035, "{actual} vs {native}");
        }
    }
}
