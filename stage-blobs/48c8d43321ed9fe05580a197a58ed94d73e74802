//! Owned, plain-data views of an instrument's script interface.
//!
//! These types cross the core seam: the UI and host state read them, and any
//! core implementation fills them. They carry no runtime handles.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Value {
    Int(i32),
    /// Native Kontakt persistence stores a menu's entry position, whereas
    /// KONTRA host state stores its assigned value. Keep the origin until the
    /// declared control and its entries are available; ordinary scalars use it
    /// as an integer. The object shape survives the imported instrument cache.
    NativeInt { native_int: i32 },
    Real(f64),
    Text(String),
    /// Dense snapshots keep numeric tables at their native element size.
    /// Untagged serialization preserves the existing JSON array format.
    IntArray(Vec<i32>),
    RealArray(Vec<f64>),
    Array(Vec<Value>),
}

/// Saved values of persistent variables, per script slot, keyed by variable name.
pub type Persisted = BTreeMap<String, Value>;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct KeyState {
    pub name: String,
    pub color: Option<Value>,
    pub kind: Option<Value>,
    pub pressed: bool,
    #[serde(skip)]
    pub color_buffer: String,
    #[serde(skip)]
    pub kind_buffer: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Control {
    /// Its UI ID (`get_ui_id`): what `$CONTROL_PAR_PARENT_PANEL` names.
    pub id: i32,
    pub variable: String,
    pub kind: String,
    pub properties: BTreeMap<String, Value>,
    pub menu: Vec<(String, i32)>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Interface {
    pub performance: bool,
    pub width: i32,
    pub height: i32,
    pub title: String,
    pub wallpaper: String,
    #[serde(default)]
    pub wallpaper_state: i32,
    /// Authored performance-view background, packed as 0xRRGGBB.
    #[serde(default)]
    pub background_color: Option<u32>,
    /// Vertical background offset in pixels, independent of picture state.
    #[serde(default)]
    pub skin_offset: i32,
    /// Init-only named bitmap fonts; IDs are 26 + their slot-local index.
    #[serde(default)]
    pub fonts: Vec<String>,
    pub controls: Vec<Control>,
    pub diagnostics: BTreeSet<String>,
    pub listeners: BTreeMap<String, i32>,
}

impl Default for Interface {
    fn default() -> Self {
        Self {
            performance: false,
            width: 632,
            height: 350,
            title: String::new(),
            wallpaper: String::new(),
            wallpaper_state: 0,
            background_color: None,
            skin_offset: 0,
            fonts: Vec::new(),
            controls: Vec::new(),
            diagnostics: BTreeSet::new(),
            listeners: BTreeMap::new(),
        }
    }
}


/// Where an incremental refresh stands; start from the default.
#[derive(Clone, Copy, Debug, Default)]
pub struct Refresh {
    pub(crate) value_revision: u64,
    pub(crate) slot: usize,
    pub(crate) item: usize,
    pub(crate) at: usize,
    /// Something differed from the buffer so far.
    pub changed: bool,
}
