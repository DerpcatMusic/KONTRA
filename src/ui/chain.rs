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
pub fn modulation(
    p: &Arc<crate::plugin::SamplerParams>,
    slot: usize,
    i: &ir::Instrument,
    g: usize,
) -> El {
    let mut rows = Vec::new();
    let mut used = Vec::new();
    let mut amplitudes = Vec::new();
    let part = p.shared.part(slot);
    for z in i.zones.iter().filter(|z| z.group == Some(ir::GroupRef(g))) {
        if let Some(source) = z.amplitude {
            if !amplitudes.contains(&source) {
                amplitudes.push(source);
            }
        }
        for r in &z.routes {
            if !used.contains(r) {
                used.push(*r);
            }
        }
    }
    // Port from v1 0cb7a8a0:src/ui/chain.rs: include primary amplitude assignments.
    let routes = used
        .into_iter()
        .filter_map(|id| i.routes.get(id.0))
        .copied()
        .chain(amplitudes.into_iter().map(|source| {
            ir::Route::new(source, ir::Target::Amplitude, ir::Depth::Normalized(1.))
        }));
    for r in routes {
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
        let part = part.clone();
        let reach = depth.abs().min(1.);
        let bar = canvas(move |s| {
            let y = ((s.height - 3.) / 2.).round();
            let mut d = vec![
                Draw::fill(rect(0., y, s.width, 3.), Role::Ink.alpha(0.1)),
                Draw::fill(rect(0., y, s.width * reach, 3.), Role::Ink.alpha(0.22)),
            ];
            // Port from v1's live source readout, over v2's bounded per-part taps.
            let voice = || {
                if state.shared.editor_watch.load(Relaxed) != slot {
                    return None;
                }
                part.as_ref()?
                    .editor_taps
                    .iter()
                    .filter_map(|v| crate::sound::edits::Tap::unpack(v.load(Relaxed)))
                    .filter(|t| usize::from(t.group) == g)
                    .last()
            };
            let v = match source {
                ir::ModulationSource::Controller(1) => {
                    Some(state.shared.modulation.load(Relaxed) as f64 / 127.)
                }
                ir::ModulationSource::PitchBend => {
                    Some(state.shared.bend.load(Relaxed) as f64 / 16383.)
                }
                ir::ModulationSource::Constant => Some(1.),
                ir::ModulationSource::Velocity => voice().map(|t| f64::from(t.velocity) / 127.),
                ir::ModulationSource::Key => voice().map(|t| f64::from(t.note) / 127.),
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
        ir::Target::SourcePhase(_) => "LFO start phase".into(),
        ir::Target::SourceRate(_) => "LFO rate".into(),
        ir::Target::LoopStart => "Loop start".into(),
        ir::Target::LoopLength => "Loop length".into(),
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
                ir::ProcessorParameter::Frequency => "Frequency",
                ir::ProcessorParameter::Quality => "Q",
                ir::ProcessorParameter::GainDecibels => "Gain",
                ir::ProcessorParameter::Bandwidth => "Bandwidth",
                ir::ProcessorParameter::ReferenceDecibels => "Reference gain",
                ir::ProcessorParameter::Enabled => "Enabled",
                ir::ProcessorParameter::CellLL => "Left to left",
                ir::ProcessorParameter::CellLR => "Left to right",
                ir::ProcessorParameter::CellRL => "Right to left",
                ir::ProcessorParameter::CellRR => "Right to right",
                ir::ProcessorParameter::Shape => "Shape",
                ir::ProcessorParameter::Drive => "Drive",
                ir::ProcessorParameter::Damping => "Damping",
                ir::ProcessorParameter::FormantTalk => "Talk",
                ir::ProcessorParameter::FormantSharp => "Sharp",
                ir::ProcessorParameter::FormantSize => "Size",
                ir::ProcessorParameter::LoFiBits => "Bits",
                ir::ProcessorParameter::LoFiFrequency => "Sample rate",
                ir::ProcessorParameter::LoFiNoise => "Noise",
                ir::ProcessorParameter::LoFiColor => "Color",
            },
            index + 1
        ),
    }
}
fn color_detail(c: &ir::fx::color::Config) -> String {
    use ir::fx::color::{Curve, Hold, Quantizer};
    let db = |g: ir::Gain| format!("{:+.1} dB", 20. * g.linear().max(1e-10).log10());
    if let Some(g) = c.gainer {
        return format!("Gainer · {} · dry {:.0}%", db(g.gain), g.dry * 100.);
    }
    if let Some(s) = c.stereo {
        return format!(
            "Stereo modeller · width {:.0}% · pan {}{}",
            s.width * 200.,
            pan_text(s.pan),
            if s.pseudo { " · pseudo stereo" } else { "" }
        );
    }
    match c.curve {
        Curve::Rectifier(ir::Rectifier::Full) => return "Rectifier · full wave".into(),
        Curve::Rectifier(ir::Rectifier::Half) => return "Rectifier · half wave".into(),
        Curve::Classic { shape } => return format!("Saturation · shape {shape:+.2}"),
        Curve::Enhanced { shape } => return format!("Saturation enhanced · shape {shape:+.2}"),
        Curve::Drums { shape } => return format!("Saturation drums · shape {shape:+.2}"),
        Curve::Tube { drive } | Curve::Transistor { drive } => {
            return format!(
                "{} distortion · drive {:.0}% · damping {:.0}%",
                if matches!(c.curve, Curve::Transistor { .. }) { "Transistor" } else { "Tube" },
                drive * 100.,
                c.damping.unwrap_or(0.) * 100.
            );
        }
        _ => {}
    }
    match (c.quantizer, c.hold, c.noise) {
        (None, None, None) => c.label().into(),
        (q, h, n) => format!(
            "LoFi · bits {} · rate {} · noise {:.2}",
            match q { Some(Quantizer::Native { bits }) => format!("{bits:.2}"), _ => "off".into() },
            match h { Some(Hold::Native { frequency }) => format!("{frequency:.2}"), _ => "off".into() },
            n.map_or(0., |n| n.level)
        ),
    }
}
fn detail(p: ir::Processor) -> String {
    let db = |g: ir::Gain| format!("{:+.1} dB", 20. * g.linear().max(1e-10).log10());
    match p {
        ir::Processor::Gain(g) => format!("Gain · {}", db(g)),
        ir::Processor::Color(c) => color_detail(&c),
        ir::Processor::Pan(p) => format!("Pan · {}", pan_text(p.position)),
        ir::Processor::LiveFormant { knobs: [talk, sharp, size], .. } => {
            format!("Formant · talk {talk:.2} · sharp {sharp:.2} · size {size:.2}")
        }
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
                ir::FilterKind::FirstOrderLowShelf { .. } => "Low shelf (6 dB/oct)",
                ir::FilterKind::FirstOrderHighShelf { .. } => "High shelf (6 dB/oct)",
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
        ir::Processor::Space(c) => match c.network {
            Some(n) => format!(
                "{} · {:.2} s · size {:.0}% · predelay {:.0} ms",
                match n.topology {
                    ir::fx::space::Topology::Fdn => "Reverb",
                    ir::fx::space::Topology::CombAllpass => "Room reverb",
                    ir::fx::space::Topology::Plate => "Plate reverb",
                    ir::fx::space::Topology::Diffuser => "Diffusion",
                    ir::fx::space::Topology::Freeverb => "Reverb (legacy)",
                    ir::fx::space::Topology::Galois => "Reverb",
                    ir::fx::space::Topology::SparkVerb => "SparkVerb",
                    ir::fx::space::Topology::Jezar => "Reverb",
                    ir::fx::space::Topology::KontaktPlate => "Plate reverb",
                    ir::fx::space::Topology::Raum => "Raum",
                },
                n.decay_seconds,
                n.size * 100.,
                n.predelay_seconds * 1000.
            ),
            None => format!(
                "Convolution · dry {:.0}% · wet {:.0}%",
                c.dry * 100.,
                c.wet * 100.
            ),
        },
        ir::Processor::Dynamics(c) => match c.law {
            ir::fx::dynamics::Law::Generic(g) => format!(
                "Dynamics (generic) · {:.1} dB · {:.1}:1 · attack {:.1} ms · release {:.1} ms",
                g.threshold_db,
                g.ratio,
                c.attack.seconds() * 1000.,
                c.release.seconds() * 1000.
            ),
            ir::fx::dynamics::Law::KontaktClassic(_) => "Compressor (Classic)".into(),
            ir::fx::dynamics::Law::KontaktEnhanced(_) => "Compressor (Enhanced)".into(),
            ir::fx::dynamics::Law::KontaktPro(_) => "Compressor (Pro)".into(),
            ir::fx::dynamics::Law::KontaktLimiter(_) => "Limiter".into(),
            ir::fx::dynamics::Law::KontaktHilbert(_) => "Transparent Limiter".into(),
            ir::fx::dynamics::Law::KontaktFeedback { .. } => "Feedback Compressor".into(),
            ir::fx::dynamics::Law::KontaktTransient { .. } => "Transient Master".into(),
            ir::fx::dynamics::Law::KontaktBusComp { .. } => "Solid Bus Comp".into(),
            ir::fx::dynamics::Law::UviMaximizer(_) => "Maximizer".into(),
        },
        ir::Processor::Daft(d) => format!(
            "Daft {} · cutoff {:.0}% · resonance {:.0}%",
            if d.highpass { "high-pass" } else { "low-pass" },
            d.cutoff * 100.,
            d.resonance * 100.
        ),
        ir::Processor::LadderLP4(d) => ["Ladder LP1", "Ladder LP2", "Ladder LP3", "Ladder LP4", "Ladder HP1", "Ladder HP2", "Ladder HP3", "Ladder HP4", "Ladder BP2", "Ladder BP4", "Ladder Peak", "Ladder Notch"][usize::from(d.taps.clamp(1, 12)) - 1].into(),
        ir::Processor::TimeMod(_) => "Delay / modulation".into(),
        ir::Processor::Phaser(_) => "Phaser filter".into(),
        ir::Processor::Eq(_) => "EQ".into(),
        ir::Processor::Formant(_) => "Formant I".into(),
        ir::Processor::FilterBank(c) => match c.topology {
            ir::fx::filter::Topology::FormantII => format!(
                "Formant II · talk {:.2} · sharp {:.2} · size {:.2}",
                c.cutoff, c.resonance, c.extra
            ),
            _ => "Filter".into(),
        },
        ir::Processor::Ar(d) => format!(
            "Adaptive-resonance {}-pole {} · cutoff {:.0}% · resonance {:.0}%",
            ["2", "4", "2/4"][usize::from(d.mode / 3)],
            ["low-pass", "band-pass", "high-pass"][usize::from(d.mode % 3)],
            d.cutoff * 100.,
            d.resonance * 100.
        ),
        ir::Processor::SendReturnGate { .. } => "Send return · follows effect bypass".into(),
        ir::Processor::StereoMatrix(_) => "Stereo routing".into(),
        ir::Processor::Branch { gain, .. } => format!("Parallel branch · {}", db(gain)),
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
