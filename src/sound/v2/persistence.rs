//! The audio owner publishes coherent values; host saves allocate only on their thread.
use super::*;
use sampler_core::{
    ScriptStateAddress as Address, ScriptStateBuffer, ScriptStateEntry, ScriptStateValue as Value,
};
use std::sync::{
    Mutex,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

#[derive(serde::Serialize, serde::Deserialize)]
struct Saved {
    schema: String,
    values: Vec<SavedValue>,
}
#[derive(serde::Serialize, serde::Deserialize)]
enum SavedValue {
    Integer(i64),
    Real(f64),
    Toggle(bool),
    Cell(i64),
    Text(String),
}

struct Atom {
    kind: u8,
    offset: usize,
    len: usize,
}
impl Atom {
    fn new(value: Value, offset: usize) -> Self {
        let kind = match value {
            Value::Control(ControlValue::Integer(_)) => 0,
            Value::Control(ControlValue::Real(_)) => 1,
            Value::Control(ControlValue::Toggle(_)) => 2,
            Value::Cell(_) => 3,
            Value::Text(_) => 4,
        };
        Self {
            kind,
            offset,
            len: if kind == 4 {
                1 + sampler_core::TEXT_CAPACITY.div_ceil(8)
            } else {
                1
            },
        }
    }
    fn write(&self, slot: &[AtomicU64], value: &Value) {
        let words = &slot[self.offset..self.offset + self.len];
        let set = |index: usize, value| {
            if words[index].load(Ordering::Relaxed) != value {
                words[index].store(value, Ordering::SeqCst);
            }
        };
        match value {
            Value::Control(ControlValue::Integer(v)) | Value::Cell(v) => set(0, *v as u64),
            Value::Control(ControlValue::Real(v)) => set(0, v.to_bits()),
            Value::Control(ControlValue::Toggle(v)) => set(0, u64::from(*v)),
            Value::Text(v) => {
                let bytes = v.as_str().as_bytes();
                set(0, bytes.len() as u64);
                for (index, chunk) in bytes.chunks(8).enumerate() {
                    let mut word = [0; 8];
                    word[..chunk.len()].copy_from_slice(chunk);
                    set(index + 1, u64::from_le_bytes(word));
                }
            }
        }
    }
    fn read(&self, slot: &[AtomicU64]) -> SavedValue {
        let words = &slot[self.offset..self.offset + self.len];
        let value = words[0].load(Ordering::SeqCst);
        match self.kind {
            0 => SavedValue::Integer(value as i64),
            1 => SavedValue::Real(f64::from_bits(value)),
            2 => SavedValue::Toggle(value != 0),
            3 => SavedValue::Cell(value as i64),
            _ => {
                let len = (value as usize).min(sampler_core::TEXT_CAPACITY);
                let mut bytes = Vec::with_capacity(len);
                for word in words.iter().skip(1).take(len.div_ceil(8)) {
                    bytes.extend_from_slice(&word.load(Ordering::SeqCst).to_le_bytes());
                }
                bytes.truncate(len);
                SavedValue::Text(
                    String::from_utf8(bytes)
                        .unwrap_or_else(|_| panic!("invalid coherent script text snapshot")),
                )
            }
        }
    }
}

pub(super) struct Snapshot {
    schema: String,
    published: AtomicUsize,
    reading: AtomicUsize,
    read_lock: Mutex<()>,
    atoms: Vec<Atom>,
    addresses: Box<[Address]>,
    dynamic: Box<[usize]>,
    #[cfg(test)]
    captured_values: AtomicUsize,
    slots: [Box<[AtomicU64]>; 3],
}
impl Snapshot {
    fn new(schema: String, state: &ScriptStateBuffer) -> Arc<Self> {
        let mut size = 0;
        let atoms = state
            .values
            .iter()
            .map(|entry| {
                let atom = Atom::new(entry.value, size);
                size += atom.len;
                atom
            })
            .collect();
        let snapshot = Arc::new(Self {
            schema,
            published: AtomicUsize::new(0),
            reading: AtomicUsize::new(usize::MAX),
            read_lock: Mutex::new(()),
            atoms,
            addresses: state.values.iter().map(|entry| entry.address).collect(),
            dynamic: state.values.iter().enumerate().filter_map(|(index, entry)|
                (!matches!(entry.address, Address::Cell { .. })).then_some(index)).collect(),
            #[cfg(test)]
            captured_values: AtomicUsize::new(0),
            slots: std::array::from_fn(|_| (0..size).map(|_| AtomicU64::new(0)).collect()),
        });
        for slot in &snapshot.slots {
            for (atom, entry) in snapshot.atoms.iter().zip(&state.values) {
                atom.write(slot, &entry.value);
            }
        }
        snapshot
    }
    fn next_slot(&self) -> usize {
        let current = self.published.load(Ordering::SeqCst);
        let mut next = (current + 1) % 3;
        if next == self.reading.load(Ordering::SeqCst) {
            next = (next + 1) % 3;
        }
        next
    }
    #[cfg(test)]
    fn publish(&self, state: &ScriptStateBuffer) {
        let next = self.next_slot();
        for (atom, entry) in self.atoms.iter().zip(&state.values) {
            atom.write(&self.slots[next], &entry.value);
        }
        self.published.store(next, Ordering::SeqCst);
    }
    fn capture(&self, runtime: &Runtime, plan: sampler_core::PlanId, dirty: Option<&[u64]>) -> Result<(), sampler_core::Error> {
        let next = self.next_slot();
        #[cfg(test)]
        self.captured_values.store(0, Ordering::Relaxed);
        let capture = |index: usize| -> Result<(), sampler_core::Error> {
            let address = self.addresses.get(index).ok_or(sampler_core::Error::InvalidInput)?;
            let value = match *address {
                Address::Control(id) => Value::Control(runtime.control_base_value(plan, id)?),
                Address::Cell { instance, index } => Value::Cell(runtime.script_cell(plan, instance, index)?),
                Address::Text { instance, index } => Value::Text(runtime.script_text(plan, instance, index)?),
            };
            self.atoms[index].write(&self.slots[next], &value);
            #[cfg(test)]
            self.captured_values.fetch_add(1, Ordering::Relaxed);
            Ok(())
        };
        if let Some(dirty) = dirty {
            for &index in &self.dynamic { capture(index)?; }
            for (word, &bits) in dirty.iter().enumerate() {
                let mut bits = bits;
                while bits != 0 {
                    let index = word * 64 + bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    capture(index)?;
                }
            }
        } else {
            for index in 0..self.addresses.len() { capture(index)?; }
        }
        // A rejected capture leaves the previous coherent slot published.
        self.published.store(next, Ordering::SeqCst);
        Ok(())
    }
    pub(super) fn save(&self) -> String {
        // Only save readers contend; audio always has one unpinned slot to publish.
        let _reader = self.read_lock.lock().unwrap();
        loop {
            let slot = self.published.load(Ordering::SeqCst);
            self.reading.store(slot, Ordering::SeqCst);
            if self.published.load(Ordering::SeqCst) != slot {
                continue;
            }
            let values = self
                .atoms
                .iter()
                .map(|atom| atom.read(&self.slots[slot]))
                .collect();
            self.reading.store(usize::MAX, Ordering::SeqCst);
            return serde_json::to_string(&Saved {
                schema: self.schema.clone(),
                values,
            })
            .expect("validated script state");
        }
    }
}

pub(super) struct Persistence {
    pending_cells: [Box<[u64]>; 3],
    revision: (sampler_core::PlanId, (u64, u64)),
    pub(super) snapshot: Arc<Snapshot>,
}
impl Persistence {
    #[cfg(test)]
    pub(super) fn values_len(&self) -> usize { self.snapshot.addresses.len() }
    #[cfg(test)]
    pub(super) fn values_bytes(&self) -> usize {
        self.snapshot.addresses.len() * std::mem::size_of::<Address>()
            + self.snapshot.dynamic.len() * std::mem::size_of::<usize>()
            + self.pending_cells.iter().map(|mask| mask.len() * std::mem::size_of::<u64>()).sum::<usize>()
            + self.snapshot.atoms.capacity() * std::mem::size_of::<Atom>()
            + self.snapshot.slots.iter().map(|slot| slot.len() * std::mem::size_of::<AtomicU64>()).sum::<usize>()
    }
    fn new(
        runtime: &mut Runtime,
        views: &[sampler_ksp::ScriptView],
        saved: &str,
    ) -> Result<Self, CoreError> {
        let core = |error| CoreError::Invalid(format!("Script persistence: {error:?}"));
        let plan = runtime.active_plan();
        let mut state = sampler_ksp::persistent_state_buffer(views).map_err(core)?;
        // Widget values also survive recall when the author omitted make_persistent.
        for widget in runtime.widget_definitions(plan).map_err(core)? {
            let (offset, len, text) = match widget.storage {
                sampler_core::WidgetStorage::Control(id) => {
                    state.values.push(ScriptStateEntry {
                        address: Address::Control(id),
                        value: Value::Control(ControlValue::Integer(0)),
                    });
                    continue;
                }
                sampler_core::WidgetStorage::Cells { offset, len, .. } => (offset, len, false),
                sampler_core::WidgetStorage::Texts { offset, len } => (offset, len, true),
                sampler_core::WidgetStorage::FileSelection { offset } => (offset, 1, true),
            };
            for index in offset
                ..offset
                    .checked_add(len)
                    .ok_or_else(|| core(sampler_core::Error::Capacity))?
            {
                state.values.push(ScriptStateEntry {
                    address: if text {
                        Address::Text {
                            instance: widget.instance,
                            index,
                        }
                    } else {
                        Address::Cell {
                            instance: widget.instance,
                            index,
                        }
                    },
                    value: if text {
                        Value::Text(Default::default())
                    } else {
                        Value::Cell(0)
                    },
                });
            }
        }
        state.values.sort_by_key(|entry| entry.address);
        state.values.dedup_by_key(|entry| entry.address);
        runtime
            .capture_script_state(plan, &mut state)
            .map_err(core)?;
        let mut hash = blake3::Hasher::new();
        for view in views {
            hash.update(format!("{:?}", view.model().persistent).as_bytes());
        }
        for entry in &state.values {
            hash.update(
                format!("{:?}:{}", entry.address, Atom::new(entry.value, 0).kind).as_bytes(),
            );
        }
        for widget in runtime.widget_definitions(plan).map_err(core)? {
            hash.update(
                format!("{:?}:{:?}:{:?}", widget.id, widget.instance, widget.storage).as_bytes(),
            );
        }
        let schema = hash.finalize().to_hex().to_string();
        if !saved.is_empty() {
            let saved: Saved = serde_json::from_str(saved)
                .map_err(|_| CoreError::Invalid("Saved script state is malformed".into()))?;
            if saved.schema != schema || saved.values.len() != state.values.len() {
                return Err(CoreError::Invalid(
                    "Saved script state schema changed".into(),
                ));
            }
            for (entry, value) in state.values.iter_mut().zip(saved.values) {
                entry.value = match (entry.value, value) {
                    (Value::Control(ControlValue::Integer(_)), SavedValue::Integer(v)) => {
                        Value::Control(ControlValue::Integer(v))
                    }
                    (Value::Control(ControlValue::Real(_)), SavedValue::Real(v))
                        if v.is_finite() =>
                    {
                        Value::Control(ControlValue::Real(v))
                    }
                    (Value::Control(ControlValue::Toggle(_)), SavedValue::Toggle(v)) => {
                        Value::Control(ControlValue::Toggle(v))
                    }
                    (Value::Cell(_), SavedValue::Cell(v)) => Value::Cell(v),
                    (Value::Text(_), SavedValue::Text(v)) => {
                        Value::Text(sampler_core::Text::try_new(&v).map_err(core)?)
                    }
                    _ => return Err(CoreError::Invalid("Saved script state type changed".into())),
                };
            }
            runtime
                .restore_script_state(plan, None, &mut state)
                .map_err(core)?;
            runtime
                .capture_script_state(plan, &mut state)
                .map_err(core)?;
        }
        runtime.watch_script_state_values(plan, &state.values).map_err(core)?;
        let snapshot = Snapshot::new(schema, &state);
        let revision = (plan, runtime.script_state_revision(plan).map_err(core)?);
        Ok(Self {
            pending_cells: std::array::from_fn(|_| vec![0; state.values.len().div_ceil(64)].into_boxed_slice()),
            revision,
            snapshot,
        })
    }
    pub(super) fn publish(&mut self, runtime: &mut Runtime) {
        let plan = runtime.active_plan();
        let Ok(revision) = runtime.script_state_revision(plan) else {
            return;
        };
        if self.revision == (plan, revision) {
            return;
        }
        let mut valid = true;
        let known = runtime.visit_dirty_script_cells(plan, |address| {
            match self.snapshot.addresses.binary_search(&address) {
                Ok(index) => for mask in &mut self.pending_cells { mask[index / 64] |= 1 << (index % 64); },
                Err(_) => valid = false,
            }
        });
        if !valid || known.is_err() { return; }
        if known == Ok(false) || self.revision.0 != plan {
            for (index, address) in self.snapshot.addresses.iter().enumerate() {
                if matches!(address, Address::Cell { .. }) {
                    for mask in &mut self.pending_cells { mask[index / 64] |= 1 << (index % 64); }
                }
            }
        }
        let next = self.snapshot.next_slot();
        // ponytail: bulk rewrites cost O(changed cells); bound deltas if they exceed the callback budget.
        if self.snapshot.capture(runtime, plan, Some(&self.pending_cells[next])).is_ok() {
            self.pending_cells[next].fill(0);
            let _ = runtime.clear_dirty_script_cells(plan);
            self.revision = (plan, revision);
        }
    }
}

impl Part {
    pub(crate) fn prepare_persistence(
        &mut self,
        views: &[sampler_ksp::ScriptView],
        saved: &str,
    ) -> Result<(), CoreError> {
        if views.is_empty() {
            return Ok(());
        }
        let persistence = Persistence::new(&mut self.runtime, views, saved)?;
        if let Some(ingress) = self.ui_controls.as_mut() {
            ingress.persistence = Some(persistence.snapshot.clone());
            ingress.widget_values = ingress
                .widgets
                .iter()
                .filter(|w| !matches!(w.storage, sampler_core::WidgetStorage::Control(_)))
                .filter_map(|w| {
                    widget_value(&self.runtime, self.runtime.active_plan(), w)
                        .map(|value| (sampler_ui_ir::ControlId(w.id.0), value))
                })
                .collect();
        }
        self.persistence = Some(persistence);
        Ok(())
    }
    pub(crate) fn persistent_control_value(&self, id: sampler_ui_ir::ControlId) -> Option<f64> {
        self.runtime
            .control_value(self.runtime.active_plan(), sampler_core::ControlId(id.0))
            .ok()
            .map(number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_widget_cell_edits_reach_compact_persistence_without_callbacks() {
        let script = sampler_ksp::compile(
            "on init\n declare ui_table %table[2](1,1,127)\n declare ui_xy ?xy[2]\n end on\n",
            48000, sampler_ksp::Limits::LIBRARY, &[],
        ).unwrap();
        let view = script.view();
        let plan = script.bind(Prepared::new(48000, vec![], vec![], 1).unwrap()).unwrap();
        let limits = sampler_core::Limits::for_plan(&plan, 128, 8);
        let mut part = Part::new(Runtime::new(plan, limits).unwrap(), MixTree::instrument("s")).unwrap();
        part.prepare_persistence(&[view], "").unwrap();
        for (name, value, bits) in [
            ("%table", sampler_core::WidgetValue::Integer(99), 99),
            ("?xy", sampler_core::WidgetValue::Real(0.25), 0.25f64.to_bits() as i64),
        ] {
            let id = sampler_ksp::derived_control_id(0, name);
            let plan = part.runtime.active_plan();
            let widget = *part.runtime.widget_definitions(plan).unwrap().iter().find(|widget| widget.id == id).unwrap();
            let sampler_core::WidgetStorage::Cells { offset, .. } = widget.storage else { panic!("cell widget"); };
            let context = sampler_core::ControlContext {
                performance: part.runtime.performance(0).unwrap(), origin: WIRE, channels: 1,
            };
            let edits = [sampler_core::WidgetEdit { id, index: 1, value, interaction: Default::default() }];
            let persistence = part.persistence.as_mut().unwrap();
            let mut edit = || {
                assert!(part.runtime.invoke_widget(context, plan, None, &edits).unwrap().1.is_none());
                persistence.publish(&mut part.runtime);
            };
            #[cfg(feature = "plugin")]
            assert_eq!(crate::plugin::tests::allocations(edit), 0);
            #[cfg(not(feature = "plugin"))]
            edit();
            let address = Address::Cell { instance: widget.instance, index: offset + 1 };
            let index = persistence.snapshot.addresses.binary_search(&address).unwrap();
            let saved: Saved = serde_json::from_str(&persistence.snapshot.save()).unwrap();
            assert!(matches!(saved.values[index], SavedValue::Cell(v) if v == bits));
        }
    }
    #[test]
    fn one_saved_cell_write_refreshes_only_changed_values_across_snapshot_slots() {
        let script = sampler_ksp::compile(
            "on init\n declare %saved[32768]\n make_persistent(%saved)\n end on\n on note\n %saved[$EVENT_NOTE] := $EVENT_NOTE + 1\n ignore_event($EVENT_ID)\n end on\n",
            48000, sampler_ksp::Limits::LIBRARY, &[],
        ).unwrap();
        let view = script.view();
        let plan = script.bind(Prepared::new(48000, vec![], vec![], 1).unwrap()).unwrap();
        let limits = sampler_core::Limits::for_plan(&plan, 128, 8);
        let mut part = Part::new(Runtime::new(plan, limits).unwrap(), MixTree::instrument("s")).unwrap();
        part.prepare_persistence(&[view], "").unwrap();
        for key in 60..66 {
            part.runtime.trigger(sampler_core::Input {
                protocol: sampler_core::Protocol::Native, port: 0, group: 0,
                channel: 0, key, external_id: None,
            }, key, 1.).unwrap();
            let persistence = part.persistence.as_mut().unwrap();
            #[cfg(feature = "plugin")]
            assert_eq!(crate::plugin::tests::allocations(|| persistence.publish(&mut part.runtime)), 0);
            #[cfg(not(feature = "plugin"))]
            persistence.publish(&mut part.runtime);
            assert!(persistence.snapshot.captured_values.load(Ordering::Relaxed) <= 16384,
                "one scalar write must not copy the whole saved array");
            let saved: Saved = serde_json::from_str(&persistence.snapshot.save()).unwrap();
            for old in 60..=key {
                assert!(matches!(saved.values[usize::from(old)], SavedValue::Cell(v) if v == i64::from(old) + 1));
            }
        }
        let mut restore = ScriptStateBuffer {
            values: vec![ScriptStateEntry { address: Address::Cell {
                instance: sampler_core::ScriptInstanceId(0), index: 32767,
            }, value: Value::Cell(99) }], callbacks: vec![],
        };
        part.runtime.restore_script_state(part.runtime.active_plan(), None, &mut restore).unwrap();
        let persistence = part.persistence.as_mut().unwrap();
        persistence.publish(&mut part.runtime);
        assert!(persistence.snapshot.captured_values.load(Ordering::Relaxed) <= 16384);
        let saved: Saved = serde_json::from_str(&persistence.snapshot.save()).unwrap();
        assert!(matches!(saved.values[32767], SavedValue::Cell(99)));
    }
    #[test]
    fn numeric_persistence_keeps_compact_storage() {
        let script = sampler_ksp::compile(
            "on init\n declare %saved[32768]\n make_persistent(%saved)\n end on\n",
            48000, sampler_ksp::Limits::LIBRARY, &[],
        ).unwrap();
        let view = script.view();
        let plan = script.bind(Prepared::new(48000, vec![], vec![], 1).unwrap()).unwrap();
        let limits = sampler_core::Limits::for_plan(&plan, 128, 8);
        let mut part = Part::new(Runtime::new(plan, limits).unwrap(), MixTree::instrument("s")).unwrap();
        part.prepare_persistence(&[view], "").unwrap();
        let persistence = part.persistence.as_ref().unwrap();
        assert_eq!(persistence.values_len(), 32768);
        assert!(persistence.values_bytes() <= 96 * persistence.values_len(),
            "numeric persistence must not reserve inline text storage per cell: {} bytes",
            persistence.values_bytes());
    }
    #[test]
    fn rejected_live_capture_keeps_previous_snapshot() {
        let plan = Prepared::new(48000, vec![], vec![], 0).unwrap()
            .with_script_instances(vec![vec![42]]).unwrap();
        let limits = sampler_core::Limits::for_plan(&plan, 1, 1);
        let runtime = Runtime::new(plan, limits).unwrap();
        let snapshot = Snapshot::new("test".into(), &values(17));
        let published = snapshot.published.load(Ordering::SeqCst);
        let saved = snapshot.save();
        assert!(snapshot.capture(&runtime, runtime.active_plan(), None).is_err());
        assert_eq!(snapshot.published.load(Ordering::SeqCst), published);
        assert_eq!(snapshot.save(), saved);
    }
    #[test]
    fn temporary_script_writes_do_not_recapture_persistent_values() {
        let script = sampler_ksp::compile(
            "on init\n declare $saved := 17\n make_persistent($saved)\n declare $temporary := 0\n end on\n on note\n $temporary := $EVENT_NOTE\n if ($EVENT_NOTE = 61)\n $saved := 61\n end if\n ignore_event($EVENT_ID)\n end on\n",
            48000, sampler_ksp::Limits::LIBRARY, &[],
        ).unwrap();
        let view = script.view();
        let plan = script.bind(Prepared::new(48000, vec![], vec![], 1).unwrap()).unwrap();
        let limits = sampler_core::Limits::for_plan(&plan, 128, 8);
        let mut part = Part::new(Runtime::new(plan, limits).unwrap(), MixTree::instrument("s")).unwrap();
        part.prepare_persistence(&[view], "").unwrap();
        let persistence = part.persistence.as_mut().unwrap();
        assert_eq!(persistence.values_len(), 1);
        let slot = persistence.snapshot.published.load(Ordering::SeqCst);
        part.runtime.trigger(sampler_core::Input {
            protocol: sampler_core::Protocol::Native, port: 0, group: 0,
            channel: 0, key: 60, external_id: None,
        }, 60, 1.).unwrap();
        #[cfg(feature = "plugin")]
        assert_eq!(crate::plugin::tests::allocations(|| persistence.publish(&mut part.runtime)), 0);
        #[cfg(not(feature = "plugin"))]
        persistence.publish(&mut part.runtime);
        assert_eq!(persistence.snapshot.published.load(Ordering::SeqCst), slot);
        part.runtime.trigger(sampler_core::Input {
            protocol: sampler_core::Protocol::Native, port: 0, group: 0,
            channel: 0, key: 61, external_id: None,
        }, 61, 1.).unwrap();
        persistence.publish(&mut part.runtime);
        let saved: Saved = serde_json::from_str(&persistence.snapshot.save()).unwrap();
        assert!(matches!(saved.values.as_slice(), [SavedValue::Cell(61)]));
    }
    #[test]
    fn dsp_control_edit_without_script_changes_is_published() {
        use sampler_core::{
            ControlDefinition, ControlDomain, ControlOperation, ControlRequest, ControlWrite,
        };
        let id = sampler_core::ControlId(700);
        let prepared = sampler_core::Prepared::new(48000, vec![], vec![], 0)
            .unwrap()
            .with_controls(vec![ControlDefinition {
                id,
                domain: ControlDomain::Real { min: 0., max: 1. },
                default: ControlValue::Real(0.25),
            }])
            .unwrap();
        let (mut runtime, mut client) = Runtime::new(
            prepared,
            sampler_core::Limits {
                notes: 1,
                channels: 0,
                performances: 1,
                families: 0,
                expressions: 1,
                voices: 0,
                decisions: 0,
                commands: 1,
                behaviors: 1,
                behavior_fuel: 16,
                behavior_cells: 1,
                note_cells: 0,
            },
        )
        .unwrap()
        .with_control_updates(1, 1)
        .unwrap();
        let plan = runtime.active_plan();
        let state = ScriptStateBuffer {
            values: vec![ScriptStateEntry {
                address: Address::Control(id),
                value: Value::Control(ControlValue::Real(0.25)),
            }],
            callbacks: vec![],
        };
        let snapshot = Snapshot::new("dsp-only".into(), &state);
        let mut persistence = Persistence {
            pending_cells: std::array::from_fn(|_| vec![0; state.values.len().div_ceil(64)].into_boxed_slice()),
            snapshot,
            revision: (plan, runtime.script_state_revision(plan).unwrap()),
        };
        client
            .submit(ControlRequest {
                plan,
                expected_revision: None,
                operation: ControlOperation::Edit(Box::from([ControlWrite {
                    id,
                    value: ControlValue::Real(0.75),
                }])),
            })
            .unwrap();
        #[cfg(feature = "plugin")]
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                runtime.poll_control_update().unwrap();
                persistence.publish(&mut runtime);
            }),
            0
        );
        #[cfg(not(feature = "plugin"))]
        {
            runtime.poll_control_update().unwrap();
            persistence.publish(&mut runtime);
        }
        let saved: Saved = serde_json::from_str(&persistence.snapshot.save()).unwrap();
        assert!(matches!(saved.values.as_slice(), [SavedValue::Real(v)] if *v == 0.75));
        let slot = persistence.snapshot.published.load(Ordering::SeqCst);
        persistence.publish(&mut runtime);
        assert_eq!(persistence.snapshot.published.load(Ordering::SeqCst), slot);
    }
    #[test]
    fn unchanged_persistent_array_is_not_recaptured_or_published() {
        let prepared = sampler_core::Prepared::new(48000, vec![], vec![], 0)
            .unwrap()
            .with_script_instances(vec![vec![17; 32768]])
            .unwrap();
        let mut runtime = Runtime::new(
            prepared,
            sampler_core::Limits {
                notes: 1,
                channels: 0,
                performances: 1,
                families: 0,
                expressions: 1,
                voices: 0,
                decisions: 0,
                commands: 1,
                behaviors: 1,
                behavior_fuel: 16,
                behavior_cells: 1,
                note_cells: 0,
            },
        )
        .unwrap();
        let mut state = ScriptStateBuffer {
            values: (0..32768)
                .map(|index| ScriptStateEntry {
                    address: Address::Cell {
                        instance: sampler_core::ScriptInstanceId(0),
                        index,
                    },
                    value: Value::Cell(0),
                })
                .collect(),
            callbacks: vec![],
        };
        runtime
            .capture_script_state(runtime.active_plan(), &mut state)
            .unwrap();
        let snapshot = Snapshot::new("large-array".into(), &state);
        let mut persistence = Persistence {
            pending_cells: std::array::from_fn(|_| vec![0; state.values.len().div_ceil(64)].into_boxed_slice()),
            snapshot,
            revision: (
                runtime.active_plan(),
                runtime
                    .script_state_revision(runtime.active_plan())
                    .unwrap(),
            ),
        };
        let slot = persistence.snapshot.published.load(Ordering::SeqCst);
        #[cfg(feature = "plugin")]
        assert_eq!(
            crate::plugin::tests::allocations(|| persistence.publish(&mut runtime)),
            0
        );
        #[cfg(not(feature = "plugin"))]
        persistence.publish(&mut runtime);
        assert_eq!(persistence.snapshot.published.load(Ordering::SeqCst), slot);
        state.values[0].value = Value::Cell(101);
        let mut restore_and_publish = || {
            runtime
                .restore_script_state(runtime.active_plan(), None, &mut state)
                .unwrap();
            persistence.publish(&mut runtime);
        };
        #[cfg(feature = "plugin")]
        assert_eq!(crate::plugin::tests::allocations(restore_and_publish), 0);
        #[cfg(not(feature = "plugin"))]
        restore_and_publish();
        let slot = persistence.snapshot.published.load(Ordering::SeqCst);
        assert_eq!(
            persistence.snapshot.slots[slot][0].load(Ordering::SeqCst),
            101
        );
    }
    fn values(value: i64) -> ScriptStateBuffer {
        ScriptStateBuffer {
            values: vec![
                ScriptStateEntry {
                    address: Address::Cell {
                        instance: sampler_core::ScriptInstanceId(0),
                        index: 0,
                    },
                    value: Value::Cell(value),
                },
                ScriptStateEntry {
                    address: Address::Cell {
                        instance: sampler_core::ScriptInstanceId(0),
                        index: 1,
                    },
                    value: Value::Cell(-value),
                },
                ScriptStateEntry {
                    address: Address::Text {
                        instance: sampler_core::ScriptInstanceId(0),
                        index: 0,
                    },
                    value: Value::Text(sampler_core::Text::new(&format!("声{value}"))),
                },
            ],
            callbacks: vec![],
        }
    }
    #[test]
    fn concurrent_host_saves_never_mix_snapshot_generations() {
        let snapshot = Snapshot::new("test".into(), &values(0));
        let writer = snapshot.clone();
        let thread = std::thread::spawn(move || {
            for n in 1..10000 {
                writer.publish(&values(n));
            }
        });
        for _ in 0..2000 {
            let saved: Saved = serde_json::from_str(&snapshot.save()).unwrap();
            let [
                SavedValue::Cell(a),
                SavedValue::Cell(b),
                SavedValue::Text(text),
            ] = saved.values.as_slice()
            else {
                panic!("snapshot types changed")
            };
            assert!(
                *a == -*b && text == &format!("声{a}"),
                "host snapshot must be coherent"
            );
        }
        thread.join().unwrap();
    }

    #[test]
    fn host_snapshot_preserves_exact_scalar_bits_and_utf8() {
        let state = ScriptStateBuffer {
            values: vec![
                ScriptStateEntry {
                    address: Address::Control(sampler_core::ControlId(1)),
                    value: Value::Control(ControlValue::Integer(i64::MAX)),
                },
                ScriptStateEntry {
                    address: Address::Control(sampler_core::ControlId(2)),
                    value: Value::Control(ControlValue::Real(-0.)),
                },
                ScriptStateEntry {
                    address: Address::Control(sampler_core::ControlId(3)),
                    value: Value::Control(ControlValue::Toggle(true)),
                },
                ScriptStateEntry {
                    address: Address::Cell {
                        instance: sampler_core::ScriptInstanceId(0),
                        index: 0,
                    },
                    value: Value::Cell(i64::MIN),
                },
                ScriptStateEntry {
                    address: Address::Text {
                        instance: sampler_core::ScriptInstanceId(0),
                        index: 0,
                    },
                    value: Value::Text(sampler_core::Text::new("声 🎹")),
                },
            ],
            callbacks: vec![],
        };
        let saved: Saved =
            serde_json::from_str(&Snapshot::new("test".into(), &state).save()).unwrap();
        assert!(
            matches!(saved.values.as_slice(),[SavedValue::Integer(i64::MAX),SavedValue::Real(real),SavedValue::Toggle(true),SavedValue::Cell(i64::MIN),SavedValue::Text(text)] if real.to_bits()==(-0f64).to_bits() && text=="声 🎹")
        );
    }
}
