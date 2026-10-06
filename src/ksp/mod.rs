//! KSP runtime: scripts compile once to flat, slot-resolved bytecode and run as
//! preallocated coroutines driven by MIDI input and sample-accurate time.
//!
//! Pipeline: `lexer` -> `parser` (per-callback ASTs) -> `compile` (typed bytecode)
//! -> `runtime` (event routing across up to five slots) -> `vm` + `calls`
//! (execution), talking to the sound engine only through [`KspEngine`].

mod builtins;
mod calls;
mod arrays;
pub use arrays::ArrayJob;
mod compile;
pub mod engine;
mod idiom;
mod inventory;
mod lexer;
mod parser;
mod performance_view;
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
pub use runtime::{FaultAction, FaultContext, Live, LiveFault, MAX_SLOTS, Runtime, settle_persistence};
/// Plain script-view data now owned by the core seam; re-exported for the runtime.
pub use crate::sound::view::{Control, Interface, KeyState, Persisted, Refresh, Value};
pub(crate) use runtime::{EVENT_CAPACITY, reset_controller_value};

use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;

/// Instrument-level services shared by all script slots (program global storage and
/// the keyboard display). Never process-global.
#[derive(Clone, Debug, Default, Serialize)]
pub struct HostState {
    pub(crate) pgs_ints: Vec<(String, Vec<i32>)>,
    pub(crate) pgs_strs: Vec<(String, String)>,
    pub keyboard: BTreeMap<u8, KeyState>,
    pub script_pressed: bool,
    /// `set_keyrange` entries (lowest key, highest key, name); they never overlap.
    pub keyranges: Vec<(u8, u8, String)>,
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

/// Decode the persistent values Kontakt saves with a script: one entry per variable,
/// `"<name> <values>"`, with integers/reals space-separated and string arrays one
/// element per line.
pub fn saved_persistence(entries: &[String]) -> Persisted {
    entries
        .iter()
        .filter_map(|entry| {
            let (name, rest) = entry.split_once(' ').unwrap_or((entry, ""));
            let reals = || rest.split_whitespace().map(|x| x.parse().map(Value::Real));
            let value = match name.as_bytes().first()? {
                b'$' => Value::NativeInt { native_int: rest.split_whitespace().next()?.parse().ok()? },
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
    // Undeclared uppercase names compile as opaque constants: right for
    // Kontakt's symbolic constants, silently wrong for a built-in it lacks.
    let constants = compile::compile(source, &compile::Setup { groups, outputs: 8, zones: 0 })
        .map(|p| p.auto_symbols.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        .unwrap_or_default();
    serde_json::json!({"requirements": requirements, "initialization": initialization, "opaque_constants": constants})
}

pub use compile::{UNSUPPORTED_FUNCTION, UNSUPPORTED_VARIABLE};
