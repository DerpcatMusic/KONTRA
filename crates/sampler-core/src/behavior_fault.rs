//! Bounded fault evidence. Captured before retiring a callback; formatted off audio.
use crate::{BehaviorId, Instruction, Outcome, Runtime};

pub const FAULT_REGISTERS: usize = 32;
pub const FAULT_CALLERS: usize = 8;

/// A frontend source site. No source text or owned strings cross the audio lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceSite {
    pub pc: u32,
    pub offset: u32,
    pub line: u32,
    pub column: u32,
    pub builtin: Option<&'static str>,
}

#[derive(Clone, Copy, Debug)]
pub struct BehaviorFault {
    pub pc: usize,
    pub source_digest: Option<[u8; 32]>,
    pub source_slot: Option<u8>,
    pub callback: Option<&'static str>,
    pub callback_id: i32,
    pub ui_id: i32,
    pub site: Option<SourceSite>,
    pub instruction: Option<Instruction>,
    /// Raw VM register bits (real registers contain IEEE-754 bits).
    pub registers: [i64; FAULT_REGISTERS],
    pub register_count: usize,
    pub register_base: u16,
    pub registers_omitted: usize,
    /// Innermost caller first; each site points at the call, not the return.
    pub callers: [Option<SourceSite>; FAULT_CALLERS],
    pub callers_omitted: usize,
    pub sample_time: u64,
}

impl Runtime {
    /// Only reads retained preallocated state. Safe with completion backpressure.
    pub fn behavior_fault(&self, id: BehaviorId) -> Option<BehaviorFault> {
        let c = self.behaviors.get(id.0)?;
        let outcome = c.outcome?;
        if !matches!(outcome, Outcome::Fault(_) | Outcome::FuelExhausted) {
            return None;
        }
        let plan = &self
            .plans
            .get(self.behavior_plan(c.owner).ok()?.0)?
            .prepared;
        let program = plan.programs.get(c.program)?;
        let pc = if matches!(outcome, Outcome::Fault(_)) {
            c.last_pc
        } else {
            c.pc
        };
        let site = |pc: usize| {
            let end = program
                .source_sites
                .partition_point(|site| site.pc as usize <= pc);
            end.checked_sub(1).map(|index| program.source_sites[index])
        };
        let instruction = program.code.get(pc).copied();
        // Deep expressions can use high registers. Keep the operand window,
        // rather than silently losing the failing input to the snapshot cap.
        let register_base = match instruction {
            Some(
                Instruction::ReadScriptArray { index, .. }
                | Instruction::WriteScriptArray { index, .. },
            ) => index,
            Some(
                Instruction::WaitMicros { local }
                | Instruction::WaitLocal { local }
                | Instruction::MicrosToFrames { local },
            ) => local,
            Some(Instruction::PlayMidi { key, velocity, .. }) => key.min(velocity),
            Some(Instruction::Op(crate::Op::EngineParameter { address, local, .. })) => {
                address.min(local)
            }
            Some(Instruction::Op(
                crate::Op::Store { key, local, .. } | crate::Op::SharedStore { key, local, .. },
            )) => key.min(local),
            Some(Instruction::Op(
                crate::Op::Waveform { args, local, .. } | crate::Op::Midi { args, local, .. },
            )) => args.min(local),
            _ => 0,
        };
        let count = program
            .locals
            .saturating_sub(usize::from(register_base))
            .min(FAULT_REGISTERS);
        let base = id.0.index * self.behavior_stride + usize::from(register_base);
        let mut registers = [0; FAULT_REGISTERS];
        registers[..count].copy_from_slice(self.behavior_locals.get(base..base + count)?);
        let mut callers = [None; FAULT_CALLERS];
        for (out, &pc) in callers.iter_mut().zip(c.frames.callers().iter().rev()) {
            *out = site(pc.saturating_sub(1) as usize);
        }
        Some(BehaviorFault {
            pc,
            source_digest: program.source_digest,
            source_slot: u8::try_from(program.source_slot).ok(),
            callback: program.diagnostic_callback,
            callback_id: c.callback_id,
            ui_id: program.ui_id,
            site: site(pc),
            instruction,
            registers,
            register_count: count,
            registers_omitted: program.locals.saturating_sub(count),
            callers,
            callers_omitted: c.frames.callers().len().saturating_sub(FAULT_CALLERS),
            sample_time: self.now(),
        })
    }
}
