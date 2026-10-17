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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// Revision of this note's physical controller domain at admission.
    pub controller_epoch: u64,
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
            SelectionPolicy::Current => self.performance_state.channel_current_index(state.performance, n.address),
        }
    }
}

// Private indices never escape the runtime. Each live note and each performance
// owns exactly one version; reclaim happens only after its last owner releases it.
#[derive(Clone, Copy)]
pub(super) struct State {
    pub articulation: u32,
    pub native_key: Option<u8>,
    pub native_tick: u64,
    pub native_seed: u64,
    pub controllers: [u32; 128],
    /// Script pitch bend (128) and mono pressure (129) are independent of the
    /// native previous-key/axis selectors addressed by `value`.
    pub script_virtual: [u32; 2],
    /// The virtual controller [`crate::PREVIOUS_KEY`]: see [`previous_key_value`].
    pub previous: u32,
    /// The virtual controllers [`crate::AXIS_BASE`]`..` : the active choice of each axis.
    pub axes: [u32; crate::MAX_AXES],
    owners: usize,
}

impl State {
    fn script_controller(&self, number: u8) -> Option<u32> {
        if number < 128 {
            self.controllers.get(usize::from(number)).copied()
        } else {
            self.script_virtual.get(usize::from(number - 128)).copied()
        }
    }
    /// A controller's value, including the virtual previous-key one.
    pub fn value(&self, controller: u8) -> u32 {
        self.controllers
            .get(usize::from(controller))
            .copied()
            .unwrap_or_else(|| match usize::from(controller) - 128 {
                0 => self.previous,
                n => self.axes[n - 1],
            })
    }
}

/// Value of the virtual previous-key controller: 0 with no other key held,
/// else the interval from the most recent held key (this key minus that one,
/// in -127..=127) offset into 1..=255.
pub fn previous_key_value(interval: Option<i16>) -> u32 {
    interval.map_or(0, |i| (i.clamp(-127, 127) + 128) as u32)
}

/// A bounded physical controller domain retains its real address for the
/// Runtime lifetime. Its current version is owned independently of global API
/// state and of every note's immutable onset version.
struct PhysicalControllers {
    performance: usize,
    address: crate::ChannelAddress,
    current: usize,
    epoch: u64,
    input: [u32; 130],
    input_poly: [u32; 128],
    poly: [u32; 128],
}

pub(super) struct PerformanceState {
    physical: Vec<PhysicalControllers>,
    physical_limit: usize,
    pub input_controllers: Box<[[u32; 130]]>,
    pub input_poly_pressure: Box<[[u32; 128]]>,
    pub poly_pressure: Box<[[u32; 128]]>,
    pub states: Box<[State]>,
    pub current: Box<[usize]>,
    free: Vec<usize>,
}

impl PerformanceState {
    pub fn validate(notes: usize, performances: usize, channels: usize) -> Result<usize, Error> {
        let domains = performances.checked_mul(channels.max(16)).ok_or(Error::Capacity)?;
        let capacity = notes.checked_add(performances).and_then(|n| n.checked_add(domains)).ok_or(Error::Capacity)?;
        std::alloc::Layout::array::<PhysicalControllers>(domains).map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<State>(capacity).map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<[u32; 130]>(performances).map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<usize>(capacity).map_err(|_| Error::Capacity)?;
        Ok(capacity)
    }

    pub fn new(capacity: usize, performances: usize, channels: usize) -> Result<Self, Error> {
        fn filled<T: Clone>(count: usize, value: T) -> Result<Box<[T]>, Error> {
            let mut rows = Vec::new(); rows.try_reserve_exact(count).map_err(|_| Error::Capacity)?;
            rows.resize(count, value); Ok(rows.into_boxed_slice())
        }
        let domains = performances.checked_mul(channels.max(16)).ok_or(Error::Capacity)?;
        let mut physical = Vec::new(); physical.try_reserve_exact(domains).map_err(|_| Error::Capacity)?;
        let mut free = Vec::new(); free.try_reserve_exact(capacity.saturating_sub(1)).map_err(|_| Error::Capacity)?;
        free.extend((1..capacity).rev());
        let mut states = filled(capacity,
            State {
                articulation: 0,
                native_key: None,
                native_tick: 0,
                native_seed: 0,
                controllers: RESET_CONTROLLERS,
                script_virtual: [0x8000_0000, 0],
                previous: 0,
                axes: [0; crate::MAX_AXES],
                owners: 0
            })?;
        states[0].owners = performances;
        Ok(Self {
            physical,
            physical_limit: domains,
            input_controllers: filled(performances, RESET_INPUT_CONTROLLERS)?,
            input_poly_pressure: filled(performances, [0; 128])?,
            poly_pressure: filled(performances, [0; 128])?,
            states,
            current: filled(performances, 0)?,
            free,
        })
    }

    fn physical_index(&self, performance: usize, address: crate::ChannelAddress) -> Option<usize> {
        self.physical.iter().position(|row| row.performance == performance && row.address == address)
    }

    pub fn channel_current_index(&self, performance: usize, address: crate::ChannelAddress) -> usize {
        self.physical_index(performance, address).map_or(self.current[performance], |i| self.physical[i].current)
    }

    pub fn channel_current(&self, performance: usize, address: crate::ChannelAddress) -> &State {
        &self.states[self.channel_current_index(performance, address)]
    }

    pub fn validate_scope(&self, performance: usize, scope: crate::ChannelScope) -> Result<(), Error> {
        if performance >= self.current.len() || scope.group >= 16 || scope.channels == 0 { return Err(Error::InvalidInput); }
        let missing = (0..16u8).filter(|&channel| scope.channels & (1 << channel) != 0
            && self.physical_index(performance, crate::ChannelAddress { protocol: scope.protocol,
                port: scope.port, group: scope.group, channel }).is_none()).count();
        if missing > self.physical_limit - self.physical.len() { return Err(Error::Capacity); }
        Ok(())
    }

    fn ensure_scope(&mut self, performance: usize, scope: crate::ChannelScope) {
        for channel in 0..16u8 {
            if scope.channels & (1 << channel) == 0 { continue; }
            let address = crate::ChannelAddress { protocol: scope.protocol, port: scope.port, group: scope.group, channel };
            if self.physical_index(performance, address).is_some() { continue; }
            let current = self.current[performance];
            self.states[current].owners += 1;
            self.physical.push(PhysicalControllers { performance, address, current, epoch: 1,
                input: self.input_controllers[performance],
                input_poly: self.input_poly_pressure[performance], poly: self.poly_pressure[performance] });
        }
    }

    pub fn prepare_scope(&mut self, performance: usize, scope: crate::ChannelScope) -> Result<(), Error> {
        self.validate_scope(performance, scope)?;
        self.ensure_scope(performance, scope);
        Ok(())
    }

    fn detach(&mut self, index: usize) -> usize {
        if self.states[index].owners == 1 { return index; }
        // Current global/physical domains and live notes are the only owners;
        // the arena is checked for their complete maximum at construction.
        let next = self.free.pop().expect("shared state leaves a free version");
        self.states[next] = self.states[index];
        self.states[next].owners = 1;
        self.states[index].owners -= 1;
        next
    }

    fn edit_physical(&mut self, index: usize) -> &mut State {
        let next = self.detach(self.physical[index].current);
        self.physical[index].current = next;
        &mut self.states[next]
    }

    pub fn capture_channel(&mut self, performance: usize, address: crate::ChannelAddress) -> usize {
        let index = self.channel_current_index(performance, address);
        self.states[index].owners += 1;
        index
    }

    pub fn scoped_controller(&mut self, performance: usize, scope: crate::ChannelScope, number: u8, value: u32) -> Result<(), Error> {
        self.prepare_scope(performance, scope)?;
        for i in 0..self.physical.len() {
            if self.physical[i].performance != performance || !scope.contains(self.physical[i].address) { continue; }
            let state = self.edit_physical(i);
            if number < 128 { state.controllers[usize::from(number)] = value; }
            else { state.script_virtual[usize::from(number - 128)] = value; }
        }
        Ok(())
    }

    pub fn global_controller(&mut self, performance: usize, number: u8, value: u32) {
        for i in 0..self.physical.len() {
            if self.physical[i].performance != performance { continue; }
            let state = self.edit_physical(i);
            if number < 128 { state.controllers[usize::from(number)] = value; }
            else { state.script_virtual[usize::from(number - 128)] = value; }
        }
    }

    pub fn sync_metadata(&mut self, performance: usize) {
        let global = *self.current(performance);
        for i in 0..self.physical.len() {
            if self.physical[i].performance != performance { continue; }
            let current = &self.states[self.physical[i].current];
            if current.articulation == global.articulation && current.native_key == global.native_key
                && current.native_tick == global.native_tick && current.native_seed == global.native_seed
                && current.previous == global.previous && current.axes == global.axes { continue; }
            let target = self.edit_physical(i);
            target.articulation = global.articulation; target.native_key = global.native_key;
            target.native_tick = global.native_tick; target.native_seed = global.native_seed;
            target.previous = global.previous; target.axes = global.axes;
        }
    }

    pub fn input_bank(&self, performance: usize, address: crate::ChannelAddress) -> &[u32; 130] {
        self.physical_index(performance, address).map_or(&self.input_controllers[performance], |i| &self.physical[i].input)
    }
    pub fn poly_bank(&self, performance: usize, address: crate::ChannelAddress, input: bool) -> &[u32; 128] {
        self.physical_index(performance, address).map_or(
            if input { &self.input_poly_pressure[performance] } else { &self.poly_pressure[performance] },
            |i| if input { &self.physical[i].input_poly } else { &self.physical[i].poly })
    }
    pub fn scoped_input(&mut self, performance: usize, scope: crate::ChannelScope, number: u8, value: u32) {
        self.ensure_scope(performance, scope);
        for row in &mut self.physical {
            if row.performance == performance && scope.contains(row.address) { row.input[usize::from(number)] = value; }
        }
    }
    pub fn scoped_poly(&mut self, performance: usize, scope: crate::ChannelScope, key: u8, value: u32, input: bool) {
        self.ensure_scope(performance, scope);
        for row in &mut self.physical {
            if row.performance == performance && scope.contains(row.address) {
                if input { row.input_poly[usize::from(key)] = value; } else { row.poly[usize::from(key)] = value; }
            }
        }
    }
    pub fn reset_poly(&mut self, performance: usize) {
        self.input_poly_pressure[performance].fill(0);
        self.poly_pressure[performance].fill(0);
        for row in &mut self.physical {
            if row.performance == performance { row.input_poly.fill(0); row.poly.fill(0); }
        }
    }
    pub fn reset_poly_scope(&mut self, performance: usize, scope: crate::ChannelScope) {
        for row in &mut self.physical {
            if row.performance == performance && scope.contains(row.address) {
                row.input_poly.fill(0);
                row.poly.fill(0);
            }
        }
    }
    pub fn epochs(&self, performance: usize, scope: crate::ChannelScope) -> [u64; 16] {
        std::array::from_fn(|channel| self.physical_index(performance, crate::ChannelAddress {
            protocol: scope.protocol, port: scope.port, group: scope.group, channel: channel as u8,
        }).map_or(1, |i| self.physical[i].epoch))
    }
    pub fn surviving_scope(&self, performance: usize, scope: crate::ChannelScope, epochs: [u64; 16]) -> crate::ChannelScope {
        let current = self.epochs(performance, scope);
        crate::ChannelScope { channels: (0..16).fold(0, |mask, channel| mask |
            if current[channel] == epochs[channel] { scope.channels & (1 << channel) } else { 0 }), ..scope }
    }
    pub fn validate_reset(&self, performance: usize, scope: crate::ChannelScope) -> Result<(), Error> {
        self.validate_scope(performance, scope)?;
        if self.physical.iter().any(|row| row.performance == performance && scope.contains(row.address) && row.epoch == u64::MAX) {
            return Err(Error::ClockOverflow);
        }
        Ok(())
    }
    pub fn reset_scope(&mut self, performance: usize, scope: crate::ChannelScope) -> Result<(), Error> {
        self.validate_reset(performance, scope)?;
        self.ensure_scope(performance, scope);
        for i in 0..self.physical.len() {
            if self.physical[i].performance != performance || !scope.contains(self.physical[i].address) { continue; }
            let state = self.edit_physical(i);
            state.controllers = RESET_CONTROLLERS; state.script_virtual = [0x8000_0000, 0];
            let row = &mut self.physical[i]; row.input = RESET_INPUT_CONTROLLERS;
            row.input_poly.fill(0); row.poly.fill(0); row.epoch += 1;
        }
        Ok(())
    }

    pub fn current(&self, performance: usize) -> &State {
        &self.states[self.current[performance]]
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
        let index = self.detach(self.current[performance]);
        self.current[performance] = index;
        &mut self.states[index]
    }

}

impl Runtime {
    /// Effective downstream CC state, after any event-processing stage has accepted
    /// the update. This is neither raw MIDI state nor per-note MPE expression.
    pub fn controller(&self, id: PerformanceId, controller: u8) -> Result<u32, Error> {
        let state = self.performance_state.current(self.performance_index(id)?);
        state
            .script_controller(controller)
            .ok_or(Error::InvalidInput)
    }

    /// Current downstream value in one captured physical channel domain.
    /// Missing physical rows use explicit global defaults; no other channel is mirrored.
    pub fn channel_controller(&self, id: PerformanceId, address: crate::ChannelAddress, controller: u8) -> Result<u32, Error> {
        if address.group >= 16 || address.channel >= 16 { return Err(Error::InvalidInput); }
        self.performance_state.channel_current(self.performance_index(id)?, address)
            .script_controller(controller).ok_or(Error::InvalidInput)
    }

    /// Pure admission check for a channel-local protocol reset.
    pub fn validate_channel_controller_reset(&self, id: PerformanceId, scope: crate::ChannelScope) -> Result<(), Error> {
        self.performance_state.validate_reset(self.performance_index(id)?, scope)
    }

    /// Reset only these physical controller domains and invalidate prior captured
    /// controller work for those targets. Global APIs and other channels retain
    /// their values. This does not synthesize callbacks or physical key releases.
    pub fn reset_channel_controllers(&mut self, id: PerformanceId, scope: crate::ChannelScope) -> Result<(), Error> {
        let performance = self.performance_index(id)?;
        self.apply_due();
        self.performance_state.validate_reset(performance, scope)?;
        // Controller reset publishes before pedal-up can invoke any downstream
        // release work. The remaining validated pedal-up needs no admission.
        self.performance_state.reset_scope(performance, scope)?;
        for slot in &mut self.plans.slots {
            if let Some(generation) = &mut slot.value { generation.controllers.reset_scope(performance, scope); }
        }
        self.sustain_scope(scope, false)?;
        self.sostenuto_scope(scope, false)?;
        Ok(())
    }

    /// Immutable controller value at this note's logical admission, retained through
    /// source EOF, child/callback lifetime and terminal retry.
    pub fn note_controller(&self, note: NoteId, controller: u8) -> Result<u32, Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let state = &self.performance_state.states[self.selections[note.0.index].snapshot];
        state
            .script_controller(controller)
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
        if self
            .performance_state
            .current(performance)
            .script_controller(controller)
            != Some(value)
        {
            let state = self.performance_state.edit(performance);
            if controller < 128 {
                state.controllers[usize::from(controller)] = value;
            } else {
                state.script_virtual[usize::from(controller - 128)] = value;
            }
        }
        self.performance_state.global_controller(performance, controller, value);
    }

    pub(super) fn articulation_now(&mut self, performance: usize, value: u32) {
        let axis = (value >> 16) as usize & 0xff;
        if value >> 24 == AXIS_SWITCH >> 24 && axis < crate::MAX_AXES {
            let choice = value & 0xffff;
            if self.performance_state.current(performance).axes[axis] != choice {
                self.performance_state.edit(performance).axes[axis] = choice;
            }
        } else {
            let key = self
                .plans
                .get(self.active_plan.0)
                .unwrap()
                .prepared
                .native_articulation_keys
                .get(value as usize)
                .copied()
                .flatten();
            let state = self.performance_state.edit(performance);
            state.articulation = value;
            if key.is_some() {
                state.native_key = key;
            }
        }
        self.performance_state.sync_metadata(performance);
    }
}

/// Tag in the top byte of a keyswitch's value: it sets nested selector `axis` to `choice`
/// (`AXIS_SWITCH | axis << 16 | choice`) instead of the articulation.
pub const AXIS_SWITCH: u32 = 0xA5 << 24;

/// Controller values before any are received: MIDI's reset state (RP-015), in
/// which expression (CC11) is full and everything else is zero. Instruments
/// that scale volume by CC11 therefore sound until a host sends it.
const RESET_CONTROLLERS: [u32; 128] = {
    let mut values = [0; 128];
    values[11] = u32::MAX;
    values
};

const RESET_INPUT_CONTROLLERS: [u32; 130] = {
    let mut values = [0; 130];
    values[11] = u32::MAX;
    values[128] = 0x8000_0000;
    values
};
