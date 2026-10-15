//! Native ordinals remain unresolved until the whole-batch audio boundary.
use crate::{ControlId, ControlValue, Error, Generation, WidgetStorage};

/// Existing frontend sparse homes, never a copied UI/model roster. The item
/// ordinal replaces element 2 of `value_key`; count uses its exact key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidgetMenuStorage {
    pub count_key: [i32; crate::STORE_KEY],
    pub value_key: [i32; crate::STORE_KEY],
}
/// A native saved ordinal, distinct from a semantic control scalar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotMenuPosition {
    pub control: ControlId,
    pub position: i32,
}
impl SnapshotMenuPosition {
    pub fn new(control: ControlId, position: i32) -> Self {
        Self { control, position }
    }
}

/// Pure bounded lookup in the CURRENT audio-owned sparse roster. Full snapshot
/// preflight calls this for every menu before any value/callback writes. Scalar,
/// cell and text homes cannot write sparse keys; callback admission does not run
/// code. Until every value is committed, the roster therefore cannot mutate.
/// Strict overlays additionally retain the captured script/control revisions;
/// linearized overlays intentionally use all preceding FIFO edits.
pub(super) fn menu_value(
    generation: &Generation,
    row: &SnapshotMenuPosition,
) -> Result<ControlValue, Error> {
    let widget = &generation.prepared.widgets[generation
        .prepared
        .widgets
        .binary_search_by_key(&row.control, |w| w.id)
        .map_err(|_| Error::InvalidInput)?];
    if widget.storage != WidgetStorage::Control(row.control) {
        return Err(Error::InvalidInput);
    }
    let menu = widget.menu.ok_or(Error::InvalidInput)?;
    let store = &generation.scripts[usize::from(widget.instance.0)].store;
    let count = store
        .get(menu.count_key)
        .and_then(|n| i32::try_from(n).ok())
        .filter(|&n| n >= 0)
        .ok_or(Error::InvalidInput)?;
    if row.position < 0 || row.position >= count {
        return Err(Error::InvalidInput);
    }
    let mut key = menu.value_key;
    key[2] = row.position;
    let value = store
        .get(key)
        .filter(|&n| i32::try_from(n).is_ok())
        .ok_or(Error::InvalidInput)?;
    let value = ControlValue::Integer(value);
    let index = generation.prepared.control_index(row.control)?;
    if !generation.prepared.controls[index].domain.accepts(value) {
        return Err(Error::InvalidInput);
    }
    Ok(value)
}
