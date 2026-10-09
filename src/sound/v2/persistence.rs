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
    fn write(&self, slot: &[AtomicU64], value: Value) {
        let words = &slot[self.offset..self.offset + self.len];
        let set = |index: usize, value| {
            if words[index].load(Ordering::Relaxed) != value {
                words[index].store(value, Ordering::SeqCst);
            }
        };
        match value {
            Value::Control(ControlValue::Integer(v)) | Value::Cell(v) => set(0, v as u64),
            Value::Control(ControlValue::Real(v)) => set(0, v.to_bits()),
            Value::Control(ControlValue::Toggle(v)) => set(0, u64::from(v)),
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
            slots: std::array::from_fn(|_| (0..size).map(|_| AtomicU64::new(0)).collect()),
        });
        snapshot.publish(state);
        snapshot
    }
    fn publish(&self, state: &ScriptStateBuffer) {
        let current = self.published.load(Ordering::SeqCst);
        let mut next = (current + 1) % 3;
        if next == self.reading.load(Ordering::SeqCst) {
            next = (next + 1) % 3;
        }
        for (atom, entry) in self.atoms.iter().zip(&state.values) {
            atom.write(&self.slots[next], entry.value);
        }
        self.published.store(next, Ordering::SeqCst);
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
    state: ScriptStateBuffer,
    pub(super) snapshot: Arc<Snapshot>,
}
impl Persistence {
    #[cfg(test)]
    pub(super) fn values_len(&self) -> usize { self.state.values.len() }
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
        let snapshot = Snapshot::new(schema, &state);
        Ok(Self { state, snapshot })
    }
    pub(super) fn publish(&mut self, runtime: &Runtime) {
        if runtime
            .capture_script_state(runtime.active_plan(), &mut self.state)
            .is_ok()
        {
            self.snapshot.publish(&self.state);
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
    fn unchanged_persistent_array_is_not_recaptured_or_published() {
        let prepared = sampler_core::Prepared::new(48000, vec![], vec![], 0)
            .unwrap()
            .with_script_instances(vec![vec![17; 32768]])
            .unwrap();
        let runtime = Runtime::new(prepared, sampler_core::Limits {
            notes: 1, channels: 0, performances: 1, families: 0,
            expressions: 1, voices: 0, decisions: 0, commands: 1,
            behaviors: 1, behavior_fuel: 16, behavior_cells: 1, note_cells: 0,
        }).unwrap();
        let mut state = ScriptStateBuffer {
            values: (0..32768).map(|index| ScriptStateEntry {
                address: Address::Cell { instance: sampler_core::ScriptInstanceId(0), index },
                value: Value::Cell(0),
            }).collect(),
            callbacks: vec![],
        };
        runtime.capture_script_state(runtime.active_plan(), &mut state).unwrap();
        let snapshot = Snapshot::new("large-array".into(), &state);
        let mut persistence = Persistence { state, snapshot };
        let slot = persistence.snapshot.published.load(Ordering::SeqCst);
        // Poison only the staging buffer: an unchanged block must never visit it.
        persistence.state.values[0].value = Value::Cell(-123);
        persistence.publish(&runtime);
        assert_eq!(persistence.state.values[0].value, Value::Cell(-123));
        assert_eq!(persistence.snapshot.published.load(Ordering::SeqCst), slot);
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
