//! A preset of the instrument a part already plays (a Kontakt snapshot)
//! recalls into the part instead of loading the instrument again: its
//! plan, samples and effects stay; only the scripts' saved values change,
//! the way the scripts asked ([`Recall`]).

use std::path::Path;
use std::sync::Arc;

use crate::instrument::Preset;
use kontra_engine::{Engine, Plan, Recall, Saved};

use super::{Command, Part, Rack, ScriptState};

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

    /// The user preset at `path` as a [`Preset`] for `part`: its values by
    /// the slot each of the part's scripts sits in. None for other files.
    pub(crate) fn user_preset(&self, part: usize, path: &Path) -> Option<Preset> {
        let user = crate::presets::read(path)?;
        let slots = &self.parts.get(part)?.as_ref()?.script.slots;
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
    /// `folder`, which it then plays; the file's path.
    ///
    /// # Errors
    /// When the part plays nothing, or the file cannot be written.
    pub fn save_preset(
        &mut self,
        part: usize,
        folder: &Path,
        library: &str,
        name: &str,
    ) -> std::io::Result<std::path::PathBuf> {
        let scripts = self.saved_scripts(part);
        let base = (self.parts.get(part).and_then(Option::as_ref)).and_then(|p| p.source.clone());
        let base = base.ok_or_else(|| std::io::Error::other("no instrument in this part"))?;
        let path = crate::presets::save(folder, library, name, &base, &scripts)?;
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
        for (script, saved) in plan.scripts.iter_mut().zip(saved) {
            (script.restore, script.restore_texts) = (saved.ints, saved.texts);
        }
        let plan = Arc::new(plan);
        let engine = Engine::new(Arc::clone(&plan), self.store.client(), self.rate);
        let pages: Vec<Option<kontra_lua::Ui>> = old.faces.iter().map(|_| None).collect();
        let view = crate::view::View::of(&old.faces, &pages, &engine, old.path.as_deref());
        let info = Part {
            recall: engine.recall_law(),
            sounds: crate::sound::Sounds::new(self.rate, engine.group_sounds()),
            view,
            plan: Some(plan),
            slot_types: Vec::new(),
            slot_responses: Vec::new(),
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
