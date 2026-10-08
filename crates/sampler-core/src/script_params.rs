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

impl Prepared {
    /// Name buses by source address so a script can route groups to them.
    pub fn with_bus_addresses(mut self, addresses: Vec<(i32, usize)>) -> Self {
        self.bus_addresses = addresses.into_boxed_slice();
        self
    }

    /// Move groups' volume from their voices to a bus fader (one entry per
    /// group, after [`Prepared::with_buses`]): script volume writes then set
    /// the fader at run time, scaling the bus's output and the sends in
    /// `follows` but not the others.
    pub fn with_group_faders(
        mut self,
        faders: Vec<Option<crate::GroupFader>>,
    ) -> Result<Self, Error> {
        if faders.len() != self.group_count as usize {
            return Err(Error::InvalidInput);
        }
        for fader in faders.iter().flatten() {
            self.buses.set_fader(fader)?;
        }
        self.group_faders = faders.into_boxed_slice();
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
    /// Attack curve 0..=1000000 (500000 straight, authored curve -1..1).
    AttackCurve,
}

/// Per-plan-generation script engine state: group and instrument layers,
/// envelope stages and the store shared by every script instance.
pub(crate) struct EngineLayers {
    pub shared: crate::ops::Store,
    pub instrument: Layer,
    pub groups: Box<[Layer]>,
    authored: Box<[Layer]>,
    /// Per group, the bus whose fader carries its volume.
    fader: Box<[Option<usize>]>,
    /// Per group, script envelope stages indexed like `EnvelopeStage`.
    envelopes: Box<[[Option<u32>; 6]]>,
    /// Per group, the bus a script routed it to (`Some(None)`: the master).
    route: Box<[Option<Option<usize>>]>,
    addresses: Box<[(i32, usize)]>,
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
            fader: (0..count)
                .map(|g| {
                    prepared
                        .group_faders
                        .get(g)
                        .and_then(|f| f.as_ref().map(|f| f.bus))
                })
                .collect(),
            authored,
            envelopes: vec![[None; 6]; count].into_boxed_slice(),
            route: vec![None; count].into_boxed_slice(),
            addresses: prepared.bus_addresses.clone(),
        }
    }

    /// Route `group` to the bus at source `address`; a bus the plan lacks is the master.
    pub fn set_route(&mut self, group: usize, address: i64) {
        let bus = self
            .addresses
            .iter()
            .find(|(a, _)| i64::from(*a) == address)
            .map(|&(_, bus)| bus);
        if let Some(slot) = self.route.get_mut(group) {
            *slot = Some(bus);
        }
    }

    /// The bus voices of `group` start on: the script's routing, else `authored`.
    pub fn bus(&self, group: Option<u32>, authored: Option<usize>) -> Option<usize> {
        group
            .and_then(|g| self.route.get(g as usize).copied().flatten())
            .unwrap_or(authored)
    }

    /// `envelope` with the group's script stages applied.
    pub fn envelope(&self, group: Option<u32>, mut envelope: crate::Envelope) -> crate::Envelope {
        let stages = [
            EnvelopeStage::Attack,
            EnvelopeStage::Hold,
            EnvelopeStage::Decay,
            EnvelopeStage::Sustain,
            EnvelopeStage::Release,
            EnvelopeStage::AttackCurve,
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
            .and_then(|g| {
                Some((
                    g as usize,
                    self.groups.get(g as usize)?,
                    self.authored[g as usize],
                ))
            })
            .map_or(self.instrument, |(index, g, base)| {
                let mut group = g.since(base);
                if self.fader[index].is_some() {
                    group.decibels = 0.0; // the bus fader carries it
                }
                self.instrument.stack(group)
            })
    }

    /// The bus fader level (linear) group `group`'s script volume sets, if
    /// its volume lives on a bus fader.
    fn fader(&self, group: usize) -> Option<(usize, f64)> {
        Some((
            self.fader[group]?,
            10f64.powf(self.groups[group].decibels / 20.0),
        ))
    }
}

/// Per-note script state, reset when a note is admitted.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NoteParams {
    pub layer: Layer,
    pub transition: Option<(Layer, u64)>,
    pub fade: Option<Fade>,
    pub mods: ModValues,
}

impl NoteParams {
    pub fn layer_at(&self, now: u64) -> Layer {
        let Some((from, start)) = self.transition else {
            return self.layer;
        };
        let amount = (now.saturating_sub(start) as f64 / super::voice_mod::CELL as f64).min(1.0);
        let mix = |a, b| a + (b - a) * amount;
        Layer {
            decibels: mix(from.decibels, self.layer.decibels),
            pan: mix(from.pan, self.layer.pan),
            pitch: mix(from.pitch, self.layer.pitch),
            attenuate: mix(from.attenuate, self.layer.attenuate),
        }
    }
}

/// A note's "from script" modulator values by id; unset ids read 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModValues {
    values: [i32; crate::USER_EVENT_PAR as usize + 16],
}

impl Default for ModValues {
    fn default() -> Self {
        Self {
            values: [0; crate::USER_EVENT_PAR as usize + 16],
        }
    }
}

impl ModValues {
    pub fn get(&self, id: u16) -> i32 {
        self.values.get(usize::from(id)).copied().unwrap_or(0)
    }

    fn set(&mut self, id: u16, value: i32) {
        if let Some(slot) = self.values.get_mut(usize::from(id)) {
            *slot = value;
        }
    }
}

impl Runtime {
    /// Diagnostics: log every script parameter write (`change_vol`, group
    /// volume, `set_event_par`, envelope stages). Allocates; keep it off the
    /// audio path.
    pub fn record_script_writes(&mut self, on: bool) {
        self.write_log = on.then(Vec::new);
    }

    /// Writes logged since the last call, oldest first.
    pub fn take_script_writes(&mut self) -> Vec<String> {
        self.write_log
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

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

    /// Set a note's script volume (decibels), pan (-1..=1) or pitch
    /// (semitones), or add to it, for a frontend that drives the runtime
    /// directly (Falcon's `changeVolume`, `changePan`, `changeTune`). Voices
    /// ramp to it across their next chunk. Stale notes are an error.
    pub fn set_note_param(
        &mut self,
        note: NoteId,
        target: ModTarget,
        value: f64,
        relative: bool,
    ) -> Result<(), Error> {
        self.set_note_param_with_immediate(note, target, value, relative, false)
    }

    /// The same native note layer with an explicit smoothing policy. Immediate
    /// writes take effect on the next sample; otherwise interpolate on the
    /// native 64-frame control interval, starting at the instruction's time.
    pub fn set_note_param_with_immediate(
        &mut self,
        note: NoteId,
        target: ModTarget,
        value: f64,
        relative: bool,
        immediate: bool,
    ) -> Result<(), Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let scale = match target {
            ModTarget::Decibels | ModTarget::Pan => 1000.0,
            ModTarget::Pitch => 100_000.0,
            _ => return Err(Error::InvalidInput),
        };
        if !value.is_finite() {
            return Err(Error::InvalidInput);
        }
        self.script_params = true;
        let params = &mut self.note_params[note.0.index];
        let from = params.layer_at(self.now);
        params
            .layer
            .write(target, (value * scale).round() as i64, relative)?;
        params.transition = (!immediate).then_some((from, self.now));
        Ok(())
    }

    /// As [`Runtime::set_note_param`] for a group of the active plan (a
    /// negative `group` is the whole instrument): a frontend writing a layer's
    /// or program's Gain (decibels) or Pan (-1..=1), as Falcon's
    /// `setParameter` does.
    pub fn set_group_param(
        &mut self,
        group: i64,
        target: ModTarget,
        value: f64,
        relative: bool,
    ) -> Result<(), Error> {
        let scale = match target {
            ModTarget::Decibels | ModTarget::Pan => 1000.0,
            _ => return Err(Error::InvalidInput),
        };
        if !value.is_finite() {
            return Err(Error::InvalidInput);
        }
        let plan = self.active_plan();
        self.write_param(
            plan,
            ParamScope::Group,
            group,
            target,
            (value * scale).round() as i64,
            relative,
        )
    }

    /// Fade a note's gain linearly from `from` (its current level when
    /// `None`) to `to` over `frames`; `stop` ends it when the fade completes
    /// at silence.
    pub fn fade_note(
        &mut self,
        note: NoteId,
        from: Option<f64>,
        to: f64,
        frames: u64,
        stop: bool,
    ) -> Result<(), Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if !to.is_finite() || to < 0.0 || from.is_some_and(|v| !v.is_finite() || v < 0.0) {
            return Err(Error::InvalidInput);
        }
        self.script_params = true;
        let now = self.now;
        let params = &mut self.note_params[note.0.index];
        let from = from.unwrap_or_else(|| params.fade.map_or(1.0, |f| f.at(now)));
        params.fade = Some(Fade {
            from,
            to,
            start: now,
            frames,
            stop,
        });
        Ok(())
    }

    /// Fade only existing voices of this note in the selected runtime group.
    /// Group identity is supplied by the frontend's physical source map. Other
    /// groups and subsequent voices keep their own gain and lifetime.
    pub fn fade_note_group(
        &mut self,
        note: NoteId,
        group: u32,
        from: Option<f64>,
        to: f64,
        frames: u64,
        stop: bool,
    ) -> Result<(), Error> {
        let owner = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let plan = self.plans.get(owner.plan.0).unwrap();
        if group >= plan.prepared.group_count
            || !to.is_finite()
            || to < 0.0
            || from.is_some_and(|v| !v.is_finite() || v < 0.0)
        {
            return Err(Error::InvalidInput);
        }
        self.script_params = true;
        for voice in self
            .voices
            .slots
            .iter_mut()
            .filter_map(|s| s.value.as_mut())
        {
            if voice.group == Some(group) && self.families.get(voice.family.0).unwrap().note == note
            {
                let start =
                    from.unwrap_or_else(|| voice.script_fade.map_or(1.0, |f| f.at(self.now)));
                voice.script_fade = Some(Fade {
                    from: start,
                    to,
                    start: self.now,
                    frames,
                    stop,
                });
            }
        }
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
        if let Some(log) = &mut self.write_log {
            log.push(format!(
                "param {scope:?} {index} {target:?} {value} relative {relative}"
            ));
        }
        let Some(layer) = self.param_layer(plan, scope, index)? else {
            return Ok(());
        };
        layer.write(target, value, relative)?;
        if let (ParamScope::Group, ModTarget::Decibels, Ok(group)) =
            (scope, target, usize::try_from(index))
        {
            let generation = self.plans.get_mut(plan.0).unwrap();
            if let Some((bus, level)) = generation.script.fader(group) {
                generation.dsp.buses.fader[bus] = level;
            }
        }
        Ok(())
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
        if let Some(log) = &mut self.write_log {
            log.push(format!("event_par event {event} id {id} value {value}"));
        }
        let user = (crate::USER_EVENT_PAR..crate::USER_EVENT_PAR + 16).contains(&id);
        if id > 1000 && !user {
            return Ok(());
        }
        if let Some(note) = self.resolve_source_event(plan, event)? {
            // Modulator values are normalized to +-1e6; user parameters keep
            // any integer (a script stores event ids in them).
            let limit = if user { i64::from(i32::MAX) } else { 1_000_000 };
            let value = if user {
                value.clamp(i64::from(i32::MIN), limit)
            } else {
                value.clamp(-limit, limit)
            } as i32;
            self.note_params[note.0.index].mods.set(id, value);
        }
        Ok(())
    }

    /// What `get_event_par` reports for a built-in parameter other than note
    /// and velocity. An event that ended or never sounded reads 0.
    pub(crate) fn read_event_info(
        &mut self,
        plan: crate::PlanId,
        event: i64,
        info: crate::EventInfo,
    ) -> Result<i64, Error> {
        let Ok(event) = i32::try_from(event) else {
            return Ok(0);
        };
        let Some(note) = self.resolve_source_event(plan, event)? else {
            return Ok(0);
        };
        Ok(match info {
            crate::EventInfo::Status => 1,
            crate::EventInfo::Key => i64::from(self.notes.get(note.0).unwrap().pitch.key()),
            crate::EventInfo::Velocity => {
                (self.notes.get(note.0).unwrap().velocity * 127.).round() as i64
            }
            crate::EventInfo::ReleaseVelocity => {
                (self.release_times[note.0.index].velocity.unwrap_or(0.) * 127.).round() as i64
            }
            crate::EventInfo::Source => i64::from(self.note_events[note.0.index].creator_slot),
            crate::EventInfo::MidiChannel => {
                i64::from(self.notes.get(note.0).unwrap().address.channel)
            }
            crate::EventInfo::ZoneId => {
                let mut family = self.notes.get(note.0).unwrap().first_family;
                while let Some(index) = family {
                    let f = self.families.slots[index.get()].value.unwrap();
                    family = f.siblings.next;
                    let mut voice = f.first_voice;
                    while let Some(v) = voice {
                        let state = self.voices.slots[v.get()].value.unwrap();
                        voice = state.siblings.next;
                        if !state.stolen {
                            return Ok(i64::from(state.source_zone));
                        }
                    }
                }
                0
            }
        })
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
        if let Some(log) = &mut self.write_log {
            log.push(format!("envelope group {group} {stage:?} {value}"));
        }
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

    /// A script volume write on a tapped group moves its fader: the direct
    /// output and post-fader send follow, the pre-fader send does not.
    #[test]
    fn script_volume_moves_the_tapped_fader_not_the_pre_fader_send() {
        use sampler_ir as ir;
        let send = |to, pre_fader| ir::GroupSend {
            to: ir::BusRef(to),
            gain: ir::Gain::UNITY,
            pre_fader,
        };
        let aux = |name: &str| ir::Bus {
            name: name.into(),
            chain: None,
            sends: Vec::new(),
            output: ir::Output::Master,
            gain: ir::Gain::UNITY,
        };
        let instrument = ir::Instrument {
            assets: vec![ir::Asset {
                location: ir::AssetLocation::Path("a".into()),
                encoding: ir::Encoding::Wav,
                root_key: None,
                loops: Vec::new(),
            }],
            buses: vec![aux("pre"), aux("post")],
            groups: vec![ir::Group {
                gain: ir::Gain::Linear(0.5),
                sends: vec![send(0, true), send(1, false)],
                ..Default::default()
            }],
            zones: vec![ir::Zone {
                keys: ir::KeyRange { low: 0, high: 127 },
                pitch: ir::KeyTracking::Fixed,
                velocity: ir::VelocityResponse::None,
                group: Some(ir::GroupRef(0)),
                ..ir::Zone::new(ir::AssetRef(0))
            }],
            ..ir::Instrument::default()
        };
        let pcm = vec![crate::Pcm::new(48000, vec![[0.5; 2]; 4800].into_boxed_slice()).unwrap()];
        let plan = crate::lower::lower(&instrument, 48000, pcm, |_, p| Ok(p)).unwrap();
        let limits = crate::Limits {
            notes: 16,
            channels: 1,
            performances: 1,
            families: 16,
            expressions: 16,
            voices: 32,
            decisions: 32,
            commands: 16,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        };
        let mut rt = Runtime::new(plan, limits).unwrap();
        rt.trigger(
            crate::Input {
                protocol: crate::Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: None,
            },
            60,
            1.,
        )
        .unwrap();
        let peaks = |rt: &mut Runtime| {
            let mut out = [[0.0; 2]; 64];
            rt.render(&mut out).unwrap();
            let mut peaks = [0.0f32; 3];
            rt.take_bus_peaks(|bus, p| peaks[bus] = p[0]);
            (peaks, out[63][0])
        };
        let ([pre, post, tap], master) = peaks(&mut rt);
        // The tap hears the voice before the fader (0.5); at -6 dB the fader
        // gives the direct output and the post-fader send 0.25 each.
        assert!(
            (tap - 0.5).abs() < 1e-3 && (pre - 0.5).abs() < 1e-3 && (post - 0.25).abs() < 1e-3,
            "{pre} {post} {tap}"
        );
        assert!((master - (0.25 + 0.5 + 0.25)).abs() < 1e-3, "{master}");
        let plan = rt.active_plan;
        rt.write_param(
            plan,
            ParamScope::Group,
            0,
            ModTarget::Decibels,
            -12000,
            false,
        )
        .unwrap();
        let ([pre, post, _], master) = peaks(&mut rt);
        let fader = 10f32.powf(-12.0 / 20.0);
        assert!((pre - 0.5).abs() < 1e-3, "pre-fader send moved: {pre}");
        assert!((post - 0.5 * fader).abs() < 1e-3, "{post}");
        assert!(
            (master - (0.5 * fader + 0.5 + 0.5 * fader)).abs() < 2e-3,
            "{master}"
        );
        // Closed: only the pre-fader send is heard.
        rt.write_param(
            plan,
            ParamScope::Group,
            0,
            ModTarget::Decibels,
            -150000,
            false,
        )
        .unwrap();
        let ([pre, post, _], master) = peaks(&mut rt);
        assert!(
            (pre - 0.5).abs() < 1e-3 && post < 1e-4 && (master - 0.5).abs() < 1e-3,
            "{pre} {post} {master}"
        );
    }
}
