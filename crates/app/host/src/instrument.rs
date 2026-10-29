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
    read_saved(path, &[])
}

/// [`read`], with the scripts' `saved` values (by instrument script) in
/// place of the file's: a UVI program's load pass then loads the samples
/// a project's saved menus and state pick.
///
/// # Errors
/// As [`read`].
pub fn read_saved(
    path: &Path,
    saved: &[Vec<String>],
) -> Result<ir::Instrument, Box<dyn std::error::Error>> {
    if kontra_format_uvi::is_uvi(path) {
        // The load pass runs once per script, in order.
        let next = std::cell::Cell::new(0);
        let swaps = |script: &ir::Script| {
            let n = next.replace(next.get() + 1);
            script_swaps(script, saved.get(n).unwrap_or(&script.saved))
        };
        Ok(kontra_format_uvi::load_with(path, &swaps)?)
    } else {
        Ok(kontra_format_kontakt::load(path)?)
    }
}

/// The file at `path` as saved values for another instrument (a Kontakt
/// snapshot), read without that instrument; None for anything else, or a
/// snapshot this cannot read (its full load then reports why).
#[must_use]
pub fn preset(path: &Path) -> Option<kontra_format_kontakt::Preset> {
    let snapshot = (path.extension()).is_some_and(|e| e.eq_ignore_ascii_case("nksn"));
    snapshot.then(|| kontra_format_kontakt::preset(path).ok())?
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
    // Each file once, however many samples it holds.
    let files = kontra_stream::Files::new(instrument.samples.iter().map(|s| s.file.as_path()));
    let (mut plan, mut diagnostics) = prepare(&instrument, rate, store, &|s| files.open(s));
    let mut faces = Vec::new();
    let mut pages = Vec::new();
    let mut persist = Vec::new();
    diagnostics.extend(instrument.diagnostics);
    // ponytail: a slot that fails to compile is dropped, and the slots after
    // it move up one ($EVENT_PAR_SOURCE then counts from the gap).
    // Discovery walks the disk, so only a KSP script triggers it, and once.
    let mut factory_folder = None;
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
                let mut planned = crate::script::plan(&compiled, &script.saved, &script.data);
                planned.source_folder =
                    crate::script::source_folder(instrument.source_path.as_deref());
                planned.library_folder = crate::script::folder(instrument.library_path.as_deref());
                planned.factory_folder = *factory_folder.get_or_insert_with(|| {
                    factory_folder_of(kontra_library::kontakt::candidates())
                });
                planned.links = crate::script::links(&compiled, &script.links);
                if planned.links.len() < script.links.len() {
                    diagnostics.push(ir::Diagnostic {
                        impact: ir::Impact::Audible,
                        feature: "MIDI automation of a missing script control".into(),
                        detail: script.name.clone(),
                    });
                }
                plan.scripts.push(planned);
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
fn script_swaps(script: &ir::Script, saved: &[String]) -> Vec<kontra_format_uvi::Swap> {
    kontra_lua::sample_loads(&script.source, &lua_modules(script), saved)
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

/// The folder holding the first installed executable: Kontakt's factory data
/// folder, as `get_folder($GET_FOLDER_FACTORY_DIR)` reports it.
fn factory_folder_of(candidates: Vec<std::path::PathBuf>) -> Option<kontra_engine::Text> {
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .and_then(|path| crate::script::folder(path.parent()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn factory_folder_is_the_first_installed_executables_folder() {
        let dir = std::env::temp_dir().join(format!("kontra-factory-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("Kontakt 8.exe");
        std::fs::write(&exe, b"").unwrap();
        let missing = dir.join("missing/Kontakt 7.exe");
        let found = super::factory_folder_of(vec![missing.clone(), exe]).unwrap();
        assert_eq!(
            found.as_str(),
            format!("{}/", dir.display()).replace('\\', "/")
        );
        assert!(super::factory_folder_of(vec![missing]).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    use std::path::Path;

    /// Augmented Orchestra's saved state picks each part's layers
    /// (`loadedLayer`): a project's state, not the program's, picks the
    /// samples the load pass puts on the oscillators.
    #[test]
    fn a_projects_saved_state_picks_the_samples_a_program_loads() {
        let path = Path::new(
            "/data/main/Libraries/UVI/UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs/Presets/00 Orchestra/01 Strings/V Strings Bartok.uvip",
        );
        if !path.ancestors().any(Path::is_file) {
            eprintln!("skipped: no {}", path.display());
            return;
        }
        // Zones playing a sample of that articulation.
        let names = |i: &kontra_engine::ir::Instrument, of: &str| {
            let zones = i.groups.iter().flat_map(|g| &g.zones);
            zones
                .filter(|z| i.samples.get(z.sample).is_some_and(|s| s.name.contains(of)))
                .count()
        };
        let program = super::read(path).unwrap();
        let mut saved: Vec<Vec<String>> = program.scripts.iter().map(|s| s.saved.clone()).collect();
        let state = r#"__state={{{["loadedLayer"]="Strings Ensemble/Strings Ensemble Marcato M"},{["loadedLayer"]="Strings Ensemble/Strings Ensemble Marcato ST"}},{{["loadedLayer"]="Strings Ensemble/Strings Ensemble Marcato M"},{["loadedLayer"]="Strings Ensemble/Strings Ensemble Marcato ST"}}}"#;
        saved[0].push(state.to_owned());
        let project = super::read_saved(path, &saved).unwrap();
        let (b, m) = (names(&program, "Bartok"), names(&program, "Marcato"));
        let (b2, m2) = (names(&project, "Bartok"), names(&project, "Marcato"));
        println!("program: {b} Bartok, {m} Marcato; project: {b2} Bartok, {m2} Marcato");
        assert!(b > 0 && b2 < b && m2 > m, "{b}/{m} → {b2}/{m2}");
    }
}
