//! A preset of the instrument a part already plays (a Kontakt snapshot)
//! recalls into the part instead of loading the instrument again: its
//! plan, samples and effects stay; only the scripts' saved values change,
//! the way the scripts asked ([`Recall`]).

use std::path::Path;
use std::sync::Arc;

use crate::instrument::Preset;
use kontra_engine::{Engine, Plan, Recall, Saved, ir};

use super::command::Recalling;
use super::{Command, Part, Rack, ScriptState};

/// Puts the effect types and responses scripts loaded after `on init` into
/// `plan`'s slots, before its `on init` runs (a load and a restart alike);
/// those the plan took.
pub(crate) fn put_slots(
    plan: &mut Plan,
    types: &[(ir::RackAt, u8, u16)],
    responses: &[(ir::RackAt, u8, String)],
    rate: u32,
) -> crate::state::LiveSlots {
    let types = (types.iter().copied())
        .filter(|&(at, slot, ty)| plan.load_type(at, slot, ty))
        .collect();
    let responses = (responses.iter())
        .filter(|(at, slot, name)| plan.load_response(*at, *slot, name, rate))
        .cloned()
        .collect();
    (types, responses)
}

/// `a` and `b` hold the same entries, in any order.
fn same<T: PartialEq>(a: &[T], b: &[T]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.contains(x))
}

impl Rack {
    /// Recalls `preset` into `part` when the part plays the preset's base
    /// file and only KSP scripts; false (nothing done) when it must load. A
    /// Lua script's saved state may pick other samples, so a part with one
    /// always loads; so does a preset with no values for a slot that keeps
    /// some (the base's own apply, which only a load reads).
    pub fn recall(&mut self, part: usize, preset: &Preset, path: Option<&Path>) -> bool {
        // Every recall is newer than a snapshot's still being made: that
        // one never lands (a load moves the part's generation instead).
        self.native.retain(|n| n.part != part);
        let Some(Some(p)) = self.parts.get_mut(part) else {
            return false;
        };
        let same = p.source.as_deref() == Some(preset.base.as_path());
        let ksp = (p.script.vars.iter()).all(|v| v.widgets.is_empty() && !v.state);
        let s = &p.script;
        // The preset's values for instrument script `n`, by its slot.
        let of = |n: usize| {
            let slot = s.slots.get(n)?;
            preset.saved.get(*slot).filter(|v| !v.is_empty())
        };
        let complete = (s.vars.iter()).all(|v| v.vars.is_empty() || of(v.script).is_some());
        let Some(plan) = p.plan.clone().filter(|_| same && ksp && complete) else {
            return false;
        };
        let saved: Vec<Saved> = (s.vars.iter())
            .map(|v| v.recall(of(v.script).map_or(&[], Vec::as_slice)))
            .collect();
        // What a save writes: the preset's values, the loaded ones where it
        // has none (scripts with nothing to save).
        let loaded: Vec<Vec<String>> = (s.loaded.iter().enumerate())
            .map(|(n, old)| of(n).unwrap_or(old).clone())
            .collect();
        p.name.clone_from(&preset.name);
        p.path = path.map(Path::to_owned);
        p.modified = false;
        if let Some(path) = path.filter(|_| preset.effects) {
            self.recall_native(part, plan, path, (saved, loaded));
            return true;
        }
        match p.recall {
            Recall::Keep => {
                p.script.loaded = loaded;
                p.script.latest = None;
                let generation = p.script.generation;
                self.send(Command::Recall(Box::new(Recalling {
                    part,
                    generation,
                    saved: saved.into_boxed_slice(),
                    engine: None,
                })));
            }
            Recall::Restart => self.restart(part, &plan, saved, loaded),
        }
        true
    }

    /// The preset at `path` played by `part`: recalled into it when it plays
    /// its base, else a user preset loads with its values and its scripts'
    /// effect types and responses. None for any other file.
    pub(super) fn load_preset(
        &mut self,
        part: usize,
        path: &Path,
    ) -> Option<Result<(), Box<dyn std::error::Error>>> {
        let preset =
            crate::instrument::preset(path, &self.known.1).or_else(|| self.user_preset(part, path));
        if let Some(preset) = preset
            && self.recall(part, &preset, Some(path))
        {
            return Some(Ok(()));
        }
        let user = crate::presets::read(path)?;
        let (types, responses) = crate::state::live_slots(&user.slot_types, &user.slot_responses);
        Some(self.load_saved(part, path, &user.scripts, &types, &responses))
    }

    /// The user preset at `path` as a [`Preset`] for `part`: its values by
    /// the slot each of the part's scripts sits in. None for other files,
    /// and when its effect types or responses differ from the part's (those
    /// only a load puts in).
    pub(crate) fn user_preset(&self, part: usize, path: &Path) -> Option<Preset> {
        let user = crate::presets::read(path)?;
        let p = self.parts.get(part)?.as_ref()?;
        let (types, responses) = crate::state::live_slots(&user.slot_types, &user.slot_responses);
        if !(same(&types, &p.slot_types) && same(&responses, &p.slot_responses)) {
            return None;
        }
        let slots = &p.script.slots;
        let mut saved = vec![Vec::new(); slots.iter().max().map_or(0, |m| m + 1)];
        for (slot, values) in slots.iter().zip(user.scripts) {
            if let Some(s) = saved.get_mut(*slot) {
                *s = values;
            }
        }
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned());
        Some(Preset {
            name: name.unwrap_or_default(),
            base: user.base,
            saved,
            effects: false,
        })
    }

    /// Saves what `part` plays as user preset `name` of `library` under
    /// `folder` (over one of that name only with [`Overwrite::Replace`]),
    /// which it then plays; the file's path.
    ///
    /// [`Overwrite::Replace`]: crate::presets::Overwrite::Replace
    ///
    /// # Errors
    /// As [`crate::presets::save`], and when the part plays nothing.
    pub fn save_preset(
        &mut self,
        part: usize,
        folder: &Path,
        library: &str,
        name: &str,
        overwrite: crate::presets::Overwrite,
    ) -> std::io::Result<std::path::PathBuf> {
        let scripts = self.saved_scripts(part);
        let p = self.parts.get(part).and_then(Option::as_ref);
        let p = p.ok_or_else(|| std::io::Error::other("no instrument in this part"))?;
        let base = p.source.clone();
        let base = base.ok_or_else(|| std::io::Error::other("no instrument in this part"))?;
        let (slot_types, slot_responses) =
            crate::state::saved_slots(&p.slot_types, &p.slot_responses);
        let preset = crate::presets::UserPreset {
            version: crate::presets::VERSION,
            base,
            scripts,
            slot_types,
            slot_responses,
        };
        let path = crate::presets::save(folder, library, name, &preset, overwrite)?;
        self.set_preset(part, &path);
        Ok(path)
    }

    /// `part` plays the preset at `path` as saved there (named by its file,
    /// not modified): a script reached it, or it was saved or renamed.
    pub fn set_preset(&mut self, part: usize, path: &Path) {
        if let Some(Some(p)) = self.parts.get_mut(part) {
            p.name = named(path);
            p.path = Some(path.to_owned());
            p.modified = false;
        }
    }

    /// The user preset file `from` was renamed to `to`, or deleted (None):
    /// every part playing it follows, keeping its changed mark; a part
    /// whose file is gone plays on as its base instrument, changed, and is
    /// named by it.
    pub fn preset_moved(&mut self, from: &Path, to: Option<&Path>) {
        for p in self.parts.iter_mut().flatten() {
            if p.path.as_deref() != Some(from) {
                continue;
            }
            match to {
                Some(to) => {
                    p.path = Some(to.to_owned());
                    p.name = named(to);
                }
                None => {
                    p.path.clone_from(&p.source);
                    p.name = p.source.as_deref().map(named).unwrap_or_default();
                    p.modified = true;
                }
            }
        }
    }

    /// Marks `part`'s preset as changed by the player ([`Part::modified`]),
    /// as a project saved it.
    pub fn set_modified(&mut self, part: usize) {
        if let Some(Some(p)) = self.parts.get_mut(part) {
            p.modified = true;
        }
    }

    /// Starts a new engine for `part` on a copy of `plan` with `saved` as its
    /// restored values: `on init` runs again, nothing is read or prepared.
    pub(super) fn restart(
        &mut self,
        part: usize,
        plan: &Plan,
        saved: Vec<Saved>,
        loaded: Vec<Vec<String>>,
    ) {
        let Some(Some(p)) = self.parts.get(part) else {
            return;
        };
        // The part's plan is as it loaded: what its scripts loaded since
        // goes in as a load puts it.
        let slots = (p.slot_types.clone(), p.slot_responses.clone());
        let engine = (self.store.client(), self.rate);
        // Its library's resources are found near the instrument file.
        let face = (
            p.faces.as_slice(),
            p.source.as_deref().or(p.path.as_deref()),
        );
        let started = started(plan.clone(), &slots, saved, engine, face);
        self.restarted(part, started, loaded);
    }

    /// Recalls into the engine the audio side lent, here, and installs it
    /// again with the plan its zone edits left; one whose part moved on
    /// is dropped.
    pub(super) fn recalled(&mut self, mut recalling: Recalling) {
        let Some(mut engine) = recalling.engine.take() else {
            return;
        };
        let part = recalling.part;
        let ours = |p: &Part| p.script.generation == recalling.generation;
        let Some(mut info) = self
            .parts
            .get_mut(part)
            .filter(|p| p.as_ref().is_some_and(ours))
            .and_then(Option::take)
        else {
            return;
        };
        engine.recall(std::mem::take(&mut recalling.saved));
        super::requests::init_tables(&mut engine, &self.store.client());
        info.plan = Some(Arc::clone(engine.plan()));
        // The Sound tab edits from what the recall left, not from before it.
        let revision = info.sounds.revision + 1;
        info.sounds = crate::sound::Sounds::new(self.rate, engine.group_sounds());
        info.sounds.revision = revision;
        // A capture still out answers the old load: ask the new one afresh.
        info.script.asked = false;
        self.install(part, engine, info);
    }

    /// `part` plays `started`, its scripts' values saved as `loaded`.
    pub(super) fn restarted(&mut self, part: usize, started: Started, loaded: Vec<Vec<String>>) {
        let Some(mut old) = self.parts.get_mut(part).and_then(Option::take) else {
            return;
        };
        let Started {
            engine,
            slots: (slot_types, slot_responses),
            view,
            diagnostics,
        } = started;
        // What it reports replaces the load's report of the same.
        let same = |d: &ir::Diagnostic, n: &ir::Diagnostic| {
            (d.impact, &d.feature) == (n.impact, &n.feature)
        };
        old.diagnostics
            .retain(|d| !diagnostics.iter().any(|n| same(d, n)));
        old.diagnostics.extend(diagnostics);
        // Its waveform displays keep the zone samples they show.
        let waves = old.view.as_mut().map(|v| std::mem::take(&mut v.waves));
        let view = view.map(|v| v.with_waves(waves.unwrap_or_default()));
        let info = Part {
            recall: engine.recall_law(),
            sounds: crate::sound::Sounds::new(self.rate, engine.group_sounds()),
            view,
            plan: Some(Arc::clone(engine.plan())),
            slot_types,
            slot_responses,
            script: ScriptState {
                vars: old.script.vars,
                loaded,
                slots: old.script.slots,
                ..ScriptState::default()
            },
            ..old
        };
        self.install(part, engine, info);
    }
}

/// A restarted part: its engine, the slots its plan took, its interface
/// on that engine and what the restart reports.
pub(super) struct Started {
    engine: Box<Engine>,
    slots: crate::state::LiveSlots,
    view: Option<crate::view::View>,
    pub(super) diagnostics: Vec<ir::Diagnostic>,
}

/// `plan` with `slots` (types, responses) put in and `saved` as its
/// restored values, an engine started on it (`on init` runs) and the
/// part's interface (`faces`, of the library at the path) on it. It needs
/// no rack, so it runs on any thread.
pub(super) fn started(
    mut plan: Plan,
    (types, responses): &crate::state::LiveSlots,
    saved: Vec<Saved>,
    (client, rate): (kontra_stream::StreamClient, u32),
    (faces, path): (&[Option<crate::ui::Face>], Option<&Path>),
) -> Started {
    let slots = put_slots(&mut plan, types, responses, rate);
    for (script, saved) in plan.scripts.iter_mut().zip(saved) {
        (script.restore, script.restore_texts) = (saved.ints, saved.texts);
    }
    let mut engine = Box::new(Engine::new(Arc::new(plan), client.clone(), rate));
    super::requests::init_tables(&mut engine, &client);
    let pages: Vec<Option<kontra_lua::Ui>> = faces.iter().map(|_| None).collect();
    let view = crate::view::View::of(faces, &pages, &engine, path);
    Started {
        engine,
        slots,
        view,
        diagnostics: Vec::new(),
    }
}

/// A part's name when it plays the file at `path`: the file's name.
fn named(path: &Path) -> String {
    (path.file_stem()).map_or_else(String::new, |s| s.to_string_lossy().into_owned())
}
