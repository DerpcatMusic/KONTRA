use crate::{
    Error, Expression, Input, NoteId, NoteOrigin, NotePitch, PlanId, ReleaseReserve, ReleaseStatus,
    Runtime, Trigger,
};

#[derive(Clone, Copy)]
struct Selection {
    plan: PlanId,
    note_pitch: NotePitch,
    velocity: f64,
    offset_micros: u32,
    address: crate::ChannelAddress,
    trigger: Trigger,
    snapshot: usize,
    groups: Option<(usize, crate::groups::GroupView)>,
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
            let note = self.note_on_pitched_in(performance, input, pitch, velocity, expression)?;
            self.articulation_now(index, value);
            self.performance_state
                .release(self.selections[note.0.index].snapshot);
            self.selections[note.0.index].snapshot = self.performance_state.capture(index);
            self.selections[note.0.index].consumed_switch = true;
            self.notes.get_mut(note.0).unwrap().attack = crate::AttackStatus::Suppressed;
            return Ok(note);
        }
        let prepared = &self.plans.get(self.active_plan.0).unwrap().prepared;
        let on_note = prepared.stages.iter().any(|stage| stage.note.is_some());
        if !on_note {
            return self.select(
                NoteOrigin::Input(input, expression, index),
                pitch,
                velocity,
                0,
                None,
                None,
            );
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
        let note = self.note_on_pitched_in(performance, input, pitch, velocity, expression)?;
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
            NoteOrigin::Input(_, expression, _) => crate::pitch::PitchRange::constant(
                self.project_expression(self.modulation_plan(plan), expression, None)?
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
        let address = match origin {
            NoteOrigin::Input(input, ..) => input.channel_address(),
            NoteOrigin::Generated(_, address, _) => address,
            NoteOrigin::Child(parent, ..) => self.notes.get(parent.0).unwrap().address,
        };
        let performance = match origin {
            NoteOrigin::Input(_, _, performance) | NoteOrigin::Generated(_, _, performance) => {
                performance
            }
            NoteOrigin::Child(parent, ..) => self.selections[parent.0.index].performance,
        };
        if matches!(origin, NoteOrigin::Input(..))
            && self.plans.get(plan.0).unwrap().prepared.tracks_previous
        {
            self.record_previous_key(performance, note_pitch.key());
        }
        let snapshot = self.performance_state.current[performance];
        let release = if routed {
            ReleaseReserve::default()
        } else {
            self.preflight_attack(
                Selection {
                    plan,
                    note_pitch,
                    velocity,
                    address,
                    offset_micros,
                    trigger: Trigger::Attack,
                    snapshot,
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
        let note = match origin {
            NoteOrigin::Input(input, expression, performance) => self.note_on_pitched_in(
                self.performance(performance).unwrap(),
                input,
                note_pitch,
                velocity,
                expression,
            )?,
            NoteOrigin::Generated(..) => self.admit(origin, note_pitch, velocity)?,
            NoteOrigin::Child(parent, linked, inheritance) => {
                self.child_pitched(parent, note_pitch, velocity, linked, inheritance)?
            }
        };
        let event = &mut self.note_events[note.0.index];
        event.source_offset_micros = offset_micros;
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
        if routed {
            self.begin_note_stages(note, entry);
        } else if let Some(id) = defer {
            // Pending until the callback waits or ends, so it can still edit
            // the note's groups; `release` is recomputed then.
            let _ = release;
            self.deferred.push((id, note, entry));
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
            note_pitch: event.pitch,
            velocity: event.velocity,
            offset_micros: self.note_events[note.0.index].source_offset_micros,
            address: n.address,
            trigger: Trigger::Attack,
            snapshot,
            groups: Some((note.0.index, crate::groups::GroupView::Note(stage))),
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
        if let Some(log) = &mut self.selection_log {
            log.push(crate::SelectionRecord {
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
            required.voices <= rt.voices.available() + extra.voices
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
        if required.voices > self.voices.available()
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
            velocity,
            address,
            trigger,
            snapshot,
            groups,
            offset_micros,
        } = selection;
        let key = note_pitch.key();
        let generation = self.plans.get(plan.0).unwrap();
        let prepared = &generation.prepared;
        let range = prepared.range(key, trigger);
        let groups = groups.map(|(index, view)| generation.groups.view(index, view));
        let mut required = ReleaseReserve::default();
        let mut from = range.start;
        while from < range.end {
            let until = prepared.group_end(from, range.end);
            let state = &self.performance_state.states[snapshot];
            let ranges = prepared.active_ranges(from..until, state.articulation);
            let mut matching = super::Matching::new(ranges);
            let first = matching.next_in_groups(prepared, state, velocity, groups);
            let choice = prepared.choose(first, address, key, &generation.sequences)?;
            let mut candidate = first;
            let mut count = 0;
            while let Some(c) = candidate {
                if prepared.regions[c.region].take == choice.map(|c| c.take) {
                    let r = prepared.regions[c.region];
                    let step = pitch.apply(prepared.step(c, note_pitch))?;
                    let asset = &prepared.pcm[r.sample];
                    let cursor = r
                        .cursor
                        .with_offset(offset_micros, asset.sample_rate())
                        .with_step(step);
                    self.check_source_ready(asset, cursor, r.envelope)?;
                    count += 1;
                }
                candidate = matching.next_in_groups(prepared, state, velocity, groups);
            }
            required.voices += count;
            required.families += usize::from(count != 0);
            required.decisions += usize::from(choice.is_some());
            from = until;
        }
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

    /// Records gathered since the last call, oldest first.
    pub fn take_selection_records(&mut self) -> Vec<crate::SelectionRecord> {
        self.selection_log
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// Verdict for every region mapped to the note's key, mirroring the checks
    /// `commit_selection` applies in order: phase, articulation, controller
    /// condition, velocity, script group, then the round-robin take.
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
        let state = &self.performance_state.states[snapshot];
        let mask = generation
            .groups
            .view(note.0.index, crate::groups::GroupView::Committed);
        let phase = prepared.range(key, trigger);
        let all = prepared.offsets[usize::from(key)]..prepared.offsets[usize::from(key) + 1];
        let mut candidates = Vec::new();
        for index in all {
            let c = prepared.candidates[index];
            let r = prepared.regions[c.region];
            let group = prepared.region_groups.get(c.region).copied().flatten();
            let verdict = if !phase.contains(&index) {
                Some(Rejection::Trigger)
            } else if prepared.articulated
                && r.articulation.is_some_and(|a| a != state.articulation)
            {
                Some(Rejection::Articulation)
            } else if r.conditions.is_some_and(|i| {
                !prepared.conditions[i].iter().all(|c| {
                    let v = state.value(c.controller);
                    c.low <= v && v <= c.high
                })
            }) {
                Some(Rejection::Condition)
            } else if !(r.velocity_low <= velocity && velocity <= r.velocity_high) {
                Some(Rejection::Velocity)
            } else if group.is_some_and(|g| mask[g as usize / 64] & (1 << (g % 64)) == 0) {
                Some(Rejection::Group)
            } else {
                None
            };
            candidates.push((index, c, group, verdict));
        }
        // The take actually chosen per sequence is the first survivor's; others
        // of the same sequence lose to the round robin.
        let mut chosen = std::collections::BTreeMap::new();
        for (_, c, _, verdict) in &candidates {
            if verdict.is_none()
                && let Some(take) = prepared.regions[c.region].take
            {
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
                }
            })
            .collect();
        crate::SelectionRecord {
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
        let range = self.plans.get(plan.0).unwrap().prepared.range(key, trigger);
        let (begin, end) = (range.start, range.end);
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
            let mut first = matching.next_in_groups(prepared, state, velocity, groups);
            let choice = prepared
                .choose(first, address, key, &generation.sequences)
                .expect("preflighted take decision");
            let decision = choice.map(|c| self.record_take(note, trigger, c.take));
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
                    .or_else(|| matching.next_in_groups(prepared, state, velocity, groups))
                else {
                    break;
                };
                let r = prepared.regions[candidate.region];
                if r.take != choice.map(|c| c.take) {
                    continue;
                }
                let group = prepared
                    .region_groups
                    .get(candidate.region)
                    .copied()
                    .flatten();
                let step = prepared.step(candidate, note_pitch);
                let seed =
                    self.now ^ ((note.0.index as u64) << 40) ^ ((candidate.region as u64) << 20);
                let held = self.held_frames(note);
                let n = self.notes.get(note.0).unwrap();
                let inputs = crate::voice_mod::Inputs::new(
                    n,
                    self.expressions.get(n.expression.0).unwrap().value,
                    &state.controllers,
                    held,
                    self.note_params[note.0.index].mods,
                );
                let start = prepared
                    .voice_modulation
                    .start_offset(candidate.region, &inputs, seed);
                let cursor = if trigger == Trigger::Attack {
                    r.cursor.with_offset(
                        self.note_events[note.0.index].source_offset_micros,
                        prepared.pcm[r.sample].sample_rate(),
                    )
                } else {
                    r.cursor
                };
                let cursor = if start != 0 {
                    cursor.skip(start)
                } else {
                    cursor
                };
                let family = *family.get_or_insert_with(|| {
                    let family = self
                        .create_family_for(note, trigger)
                        .expect("preflight family capacity");
                    self.families.get_mut(family.0).unwrap().decision = decision;
                    family
                });
                let envelope = self
                    .plans
                    .get(plan.0)
                    .unwrap()
                    .script
                    .envelope(group, r.envelope);
                let admitted = self.admit_voice(
                    family,
                    r.sample,
                    self.now,
                    r.gain * r.velocity_curve.amplitude(velocity) * r.fade_gain(key, velocity),
                    envelope,
                    cursor.with_step(step),
                );
                // Preflight reserved this start, but a page can be evicted or
                // the pool fill between then and now. Never panic on the audio
                // thread: refuse this voice, count it, carry on.
                let Ok(voice) = admitted else {
                    self.refused_starts = self.refused_starts.saturating_add(1);
                    continue;
                };

                let state = self.voices.get_mut(voice.0).unwrap();
                state.chain = r.chain;
                state.group = group;
                self.enforce_voice_limits(plan, group, voice);
                let routed = self.plans.get(plan.0).unwrap().script.bus(group, r.bus);
                let state = self.voices.get_mut(voice.0).unwrap();
                state.bus = routed;
                if r.chain.is_some() {
                    self.plans.get_mut(plan.0).unwrap().dsp.reset(voice.0.index);
                }
                let held = self.held_frames(note);
                let n = self.notes.get(note.0).unwrap();
                let controllers = &self.performance_state.states[snapshot].controllers;
                let inputs = crate::voice_mod::Inputs::new(
                    n,
                    self.expressions.get(n.expression.0).unwrap().value,
                    controllers,
                    held,
                    self.note_params[note.0.index].mods,
                );
                let clock = crate::voice_mod::Clock {
                    rate: f64::from(self.rate),
                    tempo: self.tempo,
                    now: self.now,
                };
                let g = self.plans.get_mut(plan.0).unwrap();
                g.modulation.start(
                    &g.prepared.voice_modulation,
                    voice.0.index,
                    candidate.region,
                    &inputs,
                    clock,
                    seed,
                );
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
                        velocity,
                        address,
                        trigger,
                        offset_micros: 0,
                        snapshot,
                        groups: Some((note.0.index, crate::groups::GroupView::Committed)),
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
