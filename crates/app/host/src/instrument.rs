//! Loading an instrument file into a plan the engine plays: the format reads
//! it into the IR, prepare loads its samples, its script is compiled.

use std::path::Path;

use kontra_engine::{Plan, ir};
use kontra_prepare::prepare;
use kontra_stream::Store;

/// A plan ready for [`kontra_engine::Engine::new`], and what did not carry over.
pub struct Loaded {
    pub name: String,
    pub plan: Plan,
    pub diagnostics: Vec<ir::Diagnostic>,
    /// By script slot, what a KSP script's UI tree is built from
    /// ([`crate::ui::interface`] once the engine ran `on init`).
    pub faces: Vec<Option<crate::ui::Face>>,
    /// By script slot, the page a Lua script built ([`crate::ui::lua`]).
    pub pages: Vec<Option<kontra_lua::Ui>>,
    /// By script slot, its persistent variables ([`crate::persist`]).
    pub persist: Vec<crate::persist::Vars>,
}

/// Reads the instrument at `path` into the model: a UVI program when the path
/// lies in a `.ufs` bank (`bank.ufs/Name.uvip`, see `kontra_format_uvi`), a
/// Kontakt instrument otherwise.
///
/// # Errors
/// When the file is not an instrument this can read.
pub fn read(path: &Path) -> Result<ir::Instrument, Box<dyn std::error::Error>> {
    if kontra_format_uvi::is_uvi(path) {
        Ok(kontra_format_uvi::load_with(path, &script_swaps)?)
    } else {
        Ok(kontra_format_kontakt::load(path)?)
    }
}

/// Reads the instrument at `path` for an engine at `rate`.
///
/// # Errors
/// When the file is not an instrument this can read.
pub fn load(
    path: &Path,
    rate: u32,
    store: &mut Store,
) -> Result<Loaded, Box<dyn std::error::Error>> {
    Ok(build(read(path)?, rate, store))
}

/// Builds the plan for an instrument already read into the IR.
#[must_use]
pub fn build(instrument: ir::Instrument, rate: u32, store: &mut Store) -> Loaded {
    let (mut plan, mut diagnostics) = prepare(&instrument, rate, store, &kontra_stream::open);
    let mut faces = Vec::new();
    let mut pages = Vec::new();
    let mut persist = Vec::new();
    diagnostics.extend(instrument.diagnostics);
    // ponytail: a slot that fails to compile is dropped, and the slots after
    // it move up one ($EVENT_PAR_SOURCE then counts from the gap).
    for (n, script) in instrument.scripts.iter().enumerate() {
        if script.dialect == ir::Dialect::Lua {
            match kontra_lua::compile(&script.source, &lua_modules(script), &script.saved) {
                Ok(mut compiled) => {
                    persist.push(crate::persist::Vars::lua(n, &compiled.ui));
                    pages.push(Some(std::mem::take(&mut compiled.ui)));
                    plan.scripts.push(crate::script::lua_plan(compiled));
                    faces.push(None);
                }
                Err(e) => diagnostics.push(ir::Diagnostic {
                    impact: ir::Impact::Broken,
                    feature: "script compile".into(),
                    detail: format!("{}: {e}", script.name),
                }),
            }
            continue;
        }
        match kontra_ksp::compile(&script.source, &[]) {
            Ok(compiled) => {
                plan.scripts
                    .push(crate::script::plan(&compiled, &script.saved, &script.data));
                faces.push(Some(crate::ui::Face::of(&compiled, &script.data)));
                pages.push(None);
                persist.push(crate::persist::Vars::of(n, &compiled));
            }
            Err(e) => diagnostics.push(ir::Diagnostic {
                impact: ir::Impact::Broken,
                feature: "script compile".into(),
                detail: format!(
                    "{}: {} at `{}`",
                    script.name,
                    e.message,
                    line_at(&script.source, e.span.start as usize).trim()
                ),
            }),
        }
    }
    Loaded {
        name: instrument.name,
        plan,
        diagnostics,
        faces,
        pages,
        persist,
    }
}

/// The samples a UVI script loads as its program loads, from one run of it.
fn script_swaps(script: &ir::Script) -> Vec<kontra_format_uvi::Swap> {
    kontra_lua::sample_loads(&script.source, &lua_modules(script), &script.saved)
        .into_iter()
        .map(|l| kontra_format_uvi::Swap {
            group: l.group,
            slot: l.slot,
            path: l.path,
            root: l.root,
        })
        .collect()
}

/// A Lua script's data files: the modules `require` finds.
fn lua_modules(script: &ir::Script) -> Vec<(String, String)> {
    script
        .data
        .iter()
        .map(|(name, bytes)| (name.clone(), String::from_utf8_lossy(bytes).into_owned()))
        .collect()
}

/// The source line holding byte `at`.
fn line_at(source: &str, at: usize) -> &str {
    let at = at.min(source.len());
    let start = source[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = source[at..].find('\n').map_or(source.len(), |i| at + i);
    source.get(start..end).unwrap_or_default()
}
