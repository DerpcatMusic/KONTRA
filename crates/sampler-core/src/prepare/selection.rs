use crate::{
    Error, Expression, Input, NoteId, NoteOrigin, NotePitch, PlanId, ReleaseReserve, ReleaseStatus,
    Runtime, Trigger,
};

struct Selection {
    plan: PlanId,
    note_pitch: NotePitch,
    velocity: f64,
    address: crate::ChannelAddress,
    trigger: Trigger,
    articulation: u32,
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
            self.articulations[index] = value;
            self.selections[note.0.index].articulation = value;
            self.selections[note.0.index].consumed_switch = true;
            return Ok(note);
        }
        if let Some(program) = self
            .plans
            .get(self.active_plan.0)
            .unwrap()
            .prepared
            .note_program
        {
            if self.behaviors.available() == 0 {
                return Err(Error::Capacity);
            }
            let note = self.note_on_pitched_in(performance, input, pitch, velocity, expression)?;
            self.start_behavior(note, program)
                .expect("preflighted native behavior admission");
            Ok(note)
        } else {
            self.select(NoteOrigin::Input(input, expression, index), pitch, velocity)
        }
    }

    pub(crate) fn trigger_child(
        &mut self,
        parent: NoteId,
        pitch: NotePitch,
        velocity: f64,
        linked: bool,
        inheritance: crate::Inheritance,
    ) -> Result<NoteId, Error> {
        self.select(
            NoteOrigin::Child(parent, linked, inheritance),
            pitch,
            velocity,
        )
    }

    fn select(
        &mut self,
        origin: NoteOrigin,
        note_pitch: NotePitch,
        velocity: f64,
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
        let articulation = self.articulations[performance];
        let key = note_pitch.key();
        let attack = self.preflight_selection(
            Selection {
                plan,
                note_pitch,
                velocity,
                address,
                trigger: Trigger::Attack,
                articulation,
            },
            pitch,
        )?;
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
                    prepared.pending_articulation(trigger, articulation),
                )?;
                let generation = self.plans.get(plan.0).unwrap();
                for sequence in prepared.sequence_groups(
                    key,
                    trigger,
                    prepared.pending_articulation(trigger, articulation),
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
        if required.decisions > self.decisions.available() {
            self.reclaim_internal_notes(required.decisions);
        }
        self.check_selection_capacity(required)?;
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
                    prepared.pending_articulation(trigger, articulation),
                    prepared.release_velocity(trigger, velocity, false, None),
                ) {
                    generation
                        .sequences
                        .claim_owner(&prepared.sequences[sequence], address, key);
                }
            }
        }
        self.commit_selection(note, Trigger::Attack, velocity, articulation);
        Ok(note)
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
            articulation,
        } = selection;
        let key = note_pitch.key();
        let generation = self.plans.get(plan.0).unwrap();
        let prepared = &generation.prepared;
        let range = prepared.range(key, trigger);
        let mut required = ReleaseReserve::default();
        let mut from = range.start;
        while from < range.end {
            let until = prepared.group_end(from, range.end);
            let ranges = prepared.active_ranges(from..until, articulation);
            let choice = prepared.choose(
                ranges.clone(),
                velocity,
                address,
                key,
                &generation.sequences,
            )?;
            let mut count = 0;
            for candidate in prepared.matches(ranges, velocity, choice.map(|c| c.take)) {
                pitch.apply(prepared.step(candidate, note_pitch))?;
                count += 1;
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

    fn commit_selection(
        &mut self,
        note: NoteId,
        trigger: Trigger,
        velocity: f64,
        articulation: u32,
    ) {
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
            let ranges = prepared.active_ranges(from..until, articulation);
            let choice = prepared
                .choose(
                    ranges.clone(),
                    velocity,
                    address,
                    key,
                    &generation.sequences,
                )
                .expect("preflighted take decision");
            let decision = choice.map(|c| self.record_take(note, trigger, c.take));
            let mut family = None;
            let [common, selected] = ranges;
            for i in common.chain(selected) {
                let prepared = &self.plans.get(plan.0).unwrap().prepared;
                let candidate = prepared.candidates[i];
                let r = prepared.regions[candidate.region];
                if r.velocity_low > velocity
                    || velocity > r.velocity_high
                    || r.take != choice.map(|c| c.take)
                {
                    continue;
                }
                let step = prepared.step(candidate, note_pitch);
                let family = *family.get_or_insert_with(|| {
                    let family = self
                        .create_family_for(note, trigger)
                        .expect("preflight family capacity");
                    self.families.get_mut(family.0).unwrap().decision = decision;
                    family
                });
                self.admit_voice(
                    family,
                    r.sample,
                    self.now,
                    r.gain * velocity as f32,
                    r.envelope,
                    r.cursor.with_step(step),
                )
                .expect("prepared and preflighted source admission");
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
        let articulation = self.release_articulation(note, trigger);
        let result = if musical {
            self.pitch_range(owner, true).and_then(|pitch| {
                self.preflight_selection(
                    Selection {
                        plan,
                        note_pitch,
                        velocity,
                        address,
                        trigger,
                        articulation,
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
                self.commit_selection(note, trigger, velocity, articulation);
                ReleaseStatus::Selected
            }
        };
    }
}
