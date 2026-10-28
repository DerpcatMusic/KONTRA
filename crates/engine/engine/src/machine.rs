//! A script slot's machine: the bytecode interpreter, or a front end that
//! runs its own language against the same host calls.

use kontra_vm::{CoId, Faults, HostApi, Instance, Machine, Program, Step, Text};

use crate::script::ScriptPlan;

pub(crate) enum Vm {
    Code(Instance),
    Foreign(Box<dyn Machine>),
}

impl Vm {
    /// Not real-time.
    pub(crate) fn new(script: &ScriptPlan) -> Self {
        match &script.foreign {
            Some(code) => Self::Foreign(code.machine()),
            None => Self::Code(Instance::new(&script.program, script.coroutines.max(1))),
        }
    }

    pub(crate) fn start(&mut self, entry: u32, context: u64) -> Option<CoId> {
        match self {
            Self::Code(vm) => vm.start(entry, context),
            Self::Foreign(m) => m.start(entry, context),
        }
    }

    pub(crate) fn run(&mut self, program: &Program, co: CoId, host: &mut dyn HostApi) -> Step {
        match self {
            Self::Code(vm) => vm.run(program, co, host),
            Self::Foreign(m) => m.run(co, host),
        }
    }

    pub(crate) fn cancel(&mut self, co: CoId) {
        match self {
            Self::Code(vm) => vm.cancel(co),
            Self::Foreign(m) => m.cancel(co),
        }
    }

    pub(crate) fn context(&self, co: CoId) -> Option<u64> {
        match self {
            Self::Code(vm) => vm.context(co),
            Self::Foreign(m) => m.context(co),
        }
    }

    pub(crate) fn spawned(&mut self) -> Option<(CoId, u64)> {
        match self {
            Self::Code(_) => None,
            Self::Foreign(m) => m.spawned(),
        }
    }

    pub(crate) fn faults(&self) -> Faults {
        match self {
            Self::Code(vm) => vm.faults(),
            Self::Foreign(m) => m.faults(),
        }
    }

    pub(crate) fn cell(&self, n: usize) -> Option<i64> {
        match self {
            Self::Code(vm) => vm.cell(n),
            Self::Foreign(_) => None,
        }
    }

    pub(crate) fn set_cell(&mut self, n: usize, value: i64) {
        if let Self::Code(vm) = self {
            vm.set_cell(n, value);
        }
    }

    pub(crate) fn set_text(&mut self, n: usize, value: Text) {
        if let Self::Code(vm) = self {
            vm.set_text(n, value);
        }
    }
}
