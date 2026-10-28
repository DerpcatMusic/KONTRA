//! Loading an instrument file into a plan the engine plays: the format reads
//! it into the IR, prepare loads its samples, its script is compiled.

use std::path::Path;

use kontra_engine::{Plan, ir, prepare};
use kontra_stream::Store;

/// A plan ready for [`kontra_engine::Engine::new`], and what did not carry over.
pub struct Loaded {
    pub name: String,
    pub plan: Plan,
    pub diagnostics: Vec<ir::Diagnostic>,
}

/// Reads the instrument at `path` for an engine at `rate`.
///
/// # Errors
/// When the file is not an instrument this can read.
pub fn load(
    path: &Path,
    rate: u32,
    store: &mut Store,
) -> Result<Loaded, kontra_format_kontakt::Error> {
    Ok(build(kontra_format_kontakt::load(path)?, rate, store))
}

/// Builds the plan for an instrument already read into the IR.
#[must_use]
pub fn build(instrument: ir::Instrument, rate: u32, store: &mut Store) -> Loaded {
    let (mut plan, mut diagnostics) = prepare(&instrument, rate, store, &kontra_stream::open);
    diagnostics.extend(instrument.diagnostics);
    // ponytail: a slot that fails to compile is dropped, and the slots after
    // it move up one ($EVENT_PAR_SOURCE then counts from the gap).
    for script in instrument
        .scripts
        .iter()
        .filter(|s| s.dialect == ir::Dialect::Ksp)
    {
        match kontra_ksp::compile(&script.source, &[]) {
            Ok(compiled) => {
                plan.scripts
                    .push(crate::script::plan(&compiled, &script.saved, &script.data))
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
    }
}

/// The source line holding byte `at`.
fn line_at(source: &str, at: usize) -> &str {
    let at = at.min(source.len());
    let start = source[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = source[at..].find('\n').map_or(source.len(), |i| at + i);
    source.get(start..end).unwrap_or_default()
}
