//! Array files scripts load or save by path after `on init`
//! (`load_array_str`, `save_array_str`): the engine queues the request, the
//! rack reads or writes the file here, off the audio thread, and answers.
//!
//! A save never writes into a library: saved arrays go to the user's data
//! folder under the path the script named, and a later load of that path
//! reads the saved file first.

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
    let saved = saved(path).and_then(|s| std::fs::read(s).ok());
    saved.or_else(|| std::fs::read(path).ok())
}

impl Rack {
    /// Writes the saves that came back, and answers the parts' file
    /// requests: a load's values, read here, or room for a save's.
    pub(super) fn files(&mut self) {
        // Saves first: a load the same block asked for reads them.
        while let Some(reply) = self.channels.filed.recv() {
            let r = reply.request;
            let full = !reply.ints.is_empty() || !reply.texts.is_empty();
            let at = nka(r.path.as_str()).and_then(saved);
            if let Some(at) = at.filter(|_| r.save && full) {
                let text = files::write(r.name.as_str(), r.kind, &reply.ints, &reply.texts);
                // ponytail: the save already completed as done on the audio
                // side; a failed write is lost. Report it back when a script
                // shows it reads the status of a save.
                let _ = at.parent().map(std::fs::create_dir_all);
                let _ = std::fs::write(at, text);
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
