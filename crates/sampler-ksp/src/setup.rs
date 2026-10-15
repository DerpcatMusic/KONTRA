//! Identity and capture requirements for a live script setup. This metadata
//! does not authorize snapshot hot-apply or claim that values capture UI stores.
use crate::{
    ScriptView,
    model::{Location, Persistence, SnapshotMode, SnapshotModeWrite},
};
use sampler_core::{
    ControlId, Error, ScriptInstanceId, ScriptStateAddress as Address, ScriptStateBuffer,
    ScriptStateEntry, ScriptStateValue, WidgetDefinition, WidgetStorage,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
mod snapshot;
pub use snapshot::{SnapshotSavedError, SnapshotSavedLimits, SnapshotSavedRowError, SnapshotSavedSlot, snapshot_saved_state};

pub(crate) fn source_digest(source: &str) -> [u8; 32] {
    Sha256::digest(source.as_bytes()).into()
}

fn digest_fields(args: std::fmt::Arguments<'_>) -> [u8; 32] {
    struct Sink(Sha256);
    impl std::fmt::Write for Sink {
        fn write_str(&mut self, value: &str) -> std::fmt::Result {
            self.0.update(value.as_bytes());
            Ok(())
        }
    }
    let mut sink = Sink(Sha256::new());
    std::fmt::write(&mut sink, args).expect("hash writer");
    sink.0.finalize().into()
}

/// An in-process same-source/layout gate. Base instrument, resource authority
/// and generation identity must also match before any future live transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetupIdentity {
    pub source: [u8; 32],
    pub layout: [u8; 32],
    pub context: [u8; 32],
    pub source_slot: u8,
    pub sample_rate: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupValueKind {
    Integer,
    Real,
    Text,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SetupField {
    pub name: String,
    pub kind: SetupValueKind,
    pub storage: Location,
    pub persistence: Persistence,
}

#[derive(Clone, Debug)]
pub struct SetupLayout {
    pub identity: SetupIdentity,
    /// Authored plan-owned fields only. Constants, note-owned polyphonic cells
    /// and callback scratch registers/texts are excluded.
    pub fields: Vec<SetupField>,
    pub numeric_globals: u32,
    pub text_globals: u32,
    pub controls: Vec<ControlId>,
    pub control_domains: Vec<(ControlId, sampler_core::ControlDomain)>,
    pub private_store_capacity: usize,
    pub shared_initial_entries: usize,
}

impl SetupLayout {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compiled<'a>(
        source: [u8; 32],
        rate: u32,
        hir: &crate::hir::Hir,
        init: &crate::eval::Initial,
        environment: &crate::Environment,
        conditions: &BTreeSet<String>,
        ids: &[Option<ControlId>],
        controls: impl Iterator<Item = &'a crate::Control>,
        resources: &sampler_core::ScriptResources,
        shared_initial_entries: usize,
    ) -> Self {
        use crate::hir::{Home, Ty};
        let fields: Vec<_> = hir
            .vars
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                let storage = match v.home {
                    Home::Cell(offset) => Location::Cells { offset, len: 1 },
                    Home::Cells { offset, len } => Location::Cells { offset, len },
                    Home::Text(offset) => Location::Texts { offset, len: 1 },
                    Home::Texts { offset, len } => Location::Texts { offset, len },
                    Home::Control(index) => Location::Control(*ids.get(index as usize)?.as_ref()?),
                    Home::Note(_) | Home::Const(_) => return None,
                };
                Some(SetupField {
                    name: v.name.to_string(),
                    kind: match v.ty {
                        Ty::Real => SetupValueKind::Real,
                        Ty::Str => SetupValueKind::Text,
                        _ => SetupValueKind::Integer,
                    },
                    storage,
                    persistence: init.persistence[i],
                })
            })
            .collect();
        let controls: Vec<_> = controls
            .map(|c| (c.definition.id, c.definition.domain, c.widget))
            .collect();
        // Values/defaults are deliberately absent: the same declared setup may
        // carry different snapshot values. Domains and actual control bindings
        // remain part of the schema. Context excludes native engine values.
        let layout = digest_fields(format_args!(
            "KSP setup layout 1|{}|{}|{fields:?}|{controls:?}|{:?}|{}",
            hir.cells, hir.texts, resources.controls, resources.store_capacity
        ));
        let context = digest_fields(format_args!(
            "KSP setup context 1|{conditions:?}|{:?}|{:?}|{:?}|{:?}",
            environment.groups,
            environment.zones,
            environment.engine_lookups,
            environment.performance_view
        ));
        Self {
            identity: SetupIdentity {
                source,
                layout,
                context,
                source_slot: environment.slot,
                sample_rate: rate,
            },
            fields,
            numeric_globals: hir.cells,
            text_globals: hir.texts,
            controls: controls.iter().map(|c| c.0).collect(),
            control_domains: controls.iter().map(|c| (c.0, c.1)).collect(),
            private_store_capacity: resources.store_capacity,
            shared_initial_entries,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlobalSnapshotPolicy {
    pub mode: SnapshotMode,
    pub writer: Option<(u8, SnapshotModeWrite)>,
}

/// Kontakt's one global policy, considering explicit executed writes in physical
/// slot order. A slot that made no write cannot reset a previous slot to zero.
pub fn snapshot_policy(views: &[ScriptView]) -> Result<GlobalSnapshotPolicy, Error> {
    let mut slots = BTreeMap::new();
    for view in views {
        if view.slot >= 5 || slots.insert(view.slot, view).is_some() {
            return Err(Error::InvalidInput);
        }
    }
    let mut policy = GlobalSnapshotPolicy {
        mode: SnapshotMode::NativeWithInit,
        writer: None,
    };
    for (slot, view) in slots {
        if let Some(write) = view.model.snapshot_mode_write {
            policy = GlobalSnapshotPolicy {
                mode: write.mode,
                writer: Some((slot, write)),
            };
        }
    }
    Ok(policy)
}

#[derive(Clone, Copy, Debug)]
pub struct SetupCaptureLimits {
    pub value_entries: usize,
    /// UTF-8 payload capacity of text buffers, including private properties.
    /// Metadata/container overhead is separate; this is not an RSS limit.
    pub text_bytes: usize,
    pub sparse_entries: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupStoreDomain {
    PrivateNumeric,
    PrivateTextProperties,
    SharedPgsNumeric,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetupStoreRequirement {
    pub instance: Option<ScriptInstanceId>,
    pub domain: SetupStoreDomain,
    pub capacity: usize,
}

/// A bounded roster, not a complete captured setup. `values` can use existing
/// capture APIs. The listed sparse stores and current ScriptView models require
/// a coherent generation/revision-aware capture before setup reapply is safe.
#[derive(Debug)]
pub struct SetupCaptureRoster {
    pub identities: Vec<SetupIdentity>,
    pub values: ScriptStateBuffer,
    pub stores: Vec<SetupStoreRequirement>,
    pub persistence_programs: Vec<usize>,
}

pub fn capture_roster(
    views: &[ScriptView],
    widgets: &[WidgetDefinition],
    limits: SetupCaptureLimits,
) -> Result<SetupCaptureRoster, Error> {
    let mut addresses = BTreeSet::new();
    let mut add = |address| {
        if !addresses.contains(&address) && addresses.len() >= limits.value_entries {
            return Err(Error::Capacity);
        }
        addresses.insert(address);
        Ok(())
    };
    let mut stores = Vec::new();
    let mut sparse = 4096usize;
    let mut programs = Vec::new();
    let mut base = 0usize;
    let mut slots = BTreeSet::new();
    for (i, view) in views.iter().enumerate() {
        if !slots.insert(view.slot) {
            return Err(Error::InvalidInput);
        }
        let instance = ScriptInstanceId(u16::try_from(i).map_err(|_| Error::Capacity)?);
        let setup = view.setup();
        for index in 0..setup.numeric_globals {
            add(Address::Cell { instance, index })?;
        }
        for index in 0..setup.text_globals {
            add(Address::Text { instance, index })?;
        }
        for &id in &setup.controls {
            add(Address::Control(id))?;
        }
        sparse = sparse
            .checked_add(
                setup
                    .private_store_capacity
                    .checked_mul(2)
                    .ok_or(Error::Capacity)?,
            )
            .and_then(|n| n.checked_add(setup.shared_initial_entries))
            .ok_or(Error::Capacity)?;
        if sparse > limits.sparse_entries {
            return Err(Error::Capacity);
        }
        for domain in [
            SetupStoreDomain::PrivateNumeric,
            SetupStoreDomain::PrivateTextProperties,
        ] {
            stores.push(SetupStoreRequirement {
                instance: Some(instance),
                domain,
                capacity: setup.private_store_capacity,
            });
        }
        if let Some(entry) = view
            .entries
            .iter()
            .find(|e| e.kind == crate::EntryKind::PersistenceChanged)
        {
            programs.push(base.checked_add(entry.program).ok_or(Error::Capacity)?);
        }
        base = base.checked_add(view.programs).ok_or(Error::Capacity)?;
    }
    let shared = views.iter().try_fold(4096usize, |n, v| {
        n.checked_add(v.setup().shared_initial_entries)
            .ok_or(Error::Capacity)
    })?;
    if sparse > limits.sparse_entries {
        return Err(Error::Capacity);
    }
    stores.push(SetupStoreRequirement {
        instance: None,
        domain: SetupStoreDomain::SharedPgsNumeric,
        capacity: shared,
    });
    for widget in widgets {
        let i = usize::from(widget.instance.0);
        if views.get(i).is_none_or(|v| v.slot != widget.source_slot) {
            return Err(Error::InvalidInput);
        }
        match widget.storage {
            WidgetStorage::Control(id) => add(Address::Control(id))?,
            WidgetStorage::Cells { offset, len, .. } => {
                for index in offset..offset.checked_add(len).ok_or(Error::Capacity)? {
                    add(Address::Cell {
                        instance: widget.instance,
                        index,
                    })?;
                }
            }
            WidgetStorage::Texts { offset, len } => {
                for index in offset..offset.checked_add(len).ok_or(Error::Capacity)? {
                    add(Address::Text {
                        instance: widget.instance,
                        index,
                    })?;
                }
            }
            WidgetStorage::FileSelection { offset } => add(Address::Text {
                instance: widget.instance,
                index: offset,
            })?,
        }
        // Drag/drop payload buffers, callback PCs and event ownership are transient.
    }
    let private_texts = stores
        .iter()
        .filter(|s| s.domain == SetupStoreDomain::PrivateTextProperties)
        .try_fold(0usize, |n, s| {
            n.checked_add(s.capacity).ok_or(Error::Capacity)
        })?;
    let texts = addresses
        .iter()
        .filter(|a| matches!(a, Address::Text { .. }))
        .count();
    if texts
        .checked_add(private_texts)
        .and_then(|n| n.checked_mul(sampler_core::TEXT_CAPACITY))
        .is_none_or(|n| n > limits.text_bytes)
    {
        return Err(Error::Capacity);
    }
    let mut values = ScriptStateBuffer::default();
    values.values = addresses
        .into_iter()
        .map(|address| {
            let value = match address {
                Address::Control(_) => {
                    ScriptStateValue::Control(sampler_core::ControlValue::Integer(0))
                }
                Address::Cell { .. } => ScriptStateValue::Cell(0),
                Address::Text { .. } => values.push_text(Default::default()),
            };
            ScriptStateEntry { address, value }
        })
        .collect();
    Ok(SetupCaptureRoster {
        identities: views.iter().map(|v| v.setup().identity).collect(),
        values,
        stores,
        persistence_programs: programs,
    })
}
