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
mod idiom;
mod inventory;
mod lexer;
mod parser;
mod runtime;
mod ui;
mod vm;

#[cfg(test)]
mod tests;

pub use engine::{
    ENGINE_PAR_BASE, EngineCall, EnginePar, EventId, Fade, GroupMask, KspEngine, LogEngine,
    NoteLength, NoteSpec, VoicePar, engine_par_name,
};
pub use inventory::requirements;
pub use runtime::{Live, MAX_SLOTS, Persisted, Refresh, Runtime, settle_persistence};

use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Value {
    Int(i32),
    Real(f64),
    Text(String),
    /// Dense snapshots keep numeric tables at their native element size.
    /// Untagged serialization preserves the existing JSON array format.
    IntArray(Vec<i32>),
    RealArray(Vec<f64>),
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

impl KeyState {
    pub(super) fn set_symbol(
        &mut self,
        color: bool,
        name: Option<&str>,
        number: i32,
        loading: bool,
    ) -> vm::Exec<()> {
        let (value, buffer) = if color {
            (&mut self.color, &mut self.color_buffer)
        } else {
            (&mut self.kind, &mut self.kind_buffer)
        };
        if let Some(name) = name {
            if let Some(Value::Text(text)) = value {
                return vm::put_text(text, name, loading);
            }
            vm::put_text(buffer, name, loading)?;
            *value = Some(Value::Text(std::mem::take(buffer)));
        } else {
            if let Some(Value::Text(text)) = value.take() {
                *buffer = text;
            }
            *value = Some(Value::Int(number));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Control {
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

/// Decode the persistent values Kontakt saves with a script: one entry per variable,
/// `"<name> <values>"`, with integers/reals space-separated and string arrays one
/// element per line.
pub fn saved_persistence(entries: &[String]) -> Persisted {
    entries
        .iter()
        .filter_map(|entry| {
            let (name, rest) = entry.split_once(' ').unwrap_or((entry, ""));
            let ints = || rest.split_whitespace().map(|x| x.parse().map(Value::Int));
            let reals = || rest.split_whitespace().map(|x| x.parse().map(Value::Real));
            let value = match name.as_bytes().first()? {
                b'$' => ints().next()?.ok()?,
                b'~' => reals().next()?.ok()?,
            b'%' => Value::IntArray(rest.split_whitespace().map(str::parse).collect::<Result<_, _>>().ok()?),
            b'?' => Value::RealArray(rest.split_whitespace().map(str::parse).collect::<Result<_, _>>().ok()?),
                b'@' => Value::Text(rest.to_owned()),
                b'!' => Value::Array(
                    rest.split('\n')
                        .map(|s| Value::Text(s.to_owned()))
                        .collect(),
                ),
                _ => return None,
            };
            Some((name.to_owned(), value))
        })
        .collect()
}

pub fn initialize(source: &str, groups: usize, outputs: usize) -> Result<Interface> {
    initialize_with_host(source, groups, outputs, &mut HostState::default())
}

/// Compile one script and run `on init`. Shared host state is committed only if
/// the whole initialization succeeds.
pub fn initialize_with_host(
    source: &str,
    groups: usize,
    outputs: usize,
    host: &mut HostState,
) -> Result<Interface> {
    let mut engine = LogEngine::new(vec![String::new(); groups], 48_000.0);
    let mut rt = Runtime::new(host.clone(), outputs, Vec::new());
    rt.load(&mut engine, source)?;
    let mut ui = rt.interface(0);
    ui.diagnostics.extend(rt.diagnostics());
    *host = rt.into_host();
    Ok(ui)
}

pub fn inspect(source: &str, groups: usize, host: &mut HostState) -> serde_json::Value {
    let requirements = requirements(source)
        .unwrap_or_else(|e| serde_json::json!({"inventory_error": format!("{e:#}")}));
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
