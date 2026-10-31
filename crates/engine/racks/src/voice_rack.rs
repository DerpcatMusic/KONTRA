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

use kontra_dsp::drive::{Crush, Overdrive, Reduction, Shaper, Voicing};
use kontra_dsp::filter::{Formant, Svf, VowelSet, Vowels};
use kontra_ir as ir;
use kontra_plan::route_depth;

use kontra_effects::processors::{curve, shaper};

use kontra_effects::summed::{VoiceEq, shape};

/// ponytail: the lowest damping (`1/Q`) a resonance route may reach; the
/// stages go unstable below it.
pub const MIN_DAMPING: f32 = 0.01;

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
pub struct Unit {
    pub insert: u8,
    pub kind: Kind,
    /// A drive unit's settings now and at the next refresh, `left` frames
    /// away; it steps between them every [`TICK`] frames.
    pub ramp: Option<Ramp>,
}

#[derive(Clone, Copy)]
pub struct Ramp {
    pub from: ir::EffectKind,
    pub to: ir::EffectKind,
    pub at: u32,
    pub len: u32,
}

#[derive(Clone)]
pub enum Kind {
    Filter(Svf),
    Eq(VoiceEq),
    /// An amp's bands and its folded input·output gain.
    Amp {
        eq: VoiceEq,
        gain: f32,
    },
    Formant(Formant),
    Shaper(Shaper),
    Crush(Crush),
    Overdrive(Overdrive),
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
        ir::EffectKind::Amp(a) => Kind::Amp {
            eq: VoiceEq::new(a.bands, rate),
            gain: a.gain(),
        },
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
pub fn template(group: &kontra_plan::Group, rate: u32) -> Arc<[Unit]> {
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
        .filter(|&(n, _)| last_drive.is_some_and(|l| n <= l) || route_depth::moved(&group.mods, n))
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
pub fn process(
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
                Kind::Amp { eq, gain } => {
                    eq.process(chunk);
                    for frame in chunk.iter_mut() {
                        frame[0] *= *gain;
                        frame[1] *= *gain;
                    }
                }
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
                    let k = route_depth::lerp(r.from, r.to, (r.at as f32 / r.len as f32).min(1.0));
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

pub fn tune_unit(u: &mut Unit, t: Tune, ramp: Option<Ramp>, rate: u32) {
    u.ramp = ramp;
    tune(&mut u.kind, t, rate);
}

fn tune(kind: &mut Kind, t: Tune, rate: u32) {
    match (kind, t) {
        (Kind::Filter(f), Tune::Filter(hz, k)) => {
            f.set_cutoff(hz, rate);
            f.set_damping(k);
        }
        (Kind::Eq(q) | Kind::Amp { eq: q, .. }, Tune::Eq(bells)) => q.retune(bells, rate),
        (Kind::Formant(f), Tune::Effect(ir::EffectKind::Formant(v))) => f.set(vowels(v), rate),
        (Kind::Shaper(s), Tune::Effect(ir::EffectKind::Shaper(v))) => s.set_curve(curve(v.curve)),
        (Kind::Crush(c), Tune::Effect(ir::EffectKind::Crush(v))) => c.set(reduction(v, rate)),
        (Kind::Overdrive(o), Tune::Effect(ir::EffectKind::Overdrive(v))) => o.set(voicing(v), rate),
        _ => {}
    }
}

/// A moved unit's new settings.
pub enum Tune {
    Filter(f32, [f32; 3]),
    Eq([ir::Bell; 4]),
    Effect(ir::EffectKind),
}
