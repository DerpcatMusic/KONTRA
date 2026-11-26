//! What a script changes in the engine while it plays: modulation intensities,
//! envelope stages and controller values. Values arrive in normalized
//! `0..=1_000_000` units and are mapped through the script plan's parameter laws.

use kontra_ir as ir;

use crate::plan::{Group, Holder, addressed, is_dbd};
use crate::{Engine, EnginePar, Kind, Level};
use kontra_plan::par_laws::{
    CUBE, SIGNED_CUBE, SQUARE, STAGE_10S, STAGE_15S, STAGE_25S, attack_shape, attack_x,
    routed_envelope, routed_index, setting, stage_point, stage_setting,
};
use kontra_plan::route_depth::{has_depth, set_signed, signed};

/// Glide portamento time knob: `2·(1251^x − 1)` ms (`0.00` .. `2.5k` ms).
const GLIDE_MS: ir::Law = ir::Law::ExpAffine {
    base: 1_251.0,
    offset: 0.0,
    factor: 1.0,
    shift: -1.0,
    scale: 2.0,
};

pub(crate) use kontra_plan::par_values::{BUS_OFFSET, LevelPar, RackScheme, effect_par, level_par};

impl Engine {
    /// What the running script's parameter id `id` changes.
    pub(crate) fn engine_par(&self, id: i32) -> Option<EnginePar> {
        self.script()?.module_pars.get(id)
    }

    /// A send level addressed to the send chain (generic 0) on the instrument
    /// moves its insert chain's Send Levels slot: send effects have no send
    /// levels of their own (a library's reverb amount knob raises its tail
    /// 10 dB this way; tools/oracle/controls.py, 2026-11-12).
    pub(crate) fn send_generic(&self, par: i32, group: i32, generic: i32) -> i32 {
        let send = matches!(self.engine_par(par), Some(EnginePar::SendLevel(_)));
        if send && group < 0 && generic == 0 {
            1
        } else {
            generic
        }
    }

    /// Group `group`, if the instrument has it.
    pub(crate) fn group_mut(&mut self, group: i32) -> Option<&mut Group> {
        self.groups.get_mut(usize::try_from(group).ok()?)
    }

    /// Whether `par` at this address names nothing here: a group, bus,
    /// slot, LFO, route or modulator the instrument lacks. Such a read is
    /// 0, its text "" and a set there is dropped.
    pub(crate) fn absent(&mut self, par: i32, group: i32, slot: i32, generic: i32) -> bool {
        use Holder as H;
        // A parameter of one effect type, on a slot holding another type.
        let owner = self.script().and_then(|s| s.module_pars.owner(par));
        if let Some(owner) = owner
            && self
                .effect_type(group, slot, generic)
                .is_some_and(|ty| ty != owner)
        {
            return true;
        }
        let found = self.engine_par(par).zip(self.script());
        match found.map(|(p, s)| (p, s.holder(p))) {
            Some(_) if usize::try_from(group).is_ok_and(|g| g >= self.groups.len()) => true,
            Some((_, H::Slot)) => !matches!(self.slot_mut(group, slot, generic), Some(Some(_))),
            Some((_, H::SlotKind)) => self.slot_mut(group, slot, generic).is_none(),
            Some((_, h @ (H::Lfo | H::Route | H::Mod))) => self
                .group_mut(group)
                .is_none_or(|g| g.lacks(h, slot, generic)),
            Some((_, H::Level)) => {
                group < 0 && generic >= BUS_OFFSET && self.bus_mut(group, generic).is_none()
            }
            // A group knob may address the instrument (-1); nothing else does.
            Some((p, H::Group)) => group < 0 && !matches!(p, EnginePar::Knob(_)),
            None => false,
        }
    }

    /// A group's insert slot, the instrument's insert/send/main rack, or a bus rack.
    pub(crate) fn effect_mut(
        &mut self,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> Option<&mut ir::Effect> {
        self.slot_mut(group, slot, generic)?.as_mut()
    }

    /// The addressed slot, empty or not, resolved through [`RackScheme::DEFAULT`].
    pub(crate) fn slot_mut(
        &mut self,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> Option<&mut Option<ir::Effect>> {
        let rack = match RackScheme::DEFAULT.rack_at(group, generic)? {
            ir::RackAt::Group(g) => {
                let g = g as usize;
                self.group_written(g);
                &mut self.groups.get_mut(g)?.inserts[..]
            }
            ir::RackAt::Inserts => &mut self.rack.slots[..],
            ir::RackAt::Sends => {
                let send = self.sends.get_mut(usize::try_from(slot).ok()?)?;
                return send.slots.first_mut();
            }
            ir::RackAt::Main => &mut self.main.slots[..],
            ir::RackAt::Bus(b) => {
                let bus = usize::from(b).checked_add(1)?;
                &mut self.buses.get_mut(bus)?.rack.slots[..]
            }
        };
        rack.get_mut(usize::try_from(slot).ok()?)
    }

    /// Keeps `value` for held control `name` ([`ir::held_key`]) of the
    /// addressed slot (the address exists: `absent` ruled out the rest).
    /// False when the store is full.
    fn hold(&mut self, name: u32, value: i32, group: i32, slot: i32, generic: i32) -> bool {
        let Some((at, slot)) = slot_at(group, slot, generic) else {
            return false;
        };
        let held = &mut self.behaviour.held;
        if let Some(h) = held
            .iter_mut()
            .find(|h| (h.0, h.1, h.2) == (at, slot, name))
        {
            h.3 = value;
        } else if held.len() < held.capacity() {
            held.push((at, slot, name, value));
        } else {
            return false;
        }
        true
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

    /// [`kontra_vm::Action::EffectBypass`]: bypasses (`on`) or plays slot
    /// `slot` of group `index`'s inserts (`at` 0), bus `index`'s rack (1) or
    /// the instrument's inserts (2).
    pub(crate) fn set_effect_bypass(&mut self, at: i32, index: i32, slot: i32, on: bool) {
        if let Some(e) = self.placed_effect(at, index, slot)
            && e.bypass != on
        {
            e.bypass = on;
            self.refresh_all(None);
        }
    }

    /// [`kontra_vm::Action::EffectGain`]: a gain effect's linear level.
    pub(crate) fn set_effect_gain(&mut self, at: i32, index: i32, slot: i32, gain: f32) {
        if let Some(ir::Effect {
            kind: ir::EffectKind::Gain(g),
            ..
        }) = self.placed_effect(at, index, slot)
        {
            *g = gain.max(0.0);
        }
    }

    /// [`kontra_vm::Action::EffectField`]: one control of a placed effect.
    pub(crate) fn set_effect_field(
        &mut self,
        at: i32,
        index: i32,
        slot: i32,
        code: i32,
        value: f32,
    ) {
        let Some(param) = u16::try_from(code)
            .ok()
            .and_then(ir::EffectParam::from_code)
        else {
            return;
        };
        if let Some(e) = self.placed_effect(at, index, slot)
            && param.set(e, value)
        {
            self.refresh_all(None);
        }
    }

    fn placed_effect(&mut self, at: i32, index: i32, slot: i32) -> Option<&mut ir::Effect> {
        let (group, generic) = placed(at, index);
        self.effect_mut(group, slot, generic)
    }

    /// [`kontra_vm::Action::LoadImpulse`]: the library response `name` into
    /// the placed convolution, as [`Engine::load_ir_sample`] loads one. A
    /// group's inserts run per voice: `None`, as `load_ir_sample` answers.
    /// [`kontra_vm::Action::ImpulseKnob`]: knob `code` (1 the share of the
    /// response kept, 2 predelay in ms) of the placed convolution at `y`.
    /// False when it cannot be prepared again.
    pub(crate) fn set_placed_impulse_knob(
        &mut self,
        at: i32,
        index: i32,
        slot: i32,
        code: i32,
        y: f32,
    ) -> bool {
        let (knob, y) = match code {
            1 => (ir::ImpulseKnob::Length, y.clamp(0.0, 1.0)),
            2 => (ir::ImpulseKnob::PredelayMs, y.clamp(0.0, 1000.0)),
            _ => return false,
        };
        match placed(at, index) {
            (-1, generic) => self.set_impulse_knob(knob, y, slot, generic),
            _ => false,
        }
    }

    pub(crate) fn load_placed_impulse(
        &mut self,
        name: &str,
        at: i32,
        index: i32,
        slot: i32,
    ) -> Option<i32> {
        match placed(at, index) {
            (-1, generic) => self.load_ir_sample(name, slot, generic),
            _ => None,
        }
    }

    /// A group's level, or the instrument's (group -1, generic -1). Buses
    /// (generic `bus_offset + n`) are [`Self::bus_mut`]'s.
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
        let law = self.script()?.reverb_laws.get(which);
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
                r.set_control(which, value);
                true
            }
            _ => false,
        }
    }

    /// [`Action::GroupVolume`], `GroupPan` or `GroupTune` in their units.
    pub(crate) fn set_group_level(&mut self, action: kontra_vm::Action, group: i32, value: i32) {
        let Some(level) = self.group_mut(group).map(|g| &mut g.level) else {
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
            let mut level = Level::panned(b.gain, b.pan);
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
        let generic = self.send_generic(par, group, generic);
        if self.absent(par, group, slot, generic) {
            return Some(0);
        }
        let par = self.engine_par(par)?;
        if let EnginePar::Opaque(name) = par {
            let (at, slot) = slot_at(group, slot, generic)?;
            let mut held = self.behaviour.held.iter();
            return held
                .find(|h| (h.0, h.1, h.2) == (at, slot, name))
                .map(|h| h.3);
        }
        if let EnginePar::GroupHeld { key, .. } = par {
            let held = &self.group_mut(group)?.held;
            return Some(group_held(held, key, slot).map_or(0, |h| h.2));
        }
        if par == EnginePar::OutputChannel {
            return self.output_channel(group, generic);
        }
        if let EnginePar::Knob(n) = par {
            return self.knob(n, group, slot, generic);
        }
        if par == EnginePar::EffectSubtype {
            return self.subtype(group, slot, generic);
        }
        if par == EnginePar::SlotKind {
            return self.effect_type(group, slot, generic);
        }
        if let EnginePar::Reverb(which) = par {
            let ir::EffectKind::Reverb(reverb) = &self.effect_mut(group, slot, generic)?.kind
            else {
                return None;
            };
            let physical = reverb.control(which);
            let law = self.script()?.reverb_laws.get(which);
            let physical = f64::from(physical);
            let value = law.inverse_f64(physical)?;
            return value
                .is_finite()
                .then(|| (value * 1e6).round().clamp(0.0, 1e6) as i32);
        }
        if let Some(which) = effect_par(par) {
            let e = self.effect_mut(group, slot, generic)?;
            return Some(which.get(e)?.round().clamp(0.0, 1e6) as i32);
        }
        if let Some(which) = level_par(par) {
            let level = match self.bus_mut(group, generic) {
                Some(_) if matches!(which, LevelPar::Tune) => return None,
                Some(b) => Level::panned(b.gain, b.pan),
                None => *self.level_mut(group, generic)?,
            };
            return Some(which.get(&level).round().clamp(0.0, 1e6) as i32);
        }
        let g = self.groups.get(usize::try_from(group).ok()?)?;
        if let EnginePar::Wavetable(v) = par {
            let mut read = g.wavetable?;
            let x = *crate::voice_gain::table_value(&mut read, v)?;
            return Some((f64::from(x) * 1e6).round() as i32);
        }
        if par == EnginePar::InharmonicOn {
            return Some(i32::from(g.wavetable?.inharmonic.on));
        }
        if par == EnginePar::ModOscKind {
            let kind = g.wavetable?.modosc.kind;
            let kinds = &self.script()?.mod_kinds;
            return i32::try_from(kinds.iter().position(|k| *k == Some(kind))?).ok();
        }
        if par == EnginePar::TableModWave {
            return Some(i32::from(g.wavetable?.modosc.wave));
        }
        if par == EnginePar::TableQuality {
            return Some(match g.wavetable?.quality {
                kontra_voice::Quality::Lofi => 0,
                kontra_voice::Quality::Medium => 1,
                kontra_voice::Quality::High => 2,
                kontra_voice::Quality::Best => 3,
            });
        }
        if par == EnginePar::ModOscUnit {
            return Some(match g.wavetable?.modosc.unit {
                kontra_voice::Unit::Semitones => 0,
                kontra_voice::Unit::Ratio => 1,
                kontra_voice::Unit::Hz => 2,
            });
        }
        if let EnginePar::WarpMode(n) = par {
            let w = g.wavetable?.warps.get(usize::from(n)).copied()?;
            let modes = &self.script()?.warp_modes;
            let mode = modes.iter().position(|m| *m == Some((w.shape, w.shaper)))?;
            return i32::try_from(mode).ok();
        }
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
        // A carried route's depth, else the saved one of a route not carried.
        let route = || {
            let carried = g.mods.iter().find(|m| addressed(m, slot, generic));
            carried.and_then(|m| signed(&m.route)).or_else(|| {
                let r = g.routes.iter().find(|r| i32::from(r.slot) == slot)?;
                r.depths.get(usize::try_from(generic.max(0)).ok()?).copied()
            })
        };
        // A switch, read as set: 1 restarts with each voice (measured).
        if par == EnginePar::LfoRetrigger {
            return Some(i32::from(g.lfo(slot)?.clock == ir::Clock::Voice));
        }
        if par == EnginePar::LfoUnit {
            let rate = g.lfo(slot)?.rate;
            let units = &self.script()?.lfo_units;
            let same = |u: &Option<ir::Rate>| match (u, rate) {
                (Some(ir::Rate::Hz(_)), ir::Rate::Hz(_)) => true,
                (Some(ir::Rate::Steps { beats: a, .. }), ir::Rate::Steps { beats: b, .. }) => {
                    (a - b).abs() < 1e-4
                }
                _ => false,
            };
            return units
                .iter()
                .position(same)
                .and_then(|n| i32::try_from(n).ok());
        }
        let x = match par {
            EnginePar::ModBypass => {
                let m = g.mods.iter().find(|m| i32::from(m.slot) == slot)?;
                return Some(i32::from(m.bypass));
            }
            EnginePar::ReleaseTrigger => return Some(i32::from(g.release_trigger)),
            EnginePar::RouteIntensity => setting(&SQUARE, f64::from(route()?.abs())),
            EnginePar::RouteDepth => setting(&SIGNED_CUBE, f64::from(route()?)),
            EnginePar::LfoFade => stage_setting(&STAGE_10S, f64::from(g.lfo(slot)?.fade_ms)),
            EnginePar::Attack => stage_setting(&STAGE_15S, ms(e.attack)),
            EnginePar::Hold => stage_setting(&STAGE_15S, ms(e.hold)),
            EnginePar::Decay => stage_setting(&STAGE_25S, ms(e.decay)),
            EnginePar::Release => stage_setting(&STAGE_25S, ms(e.release)),
            EnginePar::Sustain => setting(&CUBE, f64::from(e.sustain)),
            EnginePar::AttackCurve => attack_x(e.attack_shape),
            _ => return get_mod_par(g, par, slot, generic),
        };
        Some((x * 1e6).round() as i32)
    }

    /// Sets the amount of every route named with `#key` to `micros`
    /// millionths ([`kontra_vm::Action::RouteAmount`]). The voices follow
    /// once the callback returns or waits ([`Self::refresh_moved_routes`]):
    /// W Strings sets 6,984 in one `changed`.
    pub(crate) fn set_route_amount(&mut self, key: i32, micros: i32) {
        let v = micros as f32 / 1e6;
        // The groups it moved: a route names one keygroup's group as a rule.
        let (mut one, mut many) = (None, false);
        for (g, m) in self.route_tags.of(key) {
            let Some(m) = self.groups.get_mut(g).and_then(|g| g.mods.get_mut(m)) else {
                continue;
            };
            let mut r = m.route;
            // A pitch route's amount is its range; a scaled or bipolar one
            // (EQ gain, effect, start phase, loop shift) takes none here.
            let set = match r.target {
                ir::ParamAddr::Pitch => {
                    r.span = v;
                    true
                }
                ir::ParamAddr::EqBandGain { .. }
                | ir::ParamAddr::EffectParam { .. }
                | ir::ParamAddr::LoopShift { .. }
                | ir::ParamAddr::LfoParam {
                    param: ir::LfoParam::Phase,
                    ..
                } => false,
                _ if has_depth(&r) => {
                    r.depth = v;
                    true
                }
                _ => false,
            };
            m.route = r;
            if set {
                many |= one.is_some_and(|o| o != g);
                one = Some(g);
            }
        }
        if let Some(g) = one {
            self.routes_moved = Some(match self.routes_moved {
                Some((first, others)) => (first, others || many || first != g),
                None => (g, many),
            });
        }
    }

    /// Refreshes the voices of the groups whose route amounts moved.
    pub(crate) fn refresh_moved_routes(&mut self) {
        match self.routes_moved.take() {
            Some((_, true)) => self.refresh_all(None),
            Some((g, false)) => self.refresh_voices(None, |_, zone| zone.group as usize == g),
            None => {}
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
        let generic = self.send_generic(par, group, generic);
        if self.absent(par, group, slot, generic) {
            return false;
        }
        // A value outside what the control takes is refused, and it keeps
        // what it held (the reference 8.13's setter, [ref]).
        if !self
            .script()
            .is_none_or(|s| s.module_pars.takes(par, value))
        {
            return false;
        }
        let Some(par) = self.engine_par(par) else {
            return false;
        };
        if let EnginePar::Opaque(name) = par {
            return self.hold(name, value, group, slot, generic);
        }
        if let EnginePar::GroupHeld { key, set } = par {
            // A read-only control, or one the group lacks (a source mode
            // without it): the reference 8.13 drops the set and the script
            // runs on (measured 2026-11-25).
            let held = self.group_mut(group).and_then(|g| {
                let at = g.held.iter().position(|h| group_held_at(h, key, slot))?;
                g.held.get_mut(at)
            });
            if let (Some(h), Some(v)) = (held, set.apply(value)) {
                h.2 = v;
            }
            return true;
        }
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
        let needs_unit = matches!(
            par,
            EnginePar::LfoUnit | EnginePar::LfoDelayUnit | EnginePar::GlideCoefUnit
        ) || par.env_unit_index().is_some();
        let unit = needs_unit.then(|| self.lfo_unit(value)).flatten();
        let mode = matches!(par, EnginePar::WarpMode(_))
            .then(|| {
                self.script()?
                    .warp_modes
                    .get(usize::try_from(value).ok()?)
                    .copied()
                    .flatten()
            })
            .flatten();
        let kind = (par == EnginePar::ModOscKind)
            .then(|| {
                self.script()?
                    .mod_kinds
                    .get(usize::try_from(value).ok()?)
                    .copied()
                    .flatten()
            })
            .flatten();
        let tempo = self.tempo;
        let Some(g) = self.group_mut(group) else {
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
            // A route not carried keeps the depth a script writes. A carried
            // kind the engine cannot write (an amplitude envelope) keeps it
            // too, so reads agree, but the write is reported as unhandled.
            EnginePar::RouteIntensity => {
                let saved = uncarried(g, slot, generic).map(|d| {
                    *d = d.signum() * SQUARE.eval_f64(x) as f32;
                });
                match g.mod_mut(slot, generic) {
                    Some(m) => {
                        let mut r = m.route;
                        let sign = signed(&r).unwrap_or(1.0).signum();
                        let set = set_signed(&mut r, sign * SQUARE.eval_f64(x) as f32);
                        m.route = r;
                        set
                    }
                    None => saved.is_some(),
                }
            }
            EnginePar::RouteDepth => {
                let saved =
                    uncarried(g, slot, generic).map(|d| *d = SIGNED_CUBE.eval_f64(x) as f32);
                match g.mod_mut(slot, generic) {
                    Some(m) => {
                        let mut r = m.route;
                        let set = set_signed(&mut r, SIGNED_CUBE.eval_f64(x) as f32);
                        m.route = r;
                        set
                    }
                    None => saved.is_some(),
                }
            }
            // Any value from 1 up restarts it with each voice (Kontakt
            // 8.13.1 reads 0, 1, 1, 1 after 0, 1, 500000, 1000000).
            EnginePar::LfoRetrigger => g.lfo_mut(slot).is_some_and(|l| {
                l.clock = if value >= 1 {
                    ir::Clock::Voice
                } else {
                    ir::Clock::Free
                };
                true
            }),
            EnginePar::Wavetable(v) => g
                .wavetable
                .as_mut()
                .and_then(|r| crate::voice_gain::table_value(r, v))
                .map(|r| *r = x as f32)
                .is_some(),
            EnginePar::InharmonicOn => g
                .wavetable
                .as_mut()
                .map(|t| t.inharmonic.on = value != 0)
                .is_some(),
            EnginePar::ModOscKind => g
                .wavetable
                .as_mut()
                .zip(kind)
                .map(|(t, k)| t.modosc.kind = k)
                .is_some(),
            // A value outside the lists is dropped, like a form mode's.
            EnginePar::TableModWave => g
                .wavetable
                .as_mut()
                .zip(u8::try_from(value).ok().filter(|&w| w <= 8))
                .map(|(t, w)| t.modosc.wave = w)
                .is_some(),
            EnginePar::TableQuality => g
                .wavetable
                .as_mut()
                .zip(ir::Quality::from_index(value))
                .map(|(t, q)| {
                    t.quality = match q {
                        ir::Quality::Lofi => kontra_voice::Quality::Lofi,
                        ir::Quality::Medium => kontra_voice::Quality::Medium,
                        ir::Quality::High => kontra_voice::Quality::High,
                        ir::Quality::Best => kontra_voice::Quality::Best,
                    }
                })
                .is_some(),
            EnginePar::ModOscUnit => g
                .wavetable
                .as_mut()
                .zip(ir::TuneUnit::from_index(value))
                .map(|(t, u)| {
                    t.modosc.unit = match u {
                        ir::TuneUnit::Semitones => kontra_voice::Unit::Semitones,
                        ir::TuneUnit::Ratio => kontra_voice::Unit::Ratio,
                        ir::TuneUnit::Hz => kontra_voice::Unit::Hz,
                    }
                })
                .is_some(),
            EnginePar::WarpMode(n) => g
                .wavetable
                .as_mut()
                .and_then(|t| t.warps.get_mut(usize::from(n)))
                .zip(mode)
                .map(|(w, (shape, shaper))| {
                    w.shape = shape;
                    w.shaper = shaper;
                })
                .is_some(),
            EnginePar::LfoUnit => match (unit, g.lfo_mut(slot)) {
                (Some(unit), Some(l)) => {
                    l.rate = in_unit(l.rate, unit, tempo);
                    true
                }
                _ => false,
            },
            EnginePar::LfoFade => g.lfo_mut(slot).is_some_and(|l| {
                l.fade_ms = STAGE_10S.eval_f64(x) as f32;
                true
            }),
            EnginePar::LfoDelayUnit => {
                let Some(l) = g.lfo_mut(slot) else {
                    return false;
                };
                if let Some(u) = unit {
                    if let Some(ms) = sync_ms(u, tempo) {
                        l.fade_ms = ms.min(10_002.0);
                    }
                    if let Some(sync) = g.mod_sync.slot_mut(slot) {
                        sync.lfo_delay_unit = value as u8;
                    }
                }
                true
            }
            EnginePar::LfoNormalize => g.lfo_mut(slot).is_some_and(|l| {
                if let ir::Wave::Multi { normalize, .. } = &mut l.wave {
                    *normalize = value != 0;
                }
                true
            }),
            EnginePar::GlideCoef => g.glide.as_mut().is_some_and(|gl| {
                gl.seconds = (GLIDE_MS.eval_f64(x) / 1000.0) as f32;
                true
            }),
            EnginePar::GlideCoefUnit => g.glide.as_mut().is_some_and(|gl| {
                if let Some(u) = unit {
                    g.mod_sync.glide_unit = value as u8;
                    if let Some(ms) = sync_ms(u, tempo) {
                        gl.seconds = ms / 1000.0;
                    }
                }
                true
            }),
            EnginePar::IntModType => true,
            EnginePar::IntModSubtype => set_mod_subtype(g, slot, value),
            _ => return self.set_mod_or_envelope(par, value, x, unit, group, slot, generic),
        };
        // A route or LFO belongs to its group: only that group's voices read it.
        if set && let Ok(g) = u32::try_from(group) {
            self.refresh_voices(None, |_, zone| zone.group == g);
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

    /// Sets an envelope, DBD, Flex, StepMod, or sync-unit parameter on `group`/`slot`.
    #[allow(clippy::too_many_arguments)]
    fn set_mod_or_envelope(
        &mut self,
        par: EnginePar,
        value: i32,
        x: f64,
        unit: Option<ir::Rate>,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> bool {
        let (rate, tempo) = (self.rate, self.tempo);
        let Some(g) = self.group_mut(group) else {
            return false;
        };
        if let Some(handled) = set_mod_par(g, par, value, x, unit, rate, tempo, slot, generic) {
            if handled && let Ok(gid) = u32::try_from(group) {
                if par == EnginePar::EnvAhd || par.env_unit_index().is_some() {
                    self.refresh_group_envelopes(gid);
                } else {
                    self.refresh_voices(None, |_, zone| zone.group == gid);
                }
            }
            return handled;
        }
        let mut updated = false;
        let routed = routed_envelope(g, slot).map(|e| e.stages);
        if let (Some((i, stage)), Some(stages)) = (stage_point(par), routed) {
            // Measured (probes/flex_stage_writes.lua): an envelope without
            // stages (a flex) keeps its points; the write is answered.
            if !stages {
                return true;
            }
            if let Some(p) = routed_index(g, slot)
                .and_then(|n| g.envelopes.get_mut(n))
                .and_then(|e| e.points.get_mut(i))
            {
                match par {
                    EnginePar::Sustain => p.level = CUBE.eval_f64(x) as f32,
                    EnginePar::AttackCurve => p.curve = attack_shape(x),
                    _ => p.ms = stage.eval_f64(x) as f32,
                }
                updated = true;
            }
        }
        let amplitude =
            |m: &ir::Mod| i32::from(m.slot) == slot && m.route.target == ir::ParamAddr::AmpEnvelope;
        if !g.mods.iter().any(amplitude) {
            return updated;
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
            _ => return updated,
        }
        if let Ok(group_id) = u32::try_from(group) {
            self.refresh_group_envelopes(group_id);
        }
        true
    }

    fn refresh_group_envelopes(&mut self, group_id: u32) {
        let plan = std::sync::Arc::clone(&self.plan);
        for i in 0..self.voices.len() {
            let target = self.voices.get(i).and_then(|slot| {
                if !slot.voice.is_active() {
                    return None;
                }
                let zone = plan.zones.get(slot.zone)?;
                if zone.group != group_id {
                    return None;
                }
                let n = slot.note.and_then(|note| self.notes.get(note))?;
                Some(self.envelope(zone, n, crate::modulators::Age::at_start(slot.start_frame)))
            });
            if let (Some(env), Some(slot)) = (target, self.voices.get_mut(i)) {
                slot.voice.update_envelope(env);
            }
        }
    }

    /// A group's amplitude envelope in seconds, level and shapes, as a
    /// format's script runtime computes it: attack, hold, decay, sustain,
    /// release, attack shape, fall shape, explicit delay. Its floor and depth (a
    /// volume route's) stay. New voices play it.
    pub(crate) fn set_amp_envelope(&mut self, group: i32, v: [f32; 8]) -> bool {
        let rate = f64::from(self.rate);
        let Some(g) = self.group_mut(group) else {
            return false;
        };
        let frames = |s: f32| (f64::from(s.max(0.0)) * rate).round() as u32;
        let [
            attack,
            hold,
            decay,
            sustain,
            release,
            attack_shape,
            fall_shape,
            delay,
        ] = v;
        let e = &mut g.envelope;
        e.attack = frames(attack);
        e.hold = frames(hold);
        e.decay = frames(decay);
        e.sustain = sustain.clamp(0.0, 1.0);
        e.release = frames(release).max(1);
        e.attack_shape = attack_shape;
        e.decay_shape = fall_shape;
        e.release_shape = fall_shape;
        e.delay = frames(delay);
        e.lag = 0;
        true
    }

    /// Point `point` of breakpoint envelope `index` of a group. New voices
    /// play it.
    pub(crate) fn set_envelope_point(
        &mut self,
        group: i32,
        index: i32,
        point: i32,
        to: ir::Breakpoint,
    ) -> bool {
        let p = (self.group_mut(group))
            .and_then(|g| g.envelopes.get_mut(usize::try_from(index).ok()?))
            .and_then(|e| e.points.get_mut(usize::try_from(point).ok()?));
        let Some(p) = p else {
            return false;
        };
        *p = ir::Breakpoint {
            ms: to.ms.max(0.0),
            level: to.level,
            curve: to.curve,
        };
        true
    }

    /// LFO `index` of a group: each of its wave, rate and clock given.
    pub(crate) fn set_lfo(
        &mut self,
        group: i32,
        index: i32,
        wave: Option<ir::Wave>,
        rate: Option<ir::Rate>,
        clock: Option<ir::Clock>,
    ) -> bool {
        let lfo =
            (self.group_mut(group)).and_then(|g| g.lfos.get_mut(usize::try_from(index).ok()?));
        let Some(lfo) = lfo else {
            return false;
        };
        if let Some(wave) = wave {
            lfo.wave = wave;
        }
        if let Some(rate) =
            rate.filter(|r| matches!(r, ir::Rate::Hz(x) | ir::Rate::Beats(x) if *x > 0.0))
        {
            lfo.rate = rate;
        }
        if let Some(clock) = clock {
            lfo.clock = clock;
        }
        true
    }

    /// `set_controller`: the script moves a controller (0 to 127), the
    /// pitch bend (`$VCC_PITCH_BEND`, -8192 to 8191) or the channel pressure
    /// (`$VCC_MONO_AT`) as if it had arrived, on every port and channel.
    pub(crate) fn set_controller(&mut self, controller: i32, value: i32) {
        let Ok(cc) = u8::try_from(controller) else {
            return;
        };
        if self.running.is(Kind::Controller) && self.running.controller() == cc {
            self.running.set_own = true;
        }
        if cc == crate::midi::VCC_PITCH_BEND {
            let bend = value.clamp(-8192, 8191) as f32 / 8192.0;
            for channel in (0u8..).take(kontra_behaviour::CHANNELS) {
                self.behaviour.set_bend(channel, bend);
            }
            self.refresh_all(None);
            return;
        }
        let Ok(value) = u8::try_from(value.clamp(0, 127)) else {
            return;
        };
        for ch in self.cc_values.iter_mut() {
            if let Some(v) = ch.get_mut(usize::from(cc)) {
                *v = value;
            }
        }
        // A control learned to this controller moves as it would for MIDI in
        // (Conflux selects a sample layer by sending CC 122, which its MIDI
        // automation ties to the switch whose callback unpurges that layer).
        for (slot, script) in (0u8..).zip(&self.plan.scripts) {
            for link in script.links.iter().filter(|l| l.cc == cc) {
                if self.linked.len() < self.linked.capacity() {
                    self.linked.push_back((slot, link.id, link.value(value)));
                } else {
                    self.stats.events_refused += 1;
                }
            }
        }
        if cc == crate::midi::VCC_MONO_AT {
            for channel in (0u8..).take(kontra_behaviour::CHANNELS) {
                self.behaviour.set_pressure(channel, value);
            }
            self.refresh_all(None);
        } else if cc < 128 {
            for channel in (0u8..).take(kontra_behaviour::CHANNELS) {
                self.behaviour.set_cc(channel, cc, value);
            }
            self.refresh_all(Some(cc));
        }
    }

    /// Re-reads gain (and pitch, unless `cc` moved) of every sounding note.
    pub(crate) fn refresh_all(&mut self, cc: Option<u8>) {
        self.refresh_voices(cc, |_, _| true);
    }
}

/// The saved depth of `group`'s route at `slot`, target `target`, when the
/// engine does not carry it.
fn uncarried(g: &mut Group, slot: i32, target: i32) -> Option<&mut f32> {
    let r = g.routes.iter_mut().find(|r| i32::from(r.slot) == slot)?;
    r.depths.get_mut(usize::try_from(target.max(0)).ok()?)
}

impl Engine {
    /// The rate kind unit value `value` picks, from the script's table.
    fn lfo_unit(&self, value: i32) -> Option<ir::Rate> {
        let units = &self.script()?.lfo_units;
        units.get(usize::try_from(value).ok()?).copied().flatten()
    }
}

/// `rate` moved to `unit`'s kind: free Hz keeps its Hz, a note value keeps
/// its count; between kinds the period carries over at `tempo` BPM.
fn in_unit(rate: ir::Rate, unit: ir::Rate, tempo: f32) -> ir::Rate {
    let sec_per_beat = 60.0 / tempo.clamp(1.0, 999.0);
    let seconds = match rate {
        ir::Rate::Hz(hz) => 1.0 / hz.max(1e-3),
        ir::Rate::Beats(beats) => beats * sec_per_beat,
        ir::Rate::Steps { beats, count } => beats * count * sec_per_beat,
    };
    match (unit, rate) {
        (ir::Rate::Hz(_), ir::Rate::Hz(_)) => rate,
        (ir::Rate::Hz(_), _) => ir::Rate::Hz(1.0 / seconds),
        (ir::Rate::Steps { beats, .. }, ir::Rate::Steps { count, .. }) => {
            ir::Rate::Steps { beats, count }
        }
        (ir::Rate::Steps { beats, .. } | ir::Rate::Beats(beats), _) => ir::Rate::Steps {
            beats,
            count: (seconds / (beats * sec_per_beat)).round().max(1.0),
        },
    }
}

/// The rack and slot a held control lives in.
/// Whether held group control `h` is `key` at index `slot` (−1 in `h`:
/// the whole group, any index).
fn group_held_at(h: &(i8, u32, i32), key: u32, slot: i32) -> bool {
    h.1 == key && (h.0 == -1 || i32::from(h.0) == slot)
}

/// Group control `key` at index `slot`; none reads 0 (the reference 8.13
/// reads 0 at a start criterion index it lacks, e.g. −1).
fn group_held(held: &[(i8, u32, i32)], key: u32, slot: i32) -> Option<&(i8, u32, i32)> {
    held.iter().find(|h| group_held_at(h, key, slot))
}

fn slot_at(group: i32, slot: i32, generic: i32) -> Option<(ir::RackAt, u8)> {
    Some((
        crate::knobs::rack_at(group, generic)?,
        u8::try_from(slot).ok()?,
    ))
}

/// A placed effect's `(group, generic)`: 0 is group `index`'s inserts, 1 bus
/// `index`'s rack, 2 the instrument's inserts.
pub(crate) fn placed(at: i32, index: i32) -> (i32, i32) {
    match at {
        0 => (index, -1),
        1 => (-1, (BUS_OFFSET - 1).saturating_add(index)),
        _ => (-1, 1),
    }
}

/// Duration in milliseconds of a synced [`ir::Rate`] at `tempo` BPM; `None` for free `Hz`.
fn sync_ms(unit: ir::Rate, tempo: f32) -> Option<f32> {
    let ms_per_beat = 60_000.0 / tempo.clamp(1.0, 999.0);
    match unit {
        ir::Rate::Steps { beats, count } => Some(beats * count * ms_per_beat),
        ir::Rate::Beats(beats) => Some(beats * ms_per_beat),
        ir::Rate::Hz(_) => None,
    }
}

/// Exact inverse of `FormulaId::FlexShape` (`ln(((1/f) - 1)^2)`).
fn flex_slope_x(k: f32) -> f64 {
    let f = 1.0 / (1.0 + (f64::from(k) * 0.5).exp());
    if f <= 0.5 {
        (f * 0.5).sqrt()
    } else {
        1.0 - ((1.0 - f) * 0.5).sqrt()
    }
}

fn is_amp_slot(g: &Group, slot: i32) -> bool {
    g.mods
        .iter()
        .any(|m| i32::from(m.slot) == slot && m.route.target == ir::ParamAddr::AmpEnvelope)
}

fn step_signed(g: &Group, slot: i32) -> bool {
    !g.mods
        .iter()
        .any(|m| i32::from(m.slot) == slot && m.route.target == ir::ParamAddr::Volume)
}

/// Reads the modulator engine parameters backed by live group state.
fn get_mod_par(g: &Group, par: EnginePar, slot: i32, generic: i32) -> Option<i32> {
    if let Some(idx) = par.env_unit_index() {
        return Some(i32::from(*g.mod_sync.slot(slot)?.env_units.get(idx)?));
    }
    match par {
        EnginePar::IntModType => Some(g.mod_type_and_subtype(slot).0),
        EnginePar::IntModSubtype => Some(g.mod_type_and_subtype(slot).1),
        EnginePar::LfoDelayUnit => Some(i32::from(g.mod_sync.slot(slot)?.lfo_delay_unit)),
        EnginePar::LfoNormalize => Some(match g.lfo(slot)?.wave {
            ir::Wave::Multi { normalize, .. } => i32::from(normalize),
            _ => 0,
        }),
        EnginePar::GlideCoef => Some(
            (stage_setting(&GLIDE_MS, f64::from(g.glide?.seconds * 1000.0)) * 1e6).round() as i32,
        ),
        EnginePar::GlideCoefUnit => g.glide.map(|_| i32::from(g.mod_sync.glide_unit)),
        EnginePar::EnvAhd => Some(if is_amp_slot(g, slot) {
            i32::from(g.envelope.one_shot)
        } else {
            routed_envelope(g, slot).map_or(0, |e| i32::from(e.stages && e.sustain.is_none()))
        }),
        EnginePar::Decay1 | EnginePar::BreakLevel | EnginePar::Decay2 | EnginePar::DbdEasy => {
            let sync = g.mod_sync.slot(slot)?;
            let e = routed_envelope(g, slot)?;
            if !is_dbd(e, sync.dbd_easy) {
                return Some(0);
            }
            if par == EnginePar::DbdEasy {
                return Some(i32::from(sync.dbd_easy));
            }
            let [d1, brk, d2] = if sync.dbd_easy {
                sync.dbd_saved
            } else if let [p0, p1] = e.points.as_slice() {
                [p0.ms, p0.level, p1.ms]
            } else {
                return Some(0);
            };
            let x = match par {
                EnginePar::Decay1 => stage_setting(&STAGE_25S, f64::from(d1)),
                EnginePar::BreakLevel => (f64::from(brk) / 3.0 + 1.0) * 0.5,
                _ => stage_setting(&STAGE_25S, f64::from(d2)),
            };
            Some((x.clamp(0.0, 1.0) * 1e6).round() as i32)
        }
        EnginePar::FlexNumStages
        | EnginePar::FlexOneshot
        | EnginePar::FlexLoop
        | EnginePar::FlexLoopStart
        | EnginePar::FlexLoopEnd
        | EnginePar::FlexStageTime
        | EnginePar::FlexStageLevel
        | EnginePar::FlexStageSlope => {
            let sync = g.mod_sync.slot(slot)?;
            let e = routed_envelope(g, slot)?;
            if e.stages || is_dbd(e, sync.dbd_easy) {
                return Some(0);
            }
            let pt = usize::try_from(generic).ok().and_then(|i| e.points.get(i));
            Some(match par {
                EnginePar::FlexNumStages => e.points.len() as i32,
                EnginePar::FlexOneshot => i32::from(e.sustain.is_none()),
                EnginePar::FlexLoop => i32::from(sync.flex_loop || e.loop_from.is_some()),
                EnginePar::FlexLoopStart => {
                    e.loop_from
                        .unwrap_or_else(|| usize::from(sync.flex_loop_start))
                        as i32
                        + 1
                }
                EnginePar::FlexLoopEnd => {
                    e.sustain.unwrap_or_else(|| usize::from(sync.flex_sustain)) as i32 + 1
                }
                EnginePar::FlexStageTime => {
                    pt.map_or(0, |p| (f64::from(p.ms) * 1000.0).round() as i32)
                }
                EnginePar::FlexStageLevel => {
                    pt.map_or(0, |p| (f64::from(p.level) * 1e6).round() as i32)
                }
                _ => pt.map_or(0, |p| (flex_slope_x(p.curve) * 1e6).round() as i32),
            })
        }
        EnginePar::StepSeqNumSteps | EnginePar::StepSeqOneshot | EnginePar::StepSeqStepValue => {
            let sync = g.mod_sync.slot(slot)?;
            let (_, shaper_idx) = g.step_mod_indices(slot)?;
            let t = g.shapers.get(shaper_idx)?;
            let n = t.table.len().saturating_sub(1);
            Some(match par {
                EnginePar::StepSeqNumSteps => n as i32,
                EnginePar::StepSeqOneshot => i32::from(sync.step_oneshot),
                _ => usize::try_from(generic)
                    .ok()
                    .filter(|&i| i < n)
                    .and_then(|i| t.table.get(i))
                    .map_or(0, |&v| {
                        let u = if step_signed(g, slot) {
                            (f64::from(v) + 1.0) * 0.5
                        } else {
                            f64::from(v)
                        };
                        (u.clamp(0.0, 1.0) * 1e6).round() as i32
                    }),
            })
        }
        _ => None,
    }
}

/// Applies modulator parameters handled in `set_mod_or_envelope`; `None` falls through to AHDSR stages.
#[allow(clippy::too_many_arguments)]
fn set_mod_par(
    g: &mut Group,
    par: EnginePar,
    value: i32,
    x: f64,
    unit: Option<ir::Rate>,
    rate: u32,
    tempo: f32,
    slot: i32,
    generic: i32,
) -> Option<bool> {
    if let Some(idx) = par.env_unit_index() {
        let sync = g.mod_sync.slot_mut(slot)?;
        if let Some(u) = unit {
            if let Some(cell) = sync.env_units.get_mut(idx) {
                *cell = value as u8;
            }
            let easy = sync.dbd_easy;
            if let Some(ms) = sync_ms(u, tempo) {
                let clamped = ms.min(if idx <= 1 { 15_000.0 } else { 25_000.0 });
                if is_amp_slot(g, slot) {
                    let frames = (f64::from(clamped) * f64::from(rate) / 1000.0).round() as u32;
                    match idx {
                        0 => g.envelope.attack = frames,
                        1 => g.envelope.hold = frames,
                        2 => g.envelope.decay = frames,
                        _ => g.envelope.release = frames,
                    }
                }
                if let Some(e) = routed_index(g, slot).and_then(|n| g.envelopes.get_mut(n)) {
                    let pt = if e.stages {
                        Some(idx)
                    } else if is_dbd(e, easy) {
                        match par {
                            EnginePar::Decay1Unit => Some(0),
                            EnginePar::Decay2Unit => Some(1),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    if let Some(p) = pt.and_then(|i| e.points.get_mut(i)) {
                        p.ms = clamped;
                    }
                }
            }
        }
        return Some(true);
    }
    match par {
        EnginePar::EnvAhd => {
            let on = value != 0;
            let mut any = false;
            if is_amp_slot(g, slot) {
                g.envelope.one_shot = on;
                any = true;
            }
            if let Some(e) = routed_index(g, slot).and_then(|n| g.envelopes.get_mut(n))
                && e.stages
            {
                e.sustain = (!on).then_some(2);
                any = true;
            }
            Some(any)
        }
        EnginePar::Decay1 | EnginePar::BreakLevel | EnginePar::Decay2 | EnginePar::DbdEasy => {
            let idx = routed_index(g, slot)?;
            let sync = g.mod_sync.slot_mut(slot)?;
            let e = g.envelopes.get_mut(idx)?;
            if !is_dbd(e, sync.dbd_easy) {
                return Some(true);
            }
            if par == EnginePar::DbdEasy {
                let on = value != 0;
                if on
                    && !sync.dbd_easy
                    && let [p0, p1] = e.points.as_slice()
                {
                    sync.dbd_saved = [p0.ms, p0.level, p1.ms];
                    sync.dbd_easy = true;
                    e.start = 0.0;
                    e.points.clear();
                } else if !on && sync.dbd_easy && e.points.capacity() >= 2 {
                    let [d1, brk, d2] = sync.dbd_saved;
                    sync.dbd_easy = false;
                    e.start = 1.0;
                    e.points.clear();
                    e.points.extend([
                        ir::Breakpoint {
                            ms: d1,
                            level: brk,
                            curve: 0.0,
                        },
                        ir::Breakpoint {
                            ms: d2,
                            level: 0.0,
                            curve: 0.0,
                        },
                    ]);
                }
                return Some(true);
            }
            // Negative writes to DECAY1, BREAK, DECAY2 are ignored (value unchanged).
            if value < 0 {
                return Some(true);
            }
            let (field, val) = match par {
                EnginePar::Decay1 => (0, STAGE_25S.eval_f64(x) as f32),
                EnginePar::BreakLevel => (1, ((2.0 * x - 1.0) * 3.0) as f32),
                _ => (2, STAGE_25S.eval_f64(x) as f32),
            };
            if sync.dbd_easy {
                if let Some(cell) = sync.dbd_saved.get_mut(field) {
                    *cell = val;
                }
            } else if let [p0, p1] = e.points.as_mut_slice() {
                match field {
                    0 => p0.ms = val,
                    1 => p0.level = val,
                    _ => p1.ms = val,
                }
            }
            Some(true)
        }
        EnginePar::FlexNumStages
        | EnginePar::FlexOneshot
        | EnginePar::FlexLoop
        | EnginePar::FlexLoopStart
        | EnginePar::FlexLoopEnd
        | EnginePar::FlexStageTime
        | EnginePar::FlexStageLevel
        | EnginePar::FlexStageSlope => {
            let idx = routed_index(g, slot)?;
            let sync = g.mod_sync.slot_mut(slot)?;
            let e = g.envelopes.get_mut(idx)?;
            if e.stages || is_dbd(e, sync.dbd_easy) {
                return Some(true);
            }
            match par {
                EnginePar::FlexNumStages => {
                    let n = usize::try_from(value)
                        .ok()?
                        .clamp(3, 31)
                        .min(e.points.capacity());
                    e.points.resize(
                        n,
                        ir::Breakpoint {
                            ms: 100.0,
                            level: 0.0,
                            curve: 0.0,
                        },
                    );
                    let max_idx = e.points.len().saturating_sub(1);
                    let sus = usize::from(sync.flex_sustain).min(max_idx);
                    let ls = usize::from(sync.flex_loop_start).min(sus);
                    (sync.flex_sustain, sync.flex_loop_start) = (sus as u8, ls as u8);
                    if e.sustain.is_some() {
                        e.sustain = Some(sus);
                        e.loop_from = sync.flex_loop.then_some(ls);
                    }
                }
                EnginePar::FlexOneshot => {
                    if value != 0 {
                        if let Some(sus) = e.sustain.take() {
                            sync.flex_sustain = sus as u8;
                        }
                        if let Some(ls) = e.loop_from.take() {
                            sync.flex_loop_start = ls as u8;
                            sync.flex_loop = true;
                        }
                    } else {
                        let sus =
                            usize::from(sync.flex_sustain).min(e.points.len().saturating_sub(1));
                        e.sustain = Some(sus);
                        e.loop_from = sync
                            .flex_loop
                            .then(|| usize::from(sync.flex_loop_start).min(sus));
                    }
                }
                EnginePar::FlexLoop => {
                    sync.flex_loop = value != 0;
                    if let Some(sus) = e.sustain {
                        e.loop_from = sync
                            .flex_loop
                            .then(|| usize::from(sync.flex_loop_start).min(sus));
                    }
                }
                EnginePar::FlexLoopStart => {
                    let ls = usize::try_from(value - 1).ok()?;
                    let le = e.sustain.unwrap_or_else(|| usize::from(sync.flex_sustain));
                    if ls < e.points.len() && ls < le {
                        sync.flex_loop_start = ls as u8;
                        if e.loop_from.is_some() {
                            e.loop_from = Some(ls);
                        }
                    }
                }
                EnginePar::FlexLoopEnd => {
                    let le = usize::try_from(value - 1).ok()?;
                    let ls = e
                        .loop_from
                        .unwrap_or_else(|| usize::from(sync.flex_loop_start));
                    if le < e.points.len() && le > ls {
                        sync.flex_sustain = le as u8;
                        if e.sustain.is_some() {
                            e.sustain = Some(le);
                        }
                    }
                }
                _ => {
                    let len = e.points.len();
                    if let Some(i) = usize::try_from(generic).ok()
                        && let Some(p) = e.points.get_mut(i)
                    {
                        match par {
                            EnginePar::FlexStageTime => {
                                p.ms = (f64::from(value.max(1_000)) / 1000.0) as f32;
                            }
                            EnginePar::FlexStageLevel => {
                                p.level = x as f32;
                                if i + 1 == len {
                                    e.start = p.level;
                                }
                            }
                            _ => {
                                p.curve = ir::Law::Formula(ir::FormulaId::FlexShape)
                                    .eval_unclamped(x as f32);
                            }
                        }
                    }
                }
            }
            Some(true)
        }
        EnginePar::StepSeqNumSteps | EnginePar::StepSeqOneshot | EnginePar::StepSeqStepValue => {
            let (lfo_idx, shaper_idx) = g.step_mod_indices(slot)?;
            let signed = step_signed(g, slot);
            let sync = g.mod_sync.slot_mut(slot)?;
            let t = g.shapers.get_mut(shaper_idx)?;
            match par {
                EnginePar::StepSeqNumSteps => {
                    let n = usize::try_from(value).ok()?.clamp(2, 32);
                    let old_n = t.table.len().saturating_sub(1).max(1);
                    if n != old_n && n < t.table.capacity() {
                        if n > old_n
                            && let Some(last) = t.table.get_mut(old_n)
                        {
                            *last = 0.0;
                        }
                        t.table.resize(n + 1, 0.0);
                        if let Some(&prev) = t.table.get(n - 1)
                            && let Some(sentinel) = t.table.get_mut(n)
                        {
                            *sentinel = prev;
                        }
                        if let Some(l) = g.lfos.get_mut(lfo_idx) {
                            let ratio = old_n as f32 / n as f32;
                            l.rate = match l.rate {
                                ir::Rate::Hz(f) => ir::Rate::Hz(f * ratio),
                                ir::Rate::Beats(b) => ir::Rate::Beats(b / ratio),
                                ir::Rate::Steps { beats, count } => ir::Rate::Steps {
                                    beats: beats / ratio,
                                    count,
                                },
                            };
                        }
                    }
                }
                EnginePar::StepSeqOneshot => {
                    sync.step_oneshot = value != 0;
                    if sync.step_oneshot
                        && let Some(l) = g.lfos.get_mut(lfo_idx)
                    {
                        l.clock = ir::Clock::Voice;
                    }
                }
                _ => {
                    let n = t.table.len().saturating_sub(1);
                    if let Some(i) = usize::try_from(generic).ok().filter(|&i| i < n) {
                        let val = if signed {
                            (2.0 * x - 1.0) as f32
                        } else {
                            x as f32
                        };
                        if let Some(cell) = t.table.get_mut(i) {
                            *cell = val;
                        }
                        if i + 1 == n
                            && let Some(sentinel) = t.table.get_mut(n)
                        {
                            *sentinel = val;
                        }
                    }
                }
            }
            Some(true)
        }
        _ => None,
    }
}

/// Switches an LFO's waveform (`0..=5`) or routed envelope's subtype (`0` AHDSR / `1` Flex / `2` DBD)
/// in place without allocating on the audio thread.
fn set_mod_subtype(g: &mut Group, slot: i32, value: i32) -> bool {
    if let Some(l) = g.lfo_mut(slot) {
        let width = match l.wave {
            ir::Wave::Square { width } | ir::Wave::Multi { width, .. } => width,
            _ => 0.5,
        };
        l.wave = match value {
            0 => ir::Wave::Sine,
            1 => ir::Wave::Square { width },
            2 => ir::Wave::Triangle,
            3 => ir::Wave::Saw,
            4 => ir::Wave::Random,
            5 => match l.wave {
                w @ ir::Wave::Multi { .. } => w,
                _ => ir::Wave::Multi {
                    weights: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    width,
                    normalize: false,
                },
            },
            _ => return true,
        };
        return true;
    }
    let Some(idx) = routed_index(g, slot) else {
        return is_amp_slot(g, slot);
    };
    let (Some(sync), Some(e)) = (g.mod_sync.slot_mut(slot), g.envelopes.get_mut(idx)) else {
        return false;
    };
    const PT: fn(f32, f32) -> ir::Breakpoint = |ms, level| ir::Breakpoint {
        ms,
        level,
        curve: 0.0,
    };
    let (stages, start, sustain, pts): (bool, f32, Option<usize>, &[ir::Breakpoint]) = match value {
        0 => (
            true,
            0.0,
            Some(2),
            &[PT(0.0, 1.0), PT(0.0, 1.0), PT(250.0, 1.0), PT(100.0, 0.0)],
        ),
        1 => (
            false,
            0.0,
            Some(1),
            &[PT(100.0, 1.0), PT(100.0, 0.5), PT(100.0, 0.0)],
        ),
        2 => (false, 1.0, None, &[PT(250.0, 0.0), PT(500.0, 0.0)]),
        _ => return true,
    };
    if e.points.capacity() >= pts.len() {
        sync.dbd_easy = false;
        (e.stages, e.start, e.sustain, e.loop_from) = (stages, start, sustain, None);
        e.points.clear();
        e.points.extend_from_slice(pts);
        if value == 1 {
            (sync.flex_sustain, sync.flex_loop_start, sync.flex_loop) = (1, 0, false);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use kontra_ir as ir;

    use super::in_unit;

    #[test]
    fn kt13f_lfo_unit_conversion_uses_host_tempo_not_hardcoded_120_bpm() {
        // At 60 BPM, 1 beat = 1.0 s => 1.0 Hz (120 BPM would give 2.0 Hz).
        assert_eq!(
            in_unit(ir::Rate::Beats(1.0), ir::Rate::Hz(1.0), 60.0),
            ir::Rate::Hz(1.0)
        );
        // Converting 1.0 Hz back to quarter-note beats at 60 BPM gives count = 1.0
        // (120 BPM would give count = 2.0).
        assert_eq!(
            in_unit(ir::Rate::Hz(1.0), ir::Rate::Beats(1.0), 60.0),
            ir::Rate::Steps {
                beats: 1.0,
                count: 1.0,
            }
        );
        // At 240 BPM, 1 beat = 0.25 s => 4.0 Hz, and 1.0 Hz = 4 quarter notes.
        assert_eq!(
            in_unit(ir::Rate::Beats(1.0), ir::Rate::Hz(1.0), 240.0),
            ir::Rate::Hz(4.0)
        );
        assert_eq!(
            in_unit(ir::Rate::Hz(1.0), ir::Rate::Beats(1.0), 240.0),
            ir::Rate::Steps {
                beats: 1.0,
                count: 4.0,
            }
        );
    }
}
