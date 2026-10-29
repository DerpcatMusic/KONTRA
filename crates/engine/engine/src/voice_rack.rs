//! A group's inserts that run on each voice alone. A group insert acts on
//! each voice; a linear one with fixed settings commutes with the sum,
//! so it stays on the shared chain (filters.rs). An insert runs here when a
//! route moves it (it differs per note), or when it sits at or before the
//! group's last drive effect (a nonlinear stage sees one voice, not the
//! sum). A bypassed insert is built too and skipped while bypassed, so a
//! script may switch it on. Each group's units are built once, after the
//! init callback; a note copies them into its slot's preallocated rack, so
//! the audio thread never allocates, and their state ends with the voice.
//!
//! ponytail: gain, inverter and stereo inserts after a drive effect still
//! fold into the voice's matrix, before it.

use std::sync::Arc;

use kontra_dsp::drive::{self, Crush, Overdrive, Reduction, Shaper, Voicing};
use kontra_dsp::filter::{Formant, Svf, VowelSet, Vowels};
use kontra_ir as ir;

use crate::filters::{VoiceEq, shape};
use crate::ledger::Note;
use crate::modulators::Age;
use crate::{Engine, Zone};

/// ponytail: the lowest damping (`1/Q`) a resonance route may reach; the
/// stages go unstable below it.
const MIN_DAMPING: f32 = 0.01;

/// Frames between a drive unit's steps toward its next refresh: Kontakt's
/// modulated Saturation and Lo-Fi re-read their controls every 32 frames
/// (FUN_140aa4d20, FUN_140aa34b0). Held for the whole 256-frame refresh, a
/// 5 Hz LFO on Saturation differs from that by −24 dB RMS (−12.6 dB at
/// 20 Hz); stepping linearly between refreshes costs no extra spans.
///
/// ponytail: Kontakt also smooths Saturation's tick values with a one-pole
/// (coefficient at +0x1d4, not read); add it if a native render shows it.
const TICK: u32 = 32;

/// One insert of a voice's rack, and the group insert slot it runs.
#[derive(Clone)]
pub(crate) struct Unit {
    pub(crate) insert: u8,
    kind: Kind,
    /// A drive unit's settings now and at the next refresh, `left` frames
    /// away; it steps between them every [`TICK`] frames.
    ramp: Option<Ramp>,
}

#[derive(Clone, Copy)]
struct Ramp {
    from: ir::EffectKind,
    to: ir::EffectKind,
    at: u32,
    len: u32,
}

/// `a` to `b` by `t`, for the controls a route moves.
fn lerp(a: ir::EffectKind, b: ir::EffectKind, t: f32) -> ir::EffectKind {
    let mix = |x: f32, y: f32| x + (y - x) * t;
    match (a, b) {
        (ir::EffectKind::Shaper(mut s), ir::EffectKind::Shaper(e)) => {
            let mut to = e.curve;
            if std::mem::discriminant(&s.curve) == std::mem::discriminant(&to) {
                let x = s.curve.amount_mut();
                *x = mix(*x, *to.amount_mut());
            }
            ir::EffectKind::Shaper(s)
        }
        (ir::EffectKind::Crush(mut c), ir::EffectKind::Crush(e)) => {
            (c.rate, c.bits) = (mix(c.rate, e.rate), mix(c.bits, e.bits));
            ir::EffectKind::Crush(c)
        }
        (a, _) => a,
    }
}

#[derive(Clone)]
enum Kind {
    Filter(Svf),
    Eq(VoiceEq),
    Formant(Formant),
    Shaper(Shaper),
    Crush(Crush),
    Overdrive(Overdrive),
}

fn moved(mods: &[ir::Mod], n: usize) -> bool {
    mods.iter().any(|m| match m.kind {
        ir::ModKind::Cutoff { insert, .. }
        | ir::ModKind::Resonance { insert, .. }
        | ir::ModKind::EqGain { insert, .. }
        | ir::ModKind::EffectParam { insert, .. } => usize::from(insert) == n,
        _ => false,
    })
}

/// The unit for an insert, at its saved settings.
fn unit(kind: ir::EffectKind, rate: u32) -> Option<Kind> {
    Some(match kind {
        ir::EffectKind::Filter(f) => {
            let mut svf = Svf::new(shape(&f));
            svf.set_cutoff(f.hz, rate);
            Kind::Filter(svf)
        }
        ir::EffectKind::Eq(bells) => Kind::Eq(VoiceEq::new(bells, rate)),
        ir::EffectKind::Formant(f) => Kind::Formant(Formant::new(vowels(f), rate)),
        ir::EffectKind::Shaper(s) => Kind::Shaper(shaper(s, rate)),
        ir::EffectKind::Crush(c) => Kind::Crush(Crush::new(reduction(c, rate), 1)),
        ir::EffectKind::Overdrive(o) => Kind::Overdrive(Overdrive::new(voicing(o), rate)),
        _ => return None,
    })
}

fn vowels(f: ir::Formant) -> Vowels {
    Vowels {
        talk: f.talk,
        sharp: f.sharp,
        size: f.size,
        set: match f.set {
            ir::VowelSet::First => VowelSet::First,
            ir::VowelSet::Second => VowelSet::Second,
        },
    }
}

/// The waveshaper for `s` at `rate`, in a group rack or a bus rack.
pub(crate) fn shaper(s: ir::Shaper, rate: u32) -> Shaper {
    let tone = drive::Tone {
        pre_lowpass_hz: s.pre_lowpass_hz,
        lowpass_hz: s.lowpass_hz,
        highpass_hz: s.highpass_hz,
    };
    Shaper::new(curve(s.curve), tone, rate)
}

pub(crate) fn curve(c: ir::DriveCurve) -> drive::Curve {
    match c {
        ir::DriveCurve::Soft(x) => drive::Curve::Soft(x),
        ir::DriveCurve::Tube(x) => drive::Curve::Tube(x),
        ir::DriveCurve::Power(x) => drive::Curve::Power(x),
        ir::DriveCurve::Sine(x) => drive::Curve::Sine(x),
        ir::DriveCurve::Fold(x) => drive::Curve::Fold(x),
        ir::DriveCurve::Tanh(x) => drive::Curve::Tanh(x),
        ir::DriveCurve::Atan(x) => drive::Curve::Atan(x),
    }
}

fn voicing(o: ir::Overdrive) -> Voicing {
    Voicing {
        split_hz: o.split_hz,
        smooth_hz: o.smooth_hz,
        drive: o.drive,
        mix: o.mix,
        level: o.level,
        curve: o.curve,
    }
}

fn reduction(c: ir::Crush, rate: u32) -> Reduction {
    let hold = if c.rate < 1.0 {
        (1.0 / c.rate.max(1e-6)).min(rate as f32 / 50.0) + 1.0
    } else {
        1.0
    };
    Reduction {
        hold,
        bits: (c.bits < 16.0).then_some(c.bits.max(1.0)),
        noise: c.noise,
        noise_tone: c.noise_tone,
    }
}

/// A group's per-voice units in slot order; empty when all of it is shared.
fn template(group: &crate::plan::Group, rate: u32) -> Arc<[Unit]> {
    let last_drive = group.inserts.iter().rposition(|e| {
        e.as_ref().is_some_and(|e| {
            matches!(
                e.kind,
                ir::EffectKind::Shaper(_) | ir::EffectKind::Crush(_) | ir::EffectKind::Overdrive(_)
            )
        })
    });
    group
        .inserts
        .iter()
        .enumerate()
        .filter(|&(n, _)| last_drive.is_some_and(|l| n <= l) || moved(&group.mods, n))
        .filter_map(|(n, e)| {
            let e = e.as_ref()?;
            Some(Unit {
                insert: u8::try_from(n).ok()?,
                kind: unit(e.kind, rate)?,
                ramp: None,
            })
        })
        .collect()
}

/// Runs a voice's rack on `io` (before its envelope and gain), skipping
/// inserts a script has bypassed; a drive effect's output and dry levels
/// follow it, `spare` holding the dry (filters' fold into the matrix).
pub(crate) fn process(
    rack: &mut [Unit],
    inserts: &[Option<ir::Effect>],
    io: &mut [[f32; 2]],
    spare: &mut [[f32; 2]],
    rate: u32,
) {
    for u in rack {
        let Some(e) = inserts.get(usize::from(u.insert)).and_then(Option::as_ref) else {
            continue;
        };
        if e.bypass {
            continue;
        }
        let mut rest = &mut io[..];
        while !rest.is_empty() {
            let n = match &u.ramp {
                Some(r) if r.at < r.len => rest.len().min((TICK - r.at % TICK) as usize),
                _ => rest.len(),
            };
            let (chunk, tail) = rest.split_at_mut(n);
            match &mut u.kind {
                Kind::Filter(f) => f.process(chunk),
                Kind::Eq(q) => q.process(chunk),
                Kind::Formant(f) => f.process(chunk),
                Kind::Shaper(s) => drive(chunk, spare, e, |io| s.process(io)),
                Kind::Crush(c) => drive(chunk, spare, e, |io| c.process(io)),
                Kind::Overdrive(o) => drive(chunk, spare, e, |io| o.process(io)),
            }
            if let Some(r) = &mut u.ramp
                && r.at < r.len
            {
                r.at += n as u32;
                if r.at.is_multiple_of(TICK) || r.at >= r.len {
                    let k = lerp(r.from, r.to, (r.at as f32 / r.len as f32).min(1.0));
                    tune(&mut u.kind, Tune::Effect(k), rate);
                }
            }
            rest = tail;
        }
    }
}

fn drive(
    io: &mut [[f32; 2]],
    spare: &mut [[f32; 2]],
    e: &ir::Effect,
    f: impl FnOnce(&mut [[f32; 2]]),
) {
    let mut dry = spare.get_mut(..io.len()).filter(|_| e.dry != 0.0);
    if let Some(dry) = &mut dry {
        dry.copy_from_slice(io);
    }
    f(io);
    if e.output != 1.0 {
        for x in io.iter_mut().flatten() {
            *x *= e.output;
        }
    }
    if let Some(dry) = dry {
        for (x, d) in io.iter_mut().flatten().zip(dry.iter().flatten()) {
            *x += d * e.dry;
        }
    }
}

fn tune_unit(u: &mut Unit, t: Tune, ramp: Option<Ramp>, rate: u32) {
    u.ramp = ramp;
    tune(&mut u.kind, t, rate);
}

fn tune(kind: &mut Kind, t: Tune, rate: u32) {
    match (kind, t) {
        (Kind::Filter(f), Tune::Filter(hz, k)) => {
            f.set_cutoff(hz, rate);
            f.set_damping(k);
        }
        (Kind::Eq(q), Tune::Eq(bells)) => q.retune(bells, rate),
        (Kind::Formant(f), Tune::Effect(ir::EffectKind::Formant(v))) => f.set(vowels(v), rate),
        (Kind::Shaper(s), Tune::Effect(ir::EffectKind::Shaper(v))) => s.set_curve(curve(v.curve)),
        (Kind::Crush(c), Tune::Effect(ir::EffectKind::Crush(v))) => c.set(reduction(v, rate)),
        (Kind::Overdrive(o), Tune::Effect(ir::EffectKind::Overdrive(v))) => o.set(voicing(v), rate),
        _ => {}
    }
}

/// A moved unit's new settings.
enum Tune {
    Filter(f32, [f32; 3]),
    Eq([ir::Bell; 4]),
    Effect(ir::EffectKind),
}

impl Engine {
    /// Every group's per-voice units, and room for the largest in each
    /// voice. Not real-time.
    pub(crate) fn build_voice_racks(&mut self) {
        let rate = self.rate;
        self.voice_units = self.groups.iter().map(|g| template(g, rate)).collect();
        let most = self.voice_units.iter().map(|u| u.len()).max().unwrap_or(0);
        for slot in &mut self.voices {
            slot.rack = Vec::with_capacity(most);
            // A note an init callback started gets its units too; the next
            // refresh tunes them.
            let group = self.plan.zones.get(slot.zone).map(|z| z.group as usize);
            if slot.voice.is_active()
                && let Some(units) = group.and_then(|g| self.voice_units.get(g))
            {
                slot.rack.extend_from_slice(units);
            }
        }
    }

    /// Gives voice `i` its group's units, tuned for this note at `age`.
    pub(crate) fn start_rack(&mut self, i: usize, zone: &Zone, note: &Note, age: Age) {
        let template = self.voice_units.get(zone.group as usize).cloned();
        let seed = self.now.0 as u32 ^ (i as u32).wrapping_mul(0x9e37_79b9);
        if let Some(slot) = self.voices.get_mut(i) {
            slot.rack.clear();
            // Within the capacity reserved at build: no allocation.
            slot.rack
                .extend_from_slice(template.as_deref().unwrap_or_default());
            for u in &mut slot.rack {
                if let Kind::Crush(c) = &mut u.kind {
                    c.reseed(seed);
                }
            }
        }
        self.retune_rack(i, zone, note, age);
    }

    /// Moves voice `i`'s routed units to this note's settings at `age`;
    /// their state carries on.
    pub(crate) fn retune_rack(&mut self, i: usize, zone: &Zone, note: &Note, age: Age) {
        let rate = self.rate;
        let count = self.voices.get(i).map_or(0, |s| s.rack.len());
        for n in 0..count {
            let Some(u) = self.voices.get(i).and_then(|s| s.rack.get(n)) else {
                continue;
            };
            let mut ramp = None;
            let tune = match u.kind {
                Kind::Filter(_) => self
                    .voice_filter(zone, u.insert, note, age)
                    .map(|(hz, k)| Tune::Filter(hz, k)),
                Kind::Eq(_) => self.voice_bells(zone, u.insert, note, age).map(Tune::Eq),
                Kind::Formant(_) | Kind::Overdrive(_) => self
                    .voice_effect(zone, u.insert, note, age)
                    .map(Tune::Effect),
                Kind::Shaper(_) | Kind::Crush(_) => {
                    let now = self.voice_effect(zone, u.insert, note, age);
                    // The next refresh, on the grid.
                    let len = (crate::MAX_SPAN - self.now.0 % crate::MAX_SPAN) as u32;
                    let dt = len as f32 / rate as f32;
                    let later = Age {
                        held: age.held + dt,
                        now: age.now + dt,
                        ..age
                    };
                    let next = self.voice_effect(zone, u.insert, note, later);
                    if let (Some(from), Some(to)) = (now, next)
                        && from != to
                    {
                        ramp = Some(Ramp {
                            from,
                            to,
                            at: 0,
                            len,
                        });
                    }
                    now.map(Tune::Effect)
                }
            };
            if let (Some(u), Some(t)) =
                (self.voices.get_mut(i).and_then(|s| s.rack.get_mut(n)), tune)
            {
                tune_unit(u, t, ramp, rate);
            }
        }
    }

    /// The cutoff in Hz and damping of the filter in `insert` of `zone`'s
    /// group, moved by its routes for this note at `age`.
    fn voice_filter(
        &self,
        zone: &Zone,
        insert: u8,
        note: &Note,
        age: Age,
    ) -> Option<(f32, [f32; 3])> {
        let group = self.groups.get(zone.group as usize)?;
        let effect = group.inserts.get(usize::from(insert))?.as_ref()?;
        let ir::EffectKind::Filter(f) = effect.kind else {
            return None;
        };
        let (mut octaves, mut q, mut resonance) = (0.0f32, 0.0f32, 1.0f32);
        for m in group.mods.iter().filter(|m| !m.bypass) {
            match m.kind {
                ir::ModKind::Cutoff {
                    source,
                    insert: i,
                    depth,
                } if i == insert => {
                    octaves += self.routed(zone, source, note, age).unwrap_or(0.0) * depth;
                }
                ir::ModKind::Resonance {
                    source,
                    insert: i,
                    depth,
                } if i == insert => match f.ladder {
                    Some(_) => {
                        let depth = depth * self.scale(zone, source, note, age);
                        let x = self.unit(zone, source, note, age);
                        resonance *= x.map_or(1.0, |x| ir::Curve::Signed.gain(depth, x));
                    }
                    None => q += self.routed(zone, source, note, age).unwrap_or(0.0) * depth,
                },
                _ => {}
            }
        }
        let hz = f.hz * (f.octaves * octaves).exp2();
        let damping = f
            .damping
            .map(|k| ((2.0 - (2.0 - k) * resonance) * (-f.q_octaves * q).exp2()).max(MIN_DAMPING));
        Some((hz, damping))
    }

    /// The effect in `insert` of `zone`'s group, its controls moved by their
    /// routes for this note at `age` (a script's change to its base follows
    /// too).
    fn voice_effect(
        &self,
        zone: &Zone,
        insert: u8,
        note: &Note,
        age: Age,
    ) -> Option<ir::EffectKind> {
        let group = self.groups.get(zone.group as usize)?;
        let mut kind = group.inserts.get(usize::from(insert))?.as_ref()?.kind;
        for m in group.mods.iter().filter(|m| !m.bypass) {
            if let ir::ModKind::EffectParam {
                source,
                insert: i,
                param,
                depth,
                unipolar,
                ..
            } = m.kind
                && i == insert
            {
                let x = self.source(zone, source, note, age).unwrap_or(0.0);
                let x = match source {
                    ir::Source::Lfo(_) if unipolar => (x + 1.0) * 0.5,
                    _ => x,
                };
                let amount = x * depth * self.scale(zone, source, note, age);
                param.add(&mut kind, amount);
            }
        }
        Some(kind)
    }

    /// The bands of the EQ in `insert` of `zone`'s group, each gain moved by
    /// its routes for this note at `age`.
    fn voice_bells(&self, zone: &Zone, insert: u8, note: &Note, age: Age) -> Option<[ir::Bell; 4]> {
        let group = self.groups.get(zone.group as usize)?;
        let effect = group.inserts.get(usize::from(insert))?.as_ref()?;
        let ir::EffectKind::Eq(mut bells) = effect.kind else {
            return None;
        };
        let (mut shift, mut limit) = ([0.0f32; 4], [f32::INFINITY; 4]);
        for m in group.mods.iter().filter(|m| !m.bypass) {
            if let ir::ModKind::EqGain {
                source,
                insert: i,
                band,
                depth_db,
                limit_db,
                ..
            } = m.kind
                && i == insert
            {
                let amount = self.routed(zone, source, note, age).unwrap_or(0.0) * depth_db;
                if let (Some(s), Some(l)) = (
                    shift.get_mut(usize::from(band)),
                    limit.get_mut(usize::from(band)),
                ) {
                    *s += amount;
                    *l = l.min(limit_db.abs());
                }
            }
        }
        for ((b, s), l) in bells.iter_mut().zip(shift).zip(limit) {
            // min/max, not clamp: a NaN limit must not panic on the audio thread.
            b.gain_db = (b.gain_db + s).min(l).max(-l);
        }
        if let Some(ir::EffectKind::Eq(moved)) = self.voice_effect(zone, insert, note, age) {
            for (b, m) in bells.iter_mut().zip(moved) {
                b.hz = m.hz;
            }
        }
        Some(bells)
    }

    /// Groups with a filter that runs per voice, and each sounding voice's
    /// first such cutoff.
    pub(crate) fn voice_cutoffs(&self) -> (usize, Vec<f32>) {
        let is_filter = |u: &Unit| matches!(u.kind, Kind::Filter(_));
        let groups = self
            .voice_units
            .iter()
            .filter(|u| u.iter().any(is_filter))
            .count();
        let cutoffs = self
            .voices
            .iter()
            .filter(|s| s.voice.is_active())
            .filter_map(|s| {
                s.rack.iter().find_map(|u| match &u.kind {
                    Kind::Filter(f) => Some(f.cutoff()),
                    _ => None,
                })
            })
            .collect();
        (groups, cutoffs)
    }
}
