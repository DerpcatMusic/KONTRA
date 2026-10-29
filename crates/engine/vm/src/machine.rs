//! A script slot's machine: the bytecode interpreter, or a front end that
//! runs its own language against the same host calls.

use std::sync::Arc;

use crate::{CoId, Faults, Foreign, HostApi, Instance, Machine, Program, Step, Text, WidgetLook};

/// One script slot's machine.
pub enum Vm {
    Code(Instance),
    Foreign(Box<dyn Machine>),
}

impl Vm {
    /// `foreign`'s machine, else the interpreter with `coroutines` slots. Not real-time.
    #[must_use]
    pub fn new(program: &Program, coroutines: usize, foreign: Option<&Arc<dyn Foreign>>) -> Self {
        match foreign {
            Some(code) => Self::Foreign(code.machine()),
            None => Self::Code(Instance::new(program, coroutines.max(1))),
        }
    }

    pub fn start(&mut self, entry: u32, context: u64) -> Option<CoId> {
        match self {
            Self::Code(vm) => vm.start(entry, context),
            Self::Foreign(m) => m.start(entry, context),
        }
    }

    pub fn run(&mut self, program: &Program, co: CoId, host: &mut dyn HostApi) -> Step {
        match self {
            Self::Code(vm) => vm.run(program, co, host),
            Self::Foreign(m) => m.run(co, host),
        }
    }

    pub fn cancel(&mut self, co: CoId) {
        match self {
            Self::Code(vm) => vm.cancel(co),
            Self::Foreign(m) => m.cancel(co),
        }
    }

    #[must_use]
    pub fn context(&self, co: CoId) -> Option<u64> {
        match self {
            Self::Code(vm) => vm.context(co),
            Self::Foreign(m) => m.context(co),
        }
    }

    /// A foreign machine's control `id` moves to `value` ([`Machine::control`]);
    /// the interpreter has none.
    pub fn control(&mut self, id: i32, value: f64, context: u64) -> Option<CoId> {
        match self {
            Self::Code(_) => None,
            Self::Foreign(m) => m.control(id, value, context),
        }
    }

    /// A foreign machine's waits are never stopped: always epoch 0.
    #[must_use]
    pub fn wait_epoch(&self, co: CoId) -> Option<u32> {
        match self {
            Self::Code(vm) => vm.wait_epoch(co),
            Self::Foreign(_) => Some(0),
        }
    }

    /// `stop_wait` on the callback with this context; the epoch to resume it with.
    pub fn stop_wait(&mut self, matches: impl Fn(u64) -> bool, rest: bool) -> Option<(CoId, u32)> {
        match self {
            Self::Code(vm) => {
                let co = vm.find(matches)?;
                Some((co, vm.stop_wait(co, rest)?))
            }
            Self::Foreign(_) => None,
        }
    }

    pub fn spawned(&mut self) -> Option<(CoId, u64)> {
        match self {
            Self::Code(_) => None,
            Self::Foreign(m) => m.spawned(),
        }
    }

    #[must_use]
    pub fn faults(&self) -> Faults {
        match self {
            Self::Code(vm) => vm.faults(),
            Self::Foreign(m) => m.faults(),
        }
    }

    #[must_use]
    pub fn cell(&self, n: usize) -> Option<i64> {
        match self {
            Self::Code(vm) => vm.cell(n),
            Self::Foreign(_) => None,
        }
    }

    pub fn set_cell(&mut self, n: usize, value: i64) {
        if let Self::Code(vm) = self {
            vm.set_cell(n, value);
        }
    }

    pub fn set_text(&mut self, n: usize, value: Text) {
        if let Self::Code(vm) = self {
            vm.set_text(n, value);
        }
    }

    /// As [`Machine::stage_widget`]; the interpreter has no own widgets.
    pub fn stage_widget(&mut self, n: u32, value: f64) {
        if let Self::Foreign(m) = self {
            m.stage_widget(n, value);
        }
    }

    #[must_use]
    pub fn widget(&self, n: u32) -> Option<WidgetLook> {
        match self {
            Self::Code(_) => None,
            Self::Foreign(m) => m.widget(n),
        }
    }

    /// As [`Machine::save_state`].
    pub fn save_state(&mut self, out: &mut [u8]) -> Option<usize> {
        match self {
            Self::Code(_) => None,
            Self::Foreign(m) => m.save_state(out),
        }
    }

    pub fn widget_text(&self, n: u32, out: &mut Text) -> bool {
        match self {
            Self::Code(_) => false,
            Self::Foreign(m) => m.widget_text(n, out),
        }
    }

    #[must_use]
    pub fn text(&self, n: usize) -> Option<&Text> {
        match self {
            Self::Code(vm) => vm.text(n),
            Self::Foreign(_) => None,
        }
    }
}
