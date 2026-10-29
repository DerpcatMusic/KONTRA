//! EQ bands and filters as stages on summed audio.

use kontra_dsp::eq::{Bell, Biquad, Side};
use kontra_dsp::filter::{Formant, LadderKind, Pass, Shape, Svf, VowelSet, Vowels};
use kontra_ir as ir;

/// The rack's active EQs (their audible bands) and filters, in slot order.
pub fn sections(rack: &[Option<ir::Effect>]) -> Vec<ir::EffectKind> {
    rack.iter()
        .flatten()
        .filter(|e| !e.bypass)
        .filter_map(|e| match e.kind {
            ir::EffectKind::Eq(bands) => bands.iter().any(|b| b.gain_db != 0.0).then_some(e.kind),
            kind @ (ir::EffectKind::Filter(_) | ir::EffectKind::Formant(_)) => Some(kind),
            _ => None,
        })
        .collect()
}

/// One EQ band as its biquad at `rate`.
fn band(b: &ir::Bell, rate: u32) -> Biquad {
    match b.shape {
        ir::Shape::Peak => {
            let band = Bell {
                hz: b.hz,
                q: b.q,
                gain_db: b.gain_db,
            };
            kontra_dsp::eq::bell(band, rate)
        }
        ir::Shape::LowShelf => kontra_dsp::eq::shelf(b.hz, b.gain_db, b.q, Side::Low, rate),
        ir::Shape::HighShelf => kontra_dsp::eq::shelf(b.hz, b.gain_db, b.q, Side::High, rate),
    }
}

/// A group's EQ run on one voice: its four bands, moved by their routes.
#[derive(Clone)]
pub struct VoiceEq {
    biquads: [Biquad; 4],
}

impl VoiceEq {
    pub fn new(bells: [ir::Bell; 4], rate: u32) -> Self {
        Self {
            biquads: bells.map(|b| band(&b, rate)),
        }
    }

    /// Moves the bands' coefficients and keeps their state.
    pub fn retune(&mut self, bells: [ir::Bell; 4], rate: u32) {
        for (f, b) in self.biquads.iter_mut().zip(&bells) {
            f.retune(&band(b, rate));
        }
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        for f in &mut self.biquads {
            f.process(io);
        }
    }
}

/// A section's runner: biquads in series, a formant's parallel bands, or a
/// ladder (not a set of biquads).
pub enum Stage {
    Biquad(Biquad),
    // Boxed: its glide state is six times a biquad; built at load.
    Formant(Box<Formant>),
    Running(Box<Svf>),
}

impl Stage {
    pub fn is_silent(&self) -> bool {
        match self {
            Self::Biquad(b) => b.is_silent(),
            Self::Formant(f) => f.is_silent(),
            Self::Running(f) => f.is_silent(),
        }
    }
}

pub fn stages(sections: &[ir::EffectKind], rate: u32) -> Box<[Stage]> {
    let mut out = Vec::new();
    for section in sections {
        match section {
            ir::EffectKind::Eq(bands) => {
                out.extend(
                    bands
                        .iter()
                        .filter(|b| b.gain_db != 0.0)
                        .map(|b| Stage::Biquad(band(b, rate))),
                );
            }
            ir::EffectKind::Filter(f) => {
                let shape = shape(f);
                if shape.ladder.is_some() {
                    let mut svf = Svf::new(shape);
                    svf.set_cutoff(shape.hz, rate);
                    out.push(Stage::Running(Box::new(svf)));
                    continue;
                }
                out.extend(
                    kontra_dsp::filter::sections(&shape, rate)
                        .into_iter()
                        .map(Stage::Biquad),
                );
            }
            ir::EffectKind::Formant(f) => {
                let v = Vowels {
                    talk: f.talk,
                    sharp: f.sharp,
                    size: f.size,
                    set: match f.set {
                        ir::VowelSet::First => VowelSet::First,
                        ir::VowelSet::Second => VowelSet::Second,
                    },
                };
                out.push(Stage::Formant(Box::new(Formant::new(v, rate))));
            }
            _ => {}
        }
    }
    out.into()
}

pub fn shape(f: &ir::Filter) -> Shape {
    let pass = match f.pass {
        ir::Pass::Low => Pass::Low,
        ir::Pass::High => Pass::High,
        ir::Pass::Band => Pass::Band,
        ir::Pass::Notch => Pass::Notch,
    };
    Shape {
        pass,
        hz: f.hz,
        stages: f.stages,
        ladder: f.ladder.map(|l| match l {
            ir::Ladder::ZeroDelay(taps) => LadderKind::ZeroDelay(taps),
            ir::Ladder::Explicit { taps, fat } => LadderKind::Explicit { taps, fat },
            ir::Ladder::SallenKey { morph, curve } => LadderKind::SallenKey { morph, curve },
        }),
        damping: f.damping,
        gain: f.gain,
    }
}

pub fn run(stages: &mut [Stage], io: &mut [[f32; 2]]) {
    for stage in stages {
        match stage {
            Stage::Biquad(b) => b.process(io),
            Stage::Formant(f) => f.process(io),
            Stage::Running(f) => f.process(io),
        }
    }
}
