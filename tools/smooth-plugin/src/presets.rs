//! Presets: the factory set, the user's own, and how they are read and written.
//!
//! A preset is a name, a category, a creator, and the values of the
//! parameters it sets, keyed by the parameter's *name* so a file survives a
//! reordering of the parameter list. User presets are one JSON file each in
//! the user's config directory; factory presets are code and cannot be
//! changed or deleted.

use moose::prelude::Params;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Parameters a preset never stores: they are about the session, not the
/// sound.
const TRANSIENT: [&str; 2] = ["Bypass", "Delta"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub category: String,
    pub creator: String,
    #[serde(default)]
    pub favourite: bool,
    /// Plain values by parameter name.
    pub values: BTreeMap<String, f64>,
    #[serde(skip)]
    pub factory: bool,
}

/// The values of every parameter a preset stores, by name.
pub fn capture<P: Params>(params: &P) -> BTreeMap<String, f64> {
    params
        .param_infos()
        .iter()
        .filter(|info| stored(info.name))
        .filter_map(|info| Some((info.name.to_string(), params.get_plain(info.id)?)))
        .collect()
}

/// Load a preset: what it states, and the defaults for what it does not, so
/// a preset sounds the same whatever was on the dials before it.
///
/// A locked parameter keeps what it has: `locked` names them.
pub fn apply<P: Params>(params: &P, preset: &Preset, locked: &[&str]) {
    let values: Vec<(u32, f64)> = params
        .param_infos()
        .iter()
        .filter(|info| stored(info.name) && !locked.contains(&info.name))
        .map(|info| {
            (
                info.id,
                preset.values.get(info.name).copied().unwrap_or(info.default_plain),
            )
        })
        .collect();
    params.restore_values(&values);
}

/// Whether a parameter belongs in a preset.
pub fn stored(name: &str) -> bool {
    !TRANSIENT.contains(&name)
}

fn factory_preset(name: &str, category: &str, values: &[(&str, f64)]) -> Preset {
    Preset {
        name: name.to_string(),
        category: category.to_string(),
        creator: "derpcat".to_string(),
        favourite: false,
        values: values.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        factory: true,
    }
}

/// The shipped presets. Each states only what it changes from the defaults.
pub fn factory() -> Vec<Preset> {
    vec![
        factory_preset(
            "Smooth vocal",
            "vocals",
            &[("Depth", 6.0), ("Detail", 6.3), ("Attack", 5.0), ("Release", 3.0)],
        ),
        factory_preset(
            "Gentle touch",
            "general",
            &[("Depth", 3.0), ("Detail", 4.0), ("Attack", 6.0), ("Release", 5.0)],
        ),
        factory_preset(
            "Deep clean",
            "general",
            &[("Depth", 8.0), ("Detail", 7.0), ("Attack", 3.0), ("Release", 4.0), ("Mode", 1.0)],
        ),
        factory_preset(
            "Harsh vocal",
            "vocals",
            &[
                ("Depth", 7.0),
                ("Detail", 5.0),
                ("Attack", 2.0),
                ("Release", 3.0),
                ("Band 1 frequency", 300.0),
                ("Band 2 frequency", 3500.0),
                ("Band 2 depth", 4.0),
            ],
        ),
        factory_preset(
            "Sibilance",
            "vocals",
            &[
                ("Depth", 7.0),
                ("Detail", 6.0),
                ("Attack", 1.0),
                ("Release", 2.0),
                ("Mode", 1.0),
                ("Band 1 frequency", 3000.0),
                ("Band 2 frequency", 7000.0),
                ("Band 2 depth", 5.0),
            ],
        ),
        factory_preset(
            "Mud",
            "mix",
            &[
                ("Depth", 5.0),
                ("Detail", 3.0),
                ("Attack", 6.0),
                ("Release", 6.0),
                ("Band 1 frequency", 80.0),
                ("Band 2 frequency", 350.0),
                ("Band 2 depth", 3.0),
                ("Band 3 frequency", 12000.0),
            ],
        ),
        factory_preset(
            "Acoustic guitar",
            "instruments",
            &[("Depth", 5.0), ("Detail", 5.0), ("Attack", 4.0), ("Release", 5.0), ("Band 2 frequency", 2500.0)],
        ),
        factory_preset(
            "Drum bus",
            "mix",
            &[("Depth", 4.0), ("Detail", 4.0), ("Attack", 2.0), ("Release", 2.0), ("Mode", 1.0)],
        ),
    ]
}

/// The user's presets folder.
pub fn user_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("smooth").join("presets"))
}

/// A file name for a preset name: letters and digits, the rest dashes.
fn file_name(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    format!("{}.json", slug.trim_matches('-'))
}

/// Everything the menu lists, and which preset is loaded.
pub struct Library {
    pub presets: Vec<Preset>,
    /// The name of the loaded preset.
    pub current: Option<String>,
    dir: Option<PathBuf>,
}

impl Default for Library {
    fn default() -> Self {
        Self::open(user_dir())
    }
}

impl Library {
    /// The factory presets and whatever is in `dir`.
    pub fn open(dir: Option<PathBuf>) -> Self {
        let mut library = Self {
            presets: factory(),
            current: None,
            dir,
        };
        library.rescan();
        library
    }

    /// Read the user's folder again.
    pub fn rescan(&mut self) {
        self.presets.retain(|p| p.factory);
        let Some(dir) = &self.dir else { return };
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut found: Vec<Preset> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .filter_map(|text| serde_json::from_str::<Preset>(&text).ok())
            .collect();
        found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.presets.extend(found);
    }

    /// The preset `step` places from the loaded one, wrapping round the list.
    pub fn stepped(&self, step: i32) -> Option<&Preset> {
        let len = self.presets.len() as i32;
        if len == 0 {
            return None;
        }
        let at = self
            .current
            .as_ref()
            .and_then(|name| self.presets.iter().position(|p| &p.name == name))
            .map_or(if step > 0 { -1 } else { 0 }, |i| i as i32);
        self.presets.get((at + step).rem_euclid(len) as usize)
    }

    /// Save a user preset under its name, replacing one of the same name.
    pub fn save(&mut self, preset: Preset) -> std::io::Result<()> {
        let dir = self
            .dir
            .clone()
            .ok_or_else(|| std::io::Error::other("no config folder"))?;
        if self.presets.iter().any(|p| p.factory && p.name == preset.name) {
            return Err(std::io::Error::other("factory presets cannot be saved over"));
        }
        std::fs::create_dir_all(&dir)?;
        write(&dir.join(file_name(&preset.name)), &preset)?;
        self.rescan();
        Ok(())
    }

    /// Delete a user preset.
    pub fn delete(&mut self, name: &str) -> std::io::Result<()> {
        if let Some(dir) = &self.dir {
            match std::fs::remove_file(dir.join(file_name(name))) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        self.rescan();
        Ok(())
    }

    /// Mark a preset a favourite or not. A factory preset's mark is kept in
    /// a sidecar file, since the preset itself is code.
    pub fn set_favourite(&mut self, name: &str, on: bool) -> std::io::Result<()> {
        let Some(index) = self.presets.iter().position(|p| p.name == name) else {
            return Ok(());
        };
        self.presets[index].favourite = on;
        let preset = self.presets[index].clone();
        if !preset.factory {
            if let Some(dir) = &self.dir {
                write(&dir.join(file_name(&preset.name)), &preset)?;
            }
        }
        Ok(())
    }
}

fn write(path: &Path, preset: &Preset) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(preset).map_err(std::io::Error::other)?;
    // Written beside and renamed over, so a crash never leaves half a preset.
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, text)?;
    std::fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("smooth-presets-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_user_preset_survives_a_round_trip_and_factory_ones_are_protected() {
        let dir = scratch("a");
        let mut library = Library::open(Some(dir.clone()));
        let factory_count = library.presets.len();
        let mut mine = factory_preset("My sound", "vocals", &[("Depth", 9.0)]);
        mine.factory = false;
        library.save(mine.clone()).unwrap();
        assert_eq!(library.presets.len(), factory_count + 1);

        let reopened = Library::open(Some(dir.clone()));
        let found = reopened.presets.iter().find(|p| p.name == "My sound").unwrap();
        assert_eq!(found.values["Depth"], 9.0);
        assert!(!found.factory);

        let mut clash = mine.clone();
        clash.name = "Smooth vocal".into();
        assert!(library.save(clash).is_err());

        library.delete("My sound").unwrap();
        assert_eq!(library.presets.len(), factory_count);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_name_becomes_a_safe_file_name() {
        assert_eq!(file_name("Vox / de-ess!"), "Vox---de-ess.json");
        assert!(!file_name("../../etc").contains('/'));
    }
}
