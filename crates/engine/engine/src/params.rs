//! What a script changes in the engine while it plays: modulation intensities,
//! envelope stages and controller values. Values arrive in KSP's 0..=1_000_000
//! encoding; the laws are Kontakt's (sampler-core `EngineParameterLaw`).

use kontra_ir as ir;

use crate::plan::{Group, Holder, addressed};
use crate::{Engine, EnginePar, Level};
use kontra_plan::par_laws::{
    CUBE, SIGNED_CUBE, SQUARE, STAGE_10S, STAGE_15S, STAGE_25S, attack_shape, attack_x,
    routed_envelope, routed_index, setting, stage_point, stage_setting,
};
use kontra_plan::route_depth::{depth, set_signed, signed};

pub(crate) use kontra_plan::par_values::{BUS_OFFSET, LevelPar, effect_par, level_par};

impl Engine {
    /// What the running script's parameter id `id` changes.
    pub(crate) fn engine_par(&self, id: i32) -> Option<EnginePar> {
        self.script()?.engine_pars.get(id)
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
        let owner = self.script().and_then(|s| s.engine_pars.owner(par));
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

    /// A group's insert slot (generic is ignored), the instrument insert's (group
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
            // A group's slot: `<generic>` is for instrument effects only (KSP manual).
            (Ok(g), _) => &mut self.groups.get_mut(g)?.inserts[..],
            // `$NI_INSERT_BUS` 1, `$NI_SEND_BUS` 0 (one rack per slot), `$NI_MAIN_BUS` 2.
            // Kontakt takes generic -1 as the insert rack too (Una Corda's
            // transient and stereo knobs; measured, controls.py 2026-11-12).
            (Err(_), 1 | -1) => &mut self.rack.slots[..],
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
        let (group, generic) = match at {
            0 => (index, -1),
            1 => (-1, (BUS_OFFSET - 1).saturating_add(index)),
            _ => (-1, 1),
        };
        self.effect_mut(group, slot, generic)
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
        if par == EnginePar::OutputChannel {
            return self.output_channel(group, generic);
        }
        if let EnginePar::Knob(n) = par {
            return self.knob(n, group, slot, generic);
        }
        if par == EnginePar::EffectSubtype {
            return self.subtype(group, slot, generic);
        }
        if par == EnginePar::EffectType {
            return self.effect_type(group, slot, generic);
        }
        if let EnginePar::Reverb(which) = par {
            let ir::EffectKind::Reverb(reverb) = self.effect_mut(group, slot, generic)?.kind else {
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
            let e = *self.effect_mut(group, slot, generic)?;
            return Some(which.get(&e)?.round().clamp(0.0, 1e6) as i32);
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
            carried.and_then(|m| signed(&m.kind)).or_else(|| {
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
            _ => return None,
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
        for (g, m) in self.route_names.of(key) {
            let Some(m) = self.groups.get_mut(g).and_then(|g| g.mods.get_mut(m)) else {
                continue;
            };
            let set = match &mut m.kind {
                ir::ModKind::Pitch { semitones, .. } => {
                    *semitones = v;
                    true
                }
                kind => depth(kind).map(|d| *d = v).is_some(),
            };
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
        let unit = (par == EnginePar::LfoUnit)
            .then(|| self.lfo_unit(value))
            .flatten();
        let mode = matches!(par, EnginePar::WarpMode(_))
            .then(|| {
                self.script()?
                    .warp_modes
                    .get(usize::try_from(value).ok()?)
                    .copied()
                    .flatten()
            })
            .flatten();
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
                        let sign = signed(&m.kind).unwrap_or(1.0).signum();
                        set_signed(&mut m.kind, sign * SQUARE.eval_f64(x) as f32)
                    }
                    None => saved.is_some(),
                }
            }
            EnginePar::RouteDepth => {
                let saved =
                    uncarried(g, slot, generic).map(|d| *d = SIGNED_CUBE.eval_f64(x) as f32);
                match g.mod_mut(slot, generic) {
                    Some(m) => set_signed(&mut m.kind, SIGNED_CUBE.eval_f64(x) as f32),
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
                    l.rate = in_unit(l.rate, unit);
                    true
                }
                _ => false,
            },
            EnginePar::LfoFade => g.lfo_mut(slot).is_some_and(|l| {
                l.fade_ms = STAGE_10S.eval_f64(x) as f32;
                true
            }),
            _ => return self.set_envelope(par, x, group, slot),
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

    /// Sets a stage of the envelope at modulator `slot`: the group's
    /// amplitude envelope, or breakpoints with an AHDSR's shape. Script
    /// values are knobs: times through the stage law, sustain level x³
    /// (docs/re/engine/k-mod-envelopes.md, reader 0x140cffb40).
    fn set_envelope(&mut self, par: EnginePar, x: f64, group: i32, slot: i32) -> bool {
        let rate = self.rate;
        let Some(g) = self.group_mut(group) else {
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
/// its count; between kinds the period carries over at 120 BPM.
// ponytail: the period converts at a fixed 120 BPM, not the host tempo; the
// script sets the rate right after its unit, so the guess is not heard.
fn in_unit(rate: ir::Rate, unit: ir::Rate) -> ir::Rate {
    let seconds = match rate {
        ir::Rate::Hz(hz) => 1.0 / hz.max(1e-3),
        ir::Rate::Beats(beats) => beats * 0.5,
        ir::Rate::Steps { beats, count } => beats * count * 0.5,
    };
    match (unit, rate) {
        (ir::Rate::Hz(_), ir::Rate::Hz(_)) => rate,
        (ir::Rate::Hz(_), _) => ir::Rate::Hz(1.0 / seconds),
        (ir::Rate::Steps { beats, .. }, ir::Rate::Steps { count, .. }) => {
            ir::Rate::Steps { beats, count }
        }
        (ir::Rate::Steps { beats, .. } | ir::Rate::Beats(beats), _) => ir::Rate::Steps {
            beats,
            count: (seconds / (beats * 0.5)).round().max(1.0),
        },
    }
}
