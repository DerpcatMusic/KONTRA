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
                    (Some(s), Some(shaper)) => shaped(&mut out.shapers, s, &shaper.curve),
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
                        laws::eq_gain(source, insert, param, t.signed_intensity())
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
                                Some(laws::lfo_rate(source, lfo, t.signed_intensity()))
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
    shapers.push(ir::Transfer {
        source,
        table: (0..128).map(|k| curve.evaluate(k as f32 / 127.0)).collect(),
    });
    Some(ir::Source::Shaped(n))
}
