//! Running script callbacks, slot after slot. A note passes each slot's note
//! callback in turn, moving on when the callback returns or first waits; a
//! note a callback plays passes only the slots after it, and the notes
//! through every slot start when the callback that let them through returns.

use std::sync::Arc;

use kontra_time::SampleTime;
use kontra_vm::{HostValue, Step, Vm};

use crate::ledger::{NoteRef, Releases};
use crate::queue::{Exit, Work};
use crate::script::{Kind, Running, ScriptPlan, micros_to_frames};
use crate::{Engine, ReleaseZones, Trigger};

impl Engine {
    /// The plan of the slot whose callback runs.
    pub(crate) fn script(&self) -> Option<&ScriptPlan> {
        self.plan.scripts.get(usize::from(self.running.slot))
    }

    /// Runs slot `slot`'s callback at `entry` now. Notes it let through or
    /// played start before this returns.
    pub(crate) fn run_callback(&mut self, slot: u8, entry: u32, running: Running) -> Running {
        self.run_with(slot, running, |vm, context| vm.start(entry, context))
    }

    /// [`run_callback`](Self::run_callback) on the coroutine `start` takes.
    fn run_with(
        &mut self,
        slot: u8,
        mut running: Running,
        start: impl FnOnce(&mut Vm, u64) -> Option<kontra_vm::CoId>,
    ) -> Running {
        self.ids.callbacks = (self.ids.callbacks + 1) & 0xF_FFFF;
        running.callback_id = self.ids.callbacks;
        running.slot = slot;
        let outer = std::mem::replace(&mut self.running, running);
        let mark = self.forward.len();
        if let Some(mut vm) = self.take_vm(slot) {
            if let Some(co) = start(&mut vm, running.context()) {
                #[cfg(feature = "trace")]
                if let Some(trace) = &mut self.trace {
                    trace.push(crate::TraceEvent::Callback {
                        frame: self.now.0,
                        slot,
                        kind: running.callback_type,
                        id: running.callback_id,
                        note_id: running.event.map(crate::NoteRef::event_id),
                        controller: running.controller,
                        control: running.ui,
                        resume: false,
                    });
                }
                self.step_script(slot, &mut vm, co);
            }
            self.put_vm(slot, vm);
        }
        let ran = std::mem::replace(&mut self.running, outer);
        self.forward_from(mark);
        self.start_pending();
        ran
    }

    /// The editor moved widget `n` of script `slot`'s own (Lua) page to `value`.
    pub fn set_widget(&mut self, slot: u8, n: u32, value: f64) {
        let entry = (self.plan.scripts.get(usize::from(slot))).and_then(|s| s.callbacks.widget);
        if let (Some(entry), Some(vm)) = (entry, self.vm_mut(slot)) {
            vm.stage_widget(n, value);
            let mut running = Running::new(Kind::UiControl, None);
            running.ui = i32::try_from(n).unwrap_or(i32::MAX);
            self.run_callback(slot, entry, running);
        }
    }

    /// The UI or the host moved control `id` (its UI id) of script `slot` to
    /// `value`: it is stored, then `on ui_controls` runs and then the
    /// control's own `on ui_control`, as in Kontakt; a control without one
    /// still stores and runs `on ui_controls`.
    /// A foreign script's control (`Foreign::controls`) takes `value` as
    /// the bits of an `f64` and runs its own callback.
    pub fn set_control(&mut self, slot: u8, id: i32, value: i64) {
        let plan = Arc::clone(&self.plan);
        let Some(script) = plan.scripts.get(usize::from(slot)) else {
            return;
        };
        if script.foreign.is_some() {
            let value = f64::from_bits(value as u64);
            let running = Running::new(Kind::UiControl, None);
            self.run_with(slot, running, |vm, context| vm.control(id, value, context));
            return;
        }
        let own = script.callbacks.ui_control.iter().find(|(i, _)| *i == id);
        let own = own.and_then(|&(_, own)| own);
        let controls = &script.controls;
        let n = self.control_index_in(slot, id);
        let cell = n.and_then(|n| controls.cells.get(n).copied().flatten());
        if cell.is_none() && own.is_none() {
            return;
        }
        let len = n.and_then(|n| controls.steps.get(n)).copied().unwrap_or(0);
        let vm = self.vms.get_mut(usize::from(slot)).and_then(Option::as_mut);
        if let (Some(cell), Some(vm), Some(at)) = (cell, vm, self.ui.take_cell(slot, n, len)) {
            vm.set_cell((cell + at) as usize, value);
        }
        // A menu the user picks from selects that entry.
        self.ui
            .select(slot, n, i32::try_from(value).unwrap_or(i32::MIN));
        for (kind, entry) in [
            (Kind::UiControls, script.callbacks.ui_controls),
            (Kind::UiControl, own),
        ] {
            if let Some(entry) = entry {
                let running = Running {
                    ui: id,
                    ..Running::new(kind, None)
                };
                self.run_callback(slot, entry, running);
            }
        }
    }

    /// Starts an asynchronous command for the running slot: its `on
    /// async_complete` runs when the queue reaches it, its exit status
    /// `exit`. Returns its id.
    pub(crate) fn start_async(&mut self, exit: Exit) -> i32 {
        let (slot, id) = (self.running.slot, self.next_async_id());
        self.queue_async(slot, id, exit);
        id
    }

    /// Queues the completion of command `id`. A full queue drops it, and the
    /// command no longer counts as running, so no `wait_async` waits on it.
    pub(crate) fn queue_async(&mut self, slot: u8, id: i32, exit: Exit) {
        if !self.queue.push(self.now, Work::Async { slot, id, exit }) {
            self.stats.events_refused += 1;
            self.finish_async(id);
        }
    }

    fn finish_async(&mut self, id: i32) {
        for r in self.ids.running.iter_mut().filter(|r| **r == id) {
            *r = 0;
        }
    }

    pub(crate) fn next_async_id(&mut self) -> i32 {
        let id = self.ids.async_last.wrapping_add(1).max(1);
        self.ids.async_last = id;
        if let Some(free) = self.ids.running.iter_mut().find(|r| **r == 0) {
            *free = id;
        }
        id
    }

    pub(crate) fn async_complete(&mut self, slot: u8, id: i32, exit: Exit) {
        self.ids.async_exit = exit;
        self.ids.async_done = id;
        self.finish_async(id);
        let plan = Arc::clone(&self.plan);
        let entry = plan
            .scripts
            .get(usize::from(slot))
            .and_then(|s| s.callbacks.async_complete);
        if let Some(entry) = entry {
            self.run_callback(slot, entry, Running::new(Kind::AsyncComplete, None));
        }
    }

    /// A waiting callback's time has come.
    pub(crate) fn resume(&mut self, slot: u8, co: kontra_vm::CoId, wait: u32) {
        let mark = self.forward.len();
        if let Some(mut vm) = self.take_vm(slot) {
            if vm.wait_epoch(co) != Some(wait) {
                // A stopped wait: its callback already went on.
                return self.put_vm(slot, vm);
            }
            self.running = Running {
                slot,
                ..Running::resumed(vm.context(co).unwrap_or(0))
            };
            #[cfg(feature = "trace")]
            if let Some(trace) = &mut self.trace {
                trace.push(crate::TraceEvent::Callback {
                    frame: self.now.0,
                    slot,
                    kind: self.running.callback_type,
                    id: self.running.callback_id,
                    note_id: self.running.event.map(crate::NoteRef::event_id),
                    controller: self.running.controller,
                    control: self.running.ui,
                    resume: true,
                });
            }
            self.step_script(slot, &mut vm, co);
            self.put_vm(slot, vm);
        }
        self.running = Running::IDLE;
        self.forward_from(mark);
        self.start_pending();
    }

    /// KSP `stop_wait`: applied when the slot's running callback returns or waits.
    pub(crate) fn stop_wait(&mut self, id: i32, rest: bool) -> HostValue {
        if self.stop_waits.len() < self.stop_waits.capacity() {
            self.stop_waits.push((self.running.slot, id as u32, rest));
        }
        HostValue::Nothing
    }

    /// Restarts a key's release-trigger counter; out of 0..=127 Kontakt
    /// refuses it ("Note number must be between 0 and 127").
    pub(crate) fn reset_rls_trig_counter(&mut self, key: i32) -> HostValue {
        if let Some(r) = self.behaviour.rls_reset.get_mut(key as usize) {
            *r = self.now.0;
        }
        HostValue::Nothing
    }

    fn take_vm(&mut self, slot: u8) -> Option<Vm> {
        self.vms.get_mut(usize::from(slot))?.take()
    }

    fn put_vm(&mut self, slot: u8, mut vm: Vm) {
        // `stop_wait` the slot's callbacks asked for while it ran: each goes on now.
        while let Some(i) = self.stop_waits.iter().position(|s| s.0 == slot) {
            let (_, id, rest) = self.stop_waits.swap_remove(i);
            let matches = |context| Running::resumed(context).callback_id == id & 0xF_FFFF;
            if let Some((co, wait)) = vm.stop_wait(matches, rest)
                && !self.queue.push(self.now, Work::Resume(slot, co, wait))
            {
                self.stats.events_refused += 1;
                vm.cancel(co);
            }
        }
        if let Some(cell) = self.vms.get_mut(usize::from(slot)) {
            *cell = Some(vm);
        }
    }

    fn step_script(&mut self, slot: u8, vm: &mut Vm, co: kontra_vm::CoId) {
        let plan = Arc::clone(&self.plan);
        let Some(script) = plan.scripts.get(usize::from(slot)) else {
            return;
        };
        if let Step::Wait(micros) = vm.run(&script.program, co, self) {
            self.wake(slot, vm, co, micros);
        }
        while let Some((spawned, micros)) = vm.spawned() {
            self.wake(slot, vm, spawned, micros);
        }
    }

    /// Queues `co`'s resume `micros` from now; a full queue ends it.
    fn wake(&mut self, slot: u8, vm: &mut Vm, co: kontra_vm::CoId, micros: u64) {
        let micros = i64::try_from(micros).unwrap_or(i64::MAX);
        let frames = micros_to_frames(micros, self.rate).max(1);
        let at = SampleTime(self.now.0.saturating_add(frames));
        let wait = vm.wait_epoch(co).unwrap_or(0);
        if !self.queue.push(at, Work::Resume(slot, co, wait)) {
            self.stats.events_refused += 1;
            vm.cancel(co);
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

    /// Runs the release callbacks of the slots that see `note`'s release: from its player
    /// (after it, unless it [hears its own](crate::ScriptPlan::hears_own_releases) and did
    /// not `note_off` it) up to the slot that ignored it, if one did; that slot is returned.
    pub(crate) fn release_callbacks(&mut self, note: NoteRef) -> Option<u8> {
        let plan = Arc::clone(&self.plan);
        let first = self.notes.get(note).map_or(0, |n| {
            let player = n.first.checked_sub(1);
            let hears = !n.noted_off
                && player
                    .and_then(|p| plan.scripts.get(usize::from(p)))
                    .is_none_or(|s| s.hears_own_releases);
            usize::from(if hears { player.unwrap_or(0) } else { n.first })
        });
        for (slot, script) in (0u8..).zip(&plan.scripts).skip(first) {
            if let Some(entry) = script.callbacks.release
                && self
                    .run_callback(slot, entry, Running::new(Kind::Release, Some(note)))
                    .ignore_release
            {
                return Some(slot);
            }
        }
        None
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
                .count(|n| n.started && !n.ignored && !n.is_up() && !n.pedal_held)
                - 1;
            let scripted = self.plan.release_zones == ReleaseZones::WithNote;
            self.start_zones(note, true, |trigger| match trigger {
                Trigger::Attack => true,
                Trigger::First => held == 0,
                Trigger::Legato => held > 0,
                Trigger::Release => scripted,
            });
            if !scripted && self.notes.get(note).map(|n| n.releases) == Some(Releases::AtStart) {
                self.start_zones(note, false, |trigger| trigger == Trigger::Release);
            }
            self.behaviour.advance();
        }
        pending.clear();
        self.pending = pending;
    }
}

/// Ids the script host hands out.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ids {
    /// Callbacks run so far (20 bits): the source of `$NI_CALLBACK_ID`.
    pub(crate) callbacks: u32,
    /// The last asynchronous command started, and `$NI_ASYNC_ID`: the last finished.
    pub(crate) async_last: i32,
    pub(crate) async_done: i32,
    /// How the last finished command ended (`$NI_ASYNC_EXIT_STATUS`).
    pub(crate) async_exit: Exit,
    /// Commands started and not finished (0 a free cell), which
    /// `wait_async` waits on.
    // ponytail: past 32 at once, a command is not waited on.
    pub(crate) running: [i32; 32],
}

impl Ids {
    pub(crate) const START: Self = Self {
        callbacks: 0,
        async_last: 0,
        async_done: 0,
        async_exit: Exit::Done,
        running: [0; 32],
    };
}

/// A script slot's listener timer (`set_listener`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Timer {
    /// Microseconds, or ticks per quarter note.
    every: u32,
    /// Bumped on every change: a queued firing of an older one is dropped.
    generation: u16,
    /// Whether its next firing is queued; a full queue leaves it for
    /// [`Engine::rearm_timers`] to retry.
    armed: bool,
}

/// A slot's `$NI_SIGNAL_TIMER_MS` and `$NI_SIGNAL_TIMER_BEAT` timers.
pub(crate) type Timers = [Option<Timer>; 2];

// Measured, probes/callback_constants.lua.
const TIMER_MS: u8 = 3;
const TIMER_BEAT: u8 = 4;

impl Engine {
    /// `set_listener` and `change_listener_par` for the running slot: a
    /// timer of `par` microseconds (at least 1000) or `par` ticks per quarter
    /// (1..=24); `par` 0 stops it. Transport signals go to the transport.
    pub(crate) fn set_listener(&mut self, signal: i32, par: i32) {
        let slot = self.running.slot;
        let Ok(signal) = u8::try_from(signal) else {
            return;
        };
        let every = match signal {
            TIMER_MS => par.max(0).unsigned_abs().max(1000),
            TIMER_BEAT => par.clamp(0, 24).unsigned_abs(),
            _ => {
                self.listen_transport(slot, signal, par != 0);
                return;
            }
        };
        let Some(timer) = self
            .timers
            .get_mut(usize::from(slot))
            .and_then(|t| t.get_mut(usize::from(signal - TIMER_MS)))
        else {
            return;
        };
        let generation = timer.map_or(0, |t| t.generation).wrapping_add(1);
        *timer = (par > 0).then_some(Timer {
            every,
            generation,
            armed: false,
        });
        self.arm(slot, signal);
    }

    fn timer(&self, slot: u8, signal: u8) -> Option<Timer> {
        let timers = self.timers.get(usize::from(slot))?;
        *timers.get(usize::from(signal.checked_sub(TIMER_MS)?))?
    }

    fn arm(&mut self, slot: u8, signal: u8) {
        let Some(timer) = self.timer(slot, signal) else {
            return;
        };
        let seconds = if signal == TIMER_MS {
            f64::from(timer.every) / 1e6
        } else {
            60.0 / f64::from(self.tempo.max(1.0)) / f64::from(timer.every.max(1))
        };
        let frames = ((seconds * f64::from(self.rate)).round() as u64).max(1);
        let work = Work::Listen {
            slot,
            signal,
            generation: timer.generation,
        };
        let armed = self.queue.push(SampleTime(self.now.0 + frames), work);
        let timer = self
            .timers
            .get_mut(usize::from(slot))
            .and_then(|t| t.get_mut(usize::from(signal - TIMER_MS)))
            .and_then(Option::as_mut);
        if let Some(timer) = timer {
            timer.armed = armed;
        }
    }

    /// Retries each timer a full queue refused to arm.
    pub(crate) fn rearm_timers(&mut self) {
        for slot in 0..self.timers.len() {
            for signal in [TIMER_MS, TIMER_BEAT] {
                let Ok(slot) = u8::try_from(slot) else {
                    return;
                };
                if self.timer(slot, signal).is_some_and(|t| !t.armed) {
                    self.arm(slot, signal);
                }
            }
        }
    }

    /// A slot's timer fired: runs its listener callback, then rearms unless
    /// the callback changed or stopped it (that change armed its own firing).
    pub(crate) fn listen(&mut self, slot: u8, signal: u8, generation: u16) {
        if self
            .timer(slot, signal)
            .is_none_or(|t| t.generation != generation)
        {
            return;
        }
        self.run_listener(slot, signal);
        if self
            .timer(slot, signal)
            .is_some_and(|t| t.generation == generation)
        {
            self.arm(slot, signal);
        }
    }

    /// Runs slot `slot`'s listener callback, if it has one, for `signal`.
    pub(crate) fn run_listener(&mut self, slot: u8, signal: u8) {
        let plan = Arc::clone(&self.plan);
        if let Some(entry) = plan
            .scripts
            .get(usize::from(slot))
            .and_then(|s| s.callbacks.listener)
        {
            let running = Running {
                signal,
                ..Running::new(Kind::Listener, None)
            };
            self.run_callback(slot, entry, running);
        }
    }
}
