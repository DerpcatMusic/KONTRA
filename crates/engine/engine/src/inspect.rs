//! What a script did since load, for diagnostics and the parity tools.

use kontra_vm::{Action, HostApi, HostCall, Instance, Text};

use crate::Engine;

impl Engine {
    /// The last text a script printed with `message`.
    #[must_use]
    pub fn last_message(&self) -> &str {
        self.last_message.as_str()
    }

    /// Script actions this engine left unanswered, most called first.
    #[must_use]
    pub fn unhandled_actions(&self) -> Vec<(kontra_vm::Action, u32)> {
        ranked(&self.unhandled)
    }

    /// Every script action called since load, most called first.
    #[must_use]
    pub fn action_calls(&self) -> Vec<(kontra_vm::Action, u32)> {
        ranked(&self.called)
    }

    /// Whether this engine answers `action`, called once with zero arguments.
    /// Coverage tools only: the call acts on this engine (a probe engine).
    /// An action answered only for some arguments can read as unanswered.
    pub fn answers(&mut self, action: Action) -> bool {
        let before = self.unhandled.get(usize::from(action.id())).copied();
        let mut cells = [0i64; 64];
        let mut text_out = Text::EMPTY;
        self.call(HostCall {
            action,
            context: 0,
            ints: &[0; 8],
            texts: &[Text::EMPTY; 4],
            cells: &mut cells,
            text_out: &mut text_out,
        });
        self.unhandled.get(usize::from(action.id())).copied() == before
    }

    /// A script variable cell, for inspection.
    #[must_use]
    pub fn script_cell(&self, n: usize) -> Option<i64> {
        self.vm.as_ref()?.cell(n)
    }

    /// Runtime faults of the script, counted since load.
    #[must_use]
    pub fn script_faults(&self) -> Option<kontra_vm::Faults> {
        self.vm.as_ref().map(Instance::faults)
    }
}

fn ranked(counts: &[u32]) -> Vec<(kontra_vm::Action, u32)> {
    let mut out: Vec<_> = kontra_vm::Action::all()
        .iter()
        .zip(counts)
        .filter(|(_, n)| **n > 0)
        .map(|(a, n)| (*a, *n))
        .collect();
    out.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    out
}
