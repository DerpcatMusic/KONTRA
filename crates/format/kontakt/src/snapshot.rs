//! A snapshot (.nksn): saved script values for an instrument it names but
//! does not hold. Read as its base instrument with those values, found the
//! way Kontakt's library browser pairs them: by name, near the snapshot.
//! Ported from v2 `crates/sampler-kontakt/src/snapshot.rs`
//! (`resolve_snapshot_base`, `find_snapshot_base`, `snapshot_binding_name`).

use std::path::{Path, PathBuf};

use kontra_ir as ir;
use ni_file::kontakt::Chunk;
use ni_file::kontakt::objects::{BParamArrayBParFX8, Snapshot, snapshot_metadata_names};

use crate::{Error, container, translate};

const SNAPSHOT: u16 = 0x4f;
const METADATA: u16 = 0x51;
/// Directory entries the base search may read before it gives up.
const BUDGET: usize = 16_384;

/// A snapshot as a preset of its base instrument: the base's file and the
/// saved values per script slot, without reading the base.
#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    /// The snapshot's own name (its file stem).
    pub name: String,
    /// The base instrument, absolute, as its IR's `source_path` names it.
    pub base: PathBuf,
    /// Saved persistent values by script slot; an empty slot keeps the
    /// base's own.
    pub saved: Vec<Vec<String>>,
    /// It saves the instrument's effect racks and groups' insert effects
    /// too, not only script values (Kontakt's native snapshot state).
    pub effects: bool,
}

/// What a snapshot saves of the instrument besides its scripts: the
/// program's racks (instrument insert, send, main and bus chunks, as the
/// program holds them) and each group's state, by group. The translator
/// reads them in place of the base's, by the same reader.
pub(crate) struct Native {
    pub racks: Vec<Chunk>,
    pub groups: Vec<Option<Group>>,
}

/// A group as a snapshot saves it: its insert effects, its 24 public bytes
/// (volume and pan as the group's own params hold them, tune in octaves,
/// the key-tracking, reverse and release-trigger flags, the release counter
/// and the interpolation quality), its source module and its modulators.
/// ANALOG STRINGS' 9001 Punch saves tune -2.0 for a group Kontakt then plays
/// 24 semitones down.
pub(crate) struct Group {
    pub fx: BParamArrayBParFX8,
    public: [u8; 24],
    /// Its source module's record and its internal and external modulator
    /// arrays, as the group holds them: they stand in for the base's.
    source: Vec<u8>,
    mods: Vec<Chunk>,
}

impl Group {
    fn of(g: ni_file::kontakt::objects::GroupSnapshot) -> Result<Self, ni_file::Error> {
        Ok(Self {
            public: g.public_data,
            source: g.source_record()?.to_vec(),
            mods: Vec::from(g.modulator_chunks()?),
            fx: g.fx,
        })
    }

    /// Puts its params, source and modulators into the base's `group`, which the
    /// translator then reads as it reads any group.
    ///
    /// # Errors
    /// When the base group's public data or source record does not decode.
    pub(crate) fn apply(
        &mut self,
        group: &mut ni_file::kontakt::objects::Group,
    ) -> Result<(), ni_file::Error> {
        group.set_snapshot_params(&self.public)?;
        group.set_source_record(&self.source)?;
        for chunk in std::mem::take(&mut self.mods) {
            let children = &mut group.0.children;
            match children.iter_mut().find(|c| c.id == chunk.id) {
                Some(own) => *own = chunk,
                None => children.push(chunk),
            }
        }
        Ok(())
    }
}

impl Native {
    /// None for a script-only snapshot (it saves no group data).
    ///
    /// # Errors
    /// When its group data does not decode: its effects are not guessed.
    fn of(state: Snapshot) -> Result<Option<Self>, ni_file::Error> {
        if state.groups.is_none() {
            return Ok(None);
        }
        let groups = (state.group_snapshots()?.into_iter())
            .map(|(_, g)| Group::of(g).map(Some))
            .collect::<Result<_, _>>()?;
        Ok(Some(Self {
            racks: state.effect_children,
            groups,
        }))
    }
}

/// Reads the snapshot at `path` as a [`Preset`]. Its base is the instrument
/// near it, else the one of `known` (the instruments the player has) it
/// names: Kontakt keeps user snapshots apart, a folder per instrument.
///
/// # Errors
/// When the file is not a snapshot this can read, or its base instrument
/// is not found.
pub fn preset(path: &Path, known: &[PathBuf]) -> Result<Preset, Error> {
    read(path, known).map(|(preset, _)| preset)
}

/// Writes the snapshot `from` to `to` with `held` (by script slot, the
/// values its scripts hold now) in place of the values it saved. Each slot
/// saves the names Kontakt saved in it, in its order: an instrument's own
/// (`make_instr_persistent`) values stay out of it, as in Kontakt's. The
/// rest of the file is kept, its preset written unencrypted.
///
/// # Errors
/// When `from` is not an NIS snapshot this reads, or `to` cannot be written.
pub fn save(from: &Path, to: &Path, held: &[(usize, Vec<String>)]) -> Result<(), Error> {
    let decode = |what| move |e: ni_file::Error| Error::new(from, what, e);
    let (bytes, _, encrypted) = container::decoded(from)?;
    let mut chunks = ni_file::kontakt::KontaktChunks::read(std::io::Cursor::new(bytes))
        .map_err(decode("Kontakt chunks"))?;
    let state = (chunks.0.iter_mut().find(|c| c.id == SNAPSHOT))
        .ok_or_else(|| Error::new(from, "snapshot", "missing"))?;
    let saved = Snapshot::try_from(&*state).map_err(decode("snapshot"))?;
    let values: Vec<Vec<String>> = (saved.persistent.iter().enumerate())
        .map(|(slot, own)| {
            let now = held
                .iter()
                .find(|(s, _)| *s == slot)
                .map(|(_, v)| v.as_slice());
            resaved(own, now.unwrap_or_default())
        })
        .collect();
    *state = Snapshot::with_persistent(state, &values).map_err(decode("snapshot"))?;
    let mut stream = Vec::new();
    chunks
        .write(&mut stream)
        .map_err(decode("Kontakt chunks"))?;
    let file = std::fs::File::open(from).map_err(|e| Error::new(from, "open", e))?;
    let ni_file::NIFile::NISoundContainer(mut root) =
        ni_file::NIFile::read(std::io::BufReader::new(file)).map_err(decode("container"))?
    else {
        return Err(Error::new(from, "container", "not an NIS snapshot"));
    };
    let key = match encrypted {
        Some(true) => Some(
            crate::access::library_key(from).map_err(|e| Error::new(from, "library access", e))?,
        ),
        _ => None,
    };
    (root.set_preset_chunk(&stream, key.as_deref())).map_err(decode("NIS preset"))?;
    let mut out = Vec::new();
    root.write(&mut out).map_err(decode("NIS container"))?;
    std::fs::write(to, out).map_err(|e| Error::new(to, "write", e))
}

/// A slot's saved values, `own`, each taking its value from the one of
/// `now` with its name. Names match as the host's do (`persist::restore`):
/// exact (case-insensitive, with sigil) first, then bare (without sigil).
/// Each keeps the name Kontakt saved and preserves an empty value's space
/// delimiter (`"@label "` stays `"@label "`).
fn resaved(own: &[String], now: &[String]) -> Vec<String> {
    fn split(v: &str) -> (&str, Option<&str>) {
        match v.split_once(' ') {
            Some((name, rest)) => (name, Some(rest)),
            None => (v, None),
        }
    }
    let bare = |n: &str| {
        n.trim_start_matches(['$', '%', '~', '?', '@', '!'])
            .to_ascii_lowercase()
    };
    (own.iter())
        .map(|v| {
            let (name, _) = split(v);
            let found = now
                .iter()
                .find(|h| split(h).0.eq_ignore_ascii_case(name))
                .or_else(|| now.iter().find(|h| bare(split(h).0) == bare(name)));
            match found {
                Some(h) => match split(h).1 {
                    Some(rest) => format!("{name} {rest}"),
                    None => name.to_owned(),
                },
                None => v.clone(),
            }
        })
        .collect()
}

/// [`preset`], with what it saves of the instrument.
fn read(path: &Path, known: &[PathBuf]) -> Result<(Preset, Snapshot), Error> {
    let decode = |what| move |e: ni_file::Error| Error::new(path, what, e);
    let chunks = container::chunks(path)?;
    let state =
        (chunks.find_first(SNAPSHOT)).ok_or_else(|| Error::new(path, "snapshot", "missing"))?;
    let mut state = Snapshot::try_from(state).map_err(decode("snapshot"))?;
    let names = (chunks.find_first(METADATA))
        .ok_or_else(|| Error::new(path, "snapshot metadata", "missing"))?;
    let names = snapshot_metadata_names(names).map_err(decode("snapshot metadata"))?;
    let base = base(path, &names, known)?;
    let preset = Preset {
        name: (path.file_stem()).map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
        base: std::path::absolute(&base).unwrap_or(base),
        saved: std::mem::take(&mut state.persistent),
        effects: state.groups.is_some(),
    };
    Ok((preset, state))
}

pub(crate) fn load(path: &Path, known: &[PathBuf]) -> Result<ir::Instrument, Error> {
    let (preset, state) = read(path, known)?;
    let native = Native::of(state).map_err(|e| Error::new(path, "snapshot groups", e))?;
    let (base_chunks, monolith, encrypted) = container::opened(&preset.base)?;
    let mut out = translate::instrument(
        &preset.base,
        &base_chunks,
        monolith,
        &preset.saved,
        encrypted,
        native,
    )?;
    if !preset.name.is_empty() {
        out.name = preset.name;
    }
    Ok(out)
}

/// Kontakt saves a snapshot's group effects in the instrument's group
/// order, one per group: each group of the two counts the other lacks is
/// noted (a base group keeps its own effects, a saved one goes unused).
pub(crate) fn unmatched(saved: usize, groups: usize, notes: &mut kontra_format::Notes) {
    for group in groups..saved {
        notes.add(
            ir::Impact::Audible,
            "snapshot group effects without a group",
            group,
        );
    }
    for group in saved..groups {
        notes.add(
            ir::Impact::Audible,
            "group without snapshot group effects",
            group,
        );
    }
}

/// The name of the instrument a snapshot was saved from. Factory snapshots
/// may keep Kontakt's template name first and the base's second.
fn binding(names: &(String, String)) -> &str {
    if names.0 == "Kontakt" && !names.1.is_empty() {
        &names.1
    } else {
        &names.0
    }
}

/// Whether `path` is an .nki named one of `wanted`, in any case.
fn named(path: &Path, wanted: &[String]) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
        && (path.file_stem()).is_some_and(|s| wanted.iter().any(|w| s.eq_ignore_ascii_case(w)))
}

/// The base instrument: an .nki named for the snapshot's binding or for its
/// folder under `Snapshots/`, in the nearest folder (up to the library) that
/// has one within three levels; else the one of `known` named for its
/// binding or its own folder. Two candidates is an error, not a guess;
/// directory links are not followed.
fn base(path: &Path, names: &(String, String), known: &[PathBuf]) -> Result<PathBuf, Error> {
    let mut wanted = vec![binding(names).to_owned()];
    if let Some(near) = near(path, &mut wanted)? {
        return Ok(near);
    }
    // A snapshot kept apart from its library (Kontakt's user snapshots,
    // `User Content/<instrument>/`) names its instrument.
    wanted.extend(
        (path.parent().and_then(Path::file_name)).map(|s| s.to_string_lossy().into_owned()),
    );
    let mut found: Vec<&PathBuf> = known.iter().filter(|p| named(p, &wanted)).collect();
    found.sort();
    found.dedup();
    match found.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(Error::new(
            path,
            "base instrument",
            format!(
                "no {:?} near it or among the libraries",
                wanted.join(" or ")
            ),
        )),
        many => Err(Error::new(
            path,
            "base instrument",
            format!(
                "ambiguous ({} candidates, e.g. {})",
                many.len(),
                many.iter()
                    .take(3)
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

/// [`base`]'s search near the snapshot, `wanted` gaining its folder under
/// `Snapshots/`. None when nothing is found (or the folders cannot be
/// read); an error for two candidates.
fn near(path: &Path, wanted: &mut Vec<String>) -> Result<Option<PathBuf>, Error> {
    let under = |p: &Path, name: &str| {
        (p.parent().and_then(Path::file_name)).is_some_and(|s| s.eq_ignore_ascii_case(name))
    };
    if let Some(folder) =
        (path.ancestors().find(|p| under(p, "Snapshots"))).and_then(Path::file_name)
    {
        wanted.push(folder.to_string_lossy().into_owned());
    }
    let library = (path.ancestors())
        .find(|p| {
            p.file_name()
                .is_some_and(|s| s.eq_ignore_ascii_case("Snapshots"))
        })
        .and_then(Path::parent);
    let folders: Vec<&Path> = match library {
        Some(library) => (path.ancestors().skip(1))
            .take_while(|d| d.starts_with(library))
            .collect(),
        None => path.ancestors().skip(1).take(4).collect(),
    };
    let mut left = BUDGET;
    for folder in folders {
        let mut found = Vec::new();
        if find(folder, wanted, 3, &mut left, &mut found).is_err() {
            return Ok(None);
        }
        found.sort();
        found.dedup();
        match found.as_slice() {
            [] => {}
            [one] => return Ok(Some(one.clone())),
            many => {
                return Err(Error::new(
                    path,
                    "base instrument",
                    format!(
                        "ambiguous ({} candidates, e.g. {})",
                        many.len(),
                        many.iter()
                            .take(3)
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ));
            }
        }
    }
    Ok(None)
}

fn find(
    dir: &Path,
    wanted: &[String],
    depth: usize,
    left: &mut usize,
    found: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    let mut folders = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        *left = left
            .checked_sub(1)
            .ok_or(std::io::ErrorKind::QuotaExceeded)?;
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            folders.push(path);
        } else if kind.is_file() && named(&path, wanted) {
            found.push(path);
        }
    }
    if depth > 0 {
        folders.sort();
        for dir in folders {
            find(&dir, wanted, depth - 1, left, found)?;
        }
    }
    Ok(())
}

/// The instrument's saved values with a snapshot's over them by variable:
/// a snapshot has no `make_instr_persistent` ones, which keep the base's.
/// Matches variable names case-insensitively (sigil-exact or bare-name, consistent
/// with [`resaved`]).
pub(crate) fn over(own: Vec<String>, snapshot: &[String]) -> Vec<String> {
    fn key(v: &str) -> &str {
        v.split_once(' ').map_or(v, |(n, _)| n)
    }
    fn bare(n: &str) -> &str {
        n.trim_start_matches(['$', '%', '@', '!'])
    }
    let matches = |a: &str, b: &str| {
        let (ka, kb) = (key(a), key(b));
        ka.eq_ignore_ascii_case(kb) || bare(ka).eq_ignore_ascii_case(bare(kb))
    };
    let mine = |v: &String| !snapshot.iter().any(|s| matches(s, v));
    (own.into_iter().filter(mine))
        .chain(snapshot.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_value_takes_the_held_one_by_its_bare_name() {
        let own = [
            "$Level 1".to_owned(),
            "%table 1 2".to_owned(),
            "$gone 3".to_owned(),
        ];
        let now = ["level 7".to_owned(), "$TABLE 4 5".to_owned()];
        assert_eq!(resaved(&own, &now), ["$Level 7", "%table 4 5", "$gone 3"]);
    }

    #[test]
    fn a_saved_value_prefers_its_own_sigil_before_bare_name() {
        let own = ["$Tuning 0".to_owned(), "%Tuning 1 2".to_owned()];
        let now = ["%tuning 3 4".to_owned(), "$tuning 5".to_owned()];
        assert_eq!(resaved(&own, &now), ["$Tuning 5", "%Tuning 3 4"]);
    }

    #[test]
    fn an_empty_saved_text_keeps_its_space_delimiter() {
        let own = ["@label Hello".to_owned(), "@kept ".to_owned()];
        let now = ["@LABEL ".to_owned()];
        assert_eq!(resaved(&own, &now), ["@label ", "@kept "]);
    }

    #[test]
    fn a_snapshot_keeps_the_instruments_values_it_does_not_save() {
        let own = ["$power 1", "$cutoff 10", "@name Init"].map(String::from);
        let snapshot = ["$cutoff 64", "@name Big Foot"].map(String::from);
        let values = over(own.to_vec(), &snapshot);
        assert_eq!(values, ["$power 1", "$cutoff 64", "@name Big Foot"]);
        assert_eq!(over(own.to_vec(), &[]), own);
    }

    #[test]
    fn each_group_a_snapshot_and_its_instrument_do_not_share_is_noted() {
        let noted = |saved, groups| {
            let mut notes = kontra_format::Notes::default();
            unmatched(saved, groups, &mut notes);
            let found = notes.finish();
            found
                .iter()
                .map(|d| format!("{}: {}", d.feature, d.detail))
                .collect::<Vec<_>>()
        };
        assert!(noted(3, 3).is_empty());
        assert_eq!(
            noted(5, 3),
            ["snapshot group effects without a group: 2× (first: 3)"]
        );
        assert_eq!(
            noted(2, 3),
            ["group without snapshot group effects: 1× (first: 2)"]
        );
    }

    #[test]
    fn a_snapshots_group_effects_that_do_not_decode_are_an_error_not_none() {
        let state = |groups: Option<Vec<u8>>| Snapshot {
            groups: groups.map(|data| Chunk { id: 0x50, data }),
            version: 1,
            group_count: 2,
            effect_children: Vec::new(),
            persistent: Vec::new(),
        };
        assert!(matches!(Native::of(state(None)), Ok(None)), "script-only");
        // It says two groups and holds none of them.
        assert!(Native::of(state(Some(2u32.to_le_bytes().to_vec()))).is_err());
    }

    #[test]
    fn a_template_name_binds_by_its_second_name() {
        let named = |a: &str, b: &str| (a.to_owned(), b.to_owned());
        assert_eq!(binding(&named("Kontakt", "Strings")), "Strings");
        assert_eq!(binding(&named("Kontakt", "")), "Kontakt");
        assert_eq!(binding(&named("Piano", "Other")), "Piano");
    }

    #[test]
    fn a_snapshot_finds_its_base_by_its_folder_and_never_guesses() {
        let root = std::env::temp_dir().join(format!("kontra-snapshot-{}", std::process::id()));
        let snap = root.join("Snapshots/Lib/Factory/a.nksn");
        let inst = root.join("Instruments");
        std::fs::create_dir_all(snap.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&inst).unwrap();
        std::fs::write(inst.join("lib.nki"), b"").unwrap();
        let names = ("Kontakt".to_owned(), String::new());
        assert_eq!(base(&snap, &names, &[]).unwrap(), inst.join("lib.nki"));
        std::fs::create_dir_all(root.join("More")).unwrap();
        std::fs::write(root.join("More/Lib.nki"), b"").unwrap();
        assert!(base(&snap, &names, &[]).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_user_snapshot_finds_its_base_among_the_known_by_name() {
        let root =
            std::env::temp_dir().join(format!("kontra-user-snapshot-{}", std::process::id()));
        let snap = root.join("User Content/Strings/Mine.nksn");
        std::fs::create_dir_all(snap.parent().unwrap()).unwrap();
        let known = |names: &[&str]| -> Vec<PathBuf> {
            names
                .iter()
                .map(|n| PathBuf::from(format!("/libs/{n}")))
                .collect()
        };
        let names = ("Strings".to_owned(), String::new());
        let found = base(&snap, &names, &known(&["Piano/Piano.nki", "S/strings.NKI"]));
        assert_eq!(found.unwrap(), PathBuf::from("/libs/S/strings.NKI"));
        // Its folder names it too; two of a name is not a guess.
        let names = ("Kontakt".to_owned(), String::new());
        assert!(base(&snap, &names, &known(&["S/Strings.nki"])).is_ok());
        assert!(base(&snap, &names, &known(&["A/Strings.nki", "B/Strings.nki"])).is_err());
        assert!(base(&snap, &names, &[]).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn over_matches_variable_names_case_insensitively() {
        let own = vec!["$Level 1".to_owned(), "$Keep 2".to_owned()];
        let snap = vec!["$LEVEL 9".to_owned()];
        assert_eq!(over(own, &snap), ["$Keep 2", "$LEVEL 9"]);
    }
}
