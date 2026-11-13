//! Files the player picks for a script, and the files a script saves of its
//! own (a UVI script's presets), all under the instrument's folder in the
//! rack's saves: its source path below the save folder.
//!
//! A pick is a script's file request with no path (KSP `load_array` and
//! `save_array` mode 0, a UVI `browseForFile`). It waits here until the
//! editor answers ([`Rack::picked`]): a name, then the request goes on as
//! one by path, or nothing, and it fails as a cancelled dialog does.

use std::path::{Component, Path, PathBuf};

use kontra_engine::Text;
use kontra_engine::files::{FileReply, FileRequest};

use super::files::{answer, saved};
use super::{Command, Rack};

/// Folders deep and files many a listing or a load walks at most.
const DEPTH: usize = 4;
const FILES: usize = 512;

/// A pick waiting for the editor, with the names already saved of its type.
#[derive(Debug)]
pub(crate) struct Parked {
    pub(crate) part: usize,
    pub(crate) generation: u64,
    pub(crate) request: FileRequest,
    pub(crate) names: Vec<String>,
}

/// What the editor asks the player: save or open a file of a part, among
/// the names saved for its instrument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub part: usize,
    pub save: bool,
    pub names: Vec<String>,
}

/// A request's file type: a UVI pick names its own (`.VHFPreset`); a KSP
/// array is an `.nka`.
fn ext(request: &FileRequest) -> String {
    match request.len {
        0 => request.name.as_str().to_owned(),
        _ => ".nka".to_owned(),
    }
}

/// The instrument's own folder under `saves`.
fn own(saves: Option<&Path>, source: Option<&Path>) -> Option<PathBuf> {
    saved(saves, source?)
}

/// Every file under `dir` (a few folders deep), with its path below it.
fn walk(dir: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut found = Vec::new();
    let mut todo = vec![(dir.to_path_buf(), 0)];
    while let Some((at, depth)) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&at) else {
            continue;
        };
        for entry in entries.flatten().take(FILES) {
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() && depth < DEPTH => todo.push((path, depth + 1)),
                Ok(t) if t.is_file() && found.len() < FILES => {
                    let below = path.strip_prefix(dir).map(Path::to_path_buf);
                    found.extend(below.ok().map(|b| (path, b)));
                }
                _ => {}
            }
        }
    }
    found
}

/// The names saved for `request`'s instrument of its type, sorted.
pub(crate) fn names(
    saves: Option<&Path>,
    source: Option<&Path>,
    request: &FileRequest,
) -> Vec<String> {
    let ext = ext(request).to_lowercase();
    let Some(dir) = own(saves, source) else {
        return Vec::new();
    };
    let mut names: Vec<String> = walk(&dir)
        .into_iter()
        .filter_map(|(_, below)| {
            let file = below.file_name()?.to_str()?;
            let cut = file.len().checked_sub(ext.len())?;
            let stem = file
                .get(..cut)
                .filter(|_| file.to_lowercase().ends_with(&ext))?;
            Some(stem.to_owned())
        })
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

/// The files a script of the instrument at `source` saved, as the data
/// files it reads by bank path (`__file/` and the path, lower case).
pub(crate) fn script_files(saves: Option<&Path>, source: Option<&Path>) -> Vec<(String, Vec<u8>)> {
    let Some(dir) = own(saves, source) else {
        return Vec::new();
    };
    walk(&dir)
        .into_iter()
        .filter(|(path, _)| {
            !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("nka"))
        })
        .filter_map(|(path, below)| {
            let parts: Vec<&str> = below.iter().filter_map(|p| p.to_str()).collect();
            let name = format!("{}{}", kontra_lua::FILE, parts.join("/").to_lowercase());
            Some((name, super::files::bounded(&path)?))
        })
        .collect()
}

/// Writes `files` (bank path, contents) a script of the instrument at
/// `source` saved; what could not be written is reported.
pub(crate) fn write_script_files(
    saves: Option<&Path>,
    source: Option<&Path>,
    files: Vec<(String, String)>,
) -> Vec<String> {
    let mut failed = Vec::new();
    for (path, text) in files {
        let at = own(saves, source).and_then(|dir| saved(Some(&dir), Path::new(&path)));
        let made = at.as_ref().is_some_and(|at| {
            at.parent()
                .is_some_and(|d| std::fs::create_dir_all(d).is_ok())
                && std::fs::write(at, &text).is_ok()
        });
        if !made {
            failed.push(format!("preset: could not save the script's {path}"));
        }
    }
    failed
}

/// A name the player typed, as a file name: no folders, no dots first.
fn file_name(name: &str) -> Option<String> {
    let name = Path::new(name.trim()).components().next_back()?;
    let Component::Normal(name) = name else {
        return None;
    };
    let name = name.to_str()?.trim_start_matches('.');
    (!name.is_empty()).then(|| name.to_owned())
}

impl Rack {
    /// The pick the editor asks first, if any.
    #[must_use]
    pub fn pick(&self) -> Option<Pick> {
        self.picks.front().map(|p| Pick {
            part: p.part,
            save: p.request.save,
            names: p.names.clone(),
        })
    }

    /// The player's answer to [`Rack::pick`]: a name, or none to cancel.
    pub fn picked(&mut self, name: Option<&str>) {
        let Some(Parked {
            part,
            generation,
            mut request,
            ..
        }) = self.picks.pop_front()
        else {
            return;
        };
        let live = self.parts.get(part).and_then(Option::as_ref);
        let Some(p) = live.filter(|p| p.script.generation == generation) else {
            return;
        };
        let name = name.and_then(file_name);
        let path = name.map(|n| match request.len {
            // A UVI script gets the name back and saves or loads itself.
            0 => format!("{n}{}", ext(&request)),
            // A KSP array goes on as one by path, under the instrument.
            _ => {
                let source = p.source.as_deref().unwrap_or(Path::new(""));
                source
                    .join(format!("{n}.nka"))
                    .to_string_lossy()
                    .into_owned()
            }
        });
        let reply = match path.map(|path| Text::from_str_cut(&path)) {
            Some((path, false)) => {
                request.path = path;
                match request.len {
                    0 => FileReply {
                        request,
                        loaded: Some((Vec::new(), Vec::new())),
                        ints: Vec::new(),
                        texts: Vec::new(),
                        written: None,
                    },
                    _ => answer(self.saves.as_deref(), request),
                }
            }
            // Cancelled, or a path too long to name: the request fails.
            _ => FileReply {
                request,
                loaded: None,
                ints: Vec::new(),
                texts: Vec::new(),
                written: request.save.then_some(false),
            },
        };
        self.send(Command::File {
            part,
            generation,
            reply: Box::new(reply),
        });
    }
}
