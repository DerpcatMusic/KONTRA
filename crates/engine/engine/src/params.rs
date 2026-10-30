//! What a script changes in the engine while it plays: modulation intensities,
//! envelope stages and controller values. Values arrive in KSP's 0..=1_000_000
//! encoding; the laws are Kontakt's (sampler-core `EngineParameterLaw`).

use kontra_ir as ir;

use crate::plan::{Group, addressed, source};
use crate::{Engine, EnginePar, Level};
use kontra_plan::par_laws::{
    CUBE, SIGNED_CUBE, SQUARE, STAGE_10S, STAGE_15S, STAGE_25S, attack_shape, attack_x, cube_root,
    setting, stage_setting,
};
use kontra_plan::route_depth::{depth, set_signed, signed};

/// Effect gains are cubic, unity at this value, so 1_000_000 is +24 dB (v2 sampler-core,
/// fitted to slots whose saved gains follow their scripts' init writes).
const UNITY_EFFECT_GAIN: f64 = 396_851.0;

/// Volume is cubic in the setting, 0 dB at this value, +12 dB at 1_000_000.
const UNITY_VOLUME: f64 = 629_960.524_947_436_1;

/// Bus n is `BUS_OFFSET + n` (measured, probes/output_channel.lua).
pub(crate) const BUS_OFFSET: i32 = 1024;

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
            Self::Volume => level.gain = CUBE.eval_unclamped_f64(raw / UNITY_VOLUME) as f32,
            Self::Pan => level.pan = ((raw - 500_000.0) / 500_000.0) as f32,
            // ±36 semitones.
            Self::Tune => level.tune = (raw - 500_000.0) * 7.2e-5,
        }
    }

    fn get(self, level: &Level) -> f64 {
        match self {
            Self::Volume => UNITY_VOLUME * cube_root(level.gain),
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
        let gain = CUBE.eval_unclamped_f64(raw / UNITY_EFFECT_GAIN) as f32;
        match (self, &mut e.kind) {
            (Self::Bypass, _) => e.bypass = raw != 0.0,
            (Self::Output, _) => e.output = gain,
            (Self::Dry, _) => e.dry = gain,
            // The volume law: Chorus saves 0.24994 for the init write 396821.
            (Self::Send(n), ir::EffectKind::Sends(levels)) => match levels.get_mut(n) {
                Some(level) => *level = CUBE.eval_unclamped_f64(raw / UNITY_VOLUME) as f32,
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
        let gain = |g: f32| UNITY_EFFECT_GAIN * cube_root(g);
        Some(match (self, e.kind) {
            (Self::Bypass, _) => f64::from(u8::from(e.bypass)),
            (Self::Output, _) => gain(e.output),
            (Self::Dry, _) => gain(e.dry),
            (Self::Send(n), ir::EffectKind::Sends(levels)) => {
                UNITY_VOLUME * cube_root(*levels.get(n)?)
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
        self.slot_mut(group, slot, generic)?.as_mut()
    }

    /// The addressed slot, empty or not.
    pub(crate) fn slot_mut(
        &mut self,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> Option<&mut Option<ir::Effect>> {
        let rack = match (usize::try_from(group), generic) {
            (Ok(g), -1) => &mut self.groups.get_mut(g)?.inserts[..],
            // `$NI_INSERT_BUS` 1, `$NI_SEND_BUS` 0 (one rack per slot), `$NI_MAIN_BUS` 2.
            (Err(_), 1) => &mut self.rack.slots[..],
            (Err(_), 0) => {
                let send = self.sends.get_mut(usize::try_from(slot).ok()?)?;
                return send.slots.first_mut();
            }
            (Err(_), 2) => &mut self.main.slots[..],
            // `$NI_BUS_OFFSET + n`: bus n is plan bus n + 1 (0 is the main bus).
            (Err(_), b) if b >= BUS_OFFSET => {
                let bus = usize::try_from(b - (BUS_OFFSET - 1)).ok()?;
                &mut self.buses.get_mut(bus)?.rack.slots[..]
            }
            _ => return None,
        };
        rack.get_mut(usize::try_from(slot).ok()?)
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
    pub(crate) fn reverb_value(&self, par: EnginePar, raw: i32) -> Option<(ir::ReverbPar, f32)> {
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
                level.gain = ir::common::MILLIBEL_GAIN.eval_unclamped(value as f32);
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
        if par == EnginePar::EffectSubtype {
            return self.subtype(group, slot, generic);
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
        if let (Some((i, stage)), Some(e)) = (stage_point(par), routed_envelope(g, slot)) {
            // Measured (probes/flex_stage_writes.lua): an envelope without
            // stages (a flex) reads 0.
            if !e.stages {
                return Some(0);
            }
            let p = e.points.get(i)?;
            let x = match par {
                EnginePar::Sustain => setting(&CUBE, f64::from(p.level)),
                EnginePar::AttackCurve => attack_x(p.curve),
                _ => stage_setting(&stage, f64::from(p.ms)),
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
            EnginePar::RouteIntensity => setting(&SQUARE, f64::from(signed(&route()?.kind)?.abs())),
            EnginePar::RouteDepth => setting(&SIGNED_CUBE, f64::from(signed(&route()?.kind)?)),
            EnginePar::LfoFade => stage_setting(&STAGE_10S, f64::from(g.lfo(slot)?.fade_ms)),
            EnginePar::Attack => stage_setting(&STAGE_15S, ms(e.attack)),
            EnginePar::Hold => stage_setting(&STAGE_15S, ms(e.hold)),
            EnginePar::Decay => stage_setting(&STAGE_25S, ms(e.decay)),
            EnginePar::Release => stage_setting(&STAGE_25S, ms(e.release)),
            EnginePar::Sustain => setting(&CUBE, f64::from(e.sustain)),
            EnginePar::AttackCurve => attack_x(e.attack_shape),
            _ => return None,
        };
        Some((x * 1e6).round() as i32)
    }

    /// Sets the amount of every route named with `#key` to `micros`
    /// millionths ([`kontra_vm::Action::RouteAmount`]).
    pub(crate) fn set_route_amount(&mut self, key: i32, micros: i32) {
        let v = micros as f32 / 1e6;
        let mut any = false;
        for (g, m) in self.route_names.of(key) {
            let Some(m) = self.groups.get_mut(g).and_then(|g| g.mods.get_mut(m)) else {
                continue;
            };
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
            || (par == EnginePar::ReverbNetwork
                && self.set_reverb_network(value, group, slot, generic))
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
                set_signed(&mut m.kind, sign * SQUARE.eval_f64(x) as f32)
            }),
            EnginePar::RouteDepth => g
                .mod_mut(slot, generic)
                .is_some_and(|m| set_signed(&mut m.kind, SIGNED_CUBE.eval_f64(x) as f32)),
            EnginePar::LfoFade => g.lfo_mut(slot).is_some_and(|l| {
                l.fade_ms = STAGE_10S.eval_f64(x) as f32;
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
        if let (Some((i, stage)), Some(stages)) = (stage_point(par), routed) {
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
                EnginePar::Sustain => p.level = CUBE.eval_f64(x) as f32,
                EnginePar::AttackCurve => p.curve = attack_shape(x),
                _ => p.ms = stage.eval_f64(x) as f32,
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
            EnginePar::Attack => e.attack = frames(STAGE_15S.eval_f64(x)),
            EnginePar::Hold => e.hold = frames(STAGE_15S.eval_f64(x)),
            EnginePar::Decay => e.decay = frames(STAGE_25S.eval_f64(x)),
            EnginePar::Release => e.release = frames(STAGE_25S.eval_f64(x)),
            EnginePar::Sustain => e.sustain = CUBE.eval_f64(x) as f32,
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

/// The point of an AHDSR's breakpoints a stage parameter sets, and its
/// time law.
fn stage_point(par: EnginePar) -> Option<(usize, ir::Law)> {
    Some(match par {
        EnginePar::Attack | EnginePar::AttackCurve => (0, STAGE_15S),
        EnginePar::Hold => (1, STAGE_15S),
        EnginePar::Decay | EnginePar::Sustain => (2, STAGE_25S),
        EnginePar::Release => (3, STAGE_25S),
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
