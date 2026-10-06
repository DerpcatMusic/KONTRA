use crate::{
    Error, Expression, Input, NoteId, NoteOrigin, NotePitch, PlanId, ReleaseReserve, ReleaseStatus,
    Runtime, Trigger,
};

#[derive(Clone, Copy)]
struct Selection {
    plan: PlanId,
    note_pitch: NotePitch,
    velocity: f64,
    address: crate::ChannelAddress,
    trigger: Trigger,
    snapshot: usize,
    groups: Option<(usize, bool)>,
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
        let (on_note, on_release) = (prepared.note_program, prepared.release_program);
        if on_note.is_none() && on_release.is_none() {
            return self.select(
                NoteOrigin::Input(input, expression, index),
                pitch,
                velocity,
                0,
            );
        }
        let callbacks = usize::from(on_note.is_some()) + usize::from(on_release.is_some());
        if self.behaviors.available() < callbacks {
            return Err(Error::Capacity);
        }
        let note = if on_note.is_some() {
            self.note_on_pitched_in(performance, input, pitch, velocity, expression)?
        } else {
            self.select(
                NoteOrigin::Input(input, expression, index),
                pitch,
                velocity,
                0,
            )?
        };
        if on_release.is_some() {
            self.behaviors.reserve(1);
            self.release_times[note.0.index].release_behavior = true;
        }
        if let Some(program) = on_note {
            self.start_behavior(note, program)
                .expect("preflighted native behavior admission");
        }
        Ok(note)
    }

    pub(crate) fn trigger_child(
        &mut self,
        parent: NoteId,
        pitch: NotePitch,
        velocity: f64,
        linked: bool,
        inheritance: crate::Inheritance,
        offset_micros: u32,
    ) -> Result<NoteId, Error> {
        self.select(
            NoteOrigin::Child(parent, linked, inheritance),
            pitch,
            velocity,
            offset_micros,
        )
    }

    fn select(
        &mut self,
        origin: NoteOrigin,
        note_pitch: NotePitch,
        velocity: f64,
        offset_micros: u32,
    ) -> Result<NoteId, Error> {
        self.apply_due();
        if !note_pitch.valid() || !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) {
            return Err(Error::InvalidInput);
        }
        let plan = match origin {
            NoteOrigin::Input(..) => self.active_plan,
            NoteOrigin::Child(parent, ..) => {
                self.notes.get(parent.0).ok_or(Error::StaleHandle)?.plan
            }
        };
        let pitch = match origin {
            NoteOrigin::Input(_, expression, _) => crate::pitch::PitchRange::constant(
                self.project_expression(self.modulation_plan(plan), expression, None)?
                    .ratio,
            ),
            NoteOrigin::Child(_, _, crate::Inheritance::Independent) => {
                crate::pitch::PitchRange::constant(
                    self.project_expression(
                        self.modulation_plan(plan),
                        Expression::default(),
                        None,
                    )?
                    .ratio,
                )
            }
            NoteOrigin::Child(parent, _, inheritance) => {
                let owner = self.notes.get(parent.0).unwrap().expression;
                self.pitch_range(owner, inheritance == crate::Inheritance::Linked)?
            }
        };
        let address = match origin {
            NoteOrigin::Input(input, ..) => input.channel_address(),
            NoteOrigin::Child(parent, ..) => self.notes.get(parent.0).unwrap().address,
        };
        let performance = match origin {
            NoteOrigin::Input(_, _, performance) => performance,
            NoteOrigin::Child(parent, ..) => self.selections[parent.0.index].performance,
        };
        let snapshot = self.performance_state.current[performance];
        let release = self.preflight_attack(
            Selection {
                plan,
                note_pitch,
                velocity,
                address,
                trigger: Trigger::Attack,
                snapshot,
                groups: match origin {
                    NoteOrigin::Input(..) => None,
                    NoteOrigin::Child(parent, ..) => Some((parent.0.index, false)),
                },
            },
            pitch,
        )?;
        let note = match origin {
            NoteOrigin::Input(input, expression, performance) => self.note_on_pitched_in(
                self.performance(performance).unwrap(),
                input,
                note_pitch,
                velocity,
                expression,
            )?,
            NoteOrigin::Child(parent, linked, inheritance) => {
                self.child_pitched(parent, note_pitch, velocity, linked, inheritance)?
            }
        };
        self.note_events[note.0.index].source_offset_micros = offset_micros;
        self.commit_attack(note, release, snapshot);
        Ok(note)
    }

    /// Commit a pending mapped attack exactly once on its existing logical note.
    /// Uses the original plan and captured onset selection, never a substitute child.
    pub fn forward_attack(&mut self, note: NoteId) -> Result<bool, Error> {
        self.apply_due();
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if n.attack != crate::AttackStatus::Pending {
            return Ok(false);
        }
        if !n.gate() || !n.key_down() {
            return Err(Error::ClosedNote);
        }
        let snapshot = self.selections[note.0.index].snapshot;
        let event = self.note_events[note.0.index].current;
        let selection = Selection {
            plan: n.plan,
            note_pitch: event.pitch,
            velocity: event.velocity,
            address: n.address,
            trigger: Trigger::Attack,
            snapshot,
            groups: Some((note.0.index, false)),
        };
        let pitch = self.pitch_range(n.expression, true)?;
        let release = self.preflight_attack(selection, pitch)?;
        let n = self.notes.get_mut(note.0).unwrap();
        n.pitch = event.pitch;
        n.velocity = event.velocity;
        self.commit_attack(note, release, snapshot);
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
        self.check_selection_capacity(required)?;
        Ok(release)
    }

    fn commit_attack(&mut self, note: NoteId, release: ReleaseReserve, snapshot: usize) {
        let n = self.notes.get_mut(note.0).unwrap();
        let (plan, key, velocity, address) = (n.plan, n.pitch.key(), n.velocity, n.address);
        n.attack = crate::AttackStatus::Forwarded;
        self.plans
            .get_mut(plan.0)
            .unwrap()
            .groups
            .commit(note.0.index);
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
        } = selection;
        let key = note_pitch.key();
        let generation = self.plans.get(plan.0).unwrap();
        let prepared = &generation.prepared;
        let range = prepared.range(key, trigger);
        let groups = groups.map(|(index, committed)| generation.groups.view(index, committed));
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
                    pitch.apply(prepared.step(c, note_pitch))?;
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

    fn commit_selection(&mut self, note: NoteId, trigger: Trigger, velocity: f64, snapshot: usize) {
        let n = self.notes.get(note.0).unwrap();
        let (plan, note_pitch, address) = (n.plan, n.pitch, n.address);
        let key = note_pitch.key();
        let range = self.plans.get(plan.0).unwrap().prepared.range(key, trigger);
        let (begin, end) = (range.start, range.end);
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
            let groups = Some(generation.groups.view(note.0.index, true));
            let mut first = matching.next_in_groups(prepared, state, velocity, groups);
            let choice = prepared
                .choose(first, address, key, &generation.sequences)
                .expect("preflighted take decision");
            let decision = choice.map(|c| self.record_take(note, trigger, c.take));
            let mut family = None;
            loop {
                let generation = self.plans.get(plan.0).unwrap();
                let prepared = &generation.prepared;
                let groups = Some(generation.groups.view(note.0.index, true));
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
                let step = prepared.step(candidate, note_pitch);
                let cursor = if trigger == Trigger::Attack {
                    r.cursor.with_offset(
                        self.note_events[note.0.index].source_offset_micros,
                        prepared.pcm[r.sample].sample_rate(),
                    )
                } else {
                    r.cursor
                };
                let family = *family.get_or_insert_with(|| {
                    let family = self
                        .create_family_for(note, trigger)
                        .expect("preflight family capacity");
                    self.families.get_mut(family.0).unwrap().decision = decision;
                    family
                });
                let voice = self
                    .admit_voice(
                        family,
                        r.sample,
                        self.now,
                        r.gain * r.velocity_curve.amplitude(velocity),
                        r.envelope,
                        cursor.with_step(step),
                    )
                    .expect("prepared and preflighted source admission");
                self.voices.get_mut(voice.0).unwrap().chain = r.chain;
                if r.chain.is_some() {
                    self.plans.get_mut(plan.0).unwrap().dsp.reset(voice.0.index);
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
                        snapshot,
                        groups: Some((note.0.index, true)),
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
