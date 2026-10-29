//! What a script changes in the engine while it plays: modulation intensities,
//! envelope stages and controller values. Values arrive in KSP's 0..=1_000_000
//! encoding; the laws are Kontakt's (sampler-core `EngineParameterLaw`).

use kontra_ir as ir;

use crate::plan::{Group, addressed, source};
use crate::{Engine, EnginePar, Level};

/// Effect gains are cubic, unity at this value, so 1_000_000 is +24 dB (v2 sampler-core,
/// fitted to slots whose saved gains follow their scripts' init writes).
const UNITY_EFFECT_GAIN: f64 = 396_851.0;

/// Volume is cubic in the setting, 0 dB at this value, +12 dB at 1_000_000.
const UNITY_VOLUME: f64 = 629_960.524_947_436_1;

/// Attack and hold run 0..=15 s, decay and release 0..=25 s, exponentially
/// with a 2 ms shift: `exp(ln 2 + (ln(top + 2) − ln 2) × x) − 2` ms.
pub(crate) fn stage_ms(top_ms: f64, x: f64) -> f64 {
    let (low, high) = (2f64.ln(), (top_ms + 2.0).ln());
    ((low + (high - low) * x).exp() - 2.0).max(0.0)
}

/// The inverse of [`stage_ms`]: the 0..=1 setting of a stage `ms` long.
/// The attack curve knob (raw / 500000 − 1: −1 concave ..= 1 convex) as a
/// segment shape: the law the Kontakt translator applies to stored curves
/// (format/kontakt/src/laws.rs, docs/re/engine/k-mod-envelopes.md).
fn attack_shape(x: f64) -> f32 {
    let c = (x * 2.0 - 1.0).clamp(-1.0, 1.0);
    let b = f64::from(((1.0 - c.abs()) * 500_000f64.ln() - 20_000f64.ln()).exp() as f32);
    let k = if c > 0.0 {
        (b / (1.0 + b)).ln()
    } else {
        ((1.0 + b) / b).ln()
    };
    k as f32
}

/// Inverse of [`attack_shape`], as a 0..=1 knob.
fn attack_x(shape: f32) -> f64 {
    let k = f64::from(shape);
    if k == 0.0 {
        return 0.5;
    }
    let b = 1.0 / k.abs().exp_m1();
    let c = (1.0 - (b.ln() + 20_000f64.ln()) / 500_000f64.ln()).clamp(0.0, 1.0);
    0.5 * (1.0 - k.signum() * c)
}

/// An LFO's fade-in runs 0..=10 s on the stage law (measured,
/// probes/mod_par_laws.lua: 500_000 saves 139.45 ms, 1_000_000 10 002 ms).
/// Bus n is `BUS_OFFSET + n` (measured, probes/output_channel.lua).
pub(crate) const BUS_OFFSET: i32 = 1024;
const LFO_FADE_TOP_MS: f64 = 10_000.0;

fn stage_x(top_ms: f64, ms: f64) -> f64 {
    let (low, high) = (2f64.ln(), (top_ms + 2.0).ln());
    (((ms + 2.0).ln() - low) / (high - low)).clamp(0.0, 1.0)
}

/// Which of a [`Level`]'s settings a parameter is.
#[derive(Clone, Copy)]
enum LevelPar {
    Volume,
    Pan,
    Tune,
}

impl LevelPar {
    fn set(self, level: &mut Level, raw: f64) {
        match self {
            Self::Volume => level.gain = (raw / UNITY_VOLUME).powi(3) as f32,
            Self::Pan => level.pan = ((raw - 500_000.0) / 500_000.0) as f32,
            // ±36 semitones.
            Self::Tune => level.tune = (raw - 500_000.0) * 7.2e-5,
        }
    }

    fn get(self, level: &Level) -> f64 {
        match self {
            Self::Volume => UNITY_VOLUME * f64::from(level.gain.max(0.0)).cbrt(),
            Self::Pan => f64::from(level.pan) * 500_000.0 + 500_000.0,
            Self::Tune => level.tune / 7.2e-5 + 500_000.0,
        }
    }
}

/// What an effect parameter sets in a slot.
#[derive(Clone, Copy)]
enum EffectPar {
    Bypass,
    Output,
    Dry,
    /// A Send Levels slot's level to send slot n.
    Send(usize),
    Gain,
    Width,
    Pan,
}

impl EffectPar {
    /// Sets it from the raw value; false when the slot has no such control.
    fn set(self, e: &mut ir::Effect, raw: f64) -> bool {
        let gain = (raw / UNITY_EFFECT_GAIN).powi(3) as f32;
        match (self, &mut e.kind) {
            (Self::Bypass, _) => e.bypass = raw != 0.0,
            (Self::Output, _) => e.output = gain,
            (Self::Dry, _) => e.dry = gain,
            // The volume law: Chorus saves 0.24994 for the init write 396821.
            (Self::Send(n), ir::EffectKind::Sends(levels)) => match levels.get_mut(n) {
                Some(level) => *level = (raw / UNITY_VOLUME).powi(3) as f32,
                None => return false,
            },
            (Self::Gain, ir::EffectKind::Gain(g)) => *g = gain,
            // ponytail: width and pan linear in the setting; no measurement yet.
            (Self::Width, ir::EffectKind::Stereo { width, .. }) => *width = (raw / 1e6) as f32,
            (Self::Pan, ir::EffectKind::Stereo { pan, .. }) => *pan = (raw / 5e5 - 1.0) as f32,
            _ => return false,
        }
        true
    }

    fn get(self, e: &ir::Effect) -> Option<f64> {
        let gain = |g: f32| UNITY_EFFECT_GAIN * f64::from(g.max(0.0)).cbrt();
        Some(match (self, e.kind) {
            (Self::Bypass, _) => f64::from(u8::from(e.bypass)),
            (Self::Output, _) => gain(e.output),
            (Self::Dry, _) => gain(e.dry),
            (Self::Send(n), ir::EffectKind::Sends(levels)) => {
                UNITY_VOLUME * f64::from(levels.get(n)?.max(0.0)).cbrt()
            }
            (Self::Gain, ir::EffectKind::Gain(g)) => gain(g),
            (Self::Width, ir::EffectKind::Stereo { width, .. }) => f64::from(width) * 1e6,
            (Self::Pan, ir::EffectKind::Stereo { pan, .. }) => (f64::from(pan) + 1.0) * 5e5,
            _ => return None,
        })
    }
}

fn effect_par(par: EnginePar) -> Option<EffectPar> {
    Some(match par {
        EnginePar::EffectBypass => EffectPar::Bypass,
        EnginePar::EffectOutput => EffectPar::Output,
        EnginePar::EffectDry => EffectPar::Dry,
        EnginePar::SendLevel(n) => EffectPar::Send(usize::from(n)),
        EnginePar::GainerGain => EffectPar::Gain,
        EnginePar::StereoWidth => EffectPar::Width,
        EnginePar::StereoPan => EffectPar::Pan,
        _ => return None,
    })
}

fn level_par(par: EnginePar) -> Option<LevelPar> {
    Some(match par {
        EnginePar::Volume => LevelPar::Volume,
        EnginePar::Pan => LevelPar::Pan,
        EnginePar::Tune => LevelPar::Tune,
        _ => return None,
    })
}

impl Engine {
    /// What the running script's parameter id `id` changes.
    pub(crate) fn engine_par(&self, id: i32) -> Option<EnginePar> {
        self.script()?.engine_pars.get(id)
    }

    /// A group's insert slot (generic -1), the instrument insert's (group
    /// -1, generic `$NI_INSERT_BUS` = 1) or a bus's. Send and main racks are
    /// not carried yet.
    pub(crate) fn effect_mut(
        &mut self,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> Option<&mut ir::Effect> {
        let rack = match (usize::try_from(group), generic) {
            (Ok(g), -1) => &mut self.groups.get_mut(g)?.inserts[..],
            // `$NI_INSERT_BUS` 1, `$NI_SEND_BUS` 0 (one rack per slot), `$NI_MAIN_BUS` 2.
            (Err(_), 1) => &mut self.rack.slots[..],
            (Err(_), 0) => {
                let send = self.sends.get_mut(usize::try_from(slot).ok()?)?;
                return send.slots.first_mut()?.as_mut();
            }
            (Err(_), 2) => &mut self.main.slots[..],
            // `$NI_BUS_OFFSET + n`: bus n is plan bus n + 1 (0 is the main bus).
            (Err(_), b) if b >= BUS_OFFSET => {
                let bus = usize::try_from(b - (BUS_OFFSET - 1)).ok()?;
                &mut self.buses.get_mut(bus)?.rack.slots[..]
            }
            _ => return None,
        };
        rack.get_mut(usize::try_from(slot).ok()?)?.as_mut()
    }

    /// `set_engine_par` of an effect control; false when not carried.
    fn set_effect(
        &mut self,
        par: EnginePar,
        value: i32,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> bool {
        let Some(which) = effect_par(par) else {
            return false;
        };
        let raw = f64::from(value.clamp(0, 1_000_000));
        let set = self
            .effect_mut(group, slot, generic)
            .is_some_and(|e| which.set(e, raw));
        if set {
            self.refresh_all(None);
        }
        set
    }

    /// A group's level, or the instrument's (group -1, generic -1). Buses
    /// (generic `$NI_BUS_OFFSET + n`) are [`Self::bus_mut`]'s.
    fn level_mut(&mut self, group: i32, generic: i32) -> Option<&mut Level> {
        match usize::try_from(group) {
            Ok(g) => self.groups.get_mut(g).map(|g| &mut g.level),
            Err(_) if generic == -1 => Some(&mut self.instrument),
            Err(_) => None,
        }
    }

    /// A reverb control's physical value for a raw 0..=1_000_000 setting, through
    /// the translator's law; `None` when `par` is not one of them.
    fn reverb_value(&self, par: EnginePar, raw: i32) -> Option<(ir::ReverbPar, f32)> {
        let EnginePar::Reverb(which) = par else {
            return None;
        };
        let n = ir::ReverbPar::ALL.iter().position(|&p| p == which)?;
        let law = self.script()?.reverb_laws.get(n)?;
        Some((which, law.eval(raw.clamp(0, 1_000_000) as f32 / 1e6)))
    }

    /// `set_engine_par` of a reverb control on the reverb in that slot. The
    /// rack's reverb follows from the slot on its next block. False when not one of them.
    fn set_reverb(
        &mut self,
        par: EnginePar,
        value: i32,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> bool {
        let Some((which, value)) = self.reverb_value(par, value) else {
            return false;
        };
        match self.effect_mut(group, slot, generic).map(|e| &mut e.kind) {
            Some(ir::EffectKind::Reverb(r)) => {
                which.set(r, value);
                true
            }
            _ => false,
        }
    }

    /// [`Action::GroupVolume`], `GroupPan` or `GroupTune` in their units.
    pub(crate) fn set_group_level(&mut self, action: kontra_vm::Action, group: i32, value: i32) {
        let Some(level) = usize::try_from(group)
            .ok()
            .and_then(|g| self.groups.get_mut(g))
            .map(|g| &mut g.level)
        else {
            return;
        };
        match action {
            kontra_vm::Action::GroupVolume if value <= -200_000 => level.gain = 0.0,
            kontra_vm::Action::GroupVolume => {
                level.gain = 10f32.powf(value as f32 / 20_000.0);
            }
            kontra_vm::Action::GroupPan => level.pan = (value as f32 / 1000.0).clamp(-1.0, 1.0),
            kontra_vm::Action::GroupTune => level.tune = f64::from(value) / 100_000.0,
            _ => return,
        }
        self.refresh_all(None);
    }

    /// `set_engine_par` of volume, pan or tune; false when not one of them.
    fn set_level(&mut self, par: EnginePar, value: i32, group: i32, generic: i32) -> bool {
        let Some(which) = level_par(par) else {
            return false;
        };
        let raw = f64::from(value.clamp(0, 1_000_000));
        if let Some(b) = self.bus_mut(group, generic) {
            let mut level = Level {
                gain: b.gain,
                pan: b.pan,
                tune: 0.0,
            };
            which.set(&mut level, raw);
            (b.gain, b.pan) = (level.gain, level.pan);
            return !matches!(which, LevelPar::Tune);
        }
        let Some(level) = self.level_mut(group, generic) else {
            return false;
        };
        which.set(level, raw);
        self.refresh_all(None);
        true
    }

    /// Instrument bus n's live state for generic `$NI_BUS_OFFSET + n` (plan
    /// bus n + 1). Its volume reads through the volume law: ANALOG STRINGS'
    /// buses 2 and 3 answer 593624 and 525135 in Kontakt (probes/bus_volume_readback.lua),
    /// the knobs of their saved 0.8367 and 0.5793 gains.
    fn bus_mut(&mut self, group: i32, generic: i32) -> Option<&mut crate::BusState> {
        if group >= 0 || generic < BUS_OFFSET {
            return None;
        }
        self.buses
            .get_mut(usize::try_from(generic - (BUS_OFFSET - 1)).ok()?)
    }

    /// Answers `get_engine_par(par, group, slot)` for what
    /// [`Engine::set_engine_par`] sets, in the same encoding, so a script that
    /// saves and restores a parameter gets back what it had. `None`: not answered.
    pub(crate) fn get_engine_par(
        &mut self,
        par: i32,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> Option<i32> {
        let par = self.engine_par(par)?;
        if par == EnginePar::OutputChannel {
            return self.output_channel(group, generic);
        }
        if let EnginePar::Knob(n) = par {
            return self.knob(n, group, slot, generic);
        }
        if let Some(which) = effect_par(par) {
            let e = *self.effect_mut(group, slot, generic)?;
            return Some(which.get(&e)?.round().clamp(0.0, 1e6) as i32);
        }
        if let Some(which) = level_par(par) {
            if let Some(b) = self.bus_mut(group, generic) {
                if matches!(which, LevelPar::Tune) {
                    return None;
                }
                let level = Level {
                    gain: b.gain,
                    pan: b.pan,
                    tune: 0.0,
                };
                return Some(which.get(&level).round().clamp(0.0, 1e6) as i32);
            }
            let level = *self.level_mut(group, generic)?;
            return Some(which.get(&level).round().clamp(0.0, 1e6) as i32);
        }
        let g = usize::try_from(group)
            .ok()
            .and_then(|g| self.groups.get(g))?;
        if let (Some((i, top)), Some(e)) = (stage_point(par), routed_envelope(g, slot)) {
            // Measured (probes/flex_stage_writes.lua): an envelope without
            // stages (a flex) reads 0.
            if !e.stages {
                return Some(0);
            }
            let p = e.points.get(i)?;
            let x = match par {
                EnginePar::Sustain => f64::from(p.level).cbrt(),
                EnginePar::AttackCurve => attack_x(p.curve),
                _ => stage_x(top, f64::from(p.ms)),
            };
            return Some((x * 1e6).round() as i32);
        }
        let ms = |frames: u32| f64::from(frames) * 1000.0 / f64::from(self.rate);
        let e = &g.envelope;
        let route = || g.mods.iter().find(|m| addressed(m, slot, generic));
        let x = match par {
            EnginePar::ModBypass => {
                let m = g.mods.iter().find(|m| i32::from(m.slot) == slot)?;
                return Some(i32::from(m.bypass));
            }
            EnginePar::ReleaseTrigger => return Some(i32::from(g.release_trigger)),
            EnginePar::RouteIntensity => f64::from(signed(&route()?.kind)?.abs()).sqrt(),
            EnginePar::RouteDepth => (f64::from(signed(&route()?.kind)?).cbrt() + 1.0) / 2.0,
            EnginePar::LfoFade => stage_x(LFO_FADE_TOP_MS, f64::from(g.lfo(slot)?.fade_ms)),
            EnginePar::Attack => stage_x(15_000.0, ms(e.attack)),
            EnginePar::Hold => stage_x(15_000.0, ms(e.hold)),
            EnginePar::Decay => stage_x(25_000.0, ms(e.decay)),
            EnginePar::Release => stage_x(25_000.0, ms(e.release)),
            EnginePar::Sustain => f64::from(e.sustain).cbrt(),
            EnginePar::AttackCurve => attack_x(e.attack_shape),
            _ => return None,
        };
        Some((x * 1e6).round() as i32)
    }

    /// Sets the amount of every route named with `#key` to `micros`
    /// millionths ([`kontra_vm::Action::RouteAmount`]).
    pub(crate) fn set_route_amount(&mut self, key: i32, micros: i32) {
        let word = format!("#{key}");
        let v = micros as f32 / 1e6;
        let mut any = false;
        for m in self
            .groups
            .iter_mut()
            .flat_map(|g| g.mods.iter_mut())
            .filter(|m| m.name.split_whitespace().any(|w| w == word))
        {
            any |= match &mut m.kind {
                ir::ModKind::Pitch { semitones, .. } => {
                    *semitones = v;
                    true
                }
                kind => depth(kind).map(|d| *d = v).is_some(),
            };
        }
        if any {
            self.refresh_all(None);
        }
    }

    /// Applies `set_engine_par(par, value, group, slot)`. Returns whether it
    /// changed something the engine carries.
    pub(crate) fn set_engine_par(
        &mut self,
        par: i32,
        value: i32,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> bool {
        let Some(par) = self.engine_par(par) else {
            return false;
        };
        if par == EnginePar::OutputChannel {
            return self.set_output_channel(value, group, generic);
        }
        if self.set_level(par, value, group, generic)
            || self.set_knob(par, value, group, slot, generic)
            || (par == EnginePar::EffectSubtype && self.set_shape(value, group, slot, generic))
            || self.set_effect(par, value, group, slot, generic)
            || self.set_reverb(par, value, group, slot, generic)
        {
            return true;
        }
        let x = f64::from(value.clamp(0, 1_000_000)) / 1e6;
        let Some(g) = usize::try_from(group)
            .ok()
            .and_then(|g| self.groups.get_mut(g))
        else {
            return false;
        };
        let set = match par {
            EnginePar::ModBypass => {
                let mut any = false;
                for m in g.mods.iter_mut().filter(|m| i32::from(m.slot) == slot) {
                    m.bypass = value != 0;
                    any = true;
                }
                any
            }
            // Measured (probes/mod_par_laws.lua): the knob's depth is x²
            // and the route keeps its sign; the bipolar one writes (2x − 1)³.
            EnginePar::RouteIntensity => g.mod_mut(slot, generic).is_some_and(|m| {
                let sign = signed(&m.kind).unwrap_or(1.0).signum();
                set_signed(&mut m.kind, sign * (x * x) as f32)
            }),
            EnginePar::RouteDepth => g
                .mod_mut(slot, generic)
                .is_some_and(|m| set_signed(&mut m.kind, (2.0 * x - 1.0).powi(3) as f32)),
            EnginePar::LfoFade => g.lfo_mut(slot).is_some_and(|l| {
                l.fade_ms = stage_ms(LFO_FADE_TOP_MS, x) as f32;
                true
            }),
            _ => return self.set_envelope(par, x, group, slot),
        };
        if set {
            self.refresh_all(None);
        }
        set
    }

    /// A group's bus as `BUS_OFFSET + n` (plan bus n + 1), −1 for the main bus; a
    /// bus's own output is the main output (−1).
    fn output_channel(&self, group: i32, generic: i32) -> Option<i32> {
        let Ok(g) = usize::try_from(group) else {
            return (generic >= BUS_OFFSET).then_some(-1);
        };
        match self.groups.get(g)?.bus {
            0 => Some(-1),
            bus => i32::try_from(bus).ok()?.checked_add(BUS_OFFSET - 1),
        }
    }

    /// Moves a group to bus `value − BUS_OFFSET` (plan bus one higher); −1 and a
    /// host output put it on the main bus, since the engine mixes every bus
    /// into one stereo output. Playing voices follow at the next span.
    fn set_output_channel(&mut self, value: i32, group: i32, generic: i32) -> bool {
        let bus = match value {
            v if v >= BUS_OFFSET => usize::try_from(v - (BUS_OFFSET - 1))
                .ok()
                .filter(|&b| b < self.buses.len()),
            -1.. => Some(0),
            _ => None,
        };
        let Ok(g) = usize::try_from(group) else {
            // A bus's own output: already the one stereo output.
            return generic >= BUS_OFFSET && bus == Some(0);
        };
        match (bus, self.groups.get_mut(g)) {
            (Some(bus), Some(g)) => {
                g.bus = bus;
                true
            }
            _ => false,
        }
    }

    /// Sets a stage of the envelope at modulator `slot`: the group's
    /// amplitude envelope, or breakpoints with an AHDSR's shape. Script
    /// values are knobs: times through the stage law, sustain level x³
    /// (docs/re/engine/k-mod-envelopes.md, reader 0x140cffb40).
    fn set_envelope(&mut self, par: EnginePar, x: f64, group: i32, slot: i32) -> bool {
        let rate = self.rate;
        let Some(g) = usize::try_from(group)
            .ok()
            .and_then(|g| self.groups.get_mut(g))
        else {
            return false;
        };
        let routed = routed_envelope(g, slot).map(|e| e.stages);
        if let (Some((i, top)), Some(stages)) = (stage_point(par), routed) {
            // Measured (probes/flex_stage_writes.lua): an envelope without
            // stages (a flex) keeps its points; the write is answered.
            if !stages {
                return true;
            }
            let Some(p) = routed_index(g, slot)
                .and_then(|n| g.envelopes.get_mut(n))
                .and_then(|e| e.points.get_mut(i))
            else {
                return false;
            };
            match par {
                EnginePar::Sustain => p.level = x.powi(3) as f32,
                EnginePar::AttackCurve => p.curve = attack_shape(x),
                _ => p.ms = stage_ms(top, x) as f32,
            }
            return true;
        }
        let amplitude = |m: &ir::Mod| i32::from(m.slot) == slot && m.kind == ir::ModKind::Envelope;
        if !g.mods.iter().any(amplitude) {
            return false;
        }
        let frames = |ms: f64| (ms * f64::from(rate) / 1000.0).round() as u32;
        let e = &mut g.envelope;
        match par {
            EnginePar::Attack => e.attack = frames(stage_ms(15_000.0, x)),
            EnginePar::Hold => e.hold = frames(stage_ms(15_000.0, x)),
            EnginePar::Decay => e.decay = frames(stage_ms(25_000.0, x)),
            EnginePar::Release => e.release = frames(stage_ms(25_000.0, x)),
            EnginePar::Sustain => e.sustain = x.powi(3) as f32,
            EnginePar::AttackCurve => e.attack_shape = attack_shape(x),
            _ => return false,
        }
        true
    }

    /// `set_controller`: the script moves a controller as if it had arrived.
    pub(crate) fn set_controller(&mut self, controller: i32, value: i32) {
        let (Ok(cc), Ok(value)) = (u8::try_from(controller), u8::try_from(value.clamp(0, 127)))
        else {
            return;
        };
        if let Some(v) = self.cc_values.get_mut(usize::from(cc)) {
            *v = value;
        }
        if cc < 128 {
            for channel in 0..16 {
                self.behaviour.set_cc(channel, cc, value);
            }
            self.refresh_all(Some(cc));
        }
    }

    /// Re-reads gain (and pitch, unless `cc` moved) of every sounding note.
    pub(crate) fn refresh_all(&mut self, cc: Option<u8>) {
        let mut hits = std::mem::take(&mut self.hits);
        self.notes.select(&mut hits, |_| true);
        for &note in &hits {
            self.refresh_note(note, cc);
        }
        self.hits = hits;
    }
}

/// The point of an AHDSR's breakpoints a stage parameter sets, and the top
/// of its time law in ms.
fn stage_point(par: EnginePar) -> Option<(usize, f64)> {
    Some(match par {
        EnginePar::Attack | EnginePar::AttackCurve => (0, 15_000.0),
        EnginePar::Hold => (1, 15_000.0),
        EnginePar::Decay | EnginePar::Sustain => (2, 25_000.0),
        EnginePar::Release => (3, 25_000.0),
        _ => return None,
    })
}

/// The index of the breakpoints modulator `slot` of `g` routes from.
fn routed_index(g: &Group, slot: i32) -> Option<usize> {
    g.mods.iter().find_map(|m| match source(&m.kind) {
        Some(ir::Source::Envelope(n)) if i32::from(m.slot) == slot => Some(usize::from(n)),
        _ => None,
    })
}

/// The breakpoints modulator `slot` of `g` routes from.
fn routed_envelope(g: &Group, slot: i32) -> Option<&ir::Breakpoints> {
    g.envelopes.get(routed_index(g, slot)?)
}

/// A route's signed depth; an EQ gain or effect route's is its depth over
/// its span.
fn signed(kind: &ir::ModKind) -> Option<f32> {
    match *kind {
        ir::ModKind::EqGain {
            depth_db, span_db, ..
        } => Some(depth_db / span_db),
        ir::ModKind::EffectParam { depth, span, .. } => Some(depth / span),
        ir::ModKind::LfoPhase { depth, .. } | ir::ModKind::LoopShift { depth, .. } => Some(depth),
        ref other => depth(&mut other.clone()).map(|d| *d),
    }
}

/// Sets a route's signed depth. Returns whether the route has one.
fn set_signed(kind: &mut ir::ModKind, d: f32) -> bool {
    if let ir::ModKind::EqGain {
        depth_db, span_db, ..
    } = kind
    {
        *depth_db = d * *span_db;
        return true;
    }
    if let ir::ModKind::EffectParam { depth, span, .. } = kind {
        *depth = d * *span;
        return true;
    }
    // Bipolar (measured, probes/mod_par_laws.lua on ANALOG's Phase_P1).
    if let ir::ModKind::LfoPhase { depth, .. } = kind {
        *depth = d;
        return true;
    }
    // Signed; scripts set it bipolar and cubed (v1 audits/MODULATION.md:
    // ANALOG's 751086 lands on the native 0.1266360730).
    if let ir::ModKind::LoopShift { depth, .. } = kind {
        *depth = d;
        return true;
    }
    depth(kind).map(|v| *v = d).is_some()
}

fn depth(kind: &mut ir::ModKind) -> Option<&mut f32> {
    match kind {
        ir::ModKind::Volume { intensity, .. }
        | ir::ModKind::Attack { intensity, .. }
        | ir::ModKind::Release { intensity, .. } => Some(intensity),
        ir::ModKind::Pitch { depth, .. }
        | ir::ModKind::Cutoff { depth, .. }
        | ir::ModKind::Resonance { depth, .. }
        | ir::ModKind::SampleStart { depth, .. }
        | ir::ModKind::Intensity { depth, .. }
        | ir::ModKind::LfoRate { depth, .. } => Some(depth),
        ir::ModKind::Pan { amount, .. } => Some(amount),
        ir::ModKind::Envelope
        | ir::ModKind::EqGain { .. }
        | ir::ModKind::EffectParam { .. }
        | ir::ModKind::Mix { .. }
        | ir::ModKind::LfoPhase { .. }
        | ir::ModKind::LoopShift { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{attack_shape, attack_x, set_signed, signed, stage_ms, stage_x};
    use kontra_ir as ir;

    #[test]
    fn an_eq_gain_depth_is_its_gain_over_its_span() {
        // A depth past twice the limit survives a read and write back.
        let mut kind = ir::ModKind::EqGain {
            source: ir::Source::Velocity,
            insert: 0,
            band: 0,
            depth_db: -48.0,
            limit_db: 6.0,
            span_db: 72.0,
        };
        let d = signed(&kind).unwrap();
        assert!(set_signed(&mut kind, d));
        assert!(
            matches!(kind, ir::ModKind::EqGain { depth_db, .. } if (depth_db + 48.0).abs() < 1e-4)
        );
        assert!(set_signed(&mut kind, -0.25));
        assert!(matches!(kind, ir::ModKind::EqGain { depth_db, .. } if depth_db == -18.0));
        assert_eq!(signed(&kind), Some(-0.25));
    }

    #[test]
    fn a_start_phase_route_depth_is_signed() {
        let mut kind = ir::ModKind::LfoPhase {
            source: ir::Source::Constant,
            lfo: 0,
            depth: -1.0,
        };
        assert_eq!(signed(&kind), Some(-1.0));
        assert!(set_signed(&mut kind, 0.5));
        assert!(matches!(kind, ir::ModKind::LfoPhase { depth, .. } if depth == 0.5));
    }

    #[test]
    fn a_loop_route_depth_is_signed() {
        let mut kind = ir::ModKind::LoopShift {
            source: ir::Source::Constant,
            length: false,
            depth: 0.0,
        };
        // ANALOG STRINGS writes 751086; Kontakt holds 0.1266360730 (v1 audit).
        assert!(set_signed(&mut kind, (2.0f32 * 0.751_086 - 1.0).powi(3)));
        let ir::ModKind::LoopShift { depth, .. } = kind else {
            panic!("a loop route");
        };
        assert!((depth - 0.126_636_07).abs() < 1e-6, "{depth}");
    }

    #[test]
    fn an_effect_route_depth_is_its_depth_over_its_span() {
        let mut kind = ir::ModKind::EffectParam {
            source: ir::Source::Velocity,
            insert: 0,
            param: ir::EffectParam::CrushBits,
            depth: -8.0,
            span: 16.0,
            unipolar: false,
        };
        assert_eq!(signed(&kind), Some(-0.5));
        assert!(set_signed(&mut kind, -0.25));
        assert!(matches!(kind, ir::ModKind::EffectParam { depth, .. } if depth == -4.0));
    }

    #[test]
    fn attack_curve_round_trips() {
        for x in [0.1, 0.3, 0.5, 0.7, 0.95] {
            assert!((attack_x(attack_shape(x)) - x).abs() < 1e-4, "{x}");
        }
    }

    #[test]
    fn stage_times_follow_kontakts_shifted_exponential() {
        assert!(stage_ms(15_000.0, 0.0).abs() < 1e-9);
        assert!((stage_ms(15_000.0, 1.0) - 15_000.0).abs() < 1e-6);
        assert!((stage_ms(25_000.0, 0.5) - ((2.0f64 * 25_002.0).sqrt() - 2.0)).abs() < 1e-6);
        for x in [0.0, 0.25, 0.8, 1.0] {
            assert!((stage_x(25_000.0, stage_ms(25_000.0, x)) - x).abs() < 1e-9);
        }
    }
}
