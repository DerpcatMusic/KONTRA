//! `kontra-inspect load`: not written yet (lane F stopped here; lane G continues).
//! Plan (BOARD, lane F design 2026-10-29): read the IR (`kontra_host::instrument::read`),
//! describe samples/groups/routes+shapers/effects/scripts/diagnostics, then
//! `instrument::build` + `Engine::new` and report live mods, unanswered init actions,
//! script faults and purged groups, as the render example does with its env knobs.

use std::path::Path;

pub fn run(_path: &Path) -> anyhow::Result<(serde_json::Value, String)> {
    anyhow::bail!("load: not implemented yet")
}
