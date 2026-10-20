//! Presets: the factory set, the user's own, and how they are read and written.
//!
//! A preset is one JSON file in the user's folder, shaped as the reference's
//! `.preset` is: a name, a category from the reference's fourteen, a
//! description, the author, the plug-in version and revision, and the values
//! of the parameters it sets, keyed by the parameter's *name* so a file
//! survives a reordering of the parameter list. Favourites are a list of
//! names in a sidecar file, so a factory preset (which is code and cannot be
//! changed) can still be one. The default pin is `user_default.preset`, a copy
//! of the preset that new instances open with.

use moose::prelude::Params;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Parameters a preset never stores: they are about the session, not the
/// sound.
const TRANSIENT: [&str; 2] = ["Bypass", "Delta"];

/// The author's entry: who made the preset and what kind of preset it is. The
/// reference marks a user preset with type 3 and reads factory ones as any
/// other type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Author {
    pub author_name: String,
    pub author_type: u8,
}

/// The user's type; factory presets carry a different one.
pub const AUTHOR_USER: u8 = 3;
pub const AUTHOR_FACTORY: u8 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    /// One of [`CATEGORIES`].
    pub category: String,
    #[serde(default)]
    pub description: String,
    pub author: Author,
    #[serde(default)]
    pub plugin_version: String,
    #[serde(default = "first_revision")]
    pub preset_revision: u32,
    /// Plain values by parameter name. The older files called this `values`.
    #[serde(alias = "values")]
    pub params: BTreeMap<String, f64>,
    #[serde(skip)]
    pub factory: bool,
    #[serde(skip)]
    pub favourite: bool,
}

fn first_revision() -> u32 {
    1
}

impl Preset {
    /// A preset as the user saved it: the creator's name, the user's type.
    pub fn new(name: &str, category: &str, creator: &str, params: BTreeMap<String, f64>) -> Self {
        Self {
            name: name.to_string(),
            category: category.to_string(),
            description: String::new(),
            author: Author {
                author_name: creator.to_string(),
                author_type: AUTHOR_USER,
            },
            plugin_version: env!("CARGO_PKG_VERSION").to_string(),
            preset_revision: 1,
            params,
            factory: false,
            favourite: false,
        }
    }
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
                preset.params.get(info.name).copied().unwrap_or(info.default_plain),
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
        description: String::new(),
        author: Author {
            author_name: "derpcat".to_string(),
            author_type: AUTHOR_FACTORY,
        },
        plugin_version: env!("CARGO_PKG_VERSION").to_string(),
        preset_revision: 1,
        params: values.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        factory: true,
        favourite: false,
    }
}

/// The shipped presets. Each states only what it changes from the defaults.
pub fn factory() -> Vec<Preset> {
    vec![
        factory_preset(
            "Smooth vocal",
            "Vocals",
            &[("Depth", 6.0), ("Detail", 6.3), ("Attack", 5.0), ("Release", 3.0)],
        ),
        factory_preset(
            "Gentle touch",
            "Misc",
            &[("Depth", 3.0), ("Detail", 4.0), ("Attack", 6.0), ("Release", 5.0)],
        ),
        factory_preset(
            "Deep clean",
            "Misc",
            &[("Depth", 8.0), ("Detail", 7.0), ("Attack", 3.0), ("Release", 4.0), ("Mode", 1.0)],
        ),
        factory_preset(
            "Harsh vocal",
            "Vocals",
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
            "Vocals",
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
            "Master bus",
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
            "Guitar",
            &[("Depth", 5.0), ("Detail", 5.0), ("Attack", 4.0), ("Release", 5.0), ("Band 2 frequency", 2500.0)],
        ),
        factory_preset(
            "Drum bus",
            "Drums",
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
    format!("{}.preset", slug.trim_matches('-'))
}

/// The sidecar that lists the favourites by name.
const FAVOURITES: &str = "favorite_presets.json";
/// The pinned default: a copy of the preset new instances open with.
const DEFAULT: &str = "user_default.preset";

/// Everything the menu lists, and which preset is loaded.
pub struct Library {
    pub presets: Vec<Preset>,
    /// The name of the loaded preset.
    pub current: Option<String>,
    /// The name of the pinned default, if one is pinned.
    pub default: Option<String>,
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
            default: None,
            dir,
        };
        library.rescan();
        library
    }

    /// Read the user's folder again: the presets, the favourites, the pin.
    pub fn rescan(&mut self) {
        self.presets.retain(|p| p.factory);
        let favourites = self.read_favourites();
        for preset in &mut self.presets {
            preset.favourite = favourites.contains(&preset.name);
        }
        let Some(dir) = self.dir.clone() else { return };
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        let mut found: Vec<Preset> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "json" || x == "preset"))
            .filter(|e| e.file_name() != FAVOURITES && e.file_name() != DEFAULT)
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .filter_map(|text| serde_json::from_str::<Preset>(&text).ok())
            .map(|mut p| {
                p.favourite = favourites.contains(&p.name);
                p
            })
            .collect();
        found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.presets.extend(found);
        self.default = std::fs::read_to_string(dir.join(DEFAULT))
            .ok()
            .and_then(|text| serde_json::from_str::<Preset>(&text).ok())
            .map(|p| p.name);
    }

    fn read_favourites(&self) -> Vec<String> {
        self.dir
            .as_ref()
            .and_then(|dir| std::fs::read_to_string(dir.join(FAVOURITES)).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
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

    /// Delete a user preset. A deleted default is unpinned with it.
    pub fn delete(&mut self, name: &str) -> std::io::Result<()> {
        if let Some(dir) = &self.dir {
            match std::fs::remove_file(dir.join(file_name(name))) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
            if self.default.as_deref() == Some(name) {
                let _ = std::fs::remove_file(dir.join(DEFAULT));
            }
        }
        self.rescan();
        Ok(())
    }

    /// Pin a preset as the default new instances open with, or unpin it.
    pub fn set_default(&mut self, name: &str, on: bool) -> std::io::Result<()> {
        let Some(dir) = self.dir.clone() else { return Ok(()) };
        if !on {
            if self.default.as_deref() == Some(name) {
                match std::fs::remove_file(dir.join(DEFAULT)) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                    _ => {}
                }
            }
        } else if let Some(preset) = self.presets.iter().find(|p| p.name == name).cloned() {
            std::fs::create_dir_all(&dir)?;
            write(&dir.join(DEFAULT), &preset)?;
        }
        self.rescan();
        Ok(())
    }

    /// Mark a preset a favourite or not. Every preset's mark lives in the
    /// sidecar, factory ones included, so it survives a restart.
    pub fn set_favourite(&mut self, name: &str, on: bool) -> std::io::Result<()> {
        let Some(dir) = self.dir.clone() else { return Ok(()) };
        let mut names = self.read_favourites();
        names.retain(|n| n != name);
        if on {
            names.push(name.to_string());
        }
        std::fs::create_dir_all(&dir)?;
        let text = serde_json::to_string_pretty(&names).map_err(std::io::Error::other)?;
        let temporary = dir.join(format!("{FAVOURITES}.tmp"));
        std::fs::write(&temporary, text)?;
        std::fs::rename(temporary, dir.join(FAVOURITES))?;
        self.rescan();
        Ok(())
    }
}

fn write(path: &Path, preset: &Preset) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(preset).map_err(std::io::Error::other)?;
    // Written beside and renamed over, so a crash never leaves half a preset.
    let temporary = path.with_extension("tmp");
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
        let mine = Preset::new("My sound", "Vocals", "you", BTreeMap::from([("Depth".to_string(), 9.0)]));
        library.save(mine.clone()).unwrap();
        assert_eq!(library.presets.len(), factory_count + 1);

        let reopened = Library::open(Some(dir.clone()));
        let found = reopened.presets.iter().find(|p| p.name == "My sound").unwrap();
        assert_eq!(found.params["Depth"], 9.0);
        assert!(!found.factory);

        let mut clash = mine.clone();
        clash.name = "Smooth vocal".into();
        assert!(library.save(clash).is_err());

        library.delete("My sound").unwrap();
        assert_eq!(library.presets.len(), factory_count);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn favourites_and_the_default_pin_survive_a_restart_for_factory_presets_too() {
        let dir = scratch("b");
        let mut library = Library::open(Some(dir.clone()));
        library.set_favourite("Smooth vocal", true).unwrap();
        library.set_default("Smooth vocal", true).unwrap();
        let reopened = Library::open(Some(dir.clone()));
        assert!(reopened.presets.iter().find(|p| p.name == "Smooth vocal").unwrap().favourite);
        assert_eq!(reopened.default.as_deref(), Some("Smooth vocal"));
        let mut library = reopened;
        library.set_default("Smooth vocal", false).unwrap();
        assert_eq!(Library::open(Some(dir.clone())).default, None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_file_reads_the_reference_shape_and_the_older_values_key() {
        let text = r#"{"name":"Old","category":"Vocals","author":{"author_name":"me","author_type":3},
            "values":{"Depth":4.0}}"#;
        let preset: Preset = serde_json::from_str(text).unwrap();
        assert_eq!(preset.params["Depth"], 4.0);
        assert_eq!(preset.preset_revision, 1);
        assert_eq!(preset.author.author_type, AUTHOR_USER);
    }

    #[test]
    fn every_factory_preset_sits_in_a_reference_category() {
        for preset in factory() {
            assert!(smooth_ui::presets::CATEGORIES.contains(&preset.category.as_str()), "{}", preset.name);
        }
    }

    #[test]
    fn a_name_becomes_a_safe_file_name() {
        assert_eq!(file_name("Vox / de-ess!"), "Vox---de-ess.preset");
        assert!(!file_name("../../etc").contains('/'));
    }
}
