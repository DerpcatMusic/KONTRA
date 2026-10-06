use crate::{Error, Prepared, WaitLifetime};

/// One module boundary, including event kinds with no callback. Program indices
/// refer to the prepared table; script-instance state ownership remains explicit
/// on each program and is independent of this routing position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stage {
    pub note: Option<usize>,
    pub release: Option<usize>,
    pub controller: Option<usize>,
}

impl Prepared {
    /// Install exact module positions. Downstream release routing is still under
    /// implementation, so release bindings currently fail outside stage zero.
    pub fn with_stages(mut self, stages: Vec<Stage>) -> Result<Self, Error> {
        for (index, stage) in stages.iter().enumerate() {
            if index != 0 && stage.release.is_some() {
                return Err(Error::InvalidInput);
            }
            if stage
                .note
                .is_some_and(|id| self.programs.get(id).is_none_or(|p| p.requires_controller))
                || stage.release.is_some_and(|id| {
                    self.programs.get(id).is_none_or(|p| {
                        p.requires_controller || p.wait_lifetime != WaitLifetime::Callback
                    })
                })
                || stage.controller.is_some_and(|id| {
                    self.programs.get(id).is_none_or(|p| {
                        p.requires_note || p.wait_lifetime != WaitLifetime::Callback
                    })
                })
            {
                return Err(Error::InvalidInput);
            }
        }
        self.stages = stages.into_boxed_slice();
        Ok(self)
    }

    pub fn stages(&self) -> &[Stage] {
        &self.stages
    }
}

impl crate::Runtime {
    pub(super) fn release_note_callbacks(&mut self, note: crate::NoteId) {
        let count = std::mem::take(&mut self.note_events[note.0.index].pending_callbacks);
        self.behaviors.unreserve(count);
    }

    pub(super) fn begin_note_stages(&mut self, note: crate::NoteId, entry: usize) {
        let plan = self.notes.get(note.0).unwrap().plan;
        let stages = &self.plans.get(plan.0).unwrap().prepared.stages;
        let count = stages[entry..]
            .iter()
            .filter(|stage| stage.note.is_some())
            .count();
        let (next, program) = stages
            .iter()
            .enumerate()
            .skip(entry)
            .find_map(|(i, s)| s.note.map(|p| (i, p)))
            .unwrap();
        self.behaviors.reserve(count);
        self.note_events[note.0.index].pending_callbacks = count;
        self.note_events[note.0.index].routed = true;
        self.project_note(note, entry, next);
        self.start_note_stage(note, next, program);
    }

    pub(super) fn project_note(&mut self, note: crate::NoteId, from: usize, through: usize) {
        let plan = self.notes.get(note.0).unwrap().plan;
        let generation = self.plans.get_mut(plan.0).unwrap();
        generation.projections.forward(note.0.index, from, through);
        generation.groups.forward(note.0.index, from, through);
    }

    fn start_note_stage(&mut self, note: crate::NoteId, stage: usize, program: usize) {
        self.note_events[note.0.index].pending_callbacks -= 1;
        self.behaviors.unreserve(1);
        self.start_note_context(note, program, Some(stage))
            .expect("reserved note stage callback");
    }

    pub(super) fn forward_note_stage(
        &mut self,
        note: crate::NoteId,
        stage: usize,
    ) -> Result<bool, Error> {
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let plan = n.plan;
        let generation = self.plans.get(plan.0).unwrap();
        if generation
            .projections
            .get(note.0.index, stage)?
            .properties
            .is_none()
        {
            return Err(Error::InvalidInput);
        }
        if generation.projections.get(note.0.index, stage)?.forwarded
            || n.attack != crate::AttackStatus::Pending
        {
            return Ok(false);
        }
        if !n.gate() || !n.key_down() {
            return Err(Error::ClosedNote);
        }
        let stages = &generation.prepared.stages;
        let next = stages
            .iter()
            .enumerate()
            .skip(stage + 1)
            .find_map(|(i, s)| s.note.map(|p| (i, p)));
        let end = stages.len();
        if let Some((next, program)) = next {
            self.project_note(note, stage, next);
            self.start_note_stage(note, next, program);
        } else {
            self.commit_note_attack(note, stage)?;
            self.project_note(note, stage, end);
            self.plans
                .get_mut(plan.0)
                .unwrap()
                .projections
                .get_mut(note.0.index, end)?
                .forwarded = true;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Instruction as I, Program};

    #[test]
    fn bindings_validate_callback_contexts_and_preserve_other_event_kinds() {
        let prepared = || {
            Prepared::new(48000, vec![], vec![], 0)
                .unwrap()
                .with_programs(
                    vec![
                        Program::new(vec![I::ReadVelocity7 { local: 0 }]).unwrap(),
                        Program::new(vec![I::ReadVelocity7 { local: 0 }])
                            .unwrap()
                            .with_wait_lifetime(WaitLifetime::Callback),
                        Program::new(vec![I::ReadControllerNumber { local: 0 }])
                            .unwrap()
                            .with_wait_lifetime(WaitLifetime::Callback),
                    ],
                    Some(0),
                )
                .unwrap()
                .with_release_program(1)
                .unwrap()
        };
        let plan = prepared().with_controller_programs(vec![2]).unwrap();
        assert_eq!(
            plan.stages(),
            &[Stage {
                note: Some(0),
                release: Some(1),
                controller: Some(2)
            }]
        );
        let plan = plan.with_controller_programs(vec![]).unwrap();
        assert_eq!(
            plan.stages(),
            &[Stage {
                note: Some(0),
                release: Some(1),
                controller: None
            }]
        );
        for stage in [
            Stage {
                note: Some(2),
                ..Stage::default()
            },
            Stage {
                release: Some(0),
                ..Stage::default()
            },
            Stage {
                controller: Some(1),
                ..Stage::default()
            },
            Stage {
                controller: Some(3),
                ..Stage::default()
            },
        ] {
            assert!(matches!(
                prepared().with_stages(vec![stage]),
                Err(Error::InvalidInput)
            ));
        }
        assert!(matches!(
            prepared().with_stages(vec![
                Stage::default(),
                Stage {
                    release: Some(1),
                    ..Stage::default()
                }
            ]),
            Err(Error::InvalidInput)
        ));
    }
}
