//! Structural validation of an [`Instrument`]: checks index bounds, loop and
//! zone ranges, bus send cycles, and engine modulation limits without mutating
//! the IR.

use crate::{
    CrossfadeControl, Diagnostic, Generator, GraphNodeKind, Impact, Instrument, ModKind, Source,
    StartRule,
};

/// Maximum per-group LFOs or breakpoint envelopes the engine tracks per voice
/// (`kontra_plan::modulation::WARPS`).
const MAX_TIMED: usize = 8;

/// Validates structural invariants of `instrument`, returning a diagnostic for
/// every violation found. A well-formed instrument yields an empty vector.
#[must_use]
pub fn validate(instrument: &Instrument) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let sample_count = instrument.samples.len();
    let bus_count = instrument.buses.len();
    let vg_count = instrument.voice_groups.len();

    for (gi, g) in instrument.groups.iter().enumerate() {
        if bus_count > 0 && g.bus >= bus_count {
            out.push(Diagnostic {
                impact: Impact::Broken,
                feature: "ir.group.bus_out_of_bounds".into(),
                detail: format!("group {gi} references bus {} (buses: {bus_count})", g.bus),
            });
        }
        if let Some(vg) = g.voice_group
            && vg >= vg_count
        {
            out.push(Diagnostic {
                impact: Impact::Broken,
                feature: "ir.group.voice_group_out_of_bounds".into(),
                detail: format!(
                    "group {gi} references voice_group {vg} (voice_groups: {vg_count})"
                ),
            });
        }
        if let Some(vg) = g.off_by
            && vg >= vg_count
        {
            out.push(Diagnostic {
                impact: Impact::Broken,
                feature: "ir.group.off_by_out_of_bounds".into(),
                detail: format!("group {gi} references off_by {vg} (voice_groups: {vg_count})"),
            });
        }
        if g.lfos.len() > MAX_TIMED {
            out.push(Diagnostic {
                impact: Impact::Audible,
                feature: "ir.group.too_many_lfos".into(),
                detail: format!(
                    "group {gi} has {} LFOs (engine limit {MAX_TIMED})",
                    g.lfos.len()
                ),
            });
        }
        if g.envelopes.len() > MAX_TIMED {
            out.push(Diagnostic {
                impact: Impact::Audible,
                feature: "ir.group.too_many_envelopes".into(),
                detail: format!(
                    "group {gi} has {} envelopes (engine limit {MAX_TIMED})",
                    g.envelopes.len()
                ),
            });
        }

        if let Some(generator) = &g.generator {
            match generator {
                Generator::Pcm {
                    sample, start, end, ..
                } => {
                    if *sample >= sample_count {
                        out.push(Diagnostic {
                            impact: Impact::Broken,
                            feature: "ir.generator.sample_out_of_bounds".into(),
                            detail: format!(
                                "group {gi} PCM generator references sample {sample} (samples: {sample_count})"
                            ),
                        });
                    }
                    if *end > 0 && *start > *end {
                        out.push(Diagnostic {
                            impact: Impact::Broken,
                            feature: "ir.generator.invalid_bounds".into(),
                            detail: format!(
                                "group {gi} PCM generator has start {start} > end {end}"
                            ),
                        });
                    }
                }
                Generator::Oscillator {
                    tune_semitones,
                    phase,
                    ..
                } => {
                    if !tune_semitones.is_finite() || !phase.is_finite() {
                        out.push(Diagnostic {
                            impact: Impact::Broken,
                            feature: "ir.generator.non_finite_param".into(),
                            detail: format!(
                                "group {gi} oscillator generator has non-finite tune ({tune_semitones}) or phase ({phase})"
                            ),
                        });
                    }
                }
                Generator::Granular {
                    sample,
                    grain_ms,
                    density,
                    position,
                    pitch_semitones,
                    spread,
                } => {
                    if *sample >= sample_count {
                        out.push(Diagnostic {
                            impact: Impact::Broken,
                            feature: "ir.generator.sample_out_of_bounds".into(),
                            detail: format!(
                                "group {gi} granular generator references sample {sample} (samples: {sample_count})"
                            ),
                        });
                    }
                    if !grain_ms.is_finite()
                        || *grain_ms <= 0.0
                        || !density.is_finite()
                        || *density <= 0.0
                        || !position.is_finite()
                        || !pitch_semitones.is_finite()
                        || !spread.is_finite()
                    {
                        out.push(Diagnostic {
                            impact: Impact::Broken,
                            feature: "ir.generator.invalid_granular".into(),
                            detail: format!(
                                "group {gi} granular generator has invalid parameters (grain_ms={grain_ms}, density={density})"
                            ),
                        });
                    }
                }
                Generator::Wavetable(_) => {}
            }
        }

        let is_oscillator = matches!(g.generator, Some(Generator::Oscillator { .. }));
        for (zi, z) in g.zones.iter().enumerate() {
            if !is_oscillator && z.sample >= sample_count {
                out.push(Diagnostic {
                    impact: Impact::Broken,
                    feature: "ir.zone.sample_out_of_bounds".into(),
                    detail: format!(
                        "group {gi} zone {zi} (id {}) references sample {} (samples: {sample_count})",
                        z.id, z.sample
                    ),
                });
            }
            if z.end > 0 && z.start > z.end {
                out.push(Diagnostic {
                    impact: Impact::Broken,
                    feature: "ir.zone.invalid_bounds".into(),
                    detail: format!(
                        "group {gi} zone {zi} (id {}) has start {} > end {}",
                        z.id, z.start, z.end
                    ),
                });
            }
            for (li, l) in z.loops.iter().enumerate() {
                if l.start > l.end {
                    out.push(Diagnostic {
                        impact: Impact::Broken,
                        feature: "ir.loop.invalid_bounds".into(),
                        detail: format!(
                            "group {gi} zone {zi} (id {}) loop {li} has start {} > end {}",
                            z.id, l.start, l.end
                        ),
                    });
                }
            }
            for (xi, xf) in z.crossfades.iter().enumerate() {
                if let CrossfadeControl::Cc(cc) = xf.control
                    && cc > 127
                {
                    out.push(Diagnostic {
                        impact: Impact::Broken,
                        feature: "ir.zone.cc_out_of_bounds".into(),
                        detail: format!(
                            "group {gi} zone {zi} (id {}) crossfade {xi} references cc {cc} (> 127)",
                            z.id
                        ),
                    });
                }
                if xf.fade_in.is_some_and(|(lo, hi)| lo > hi)
                    || xf.fade_out.is_some_and(|(lo, hi)| lo > hi)
                {
                    out.push(Diagnostic {
                        impact: Impact::Broken,
                        feature: "ir.zone.invalid_crossfade".into(),
                        detail: format!(
                            "group {gi} zone {zi} (id {}) crossfade {xi} has inverted bounds",
                            z.id
                        ),
                    });
                }
            }
        }
        for (ri, (_, rule)) in g.start.iter().enumerate() {
            if let StartRule::Random { lo, hi } = rule
                && (lo > hi || *hi > 10_000)
            {
                out.push(Diagnostic {
                    impact: Impact::Broken,
                    feature: "ir.start.invalid_random_range".into(),
                    detail: format!(
                        "group {gi} start rule {ri} has invalid random range [{lo}, {hi})"
                    ),
                });
            }
        }

        let limits = SourceLimits {
            lfos: g.lfos.len(),
            envelopes: g.envelopes.len(),
            shapers: g.shapers.len(),
            blends: g.blends.len(),
        };
        for (mi, m) in g.mods.iter().enumerate() {
            check_mod_kind(
                &m.kind,
                limits,
                g.zones.len(),
                &format!("group {gi} mod {mi}"),
                &mut out,
            );
        }
        for (si, s) in g.shapers.iter().enumerate() {
            check_source(
                s.source,
                limits,
                &format!("group {gi} shaper {si}"),
                &mut out,
            );
        }
        for (bi, b) in g.blends.iter().enumerate() {
            for (ti, t) in b.terms.iter().enumerate() {
                check_source(
                    t.input.source,
                    limits,
                    &format!("group {gi} blend {bi} term {ti}"),
                    &mut out,
                );
                for (fi, f) in t.factors.iter().enumerate() {
                    check_source(
                        f.input.source,
                        limits,
                        &format!("group {gi} blend {bi} term {ti} factor {fi}"),
                        &mut out,
                    );
                }
            }
        }
    }

    // Bus send bounds and cycle detection (3-color DFS).
    for (bi, bus) in instrument.buses.iter().enumerate() {
        for (si, send) in bus.sends.iter().enumerate() {
            if send.bus >= bus_count {
                out.push(Diagnostic {
                    impact: Impact::Broken,
                    feature: "ir.bus.send_out_of_bounds".into(),
                    detail: format!(
                        "bus {bi} send {si} references bus {} (buses: {bus_count})",
                        send.bus
                    ),
                });
            }
        }
        let limits = SourceLimits {
            lfos: bus.lfos.len(),
            envelopes: 0,
            shapers: 0,
            blends: bus.blends.len(),
        };
        for (mi, kind) in bus.mods.iter().enumerate() {
            check_mod_kind(kind, limits, 0, &format!("bus {bi} mod {mi}"), &mut out);
        }
    }
    detect_bus_cycles(instrument, &mut out);

    for (ii, imp) in instrument.impulses.iter().enumerate() {
        if imp.sample >= sample_count {
            out.push(Diagnostic {
                impact: Impact::Broken,
                feature: "ir.impulse.sample_out_of_bounds".into(),
                detail: format!(
                    "impulse {ii} references sample {} (samples: {sample_count})",
                    imp.sample
                ),
            });
        }
    }

    let knob_limits = SourceLimits {
        lfos: instrument.knob_lfos.len(),
        envelopes: 0,
        shapers: 0,
        blends: instrument.knob_blends.len(),
    };
    for (ki, km) in instrument.knob_mods.iter().enumerate() {
        check_source(km.source, knob_limits, &format!("knob_mod {ki}"), &mut out);
    }

    if let Some(graph) = &instrument.graph {
        out.extend(graph.validate());
        let has_legacy_racks = instrument.inserts.iter().any(Option::is_some)
            || instrument.sends.iter().any(Option::is_some)
            || instrument.main.iter().any(Option::is_some);
        if has_legacy_racks {
            out.push(Diagnostic {
                impact: Impact::Broken,
                feature: "ir.graph.conflicts_with_legacy_racks".into(),
                detail: "instrument defines both an audio graph and legacy insert/send/main racks"
                    .into(),
            });
        }
        let group_count = instrument.groups.len();
        for (ni, node) in graph.nodes.iter().enumerate() {
            if let GraphNodeKind::GroupInput { group } = node.kind
                && usize::from(group) >= group_count
            {
                out.push(Diagnostic {
                    impact: Impact::Broken,
                    feature: "ir.graph.group_out_of_bounds".into(),
                    detail: format!(
                        "graph node {ni} references group {group} (groups: {group_count})"
                    ),
                });
            }
        }
    }

    out
}

#[derive(Clone, Copy)]
struct SourceLimits {
    lfos: usize,
    envelopes: usize,
    shapers: usize,
    blends: usize,
}

fn check_source(source: Source, limits: SourceLimits, where_: &str, out: &mut Vec<Diagnostic>) {
    match source {
        Source::Lfo(n) if usize::from(n) >= limits.lfos => out.push(Diagnostic {
            impact: Impact::Broken,
            feature: "ir.mod.lfo_out_of_bounds".into(),
            detail: format!("{where_} references LFO {n} (lfos: {})", limits.lfos),
        }),
        Source::Envelope(n) if usize::from(n) >= limits.envelopes => out.push(Diagnostic {
            impact: Impact::Broken,
            feature: "ir.mod.envelope_out_of_bounds".into(),
            detail: format!(
                "{where_} references envelope {n} (envelopes: {})",
                limits.envelopes
            ),
        }),
        Source::Shaped(n) if usize::from(n) >= limits.shapers => out.push(Diagnostic {
            impact: Impact::Broken,
            feature: "ir.mod.shaper_out_of_bounds".into(),
            detail: format!(
                "{where_} references shaper {n} (shapers: {})",
                limits.shapers
            ),
        }),
        Source::Blend(n) if usize::from(n) >= limits.blends => out.push(Diagnostic {
            impact: Impact::Broken,
            feature: "ir.mod.blend_out_of_bounds".into(),
            detail: format!("{where_} references blend {n} (blends: {})", limits.blends),
        }),
        _ => {}
    }
}

fn check_mod_kind(
    kind: &ModKind,
    limits: SourceLimits,
    zones: usize,
    where_: &str,
    out: &mut Vec<Diagnostic>,
) {
    match *kind {
        ModKind::Envelope => {}
        ModKind::Volume { source, .. }
        | ModKind::Pitch { source, .. }
        | ModKind::Pan { source, .. }
        | ModKind::Cutoff { source, .. }
        | ModKind::CutoffHz { source, .. }
        | ModKind::Resonance { source, .. }
        | ModKind::EqGain { source, .. }
        | ModKind::SampleStart { source, .. }
        | ModKind::LoopShift { source, .. }
        | ModKind::Wavetable { source, .. }
        | ModKind::EffectParam { source, .. }
        | ModKind::Send { source, .. } => check_source(source, limits, where_, out),
        ModKind::Attack {
            source, envelope, ..
        }
        | ModKind::Hold {
            source, envelope, ..
        }
        | ModKind::Decay {
            source, envelope, ..
        }
        | ModKind::Sustain {
            source, envelope, ..
        }
        | ModKind::Release {
            source, envelope, ..
        } => {
            check_source(source, limits, where_, out);
            if let Some(e) = envelope
                && usize::from(e) >= limits.envelopes
            {
                out.push(Diagnostic {
                    impact: Impact::Broken,
                    feature: "ir.mod.envelope_out_of_bounds".into(),
                    detail: format!(
                        "{where_} targets envelope {e} (envelopes: {})",
                        limits.envelopes
                    ),
                });
            }
        }
        ModKind::LfoPhase { source, lfo, .. }
        | ModKind::LfoRate { source, lfo, .. }
        | ModKind::LfoHz { source, lfo, .. }
        | ModKind::LfoDivision { source, lfo, .. } => {
            check_source(source, limits, where_, out);
            if usize::from(lfo) >= limits.lfos {
                out.push(Diagnostic {
                    impact: Impact::Broken,
                    feature: "ir.mod.lfo_out_of_bounds".into(),
                    detail: format!("{where_} targets LFO {lfo} (lfos: {})", limits.lfos),
                });
            }
        }
        ModKind::Intensity { source, of, .. } => {
            check_source(source, limits, where_, out);
            check_source(of, limits, where_, out);
        }
        ModKind::Mix { source, zone, .. } => {
            check_source(source, limits, where_, out);
            if usize::from(zone) >= zones {
                out.push(Diagnostic {
                    impact: Impact::Broken,
                    feature: "ir.mod.zone_out_of_bounds".into(),
                    detail: format!("{where_} targets zone {zone} (zones: {zones})"),
                });
            }
        }
    }
}

fn detect_bus_cycles(instrument: &Instrument, out: &mut Vec<Diagnostic>) {
    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;

    let n = instrument.buses.len();
    let mut color = vec![WHITE; n];
    // Iterative 3-color DFS avoids deep recursion on long bus chains.
    for start in 0..n {
        if color.get(start).copied() != Some(WHITE) {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        if let Some(c) = color.get_mut(start) {
            *c = GRAY;
        }
        while let Some((u, next_edge)) = stack.last_mut() {
            let u = *u;
            let Some(bus) = instrument.buses.get(u) else {
                stack.pop();
                continue;
            };
            if let Some(send) = bus.sends.get(*next_edge) {
                let v = send.bus;
                *next_edge += 1;
                match color.get(v).copied() {
                    Some(WHITE) => {
                        if let Some(c) = color.get_mut(v) {
                            *c = GRAY;
                        }
                        stack.push((v, 0));
                    }
                    Some(GRAY) => {
                        out.push(Diagnostic {
                            impact: Impact::Broken,
                            feature: "ir.bus.send_cycle".into(),
                            detail: format!("bus send cycle detected on edge {u} -> {v}"),
                        });
                    }
                    _ => {}
                }
            } else {
                if let Some(c) = color.get_mut(u) {
                    *c = BLACK;
                }
                stack.pop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{
        Blend, Breakpoints, Bus, BusSend, Clock, Curve, Group, Lfo, Loop, Mod, Rate, Sample,
        Transfer, Wave, Zone,
    };

    fn sample() -> Sample {
        Sample {
            name: "s".into(),
            file: PathBuf::from("s.wav"),
            offset: 0,
            len: None,
            channels: 2,
            frames: 1000,
            rate: 48_000,
            keystream: None,
            raw: None,
        }
    }

    fn zone(sample: usize) -> Zone {
        Zone {
            id: 0,
            sample,
            keys: 0..=127,
            velocities: 1..=127,
            velocity_fade: (0, 0),
            key_fade: (0, 0),
            crossfades: Vec::new(),
            root: 60,
            tune: 0.0,
            volume_db: 0.0,
            pan: 0.0,
            start: 0,
            end: 1000,
            start_mod: 0,
            loops: Vec::new(),
            reverse: false,
            mix: Vec::new(),
            slot: 0,
            envelopes: Vec::new(),
        }
    }

    fn lfo() -> Lfo {
        Lfo {
            wave: Wave::Sine,
            rate: Rate::Hz(1.0),
            phase: 0.0,
            fade_ms: 0.0,
            clock: Clock::Voice,
        }
    }

    fn env() -> Breakpoints {
        Breakpoints {
            start: 0.0,
            points: Vec::new(),
            sustain: None,
            loop_from: None,
            stages: false,
        }
    }

    fn vol_mod(source: Source) -> Mod {
        Mod {
            name: String::new(),
            slot: 0,
            target: 0,
            bypass: false,
            kind: ModKind::Volume {
                source,
                intensity: 1.0,
                curve: Curve::Decibels(24.0),
                tau_ms: 0.0,
            },
        }
    }

    #[test]
    fn valid_instrument_has_no_diagnostics() {
        let mut bus0 = Bus::new("Main");
        bus0.sends.push(BusSend {
            bus: 1,
            level: 0.5,
            post_fader: true,
        });
        let bus1 = Bus::new("Aux");
        let mut z = zone(0);
        z.loops.push(Loop {
            start: 100,
            end: 500,
            ..Loop::default()
        });
        let inst = Instrument {
            samples: vec![sample()],
            buses: vec![bus0, bus1],
            groups: vec![Group {
                bus: 0,
                zones: vec![z],
                lfos: vec![lfo()],
                envelopes: vec![env()],
                shapers: vec![Transfer {
                    source: Source::Lfo(0),
                    ..Transfer::default()
                }],
                blends: vec![Blend {
                    base: 0.0,
                    terms: Vec::new(),
                }],
                mods: vec![
                    vol_mod(Source::Lfo(0)),
                    vol_mod(Source::Envelope(0)),
                    vol_mod(Source::Shaped(0)),
                    vol_mod(Source::Blend(0)),
                ],
                ..Group::DEFAULT
            }],
            ..Instrument::DEFAULT
        };
        assert!(validate(&inst).is_empty());
    }

    #[test]
    fn reports_zone_sample_out_of_bounds() {
        let inst = Instrument {
            samples: vec![sample()],
            groups: vec![Group {
                zones: vec![zone(1)],
                ..Group::DEFAULT
            }],
            ..Instrument::DEFAULT
        };
        let diags = validate(&inst);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].feature, "ir.zone.sample_out_of_bounds");
    }

    #[test]
    fn reports_invalid_zone_and_loop_bounds() {
        let mut z = zone(0);
        z.start = 500;
        z.end = 200;
        z.loops.push(Loop {
            start: 300,
            end: 100,
            ..Loop::default()
        });
        let inst = Instrument {
            samples: vec![sample()],
            groups: vec![Group {
                zones: vec![z],
                ..Group::DEFAULT
            }],
            ..Instrument::DEFAULT
        };
        let features: Vec<_> = validate(&inst).into_iter().map(|d| d.feature).collect();
        assert_eq!(
            features,
            ["ir.zone.invalid_bounds", "ir.loop.invalid_bounds"]
        );

        // `end == 0` means whole sample / unset and is allowed.
        let mut open_end = zone(0);
        open_end.start = 100;
        open_end.end = 0;
        let inst_open = Instrument {
            samples: vec![sample()],
            groups: vec![Group {
                zones: vec![open_end],
                ..Group::DEFAULT
            }],
            ..Instrument::DEFAULT
        };
        assert!(validate(&inst_open).is_empty());
    }

    #[test]
    fn reports_group_bus_out_of_bounds_only_when_buses_non_empty() {
        let no_buses = Instrument {
            groups: vec![Group {
                bus: 0,
                ..Group::DEFAULT
            }],
            ..Instrument::DEFAULT
        };
        assert!(validate(&no_buses).is_empty());

        let with_buses = Instrument {
            buses: vec![Bus::new("Main")],
            groups: vec![Group {
                bus: 2,
                ..Group::DEFAULT
            }],
            ..Instrument::DEFAULT
        };
        let diags = validate(&with_buses);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].feature, "ir.group.bus_out_of_bounds");
    }

    #[test]
    fn reports_bus_send_out_of_bounds_and_cycles() {
        let mut a = Bus::new("A");
        a.sends.push(BusSend {
            bus: 1,
            level: 1.0,
            post_fader: true,
        });
        a.sends.push(BusSend {
            bus: 9,
            level: 1.0,
            post_fader: true,
        });
        let mut b = Bus::new("B");
        b.sends.push(BusSend {
            bus: 0,
            level: 1.0,
            post_fader: true,
        });
        let inst = Instrument {
            buses: vec![a, b],
            ..Instrument::DEFAULT
        };
        let features: Vec<_> = validate(&inst).into_iter().map(|d| d.feature).collect();
        assert_eq!(features, ["ir.bus.send_out_of_bounds", "ir.bus.send_cycle"]);
    }

    #[test]
    fn reports_self_loop_bus_cycle() {
        let mut a = Bus::new("A");
        a.sends.push(BusSend {
            bus: 0,
            level: 1.0,
            post_fader: true,
        });
        let inst = Instrument {
            buses: vec![a],
            ..Instrument::DEFAULT
        };
        let features: Vec<_> = validate(&inst).into_iter().map(|d| d.feature).collect();
        assert_eq!(features, ["ir.bus.send_cycle"]);
    }

    #[test]
    fn reports_mod_source_out_of_bounds_and_excess_timed_modulators() {
        let inst = Instrument {
            groups: vec![Group {
                lfos: vec![lfo(); 9],
                envelopes: vec![env(); 9],
                mods: vec![
                    vol_mod(Source::Lfo(9)),
                    vol_mod(Source::Envelope(9)),
                    vol_mod(Source::Shaped(0)),
                    vol_mod(Source::Blend(0)),
                ],
                ..Group::DEFAULT
            }],
            ..Instrument::DEFAULT
        };
        let features: Vec<_> = validate(&inst).into_iter().map(|d| d.feature).collect();
        assert_eq!(
            features,
            [
                "ir.group.too_many_lfos",
                "ir.group.too_many_envelopes",
                "ir.mod.lfo_out_of_bounds",
                "ir.mod.envelope_out_of_bounds",
                "ir.mod.shaper_out_of_bounds",
                "ir.mod.blend_out_of_bounds",
            ]
        );
    }

    #[test]
    fn validates_phase3_generators_and_audio_graph() {
        // Valid oscillator group needs no samples in `instrument.samples`.
        let mut graph = crate::AudioGraph::default();
        let g0 = graph.push_node(GraphNodeKind::GroupInput { group: 0 });
        let out = graph.push_node(GraphNodeKind::Output {
            bus_index: 0,
            layout: crate::ChannelLayout::Stereo,
        });
        graph.connect(g0, out, 1.0, true);
        let valid_osc = Instrument {
            groups: vec![Group {
                generator: Some(Generator::Oscillator {
                    wave: Wave::Saw,
                    tune_semitones: 0.0,
                    phase: 0.0,
                }),
                zones: vec![zone(0)],
                ..Group::DEFAULT
            }],
            graph: Some(graph),
            ..Instrument::DEFAULT
        };
        assert!(validate(&valid_osc).is_empty());

        // Invalid generator parameters & out-of-bounds graph group input.
        let mut bad_graph = crate::AudioGraph::default();
        let bad_in = bad_graph.push_node(GraphNodeKind::GroupInput { group: 5 });
        let bad_out = bad_graph.push_node(GraphNodeKind::Output {
            bus_index: 0,
            layout: crate::ChannelLayout::Stereo,
        });
        bad_graph.connect(bad_in, bad_out, f32::NAN, true);
        let bad_inst = Instrument {
            groups: vec![
                Group {
                    generator: Some(Generator::Oscillator {
                        wave: Wave::Sine,
                        tune_semitones: f32::INFINITY,
                        phase: 0.0,
                    }),
                    ..Group::DEFAULT
                },
                Group {
                    generator: Some(Generator::Granular {
                        sample: 2,
                        grain_ms: 0.0,
                        density: -1.0,
                        position: 0.0,
                        pitch_semitones: 0.0,
                        spread: 0.0,
                    }),
                    ..Group::DEFAULT
                },
            ],
            graph: Some(bad_graph),
            ..Instrument::DEFAULT
        };
        let features: Vec<_> = validate(&bad_inst).into_iter().map(|d| d.feature).collect();
        assert_eq!(
            features,
            [
                "ir.generator.non_finite_param",
                "ir.generator.sample_out_of_bounds",
                "ir.generator.invalid_granular",
                "ir.graph.non_finite_gain",
                "ir.graph.group_out_of_bounds",
            ]
        );

        // Defining legacy insert/send/main racks alongside an explicit graph is flagged.
        let mut conflict = valid_osc;
        conflict.inserts.push(Some(crate::Effect {
            kind: crate::EffectKind::Gain(0.5),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        }));
        let conflict_features: Vec<_> =
            validate(&conflict).into_iter().map(|d| d.feature).collect();
        assert_eq!(conflict_features, ["ir.graph.conflicts_with_legacy_racks"]);
    }
}
