//! Array files scripts load or save by path after `on init`
//! (`load_array_str`, `save_array_str`): the engine queues the request, the
//! rack reads or writes the file here, off the audio thread, and answers.
//!
//! A save never writes into a library: saved arrays go to the user's data
//! folder under the path the script named, and a later load of that path
//! reads the saved file first.

use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use kontra_engine::Text;
use kontra_engine::files::{self, ArrayKind, FileReply, FileRequest};

use super::{Command, Rack};

/// Reads the array files a script names: what it saved, else the library's.
#[derive(Debug)]
pub(crate) struct DiskFiles;

impl files::Files for DiskFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        read(path)
    }
}

/// Only `.nka` files: a script names nothing else to read or write.
fn nka(path: &str) -> Option<&Path> {
    let path = Path::new(path);
    let ext = path.extension()?;
    ext.eq_ignore_ascii_case("nka").then_some(path)
}

/// Where a save of `path` goes.
fn saved(path: &Path) -> Option<PathBuf> {
    let parts = path.components().filter_map(|c| match c {
        Component::Normal(part) => Some(part),
        _ => None,
    });
    let mut at = dirs::data_dir()?.join("kontra/arrays");
    at.extend(parts);
    Some(at)
}

fn read(path: &str) -> Option<Vec<u8>> {
    let path = nka(path)?;
    saved(path)
        .and_then(|s| bounded(&s))
        .or_else(|| bounded(path))
}

/// The bytes of a regular file no larger than any resource a script loads
/// ([`kontra_format_kontakt::RESOURCE_LIMIT`]); nothing for a larger file,
/// a device or a pipe, so a script cannot make the host read without end.
fn bounded(path: &Path) -> Option<Vec<u8>> {
    const LIMIT: u64 = kontra_format_kontakt::RESOURCE_LIMIT;
    if !std::fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    let file = std::fs::File::open(path).ok()?;
    file.take(LIMIT + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= LIMIT).then_some(bytes)
}

/// Writes a save's values; whether it could.
fn write(reply: &FileReply) -> bool {
    let r = reply.request;
    let Some(at) = nka(r.path.as_str()).and_then(saved) else {
        return false;
    };
    let text = files::write(r.name.as_str(), r.kind, &reply.ints, &reply.texts);
    let made = at
        .parent()
        .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok());
    made && std::fs::write(at, text).is_ok()
}

impl Rack {
    /// Writes the saves that came back, and answers the parts' file
    /// requests: a load's values, read here, or room for a save's.
    pub(super) fn files(&mut self) {
        // Saves first: a load the same block asked for reads them. A save
        // goes back once written, for its script to hear how it went.
        while let Some((part, generation, mut reply)) = self.channels.filed.recv() {
            // A save of a part since replaced is not written: its values are
            // the old load's.
            let live = self.parts.get(part).and_then(Option::as_ref);
            let live = live.is_some_and(|p| p.script.generation == generation);
            if reply.request.save && reply.written.is_none() && live {
                reply.written = Some(write(&reply));
                self.send(Command::File {
                    part,
                    generation,
                    reply,
                });
            }
        }
        while let Some((part, generation, request)) = self.channels.files.recv() {
            let live = self.parts.get(part).and_then(Option::as_ref);
            if live.is_none_or(|p| p.script.generation != generation) {
                continue;
            }
            let reply = Box::new(answer(request));
            self.send(Command::File {
                part,
                generation,
                reply,
            });
        }
    }
}

/// A load's values, filled from the file, or a save's reserved room.
fn answer(request: FileRequest) -> FileReply {
    let len = request.len as usize;
    let text = request.kind == ArrayKind::Text;
    let mut reply = FileReply {
        request,
        loaded: None,
        ints: Vec::with_capacity(if request.save && !text { len } else { 0 }),
        texts: Vec::with_capacity(if request.save && text { len } else { 0 }),
        written: None,
    };
    if request.save {
        return reply;
    }
    let mut ints = vec![0; if text { 0 } else { len }];
    let mut texts = vec![Text::EMPTY; if text { len } else { 0 }];
    let bytes = read(request.path.as_str());
    let name = request.name.as_str();
    let filled =
        bytes.and_then(|b| files::fill(&b, name, (0, len), request.kind, &mut ints, &mut texts));
    reply.loaded = filled.map(|n| {
        ints.truncate(n);
        texts.truncate(n);
        (ints, texts)
    });
    reply
}
