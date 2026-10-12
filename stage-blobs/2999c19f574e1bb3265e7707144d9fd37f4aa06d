//! Port from v1 0cb7a8a0:src/ui/chain.rs. Read-only modulation and effect
//! lists adapted to the v2 source IR; native routing and bypass stay explicit.
use super::theme::*;
use moose::mui::mui::prelude::*;
use sampler_ir as ir;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
fn line(cells: Vec<El>) -> El {
    row(cells)
        .gap(SPACE)
        .align(Align::Center)
        .h(CONTROL - TIGHT)
        .min_w(0)
        .shrink(0)
}
pub fn bypass_light(on: bool) -> El {
    canvas(move |s| {
        let (w, x, y) = (7., 0., ((s.height - 7.) / 2.).round());
        if on {
            vec![Draw::fill(rect(x, y, w, w), value_ink(0.))]
        } else {
            vec![Draw::stroke(
                rect(x + 0.5, y + 0.5, w - 1., w - 1.),
                Role::Ink.alpha(0.45),
                1.,
            )]
        }
    })
    .w(7)
    .h(CONTROL - TIGHT)
    .shrink(0)
    .named(if on { "On" } else { "Bypassed" })
}
pub fn modulation(p: &Arc<crate::plugin::SamplerParams>, i: &ir::Instrument, g: usize) -> El {
    let mut rows = Vec::new();
    let mut used = Vec::new();
    for z in i.zones.iter().filter(|z| z.group == Some(ir::GroupRef(g))) {
        for r in &z.routes {
            if !used.contains(r) {
                used.push(*r);
            }
        }
    }
    for id in used {
        let Some(r) = i.routes.get(id.0) else {
            continue;
        };
        let Some(source) = i.modulators.get(r.source.0) else {
            continue;
        };
        let depth = match r.depth {
            ir::Depth::Normalized(v) => v,
            ir::Depth::Gain(v) => v.linear(),
            ir::Depth::Pitch(v) => v.semitones(),
        };
        let source = source.source.clone();
        let state = p.clone();
        let reach = depth.abs().min(1.);
        let bar = canvas(move |s| {
            let y = ((s.height - 3.) / 2.).round();
            let mut d = vec![
                Draw::fill(rect(0., y, s.width, 3.), Role::Ink.alpha(0.1)),
                Draw::fill(rect(0., y, s.width * reach, 3.), Role::Ink.alpha(0.22)),
            ];
            let v = match source {
                ir::ModulationSource::Controller(1) => {
                    Some(state.shared.modulation.load(Relaxed) as f64 / 127.)
                }
                ir::ModulationSource::PitchBend => {
                    Some(state.shared.bend.load(Relaxed) as f64 / 16383.)
                }
                ir::ModulationSource::Constant => Some(1.),
                _ => None,
            };
            if let Some(v) = v {
                d.push(Draw::fill(
                    rect(0., y, s.width * reach * v, 3.),
                    value_ink(0.),
                ));
            }
            d
        })
        .w(TEXT * 5.)
        .h(CONTROL - TIGHT)
        .shrink(0);
        rows.push(
            line(vec![
                caption(super::inside::source_name(&i.modulators[r.source.0].source))
                    .text_size(TEXT)
                    .lines(1)
                    .shrink(0),
                glyph(Icon::Right, TEXT, secondary()),
                caption(target_name(i, r.target))
                    .text_size(TEXT)
                    .lines(1)
                    .min_w(0),
                bar,
                caption(format!(
                    "{:+.0}%{}",
                    depth * 100.,
                    if r.invert { " inv" } else { "" }
                ))
                .fill(secondary())
                .lines(1)
                .shrink(0),
                spacer(),
            ])
            .tip(format!(
                "Lag {:.1} ms{}",
                r.smoothing.seconds() * 1000.,
                if r.shape.is_some() { " · shaped" } else { "" }
            )),
        );
    }
    if rows.is_empty() {
        rows.push(
            caption("Nothing modulates this group.")
                .fill(secondary())
                .lines(2)
                .min_w(0),
        );
    }
    col(rows).gap(0).align(Align::Stretch).min_w(0).shrink(0)
}
/// Native Kontakt group inserts live on shared zone voice chains.
pub fn group_chains(i: &ir::Instrument, g: usize) -> Vec<ir::ChainRef> {
    let mut chains = Vec::new();
    for reference in i
        .zones
        .iter()
        .filter(|z| z.group == Some(ir::GroupRef(g)))
        .filter_map(|z| z.chain)
        .chain(i.groups.get(g).and_then(|group| group.chain))
    {
        if !chains.contains(&reference) {
            chains.push(reference);
        }
    }
    chains
}

pub fn effects(i: &ir::Instrument, g: usize) -> El {
    let mut rows = Vec::new();
    let mut chain = |title: String, references: &[ir::ChainRef]| {
        let mut processors = references
            .iter()
            .filter_map(|r| i.chains.get(r.0))
            .flat_map(|c| c.pre_amplitude.iter().chain(&c.post_amplitude))
            .peekable();
        if processors.peek().is_none() {
            return;
        }
        rows.push(
            row![section(&title)]
                .pad(edges(TIGHT, 0., 0., 0.))
                .shrink(0),
        );
        for p in processors {
            let on = !matches!(p, ir::Processor::Mix { bypass: true, .. });
            let detail = detail(*p);
            rows.push(
                line(vec![
                    bypass_light(on),
                    caption(detail.clone())
                        .fill(if on { Role::Ink.into() } else { secondary() })
                        .text_size(TEXT)
                        .lines(1)
                        .min_w(0),
                ])
                .tip(detail),
            );
        }
    };
    // port from v1 0cb7a8a0:src/ui/chain.rs: inventory every group insert once.
    let group = group_chains(i, g);
    chain("Group inserts".into(), &group);
    for (n, c) in i.chains.iter().enumerate() {
        if group.contains(&ir::ChainRef(n)) {
            continue;
        }
        match c.scope {
            ir::Scope::Master => chain("Instrument inserts".into(), &[ir::ChainRef(n)]),
            ir::Scope::Bus(b) => chain(format!("Bus {}", b.0 + 1), &[ir::ChainRef(n)]),
            _ => {}
        }
    }
    if rows.is_empty() {
        rows.push(caption("No effects.").fill(secondary()).lines(1).min_w(0));
    }
    col(rows).gap(0).align(Align::Stretch).min_w(0).shrink(0)
}

fn target_name(i: &ir::Instrument, t: ir::Target) -> String {
    match t {
        ir::Target::Amplitude => "Volume".into(),
        ir::Target::Pitch => "Pitch".into(),
        ir::Target::Pan => "Pan".into(),
        ir::Target::SampleStart => "Sample start".into(),
        ir::Target::Control(c) => i
            .controls
            .get(c.0)
            .map_or("Control".into(), |c| c.label.clone()),
        ir::Target::Processor {
            index, parameter, ..
        } => format!(
            "{} · stage {}",
            match parameter {
                ir::ProcessorParameter::Response => "Response",
                ir::ProcessorParameter::Width => "Width",
                ir::ProcessorParameter::Pan => "Pan",
                ir::ProcessorParameter::Cutoff => "Cutoff",
                ir::ProcessorParameter::Resonance => "Resonance",
                ir::ProcessorParameter::Gain => "Gain",
                ir::ProcessorParameter::Threshold => "Threshold",
                ir::ProcessorParameter::Ratio => "Ratio",
                ir::ProcessorParameter::Attack => "Attack",
                ir::ProcessorParameter::Release => "Release",
            },
            index + 1
        ),
    }
}
fn detail(p: ir::Processor) -> String {
    let db = |g: ir::Gain| format!("{:+.1} dB", 20. * g.linear().max(1e-10).log10());
    match p {
        ir::Processor::Gain(g) => format!("Gain · {}", db(g)),
        ir::Processor::Gainer { gain, dry } => {
            format!("Gainer · {} · dry {:.0}%", db(gain), dry * 100.)
        }
        ir::Processor::LoFi {
            bits,
            frequency,
            noise,
            color,
        } => format!(
            "LoFi · {bits:.1} bits · {frequency:.0} Hz · noise {noise:.2} · color {color:.2}"
        ),
        ir::Processor::Pan(p) => format!("Pan · {}", pan_text(p.position)),
        ir::Processor::StereoModeller { width, pan, pseudo } => format!(
            "Stereo modeller · width {:.0}% · pan {}{}",
            width * 200.,
            pan_text(pan),
            if pseudo { " · pseudo stereo" } else { "" }
        ),
        ir::Processor::Filter(f) => format!(
            "{} · {}",
            match f.kind {
                ir::FilterKind::LowPass { .. } => "Low-pass",
                ir::FilterKind::HighPass { .. } => "High-pass",
                ir::FilterKind::BandPass { .. } => "Band-pass",
                ir::FilterKind::Notch { .. } => "Notch",
                ir::FilterKind::AllPass => "All-pass",
                ir::FilterKind::Peak { .. } => "EQ peak",
                ir::FilterKind::LowShelf { .. } => "Low shelf",
                ir::FilterKind::HighShelf { .. } => "High shelf",
            },
            match f.cutoff {
                ir::Frequency::Hertz(h) => super::viz::hz_text(h as f32),
                ir::Frequency::Beats(b) => format!("{b} beats"),
            }
        ),
        ir::Processor::Delay {
            time,
            feedback,
            mix,
        } => format!(
            "Delay · {:.0} ms · feedback {:.0}% · mix {:.0}%",
            time.seconds() * 1000.,
            feedback * 100.,
            mix * 100.
        ),
        ir::Processor::Reverb(r) => format!(
            "Reverb · {:.2} s · size {:.0}% · predelay {:.0} ms",
            r.decay_seconds,
            r.size * 100.,
            r.predelay_seconds * 1000.
        ),
        ir::Processor::Compressor(c) => format!(
            "Compressor · {:.1} dB · {:.1}:1 · attack {:.1} ms · release {:.1} ms",
            c.threshold_db,
            c.ratio,
            c.attack.seconds() * 1000.,
            c.release.seconds() * 1000.
        ),
        ir::Processor::Rectify(ir::Rectifier::Full) => "Rectifier · full wave".into(),
        ir::Processor::Rectify(ir::Rectifier::Half) => "Rectifier · half wave".into(),
        ir::Processor::Daft(d) => format!(
            "Daft {} · cutoff {:.0}% · resonance {:.0}%",
            if d.highpass { "high-pass" } else { "low-pass" },
            d.cutoff * 100.,
            d.resonance * 100.
        ),
        ir::Processor::LadderLP4(_) => "Ladder low-pass · 4 poles".into(),
        ir::Processor::SendReturnGate { .. } => "Send return · follows effect bypass".into(),
        ir::Processor::StereoMatrix(_) => "Stereo routing".into(),
        ir::Processor::Branch { gain, .. } => format!("Parallel branch · {}", db(gain)),
        ir::Processor::Convolution { dry, wet, .. } => format!(
            "Convolution · dry {:.0}% · wet {:.0}%",
            dry * 100.,
            wet * 100.
        ),
        ir::Processor::Mix {
            dry, wet, bypass, ..
        } => format!(
            "Effect mix · dry {:.0}% · wet {:.0}%{}",
            dry * 100.,
            wet * 100.,
            if bypass { " · bypassed" } else { "" }
        ),
    }
}
