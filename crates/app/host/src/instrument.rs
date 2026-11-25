//! Loading an instrument file into a plan the engine plays: the format reads
//! it into the IR, prepare loads its samples, its script is compiled.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use kontra_engine::{Plan, ir};
use kontra_format::{EntryRef, FormatError, FormatId, LoadCx, LoadOptions, Notes, Registry};
use kontra_prepare::prepare;
use kontra_stream::Store;

use crate::Error;

/// Returns the process-wide [`Registry`] containing all built-in sampler formats.
#[must_use]
pub fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut reg = Registry::new();
        reg.register(kontra_format_native::Native)
            .register(kontra_format_sfz::Sfz)
            .register(kontra_format_decentsampler::DecentSampler)
            .register(kontra_format_sf2::Sf2)
            .register(kontra_format_uvi::Uvi)
            .register(kontra_format_kontakt::Kontakt);
        reg
    })
}

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

/// Reads an SFZ file, a SoundFont's first preset, a UVI program inside a `.ufs` bank, or a Kontakt instrument.
///
/// # Errors
/// When the file is not an instrument this can read.
pub fn read(path: &Path) -> Result<ir::Instrument, Error> {
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
    Error,
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
) -> Result<ir::Instrument, Error> {
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
    if let Some((multi, slot)) = multi_slot(path) {
        let loaded = kontra_format_kontakt::multi_load(
            multi,
            Some(slot),
            &kontra_library::kontakt::candidates(),
        )?;
        let mut out = (loaded.slots.into_iter().next())
            .ok_or_else(|| Error::Multi(loaded.failed.join("; ")))?
            .instrument;
        for (script, saved) in out.scripts.iter_mut().zip(saved) {
            script.saved.clone_from(saved);
        }
        out.source_path = Some(path.to_owned());
        return Ok(out);
    }
    let reg = registry();
    let matched = reg.sniff_file(path).map_err(|e| FormatError::io(path, e))?;
    match matched {
        Some((fmt, _)) if fmt.id() == FormatId::Uvi => {
            // The load pass runs once per script, in order.
            let next = std::cell::Cell::new(0);
            let swaps = |script: &ir::Script| {
                let n = next.replace(next.get() + 1);
                script_swaps(script, saved.get(n).unwrap_or(&script.saved))
            };
            Ok(kontra_format_uvi::load_with(path, &swaps)?)
        }
        Some((fmt, _)) => {
            let mut notes = Notes::default();
            let options = LoadOptions {
                known_instruments: known.to_vec(),
            };
            let mut cx = LoadCx::new(&mut notes, &options);
            Ok(fmt.load(&EntryRef::new(path), &mut cx)?)
        }
        None => {
            Err(FormatError::unsupported(path, "no registered format recognizes this file").into())
        }
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

/// A SoundFont preset as a virtual path: `Bank.sf2/000:001 Name.sf2p` (or
/// `Bank.sf2/0:1`), so the browser lists every preset in a bank and projects
/// reopen the chosen preset by path.
#[must_use]
pub fn sf2_slot(path: &Path) -> Option<(&Path, u16, u16)> {
    kontra_format_sf2::parse_slot(path)
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
pub fn load(path: &Path, rate: u32, store: &mut Store) -> Result<Loaded, Error> {
    Ok(build(read(path)?, rate, store))
}

/// Scripts compiled by every build of the process.
static COMPILES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many scripts [`build`] has compiled so far.
#[must_use]
pub fn script_compiles() -> u64 {
    COMPILES.load(std::sync::atomic::Ordering::Relaxed)
}

fn compiled() {
    COMPILES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// What a KSP compile reads: the source, the conditions set before it and
/// its performance view's controls.
type KspKey = (
    String,
    Vec<String>,
    Vec<(String, kontra_ksp::ControlKind, Vec<i32>)>,
);

/// [`kontra_ksp::compile_with_view`], from the scripts compiled last when
/// one had the same inputs: the instruments of a library mostly share one
/// script, and a swap between them compiles nothing.
// ponytail: the last 8 by exact inputs, scanned in a line; a map when a
// rack holds more distinct scripts than that.
fn compile_ksp(
    source: &str,
    set: &[&str],
    view: &[(&str, kontra_ksp::ControlKind, &[i32])],
) -> Result<std::sync::Arc<kontra_ksp::Script>, kontra_ksp::Error> {
    use std::sync::{Arc, Mutex, PoisonError};
    static RECENT: Mutex<Vec<(KspKey, Arc<kontra_ksp::Script>)>> = Mutex::new(Vec::new());
    let same = |(s, c, v): &KspKey| {
        s == source
            && c.iter().map(String::as_str).eq(set.iter().copied())
            && v.iter()
                .map(|(n, k, p)| (n.as_str(), *k, p.as_slice()))
                .eq(view.iter().copied())
    };
    let recent = || RECENT.lock().unwrap_or_else(PoisonError::into_inner);
    {
        let mut recent = recent();
        if let Some(at) = recent.iter().position(|(key, _)| same(key)) {
            let hit = recent.remove(at);
            let script = Arc::clone(&hit.1);
            recent.push(hit);
            return Ok(script);
        }
    }
    compiled();
    let script = Arc::new(kontra_ksp::compile_with_view(source, set, view)?);
    let key = (
        source.to_owned(),
        set.iter().map(|&c| c.to_owned()).collect(),
        view.iter()
            .map(|&(n, k, p)| (n.to_owned(), k, p.to_vec()))
            .collect(),
    );
    let mut recent = recent();
    recent.push((key, Arc::clone(&script)));
    if recent.len() > 8 {
        recent.remove(0);
    }
    Ok(script)
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
    plan.knob_blends = instrument.knob_blends.clone();
    plan.knob_lfos = instrument.knob_lfos.clone();
    // ponytail: a slot that fails to compile is dropped, and the slots after
    // it move up one ($EVENT_PAR_SOURCE then counts from the gap).
    // Discovery walks the disk, so only a KSP script triggers it, and once.
    let mut factory_folder = None;
    // `SET_CONDITION` symbols pass on to the following slots (KSP manual,
    // "Preprocessor & System Scripts"), and Kontakt's own script runs after
    // the last: what is set when every slot has run is what it reads.
    let mut conditions: Vec<String> = Vec::new();
    let mut origin: Option<PathBuf> = None;
    for (n, script) in instrument.scripts.iter().enumerate() {
        if script.dialect == ir::Dialect::Lua {
            compiled();
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
                                id,
                                source: k.source,
                                scale: k.scale,
                            });
                        } else {
                            diagnostics.push(ir::Diagnostic {
                                impact: ir::Impact::Audible,
                                feature: "script knob route to a missing control".into(),
                                detail: format!("{}: {}", script.name, k.name),
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
        // A multi's folded player keeps its own instrument's conditions:
        // they reach its own slots, never the sounding instrument's (whose
        // last slot decides what Kontakt's own script does).
        // ponytail: told apart by the container folder, which only a tool
        // player carries; two folded players without one still share.
        if script.container != origin {
            conditions.clear();
            origin.clone_from(&script.container);
        }
        let set: Vec<&str> = conditions.iter().map(String::as_str).collect();
        let view = crate::perf_view::find(&script.data).map(|(_, v)| v);
        let view: Vec<(&str, kontra_ksp::ControlKind, &[i32])> =
            (view.iter().flat_map(|v| &v.controls))
                .map(|c| (c.name.as_str(), c.kind, c.params.as_slice()))
                .collect();
        match compile_ksp(&script.source, &set, &view) {
            Ok(compiled) => {
                conditions.clone_from(&compiled.conditions);
                let mut planned = crate::script::plan(&compiled, &script.saved, &script.data);
                planned.source_folder =
                    crate::script::source_folder(instrument.source_path.as_deref());
                planned.library_folder = crate::script::folder(instrument.library_path.as_deref());
                // The resource container's folder: the one the script came
                // with, else looked up only for a script that names it (one
                // passing a variable, as Kontakt's Chords tool does, needs
                // its translator to carry it).
                if let Some(own) = &script.container {
                    planned.rc_folder = crate::script::folder(Some(own));
                } else if script.source.contains("GET_FOLDER_RC_DIR") {
                    planned.rc_folder = instrument.source_path.as_deref().and_then(|p| {
                        let found = kontra_format_kontakt::Resources::open(p).container_folder();
                        crate::script::folder(found.as_deref())
                    });
                }
                planned.files = Some(std::sync::Arc::new(crate::rack::DiskFiles::default()));
                planned.factory_folder = *factory_folder.get_or_insert_with(|| {
                    factory_folder_of(kontra_library::kontakt::candidates())
                });
                planned.links = crate::script::links(&compiled, &script.links);
                // A link to a control the script never declares moves nothing
                // in Kontakt either: one left from an older script. One to a
                // declared control that we could not range is ours to fix.
                let absent = (script.links.iter())
                    .filter(|l| crate::script::linked(&compiled, l).is_none())
                    .count();
                let unranged = script.links.len() - planned.links.len() - absent;
                for (n, impact, feature) in [
                    (
                        absent,
                        ir::Impact::Cosmetic,
                        "MIDI automation of a missing script control",
                    ),
                    (
                        unranged,
                        ir::Impact::Audible,
                        "MIDI automation of an unranged script control",
                    ),
                ] {
                    if n > 0 {
                        diagnostics.push(ir::Diagnostic {
                            impact,
                            feature: feature.into(),
                            detail: script.name.clone(),
                        });
                    }
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
    /// A native file quoting an SFZ tag stays native with SFZ registered.
    #[test]
    fn a_quoted_sfz_tag_does_not_take_a_native_file() {
        let (fmt, _) = super::registry()
            .sniff(
                std::path::Path::new("Piano.kontra"),
                b"name = \"<region>\"\nversion = 1\n",
            )
            .expect("native recognizes its extension");
        assert_eq!(fmt.id(), kontra_format::FormatId::Native);
    }

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
            container: None,
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
        // A folded multi player (its own container folder) keeps its
        // conditions: the synth after it holds the pedal as before.
        let mut player = ksp(0, set);
        player.container = Some("Tools/Chords/Samples".into());
        let folded = plan(vec![player, ksp(1, "on init\nend on")]);
        assert_eq!(folded.sustain_pedal, SustainPedal::Holds);
        assert_eq!(folded.release_zones, ReleaseZones::AtRelease);
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
