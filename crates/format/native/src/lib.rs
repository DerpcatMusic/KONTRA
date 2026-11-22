//! Native `.kontra` instrument format reader, writer, and IR bridge
//! (IR vNext Phase 2).
//!
//! Supports three interchangeable encodings of the same versioned schema:
//! - **TOML** (default human-readable `.kontra`)
//! - **JSON** (auto-detected when the payload begins with `{`, or via [`parse_json`])
//! - **Binary envelope** (`b"KTRA\x01\x00"` + length-prefixed payload via [`to_binary`] / [`parse_bytes`])
//!
//! Sample paths resolve relative to the `.kontra` file's directory and are
//! deduplicated and probed with [`kontra_format::builder::SamplePool`]. Every
//! translated instrument is checked with [`kontra_ir::validate`].

pub mod schema;

use std::path::Path;

use kontra_format::Notes;
use kontra_format::builder::SamplePool;
use kontra_ir as ir;

pub use schema::{
    AudioGraphSpec, BINARY_MAGIC, BellSpec, BusSpec, ChannelLayoutSpec, CrossfadeControlSpec,
    CrossfadeLawSpec, CrossfadeSpec, CurveSpec, EnvelopeSpec, EqShapeSpec, FORMAT_VERSION,
    GeneratorSpec, GraphEdgeSpec, GraphEffectSpec, GraphNodeSpec, GroupSpec, InstrumentSpec,
    JoinSpec, LoopSpec, ModKindSpec, ModSpec, NoteExprSpec, OscWaveSpec, PassSpec, RrScopeSpec,
    SourceSpec, StartRuleKindSpec, StartRuleSpec, StealSpec, TimeScaleSpec, TriggerSpec,
    VoiceGroupSpec, ZoneSpec,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not read native instrument: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid TOML in .kontra instrument: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("could not serialize .kontra TOML: {0}")]
    TomlWrite(#[from] toml::ser::Error),
    #[error("invalid JSON in .kontra instrument: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid binary .kontra envelope: {0}")]
    Binary(String),
    #[error("unsupported .kontra format version {0} (max supported: {FORMAT_VERSION})")]
    UnsupportedVersion(u16),
    #[error("cannot losslessly export to .kontra: {0}")]
    UnsupportedExport(String),
}

/// Loads a `.kontra` file from disk (TOML, JSON, or binary envelope), resolving
/// sample paths relative to the file's parent directory.
pub fn load(path: &Path) -> Result<ir::Instrument, Error> {
    let bytes = std::fs::read(path)?;
    let base = path.parent().unwrap_or_else(|| Path::new(""));
    let mut inst = parse_bytes(&bytes, base)?;
    if inst.name.is_empty()
        && let Some(stem) = path.file_stem()
    {
        inst.name = stem.to_string_lossy().into_owned();
    }
    inst.source_path = Some(path.to_path_buf());
    Ok(inst)
}

/// Parses `.kontra` bytes (auto-detecting binary magic, JSON, or TOML) and
/// translates them into an [`ir::Instrument`].
pub fn parse_bytes(bytes: &[u8], base: &Path) -> Result<ir::Instrument, Error> {
    let spec = decode_bytes(bytes)?;
    to_ir(&spec, base)
}

/// Decodes raw `.kontra` bytes into an [`InstrumentSpec`] without touching disk samples.
pub fn decode_bytes(bytes: &[u8]) -> Result<InstrumentSpec, Error> {
    if let Some(rest) = bytes.strip_prefix(BINARY_MAGIC) {
        let len_bytes: [u8; 4] = rest
            .get(..4)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| Error::Binary("truncated length header".into()))?;
        let payload_len = u32::from_le_bytes(len_bytes) as usize;
        let payload = rest
            .get(4..4 + payload_len)
            .ok_or_else(|| Error::Binary("truncated payload".into()))?;
        let spec: InstrumentSpec = serde_json::from_slice(payload)?;
        return check_version(spec);
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|e| Error::Binary(format!("non-UTF-8 text payload: {e}")))?;
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with('{') {
        check_version(serde_json::from_str(trimmed)?)
    } else {
        check_version(toml::from_str(trimmed)?)
    }
}

/// Parses a TOML `.kontra` string into an [`ir::Instrument`].
pub fn parse_toml(text: &str, base: &Path) -> Result<ir::Instrument, Error> {
    let spec = check_version(toml::from_str(text)?)?;
    to_ir(&spec, base)
}

/// Parses a JSON `.kontra` string into an [`ir::Instrument`].
pub fn parse_json(text: &str, base: &Path) -> Result<ir::Instrument, Error> {
    let spec = check_version(serde_json::from_str(text)?)?;
    to_ir(&spec, base)
}

/// Serializes an [`InstrumentSpec`] to pretty TOML.
pub fn to_toml(spec: &InstrumentSpec) -> Result<String, Error> {
    Ok(toml::to_string_pretty(spec)?)
}

/// Serializes an [`InstrumentSpec`] to pretty JSON.
pub fn to_json(spec: &InstrumentSpec) -> Result<String, Error> {
    Ok(serde_json::to_string_pretty(spec)?)
}

/// Serializes an [`InstrumentSpec`] into KONTRA's compact binary envelope
/// (`BINARY_MAGIC` + u32 LE payload length + compact payload).
pub fn to_binary(spec: &InstrumentSpec) -> Result<Vec<u8>, Error> {
    let payload = serde_json::to_vec(spec)?;
    let len = u32::try_from(payload.len())
        .map_err(|_| Error::Binary("payload exceeds 4 GiB limit".into()))?;
    let mut out = Vec::with_capacity(BINARY_MAGIC.len() + 4 + payload.len());
    out.extend_from_slice(BINARY_MAGIC);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

fn check_version(spec: InstrumentSpec) -> Result<InstrumentSpec, Error> {
    if spec.version == 0 || spec.version > FORMAT_VERSION {
        return Err(Error::UnsupportedVersion(spec.version));
    }
    Ok(spec)
}

/// Translates a decoded [`InstrumentSpec`] into a validated [`ir::Instrument`].
pub fn to_ir(spec: &InstrumentSpec, base: &Path) -> Result<ir::Instrument, Error> {
    if spec.version == 0 || spec.version > FORMAT_VERSION {
        return Err(Error::UnsupportedVersion(spec.version));
    }
    let mut pool = SamplePool::new();
    let mut notes = Notes::default();
    let mut next_zone_id = 0u32;

    let voice_groups = spec
        .voice_groups
        .iter()
        .map(|vg| ir::VoiceGroup {
            voices: vg.voices,
            steal: map_steal(vg.steal),
            prefer_released: vg.prefer_released,
            fade_ms: vg.fade_ms,
            exclusive: vg.exclusive,
            per_note: vg.per_note,
            legato: vg.legato,
            returns: vg.returns,
            per_key: vg.per_key,
            quieter_only: vg.quieter_only,
            release: vg.release,
        })
        .collect();

    let buses = spec
        .buses
        .iter()
        .map(|b| ir::Bus {
            volume_db: b.volume_db,
            pan: b.pan.clamp(-1.0, 1.0),
            to_main: b.to_main,
            ..ir::Bus::new(b.name.clone())
        })
        .collect();

    let mut groups = Vec::with_capacity(spec.groups.len());
    for g in &spec.groups {
        let is_osc = matches!(g.generator, Some(GeneratorSpec::Oscillator { .. }));
        let mut zones = Vec::with_capacity(g.zones.len());
        for z in &g.zones {
            let (sample_idx, total) = if is_osc && z.sample.is_empty() {
                (0, 0)
            } else {
                let sample_path = resolve_sample_path(base, &z.sample);
                let (idx, hdr) = pool.add(z.sample.clone(), &sample_path, &mut notes);
                (idx, hdr.map_or(0, |h| h.frames))
            };
            let end = z.end.unwrap_or(total);
            let loops = z
                .loops
                .iter()
                .map(|l| ir::Loop {
                    start: l.start,
                    end: l.end,
                    crossfade: l.crossfade,
                    count: l.count,
                    alternating: l.alternating,
                    until_release: l.until_release,
                    tune_ratio: l.tune_ratio,
                })
                .collect();
            let crossfades = z
                .crossfades
                .iter()
                .map(|xf| ir::Crossfade {
                    control: match xf.control {
                        CrossfadeControlSpec::Velocity => ir::CrossfadeControl::Velocity,
                        CrossfadeControlSpec::Key => ir::CrossfadeControl::Key,
                        CrossfadeControlSpec::Cc { cc } => ir::CrossfadeControl::Cc(cc),
                    },
                    fade_in: xf.fade_in.map(|[a, b]| (a, b)),
                    fade_out: xf.fade_out.map(|[a, b]| (a, b)),
                    law: match xf.law {
                        CrossfadeLawSpec::Linear => ir::CrossfadeLaw::Linear,
                        CrossfadeLawSpec::EqualPower => ir::CrossfadeLaw::EqualPower,
                    },
                })
                .collect();
            let [klo, khi] = z.keys;
            let [vlo, vhi] = z.velocities;
            zones.push(ir::Zone {
                id: next_zone_id,
                sample: sample_idx,
                keys: klo.min(khi)..=klo.max(khi),
                velocities: vlo.min(vhi)..=vlo.max(vhi),
                velocity_fade: z.velocity_fade.map_or((0, 0), |[a, b]| (a, b)),
                key_fade: z.key_fade.map_or((0, 0), |[a, b]| (a, b)),
                crossfades,
                root: z.root.min(127),
                tune: z.tune,
                volume_db: z.volume_db,
                pan: z.pan.clamp(-1.0, 1.0),
                start: z.start.min(end),
                end,
                start_mod: z.start_mod,
                loops,
                reverse: z.reverse,
                mix: Vec::new(),
                slot: 0,
                envelopes: Vec::new(),
            });
            next_zone_id = next_zone_id.saturating_add(1);
        }

        let start = g
            .start
            .iter()
            .map(|r| {
                let join = match r.join {
                    JoinSpec::And => ir::Join::And,
                    JoinSpec::AndNot => ir::Join::AndNot,
                    JoinSpec::Or => ir::Join::Or,
                };
                let rule = match r.rule {
                    StartRuleKindSpec::Keyswitch { range: [a, b] } => ir::StartRule::Keyswitch {
                        range: a.min(b)..=a.max(b),
                    },
                    StartRuleKindSpec::Controller { cc, range: [a, b] } => {
                        ir::StartRule::Controller {
                            cc,
                            range: a.min(b)..=a.max(b),
                        }
                    }
                    StartRuleKindSpec::RoundRobin {
                        scope,
                        position,
                        length,
                        random,
                    } => ir::StartRule::RoundRobin {
                        scope: match scope {
                            RrScopeSpec::Global => ir::RrScope::Global,
                            RrScopeSpec::Key => ir::RrScope::Key,
                            RrScopeSpec::Group => ir::RrScope::Group,
                        },
                        position,
                        length,
                        random,
                    },
                    StartRuleKindSpec::Cycle { random } => ir::StartRule::Cycle { random },
                    StartRuleKindSpec::Random { lo, hi } => ir::StartRule::Random { lo, hi },
                };
                (join, rule)
            })
            .collect();

        let mods = g
            .mods
            .iter()
            .enumerate()
            .map(|(slot, m)| ir::Mod {
                name: m.name.clone(),
                slot: u16::try_from(slot).unwrap_or(u16::MAX),
                target: 0,
                bypass: m.bypass,
                kind: match m.kind {
                    ModKindSpec::Volume {
                        source,
                        intensity,
                        curve,
                        tau_ms,
                    } => ir::ModKind::Volume {
                        source: map_source(source),
                        intensity,
                        curve: map_curve(curve),
                        tau_ms,
                    },
                    ModKindSpec::Pitch {
                        source,
                        semitones,
                        depth,
                        tau_ms,
                    } => ir::ModKind::Pitch {
                        source: map_source(source),
                        semitones,
                        depth,
                        tau_ms,
                    },
                    ModKindSpec::Pan { source, amount } => ir::ModKind::Pan {
                        source: map_source(source),
                        amount,
                    },
                    ModKindSpec::SampleStart { source, depth } => ir::ModKind::SampleStart {
                        source: map_source(source),
                        depth,
                    },
                    ModKindSpec::Release {
                        source,
                        intensity,
                        scale,
                    } => ir::ModKind::Release {
                        source: map_source(source),
                        intensity,
                        scale: match scale {
                            TimeScaleSpec::Linear { power } => ir::TimeScale::Linear { power },
                            TimeScaleSpec::LogKnob { floor_ms, power } => {
                                ir::TimeScale::LogKnob { floor_ms, power }
                            }
                        },
                        envelope: None,
                    },
                },
            })
            .collect();

        groups.push(ir::Group {
            name: g.name.clone(),
            volume_db: g.volume_db,
            pan: g.pan.clamp(-1.0, 1.0),
            tune: g.tune,
            keytrack: g.keytrack,
            envelope: ir::Envelope {
                delay: g.envelope.delay.max(0.0),
                attack: g.envelope.attack.max(0.0),
                hold: g.envelope.hold.max(0.0),
                decay: g.envelope.decay.max(0.0),
                sustain: g.envelope.sustain.clamp(0.0, 1.0),
                release: g.envelope.release.max(0.0),
                attack_shape: g.envelope.attack_shape,
                decay_shape: g.envelope.decay_shape,
                release_shape: g.envelope.release_shape,
                lag: 0.0,
                floor: 0.0,
                depth: 1.0,
                one_shot: g.envelope.one_shot,
            },
            mods,
            trigger: match g.trigger {
                TriggerSpec::Attack => ir::Trigger::Attack,
                TriggerSpec::Release => ir::Trigger::Release,
                TriggerSpec::First => ir::Trigger::First,
                TriggerSpec::Legato => ir::Trigger::Legato,
            },
            one_shot: g.one_shot,
            start,
            polyphony: g.polyphony,
            voice_group: g.voice_group,
            off_by: g.off_by,
            bus: g.bus,
            generator: g
                .generator
                .as_ref()
                .map(|gen_spec| map_generator(gen_spec, base, &mut pool, &mut notes)),
            zones,
            ..ir::Group::DEFAULT
        });
    }

    let mut instrument = ir::Instrument {
        name: spec.name.clone(),
        samples: pool.into_samples(),
        groups,
        buses,
        voices: spec.voices,
        steal: map_steal(spec.steal),
        steal_released_first: spec.steal_released_first,
        steal_fade_ms: spec.steal_fade_ms,
        voice_groups,
        graph: spec.graph.as_ref().map(map_graph),
        diagnostics: notes.finish(),
        ..ir::Instrument::DEFAULT
    };
    let validation = ir::validate(&instrument);
    instrument.diagnostics.extend(validation);
    Ok(instrument)
}

/// Exports an [`ir::Instrument`] into a portable [`InstrumentSpec`] (for
/// saving as `.kontra` TOML/JSON/binary or debugging translated instruments).
///
/// # Errors
/// Returns [`Error::UnsupportedExport`] if the instrument contains a generator
/// or audio-graph effect that cannot be represented losslessly by the `.kontra`
/// schema.
pub fn from_ir(instrument: &ir::Instrument, base: Option<&Path>) -> Result<InstrumentSpec, Error> {
    let sample_ref = |idx: usize| -> Result<String, Error> {
        let Some(s) = instrument.samples.get(idx) else {
            return Err(Error::UnsupportedExport(format!(
                "sample index {idx} out of bounds (samples: {})",
                instrument.samples.len()
            )));
        };
        if let Some(root) = base
            && let Ok(rel) = s.file.strip_prefix(root)
        {
            return Ok(rel.to_string_lossy().replace('\\', "/"));
        }
        Ok(s.file.to_string_lossy().replace('\\', "/"))
    };

    let graph = instrument.graph.as_ref().map(unmap_graph).transpose()?;

    let mut groups = Vec::with_capacity(instrument.groups.len());
    for g in &instrument.groups {
        let generator = g
            .generator
            .as_ref()
            .map(|gen_| unmap_generator(gen_, &sample_ref))
            .transpose()?;
        let is_osc = matches!(g.generator, Some(ir::Generator::Oscillator { .. }));
        let mut zones = Vec::with_capacity(g.zones.len());
        for z in &g.zones {
            let sample = if is_osc && instrument.samples.get(z.sample).is_none() {
                String::new()
            } else {
                sample_ref(z.sample)?
            };
            zones.push(ZoneSpec {
                sample,
                keys: [*z.keys.start(), *z.keys.end()],
                velocities: [*z.velocities.start(), *z.velocities.end()],
                root: z.root,
                tune: z.tune,
                volume_db: z.volume_db,
                pan: z.pan,
                start: z.start,
                end: Some(z.end),
                start_mod: z.start_mod,
                reverse: z.reverse,
                velocity_fade: (z.velocity_fade != (0, 0))
                    .then_some([z.velocity_fade.0, z.velocity_fade.1]),
                key_fade: (z.key_fade != (0, 0)).then_some([z.key_fade.0, z.key_fade.1]),
                loops: z
                    .loops
                    .iter()
                    .map(|l| LoopSpec {
                        start: l.start,
                        end: l.end,
                        crossfade: l.crossfade,
                        count: l.count,
                        alternating: l.alternating,
                        until_release: l.until_release,
                        tune_ratio: l.tune_ratio,
                    })
                    .collect(),
                crossfades: z
                    .crossfades
                    .iter()
                    .map(|xf| CrossfadeSpec {
                        control: match xf.control {
                            ir::CrossfadeControl::Velocity => CrossfadeControlSpec::Velocity,
                            ir::CrossfadeControl::Key => CrossfadeControlSpec::Key,
                            ir::CrossfadeControl::Cc(cc) => CrossfadeControlSpec::Cc { cc },
                        },
                        fade_in: xf.fade_in.map(|(a, b)| [a, b]),
                        fade_out: xf.fade_out.map(|(a, b)| [a, b]),
                        law: match xf.law {
                            ir::CrossfadeLaw::Linear => CrossfadeLawSpec::Linear,
                            ir::CrossfadeLaw::EqualPower => CrossfadeLawSpec::EqualPower,
                        },
                    })
                    .collect(),
            });
        }
        groups.push(GroupSpec {
            name: g.name.clone(),
            volume_db: g.volume_db,
            pan: g.pan,
            tune: g.tune,
            keytrack: g.keytrack,
            trigger: match g.trigger {
                ir::Trigger::Attack => TriggerSpec::Attack,
                ir::Trigger::Release => TriggerSpec::Release,
                ir::Trigger::First => TriggerSpec::First,
                ir::Trigger::Legato => TriggerSpec::Legato,
            },
            one_shot: g.one_shot,
            polyphony: g.polyphony,
            voice_group: g.voice_group,
            off_by: g.off_by,
            bus: g.bus,
            generator,
            envelope: EnvelopeSpec {
                delay: g.envelope.delay,
                attack: g.envelope.attack,
                hold: g.envelope.hold,
                decay: g.envelope.decay,
                sustain: g.envelope.sustain,
                release: g.envelope.release,
                attack_shape: g.envelope.attack_shape,
                decay_shape: g.envelope.decay_shape,
                release_shape: g.envelope.release_shape,
                one_shot: g.envelope.one_shot,
            },
            start: g
                .start
                .iter()
                .map(|(join, rule)| StartRuleSpec {
                    join: match join {
                        ir::Join::And => JoinSpec::And,
                        ir::Join::AndNot => JoinSpec::AndNot,
                        ir::Join::Or => JoinSpec::Or,
                    },
                    rule: match rule {
                        ir::StartRule::Keyswitch { range } => StartRuleKindSpec::Keyswitch {
                            range: [*range.start(), *range.end()],
                        },
                        ir::StartRule::Controller { cc, range } => StartRuleKindSpec::Controller {
                            cc: *cc,
                            range: [*range.start(), *range.end()],
                        },
                        ir::StartRule::RoundRobin {
                            scope,
                            position,
                            length,
                            random,
                        } => StartRuleKindSpec::RoundRobin {
                            scope: match scope {
                                ir::RrScope::Global => RrScopeSpec::Global,
                                ir::RrScope::Key => RrScopeSpec::Key,
                                ir::RrScope::Group => RrScopeSpec::Group,
                            },
                            position: *position,
                            length: *length,
                            random: *random,
                        },
                        ir::StartRule::Cycle { random } => {
                            StartRuleKindSpec::Cycle { random: *random }
                        }
                        ir::StartRule::Random { lo, hi } => {
                            StartRuleKindSpec::Random { lo: *lo, hi: *hi }
                        }
                    },
                })
                .collect(),
            mods: g
                .mods
                .iter()
                .filter_map(|m| {
                    let kind = match m.kind {
                        ir::ModKind::Volume {
                            source,
                            intensity,
                            curve,
                            tau_ms,
                        } => ModKindSpec::Volume {
                            source: unmap_source(source)?,
                            intensity,
                            curve: unmap_curve(curve),
                            tau_ms,
                        },
                        ir::ModKind::Pitch {
                            source,
                            semitones,
                            depth,
                            tau_ms,
                        } => ModKindSpec::Pitch {
                            source: unmap_source(source)?,
                            semitones,
                            depth,
                            tau_ms,
                        },
                        ir::ModKind::Pan { source, amount } => ModKindSpec::Pan {
                            source: unmap_source(source)?,
                            amount,
                        },
                        ir::ModKind::SampleStart { source, depth } => ModKindSpec::SampleStart {
                            source: unmap_source(source)?,
                            depth,
                        },
                        ir::ModKind::Release {
                            source,
                            intensity,
                            scale,
                            envelope: None,
                        } => ModKindSpec::Release {
                            source: unmap_source(source)?,
                            intensity,
                            scale: match scale {
                                ir::TimeScale::Linear { power } => TimeScaleSpec::Linear { power },
                                ir::TimeScale::LogKnob { floor_ms, power } => {
                                    TimeScaleSpec::LogKnob { floor_ms, power }
                                }
                            },
                        },
                        _ => return None,
                    };
                    Some(ModSpec {
                        name: m.name.clone(),
                        bypass: m.bypass,
                        kind,
                    })
                })
                .collect(),
            zones,
        });
    }

    Ok(InstrumentSpec {
        version: FORMAT_VERSION,
        name: instrument.name.clone(),
        voices: instrument.voices,
        steal: unmap_steal(instrument.steal),
        steal_released_first: instrument.steal_released_first,
        steal_fade_ms: instrument.steal_fade_ms,
        voice_groups: instrument
            .voice_groups
            .iter()
            .map(|vg| VoiceGroupSpec {
                voices: vg.voices,
                steal: unmap_steal(vg.steal),
                prefer_released: vg.prefer_released,
                fade_ms: vg.fade_ms,
                exclusive: vg.exclusive,
                per_note: vg.per_note,
                legato: vg.legato,
                returns: vg.returns,
                per_key: vg.per_key,
                quieter_only: vg.quieter_only,
                release: vg.release,
            })
            .collect(),
        buses: instrument
            .buses
            .iter()
            .map(|b| BusSpec {
                name: b.name.clone(),
                volume_db: b.volume_db,
                pan: b.pan,
                to_main: b.to_main,
            })
            .collect(),
        graph,
        groups,
    })
}

fn resolve_sample_path(base: &Path, raw: &str) -> std::path::PathBuf {
    let normalized = raw.trim().replace('\\', "/");
    let p = Path::new(&normalized);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

const fn map_steal(s: StealSpec) -> ir::Steal {
    match s {
        StealSpec::Oldest => ir::Steal::Oldest,
        StealSpec::Newest => ir::Steal::Newest,
        StealSpec::Highest => ir::Steal::Highest,
        StealSpec::Lowest => ir::Steal::Lowest,
        StealSpec::Quietest => ir::Steal::Quietest,
    }
}

fn map_generator(
    g: &GeneratorSpec,
    base: &Path,
    pool: &mut SamplePool,
    notes: &mut Notes,
) -> ir::Generator {
    match g {
        GeneratorSpec::Pcm {
            sample,
            start,
            end,
            reverse,
        } => {
            let sample_path = resolve_sample_path(base, sample);
            let (idx, _) = pool.add(sample.clone(), &sample_path, notes);
            ir::Generator::Pcm {
                sample: idx,
                start: *start,
                end: *end,
                reverse: *reverse,
            }
        }
        GeneratorSpec::Oscillator {
            wave,
            tune_semitones,
            phase,
        } => ir::Generator::Oscillator {
            wave: match *wave {
                OscWaveSpec::Sine => ir::Wave::Sine,
                OscWaveSpec::Saw => ir::Wave::Saw,
                OscWaveSpec::Ramp => ir::Wave::Ramp,
                OscWaveSpec::Square { width } => ir::Wave::Square { width },
                OscWaveSpec::Triangle => ir::Wave::Triangle,
                OscWaveSpec::Noise => ir::Wave::Random,
            },
            tune_semitones: *tune_semitones,
            phase: *phase,
        },
        GeneratorSpec::Granular {
            sample,
            grain_ms,
            density,
            position,
            pitch_semitones,
            spread,
        } => {
            let sample_path = resolve_sample_path(base, sample);
            let (idx, _) = pool.add(sample.clone(), &sample_path, notes);
            ir::Generator::Granular {
                sample: idx,
                grain_ms: *grain_ms,
                density: *density,
                position: *position,
                pitch_semitones: *pitch_semitones,
                spread: *spread,
            }
        }
    }
}

fn unmap_generator(
    g: &ir::Generator,
    sample_ref: &impl Fn(usize) -> Result<String, Error>,
) -> Result<GeneratorSpec, Error> {
    match *g {
        ir::Generator::Pcm {
            sample,
            start,
            end,
            reverse,
        } => Ok(GeneratorSpec::Pcm {
            sample: sample_ref(sample)?,
            start,
            end,
            reverse,
        }),
        ir::Generator::Oscillator {
            wave,
            tune_semitones,
            phase,
        } => {
            let wave = match wave {
                ir::Wave::Sine => OscWaveSpec::Sine,
                ir::Wave::Saw => OscWaveSpec::Saw,
                ir::Wave::Ramp => OscWaveSpec::Ramp,
                ir::Wave::Square { width } => OscWaveSpec::Square { width },
                ir::Wave::Triangle => OscWaveSpec::Triangle,
                ir::Wave::Random => OscWaveSpec::Noise,
                other => {
                    return Err(Error::UnsupportedExport(format!(
                        "oscillator waveform {other:?} is not supported in .kontra"
                    )));
                }
            };
            Ok(GeneratorSpec::Oscillator {
                wave,
                tune_semitones,
                phase,
            })
        }
        ir::Generator::Granular {
            sample,
            grain_ms,
            density,
            position,
            pitch_semitones,
            spread,
        } => Ok(GeneratorSpec::Granular {
            sample: sample_ref(sample)?,
            grain_ms,
            density,
            position,
            pitch_semitones,
            spread,
        }),
        ir::Generator::Wavetable(_) => Err(Error::UnsupportedExport(
            "wavetable generator export is not supported in .kontra".into(),
        )),
    }
}

fn map_layout(l: ChannelLayoutSpec) -> ir::ChannelLayout {
    match l {
        ChannelLayoutSpec::Mono => ir::ChannelLayout::Mono,
        ChannelLayoutSpec::Stereo => ir::ChannelLayout::Stereo,
        ChannelLayoutSpec::Multi { channels } => ir::ChannelLayout::Multi(channels),
        ChannelLayoutSpec::Surround51 => ir::ChannelLayout::Surround51,
        ChannelLayoutSpec::Surround71 => ir::ChannelLayout::Surround71,
        ChannelLayoutSpec::Ambisonics { order } => ir::ChannelLayout::Ambisonics { order },
    }
}

fn unmap_layout(l: ir::ChannelLayout) -> ChannelLayoutSpec {
    match l {
        ir::ChannelLayout::Mono => ChannelLayoutSpec::Mono,
        ir::ChannelLayout::Stereo => ChannelLayoutSpec::Stereo,
        ir::ChannelLayout::Multi(channels) => ChannelLayoutSpec::Multi { channels },
        ir::ChannelLayout::Surround51 => ChannelLayoutSpec::Surround51,
        ir::ChannelLayout::Surround71 => ChannelLayoutSpec::Surround71,
        ir::ChannelLayout::Ambisonics { order } => ChannelLayoutSpec::Ambisonics { order },
    }
}

const fn map_pass(p: PassSpec) -> ir::Pass {
    match p {
        PassSpec::Low => ir::Pass::Low,
        PassSpec::High => ir::Pass::High,
        PassSpec::Band => ir::Pass::Band,
        PassSpec::Notch => ir::Pass::Notch,
    }
}

const fn unmap_pass(p: ir::Pass) -> PassSpec {
    match p {
        ir::Pass::Low => PassSpec::Low,
        ir::Pass::High => PassSpec::High,
        ir::Pass::Band => PassSpec::Band,
        ir::Pass::Notch => PassSpec::Notch,
    }
}

const fn map_eq_shape(s: EqShapeSpec) -> ir::Shape {
    match s {
        EqShapeSpec::Peak => ir::Shape::Peak,
        EqShapeSpec::LowShelf => ir::Shape::LowShelf,
        EqShapeSpec::HighShelf => ir::Shape::HighShelf,
    }
}

const fn unmap_eq_shape(s: ir::Shape) -> EqShapeSpec {
    match s {
        ir::Shape::Peak => EqShapeSpec::Peak,
        ir::Shape::LowShelf => EqShapeSpec::LowShelf,
        ir::Shape::HighShelf => EqShapeSpec::HighShelf,
    }
}

fn map_graph(spec: &AudioGraphSpec) -> ir::AudioGraph {
    let effects = spec
        .effects
        .iter()
        .map(|e| match *e {
            GraphEffectSpec::Gain {
                gain,
                bypass,
                output,
                dry,
            } => ir::Effect {
                kind: ir::EffectKind::Gain(gain),
                bypass,
                output,
                dry,
            },
            GraphEffectSpec::Invert {
                phase,
                swap,
                bypass,
                output,
                dry,
            } => ir::Effect {
                kind: ir::EffectKind::Invert { phase, swap },
                bypass,
                output,
                dry,
            },
            GraphEffectSpec::Stereo {
                width,
                pan,
                bypass,
                output,
                dry,
            } => ir::Effect {
                kind: ir::EffectKind::Stereo { width, pan },
                bypass,
                output,
                dry,
            },
            GraphEffectSpec::Filter {
                pass,
                hz,
                stages,
                damping,
                gain,
                octaves,
                q_octaves,
                bypass,
                output,
                dry,
            } => ir::Effect {
                kind: ir::EffectKind::Filter(ir::Filter {
                    pass: map_pass(pass),
                    hz,
                    stages,
                    ladder: None,
                    damping,
                    gain,
                    octaves,
                    q_octaves,
                }),
                bypass,
                output,
                dry,
            },
            GraphEffectSpec::Eq {
                bands,
                bypass,
                output,
                dry,
            } => ir::Effect {
                kind: ir::EffectKind::Eq(bands.map(|b| ir::Bell {
                    shape: map_eq_shape(b.shape),
                    hz: b.hz,
                    q: b.q,
                    gain_db: b.gain_db,
                })),
                bypass,
                output,
                dry,
            },
            GraphEffectSpec::Sends {
                levels,
                bypass,
                output,
                dry,
            } => ir::Effect {
                kind: ir::EffectKind::Sends(levels),
                bypass,
                output,
                dry,
            },
        })
        .collect();
    let nodes = spec
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| ir::GraphNode {
            id: ir::NodeId(u16::try_from(i).unwrap_or(u16::MAX)),
            kind: match n {
                GraphNodeSpec::GroupInput { group } => {
                    ir::GraphNodeKind::GroupInput { group: *group }
                }
                GraphNodeSpec::Bus {
                    name,
                    effects,
                    gain_db,
                    pan,
                    layout,
                    to_main,
                } => ir::GraphNodeKind::Bus {
                    name: name.clone(),
                    effects: effects.iter().copied().map(ir::EffectId).collect(),
                    gain_db: *gain_db,
                    pan: *pan,
                    layout: map_layout(*layout),
                    to_main: *to_main,
                },
                GraphNodeSpec::Rack { effects, layout } => ir::GraphNodeKind::Rack {
                    effects: effects.iter().copied().map(ir::EffectId).collect(),
                    layout: map_layout(*layout),
                },
                GraphNodeSpec::Split {
                    pre_amp,
                    post_amp,
                    amp_gain,
                    amp_pan,
                } => ir::GraphNodeKind::Split {
                    pre_amp: pre_amp.iter().copied().map(ir::EffectId).collect(),
                    post_amp: post_amp.iter().copied().map(ir::EffectId).collect(),
                    amp_gain: *amp_gain,
                    amp_pan: *amp_pan,
                },
                GraphNodeSpec::Mix { gain, pan } => ir::GraphNodeKind::Mix {
                    gain: *gain,
                    pan: *pan,
                },
                GraphNodeSpec::Output { bus_index, layout } => ir::GraphNodeKind::Output {
                    bus_index: *bus_index,
                    layout: map_layout(*layout),
                },
            },
        })
        .collect();
    let edges = spec
        .edges
        .iter()
        .map(|e| ir::GraphEdge {
            from: ir::NodeId(e.from),
            to: ir::NodeId(e.to),
            gain: e.gain,
            post_fader: e.post_fader,
            sidechain: e.sidechain,
        })
        .collect();
    ir::AudioGraph {
        nodes,
        edges,
        effects,
    }
}

fn unmap_graph(graph: &ir::AudioGraph) -> Result<AudioGraphSpec, Error> {
    let mut effects = Vec::with_capacity(graph.effects.len());
    for e in &graph.effects {
        let mapped = match &e.kind {
            ir::EffectKind::Gain(gain) => GraphEffectSpec::Gain {
                gain: *gain,
                bypass: e.bypass,
                output: e.output,
                dry: e.dry,
            },
            ir::EffectKind::Invert { phase, swap } => GraphEffectSpec::Invert {
                phase: *phase,
                swap: *swap,
                bypass: e.bypass,
                output: e.output,
                dry: e.dry,
            },
            ir::EffectKind::Stereo { width, pan } => GraphEffectSpec::Stereo {
                width: *width,
                pan: *pan,
                bypass: e.bypass,
                output: e.output,
                dry: e.dry,
            },
            ir::EffectKind::Filter(f) if f.ladder.is_none() => GraphEffectSpec::Filter {
                pass: unmap_pass(f.pass),
                hz: f.hz,
                stages: f.stages,
                damping: f.damping,
                gain: f.gain,
                octaves: f.octaves,
                q_octaves: f.q_octaves,
                bypass: e.bypass,
                output: e.output,
                dry: e.dry,
            },
            ir::EffectKind::Eq(bands) => GraphEffectSpec::Eq {
                bands: bands.map(|b| BellSpec {
                    shape: unmap_eq_shape(b.shape),
                    hz: b.hz,
                    q: b.q,
                    gain_db: b.gain_db,
                }),
                bypass: e.bypass,
                output: e.output,
                dry: e.dry,
            },
            ir::EffectKind::Sends(levels) => GraphEffectSpec::Sends {
                levels: *levels,
                bypass: e.bypass,
                output: e.output,
                dry: e.dry,
            },
            other => {
                return Err(Error::UnsupportedExport(format!(
                    "graph effect {:?} cannot be exported losslessly to .kontra",
                    other
                )));
            }
        };
        effects.push(mapped);
    }
    let nodes = graph
        .nodes
        .iter()
        .map(|n| match &n.kind {
            ir::GraphNodeKind::GroupInput { group } => GraphNodeSpec::GroupInput { group: *group },
            ir::GraphNodeKind::Bus {
                name,
                effects,
                gain_db,
                pan,
                layout,
                to_main,
            } => GraphNodeSpec::Bus {
                name: name.clone(),
                effects: effects.iter().map(|e| e.0).collect(),
                gain_db: *gain_db,
                pan: *pan,
                layout: unmap_layout(*layout),
                to_main: *to_main,
            },
            ir::GraphNodeKind::Rack { effects, layout } => GraphNodeSpec::Rack {
                effects: effects.iter().map(|e| e.0).collect(),
                layout: unmap_layout(*layout),
            },
            ir::GraphNodeKind::Split {
                pre_amp,
                post_amp,
                amp_gain,
                amp_pan,
            } => GraphNodeSpec::Split {
                pre_amp: pre_amp.iter().map(|e| e.0).collect(),
                post_amp: post_amp.iter().map(|e| e.0).collect(),
                amp_gain: *amp_gain,
                amp_pan: *amp_pan,
            },
            ir::GraphNodeKind::Mix { gain, pan } => GraphNodeSpec::Mix {
                gain: *gain,
                pan: *pan,
            },
            ir::GraphNodeKind::Output { bus_index, layout } => GraphNodeSpec::Output {
                bus_index: *bus_index,
                layout: unmap_layout(*layout),
            },
        })
        .collect();
    let edges = graph
        .edges
        .iter()
        .map(|e| GraphEdgeSpec {
            from: e.from.0,
            to: e.to.0,
            gain: e.gain,
            post_fader: e.post_fader,
            sidechain: e.sidechain,
        })
        .collect();
    Ok(AudioGraphSpec {
        nodes,
        edges,
        effects,
    })
}

const fn unmap_steal(s: ir::Steal) -> StealSpec {
    match s {
        ir::Steal::Oldest => StealSpec::Oldest,
        ir::Steal::Newest => StealSpec::Newest,
        ir::Steal::Highest => StealSpec::Highest,
        ir::Steal::Lowest => StealSpec::Lowest,
        ir::Steal::Quietest => StealSpec::Quietest,
    }
}

const fn map_note_expr(e: NoteExprSpec) -> ir::NoteExpr {
    match e {
        NoteExprSpec::Pressure => ir::NoteExpr::Pressure,
        NoteExprSpec::Brightness => ir::NoteExpr::Brightness,
        NoteExprSpec::Vibrato => ir::NoteExpr::Vibrato,
        NoteExprSpec::Expression => ir::NoteExpr::Expression,
        NoteExprSpec::Tune => ir::NoteExpr::Tune,
        NoteExprSpec::Gain => ir::NoteExpr::Gain,
        NoteExprSpec::Pan => ir::NoteExpr::Pan,
    }
}

const fn unmap_note_expr(e: ir::NoteExpr) -> NoteExprSpec {
    match e {
        ir::NoteExpr::Pressure => NoteExprSpec::Pressure,
        ir::NoteExpr::Brightness => NoteExprSpec::Brightness,
        ir::NoteExpr::Vibrato => NoteExprSpec::Vibrato,
        ir::NoteExpr::Expression => NoteExprSpec::Expression,
        ir::NoteExpr::Tune => NoteExprSpec::Tune,
        ir::NoteExpr::Gain => NoteExprSpec::Gain,
        ir::NoteExpr::Pan => NoteExprSpec::Pan,
    }
}

const fn map_source(s: SourceSpec) -> ir::Source {
    match s {
        SourceSpec::Velocity => ir::Source::Velocity,
        SourceSpec::ReleaseVelocity => ir::Source::ReleaseVelocity,
        SourceSpec::Controller { cc } => ir::Source::Controller(cc),
        SourceSpec::ControllerFrom { cc, start } => ir::Source::ControllerFrom { cc, start },
        SourceSpec::PitchBend => ir::Source::PitchBend,
        SourceSpec::Pressure => ir::Source::Pressure,
        SourceSpec::NoteExpr { expr } => ir::Source::NoteExpr(map_note_expr(expr)),
        SourceSpec::PerNoteCc { registered, index } => ir::Source::PerNoteCc { registered, index },
        SourceSpec::Key => ir::Source::Key,
        SourceSpec::Constant => ir::Source::Constant,
        SourceSpec::HeldTime { span_ms } => ir::Source::HeldTime { span_ms },
        SourceSpec::Random { bipolar } => ir::Source::Random { bipolar },
    }
}

const fn unmap_source(s: ir::Source) -> Option<SourceSpec> {
    Some(match s {
        ir::Source::Velocity => SourceSpec::Velocity,
        ir::Source::ReleaseVelocity => SourceSpec::ReleaseVelocity,
        ir::Source::Controller(cc) => SourceSpec::Controller { cc },
        ir::Source::ControllerFrom { cc, start } => SourceSpec::ControllerFrom { cc, start },
        ir::Source::PitchBend => SourceSpec::PitchBend,
        ir::Source::Pressure => SourceSpec::Pressure,
        ir::Source::NoteExpr(expr) => SourceSpec::NoteExpr {
            expr: unmap_note_expr(expr),
        },
        ir::Source::PerNoteCc { registered, index } => SourceSpec::PerNoteCc { registered, index },
        ir::Source::Key => SourceSpec::Key,
        ir::Source::Constant => SourceSpec::Constant,
        ir::Source::HeldTime { span_ms } => SourceSpec::HeldTime { span_ms },
        ir::Source::Random { bipolar } => SourceSpec::Random { bipolar },
        ir::Source::Envelope(_)
        | ir::Source::Shaped(_)
        | ir::Source::Lfo(_)
        | ir::Source::Blend(_)
        | ir::Source::Script(_)
        | ir::Source::ZoneEnvelope(_) => return None,
    })
}

const fn map_curve(c: CurveSpec) -> ir::Curve {
    match c {
        CurveSpec::Decibels { range } => ir::Curve::Decibels(range),
        CurveSpec::Depth { power } => ir::Curve::Depth { power },
        CurveSpec::Squared => ir::Curve::Squared,
        CurveSpec::Falling => ir::Curve::Falling,
        CurveSpec::Signed => ir::Curve::Signed,
    }
}

const fn unmap_curve(c: ir::Curve) -> CurveSpec {
    match c {
        ir::Curve::Decibels(range) => CurveSpec::Decibels { range },
        ir::Curve::Depth { power } => CurveSpec::Depth { power },
        ir::Curve::Squared => CurveSpec::Squared,
        ir::Curve::Falling => CurveSpec::Falling,
        ir::Curve::Signed => CurveSpec::Signed,
    }
}
