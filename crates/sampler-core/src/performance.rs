//! Musical routing domains are independent of physical/expressive MIDI channels.
use crate::{Error, Event, NoteId, Runtime, RuntimeId, Trigger};

/// A fixed performance domain, valid only in its originating runtime. Domains are
/// budgeted at construction and cannot be recycled while notes refer to them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerformanceId {
    runtime: RuntimeId,
    index: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionPolicy {
    #[default]
    Onset,
    Current,
}

/// Physical input key consumed by a native latched articulation switch.
#[derive(Clone, Copy, Debug)]
pub struct Keyswitch {
    pub key: u8,
    pub articulation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionSnapshot {
    pub performance: PerformanceId,
    pub articulation: u32,
    pub consumed_switch: bool,
}

#[derive(Clone, Copy, Default)]
pub(super) struct NoteSelection {
    pub performance: usize,
    pub snapshot: usize,
    pub consumed_switch: bool,
}

impl Runtime {
    /// Domain zero is the default for trigger/note_on and existing MIDI ingress.
    pub fn performance(&self, index: usize) -> Result<PerformanceId, Error> {
        if index >= self.performance_state.current.len() {
            return Err(Error::InvalidInput);
        }
        Ok(PerformanceId {
            runtime: self.id(),
            index,
        })
    }

    pub(super) fn performance_index(&self, id: PerformanceId) -> Result<usize, Error> {
        if id.runtime != self.id() || id.index >= self.performance_state.current.len() {
            return Err(Error::StaleHandle);
        }
        Ok(id.index)
    }

    pub fn articulation(&self, id: PerformanceId) -> Result<u32, Error> {
        Ok(self
            .performance_state
            .current(self.performance_index(id)?)
            .articulation)
    }

    pub fn set_articulation(&mut self, id: PerformanceId, value: u32) -> Result<(), Error> {
        self.schedule_event(self.now, Event::Articulation(id, value))
    }

    pub fn note_selection(&self, note: NoteId) -> Result<SelectionSnapshot, Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let state = self.selections[note.0.index];
        Ok(SelectionSnapshot {
            performance: self.performance(state.performance).unwrap(),
            articulation: self.performance_state.states[state.snapshot].articulation,
            consumed_switch: state.consumed_switch,
        })
    }

    pub(super) fn release_selection(&self, note: NoteId, trigger: Trigger) -> usize {
        let n = self.notes.get(note.0).unwrap();
        let state = self.selections[note.0.index];
        match self.plans.get(n.plan.0).unwrap().prepared.release_selection
            [trigger.release_index().unwrap()]
        {
            SelectionPolicy::Onset => state.snapshot,
            SelectionPolicy::Current => self.performance_state.current[state.performance],
        }
    }
}

// Private indices never escape the runtime. Each live note and each performance
// owns exactly one version; reclaim happens only after its last owner releases it.
#[derive(Clone, Copy)]
pub(super) struct State {
    pub articulation: u32,
    pub controllers: [u32; 128],
    /// The virtual controller [`crate::PREVIOUS_KEY`]: see [`previous_key_value`].
    pub previous: u32,
    owners: usize,
}

impl State {
    /// A controller's value, including the virtual previous-key one.
    pub fn value(&self, controller: u8) -> u32 {
        self.controllers
            .get(usize::from(controller))
            .copied()
            .unwrap_or(self.previous)
    }
}

/// Value of the virtual previous-key controller: 0 with no other key held,
/// else the interval from the most recent held key (this key minus that one,
/// in -127..=127) offset into 1..=255.
pub fn previous_key_value(interval: Option<i16>) -> u32 {
    interval.map_or(0, |i| (i.clamp(-127, 127) + 128) as u32)
}

pub(super) struct PerformanceState {
    pub input_controllers: Box<[[u32; 128]]>,
    pub states: Box<[State]>,
    pub current: Box<[usize]>,
    free: Vec<usize>,
}

impl PerformanceState {
    pub fn validate(notes: usize, performances: usize) -> Result<usize, Error> {
        let capacity = notes.checked_add(performances).ok_or(Error::Capacity)?;
        std::alloc::Layout::array::<State>(capacity).map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<[u32; 128]>(performances).map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<usize>(capacity).map_err(|_| Error::Capacity)?;
        Ok(capacity)
    }

    pub fn new(capacity: usize, performances: usize) -> Self {
        let mut states = vec![
            State {
                articulation: 0,
                controllers: RESET_CONTROLLERS,
                previous: 0,
                owners: 0
            };
            capacity
        ]
        .into_boxed_slice();
        states[0].owners = performances;
        Self {
            input_controllers: vec![RESET_CONTROLLERS; performances].into_boxed_slice(),
            states,
            current: vec![0; performances].into_boxed_slice(),
            free: (1..capacity).rev().collect(),
        }
    }

    pub fn current(&self, performance: usize) -> &State {
        &self.states[self.current[performance]]
    }

    pub fn capture(&mut self, performance: usize) -> usize {
        let index = self.current[performance];
        self.states[index].owners += 1;
        index
    }

    pub fn release(&mut self, index: usize) {
        let state = &mut self.states[index];
        debug_assert!(state.owners != 0);
        state.owners -= 1;
        if state.owners == 0 {
            self.free.push(index);
        }
    }

    pub fn edit(&mut self, performance: usize) -> &mut State {
        let mut index = self.current[performance];
        if self.states[index].owners != 1 {
            // At most notes + performances owners exist. A shared version implies
            // fewer distinct versions than owners, so a free slot always exists.
            let next = self.free.pop().expect("shared state leaves a free version");
            self.states[next] = self.states[index];
            self.states[next].owners = 1;
            self.states[index].owners -= 1;
            self.current[performance] = next;
            index = next;
        }
        &mut self.states[index]
    }
}

impl Runtime {
    /// Effective downstream CC state, after any event-processing stage has accepted
    /// the update. This is neither raw MIDI state nor per-note MPE expression.
    pub fn controller(&self, id: PerformanceId, controller: u8) -> Result<u32, Error> {
        let state = self.performance_state.current(self.performance_index(id)?);
        state
            .controllers
            .get(usize::from(controller))
            .copied()
            .ok_or(Error::InvalidInput)
    }

    /// Immutable controller value at this note's logical admission, retained through
    /// source EOF, child/callback lifetime and terminal retry.
    pub fn note_controller(&self, note: NoteId, controller: u8) -> Result<u32, Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let state = &self.performance_state.states[self.selections[note.0.index].snapshot];
        state
            .controllers
            .get(usize::from(controller))
            .copied()
            .ok_or(Error::InvalidInput)
    }

    pub fn set_controller(
        &mut self,
        id: PerformanceId,
        controller: u8,
        value: u32,
    ) -> Result<(), Error> {
        self.schedule_event(self.now, Event::Controller(id, controller, value))
    }

    pub(super) fn controller_now(&mut self, performance: usize, controller: u8, value: u32) {
        if self.performance_state.current(performance).controllers[usize::from(controller)] != value
        {
            self.performance_state.edit(performance).controllers[usize::from(controller)] = value;
        }
    }

    pub(super) fn articulation_now(&mut self, performance: usize, value: u32) {
        if self.performance_state.current(performance).articulation != value {
            self.performance_state.edit(performance).articulation = value;
        }
    }
}

/// Controller values before any are received: MIDI's reset state (RP-015), in
/// which expression (CC11) is full and everything else is zero. Instruments
/// that scale volume by CC11 therefore sound until a host sends it.
const RESET_CONTROLLERS: [u32; 128] = {
    let mut values = [0; 128];
    values[11] = u32::MAX;
    values
};
