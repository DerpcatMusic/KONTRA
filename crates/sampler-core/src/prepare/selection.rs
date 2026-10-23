use crate::{
    Error, Expression, Input, NoteId, NoteOrigin, NotePitch, PlanId, ReleaseReserve, ReleaseStatus,
    Runtime, Trigger,
};

#[derive(Clone, Copy)]
struct Selection {
    plan: PlanId,
    note_pitch: NotePitch,
    intrinsic_ratio:f64,
    velocity: f64,
    offset_micros: u32,
    address: crate::ChannelAddress,
    trigger: Trigger,
    snapshot: usize,
    groups: Option<(usize, crate::groups::GroupView)>,
    /// Reuse only an explicit native frontier on THIS existing pending note.
    selection_owner:Option<NoteId>,
}

impl Runtime {
    /// Select matching native layers, coordinating one family per take sequence
    /// and one for unconditioned layers. Preflight
    /// reserves the entire selection conceptually before publishing the note; no
    /// partial layer set sounds when capacity is exhausted. No match is a logical
    /// no-source note, still paired with its key-up and terminal acceptance.
    pub fn trigger(&mut self, input: Input, key: u8, velocity: f64) -> Result<NoteId, Error> {
        self.trigger_with_expression(input, key, velocity, Expression::default())
    }

    /// Admit a complete initial expression before selection or a bound note program.
    /// Failed native source preflight publishes neither an input nor partial layers.
    pub fn trigger_with_expression(
        &mut self,
        input: Input,
        key: u8,
        velocity: f64,
        expression: Expression,
    ) -> Result<NoteId, Error> {
        self.trigger_pitched(input, NotePitch::Key(key), velocity, expression)
    }

    /// Select regions using inherent pitch, preserving the independent input address.
    /// Absolute pitch bypasses per-key tuning; expression remains a relative offset.
    pub fn trigger_pitched(
        &mut self,
        input: Input,
        pitch: NotePitch,
        velocity: f64,
        expression: Expression,
    ) -> Result<NoteId, Error> {
        self.trigger_in(
            self.performance(0).unwrap(),
            input,
            pitch,
            velocity,
            expression,
        )
    }

    /// Route a physical input independently of its expressive channel. Native latched
    /// switches consume a silent logical input before bound programs or source selection.
    pub fn trigger_in(
        &mut self,
        performance: crate::PerformanceId,
        input: Input,
        pitch: NotePitch,
        velocity: f64,
        expression: Expression,
    ) -> Result<NoteId, Error> {
        self.trigger_in_with_source(performance, input, input, pitch, velocity, expression)
    }

    /// Keep the original physical tuple before the first authored callback,
    /// independently of the logical execution/pedal/controller address.
    pub fn trigger_in_with_source(
        &mut self, performance: crate::PerformanceId, input: Input, source: Input,
        pitch: NotePitch, velocity: f64, expression: Expression,
    ) -> Result<NoteId, Error> {
        if source.group >= 16 || source.channel >= 16 || source.key >= 128
            || input.group >= 16 || input.channel >= 16 || input.key >= 128 { return Err(Error::InvalidInput); }
        self.trigger_in_source(performance, input, pitch, velocity, expression, source)
    }

    fn trigger_in_source(
        &mut self, performance: crate::PerformanceId, input: Input,
        pitch: NotePitch, velocity: f64, expression: Expression, source: Input,
    ) -> Result<NoteId, Error> {
        self.trigger_owned_input(performance, input, source, pitch, velocity, expression, None)
    }

    pub(crate) fn trigger_owned_input(
        &mut self, performance: crate::PerformanceId, input: Input, source: Input,
        pitch: NotePitch, velocity: f64, expression: Expression,
        receiver: Option<crate::receiver::components::ReceiverComponents>,
    ) -> Result<NoteId, Error> {
        self.trigger_owned_pitch_base(performance,input,source,pitch,None,velocity,expression,receiver)
    }
    pub fn trigger_in_with_pitch_base(&mut self,performance:crate::PerformanceId,input:Input,source:Input,pitch:NotePitch,intrinsic:Option<f64>,velocity:f64,expression:Expression)->Result<NoteId,Error>{
        self.trigger_owned_pitch_base(performance,input,source,pitch,intrinsic,velocity,expression,None)
    }
    pub(crate) fn trigger_owned_pitch_base(&mut self,performance:crate::PerformanceId,input:Input,source:Input,pitch:NotePitch,intrinsic:Option<f64>,velocity:f64,expression:Expression,receiver:Option<crate::receiver::components::ReceiverComponents>)->Result<NoteId,Error>{
        if intrinsic.is_some_and(|value|!NotePitch::Absolute(value).valid()){return Err(Error::InvalidInput);}
        if self.external_execution_aborted{return Err(Error::Canceled);}
        if
input.group>=16||input.channel>=16||input.key>=128
            ||source.group>=16||source.channel>=16||source.key>=128 {return Err(Error::InvalidInput);}
        if let Some(component)=receiver {self.receiver(component.receiver)?;}
        self.validate_zone_routing_pitch(self.active_plan, pitch)?;
        let index = self.performance_index(performance)?;
        self.apply_due();
        if !expression.valid() {
            return Err(Error::InvalidInput);
        }
        if let Some(value) = self
            .plans
            .get(self.active_plan.0)
            .unwrap()
            .prepared
            .keyswitches
            .get(usize::from(input.key))
            .copied()
            .flatten()
        {
            // A keyswitch note consumes neither an RR step nor a random draw.
            let saved = self.plans.get(self.active_plan.0).unwrap();
            let (random, draw) = (saved.native_random, saved.native_draw);
            let note = self.note_on_owned_pitch_base(performance, input, source, pitch,intrinsic, velocity, expression, receiver)?;
            if !self
                .plans
                .get(self.active_plan.0)
                .unwrap()
                .prepared
                .native_start
                .is_empty()
            {
                let generation = self.plans.get_mut(self.active_plan.0).unwrap();
                generation.native_cycle = generation.native_cycle.wrapping_sub(1);
                generation.native_random = random;
                generation.native_draw = draw;
            }
            self.articulation_now(index, value);
            if !self
                .plans
                .get(self.active_plan.0)
                .unwrap()
                .prepared
                .native_start
                .is_empty()
            {
                self.performance_state.edit(index).native_key = Some(input.key);
                self.performance_state.sync_metadata(index);
            }
            self.performance_state
                .release(self.selections[note.0.index].snapshot);
            self.selections[note.0.index].snapshot = self.performance_state.capture_channel(index, input.channel_address());
            self.selections[note.0.index].consumed_switch = true;
            self.notes.get_mut(note.0).unwrap().attack = crate::AttackStatus::Suppressed;
            return Ok(note);
        }
        let prepared = &self.plans.get(self.active_plan.0).unwrap().prepared;
        if prepared.source_owners.note_root.is_some(){
            if !self.behavior_room(1){return Err(Error::Capacity);}
            let note=self.note_on_owned_pitch_base(performance,input,source,pitch,intrinsic,velocity,expression,receiver)?;
            self.start_source_note(note,source.channel)?;return Ok(note);
        }
        let on_note = prepared.stages.iter().any(|stage| stage.note.is_some());
        if !on_note {
            let note = self.select(
                NoteOrigin::Input(crate::InputOrigin {input,source,expression,intrinsic,performance:index,receiver}), pitch, velocity, 0, None, None,
            )?;
            return Ok(note);
        }
        let callbacks = prepared
            .stages
            .iter()
            .filter(|stage| stage.note.is_some())
            .count()
            + prepared
                .stages
                .iter()
                .filter(|stage| stage.release.is_some())
                .count();
        if !self.behavior_room(callbacks) {
            return Err(Error::Capacity);
        }
        let note = self.note_on_owned_pitch_base(performance, input, source, pitch,intrinsic, velocity, expression, receiver)?;
        self.reserve_release_callbacks(note, 0);
        self.begin_note_stages(note, 0);
        Ok(note)
    }

    /// Write the interval from the most recently started, still-held input key
    /// of `performance` to `key` into the performance state the new note
    /// will capture. Called before the note exists, so it is never its own
    /// predecessor.
    fn record_previous_key(&mut self, performance: usize, key: u8) {
        let held = self
            .notes
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| Some((i, s.value.as_ref()?)))
            .filter(|(i, n)| {
                n.input_down && n.input.is_some() && self.selections[*i].performance == performance
            })
            .max_by_key(|(_, n)| n.order)
            .map(|(_, n)| i16::from(key) - i16::from(n.pitch.key()));
        self.performance_state.edit(performance).previous = crate::previous_key_value(held);
        self.performance_state.sync_metadata(performance);
    }

    pub(crate) fn select(
        &mut self,
        origin: NoteOrigin,
        note_pitch: NotePitch,
        velocity: f64,
        offset_micros: u32,
        source_stage: Option<crate::behavior::NoteStage>,
        defer: Option<crate::BehaviorId>,
    ) -> Result<NoteId, Error> {
        self.apply_due();
        if !note_pitch.valid() || !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) {
            return Err(Error::InvalidInput);
        }
        let plan = match origin {
            NoteOrigin::Input(..) => self.active_plan,
            NoteOrigin::Generated(plan, ..) => plan,
            NoteOrigin::Child(parent, ..) => {
                self.notes.get(parent.0).ok_or(Error::StaleHandle)?.plan
            }
        };
        self.validate_zone_routing_pitch(plan, note_pitch)?;
        let entry = source_stage.map_or(0, |stage| stage.index() + 1);
        // As in Kontakt, a note a module's note callback plays skips that note
        // callback but its release runs the module's release callback too (Una
        // Corda sustains its own notes there). Notes played by a release
        // callback do not re-enter it.
        let release_entry = match source_stage {
            Some(crate::behavior::NoteStage::Attack(stage)) => stage,
            _ => entry,
        };
        let prepared = &self.plans.get(plan.0).unwrap().prepared;
        let routed =
            source_stage.is_some() && prepared.stages[entry..].iter().any(|s| s.note.is_some());
        let release_route = source_stage.is_some() || matches!(origin, NoteOrigin::Input(..));
        let callbacks = prepared.stages[entry..]
            .iter()
            .filter(|s| s.note.is_some())
            .count();
        let callbacks = usize::from(routed) * callbacks
            + if release_route {
                prepared.stages[release_entry..]
                    .iter()
                    .filter(|s| s.release.is_some())
                    .count()
            } else {
                0
            };
        if !self.behavior_room(callbacks) {
            return Err(Error::Capacity);
        }
        let pitch = match origin {
            NoteOrigin::Input(input) => crate::pitch::PitchRange::constant(
                self.project_expression(self.modulation_plan(plan), input.expression, None)?
                    .ratio,
            ),
            NoteOrigin::Child(_, _, crate::Inheritance::Independent)
            | NoteOrigin::Generated(..) => crate::pitch::PitchRange::constant(
                self.project_expression(self.modulation_plan(plan), Expression::default(), None)?
                    .ratio,
            ),
            NoteOrigin::Child(parent, _, inheritance) => {
                let owner = self.notes.get(parent.0).unwrap().expression;
                self.pitch_range(owner, inheritance == crate::Inheritance::Linked)?
            }
        };
        let intrinsic_ratio=match origin{
            NoteOrigin::Input(input)=>crate::pitch::IntrinsicPitch{reference:self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.inherent_pitch(note_pitch)?,absolute:input.intrinsic}.ratio(),
            NoteOrigin::Child(parent,_,inheritance) if inheritance!=crate::Inheritance::Independent=>self.intrinsic_pitch(self.notes.get(parent.0).ok_or(Error::StaleHandle)?.expression)?.ratio(),
            _=>1.,
        };
        let address = match origin {
            NoteOrigin::Input(input) => input.input.channel_address(),
            NoteOrigin::Generated(_, address, _) => address,
            NoteOrigin::Child(parent, ..) => self.notes.get(parent.0).unwrap().address,
        };
        let performance = match origin {
            NoteOrigin::Input(input) => input.performance,
            NoteOrigin::Generated(_, _, performance) => performance,
            NoteOrigin::Child(parent, ..) => self.selections[parent.0.index].performance,
        };
        if matches!(origin, NoteOrigin::Input(..))
            && self.plans.get(plan.0).unwrap().prepared.tracks_previous
        {
            self.record_previous_key(performance, note_pitch.key());
        }
        if !self
            .plans
            .get(plan.0)
            .unwrap()
            .prepared
            .native_start
            .is_empty()
        {
            let generation = self.plans.get(plan.0).unwrap();
            let state = self.performance_state.edit(performance);
            state.native_tick = generation.native_cycle;
            state.native_draw = generation.native_draw;
            self.performance_state.sync_metadata(performance);
        }
        let snapshot = self.performance_state.channel_current_index(performance, address);
        let release = if routed {
            ReleaseReserve::default()
        } else {
            self.preflight_attack(
                Selection {
                    plan,
                    note_pitch,
                    intrinsic_ratio,
                    velocity,
                    address,
                    offset_micros,
                    trigger: Trigger::Attack,
                    snapshot,
                    selection_owner:None,
                    groups: match origin {
                        NoteOrigin::Input(..) | NoteOrigin::Generated(..) => None,
                        NoteOrigin::Child(parent, ..) => Some((
                            parent.0.index,
                            source_stage
                                .map_or(crate::groups::GroupView::Note(0), |stage| stage.groups()),
                        )),
                    },
                },
                pitch,
            )?
        };
        // Refuse before admitting a note or pinning its lifecycle. Every accepted
        // deferred attack consumes exactly one preallocated slot.
        if defer.is_some() && self.deferred.len() == self.deferred.capacity() {
            return Err(Error::Capacity);
        }
        let note = match origin {
            NoteOrigin::Input(input) => self.note_on_owned_pitch_base(
                self.performance(input.performance).unwrap(), input.input, input.source,
                note_pitch, input.intrinsic, velocity, input.expression, input.receiver,
            )?,
            NoteOrigin::Generated(..) => self.admit(origin, note_pitch, velocity)?,
            NoteOrigin::Child(parent, linked, inheritance) => {
                self.child_pitched(parent, note_pitch, velocity, linked, inheritance)?
            }
        };
        let event = &mut self.note_events[note.0.index];
        event.source_offset_micros = offset_micros;
        event.creator_slot = defer.map_or(-1, |id| {
            let c = self.behaviors.get(id.0).unwrap();
            self.plans.get(plan.0).unwrap().prepared.programs[c.program].source_slot
        });
        let origin_stage = source_stage.map_or(0, |stage| stage.index());
        event.entry = origin_stage;
        let generation = self.plans.get_mut(plan.0).unwrap();
        generation.projections.admit(
            note.0.index,
            origin_stage,
            crate::NoteProperties {
                pitch: note_pitch,
                velocity,
            },
        );
        generation.groups.inherit(
            note.0.index,
            origin_stage,
            match origin {
                NoteOrigin::Child(parent, ..) => Some((
                    parent.0.index,
                    source_stage.map_or(crate::groups::GroupView::Note(0), |stage| stage.groups()),
                )),
                _ => None,
            },
        );
        self.project_note(note, origin_stage, entry);
        if release_route {
            self.reserve_release_callbacks(note, release_entry);
        }
        if let Some(id) = defer {
            // Pending until the callback waits or ends, so it can still edit
            // the note's groups; `release` is recomputed then.
            let _ = release;
            let n = self.notes.get_mut(note.0).unwrap();
            n.work = n.work.checked_add(1).expect("bounded deferred attack pin");
            self.deferred.push((id, note, entry));
            if routed {
                let count = self.plans.get(plan.0).unwrap().prepared.stages[entry..]
                    .iter()
                    .filter(|stage| stage.note.is_some())
                    .count();
                self.behaviors.reserve(count);
                self.note_events[note.0.index].pending_callbacks = count;
            }
        } else if routed {
            self.begin_note_stages(note, entry);
        } else {
            self.commit_attack(note, release, snapshot, entry);
            let end = self.plans.get(plan.0).unwrap().prepared.stages.len();
            self.project_note(note, entry, end);
            self.plans
                .get_mut(plan.0)
                .unwrap()
                .projections
                .get_mut(note.0.index, end)?
                .forwarded = true;
        }
        Ok(note)
    }

    /// Commit a pending mapped attack exactly once on its existing logical note.
    /// Uses the original plan and captured onset selection, never a substitute child.
    pub fn forward_attack(&mut self, note: NoteId) -> Result<bool, Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        self.forward_attack_at(note, self.note_events[note.0.index].entry)
    }

    /// Continue a reached module exactly once. Final native selection is atomic.
    pub fn forward_attack_at(&mut self, note: NoteId, stage: usize) -> Result<bool, Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if self.note_events[note.0.index].routed {
            self.forward_note_stage(note, stage)
        } else {
            self.commit_note_attack(note, stage)
        }
    }

    pub(crate) fn commit_note_attack(&mut self, note: NoteId, stage: usize) -> Result<bool, Error> {
        self.apply_due();
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if n.attack != crate::AttackStatus::Pending {
            return Ok(false);
        }
        if !n.gate() || !n.key_down() {
            return Err(Error::ClosedNote);
        }
        let snapshot = self.selections[note.0.index].snapshot;
        let event = self
            .note_event_at(note, stage)?
            .ok_or(Error::InvalidInput)?;
        let selection = Selection {
            plan: n.plan,
            intrinsic_ratio:self.intrinsic_pitch(n.expression)?.ratio(),
            note_pitch: event.pitch,
            velocity: event.velocity,
            offset_micros: self.note_events[note.0.index].source_offset_micros,
            address: n.address,
            trigger: Trigger::Attack,
            snapshot,
            groups: Some((note.0.index, crate::groups::GroupView::Note(stage))),
            selection_owner:Some(note),
        };
        let pitch = self.pitch_range(n.expression, true)?;
        // Preflight may reclaim idle notes; this one has no voices yet, so pin it.
        self.notes.get_mut(note.0).unwrap().pins += 1;
        let release = self.preflight_attack(selection, pitch);
        let n = self.notes.get_mut(note.0).unwrap();
        n.pins -= 1;
        let release = release?;
        n.pitch = event.pitch;
        n.velocity = event.velocity;
        self.commit_attack(note, release, snapshot, stage);
        Ok(true)
    }

    /// Suppress pending mapping. After forwarding/suppression, this is a no-op;
    /// stopping an already sounding note is a separate musical operation.
    pub fn suppress_attack(&mut self, note: NoteId) -> Result<bool, Error> {
        self.apply_due();
        let n = self.notes.get_mut(note.0).ok_or(Error::StaleHandle)?;
        if n.attack != crate::AttackStatus::Pending {
            return Ok(false);
        }
        n.attack = crate::AttackStatus::Suppressed;
        let key = n.pitch.key();
        self.silent = Some(crate::SilentNote {
            key,
            suppressed: true,
            ..Default::default()
        });
        if let Some(log) = &mut self.selection_log {
            log.push(crate::SelectionRecord {
                event: n.order,
                parent_event: None,
                at: self.now,
                key,
                velocity: 0.,
                trigger: Trigger::Attack,
                suppressed: true,
                candidates: Vec::new(),
            });
        }
        self.release_note_callbacks(note);
        self.trim_release_callbacks(note, false);
        Ok(true)
    }

    fn preflight_attack(
        &mut self,
        selection: Selection,
        pitch: crate::pitch::PitchRange,
    ) -> Result<ReleaseReserve, Error> {
        let Selection {
            plan,
            note_pitch,
            intrinsic_ratio,
            velocity,
            address,
            snapshot,
            ..
        } = selection;
        let key = note_pitch.key();
        let attack = self.preflight_selection(selection, pitch)?;
        let prepared = &self.plans.get(plan.0).unwrap().prepared;
        let release = prepared.release_reserves[key as usize][0]
            .plus(prepared.release_reserves[key as usize][1]);
        if release.voices != 0 {
            for trigger in [Trigger::KeyRelease, Trigger::GateRelease] {
                prepared.validate_release_pitch(
                    note_pitch,
                    trigger,
                    pitch,
                    intrinsic_ratio,
                    prepared.release_velocity(trigger, velocity, false, None),
                    prepared.pending_selection(trigger, &self.performance_state.states[snapshot]),
                )?;
                let generation = self.plans.get(plan.0).unwrap();
                for sequence in prepared.sequence_groups(
                    key,
                    trigger,
                    prepared.pending_selection(trigger, &self.performance_state.states[snapshot]),
                    prepared.release_velocity(trigger, velocity, false, None),
                ) {
                    generation.sequences.check_owner(
                        &prepared.sequences[sequence],
                        address,
                        key,
                    )?;
                }
            }
        }
        let required = attack.plus(release);
        self.reclaim_internal_notes(required);
        self.steal_release_reserves(required);
        self.steal_voices(required.voices);
        self.check_selection_capacity(required)?;
        Ok(release)
    }

    fn commit_attack(
        &mut self,
        note: NoteId,
        release: ReleaseReserve,
        snapshot: usize,
        stage: usize,
    ) {
        let n = self.notes.get_mut(note.0).unwrap();
        let (plan, key, velocity, address) = (n.plan, n.pitch.key(), n.velocity, n.address);
        n.attack = crate::AttackStatus::Forwarded;
        self.plans
            .get_mut(plan.0)
            .unwrap()
            .groups
            .commit_at(note.0.index, stage);
        if release.voices != 0 {
            self.reserve_release(release);
            let generation = self.plans.get_mut(plan.0).unwrap();
            let prepared = &generation.prepared;
            for trigger in [Trigger::KeyRelease, Trigger::GateRelease] {
                let range = prepared.range(key, trigger);
                if range.is_empty() {
                    continue;
                }
                self.release_times[note.0.index].selection[trigger.release_index().unwrap()] =
                    ReleaseStatus::Pending;
                for sequence in prepared.sequence_groups(
                    key,
                    trigger,
                    prepared.pending_selection(trigger, &self.performance_state.states[snapshot]),
                    prepared.release_velocity(trigger, velocity, false, None),
                ) {
                    generation
                        .sequences
                        .claim_owner(&prepared.sequences[sequence], address, key);
                }
            }
        }
        self.commit_selection(note, Trigger::Attack, velocity, snapshot);
    }

    /// When reservations alone keep `required` from fitting, suppress the
    /// pending release phases of the oldest notes (returning their quotas)
    /// until it fits, if `set_release_stealing` allows. Nothing is taken
    /// unless the outstanding reservations could cover the shortfall.
    fn steal_release_reserves(&mut self, required: ReleaseReserve) {
        let fits = |rt: &Self, extra: ReleaseReserve| {
            required.processing_owners <= rt.processing_owners.available() + extra.processing_owners
                && required.voices <= rt.voices.available() + extra.voices
                && required.families <= rt.families.available() + extra.families
                && required.decisions <= rt.decisions.available() + extra.decisions
                && required.commands <= rt.available_commands() + extra.commands
        };
        if !self.steal_releases
            || fits(self, ReleaseReserve::default())
            || !fits(self, self.release_reserve())
        {
            return;
        }
        let pending = |rt: &Self, i: usize| {
            rt.notes.slots[i].value.is_some()
                && rt.release_times[i]
                    .selection
                    .contains(&ReleaseStatus::Pending)
        };
        // ponytail: a scan per stolen note over the bounded note pool.
        while !fits(self, ReleaseReserve::default()) {
            let Some(oldest) = (0..self.notes.slots.len())
                .filter(|&i| pending(self, i))
                .min_by_key(|&i| self.notes.slots[i].value.unwrap().order)
            else {
                return;
            };
            let note = NoteId(self.notes.id(oldest));
            for trigger in [Trigger::KeyRelease, Trigger::GateRelease] {
                self.run_release(note, trigger, false);
            }
        }
    }

    fn check_selection_capacity(&self, required: ReleaseReserve) -> Result<(), Error> {
        if required.processing_owners > self.processing_owners.available()
            || required.voices > self.voices.available()
            || required.families > self.families.available()
            || required.decisions > self.decisions.available()
            || required.commands > self.available_commands()
        {
            Err(Error::Capacity)
        } else {
            Ok(())
        }
    }

    fn preflight_selection(
        &self,
        selection: Selection,
        pitch: crate::pitch::PitchRange,
    ) -> Result<ReleaseReserve, Error> {
        let Selection {
            plan,
            note_pitch,
            intrinsic_ratio,
            velocity,
            address,
            trigger,
            snapshot,
            groups,
            offset_micros,
            selection_owner,
        } = selection;
        let key = note_pitch.key();
        let generation = self.plans.get(plan.0).unwrap();
        let prepared = &generation.prepared;
        let range = prepared.range(key, trigger);
        let note_index = groups.map(|(index, _)| index);
        let groups = groups.map(|(index, view)| generation.groups.view(index, view));
        let mut required = ReleaseReserve::default();
        let mut processing_scopes = [0u64; sampler_ir::MAX_PROCESSING_SCOPES.div_ceil(64)];
        let mut stream_pages=0usize;
        let mut from = range.start;
        while from < range.end {
            let until = prepared.group_end(from, range.end);
            let state = &self.performance_state.states[snapshot];
            let ranges = prepared.active_ranges(from..until, state.articulation);
            let mut matching = super::Matching::new(ranges);
            let first=self.next_source_selected_candidate(selection_owner,&mut matching,plan,snapshot,velocity,groups);
            let retained=selection_owner.filter(|note|self.source_frontier(*note).is_some()).and_then(|note|
                first.and_then(|candidate|prepared.regions[candidate.region].take).and_then(|take|self.source_take(note,trigger,take.sequence)));
            let choice=if retained.is_some(){None}else{prepared.choose(first,address,key,&generation.sequences)?};
            let chosen=retained.map(|(_,take)|take).or(choice.map(|pending|pending.take));
            let mut candidate = first;
            let mut count = 0;
            while let Some(c) = candidate {
                if prepared.regions[c.region].take == chosen {
                    let r = prepared.regions[c.region];
                    if let Some((sample,cursor))=generation.samples.source(prepared,c,note_pitch) {
                        let step=pitch.apply_source(cursor.step()*if r.root_key.is_some(){intrinsic_ratio}else{1.},cursor.wavetable().is_some())?;
                        let source=generation.source(sample).ok_or(Error::NotReady)?;
                        if source.stereo().is_none() {
                            // A pending event can change key after a legal stereo
                            // pan write. Validate the ACTUAL chosen source before
                            // any family/owner/voice publication, not during render.
                            let group = prepared.region_groups.get(c.region).copied().flatten();
                            if generation.script.layer(group).has_source_pan()
                                || note_index.is_some_and(|index| self.note_params[index].has_source_pan(self.now)) {
                                return Err(Error::UnsupportedSourceLayout);
                            }
                        }
                        let cursor=cursor.with_offset(offset_micros,source.sample_rate()).with_step(step);
                        let cursor = self.stream_start_cursor(plan, cursor);
                        let envelope=self.region_envelope(r.envelope,r.fallback_envelope);
                        let (cold,streamed)=self.check_logical_source_ready(source,cursor,envelope)?;
                        if streamed {
                            let pages=self.admit_source_streaming(source,cursor,cold).map_err(|e|if e==crate::StreamError::Capacity {Error::Capacity}else{Error::NotReady})?;
                            stream_pages=stream_pages.checked_add(pages).ok_or(Error::Capacity)?;
                            self.check_stream_selection_pages(stream_pages)?;
                        }
                        if let Some((scope, _)) = prepared.region_processing.get(c.region).copied().flatten() {
                            processing_scopes[scope / 64] |= 1 << (scope % 64);
                        }
                        count+=1;
                    }
                }
                candidate=self.next_source_selected_candidate(selection_owner,&mut matching,plan,snapshot,velocity,groups);
            }
            required.voices += count;
            required.families += usize::from(count != 0);
            required.decisions += usize::from(choice.is_some());
            from = until;
        }
        required.processing_owners = processing_scopes.iter().map(|word| word.count_ones() as usize).sum();
        if let Some(index) = trigger.release_index()
            && let Some(frames) = prepared.release_options[index].duration
        {
            self.now
                .checked_add(u64::from(frames))
                .ok_or(Error::ClockOverflow)?;
            if frames != 0 {
                required.commands = required.families;
            }
        }
        Ok(required)
    }

    /// Start (or stop and discard) recording why each region was or was not
    /// chosen by every selection. Off by default and never on the audio path's
    /// critical cost: the recorder allocates, so enable it only off-thread or
    /// in diagnostics.
    pub fn record_selections(&mut self, on: bool) {
        self.selection_log = on.then(Vec::new);
    }

    /// The latest attack that selected nothing (audio-thread safe, no
    /// allocation), once.
    pub fn take_silent_note(&mut self) -> Option<crate::SilentNote> {
        self.silent.take()
    }

    /// Records gathered since the last call, oldest first.
    pub fn take_selection_records(&mut self) -> Vec<crate::SelectionRecord> {
        self.selection_log
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// Walk every region mapped to the note's key and report its verdict, in the
    /// order `commit_selection` checks: phase, articulation, controller
    /// condition, velocity, script group. Allocation-free.
    fn each_verdict(
        &self,
        note: NoteId,
        trigger: Trigger,
        velocity: f64,
        snapshot: usize,
        mut each: impl FnMut(usize, Option<u32>, Option<crate::Rejection>),
    ) {
        use crate::Rejection;
        let n = self.notes.get(note.0).unwrap();
        let (plan, key) = (n.plan, n.pitch.key());
        let generation = self.plans.get(plan.0).unwrap();
        let prepared = &generation.prepared;
        let state = &self.performance_state.states[snapshot];
        let mask = generation
            .groups
            .view(note.0.index, crate::groups::GroupView::Committed);
        let phase = prepared.range(key, trigger);
        let all = prepared.offsets[usize::from(key)]..prepared.offsets[usize::from(key) + 1];
        for index in all {
            let c = prepared.candidates[index];
            let r = prepared.regions[c.region];
            let group = prepared.region_groups.get(c.region).copied().flatten();
            let verdict = if generation.zone_routing.as_ref().is_some_and(|s| s.next_eligible(index..index + 1).is_none()) {
                Some(Rejection::Group)
            } else if !phase.contains(&index) {
                Some(Rejection::Trigger)
            } else if prepared.articulated
                && r.articulation.is_some_and(|a| a != state.articulation)
            {
                Some(Rejection::Articulation)
            } else if group.is_some_and(|g| !prepared.native_group_allowed(g, state)) {
                Some(Rejection::Condition)
            } else if r.conditions.is_some_and(|i| {
                !prepared.conditions[i].iter().all(|c| {
                    let v = state.value(c.controller);
                    c.low <= v && v <= c.high
                })
            }) {
                Some(Rejection::Condition)
            } else if !(r.velocity_low <= velocity && velocity <= r.velocity_high) {
                Some(Rejection::Velocity)
            } else if group.is_some_and(|g| mask[g as usize / 64] & (1 << (g % 64)) == 0)
                ||!self.selected_source_region(note,c.region) {
                Some(Rejection::Group)
            } else {
                generation.samples.rejection(prepared,c,n.pitch)
            };
            each(index, group, verdict);
        }
    }

    /// Eligible groups at this callback's selection view. Do not advance RR or random state.
    pub(crate) fn affected_group(
        &self,
        note: NoteId,
        view: crate::groups::GroupView,
        index: Option<i64>,
    ) -> i64 {
        let n = self.notes.get(note.0).unwrap();
        let generation = self.plans.get(n.plan.0).unwrap();
        let prepared = &generation.prepared;
        let stage = match view {
            crate::groups::GroupView::Note(s) | crate::groups::GroupView::Release(s) => s,
            crate::groups::GroupView::Committed => self.note_events[note.0.index].entry,
        };
        let properties = generation
            .projections
            .get(note.0.index, stage)
            .ok()
            .and_then(|p| p.properties)
            .unwrap_or(self.note_events[note.0.index].initial);
        let snapshot = self.selections[note.0.index].snapshot;
        let state = &self.performance_state.states[snapshot];
        let trigger = match view {
            crate::groups::GroupView::Release(_) => Trigger::KeyRelease,
            _ => Trigger::Attack,
        };
        let range = prepared.range(properties.pitch.key(), trigger);
        // GROUPS_AFFECTED describes native mapping before script allow/disallow edits.
        let mask = None;
        let mut count = 0;
        // ponytail: bounded group/candidate scan; cache a group index if query profiling warrants it.
        for group in 0..prepared.group_count {
            let mut from = range.start;
            let mut found = false;
            while from < range.end && !found {
                let until = prepared.group_end(from, range.end);
                let ranges = prepared.active_ranges(from..until, state.articulation);
                let mut matching = super::Matching::new(ranges);
                while let Some(c) =
                    matching.next_in_groups(prepared, state, properties.velocity, mask, generation.zone_routing.as_ref())
                {
                    if prepared.region_groups[c.region] == Some(group) {
                        found = true;
                        break;
                    }
                }
                from = until;
            }
            if found {
                if index == Some(count) {
                    return i64::from(group);
                }
                count += 1;
            }
        }
        if index.is_none() { count } else { -1 }
    }

    /// Verdict for every region mapped to the note's key, then the round-robin take.
    fn diagnose(
        &self,
        note: NoteId,
        trigger: Trigger,
        velocity: f64,
        snapshot: usize,
    ) -> crate::SelectionRecord {
        use crate::{RegionVerdict, Rejection};
        let n = self.notes.get(note.0).unwrap();
        let (plan, key) = (n.plan, n.pitch.key());
        let generation = self.plans.get(plan.0).unwrap();
        let prepared = &generation.prepared;
        let mut candidates = Vec::new();
        self.each_verdict(
            note,
            trigger,
            velocity,
            snapshot,
            |index, group, verdict| {
                candidates.push((index, prepared.candidates[index], group, verdict));
            },
        );
        // The take actually chosen per sequence is the first survivor's; others
        // of the same sequence lose to the round robin.
        let mut chosen = std::collections::BTreeMap::new();
        for (_, c, _, verdict) in &candidates {
            if verdict.is_none()
                && let Some(take) = prepared.regions[c.region].take
            {
                let take=if self.source_frontier(note).is_some(){self.source_take(note,trigger,take.sequence).map_or(take,|(_,chosen)|chosen)}else{take};
                chosen.entry(take.sequence).or_insert(take);
            }
        }
        let candidates = candidates
            .into_iter()
            .map(|(_, c, group, verdict)| {
                let take = prepared.regions[c.region].take;
                let verdict = verdict.or_else(|| {
                    take.filter(|t| chosen.get(&t.sequence).is_some_and(|k| k != t))
                        .map(|_| Rejection::Take)
                });
                RegionVerdict {
                    region: c.region,
                    group,
                    rejected: verdict,
                    started: None,
                }
            })
            .collect();
        crate::SelectionRecord {
            event: n.order,
            parent_event: n
                .parent
                .and_then(|id| self.notes.get(id.0).map(|n| n.order)),
            at: self.now,
            key,
            velocity,
            trigger,
            suppressed: false,
            candidates,
        }
    }

    fn commit_selection(&mut self, note: NoteId, trigger: Trigger, velocity: f64, snapshot: usize) {
        let n = self.notes.get(note.0).unwrap();
        let (plan, note_pitch, address) = (n.plan, n.pitch, n.address);
        let key = note_pitch.key();
        let processing_activation = self.begin_processing_activation();
        let range = self.plans.get(plan.0).unwrap().prepared.range(key, trigger);
        let (begin, end) = (range.start, range.end);
        if trigger == Trigger::Attack {
            // Always on and allocation-free: the plugin's "why silent" line.
            let mut silent = crate::SilentNote {
                key,
                ..Default::default()
            };
            let mut accepted = false;
            self.each_verdict(note, trigger, velocity, snapshot, |_, _, v| match v {
                Some(r) => silent.counts[crate::SilentNote::slot(r)] += 1,
                None => accepted = true,
            });
            if !accepted {
                self.silent = Some(silent);
            }
        }
        if self.selection_log.is_some() {
            let record = self.diagnose(note, trigger, velocity, snapshot);
            if let Some(log) = self.selection_log.as_mut() {
                log.push(record);
            }
        }
        // Everything below was validated by Prepared and preflight. No callbacks,
        // concurrent writers or newly due work can consume the reserved resources.
        let mut from = begin;
        while from < end {
            let generation = self.plans.get(plan.0).unwrap();
            let prepared = &generation.prepared;
            let until = prepared.group_end(from, end);
            let state = &self.performance_state.states[snapshot];
            let ranges = prepared.active_ranges(from..until, state.articulation);
            let mut matching = super::Matching::new(ranges);
            let groups = Some(
                generation
                    .groups
                    .view(note.0.index, crate::groups::GroupView::Committed),
            );
            let mut first=self.next_source_selected_candidate(Some(note),&mut matching,plan,snapshot,velocity,groups);
            let retained=self.source_frontier(note).and_then(|_|first.and_then(|candidate|prepared.regions[candidate.region].take))
                .and_then(|take|self.source_take(note,trigger,take.sequence));
            let choice=if retained.is_some(){None}else{prepared.choose(first,address,key,&generation.sequences).expect("preflighted take decision")};
            let chosen=retained.map(|(_,take)|take).or(choice.map(|pending|pending.take));
            let decision=retained.map(|(index,_)|index).or_else(||choice.map(|pending|self.record_take(note,trigger,pending.take)));
            let mut family = None;
            loop {
                let generation = self.plans.get(plan.0).unwrap();
                let prepared = &generation.prepared;
                let groups = Some(
                    generation
                        .groups
                        .view(note.0.index, crate::groups::GroupView::Committed),
                );
                let state = &self.performance_state.states[snapshot];
                let Some(candidate) = first
                    .take()
                    .or_else(||self.next_source_selected_candidate(Some(note),&mut matching,plan,snapshot,velocity,groups))
                else {
                    break;
                };
                let mut r = prepared.regions[candidate.region];
                if let Some((low, high)) = generation.zone_routing.as_ref()
                    .zip(prepared.zone_routing.as_ref())
                    .and_then(|(s, p)| s.bounds_for_region(p, candidate.region)) {
                    r.bounds[0] = low;
                    r.bounds[1] = high;
                }
                if r.take != chosen {
                    continue;
                }
                let group = prepared
                    .region_groups
                    .get(candidate.region)
                    .copied()
                    .flatten();
                let Some((sample,source_cursor))=self.plans.get(plan.0).unwrap().samples.source(prepared,candidate,note_pitch) else {continue;};
                let step=source_cursor.step();
                let seed=self.now^((note.0.index as u64)<<40)^((candidate.region as u64)<<20);
                let provider_scope=prepared.voice_modulation.region_provider_scope(candidate.region);
                // Claim the real shared owner BEFORE a provider-dependent
                // SampleStart is read and before the physical cursor offset
                // and voice admission. source_cursor above is pure geometry.
                let processing=match self.processing_member_admission(note,processing_activation,candidate.region){
                    Ok(processing)=>processing,
                    Err(_)=>{self.refused_starts=self.refused_starts.saturating_add(1);continue;}
                };
                if provider_scope.is_some(){
                    let Some((owner,_,_))=processing else{self.refused_starts=self.refused_starts.saturating_add(1);continue;};
                    if self.prepare_processing_modulation(owner,self.now,crate::topology::ProcessingModulationPhase::AdmissionOnset).is_err(){
                        self.fail_processing_owner(owner);self.refused_starts=self.refused_starts.saturating_add(1);continue;
                    }
                }
                let generation=self.plans.get(plan.0).unwrap();
                let prepared=&generation.prepared;
                let state=&self.performance_state.states[snapshot];
                let held=self.release_counter_frames(note).unwrap();
                let n=self.notes.get(note.0).unwrap();
                let inputs=crate::voice_mod::Inputs::new(n,self.expressions.get(n.expression.0).unwrap().value,
                    &state.controllers,held,&self.note_params[note.0.index].mods,generation.controls.base_reals());
                let start=if let (Some(scope),Some((owner,_,_)))=(provider_scope,processing){
                    generation.dsp.owner_modulation.provider_view(&prepared.processing_modulation,owner.0.index,scope)
                        .and_then(|view|prepared.voice_modulation.start_offset_provided(candidate.region,&inputs,seed,self.now,view))
                }else{prepared.voice_modulation.start_offset(candidate.region,&inputs,seed)};
                let start=match start{Ok(start)=>start,Err(_)=>{
                    if let Some((owner,_,_))=processing{self.fail_processing_owner(owner);}
                    self.refused_starts=self.refused_starts.saturating_add(1);continue;
                }};
                let cursor=if trigger==Trigger::Attack{
                    source_cursor.with_offset(self.note_events[note.0.index].source_offset_micros,
                        generation.source(sample).expect("admitted logical source").sample_rate())
                }else{source_cursor};
                let cursor=if start!=0{cursor.skip(start)}else{cursor};
                let family = *family.get_or_insert_with(|| {
                    let family = self
                        .create_family_for(note, trigger)
                        .expect("preflight family capacity");
                    self.families.get_mut(family.0).unwrap().decision = decision;
                    family
                });
                let envelope = self.controlled_envelope(
                    plan,
                    group,
                    self.region_envelope(r.envelope, r.fallback_envelope),
                );
                self.processing_admission = processing.is_some();
                let admitted = self.admit_voice(
                    family,
                    sample,
                    self.now,
                    r.gain * r.velocity_curve.amplitude(velocity) * r.fade_gain(key, velocity),
                    envelope,
                    cursor.with_step(step),
                    r.root_key.is_some(),
                );
                self.processing_admission = false;
                // Preflight reserved this start, but a page can be evicted or
                // the pool fill between then and now. Never panic on the audio
                // thread: refuse this voice, count it, carry on.
                let Ok(voice) = admitted else {
                    if let Some((owner, _, _)) = processing { self.fail_processing_owner(owner); }
                    self.refused_starts = self.refused_starts.saturating_add(1);
                    continue;
                };

                if let Some((owner, ordinal, gate)) = processing {
                    self.attach_processing_member(owner, ordinal, voice, gate);
                }
                let state = self.voices.get_mut(voice.0).unwrap();
                state.chain = r.chain;
                state.group = group;
                self.assign_stretch(voice, plan, group);
                self.enforce_voice_limits(plan, group, voice);
                let routed = self.plans.get(plan.0).unwrap().script.bus(group, r.bus);
                let state = self.voices.get_mut(voice.0).unwrap();
                state.bus = routed;
                state.source_zone = self
                    .plans
                    .get(plan.0)
                    .unwrap()
                    .prepared
                    .region_zone_ids
                    .get(candidate.region)
                    .copied()
                    .unwrap_or(candidate.region as u32 + 1);
                state.native_zone = self.plans.get(plan.0).unwrap().prepared
                    .region_identities.get(candidate.region).copied().flatten();
                if let Some(record) = self.selection_log.as_mut().and_then(|log| log.last_mut()) {
                    if let Some(candidate) = record
                        .candidates
                        .iter_mut()
                        .find(|c| c.region == candidate.region)
                    {
                        candidate.started = Some(crate::SelectedSource {
                            zone: state.source_zone,
                            sample: r.sample,
                            frame: state.cursor.trace_position(),
                            direction: state.cursor.trace_direction(),
                            loops: state.cursor.trace_loops(),
                        });
                    }
                }
                if r.chain.is_some() {
                    self.plans.get_mut(plan.0).unwrap().dsp.reset(voice.0.index);
                }
                let held = self.release_counter_frames(note).unwrap();
                let n = self.notes.get(note.0).unwrap();
                let controllers = &self.performance_state.states[snapshot].controllers;
                let g = self.plans.get_mut(plan.0).unwrap();
                let mut inputs = crate::voice_mod::Inputs::new(
                    n,
                    self.expressions.get(n.expression.0).unwrap().value,
                    controllers,
                    held,
                    &self.note_params[note.0.index].mods,
                    g.controls.base_reals(),
                );
                // Glide origin: semitones from this plan's previous started note.
                inputs.interval = g.modulation.glide_interval(i32::from(n.pitch.key()));
                let clock = crate::voice_mod::Clock {
                    rate: f64::from(self.rate),
                    tempo: self.tempo,
                    now: self.now,
                };
                let native_allowed = self.voices.get(voice.0).unwrap().envelope.admits_v1_controls();
                g.modulation.start_with_controls(
                    &g.prepared.voice_modulation,
                    voice.0.index,
                    candidate.region,
                    &inputs,
                    clock,
                    seed, native_allowed,
                );
                if let Some(Some(state)) = g.modulation.native.as_mut_slice().get(voice.0.index) {
                    self.voices.get_mut(voice.0).unwrap().envelope.set_v1_amplitude(state.amplitude(), self.rate as f32);
                }
                // An envelope stage time addressed as a modulation target
                // moves the voice's own stage before the voice starts.
                let stages = g.modulation.envelope_stages(voice.0.index);
                if stages.iter().any(|factor| *factor != 1.0) {
                    let rate = self.rate;
                    let envelope = &mut self.voices.get_mut(voice.0).unwrap().envelope;
                    for (stage, factor) in stages.iter().enumerate() {
                        if *factor == 1.0 {
                            continue;
                        }
                        let stage = match stage {
                            0 => crate::EnvelopeStage::Attack,
                            1 => crate::EnvelopeStage::Hold,
                            2 => crate::EnvelopeStage::Decay,
                            3 => crate::EnvelopeStage::Sustain,
                            4 => crate::EnvelopeStage::Release,
                            _ => continue,
                        };
                        envelope.scale_stage(stage, *factor, rate);
                    }
                }
                if self.signal_trace {
                    self.trace_selected_birth(voice, start, seed);
                }
            }
            if let Some(family) = family {
                self.finish_family(family).expect("admitted family");
                if let Some(index) = trigger.release_index()
                    && let Some(frames) =
                        self.plans.get(plan.0).unwrap().prepared.release_options[index].duration
                {
                    if frames == 0 {
                        self.release_family_now(family);
                    } else {
                        self.queue(
                            self.now + u64::from(frames),
                            crate::Action::Event(crate::Event::ReleaseFamily(family)),
                        );
                    }
                }
            }
            if let Some(choice) = choice {
                let generation = self.plans.get_mut(plan.0).unwrap();
                generation.sequences.commit(choice);
            }
            from = until;
        }
        self.finish_processing_activation(processing_activation);
    }
    /// Always consumes an armed phase, including suppressed/failed selections.
    /// Physical cleanup must never be rolled back by a release-policy failure.
    pub(crate) fn run_release(&mut self, note: NoteId, trigger: Trigger, musical: bool) {
        let index = trigger.release_index().unwrap();
        if self.release_times[note.0.index].selection[index] != ReleaseStatus::Pending {
            return;
        }
        let n = self.notes.get(note.0).unwrap();
        let (plan, note_pitch, address, owner) = (n.plan, n.pitch, n.address, n.expression);
        let prepared = &self.plans.get(plan.0).unwrap().prepared;
        let musical = musical && !prepared.script_release_triggers;
        let reserve = prepared.release_reserves[note_pitch.key() as usize][index];
        let velocity = prepared
            .release_velocity(
                trigger,
                n.velocity,
                true,
                self.release_times[note.0.index].velocity,
            )
            .unwrap();
        let snapshot = self.release_selection(note, trigger);
        let result = if musical {
            self.pitch_range(owner, true).and_then(|pitch| {
                self.preflight_selection(
                    Selection {
                        plan,
                        note_pitch,
                        intrinsic_ratio:self.intrinsic_pitch(owner)?.ratio(),
                        velocity,
                        address,
                        trigger,
                        offset_micros: 0,
                        snapshot,
                        groups: Some((note.0.index, crate::groups::GroupView::Committed)),
                        selection_owner:Some(note),
                    },
                    pitch,
                )
            })
        } else {
            Ok(ReleaseReserve::default())
        };
        self.unreserve_release(reserve);
        self.release_times[note.0.index].selection[index] = match result {
            Err(error) => ReleaseStatus::Failed(error),
            Ok(_) if !musical => ReleaseStatus::Suppressed,
            Ok(required) => {
                self.check_selection_capacity(required)
                    .expect("owned release reservation");
                self.commit_selection(note, trigger, velocity, snapshot);
                ReleaseStatus::Selected
            }
        };
    }
}

impl Runtime {
    fn next_source_selected_candidate(&self,note:Option<NoteId>,matching:&mut super::Matching,plan:PlanId,snapshot:usize,velocity:f64,groups:Option<&[u64]>)->Option<super::Candidate>{
        let generation=self.plans.get(plan.0)?;let prepared=&generation.prepared;
        loop{let candidate=matching.next_in_groups(prepared,&self.performance_state.states[snapshot],velocity,groups,generation.zone_routing.as_ref())?;
            if note.is_none_or(|note|self.selected_source_region(note,candidate.region)){return Some(candidate);}
        }
    }
}

impl Runtime {
    /// Pure input eligibility in the actual prepared source-owner table. This
    /// deliberately does not claim native KG/take selection or advance it.
    /// `channel` is the current authored logical event channel, independently
    /// of the original physical source tuple retained by its real creator.
    pub fn source_owner_input_eligible(&self,plan:PlanId,owner:crate::PreparedSourceOwnerIndex,properties:crate::NoteProperties,channel:u8)->Result<bool,Error>{
        if channel>=16||!properties.pitch.valid()||!properties.velocity.is_finite()||!(0.0..=1.0).contains(&properties.velocity){return Err(Error::InvalidInput);}
        let generation=self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let row=generation.prepared.source_owners.get(owner).ok_or(Error::InvalidInput)?;
        let mut active=true;
        for switch in [row.bypass,row.mute].into_iter().flatten(){
            let value=if let Some(control)=switch.control {match self.control_value(plan,control)?{crate::ControlValue::Toggle(value)=>value,_=>return Err(Error::InvalidInput)}}else{switch.saved};
            active&=!value;
        }
        for predicate in &row.predicates {use sampler_ir::SourceInputPredicate as P;
            active&=match predicate {
                P::Keys(keys)=>keys.low<=properties.pitch.key()&&properties.pitch.key()<=keys.high,
                P::Velocities(range)=>f64::from(range.low)/127.<=properties.velocity&&properties.velocity<=f64::from(range.high)/127.,
                P::Channel(expected)=>*expected==channel,
                P::Unsupported{..}=>return Err(Error::InvalidInput),
            };
        }Ok(active)
    }
    fn frontier_candidate_owner(&self,note:NoteId,parent:crate::PreparedSourceOwnerIndex,stage:usize,channel:u8,region:usize)->Result<Option<crate::PreparedSourceOwnerIndex>,Error>{
        let n=self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let properties=self.note_event_at(note,stage)?.ok_or(Error::InvalidInput)?;
        if !self.source_note_owner_entered(note,parent) && !self.source_owner_input_eligible(n.plan,parent,properties,channel)?{return Ok(None);}
        self.plans.get(n.plan.0).ok_or(Error::StaleHandle)?.prepared.source_owners.get(parent).ok_or(Error::InvalidInput)?;
        let mut found=None;
        let table=&self.plans.get(n.plan.0).ok_or(Error::StaleHandle)?.prepared.source_owners;
        for &(_,child) in table.keygroups_for_region(region){
            let child_row=table.get(child).ok_or(Error::InvalidInput)?;
            if child_row.parent!=Some(parent){continue;}
            if self.source_owner_input_eligible(n.plan,child,properties,channel)?{
                if found.is_some(){return Err(Error::InvalidInput);}
                found=Some(child);
            }
        }Ok(found)
    }
    fn next_frontier_candidate(&self,note:NoteId,parent:crate::PreparedSourceOwnerIndex,stage:usize,channel:u8,matching:&mut super::Matching)->Result<Option<super::Candidate>,Error>{
        let n=self.notes.get(note.0).ok_or(Error::StaleHandle)?;let generation=self.plans.get(n.plan.0).ok_or(Error::StaleHandle)?;
        let properties=self.note_event_at(note,stage)?.ok_or(Error::InvalidInput)?;
        let snapshot=self.selections[note.0.index].snapshot;let state=&self.performance_state.states[snapshot];
        let groups=Some(generation.groups.view(note.0.index,crate::groups::GroupView::Note(stage)));
        loop{let Some(candidate)=matching.next_in_groups(&generation.prepared,state,properties.velocity,groups,generation.zone_routing.as_ref())else{return Ok(None);};
            if self.frontier_candidate_owner(note,parent,stage,channel,candidate.region)?.is_some(){return Ok(Some(candidate));}
        }
    }
    fn frontier_child_chosen(&self,note:NoteId,parent:crate::PreparedSourceOwnerIndex,child:crate::PreparedSourceOwnerIndex,stage:usize,channel:u8,recorded:bool)->Result<bool,Error>{
        let n=self.notes.get(note.0).ok_or(Error::StaleHandle)?;let generation=self.plans.get(n.plan.0).ok_or(Error::StaleHandle)?;let prepared=&generation.prepared;
        let properties=self.note_event_at(note,stage)?.ok_or(Error::InvalidInput)?;let state=&self.performance_state.states[self.selections[note.0.index].snapshot];
        let range=prepared.range(properties.pitch.key(),Trigger::Attack);let mut from=range.start;
        while from<range.end{
            let until=prepared.group_end(from,range.end);let mut matching=super::Matching::new(prepared.active_ranges(from..until,state.articulation));
            let first=self.next_frontier_candidate(note,parent,stage,channel,&mut matching)?;
            let retained=first.and_then(|candidate|prepared.regions[candidate.region].take).and_then(|take|self.source_take(note,Trigger::Attack,take.sequence));
            let choice=if recorded{None}else{prepared.choose(first,n.address,properties.pitch.key(),&generation.sequences)?};
            let chosen=retained.map(|(_,take)|take).or(choice.map(|pending|pending.take));let mut candidate=first;
            while let Some(current)=candidate{
                if prepared.regions[current.region].take==chosen&&self.frontier_candidate_owner(note,parent,stage,channel,current.region)?==Some(child){return Ok(true);}
                candidate=self.next_frontier_candidate(note,parent,stage,channel,&mut matching)?;
            }from=until;
        }Ok(false)
    }
    /// Commit Layer->KG native selection ONCE on this real pending event owner,
    /// before a KG/audio branch can park or change its source. The SAME Take
    /// decisions and chosen owner records remain in the normal Decision arena.
    /// Final native audio consumes them; it never chooses/advances RR twice.
    pub fn commit_source_frontier(&mut self,note:NoteId,parent:crate::PreparedSourceOwnerIndex,stage:usize,channel:u8)->Result<usize,Error>{
        if self.external_execution_aborted{return Err(Error::Canceled);}
        self.apply_due();
        if channel>=16{return Err(Error::InvalidInput);}
        let n=self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if let Some((existing,at))=self.source_frontier(note){
            if existing!=parent||at!=stage{return Err(Error::InvalidInput);}
            let row=self.plans.get(n.plan.0).ok_or(Error::StaleHandle)?.prepared.source_owners.get(parent).ok_or(Error::InvalidInput)?;
            return row.children.iter().try_fold(0usize,|count,&child|Ok(count+usize::from(self.source_child_selected(note,child)?)));
        }
        if n.attack!=crate::AttackStatus::Pending{return Err(Error::InvalidInput);}
        if !n.gate()||!n.key_down(){return Err(Error::ClosedNote);}
        let plan=n.plan;let address=n.address;let properties=self.note_event_at(note,stage)?.ok_or(Error::InvalidInput)?;
        let generation=self.plans.get(plan.0).ok_or(Error::StaleHandle)?;let prepared=&generation.prepared;
        let row=prepared.source_owners.get(parent).ok_or(Error::InvalidInput)?;
        if row.kind!=sampler_ir::SourceOwnerKind::Layer{return Err(Error::InvalidInput);}
        self.source_owner_input_eligible(plan,parent,properties,channel)?;
        let mut selected=0usize;
        for &child in &row.children{
            let child_row=prepared.source_owners.get(child).ok_or(Error::InvalidInput)?;
            if !matches!(child_row.kind,sampler_ir::SourceOwnerKind::Keygroup|sampler_ir::SourceOwnerKind::Aux){return Err(Error::InvalidInput);}
            if child_row.kind==sampler_ir::SourceOwnerKind::Keygroup{
                self.source_owner_input_eligible(plan,child,properties,channel)?;
                selected=selected.checked_add(usize::from(self.frontier_child_chosen(note,parent,child,stage,channel,false)?)).ok_or(Error::Capacity)?;
            }
        }
        let state=&self.performance_state.states[self.selections[note.0.index].snapshot];let range=prepared.range(properties.pitch.key(),Trigger::Attack);
        let mut required=selected.checked_add(1).ok_or(Error::Capacity)?;let mut from=range.start;
        while from<range.end{let until=prepared.group_end(from,range.end);let mut matching=super::Matching::new(prepared.active_ranges(from..until,state.articulation));
            let first=self.next_frontier_candidate(note,parent,stage,channel,&mut matching)?;
            let choice=prepared.choose(first,address,properties.pitch.key(),&generation.sequences)?;
            required=required.checked_add(usize::from(choice.is_some())).ok_or(Error::Capacity)?;from=until;
        }
        if required>self.decisions.available(){return Err(Error::Capacity);}
        // Pure full preflight above completed. No callbacks/due work run below;
        // sequence cells are disjoint and choose is deterministic without commit.
        let mut from=range.start;
        while from<range.end{
            let generation=self.plans.get(plan.0).ok_or(Error::StaleHandle)?;let prepared=&generation.prepared;
            let state=&self.performance_state.states[self.selections[note.0.index].snapshot];
            let until=prepared.group_end(from,range.end);let mut matching=super::Matching::new(prepared.active_ranges(from..until,state.articulation));
            let first=self.next_frontier_candidate(note,parent,stage,channel,&mut matching)?;
            let choice=prepared.choose(first,address,properties.pitch.key(),&generation.sequences)?;
            if let Some(choice)=choice{self.record_source_decision(note,crate::variation::DecisionValue::Take(choice.take))?;self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?.sequences.commit(choice);}
            from=until;
        }
        let children=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_owners.get(parent).ok_or(Error::InvalidInput)?.children.len();
        for ordinal in 0..children{
            let child=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_owners.get(parent).ok_or(Error::InvalidInput)?.children[ordinal];
            if self.frontier_child_chosen(note,parent,child,stage,channel,true)?{self.record_source_decision(note,crate::variation::DecisionValue::SourceOwner(child))?;}
        }
        self.record_source_decision(note,crate::variation::DecisionValue::SourceFrontier{parent,stage})?;Ok(selected)
    }
}
