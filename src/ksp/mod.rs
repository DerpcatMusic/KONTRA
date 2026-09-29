//! KSP runtime: scripts compile once to flat, slot-resolved bytecode and run as
//! preallocated coroutines driven by MIDI input and sample-accurate time.
//!
//! Pipeline: `lexer` -> `parser` (per-callback ASTs) -> `compile` (typed bytecode)
//! -> `runtime` (event routing across up to five slots) -> `vm` + `calls`
//! (execution), talking to the sound engine only through [`KspEngine`].

mod builtins;
mod calls;
mod compile;
pub mod engine;
mod inventory;
mod lexer;
mod parser;
mod runtime;
mod ui;
mod vm;

#[cfg(test)]
mod tests;

pub use engine::{EngineCall, EnginePar, Fade, GroupMask, KspEngine, LogEngine, NoteLength, NoteSpec, VoiceId, VoicePar};
pub use inventory::requirements;
pub use runtime::{MAX_SLOTS, Persisted, Runtime};

use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Value {
    Int(i32),
    Real(f64),
    Text(String),
    Array(Vec<Value>),
}

/// Instrument-level services shared by all script slots (program global storage and
/// the keyboard display). Never process-global.
#[derive(Clone, Debug, Default, Serialize)]
pub struct HostState {
    pub(crate) pgs_ints: Vec<(String, Vec<i32>)>,
    pub(crate) pgs_strs: Vec<(String, String)>,
    pub keyboard: BTreeMap<u8, KeyState>,
    pub script_pressed: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct KeyState {
    pub name: String,
    pub color: Option<Value>,
    pub kind: Option<Value>,
    pub pressed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Control {
    pub variable: String,
    pub kind: String,
    pub properties: BTreeMap<String, Value>,
    pub menu: Vec<(String, i32)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Interface {
    pub performance: bool,
    pub width: i32,
    pub height: i32,
    pub title: String,
    pub wallpaper: String,
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
            controls: Vec::new(),
            diagnostics: BTreeSet::new(),
            listeners: BTreeMap::new(),
        }
    }
}

pub fn initialize(source: &str, groups: usize, outputs: usize) -> Result<Interface> {
    initialize_with_host(source, groups, outputs, &mut HostState::default())
}

/// Compile one script and run `on init`. Shared host state is committed only if
/// the whole initialization succeeds.
pub fn initialize_with_host(source: &str, groups: usize, outputs: usize, host: &mut HostState) -> Result<Interface> {
    let mut engine = LogEngine::new(vec![String::new(); groups], 48_000.0);
    let mut rt = Runtime::new(host.clone(), outputs, Vec::new());
    rt.load(&mut engine, source)?;
    let mut ui = rt.interface(0);
    ui.diagnostics.extend(rt.diagnostics());
    *host = rt.into_host();
    Ok(ui)
}

pub fn inspect(source: &str, groups: usize, host: &mut HostState) -> serde_json::Value {
    let requirements = requirements(source).unwrap_or_else(|e| serde_json::json!({"inventory_error": format!("{e:#}")}));
    let initialization = match initialize_with_host(source, groups, 8, host) {
        Ok(ui) => serde_json::json!({
            "controls": ui.controls.len(),
            "listeners": ui.listeners,
            "diagnostics": ui.diagnostics,
        }),
        Err(e) => serde_json::json!({"error": format!("{e:#}")}),
    };
    serde_json::json!({"requirements": requirements, "initialization": initialization})
}
