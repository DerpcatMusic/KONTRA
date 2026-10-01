//! What drives a group and what it plays through, as lists: its
//! modulation (source to target, depth, and the source's value now, where
//! the editor can see it) and its effects (the group's inserts, then the
//! instrument's chains), each with its bypass. Read-only: KONTRA plays
//! these as the library set them.

use super::theme::*;
use super::viz;
use crate::fx::{Chain, Effect, Params};
use crate::import::{Group, Instrument};
use crate::modulation::{ModAssignment, ModSource, ModTarget};
use crate::plugin::SamplerParams;
use moose::mui::mui::prelude::*;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

pub fn source_name(s: &ModSource) -> String {
    match s {
        ModSource::PitchBend => "Pitch bend".into(),
        ModSource::PolyAftertouch => "Poly pressure".into(),
        ModSource::MonoAftertouch => "Pressure".into(),
        ModSource::MidiCc(1) => "Mod wheel".into(),
        ModSource::MidiCc(n) => format!("CC {n}"),
        ModSource::KeyPosition => "Key".into(),
        ModSource::Velocity => "Velocity".into(),
        ModSource::ReleaseVelocity => "Release velocity".into(),
        ModSource::ReleaseTriggerCounter => "Release counter".into(),
        ModSource::Constant => "Constant".into(),
        ModSource::RandomUnipolar => "Random".into(),
        ModSource::RandomBipolar => "Random ±".into(),
        ModSource::Script(n) => format!("Script {}", n + 1),
        ModSource::Unassigned => "None".into(),
    }
}

pub fn target_name(t: &ModTarget) -> String {
    match t {
        ModTarget::Volume => "Volume".into(),
        ModTarget::Pitch => "Pitch".into(),
        ModTarget::Group(param) => param.clone(),
        ModTarget::SampleStart => "Sample start".into(),
        ModTarget::Attack => "Attack".into(),
        ModTarget::Release => "Release".into(),
        ModTarget::Module { param, slot } => {
            let mut name: String = param.replace('_', " ");
            if let Some(first) = name.get(..1) {
                name.replace_range(..1, &first.to_uppercase());
            }
            format!("{name} · slot {}", slot + 1)
        }
    }
}

/// The source's value now, 0..=1, for what the editor can read: the wheels,
/// and the newest voice's velocity and key in the watched group.
fn live(p: &SamplerParams, source: &ModSource, watch: u64, group: u16) -> Option<f32> {
    let shared = &p.shared;
    let voice = || {
        (shared.probe.published.load(Relaxed) == watch)
            .then(|| shared.probe.taps().filter(|t| t.group == group).last())
            .flatten()
    };
    match source {
        ModSource::MidiCc(1) => Some(shared.modulation.load(Relaxed) as f32 / 127.),
        ModSource::PitchBend => Some(shared.bend.load(Relaxed) as f32 / 16383.),
        ModSource::Constant => Some(1.),
        ModSource::Velocity => voice().map(|t| f32::from(t.velocity) / 127.),
        ModSource::KeyPosition => voice().map(|t| f32::from(t.note) / 127.),
        _ => None,
    }
}

/// A depth bar: the assignment's reach faint, what its source gives now
/// solid. Read as it is laid out, so it follows the wheels without a rebuild.
fn depth(m: &ModAssignment, p: Arc<SamplerParams>, watch: u64, group: u16) -> El {
    let (m, reach) = (m.clone(), m.intensity.abs().min(1.));
    canvas(move |s| {
        let h = 3.;
        let y = ((s.height - h) / 2.).round();
        let mut d = vec![
            Draw::fill(rect(0., y, s.width, h), Role::Ink.alpha(0.1)),
            Draw::fill(rect(0., y, (s.width * f64::from(reach)).round(), h), Role::Ink.alpha(0.22)),
        ];
        if let Some(v) = live(&p, &m.source, watch, group) {
            let v = if m.invert { 1. - m.shape(v) } else { m.shape(v) };
            d.push(Draw::fill(rect(0., y, (s.width * f64::from(reach * v)).round(), h), value_ink(0.)));
        }
        d
    })
    .w(TEXT * 5.)
    .h(CONTROL - TIGHT)
    .shrink(0)
}

/// One line of a list: `cells` left to right, the last one flexing.
fn line(cells: Vec<El>) -> El {
    row(cells).gap(SPACE).align(Align::Center).h(CONTROL - TIGHT).min_w(0).shrink(0)
}

fn percent(v: f32) -> String {
    format!("{:+.0}%", v * 100.)
}

/// The group's modulation: each external source and internal envelope
/// with what it drives.
pub fn modulation(p: &Arc<SamplerParams>, group: &Group, index: u16, watch: u64) -> El {
    let mut rows = Vec::new();
    let assignment = |m: &ModAssignment, source: String| {
        line(vec![
            caption(source).text_size(TEXT).lines(1).shrink(0).reserve("Release velocity".to_owned()),
            glyph(Icon::Right, TEXT, secondary()),
            caption(target_name(&m.target)).text_size(TEXT).lines(1).min_w(0).reserve("Resonance · slot 8".to_owned()),
            depth(m, p.clone(), watch, index),
            caption(if m.invert { format!("{} inv", percent(m.intensity)) } else { percent(m.intensity) })
                .fill(secondary())
                .lines(1)
                .shrink(0)
                .reserve("-100% inv".to_owned()),
            spacer(),
        ])
        .tip(format!("{} · lag {} ms{}", m.name, m.lag_ms, if m.shaper.is_some() { " · shaped" } else { "" }))
    };
    // An assignment at no depth drives nothing: left out.
    for m in group.mods.iter().filter(|m| m.intensity != 0.) {
        rows.push(assignment(m, source_name(&m.source)));
    }
    for (n, e) in group.envelopes.iter().enumerate() {
        for m in e.targets.iter().filter(|m| m.intensity != 0.) {
            rows.push(assignment(m, format!("Envelope {}", n + 1)));
        }
    }
    if rows.is_empty() {
        rows.push(caption("Nothing modulates this group.").fill(secondary()).lines(2).min_w(0));
    }
    col(rows).gap(0).align(Align::Stretch).min_w(0).shrink(0)
}

/// An effect's key values, as its panel would show them first.
pub fn detail(e: &Effect) -> String {
    match &e.params {
        Params::Gainer(g) => format!("{:+.1} dB", 20. * g.gain.max(1e-6).log10()),
        Params::StereoModeller(m) => format!("Width {:.0}% · Pan {}", (1. + m.spread) * 100., pan_text(f64::from(m.pan))),
        Params::Reverb(r) => format!(
            "{} · time {:.0}% · size {:.0}%",
            if r.room_type >= 0.5 { "Hall" } else { "Room" },
            r.time * 100.,
            r.size * 100.
        ),
        Params::Convolution(c) => c
            .ir_file
            .as_deref()
            .map(super::header::stem)
            .unwrap_or_else(|| "No impulse".into()),
        Params::Filter(f) => {
            let (hz, _) = crate::engine::filter::filter_settings(f.cutoff, f.resonance);
            format!("{} · res {:.0}%", viz::hz_text(hz), f.resonance * 100.)
        }
        Params::Eq(q) => match q.bands.len() {
            1 => "1 band".into(),
            n => format!("{n} bands"),
        },
        Params::SendLevels(s) => match s.sends.iter().filter(|&&v| v > 0.).count() {
            1 => "1 send".into(),
            n => format!("{n} sends"),
        },
        Params::Fields(_) | Params::Opaque { .. } => String::new(),
    }
}

/// Whether KONTRA runs `e` where it sits: a group plays its filters and
/// EQs; the instrument, the effects with DSP here.
fn played(e: &Effect, in_group: bool) -> bool {
    if in_group { matches!(e.params, Params::Filter(_) | Params::Eq(_)) } else { e.is_implemented() }
}

/// A slot's bypass light: filled when it plays, hollow when bypassed.
pub fn bypass_light(on: bool) -> El {
    canvas(move |s| {
        let (w, x, y) = (7., 0., ((s.height - 7.) / 2.).round());
        if on {
            vec![Draw::fill(rect(x, y, w, w), value_ink(0.))]
        } else {
            vec![Draw::stroke(rect(x + 0.5, y + 0.5, w - 1., w - 1.), Role::Ink.alpha(0.45), 1.)]
        }
    })
    .w(7)
    .h(CONTROL - TIGHT)
    .shrink(0)
    .named(if on { "On" } else { "Bypassed" })
}

fn effect_line(e: &Effect, in_group: bool) -> El {
    let played = played(e, in_group);
    let lit = played && !e.bypass;
    let mut cells = vec![
        bypass_light(!e.bypass),
        caption(e.kind.name()).text_size(TEXT).fill(if lit { Fill::from(Role::Ink) } else { secondary() }).lines(1).shrink(0),
        caption(detail(e)).fill(secondary()).lines(1).flex(1).min_w(0),
    ];
    if !played {
        cells.push(caption("Not played").fill(secondary()).lines(1).shrink(0));
    }
    let state = match (e.bypass, played) {
        (true, _) => "bypassed",
        (false, true) => "on",
        (false, false) => "not played by KONTRA",
    };
    line(cells).tip(format!("Slot {} · {} · {state}", e.slot + 1, e.kind.name()))
}

/// The group's inserts, then the instrument's inserts, sends, main chain
/// and buses that hold anything.
pub fn effects(instrument: &Instrument, group: &Group) -> El {
    let mut rows = Vec::new();
    let mut chain = |title: String, c: &Chain, in_group: bool| {
        if c.slots.is_empty() {
            return;
        }
        rows.push(row![section(&title)].pad(edges(TIGHT, 0., 0., 0.)).shrink(0));
        rows.extend(c.slots.iter().map(|e| effect_line(e, in_group)));
    };
    chain("Group inserts".into(), &group.fx, true);
    let fx = &instrument.fx;
    chain("Instrument inserts".into(), &fx.insert, false);
    chain("Sends".into(), &fx.send, false);
    chain("Main".into(), &fx.main, false);
    for b in &fx.buses {
        let name = if b.name.is_empty() { format!("Bus {}", b.index + 1) } else { format!("Bus {} · {}", b.index + 1, b.name) };
        chain(name, &b.chain, false);
    }
    if rows.is_empty() {
        rows.push(caption("No effects.").fill(secondary()).lines(1).min_w(0));
    }
    col(rows).gap(0).align(Align::Stretch).min_w(0).shrink(0)
}

/// The instrument's insert chain in a mixer strip: a light and a name a
/// line, the details on hover.
pub fn inserts(instrument: Option<&Instrument>, rows: usize) -> El {
    let slots = instrument.map_or(&[][..], |i| &i.fx.insert.slots[..]);
    let mut lines: Vec<El> = slots
        .iter()
        .take(rows)
        .map(|e| {
            let lit = played(e, false) && !e.bypass;
            row![
                bypass_light(!e.bypass),
                caption(e.kind.name()).fill(if lit { Fill::from(Role::Ink) } else { secondary() }).lines(1).min_w(0)
            ]
            .gap(TIGHT)
            .align(Align::Center)
            .h(SMALL + TIGHT)
            .min_w(0)
            .shrink(0)
            .tip(match detail(e) {
                d if d.is_empty() => e.kind.name(),
                d => format!("{} · {d}", e.kind.name()),
            })
        })
        .collect();
    if slots.len() > rows {
        lines[rows - 1] = caption(format!("+{} more", slots.len() - rows + 1)).fill(secondary()).lines(1).h(SMALL + TIGHT).shrink(0);
    }
    if lines.is_empty() {
        lines.push(caption("No inserts").fill(secondary()).lines(1).h(SMALL + TIGHT).shrink(0));
    }
    // Every strip keeps the same room, so the faders below line up.
    while lines.len() < rows {
        lines.push(block(Len::Pct(100.), SMALL + TIGHT).shrink(0));
    }
    col(lines).gap(0).align(Align::Stretch).min_w(0).shrink(0)
}

