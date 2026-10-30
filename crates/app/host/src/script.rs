//! A compiled KSP script as the engine's [`ScriptPlan`].

use kontra_engine::{Callbacks, Controls, EnginePar, EnginePars, LookPars, ScriptPlan, Text, ir};
use kontra_ksp::ControlKind;
use kontra_ksp::{CallbackKind, Cells, FIRST_UI_ID, SYMBOL_BASE, Script};

/// Whether the engine or the UI tree understands catalog name `name`
/// (every other one is an opaque symbol nothing acts on), for coverage
/// reports.
#[must_use]
pub fn understood(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper == CONTROL_VALUE
        || engine_par(&kontra_format_kontakt::engine_pars(), name).is_some()
        || crate::ui::mapped(&upper)
        || LOOK.contains(&upper.as_str())
        || ["$CONTROL_PAR_NUM_ITEMS", "$CONTROL_PAR_SELECTED_ITEM_IDX"].contains(&upper.as_str())
}

/// What `name` drives, and its text law: a knob row is
/// [`EnginePar::Knob`] by its place among the table's knob rows.
fn engine_par<'a>(
    pars: &'a [ir::ScriptPar],
    name: &str,
) -> Option<(EnginePar, Option<&'a ir::Readout>)> {
    let mut knobs = 0u16;
    for p in pars {
        let par = match &p.drives {
            ir::Drives::Engine(par) => *par,
            ir::Drives::Knob(..) => {
                knobs = knobs.checked_add(1)?;
                EnginePar::Knob(knobs - 1)
            }
        };
        if p.name.eq_ignore_ascii_case(name) {
            return Some((par, p.readout.as_ref()));
        }
    }
    None
}

const CONTROL_VALUE: &str = "$CONTROL_PAR_VALUE";

/// Callbacks a KSP instrument may have running or waiting at once.
const COROUTINES: usize = 64;

/// Whether the engine runs callbacks of `kind` (the ones [`plan`] wires),
/// for coverage reports.
#[must_use]
pub fn runs(kind: CallbackKind) -> bool {
    !matches!(
        kind,
        CallbackKind::PolyAt | CallbackKind::UiUpdate | CallbackKind::Rpn | CallbackKind::Nrpn
    )
}

/// The script's arrays, by first cell, for the array files it names.
fn arrays(script: &Script) -> Vec<(u32, bool, Box<str>)> {
    let array = |v: &kontra_ksp::Variable| match v.cells {
        Cells::Int { base, .. } | Cells::Real { base, .. } => Some((base, false)),
        Cells::Text { base, .. } => Some((base, true)),
        _ => None,
    };
    (script.variables.iter())
        .filter(|v| v.name.starts_with(['%', '?', '!']))
        .filter_map(|v| array(v).map(|(base, text)| (base, text, v.name.clone())))
        .collect()
}

/// The engine's view of `script` with `saved` persistent values restored and
/// the library `data` files it loads. UI metadata stays with the caller.
#[must_use]
pub fn plan(script: &Script, saved: &[String], data: &[(String, Vec<u8>)]) -> ScriptPlan {
    let entry = |kind| script.callback(kind, None);
    let restore = script.restore(saved);
    let pars = kontra_format_kontakt::engine_pars();
    let found: Vec<_> = script
        .symbols
        .iter()
        .map(|n| engine_par(&pars, n))
        .collect();
    ScriptPlan {
        program: script.program.clone(),
        callbacks: Callbacks {
            init: entry(CallbackKind::Init),
            note: entry(CallbackKind::Note),
            release: entry(CallbackKind::Release),
            controller: entry(CallbackKind::Controller),
            persistence_changed: entry(CallbackKind::PersistenceChanged),
            listener: entry(CallbackKind::Listener),
            pgs_changed: entry(CallbackKind::PgsChanged),
            ui_controls: entry(CallbackKind::UiControls),
            async_complete: entry(CallbackKind::AsyncComplete),
            ui_control: (0u32..)
                .zip(&script.controls)
                .map(|(n, c)| {
                    let id = FIRST_UI_ID + c.variable as i32;
                    (id, script.callback(CallbackKind::UiControl, Some(n)))
                })
                .collect(),
            widget: None,
        },
        coroutines: COROUTINES,
        foreign: None,
        files: None,
        arrays: arrays(script),
        hears_own_releases: true,
        held_release_ends_children: true,
        links: Vec::new(),
        controls: controls(script),
        engine_pars: EnginePars {
            first: SYMBOL_BASE,
            by_id: found.iter().map(|f| f.map(|(par, _)| par)).collect(),
            readouts: found
                .iter()
                .map(|f| f.and_then(|(_, r)| r.cloned()))
                .collect(),
        },
        reverb_laws: kontra_format_kontakt::reverb_laws(),
        reverb_networks: kontra_format_kontakt::reverb_networks().into(),
        knobs: pars
            .iter()
            .filter_map(|p| match &p.drives {
                ir::Drives::Knob(target, law) => Some((*target, law.clone())),
                ir::Drives::Engine(_) => None,
            })
            .collect(),
        data: data
            .iter()
            .filter_map(|(name, bytes)| data_array(script, name, bytes))
            .collect(),
        source_folder: None,
        library_folder: None,
        factory_folder: None,
        text_data: data
            .iter()
            .filter_map(|(name, bytes)| text_data_array(script, name, bytes))
            .collect(),
        restore: restore.ints,
        restore_texts: restore
            .texts
            .iter()
            .map(|(cell, text)| (*cell, Text::from_str_cut(text).0))
            .collect(),
    }
}

/// The folder of a source file, prepared for script access.
pub(crate) fn source_folder(path: Option<&std::path::Path>) -> Option<Text> {
    let path = std::path::absolute(path?).ok()?;
    folder(path.parent())
}

/// A folder with a trailing slash, when it fits a text cell.
pub(crate) fn folder(path: Option<&std::path::Path>) -> Option<Text> {
    let path = std::path::absolute(path?).ok()?;
    let folder = path.to_str()?;
    let folder = if std::path::MAIN_SEPARATOR == '\\' {
        folder.replace('\\', "/")
    } else {
        folder.to_owned()
    };
    let path = if folder.ends_with('/') {
        folder
    } else {
        format!("{folder}/")
    };
    let (text, cut) = Text::from_str_cut(&path);
    (!cut).then_some(text)
}

/// `links` as the engine applies them: each on its control's UI id and
/// declared range (a switch's is 0..=1); a link to a control the script
/// lacks is dropped.
#[must_use]
pub fn links(script: &Script, links: &[kontra_engine::ir::CcLink]) -> Vec<kontra_engine::CcLink> {
    use kontra_engine::ir::Linked;
    links
        .iter()
        .filter_map(|l| {
            let (kind, n) = match l.control {
                Linked::Knob(n) => (ControlKind::Knob, n),
                Linked::Slider(n) => (ControlKind::Slider, n),
                Linked::Switch(n) => (ControlKind::Switch, n),
            };
            let c = script
                .controls
                .iter()
                .filter(|c| c.kind == kind)
                .nth(usize::from(n))?;
            let (lo, hi) = match (kind, c.params.as_slice()) {
                (ControlKind::Switch, _) => (0, 1),
                (_, [lo, hi, ..]) => (i64::from(*lo), i64::from(*hi)),
                _ => return None,
            };
            Some(kontra_engine::CcLink {
                cc: l.cc,
                id: FIRST_UI_ID + i32::try_from(c.variable).ok()?,
                lo,
                hi,
                from: l.from,
                to: l.to,
            })
        })
        .collect()
}

/// A data file (`.nka`: a name line, then one value per line) as the cells of
/// the number array it names. A short file fills a prefix; extra lines are
/// ignored, as in Kontakt.
fn data_array(script: &Script, name: &str, bytes: &[u8]) -> Option<(u32, Vec<i64>)> {
    let var = script.variables.iter().find(|v| {
        v.name
            .get(1..)
            .is_some_and(|n| n.eq_ignore_ascii_case(name))
    })?;
    let (base, len, real) = match var.cells {
        Cells::Int { base, len } => (base, len, false),
        Cells::Real { base, len } => (base, len, true),
        _ => return None,
    };
    let text = String::from_utf8_lossy(bytes);
    let values = text
        .lines()
        .skip(1)
        .take(len as usize)
        .map_while(|line| {
            let line = line.trim();
            if real {
                line.parse::<f64>().ok().map(|v| v.to_bits() as i64)
            } else {
                line.parse::<i32>().ok().map(i64::from)
            }
        })
        .collect();
    Some((base, values))
}

/// A resource NKA's string values, prepared before script execution.
fn text_data_array(script: &Script, name: &str, bytes: &[u8]) -> Option<(u32, Vec<Text>)> {
    let var = script.variables.iter().find(|v| {
        v.name
            .get(1..)
            .is_some_and(|n| n.eq_ignore_ascii_case(name))
    })?;
    let Cells::Text { base, len } = var.cells else {
        return None;
    };
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    if lines.next()?.trim() != &*var.name {
        return None;
    }
    let values = lines
        .take(len as usize)
        .map(|line| Text::from_str_cut(line).0)
        .collect();
    Some((base, values))
}

/// Control ids, the value parameter's symbol and, by variable (a UI id
/// counts every variable), each control's value cell.
fn controls(script: &Script) -> Controls {
    let value_par = symbol(script, CONTROL_VALUE);
    let mut cells = vec![None; script.variables.len()];
    let mut steps = vec![0; script.variables.len()];
    for c in &script.controls {
        let (value, len) = match script.variables.get(c.variable as usize).map(|v| v.cells) {
            Some(Cells::Int { base, len }) => (Some(base), len),
            Some(Cells::Real { base, .. }) => (Some(base), 0),
            _ => (None, 0),
        };
        if let Some(cell) = cells.get_mut(c.variable as usize) {
            *cell = value;
        }
        if let Some(n) = steps.get_mut(c.variable as usize) {
            *n = len;
        }
    }
    let mut by_cell: Vec<(u32, u32)> = (cells.iter().zip(0u32..))
        .filter_map(|(c, n)| Some(((*c)?, n)))
        .collect();
    // The first control on a cell wins, as the scan it replaces found it.
    by_cell.sort_unstable();
    by_cell.dedup_by_key(|&mut (c, _)| c);
    Controls {
        first: FIRST_UI_ID,
        by_cell,
        value_par,
        num_items_par: symbol(script, "$CONTROL_PAR_NUM_ITEMS"),
        selected_item_par: symbol(script, "$CONTROL_PAR_SELECTED_ITEM_IDX"),
        cells,
        steps,
        look: look(script),
        key_pars: [
            "$CONTROL_PAR_KEY_SHIFT",
            "$CONTROL_PAR_KEY_ALT",
            "$CONTROL_PAR_KEY_CONTROL",
        ]
        .map(|name| symbol(script, name)),
    }
}

/// The parameters the UI-look builtins set, after the script's own symbols:
/// a script that names one shares its id; `move_control`'s grid and the
/// table's shown steps have no constant, so always get their own.
pub(crate) const LOOK: [&str; 16] = [
    "$CONTROL_PAR_TEXT",
    "$CONTROL_PAR_LABEL",
    "$CONTROL_PAR_HELP",
    "$CONTROL_PAR_UNIT",
    "$CONTROL_PAR_DEFAULT_VALUE",
    "$CONTROL_PAR_POS_X",
    "$CONTROL_PAR_POS_Y",
    "$CONTROL_PAR_HIDE",
    "grid_x",
    "grid_y",
    "table_steps_shown",
    "level_meter",
    "level_meter_group",
    "level_meter_generic",
    "$CONTROL_PAR_PICTURE",
    "$CONTROL_PAR_PICTURE_STATE",
];

/// The id the look parameter `name` is stored under.
pub(crate) fn look_par(script: &Script, name: &str) -> i32 {
    match symbol(script, name) {
        i32::MIN => {
            let k = LOOK.iter().position(|l| *l == name).unwrap_or(LOOK.len());
            SYMBOL_BASE + (script.symbols.len() + k) as i32
        }
        id => id,
    }
}

fn look(script: &Script) -> LookPars {
    let par = |name| look_par(script, name);
    let id = |name| kontra_ksp::constant(name).unwrap_or(i32::MIN);
    LookPars {
        text: par("$CONTROL_PAR_TEXT"),
        label: par("$CONTROL_PAR_LABEL"),
        help: par("$CONTROL_PAR_HELP"),
        unit: par("$CONTROL_PAR_UNIT"),
        default: par("$CONTROL_PAR_DEFAULT_VALUE"),
        pos_x: par("$CONTROL_PAR_POS_X"),
        pos_y: par("$CONTROL_PAR_POS_Y"),
        hide: par("$CONTROL_PAR_HIDE"),
        grid_x: par("grid_x"),
        grid_y: par("grid_y"),
        steps_shown: par("table_steps_shown"),
        meter: par("level_meter"),
        meter_group: par("level_meter_group"),
        meter_generic: par("level_meter_generic"),
        picture: par("$CONTROL_PAR_PICTURE"),
        picture_state: par("$CONTROL_PAR_PICTURE_STATE"),
        wallpaper: id("$INST_WALLPAPER_ID"),
        icon: id("$INST_ICON_ID"),
    }
}

/// The value a catalog constant has in `script`; `i32::MIN` (matches nothing)
/// when the script never names it.
fn symbol(script: &Script, name: &str) -> i32 {
    script
        .symbols
        .iter()
        .position(|s| s.eq_ignore_ascii_case(name))
        .and_then(|n| i32::try_from(n).ok())
        .map_or(i32::MIN, |n| SYMBOL_BASE + n)
}

/// The engine's view of a UVI (Lua) script: its own machine runs it, so the
/// bytecode program stays empty.
#[must_use]
pub fn lua_plan(compiled: kontra_lua::Compiled) -> ScriptPlan {
    let defined = compiled.callbacks;
    let entry = |on: bool, n: u32| on.then_some(n);
    ScriptPlan {
        program: kontra_engine::Program::default(),
        callbacks: Callbacks {
            init: entry(defined.init, kontra_lua::Callbacks::INIT),
            note: entry(defined.note, kontra_lua::Callbacks::NOTE),
            release: entry(defined.release, kontra_lua::Callbacks::RELEASE),
            controller: entry(defined.controller, kontra_lua::Callbacks::CONTROLLER),
            persistence_changed: None,
            listener: None,
            pgs_changed: None,
            ui_controls: None,
            async_complete: None,
            ui_control: Vec::new(),
            widget: Some(kontra_lua::Callbacks::WIDGET),
        },
        coroutines: COROUTINES,
        controls: Controls::default(),
        engine_pars: EnginePars::default(),
        // No reverb parameter ids are bound, so these are never read.
        reverb_laws: std::array::from_fn(|_| kontra_engine::ir::Law::Linear { lo: 0.0, hi: 1.0 }),
        reverb_networks: Vec::new(),
        knobs: Vec::new(),
        data: Vec::new(),
        text_data: Vec::new(),
        source_folder: None,
        library_folder: None,
        factory_folder: None,
        restore: Vec::new(),
        restore_texts: Vec::new(),
        foreign: Some(std::sync::Arc::new(compiled)),
        files: None,
        arrays: Vec::new(),
        // A UVI script never sees the notes it plays. In UVIWorkstation's
        // built-in Lua (0x1425676a0) `releaseVoice` and a `playNote` duration
        // post a NoteOff with `postEvent`, which only appends to the
        // manager's outgoing events (0x141397330 → 0x1413889b0), and a
        // script without `onRelease` forwards each NoteOff by `postEvent`
        // (`_onRelease`): fed back, every pass-through script would loop. A
        // host note-off still reaches it. (Augmented Orchestra's arpeggiator
        // releases the held key's notes from `onRelease`.)
        hears_own_releases: false,
        // A held-back NoteOff keeps the posted note (`_onRelease` posts it).
        held_release_ends_children: false,
        links: Vec::new(),
    }
}

#[cfg(test)]
mod folder_tests {
    use std::path::Path;

    #[test]
    fn folder_handles_relative_paths_and_native_separators() {
        let actual = super::source_folder(Some(Path::new("Test.nki"))).unwrap();
        let expected = format!("{}/", std::env::current_dir().unwrap().display());
        assert_eq!(actual.as_str(), expected.replace('\\', "/"));
        #[cfg(unix)]
        assert_eq!(
            super::source_folder(Some(Path::new(r"/library/foo\bar/Test.nki")))
                .unwrap()
                .as_str(),
            r"/library/foo\bar/"
        );
    }
}
