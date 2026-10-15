//! Bounded native-snapshot saved rows. No evaluator or initialization is run.
use super::{SetupField, SetupValueKind};
use crate::{
    ScriptView,
    model::{Location, Persistence},
};
use sampler_core::{
    ControlDomain, ControlValue, ScriptInstanceId, ScriptStateAddress as A, ScriptStateBuffer,
    ScriptStateCallback, ScriptStateEntry, ScriptStateValue as V, Text,
};
use sampler_ir::Saved;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct SnapshotSavedSlot {
    pub slot: u8,
    /// Raw rows retain duplicates so malformed input cannot silently win.
    pub values: Vec<(String, Saved)>,
}

#[derive(Clone, Copy, Debug)]
pub struct SnapshotSavedLimits {
    pub value_entries: usize,
    /// Fixed-capacity Text backing, rather than just current UTF-8 lengths.
    pub text_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotSavedRowError {
    Duplicate,
    Missing,
    NotSnapshotPersistent,
    Type,
    Count,
    Domain,
    TextCapacity,
    /// Native menus save an item position; the mutable live menu roster owns
    /// its semantic value and has no atomic roster fence in this first slice.
    MenuRosterUnverified,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotSavedError {
    InvalidSlots,
    InvalidLayout,
    Capacity,
    Row {
        slot: u8,
        name: String,
        reason: SnapshotSavedRowError,
    },
}
impl std::fmt::Display for SnapshotSavedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SnapshotSavedError {}

fn row_error(slot: u8, name: &str, reason: SnapshotSavedRowError) -> SnapshotSavedError {
    SnapshotSavedError::Row {
        slot,
        name: name.to_owned(),
        reason,
    }
}

fn shape(field: &SetupField, value: &Saved) -> Result<usize, SnapshotSavedRowError> {
    use SnapshotSavedRowError as E;
    let (kind, array, count) = match value {
        Saved::Int(value) => {
            if i32::try_from(*value).is_err() {
                return Err(E::Domain);
            }
            (SetupValueKind::Integer, false, 1)
        }
        Saved::Real(value) => {
            if !value.is_finite() {
                return Err(E::Domain);
            }
            (SetupValueKind::Real, false, 1)
        }
        Saved::Text(value) => {
            if value.len() > sampler_core::TEXT_CAPACITY {
                return Err(E::TextCapacity);
            }
            (SetupValueKind::Text, false, 1)
        }
        Saved::Ints(values) => {
            if values.iter().any(|value| i32::try_from(*value).is_err()) {
                return Err(E::Domain);
            }
            (SetupValueKind::Integer, true, values.len())
        }
        Saved::Reals(values) => {
            if values.iter().any(|value| !value.is_finite()) {
                return Err(E::Domain);
            }
            (SetupValueKind::Real, true, values.len())
        }
        Saved::Texts(values) => {
            if values
                .iter()
                .any(|value| value.len() > sampler_core::TEXT_CAPACITY)
            {
                return Err(E::TextCapacity);
            }
            (SetupValueKind::Text, true, values.len())
        }
    };
    let sigil = field.name.as_bytes().first().copied().ok_or(E::Type)?;
    if field.kind != kind
        || !matches!(
            (sigil, kind, array),
            (b'$', SetupValueKind::Integer, false)
                | (b'~', SetupValueKind::Real, false)
                | (b'@', SetupValueKind::Text, false)
                | (b'%', SetupValueKind::Integer, true)
                | (b'?', SetupValueKind::Real, true)
                | (b'!', SetupValueKind::Text, true)
        )
    {
        return Err(E::Type);
    }
    let expected = match field.storage {
        Location::Control(_) => 1,
        Location::Cells { len, .. } | Location::Texts { len, .. } => len as usize,
    };
    if count != expected {
        return Err(E::Count);
    }
    Ok(count)
}

/// Map exact source names to current dense bank homes. Only Snapshot persistence
/// is writable. Every expected row is required, with no coercion, broadcasting,
/// truncation, or initialization fallback. Physical slots may be sparse (0/4).
pub fn snapshot_saved_state(
    views: &[ScriptView],
    incoming: &[SnapshotSavedSlot],
    limits: SnapshotSavedLimits,
) -> Result<ScriptStateBuffer, SnapshotSavedError> {
    use SnapshotSavedError as E;
    let mut slots = BTreeMap::new();
    for (instance, view) in views.iter().enumerate() {
        if view.slot >= 5 || slots.insert(view.slot, (instance, view)).is_some() {
            return Err(E::InvalidSlots);
        }
    }
    let mut rows = BTreeMap::new();
    let mut incoming_slots = BTreeSet::new();
    for saved in incoming {
        if saved.slot >= 5
            || !incoming_slots.insert(saved.slot)
            || (!slots.contains_key(&saved.slot) && !saved.values.is_empty())
        {
            return Err(E::InvalidSlots);
        }
        for (name, value) in &saved.values {
            if rows.insert((saved.slot, name.as_str()), value).is_some() {
                return Err(row_error(
                    saved.slot,
                    name,
                    SnapshotSavedRowError::Duplicate,
                ));
            }
            let Some((_, view)) = slots.get(&saved.slot) else {
                return Err(E::InvalidSlots);
            };
            if !view
                .setup()
                .fields
                .iter()
                .any(|field| field.name == *name && field.persistence == Persistence::Snapshot)
            {
                return Err(row_error(
                    saved.slot,
                    name,
                    SnapshotSavedRowError::NotSnapshotPersistent,
                ));
            }
        }
    }
    let (mut count, mut text_count) = (0usize, 0usize);
    for view in views {
        let mut names = BTreeSet::new();
        for field in view
            .setup()
            .fields
            .iter()
            .filter(|field| field.persistence == Persistence::Snapshot)
        {
            if view.model().interface.widgets.iter().any(|widget| {
                widget.name == field.name && widget.kind == crate::model::WidgetKind::Menu
            }) {
                return Err(row_error(
                    view.slot,
                    &field.name,
                    SnapshotSavedRowError::MenuRosterUnverified,
                ));
            }
            if !names.insert(field.name.as_str()) {
                return Err(E::InvalidLayout);
            }
            let value = rows
                .get(&(view.slot, field.name.as_str()))
                .ok_or_else(|| row_error(view.slot, &field.name, SnapshotSavedRowError::Missing))?;
            let len =
                shape(field, value).map_err(|reason| row_error(view.slot, &field.name, reason))?;
            count = count.checked_add(len).ok_or(E::Capacity)?;
            if field.kind == SetupValueKind::Text {
                text_count = text_count.checked_add(len).ok_or(E::Capacity)?;
            }
            if let Location::Control(id) = field.storage {
                let Saved::Int(value) = value else {
                    return Err(E::InvalidLayout);
                };
                if !view.setup().control_domains.iter().any(|(control, domain)| *control == id && matches!(domain, ControlDomain::Integer { min, max } if min <= value && value <= max)) {
                    return Err(row_error(view.slot, &field.name, SnapshotSavedRowError::Domain));
                }
            }
        }
    }
    if count
        > limits
            .value_entries
            .min(sampler_core::SNAPSHOT_OVERLAY_VALUE_CAPACITY)
        || text_count
            .checked_mul(sampler_core::TEXT_CAPACITY)
            .is_none_or(|bytes| bytes > limits.text_bytes)
    {
        return Err(E::Capacity);
    }
    let mut state = ScriptStateBuffer {
        values: Vec::with_capacity(count),
        callbacks: Vec::with_capacity(views.len()),
        texts: Vec::with_capacity(text_count),
    };
    let mut base = 0usize;
    for (instance, view) in views.iter().enumerate() {
        let instance = ScriptInstanceId(instance as u16);
        for field in view
            .setup()
            .fields
            .iter()
            .filter(|field| field.persistence == Persistence::Snapshot)
        {
            let saved = rows[&(view.slot, field.name.as_str())];
            let len = match field.storage {
                Location::Control(_) => 1,
                Location::Cells { len, .. } | Location::Texts { len, .. } => len,
            };
            for at in 0..len {
                let address = match field.storage {
                    Location::Control(id) => A::Control(id),
                    Location::Cells { offset, .. } => A::Cell {
                        instance,
                        index: offset.checked_add(at).ok_or(E::InvalidLayout)?,
                    },
                    Location::Texts { offset, .. } => A::Text {
                        instance,
                        index: offset.checked_add(at).ok_or(E::InvalidLayout)?,
                    },
                };
                let value = match saved {
                    Saved::Int(value) if matches!(address, A::Control(_)) => {
                        V::Control(ControlValue::Integer(*value))
                    }
                    Saved::Int(value) => V::Cell(*value),
                    Saved::Real(value) => V::Cell(sampler_core::real_bits(*value)),
                    Saved::Text(value) => state.push_text(Text::new(value)),
                    Saved::Ints(values) => V::Cell(values[at as usize]),
                    Saved::Reals(values) => V::Cell(sampler_core::real_bits(values[at as usize])),
                    Saved::Texts(values) => state.push_text(Text::new(&values[at as usize])),
                };
                state.values.push(ScriptStateEntry { address, value });
            }
        }
        if let Some(entry) = view
            .entries
            .iter()
            .find(|entry| entry.kind == crate::EntryKind::PersistenceChanged)
        {
            state.callbacks.push(ScriptStateCallback {
                program: base.checked_add(entry.program).ok_or(E::Capacity)?,
                behavior: None,
                outcome: None,
            });
        }
        base = base.checked_add(view.programs).ok_or(E::Capacity)?;
    }
    state.values.sort_unstable_by_key(|entry| entry.address);
    if state
        .values
        .windows(2)
        .any(|rows| rows[0].address == rows[1].address)
    {
        return Err(E::InvalidLayout);
    }
    Ok(state)
}
