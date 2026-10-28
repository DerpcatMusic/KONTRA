//! Running script callbacks, slot after slot. A note passes each slot's note
//! callback in turn, moving on when the callback returns or first waits; a
//! note a callback plays passes only the slots after it, and the notes
//! through every slot start when the callback that let them through returns.

use std::sync::Arc;

use kontra_time::SampleTime;
use kontra_vm::Step;

use crate::ledger::NoteRef;
use crate::queue::Work;
use crate::script::{Kind, Running, ScriptPlan, micros_to_frames};
use crate::{Engine, Trigger};

impl Engine {
    /// The plan of the slot whose callback runs.
    pub(crate) fn script(&self) -> Option<&ScriptPlan> {
        self.plan.scripts.get(usize::from(self.running.slot))
    }

    /// Runs slot `slot`'s callback at `entry` now. Notes it let through or
    /// played start before this returns.
    pub(crate) fn run_callback(&mut self, slot: u8, entry: u32, mut running: Running) -> Running {
        self.callbacks_started = (self.callbacks_started + 1) & 0x00FF_FFFF;
        running.callback_id = self.callbacks_started;
        running.slot = slot;
        let outer = std::mem::replace(&mut self.running, running);
        let mark = self.forward.len();
        if let Some(mut vm) = self.take_vm(slot) {
            if let Some(co) = vm.start(entry, running.context()) {
                self.step_script(slot, &mut vm, co);
            }
            self.put_vm(slot, vm);
        }
        let ran = std::mem::replace(&mut self.running, outer);
        self.forward_from(mark);
        self.start_pending();
        ran
    }

    /// A waiting callback's time has come.
    pub(crate) fn resume(&mut self, slot: u8, co: kontra_vm::CoId) {
        let mark = self.forward.len();
        if let Some(mut vm) = self.take_vm(slot) {
            self.running = Running {
                slot,
                ..Running::resumed(vm.context(co).unwrap_or(0))
            };
            self.step_script(slot, &mut vm, co);
            self.put_vm(slot, vm);
        }
        self.running = Running::IDLE;
        self.forward_from(mark);
        self.start_pending();
    }

    fn take_vm(&mut self, slot: u8) -> Option<kontra_vm::Instance> {
        self.vms.get_mut(usize::from(slot))?.take()
    }

    fn put_vm(&mut self, slot: u8, vm: kontra_vm::Instance) {
        if let Some(cell) = self.vms.get_mut(usize::from(slot)) {
            *cell = Some(vm);
        }
    }

    fn step_script(&mut self, slot: u8, vm: &mut kontra_vm::Instance, co: kontra_vm::CoId) {
        let plan = Arc::clone(&self.plan);
        let Some(script) = plan.scripts.get(usize::from(slot)) else {
            return;
        };
        if let Step::Wait(micros) = vm.run(&script.program, co, self) {
            let at = SampleTime(self.now.0 + micros_to_frames(micros as i64, self.rate).max(1));
            if !self.queue.push(at, Work::Resume(slot, co)) {
                self.stats.events_refused += 1;
                vm.cancel(co);
            }
        }
    }

    /// Passes the notes played since `mark` through the slots after the one that played them.
    fn forward_from(&mut self, mark: usize) {
        let mut i = mark;
        while let Some(&note) = self.forward.get(i) {
            self.pass_note(note);
            i += 1;
        }
        self.forward.truncate(mark);
    }

    /// Runs the note callbacks `note` has still to pass, while no slot ignores
    /// it, and queues it to start: before the last slot's callback runs, so
    /// it starts ahead of the notes that callback plays.
    pub(crate) fn pass_note(&mut self, note: NoteRef) {
        let plan = Arc::clone(&self.plan);
        let mut queued = false;
        while let Some(n) = self.notes.get_mut(note).filter(|n| !n.ignored) {
            let slot = n.pass;
            let Some(script) = plan.scripts.get(usize::from(slot)) else {
                break;
            };
            n.pass = slot.saturating_add(1);
            if usize::from(n.pass) == plan.scripts.len() {
                queued = self.queue_start(note);
            }
            if let Some(entry) = script.callbacks.note {
                self.run_callback(slot, entry, Running::new(Kind::Note, Some(note)));
            }
        }
        if !queued {
            self.queue_start(note);
        }
    }

    fn queue_start(&mut self, note: NoteRef) -> bool {
        if self.pending.len() < self.pending.capacity() {
            self.pending.push(note);
        }
        true
    }

    /// Runs the release callbacks of the slots that see `note`'s release,
    /// from the slot that played it; whether one ignored it (which stops it
    /// reaching the slots after).
    pub(crate) fn release_callbacks(&mut self, note: NoteRef) -> bool {
        let plan = Arc::clone(&self.plan);
        let first = self
            .notes
            .get(note)
            .map_or(0, |n| usize::from(n.first.saturating_sub(1)));
        for (slot, script) in (0u8..).zip(&plan.scripts).skip(first) {
            if let Some(entry) = script.callbacks.release
                && self
                    .run_callback(slot, entry, Running::new(Kind::Release, Some(note)))
                    .ignore_release
            {
                return true;
            }
        }
        false
    }

    /// Starts every note waiting for its callback, in arrival order.
    pub(crate) fn start_pending(&mut self) {
        let mut pending = std::mem::take(&mut self.pending);
        for &note in &pending {
            let Some(n) = self.notes.get_mut(note) else {
                continue;
            };
            if n.started {
                continue;
            }
            n.started = true;
            if n.ignored || n.released {
                continue;
            }
            let held = self
                .notes
                .count(|n| n.started && !n.ignored && !n.released && !n.pedal_held)
                - 1;
            self.start_zones(note, true, |trigger| match trigger {
                Trigger::Attack => true,
                Trigger::First => held == 0,
                Trigger::Legato => held > 0,
                Trigger::Release => false,
            });
            self.behaviour.advance();
        }
        pending.clear();
        self.pending = pending;
    }
}
