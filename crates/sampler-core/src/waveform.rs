//! Narrow, instance-store-backed waveform state. No sample mutation or painting.
//! Tags and property ordinals are owned addresses, never vendor constant values.
use crate::{Error, ops::Store};

pub const STATE_TAG: i32 = i32::MIN + 4;
pub const SOURCE_TAG: i32 = i32::MIN + 5;
pub const SYMBOL_TAG: i32 = i32::MIN + 6;
pub const TABLE_INDEX_LIMIT: i32 = 65536;

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Attach,
    Set,
    Get,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Property {
    Cursor = 1,
    Flags = 2,
    MidiStart = 3,
    Highlight = 4,
    Table = 5,
}
impl Property {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "$UI_WF_PROP_PLAY_CURSOR" => Self::Cursor,
            "$UI_WF_PROP_FLAGS" => Self::Flags,
            "$UI_WF_PROP_MIDI_DRAG_START_NOTE" => Self::MidiStart,
            "$UI_WF_PROP_TABLE_IDX_HIGHLIGHT" => Self::Highlight,
            "$UI_WF_PROP_TABLE_VAL" => Self::Table,
            _ => return None,
        })
    }
    fn from_address(value: i64) -> Option<Self> {
        Some(match value {
            1 => Self::Cursor,
            2 => Self::Flags,
            3 => Self::MidiStart,
            4 => Self::Highlight,
            5 => Self::Table,
            _ => return None,
        })
    }
    /// Scalar index is unused by NI; this owned service admits only canonical 0.
    /// Highlight -1 retains the old owned clear policy; toggle laws remain unknown.
    pub fn validate_index(self, index: i32) -> Result<(), Error> {
        let valid = match self {
            Self::Table => (0..TABLE_INDEX_LIMIT).contains(&index),
            Self::Highlight => (-1..TABLE_INDEX_LIMIT).contains(&index),
            _ => index == 0,
        };
        if valid { Ok(()) } else { Err(Error::InvalidInput) }
    }
    pub fn value(self, index: i32, value: i32) -> i32 {
        match self {
            Self::MidiStart => value.clamp(0, 127),
            Self::Highlight => index,
            _ => value,
        }
    }
    pub fn key(self, ui: i32, index: i32) -> [i32; 4] {
        [ui, self as i32, if self == Self::Table { index } else { 0 }, STATE_TAG]
    }
}

pub fn attachment_key(ui: i32) -> [i32; 4] { [ui, 0, 0, STATE_TAG] }
pub fn source_key(zone: i32) -> [i32; 4] { [zone, 0, 0, SOURCE_TAG] }
pub fn symbol_key(symbol: i32) -> [i32; 4] { [symbol, 0, 0, SYMBOL_TAG] }

/// Five existing keys make attachment/reset all-or-none, even at full capacity.
/// Unattached widgets cannot use property getters/setters.
pub fn initial(ui: i32) -> [([i32; 4], i64); 5] {
    [
        (attachment_key(ui), -1),
        (Property::Cursor.key(ui, 0), 0),
        (Property::Flags.key(ui, 0), 0),
        (Property::MidiStart.key(ui, 0), 60),
        (Property::Highlight.key(ui, 0), -1),
    ]
}

pub(crate) struct Admitted {
    pub args: [i32; 4],
    property: Option<Property>,
    action: Action,
}

/// Read-only admission: no partially changed state on any validation failure.
pub(crate) fn admit(store: &Store, action: Action, mut args: [i32; 4]) -> Result<Admitted, Error> {
    let ui = args[0];
    let zone = store.get(attachment_key(ui)).ok_or(Error::InvalidInput)?;
    for (key, _) in initial(ui) {
        if store.get(key).is_none() { return Err(Error::InvalidInput); }
    }
    let property = if action == Action::Attach {
        if args[1] <= 0 || store.get(source_key(args[1])) != Some(1) {
            return Err(Error::InvalidInput);
        }
        None
    } else {
        let zone = i32::try_from(zone).map_err(|_| Error::InvalidInput)?;
        if zone <= 0 || store.get(source_key(zone)) != Some(1) { return Err(Error::InvalidInput); }
        let p = store.get(symbol_key(args[1])).and_then(Property::from_address)
            .ok_or(Error::InvalidInput)?;
        p.validate_index(args[2])?;
        if action == Action::Set {
            args[3] = p.value(args[2], args[3]);
            if !store.can_set(p.key(ui, args[2])) { return Err(Error::Capacity); }
        }
        Some(p)
    };
    Ok(Admitted { args, property, action })
}

/// Called only after outbox admission. No allocation; no fallible step remains.
pub(crate) fn commit(store: &mut Store, admitted: &Admitted) -> i64 {
    let [ui, zone_or_symbol, index, value] = admitted.args;
    match admitted.action {
        Action::Attach => {
            store.clear_waveform_table(ui);
            for (key, default) in initial(ui) {
                let value = if key == attachment_key(ui) { i64::from(zone_or_symbol) }
                    else if key == Property::Flags.key(ui, 0) { i64::from(index) }
                    else { default };
                // Admission proved all five keys already exist.
                store.set(key, value);
            }
            0
        }
        Action::Set => {
            if let Some(p) = admitted.property { store.set(p.key(ui, index), i64::from(value)); }
            0
        }
        Action::Get => admitted.property.and_then(|p| store.get(p.key(ui, index))).unwrap_or(0),
    }
}
