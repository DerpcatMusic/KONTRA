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
                ..ScriptState::default()
            },
            ..old
        };
        self.install(part, Box::new(engine), info);
    }
}
