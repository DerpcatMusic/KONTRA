//! Script-owned voice parameters: a script's per-note volume, pan, tune and
//! fades (KSP `change_vol`, `fade_out`, ...) and its per-group or
//! instrument-wide volume, pan, tune and mute (`set_engine_par`,
//! `purge_group`). Writes apply at the instruction's sample time; voices ramp
//! gain changes across their next render chunk. The targets reuse
//! [`ModTarget`]'s laws so a script layer composes with per-voice modulation.
use crate::{Error, ModTarget, Runtime};

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
    #[cfg(test)]
    pub fn write(&mut self, target: ModTarget, value: i64, relative: bool) -> Result<(), Error> {
        self.write_from(GroupBase::NEUTRAL, target, value, relative)
    }

    /// [`write`](Self::write) where absolute values include the authored
    /// `base`, which the layer's offset is kept relative to.
    pub fn write_from(
        &mut self,
        base: GroupBase,
        target: ModTarget,
        value: i64,
        relative: bool,
    ) -> Result<(), Error> {
        let v = value as f64;
        let (slot, value, bounds) = match target {
            ModTarget::Decibels => (&mut self.decibels, v / 1000.0, None),
            ModTarget::Pan => (&mut self.pan, v / 1000.0, Some((-1.0, 1.0))),
            ModTarget::Pitch => (&mut self.pitch, v / 100_000.0, None),
            ModTarget::Attenuate => (&mut self.attenuate, v / 1000.0, Some((0.0, 1.0))),
            _ => return Err(Error::InvalidInput),
        };
        let base = base.of(target);
        let next = if relative { *slot + value } else { value - base };
        *slot = bounds.map_or(next, |(lo, hi)| (next + base).clamp(lo, hi) - base);
        Ok(())
    }

    #[cfg(test)]
    pub fn read(&self, target: ModTarget) -> i64 {
        self.read_from(GroupBase::NEUTRAL, target)
    }

    /// The layer's value including the authored `base`, in `write` units.
    pub fn read_from(&self, base: GroupBase, target: ModTarget) -> i64 {
        let base = base.of(target);
        (match target {
            ModTarget::Decibels => (self.decibels + base) * 1000.0,
            ModTarget::Pan => (self.pan + base) * 1000.0,
            ModTarget::Pitch => (self.pitch + base) * 100_000.0,
            ModTarget::Attenuate => self.attenuate * 1000.0,
            _ => 0.0,
        })
        .round() as i64
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

/// A group's authored volume, pan and tune, which its script layer is an
/// offset from: scripts read and set absolute values (`get_engine_par`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GroupBase {
    pub decibels: f64,
    /// -1 (left) ..= 1 (right).
    pub pan: f64,
    pub semitones: f64,
}

impl GroupBase {
    pub const NEUTRAL: Self = Self {
        decibels: 0.0,
        pan: 0.0,
        semitones: 0.0,
    };

    fn of(self, target: ModTarget) -> f64 {
        match target {
            ModTarget::Decibels => self.decibels,
            ModTarget::Pan => self.pan,
            ModTarget::Pitch => self.semitones,
            _ => 0.0,
        }
    }
}

/// Per-plan-generation group and instrument layers.
pub(crate) struct EngineLayers {
    pub instrument: Layer,
    pub groups: Box<[Layer]>,
    /// Per group, script envelope stages indexed like `EnvelopeStage`.
    envelopes: Box<[[Option<u32>; 5]]>,
    /// Per group, its authored values (neutral when the plan has none).
    bases: Box<[GroupBase]>,
}

impl EngineLayers {
    pub fn new(groups: u32, bases: &[GroupBase]) -> Self {
        Self {
            bases: (0..groups as usize)
                .map(|g| bases.get(g).copied().unwrap_or_default())
                .collect(),
            instrument: Layer::default(),
            groups: vec![Layer::default(); groups as usize].into_boxed_slice(),
            envelopes: vec![[None; 5]; groups as usize].into_boxed_slice(),
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
            .and_then(|g| self.groups.get(g as usize))
            .map_or(self.instrument, |g| self.instrument.stack(*g))
    }
}

/// Per-note script state, reset when a note is admitted.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NoteParams {
    pub layer: Layer,
    pub fade: Option<Fade>,
}

impl Runtime {
    fn param_layer(
        &mut self,
        plan: crate::PlanId,
        scope: ParamScope,
        index: i64,
    ) -> Result<Option<(&mut Layer, GroupBase)>, Error> {
        Ok(match scope {
            ParamScope::Note => {
                let Ok(id) = i32::try_from(index) else {
                    return Ok(None);
                };
                self.resolve_source_event(plan, id)?
                    .map(|note| (&mut self.note_params[note.0.index].layer, GroupBase::NEUTRAL))
            }
            ParamScope::Group => {
                let layers = &mut self.plans.get_mut(plan.0).unwrap().script;
                if index < 0 {
                    Some((&mut layers.instrument, GroupBase::NEUTRAL))
                } else {
                    let g = usize::try_from(index).unwrap_or(usize::MAX);
                    let base = layers.bases.get(g).copied().unwrap_or_default();
                    layers.groups.get_mut(g).map(|layer| (layer, base))
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
            Some((layer, base)) => layer.write_from(base, target, value, relative),
            None => Ok(()),
        }
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
            .map_or(0, |(layer, base)| layer.read_from(base, target)))
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
        let base = GroupBase {
            pan: 0.5,
            ..GroupBase::NEUTRAL
        };
        let mut panned = Layer::default();
        panned.write_from(base, ModTarget::Pan, 1000, false).unwrap();
        assert_eq!((panned.read(ModTarget::Pan), panned.read_from(base, ModTarget::Pan)), (500, 1000));
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
