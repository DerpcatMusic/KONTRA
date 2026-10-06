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
    /// Install exact module positions. Downstream note/release routing is still
    /// under implementation, so those bindings currently fail outside stage zero.
    pub fn with_stages(mut self, stages: Vec<Stage>) -> Result<Self, Error> {
        for (index, stage) in stages.iter().enumerate() {
            if index != 0 && (stage.note.is_some() || stage.release.is_some()) {
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
        // Downstream note/release execution must not silently use stage zero's
        // projection until the routed note service is implemented.
        for stage in [
            Stage {
                note: Some(0),
                ..Stage::default()
            },
            Stage {
                release: Some(1),
                ..Stage::default()
            },
        ] {
            assert!(matches!(
                prepared().with_stages(vec![Stage::default(), stage]),
                Err(Error::InvalidInput)
            ));
        }
    }
}
