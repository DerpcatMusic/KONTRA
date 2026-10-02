//! Worker-prepared storage for successfully applied native script edits.
use super::params::Address;
use crate::ksp::EnginePar;
use rustc_hash::FxHashMap;

#[derive(Clone, Copy, PartialEq)]
struct Record {
    par: EnginePar,
    value: i32,
    current: f32,
    order: u64,
}

// Intensity aliases encode the same physical target with different value laws.
// Keep the last writer's native address/value so restoring preserves its law.
fn key(address: Address) -> Address {
    match address {
        Address::Intensity { group, index, .. } => Address::Intensity {
            group,
            index,
            bipolar: false,
            cubic: None,
        },
        Address::InternalIntensity {
            group,
            envelope,
            target,
            ..
        }
        | Address::LegacyInternalIntensity {
            group,
            envelope,
            target,
        } => Address::InternalIntensity {
            group,
            envelope,
            target,
            bipolar: false,
            cubic: None,
        },
        _ => address,
    }
}

#[derive(Default)]
pub(crate) struct NativeState {
    index: FxHashMap<Address, usize>,
    records: Vec<Record>,
    active: Vec<usize>,
    order: u64,
    misses: u64,
    last_miss: Option<EnginePar>,
}

impl NativeState {
    // Called only by the loader, for addresses with real writable storage.
    pub(super) fn prepare(&mut self, address: Address, par: EnginePar, value: i32) {
        let current = address.decode(value);
        let address = key(address);
        if self.index.contains_key(&address) {
            return;
        }
        self.index.insert(address, self.records.len());
        self.active.reserve(self.records.len() + 1);
        self.records.push(Record {
            par,
            value,
            current,
            order: 0,
        });
    }

    // No insertion, growth, formatting or allocation on the audio thread.
    pub(super) fn capture(&mut self, address: Address, par: EnginePar, value: i32, current: f32) {
        if matches!(
            address,
            Address::GroupType(..) | Address::Fx(_, _, crate::fx::FxParam::Type)
        ) {
            return;
        }
        if let Some(&i) = self.index.get(&key(address)) {
            let record = &mut self.records[i];
            if record.current == current
                && (record.order == 0 || (record.par == par && record.value == value))
            {
                return;
            }
            if record.order == 0 {
                self.active.push(i);
            }
            self.order = self.order.saturating_add(1);
            *record = Record {
                par,
                value,
                current,
                order: self.order,
            };
        } else {
            self.misses = self.misses.saturating_add(1);
            self.last_miss = Some(par);
        }
    }

    pub(super) fn restored(&mut self, address: Address, par: EnginePar, value: i32) {
        // A restored edit remains saved even when it equals today's baseline.
        if let Some(&i) = self.index.get(&key(address)) {
            if self.records[i].order == 0 {
                self.active.push(i);
            }
            self.order = self.order.saturating_add(1);
            self.records[i] = Record {
                par,
                value,
                current: address.decode(value),
                order: self.order,
            };
        }
    }

    pub(super) fn replay_fx(&self, fx: &mut crate::fx::FxProcessor) {
        // Effect rebuilds also restore edits made after the last host snapshot.
        // Resolved global FX addresses need no group lookup or allocation.
        for record in self.active.iter().map(|&i| &self.records[i]) {
            if let Some(address @ Address::Fx(rack, slot, par)) = Address::resolve(record.par, &[])
            {
                fx.set_param(rack, slot, par, address.decode(record.value));
            }
        }
    }

    pub(crate) fn snapshot(&self) -> NativeSnapshot {
        NativeSnapshot {
            records: self.records.clone(),
            ..Default::default()
        }
    }

    pub(crate) fn capacity(&self) -> (usize, usize) {
        (
            self.records.len(),
            self.records.capacity() * std::mem::size_of::<Record>()
                + self.active.capacity() * std::mem::size_of::<usize>()
                + self.index.capacity()
                    * (std::mem::size_of::<Address>() + std::mem::size_of::<usize>() + 1),
        )
    }

    pub(crate) fn refresh(&self, saved: &mut NativeSnapshot, budget: usize) -> bool {
        // Source epochs are checked before this call; shapes never grow in place.
        if saved.records.len() != self.records.len() {
            saved.misses = self.misses.saturating_add(1);
            saved.changed = true;
            return true;
        }
        let end = (saved.at + budget).min(self.active.len());
        for &i in &self.active[saved.at..end] {
            saved.changed |= saved.records[i] != self.records[i];
            saved.records[i] = self.records[i];
        }
        saved.at = end;
        saved.changed |= saved.misses != self.misses;
        saved.misses = self.misses;
        saved.last_miss = self.last_miss;
        end == self.active.len()
    }
}

#[derive(Clone, Default)]
pub(crate) struct NativeSnapshot {
    records: Vec<Record>,
    at: usize,
    pub(crate) changed: bool,
    pub(crate) misses: u64,
    pub(crate) last_miss: Option<EnginePar>,
    pub(crate) reported_misses: u64,
}

impl NativeSnapshot {
    // Worker only: compact and order changed records; untouched catalog stays here.
    pub(crate) fn saved(&self) -> Vec<crate::ksp::engine::NativeEdit> {
        let mut edits: Vec<_> = self.records.iter().filter(|r| r.order != 0).collect();
        edits.sort_unstable_by_key(|r| r.order);
        edits
            .into_iter()
            .map(|r| crate::ksp::engine::NativeEdit {
                par: r.par,
                value: r.value,
            })
            .collect()
    }
    pub(crate) fn rewind(&mut self) {
        self.at = 0;
        self.changed = false;
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

#[cfg(all(test, feature = "plugin"))]
mod tests {
    use super::*;
    use crate::engine::params::{GroupPar, id};

    #[test]
    fn sparse_native_refresh_visits_two_edits_not_thirty_thousand_default_slots() {
        let mut state = NativeState::default();
        let par = |g| EnginePar {
            id: id::VOLUME,
            group: g,
            slot: -1,
            generic: -1,
        };
        for g in 0..30_000 {
            state.prepare(
                Address::Group(g, GroupPar::Volume),
                par(i32::from(g)),
                1_000_000,
            );
        }
        let mut saved = state.snapshot();
        assert!(saved.saved().is_empty());
        let volume = Address::Group(0, GroupPar::Volume);
        let decoded = [125000, 250000, 500000].map(|value| volume.decode(value));
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                state.capture(volume, par(0), 125000, decoded[0]);
                state.capture(
                    Address::Group(29999, GroupPar::Volume),
                    par(29999),
                    250000,
                    decoded[1],
                );
                // Alias writes and restored edits share the same physical slot.
                state.capture(
                    Address::Group(0, GroupPar::Volume),
                    EnginePar { slot: 7, ..par(0) },
                    500000,
                    decoded[2],
                );
                state.restored(Address::Group(0, GroupPar::Volume), par(0), 500000);
                assert_eq!(state.active.len(), 2);
                assert!(state.refresh(&mut saved, 2));
            }),
            0
        );
        assert!(saved.changed);
        assert_eq!(
            saved.saved(),
            vec![
                crate::ksp::engine::NativeEdit {
                    par: par(29999),
                    value: 250000
                },
                crate::ksp::engine::NativeEdit {
                    par: par(0),
                    value: 500000
                }
            ]
        );
        saved.rewind();
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                assert!(state.refresh(&mut saved, 2));
            }),
            0
        );
        assert!(!saved.changed);
        assert_eq!(state.misses, 0);
    }
}
