//! Loading an instrument file into a plan the engine plays: the format reads
//! it into the IR, prepare loads its samples, its script is compiled.

use std::path::{Path, PathBuf};

use kontra_engine::{Plan, ir};
use kontra_prepare::prepare;
use kontra_stream::Store;

/// Wall time spent in the stages of building a plan.
#[derive(Clone, Copy, Debug, Default)]
pub struct BuildTimings {
    pub files: std::time::Duration,
    pub prepare: std::time::Duration,
    pub scripts: std::time::Duration,
    pub faces: std::time::Duration,
}

/// A plan ready for [`kontra_engine::Engine::new`], and what did not carry over.
pub struct Loaded {
    pub timings: BuildTimings,
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
    /// By script slot, a KSP script's catalog names as compiled: the
    /// engine-parameter id `SYMBOL_BASE + n` it passes is name n.
    pub symbols: Vec<Box<[Box<str>]>>,
}

/// Reads an SFZ file, a UVI program inside a `.ufs` bank, or a Kontakt instrument.
///
/// # Errors
/// When the file is not an instrument this can read.
pub fn read(path: &Path) -> Result<ir::Instrument, Box<dyn std::error::Error>> {
    read_saved(path, &[], &[])
}

/// Reads an ordinary instrument with separate format read and translation timings.
///
/// # Errors
/// As [`read`]. For other formats the translation time is unavailable.
pub fn read_timed(
    path: &Path,
) -> Result<
    (
        ir::Instrument,
        std::time::Duration,
        Option<std::time::Duration>,
    ),
    Box<dyn std::error::Error>,
> {
    let start = std::time::Instant::now();
    let (instrument, timings) = kontra_format::timing::collect(|| read(path));
    Ok((
        instrument?,
        timings.read.unwrap_or_else(|| start.elapsed()),
        timings.translate,
    ))
}

/// [`read`], with the scripts' `saved` values (by instrument script) in
/// place of the file's: a UVI program's load pass then loads the samples
/// a project's saved menus and state pick. A snapshot with no base near it
/// binds to one of `known` ([`preset`]).
///
/// # Errors
/// As [`read`].
pub fn read_saved(
    path: &Path,
    saved: &[Vec<String>],
    known: &[PathBuf],
) -> Result<ir::Instrument, Box<dyn std::error::Error>> {
    // A user preset: its base with its values, unless a project gave its own.
    if let Some(user) = crate::presets::read(path) {
        let saved = if saved.is_empty() {
            &user.scripts
        } else {
            saved
        };
        let mut out = read_saved(&user.base, saved, known)?;
        for (script, saved) in out.scripts.iter_mut().zip(saved) {
            script.saved.clone_from(saved);
        }
        if let Some(name) = path.file_stem() {
            out.name = name.to_string_lossy().into_owned();
        }
        return Ok(out);
    }
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("sfz"))
    {
        return Ok(kontra_format_sfz::load(path)?);
    }
    if let Some((multi, slot)) = multi_slot(path) {
        let (slots, failed) = kontra_format_kontakt::multi(multi, Some(slot))?;
        let mut out = (slots.into_iter().next())
            .ok_or_else(|| failed.join("; "))?
            .instrument;
        for (script, saved) in out.scripts.iter_mut().zip(saved) {
            script.saved.clone_from(saved);
        }
        out.source_path = Some(path.to_owned());
        return Ok(out);
    }
    if kontra_format_uvi::is_uvi(path) {
        // The load pass runs once per script, in order.
        let next = std::cell::Cell::new(0);
        let swaps = |script: &ir::Script| {
            let n = next.replace(next.get() + 1);
            script_swaps(script, saved.get(n).unwrap_or(&script.saved))
        };
        Ok(kontra_format_uvi::load_with(path, &swaps)?)
    } else {
        Ok(kontra_format_kontakt::load_known(path, known)?)
    }
}

/// Saved script values for an instrument held in another file, which
/// [`crate::rack::Rack::recall`] can apply to a part playing that file.
#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub name: String,
    /// The instrument file the values are for, absolute, as its IR's
    /// `source_path` names it.
    pub base: PathBuf,
    /// Saved values by script slot ([`ir::Script::slot`]); an empty slot
    /// means the base's own, which only a full load reads.
    pub saved: Vec<Vec<String>>,
    /// It saves the instrument's effects too: a recall rebuilds them.
    pub effects: bool,
}

/// A multi's slot as a path: `Multi.nkm/<slot>`, the slot counted from 0
/// (so a project saves and reopens a multi's part by path).
#[must_use]
pub fn multi_slot(path: &Path) -> Option<(&Path, u16)> {
    let multi = path
        .parent()
        .filter(|p| kontra_format_kontakt::is_multi(p))?;
    Some((multi, path.file_name()?.to_str()?.parse().ok()?))
}

/// Whether `path` is a preset of an instrument in another file: the
/// player's own, or a Kontakt snapshot.
#[must_use]
pub fn is_preset(path: &Path) -> bool {
    crate::presets::ours(path)
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nksn"))
}

/// The file at `path` as a [`Preset`] (a Kontakt snapshot), read without
/// its base instrument, which is near it or else the one of `known` (the
/// instruments the player has) it names; None for anything else, or a
/// snapshot this cannot read (its full load then reports why).
#[must_use]
pub fn preset(path: &Path, known: &[PathBuf]) -> Option<Preset> {
    let snapshot = (path.extension()).is_some_and(|e| e.eq_ignore_ascii_case("nksn"));
    let p = snapshot.then(|| kontra_format_kontakt::preset(path, known).ok())??;
    Some(Preset {
        name: p.name,
        base: p.base,
        saved: p.saved,
        effects: p.effects,
    })
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
    if let Some(progress) = store.progress() {
        progress.stage(kontra_protocol::LoadStage::Samples);
    }
    let start = std::time::Instant::now();
    let files = kontra_stream::Files::new(instrument.samples.iter().map(|s| s.file.as_path()));
    let files_time = start.elapsed();
    let start = std::time::Instant::now();
    let (mut plan, mut diagnostics) = prepare(&instrument, rate, store, &|s| files.open(s));
    let prepare_time = start.elapsed();
    if let Some(progress) = store.progress() {
        progress.stage(kontra_protocol::LoadStage::Scripts);
    }
    let scripts_start = std::time::Instant::now();
    let mut faces_time = std::time::Duration::ZERO;
    let mut faces = Vec::new();
    let mut pages = Vec::new();
    let mut persist = Vec::new();
    let mut symbols = Vec::new();
    diagnostics.extend(instrument.diagnostics);
    // ponytail: a slot that fails to compile is dropped, and the slots after
    // it move up one ($EVENT_PAR_SOURCE then counts from the gap).
    // Discovery walks the disk, so only a KSP script triggers it, and once.
    let mut factory_folder = None;
    // `SET_CONDITION` symbols pass on to the following slots (KSP manual,
    // "Preprocessor & System Scripts"), and Kontakt's own script runs after
    // the last: what is set when every slot has run is what it reads.
    let mut conditions: Vec<String> = Vec::new();
    for (n, script) in instrument.scripts.iter().enumerate() {
        if script.dialect == ir::Dialect::Lua {
            match kontra_lua::compile(&script.source, &lua_modules(script), &script.saved) {
                Ok(mut compiled) => {
                    persist.push(crate::persist::Vars::lua(n, &compiled.ui));
                    pages.push(Some(std::mem::take(&mut compiled.ui)));
                    // Controllers a format routes to this script's knobs, by name.
                    let slot = u8::try_from(plan.scripts.len()).unwrap_or(u8::MAX);
                    for k in instrument.knob_mods.iter().filter(|k| k.script == n) {
                        let found = kontra_engine::Foreign::controls(&compiled)
                            .iter()
                            .rposition(|c| *c == k.name);
                        if let Some(id) = found.and_then(|i| i32::try_from(i).ok()) {
                            plan.knob_mods.push(kontra_engine::KnobModPlan {
                                slot,
                                cc: k.cc,
                                id,
                                saved: k.saved,
                                ratio: k.ratio,
                            });
                        }
                    }
                    plan.scripts.push(crate::script::lua_plan(compiled));
                    symbols.push(Box::default());
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
        let set: Vec<&str> = conditions.iter().map(String::as_str).collect();
        let view = crate::perf_view::find(&script.data).map(|(_, v)| v);
        let view: Vec<(&str, &[i32])> = (view.iter().flat_map(|v| &v.controls))
            .map(|c| (c.name.as_str(), c.params.as_slice()))
            .collect();
        match kontra_ksp::compile_with_view(&script.source, &set, &view) {
            Ok(compiled) => {
                conditions.clone_from(&compiled.conditions);
                let mut planned = crate::script::plan(&compiled, &script.saved, &script.data);
                planned.source_folder =
                    crate::script::source_folder(instrument.source_path.as_deref());
                planned.library_folder = crate::script::folder(instrument.library_path.as_deref());
                planned.files = Some(std::sync::Arc::new(crate::rack::DiskFiles::default()));
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
                symbols.push(compiled.symbols.clone().into_boxed_slice());
                let start = std::time::Instant::now();
                faces.push(Some(crate::ui::Face::of(&compiled, &script.data)));
                faces_time += start.elapsed();
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
    // Kontakt's own script plays release triggers and holds notes under the
    // sustain pedal unless the slots leave that part switched off.
    let declares = |part: &str| conditions.iter().any(|c| c.eq_ignore_ascii_case(part));
    if declares("NO_SYS_SCRIPT_RLS_TRIG") {
        plan.release_zones = kontra_engine::ReleaseZones::WithNote;
    }
    if declares("NO_SYS_SCRIPT_PEDAL") {
        plan.sustain_pedal = kontra_engine::SustainPedal::ToScripts;
    }
    Loaded {
        timings: BuildTimings {
            files: files_time,
            prepare: prepare_time,
            scripts: scripts_start.elapsed().saturating_sub(faces_time),
            faces: faces_time,
        },
        name: instrument.name,
        plan,
        diagnostics,
        faces,
        pages,
        persist,
        symbols,
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
    /// A slot's `SET_CONDITION` reaches the slots after it and Kontakt's own
    /// script after the last; a later `RESET_CONDITION` takes it back.
    #[test]
    fn the_system_script_reads_the_conditions_every_slot_leaves() {
        use kontra_engine::{ReleaseZones, SustainPedal};
        let ksp = |slot: usize, source: &str| super::ir::Script {
            name: format!("slot {slot}"),
            slot,
            dialect: super::ir::Dialect::Ksp,
            source: source.into(),
            saved: Vec::new(),
            data: Vec::new(),
            links: Vec::new(),
        };
        let set = "on init\nSET_CONDITION(NO_SYS_SCRIPT_PEDAL)\nSET_CONDITION(NO_SYS_SCRIPT_RLS_TRIG)\nend on";
        let reset = "on init\nRESET_CONDITION(NO_SYS_SCRIPT_PEDAL)\nend on";
        let plan = |scripts| {
            let mut store = kontra_stream::Store::new(kontra_stream::StoreConfig::DEFAULT).unwrap();
            let instrument = super::ir::Instrument {
                scripts,
                ..super::ir::Instrument::DEFAULT
            };
            super::build(instrument, 48_000, &mut store).plan
        };
        let first = plan(vec![ksp(0, set), ksp(1, "on init\nend on")]);
        assert_eq!(first.sustain_pedal, SustainPedal::ToScripts);
        assert_eq!(first.release_zones, ReleaseZones::WithNote);
        let undone = plan(vec![ksp(0, set), ksp(1, reset)]);
        assert_eq!(undone.sustain_pedal, SustainPedal::Holds);
        assert_eq!(undone.release_zones, ReleaseZones::WithNote);
    }

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
        let project = super::read_saved(path, &saved, &[]).unwrap();
        let (b, m) = (names(&program, "Bartok"), names(&program, "Marcato"));
        let (b2, m2) = (names(&project, "Bartok"), names(&project, "Marcato"));
        println!("program: {b} Bartok, {m} Marcato; project: {b2} Bartok, {m2} Marcato");
        assert!(b > 0 && b2 < b && m2 > m, "{b}/{m} → {b2}/{m2}");
    }
}
