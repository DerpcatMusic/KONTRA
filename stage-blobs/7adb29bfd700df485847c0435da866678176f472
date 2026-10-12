//! Adapter for the directly ported v1 editor. All values use admitted native
//! bindings and their laws; this contains pictures, never a playback bank.
use crate::sound::edits::{Edits, Param};
use sampler_core::{EngineParameterBinding as Binding, EnvelopeCurve};
use sampler_ir as ir;
#[derive(Clone, PartialEq)]
pub struct Ahdsr {
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub attack_shape: ir::Curve,
    pub decay_shape: ir::Curve,
    pub release_shape: ir::Curve,
    pub one_shot: bool,
}
impl Ahdsr {
    pub fn trace(&self, points: usize) -> [Vec<f32>; 3] {
        let curve = |c: ir::Curve| match c {
            ir::Curve::Linear => EnvelopeCurve::default(),
            ir::Curve::Exponential(k) => EnvelopeCurve::exponential(k).unwrap_or_default(),
            ir::Curve::Step => EnvelopeCurve::step(),
        };
        let trace = |time: f32, c: ir::Curve, from: f32, to: f32| {
            if time == 0. {
                vec![to]
            } else {
                (0..=points)
                    .map(|n| from + (to - from) * curve(c).value(n as f64 / points as f64) as f32)
                    .collect()
            }
        };
        [
            trace(self.attack, self.attack_shape, 0., 1.),
            trace(
                self.decay,
                self.decay_shape,
                1.,
                if self.one_shot { 0. } else { self.sustain },
            ),
            trace(self.release, self.release_shape, self.sustain, 0.),
        ]
    }
}
#[derive(Clone, PartialEq)]
pub struct GroupSettings {
    pub envelope: Option<Ahdsr>,
    pub values: Vec<(Param, f32)>,
    pub bindings: Vec<(Param, Binding)>,
    filters: Vec<ir::Filter>,
    ladders: Vec<ir::LadderLP4>,
    chains: Vec<ir::ChainRef>,
    rate: u32,
}
impl Param {
    pub fn read(self, s: &GroupSettings) -> Option<f32> {
        s.values.iter().find(|(p, _)| *p == self).map(|(_, v)| *v)
    }
}
impl GroupSettings {
    fn ladder(&self, p: Param) -> Option<&ir::LadderLP4> {
        let binding = self.bindings.iter().find(|(q, _)| *q == p)?.1;
        self.ladders.iter().find(|f| f.address.is_some_and(|a| {
            a.group == binding.address.group && a.slot == binding.address.slot && a.generic == binding.address.generic
        }))
    }

    pub fn frequency(&self, p: Param, n: f32) -> Option<f32> {
        let binding = self.bindings.iter().find(|(q, _)| *q == p)?.1;
        let native = binding.law.decode((n * 1e6).round() as i32) as f32;
        Some(if matches!(p, Param::Cutoff(_)) && self.ladder(p).is_some() {
            sampler_core::LadderSettings::cutoff_hz(native)
        } else { native })
    }

    pub fn gain_db(&self, p: Param, n: f32) -> Option<f32> {
        let binding = self.bindings.iter().find(|(q, _)| *q == p)?.1;
        let native = binding.law.decode((n * 1e6).round() as i32) as f32;
        Some(20. * native.max(1e-10).log10())
    }

    pub fn magnitude(&self, hz: f32) -> f32 {
        self.filters
            .iter()
            .map(|f| {
                let ir::Frequency::Hertz(cutoff) = f.cutoff else {
                    return 1.;
                };
                let q = match f.resonance {
                    ir::Resonance::Q(q) => q,
                    ir::Resonance::Decibels(db) => {
                        std::f64::consts::FRAC_1_SQRT_2 * 10f64.powf(db / 20.)
                    }
                    ir::Resonance::Normalized(_) => return 1.,
                };
                let cutoff = cutoff.min(self.rate as f64 * 0.49);
                use ir::FilterKind as I;
                use sampler_core::FilterKind as C;
                if matches!(f.kind, I::LowPass { poles: 1 } | I::HighPass { poles: 1 }) {
                    let b = -(-std::f64::consts::TAU * cutoff / self.rate as f64).exp_m1();
                    let a = 1. - b;
                    let w = std::f64::consts::TAU * hz as f64 / self.rate as f64;
                    let den = (1. + a * a - 2. * a * w.cos()).sqrt();
                    return if matches!(f.kind, I::LowPass { .. }) {
                        b / den
                    } else {
                        a * (2. - 2. * w.cos()).sqrt() / den
                    } as f32;
                }
                let (kind, poles) = match f.kind {
                    I::LowPass { poles } => (C::LowPass, poles),
                    I::HighPass { poles } => (C::HighPass, poles),
                    I::BandPass { poles } => (C::BandPass, poles),
                    I::Notch { poles } => (C::Notch, poles),
                    I::AllPass => (C::AllPass, 2),
                    I::Peak { gain } => (
                        C::Peak {
                            gain_db: 20. * gain.linear().log10(),
                        },
                        2,
                    ),
                    I::LowShelf { gain } => (
                        C::LowShelf {
                            gain_db: 20. * gain.linear().log10(),
                        },
                        2,
                    ),
                    I::HighShelf { gain } => (
                        C::HighShelf {
                            gain_db: 20. * gain.linear().log10(),
                        },
                        2,
                    ),
                };
                let m = sampler_core::Biquad::new(self.rate, kind, cutoff, q)
                    .map_or(1., |b| b.magnitude(hz as f64));
                (if poles == 4 { m * m } else { m }) as f32
            })
            .product::<f32>()
            * self.ladders.iter().map(|f| sampler_core::LadderSettings::magnitude(
                [f.cutoff as f32, f.resonance as f32, f.gain as f32], hz, self.rate)).product::<f32>()
    }
}
#[derive(Clone, PartialEq)]
pub struct Model {
    pub playing: GroupSettings,
    pub base: GroupSettings,
    pub params: Vec<(usize, Param)>,
    pub group: u16,
    pub runtime_group: u32,
}
impl Model {
    pub fn new(
        i: &ir::Instrument,
        g: usize,
        edits: &Edits,
        bindings: &[Binding],
        values: &[(sampler_ui_ir::ControlId, f64)],
        rate: f64,
    ) -> Self {
        let physical = i
            .source_indices
            .groups
            .iter()
            .position(|r| *r == Some(ir::GroupRef(g)))
            .unwrap_or(g);
        let group = u16::try_from(physical).unwrap_or(u16::MAX);
        let rate = if rate > 0. { rate as u32 } else { 48_000 };
        let envelope = i
            .zones
            .iter()
            .filter(|z| z.group == Some(ir::GroupRef(g)))
            .find_map(|z| z.amplitude.and_then(|m| i.modulators.get(m.0)))
            .and_then(|m| match m.source {
                ir::ModulationSource::Envelope(e) => Some(Ahdsr {
                    attack: e.attack.seconds() as f32,
                    hold: e.hold.seconds() as f32,
                    decay: e.decay.seconds() as f32,
                    sustain: e.sustain as f32,
                    release: e.release.seconds() as f32,
                    attack_shape: e.attack_shape,
                    decay_shape: e.decay_shape,
                    release_shape: e.release_shape,
                    one_shot: e.one_shot,
                }),
                _ => None,
            });
        let admitted = bindings;
        let bindings: Vec<_> = bindings
            .iter()
            .copied()
            .filter(|b| b.address.group == physical as i32)
            .filter_map(|b| Param::of(b).map(|p| (p, b)))
            .collect();
        let vals = bindings
            .iter()
            .filter_map(|&(p, b)| {
                values
                    .iter()
                    .find(|(id, _)| id.0 == b.control.0)
                    .map(|(_, v)| (p, b.law.encode(*v) as f32 / 1e6))
            })
            .collect();
        let chains = super::chain::group_chains(i, g);
        let processors = || chains.iter().filter_map(|r| i.chains.get(r.0))
            .flat_map(|chain| chain.pre_amplitude.iter().chain(&chain.post_amplitude));
        let filters = processors().filter_map(|p| match p {
            ir::Processor::Filter(f) => Some(*f), _ => None,
        }).collect();
        let ladders = processors()
            .filter_map(|p| if let ir::Processor::LadderLP4(f) = p { Some(*f) } else { None })
            .map(|mut f| {
                if let Some(a) = f.address {
                    if let Some(binding) = admitted.iter().find(|b| b.address.group == a.group
                        && b.address.slot == a.slot && b.address.generic == a.generic
                        && Some(b.address.parameter) == sampler_core::engine_parameter_id("ENGINE_PAR_GAIN"))
                    {
                        if let Some((_, gain)) = values.iter().find(|(id, _)| id.0 == binding.control.0) { f.gain = *gain; }
                    }
                }
                f
            }).collect();
        let mut base = GroupSettings {
            envelope,
            values: vals,
            bindings,
            filters,
            ladders,
            chains,
            rate,
        };
        let mut playing = base.clone();
        for (p, v) in &mut playing.values {
            *v = (*v + edits.offset(group, *p)).clamp(0., 1.);
        }
        for (s, edited) in [(&mut base, false), (&mut playing, true)] {
            for &(p, b) in &s.bindings {
                let Some(n) = p.read(s) else { continue };
                let native = if !edited || edits.offset(group, p) == 0. {
                    values
                        .iter()
                        .find(|(id, _)| id.0 == b.control.0)
                        .map(|(_, v)| *v)
                        .unwrap_or_else(|| b.law.decode((n * 1e6).round() as i32))
                } else {
                    b.law.decode((n * 1e6).round() as i32)
                };
                if let Some(env) = s.envelope.as_mut() {
                    match p {
                        Param::Attack => env.attack = native as f32 / rate as f32,
                        Param::Hold => env.hold = native as f32 / rate as f32,
                        Param::Decay => env.decay = native as f32 / rate as f32,
                        Param::Sustain => env.sustain = native as f32,
                        Param::Release => env.release = native as f32 / rate as f32,
                        Param::Curve => {
                            env.attack_shape = if native == 0. {
                                ir::Curve::Linear
                            } else {
                                ir::Curve::Exponential(native)
                            }
                        }
                        _ => {}
                    }
                }
                for ladder in &mut s.ladders {
                    if !ladder.address.is_some_and(|a| a.group == b.address.group
                        && a.slot == b.address.slot && a.generic == b.address.generic) { continue; }
                    match p {
                        Param::Cutoff(_) => ladder.cutoff = native,
                        Param::Resonance(_) => ladder.resonance = native,
                        _ => {}
                    }
                }
                for target in i.processor_controls.iter().filter(|t| {
                    i.controls
                        .get(t.control.0)
                        .is_some_and(|c| sampler_core::lower::ir_control_id(&c.key) == b.control)
                }) {
                    // Keep each native control on its own voice/group chain.
                    if !s.chains.contains(&target.chain) {
                        continue;
                    }
                    let Some(chain) = i.chains.get(target.chain.0) else {
                        continue;
                    };
                    let preceding = s.chains.iter().take_while(|r| **r != target.chain)
                        .filter_map(|r| i.chains.get(r.0))
                        .flat_map(|c| c.pre_amplitude.iter().chain(&c.post_amplitude))
                        .filter(|p| matches!(p, ir::Processor::Filter(_))).count();
                    let index = preceding + chain
                        .pre_amplitude
                        .iter()
                        .chain(&chain.post_amplitude)
                        .take(target.index)
                        .filter(|p| matches!(p, ir::Processor::Filter(_)))
                        .count();
                    if let Some(f) = s.filters.get_mut(index) {
                        match target.parameter {
                            ir::ProcessorParameter::Cutoff => {
                                f.cutoff = ir::Frequency::Hertz(native)
                            }
                            ir::ProcessorParameter::Resonance => {
                                f.resonance = ir::Resonance::Q(native)
                            }
                            ir::ProcessorParameter::Gain => match &mut f.kind {
                                ir::FilterKind::Peak { gain }
                                | ir::FilterKind::LowShelf { gain }
                                | ir::FilterKind::HighShelf { gain } => {
                                    *gain = ir::Gain::Linear(native)
                                }
                                _ => {}
                            },
                            _ => {}
                        }
                    }
                }
            }
        }
        let params = base
            .values
            .iter()
            .enumerate()
            .map(|(i, (p, _))| (i, *p))
            .collect();
        Self {
            base,
            playing,
            params,
            group,
            runtime_group: g as u32,
        }
    }
    pub fn display(&self, p: Param, n: f32) -> f32 {
        let Some((_, b)) = self.base.bindings.iter().find(|(q, _)| *q == p) else {
            return n;
        };
        let native = b.law.decode((n * 1e6).round() as i32) as f32;
        match p {
            Param::Attack | Param::Hold | Param::Decay | Param::Release => {
                native / self.base.rate as f32
            }
            Param::Curve => n * 2. - 1.,
            Param::Cutoff(_) | Param::Freq(..) => self.base.frequency(p, n).unwrap_or(native),
            Param::Resonance(_) => n,
            Param::Bandwidth(..) => ((0.5 / native).asinh() * 2. / std::f32::consts::LN_2),
            Param::Gain(..) => self.base.gain_db(p, n).unwrap_or(native),
            _ => native,
        }
    }
    pub fn typed(&self, p: Param, text: &str) -> Option<f32> {
        super::viz::typed(p, text, |n| {
            let v = self.display(p, n);
            match p {
                Param::Sustain => 20. * v.max(1e-10).log10(),
                Param::Resonance(_) => v * 100.,
                _ => v,
            }
        })
    }
}
