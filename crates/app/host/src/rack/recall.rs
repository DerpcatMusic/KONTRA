//! A preset of the instrument a part already plays (a Kontakt snapshot)
//! recalls into the part instead of loading the instrument again: its
//! plan, samples and effects stay; only the scripts' saved values change,
//! the way the scripts asked ([`Recall`]).

use std::path::Path;
use std::sync::Arc;

use crate::instrument::Preset;
use kontra_engine::{Engine, Plan, Recall, Saved, ir};

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
        match p.recall {
            Recall::Keep => {
                p.script.loaded = loaded;
                p.script.latest = None;
                let generation = p.script.generation;
                let saved = saved.into_boxed_slice();
                self.send(Command::Recall {
                    part,
                    generation,
                    saved,
                });
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
        let preset = crate::instrument::preset(path).or_else(|| self.user_preset(part, path));
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
            p.name = path
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
            p.path = Some(path.to_owned());
            p.modified = false;
        }
    }

    /// The user preset file `from` was renamed to `to`, or deleted (None):
    /// every part playing it follows, keeping its changed mark; a part
    /// whose file is gone plays on as its base instrument, changed.
    pub fn preset_moved(&mut self, from: &Path, to: Option<&Path>) {
        for p in self.parts.iter_mut().flatten() {
            if p.path.as_deref() != Some(from) {
                continue;
            }
            match to {
                Some(to) => {
                    p.path = Some(to.to_owned());
                    let stem = to.file_stem().map(|s| s.to_string_lossy().into_owned());
                    p.name = stem.unwrap_or_default();
                }
                None => {
                    p.path.clone_from(&p.source);
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
    fn restart(&mut self, part: usize, plan: &Plan, saved: Vec<Saved>, loaded: Vec<Vec<String>>) {
        let Some(old) = self.parts.get_mut(part).and_then(Option::take) else {
            return;
        };
        let mut plan = plan.clone();
        // The part's plan is as it loaded: what its scripts loaded since
        // goes in as a load puts it.
        let (slot_types, slot_responses) =
            put_slots(&mut plan, &old.slot_types, &old.slot_responses, self.rate);
        for (script, saved) in plan.scripts.iter_mut().zip(saved) {
            (script.restore, script.restore_texts) = (saved.ints, saved.texts);
        }
        let plan = Arc::new(plan);
        let mut engine = Engine::new(plan, self.store.client(), self.rate);
        super::requests::init_tables(&mut engine, &self.store.client());
        let pages: Vec<Option<kontra_lua::Ui>> = old.faces.iter().map(|_| None).collect();
        let near = old.source.as_deref().or(old.path.as_deref());
        let view = crate::view::View::of(&old.faces, &pages, &engine, near);
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
        self.install(part, Box::new(engine), info);
    }
}
