//! Script-owned voice parameters: a script's per-note volume, pan, tune and
//! fades (KSP `change_vol`, `fade_out`, ...) and its per-group or
//! instrument-wide volume, pan, tune and mute (`set_engine_par`,
//! `purge_group`). Writes apply at the instruction's sample time; voices ramp
//! gain changes across their next render chunk. The targets reuse
//! [`ModTarget`]'s laws so a script layer composes with per-voice modulation.
//!
//! Group layers hold absolute values that start at the authored
//! [`GroupParams`]; the authored values are already baked into each region,
//! so voices apply only the difference.
use crate::{Error, ModTarget, NoteId, Prepared, Runtime};

/// A group's authored volume, pan and tune. Scripts read these back and set
/// them absolutely (KSP `get_engine_par`/`set_engine_par`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GroupParams {
    pub decibels: f64,
    /// -1 (left) ..= 1 (right).
    pub pan: f64,
    pub semitones: f64,
}

impl Prepared {
    /// Initial entries and capacity of the store all script instances share
    /// (`Op::SharedStore`).
    pub fn with_shared_store(
        mut self,
        entries: Vec<([i32; crate::STORE_KEY], i64)>,
        capacity: usize,
    ) -> Result<Self, Error> {
        crate::ops::Store::new(entries.clone(), capacity)?;
        self.shared_store = (entries.into_boxed_slice(), capacity);
        Ok(self)
    }

    /// One [`GroupParams`] per group of [`Prepared::with_groups`].
    pub fn with_group_params(mut self, params: Vec<GroupParams>) -> Result<Self, Error> {
        if params.len() != self.group_count as usize
            || params
                .iter()
                .any(|p| !(p.decibels.is_finite() && p.pan.is_finite() && p.semitones.is_finite()))
        {
            return Err(Error::InvalidInput);
        }
        self.group_params = params.into_boxed_slice();
        Ok(self)
    }
}

/// Which layer a [`crate::Instruction::WriteParam`] edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamScope {
    /// A plan-scoped source event ID (unknown or retired IDs are no-ops).
    Note,
    /// A group index of the callback's plan; negative selects the instrument.
    Group,
}

/// One layer's offsets. Integer units at the instruction boundary:
/// Decibels in millidecibels, Pan in -1000..=1000, Pitch in millicents,
/// Attenuate as a 0..=1000 gain factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Layer {
    decibels: f64,
    pan: f64,
    pitch: f64,
    attenuate: f64,
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            decibels: 0.0,
            pan: 0.0,
            pitch: 0.0,
            attenuate: 1.0,
        }
    }
}

impl Layer {
    pub fn write(&mut self, target: ModTarget, value: i64, relative: bool) -> Result<(), Error> {
        let v = value as f64;
        let (slot, value, bounds) = match target {
            ModTarget::Decibels => (&mut self.decibels, v / 1000.0, None),
            ModTarget::Pan => (&mut self.pan, v / 1000.0, Some((-1.0, 1.0))),
            ModTarget::Pitch => (&mut self.pitch, v / 100_000.0, None),
            ModTarget::Attenuate => (&mut self.attenuate, v / 1000.0, Some((0.0, 1.0))),
            _ => return Err(Error::InvalidInput),
        };
        let next = if relative { *slot + value } else { value };
        *slot = bounds.map_or(next, |(lo, hi)| next.clamp(lo, hi));
        Ok(())
    }

    pub fn read(&self, target: ModTarget) -> i64 {
        (match target {
            ModTarget::Decibels => self.decibels * 1000.0,
            ModTarget::Pan => self.pan * 1000.0,
            ModTarget::Pitch => self.pitch * 100_000.0,
            ModTarget::Attenuate => self.attenuate * 1000.0,
            _ => 0.0,
        })
        .round() as i64
    }

    fn authored(p: GroupParams) -> Self {
        Self {
            decibels: p.decibels,
            pan: p.pan.clamp(-1.0, 1.0),
            pitch: p.semitones,
            attenuate: 1.0,
        }
    }

    /// This layer relative to `base` (attenuation is never authored).
    fn since(self, base: Self) -> Self {
        Self {
            decibels: self.decibels - base.decibels,
            pan: self.pan - base.pan,
            pitch: self.pitch - base.pitch,
            attenuate: self.attenuate,
        }
    }

    pub fn stack(self, other: Self) -> Self {
        Self {
            decibels: self.decibels + other.decibels,
            pan: (self.pan + other.pan).clamp(-1.0, 1.0),
            pitch: self.pitch + other.pitch,
            attenuate: self.attenuate * other.attenuate,
        }
    }

    pub fn semitones(self) -> f64 {
        self.pitch
    }

    /// Stereo gains under the balance law [`ModTarget::Pan`] uses, times `fade`.
    pub fn gains(self, fade: f64) -> [f32; 2] {
        let gain = (self.attenuate * fade * 10f64.powf(self.decibels / 20.0)).max(0.0);
        [
            (gain * (1.0 - self.pan.max(0.0))) as f32,
            (gain * (1.0 + self.pan.min(0.0))) as f32,
        ]
    }
}

/// A note's linear fade from `from` to `to` over `frames` from `start`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fade {
    from: f64,
    to: f64,
    start: u64,
    frames: u64,
    /// End the note's voices once the fade is complete.
    pub stop: bool,
}

impl Fade {
    pub fn at(&self, now: u64) -> f64 {
        let elapsed = now.saturating_sub(self.start);
        if elapsed >= self.frames {
            self.to
        } else {
            self.from + (self.to - self.from) * elapsed as f64 / self.frames as f64
        }
    }

    pub fn done(&self, now: u64) -> bool {
        now >= self.start.saturating_add(self.frames)
    }
}

/// A group amplitude envelope stage a script sets (KSP `set_engine_par`
/// `$ENGINE_PAR_ATTACK`, ...). Applies to voices that start afterwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvelopeStage {
    Attack,
    Hold,
    Decay,
    /// Level as a 0..=1000 gain factor.
    Sustain,
    Release,
}

/// Per-plan-generation script engine state: group and instrument layers,
/// envelope stages and the store shared by every script instance.
pub(crate) struct EngineLayers {
    pub shared: crate::ops::Store,
    pub instrument: Layer,
    pub groups: Box<[Layer]>,
    authored: Box<[Layer]>,
    /// Per group, script envelope stages indexed like `EnvelopeStage`.
    envelopes: Box<[[Option<u32>; 5]]>,
}

impl EngineLayers {
    pub fn new(prepared: &Prepared) -> Self {
        let count = prepared.group_count as usize;
        let authored: Box<[Layer]> = (0..count)
            .map(|g| {
                prepared
                    .group_params
                    .get(g)
                    .map_or(Layer::default(), |p| Layer::authored(*p))
            })
            .collect();
        let (entries, capacity) = &prepared.shared_store;
        Self {
            // Capacity was reserved by `with_shared_store`.
            shared: crate::ops::Store::new(entries.to_vec(), *capacity).unwrap_or_default(),
            instrument: Layer::default(),
            groups: authored.clone(),
            authored,
            envelopes: vec![[None; 5]; count].into_boxed_slice(),
        }
    }

    /// `envelope` with the group's script stages applied.
    pub fn envelope(&self, group: Option<u32>, mut envelope: crate::Envelope) -> crate::Envelope {
        let stages = [
            EnvelopeStage::Attack,
            EnvelopeStage::Hold,
            EnvelopeStage::Decay,
            EnvelopeStage::Sustain,
            EnvelopeStage::Release,
        ];
        if let Some(set) = group.and_then(|g| self.envelopes.get(g as usize)) {
            for (stage, value) in stages.into_iter().zip(set) {
                if let Some(value) = *value {
                    envelope = envelope.with_stage(stage, value);
                }
            }
        }
        envelope
    }

    pub fn layer(&self, group: Option<u32>) -> Layer {
        group
            .and_then(|g| Some((self.groups.get(g as usize)?, self.authored[g as usize])))
            .map_or(self.instrument, |(g, base)| {
                self.instrument.stack(g.since(base))
            })
    }
}

/// Per-note script state, reset when a note is admitted.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NoteParams {
    pub layer: Layer,
    pub fade: Option<Fade>,
    pub mods: ModValues,
}

/// A note's "from script" modulator values by id; unset ids read 0.
// ponytail: twelve ids per note (KSP scripts use at most three, Pacific 1, 3,
// 4; UVI's VWinds sets seven); further ids are dropped. Grow if a library
// needs more.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModValues {
    ids: [u16; 12],
    values: [i32; 12],
    len: u8,
}

impl ModValues {
    pub fn get(&self, id: u16) -> i32 {
        let len = usize::from(self.len);
        self.ids[..len]
            .iter()
            .position(|&i| i == id)
            .map_or(0, |at| self.values[at])
    }

    fn set(&mut self, id: u16, value: i32) {
        let len = usize::from(self.len);
        match self.ids[..len].iter().position(|&i| i == id) {
            Some(at) => self.values[at] = value,
            None if len < self.ids.len() => {
                self.ids[len] = id;
                self.values[len] = value;
                self.len += 1;
            }
            None => {}
        }
    }
}

impl Runtime {
    /// Set a note's "from script" modulator `id` for a frontend that drives
    /// the runtime directly (the Falcon Lua host); `value` clamps to ±1.
    /// Voices of the note read it on their next chunk.
    pub fn set_note_script_value(
        &mut self,
        note: NoteId,
        id: u16,
        value: f64,
    ) -> Result<(), Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let value = (value.clamp(-1.0, 1.0) * 1_000_000.0).round() as i32;
        self.note_params[note.0.index].mods.set(id, value);
        Ok(())
    }

    fn param_layer(
        &mut self,
        plan: crate::PlanId,
        scope: ParamScope,
        index: i64,
    ) -> Result<Option<&mut Layer>, Error> {
        Ok(match scope {
            ParamScope::Note => {
                let Ok(id) = i32::try_from(index) else {
                    return Ok(None);
                };
                self.resolve_source_event(plan, id)?
                    .map(|note| &mut self.note_params[note.0.index].layer)
            }
            ParamScope::Group => {
                let layers = &mut self.plans.get_mut(plan.0).unwrap().script;
                if index < 0 {
                    Some(&mut layers.instrument)
                } else {
                    usize::try_from(index)
                        .ok()
                        .and_then(|g| layers.groups.get_mut(g))
                }
            }
        })
    }

    pub(crate) fn write_param(
        &mut self,
        plan: crate::PlanId,
        scope: ParamScope,
        index: i64,
        target: ModTarget,
        value: i64,
        relative: bool,
    ) -> Result<(), Error> {
        self.script_params = true;
        match self.param_layer(plan, scope, index)? {
            Some(layer) => layer.write(target, value, relative),
            None => Ok(()),
        }
    }

    /// Set a source event's "from script" modulator `id` (0..=1000);
    /// `value` clamps to ±1,000,000. Unknown or retired events are no-ops.
    pub(crate) fn write_mod_value(
        &mut self,
        plan: crate::PlanId,
        event: i64,
        id: i64,
        value: i64,
    ) -> Result<(), Error> {
        let (Ok(event), Ok(id)) = (i32::try_from(event), u16::try_from(id)) else {
            return Ok(());
        };
        let user = (crate::USER_EVENT_PAR..crate::USER_EVENT_PAR + 4).contains(&id);
        if id > 1000 && !user {
            return Ok(());
        }
        if let Some(note) = self.resolve_source_event(plan, event)? {
            // Modulator values are normalized to +-1e6; user parameters keep
            // any integer (a script stores event ids in them).
            let limit = if user { i64::from(i32::MAX) } else { 1_000_000 };
            let value = value.clamp(-limit, limit) as i32;
            self.note_params[note.0.index].mods.set(id, value);
        }
        Ok(())
    }

    pub(crate) fn read_mod_value(
        &mut self,
        plan: crate::PlanId,
        event: i64,
        id: i64,
    ) -> Result<i64, Error> {
        let (Ok(event), Ok(id)) = (i32::try_from(event), u16::try_from(id)) else {
            return Ok(0);
        };
        Ok(self
            .resolve_source_event(plan, event)?
            .map_or(0, |note| self.note_params[note.0.index].mods.get(id).into()))
    }

    pub(crate) fn read_param(
        &mut self,
        plan: crate::PlanId,
        scope: ParamScope,
        index: i64,
        target: ModTarget,
    ) -> Result<i64, Error> {
        Ok(self
            .param_layer(plan, scope, index)?
            .map_or(0, |layer| layer.read(target)))
    }

    /// Set a group's envelope stage; out-of-range groups are no-ops.
    pub(crate) fn write_envelope(
        &mut self,
        plan: crate::PlanId,
        group: i64,
        stage: EnvelopeStage,
        value: i64,
    ) -> Result<(), Error> {
        let value = u32::try_from(value).map_err(|_| Error::InvalidInput)?;
        let layers = &mut self.plans.get_mut(plan.0).unwrap().script;
        if let Some(set) = usize::try_from(group)
            .ok()
            .and_then(|g| layers.envelopes.get_mut(g))
        {
            set[stage as usize] = Some(value);
        }
        Ok(())
    }

    /// Fade a source event in from silence or out from its current level.
    pub(crate) fn fade_event(
        &mut self,
        plan: crate::PlanId,
        event: i64,
        frames: u32,
        out: bool,
        stop: bool,
    ) -> Result<(), Error> {
        self.script_params = true;
        let Ok(id) = i32::try_from(event) else {
            return Ok(());
        };
        let Some(note) = self.resolve_source_event(plan, id)? else {
            return Ok(());
        };
        let now = self.now;
        let params = &mut self.note_params[note.0.index];
        let current = params.fade.map_or(1.0, |f| f.at(now));
        params.fade = Some(Fade {
            from: if out { current } else { 0.0 },
            to: if out { 0.0 } else { 1.0 },
            start: now,
            frames: u64::from(frames),
            stop: out && stop,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_stack_with_the_modulation_laws() {
        let mut note = Layer::default();
        note.write(ModTarget::Decibels, -6000, false).unwrap();
        note.write(ModTarget::Decibels, -6000, true).unwrap();
        note.write(ModTarget::Pan, 800, false).unwrap();
        note.write(ModTarget::Pan, 800, true).unwrap();
        assert_eq!(
            (note.read(ModTarget::Decibels), note.read(ModTarget::Pan)),
            (-12000, 1000)
        );
        assert!(note.write(ModTarget::Cutoff, 1, false).is_err());
        let gains = note.gains(1.0);
        assert!(gains[0] == 0.0 && (gains[1] - 0.2512).abs() < 1e-3);
        let mut group = Layer::default();
        group.write(ModTarget::Attenuate, 0, false).unwrap();
        assert_eq!(group.stack(note).gains(1.0), [0.0; 2]);
        let fade = Fade {
            from: 1.0,
            to: 0.0,
            start: 10,
            frames: 10,
            stop: true,
        };
        assert_eq!(
            (fade.at(15), fade.done(19), fade.done(20)),
            (0.5, false, true)
        );
    }
}
