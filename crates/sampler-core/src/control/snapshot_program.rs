//! Immutable callback safety computed during lowering, never on audio.
use crate::{Error, Instruction as I, Op, Program};
use std::ops::Range;

pub(crate) fn snapshot_callback_safe(
    code: &[I],
    ranges: &[Range<usize>],
    services: &[u16],
) -> bool {
    let target = |at| at == code.len() || ranges.iter().any(|range| range.contains(&at));
    ranges.iter().all(|range| {
        code[range.clone()].iter().all(|op| match *op {
            I::Jump { target: at } | I::JumpIfZero { target: at, .. } => target(at),
            I::LoopGuard { exit: at, .. } => target(at as usize),
            I::Op(Op::Call { target: at }) => target(at as usize),
            I::LoopReset
            | I::SetLocal { .. }
            | I::AddLocal { .. }
            | I::Binary32 { .. }
            | I::Unary32 { .. }
            | I::CompareLocal { .. }
            | I::ReadScriptCell { .. }
            | I::WriteScriptCell { .. }
            | I::ReadScriptArray { .. }
            | I::WriteScriptArray { .. }
            | I::ReadControl { .. }
            | I::WriteControl { .. }
            | I::ReadGroupCount { .. }
            | I::ReadCallbackId { .. }
            | I::ReadInputController { .. }
            | I::ControllerToScript { .. }
            | I::ControllerFromScript { .. }
            | I::ControllerToMidi7 { .. }
            | I::ControllerFromMidi7 { .. }
            | I::MicrosToFrames { .. }
            | I::WaitLocal { .. }
            | I::WaitMicros { .. }
            | I::Wait(_)
            | I::End => true,
            I::Op(Op::Emit { service, .. }) => services.contains(&service),
            I::Op(
                Op::Real { .. }
                | Op::RealUnary { .. }
                | Op::CompareReal { .. }
                | Op::IntegerToReal { .. }
                | Op::RealToInteger { .. }
                | Op::Integer { .. }
                | Op::Random { .. }
                | Op::Return
                | Op::TextClear { .. }
                | Op::TextAppend { .. }
                | Op::CompareText { .. }
                | Op::TextFind { .. }
                | Op::TextIndex { .. }
                | Op::Store { .. }
                | Op::SharedStore { .. }
                | Op::ControlAt { .. }
                | Op::ReadCallbackUiId { .. }
                | Op::ReadHost { .. }
                | Op::ReadClock { .. }
                | Op::ReadTimer { .. }
                | Op::ResetTimer
                | Op::TextProperty { .. }
                | Op::TimeConversion { .. },
            ) => true,
            _ => false,
        }) && range.clone().all(|at| {
            // A range ending on fallthrough cannot hide an unsafe next instruction.
            at + 1 != range.end
                || target(at + 1)
                || matches!(code[at], I::End | I::Jump { .. } | I::Op(Op::Return))
        })
    })
}

impl Program {
    /// A frontend elided or approximated unsupported source semantics within
    /// this entry's reachable body. This denial is monotone across clones and
    /// later entry/service configuration; it never changes runtime bytecode.
    pub fn without_snapshot_overlay(mut self) -> Self {
        self.snapshot_frontend_opaque = true;
        self.snapshot_overlay_safe = false;
        self.snapshot_listener_driver = None;
        self
    }

    /// Frontend-proven presentation-only effect service IDs. Setting the table
    /// invalidates safety until with_entry proves the complete callback ranges.
    /// Runtime service numbers have no universal semantics outside the frontend.
    pub fn with_snapshot_overlay_services(mut self, services: &[u16]) -> Self {
        self.snapshot_overlay_services = services.into();
        self.snapshot_overlay_safe = false;
        self
    }

    /// Mark a compiler-generated listener driver after proving its code can only
    /// read its private period, wait, and dispatch one immutable body target.
    pub fn with_snapshot_listener_driver(mut self) -> Result<Self, Error> {
        if self.snapshot_frontend_opaque {
            return Err(Error::InvalidInput);
        }
        let mut body = None;
        let mut waits = false;
        for op in self.code.iter() {
            match *op {
                I::StartProgram { program } if body.is_none() => body = Some(program),
                I::Wait(_) | I::WaitLocal { .. } => waits = true,
                I::SetLocal { .. }
                | I::AddLocal { .. }
                | I::Binary32 { .. }
                | I::Unary32 { .. }
                | I::CompareLocal { .. }
                | I::Jump { .. }
                | I::JumpIfZero { .. }
                | I::MicrosToFrames { .. }
                | I::Op(
                    Op::Store { write: false, .. } | Op::ReadHost { .. } | Op::Integer { .. },
                ) => {}
                _ => return Err(Error::InvalidInput),
            }
        }
        if !waits || body.is_none() || self.entry != 0 {
            return Err(Error::InvalidInput);
        }
        self.snapshot_listener_driver = body;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_snapshot_denial_survives_cloning_entry_and_service_configuration() {
        let program = Program::new(vec![I::End]).unwrap();
        let code = program.code.clone();
        let program = program.without_snapshot_overlay();
        assert!(std::sync::Arc::ptr_eq(&code, &program.code));
        assert!(!program.snapshot_overlay_safe);
        assert!(
            !program
                .clone()
                .with_entry(0, &[0..1], 0)
                .unwrap()
                .snapshot_overlay_safe
        );
        assert!(
            !program
                .with_snapshot_overlay_services(&[])
                .with_entry(0, &[0..1], 0)
                .unwrap()
                .snapshot_overlay_safe
        );
        let driver = Program::new(vec![
            I::StartProgram { program: 2 },
            I::Wait(480),
            I::Jump { target: 0 },
        ])
        .unwrap()
        .with_snapshot_listener_driver()
        .unwrap()
        .without_snapshot_overlay();
        assert_eq!(driver.snapshot_listener_driver, None);
        assert!(matches!(
            driver.with_snapshot_listener_driver(),
            Err(Error::InvalidInput)
        ));
    }

    #[test]
    fn callback_ranges_prove_calls_jumps_fallthrough_and_service_allowlist() {
        let code = vec![
            I::Op(Op::Call { target: 2 }),
            I::End,
            I::Op(Op::Purge {
                group: 0,
                local: 1,
                write: true,
            }),
            I::Op(Op::Return),
        ];
        let program = Program::new(code).unwrap();
        assert!(
            !program
                .clone()
                .with_entry(0, &[0..2], 0)
                .unwrap()
                .snapshot_overlay_safe
        );
        assert!(
            !program
                .with_entry(0, &[0..2, 2..4], 0)
                .unwrap()
                .snapshot_overlay_safe
        );
        let program = Program::new(vec![
            I::SetLocal { local: 0, value: 0 },
            I::End,
            I::Op(Op::Purge {
                group: 0,
                local: 1,
                write: true,
            }),
        ])
        .unwrap();
        assert!(
            program
                .clone()
                .with_entry(0, &[0..2], 0)
                .unwrap()
                .snapshot_overlay_safe
        );
        assert!(
            !program
                .with_entry(0, &[0..1], 0)
                .unwrap()
                .snapshot_overlay_safe
        );
        let program = Program::new(vec![
            I::Op(Op::Emit {
                service: 7,
                args: 0,
                count: 0,
                text: None,
            }),
            I::End,
        ])
        .unwrap();
        assert!(!program.snapshot_overlay_safe);
        let program = program.with_snapshot_overlay_services(&[7]);
        assert!(!program.snapshot_overlay_safe);
        assert!(
            program
                .with_entry(0, &[0..2], 0)
                .unwrap()
                .snapshot_overlay_safe
        );
    }

    #[test]
    fn listener_marker_is_validated_offset_and_invalidated_by_entry_change() {
        let program = Program::new(vec![
            I::Wait(480),
            I::StartProgram { program: 2 },
            I::Jump { target: 0 },
        ])
        .unwrap()
        .with_snapshot_listener_driver()
        .unwrap();
        assert_eq!(program.snapshot_listener_driver, Some(2));
        let program = program.with_program_base(3);
        assert_eq!(program.snapshot_listener_driver, Some(5));
        assert!(matches!(program.code[1], I::StartProgram { program: 5 }));
        assert_eq!(
            program
                .with_entry(0, &[0..3], 0)
                .unwrap()
                .snapshot_listener_driver,
            None
        );
        assert!(
            Program::new(vec![
                I::Wait(480),
                I::WriteScriptCell { cell: 0, local: 0 },
                I::StartProgram { program: 0 },
                I::Jump { target: 0 }
            ])
            .unwrap()
            .with_snapshot_listener_driver()
            .is_err()
        );
    }
}
