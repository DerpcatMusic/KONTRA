//! Array files scripts load or save by path after `on init`
//! (`load_array_str`, `save_array_str`): the engine queues the request, the
//! rack reads or writes the file here, off the audio thread, and answers.
//!
//! A save never writes into a library: saved arrays go under the rack's
//! save folder ([`Rack::set_saves`]; the plugin's is [`user_saves`]) at the
//! path the script named, and a later load of that path reads the saved
//! file first. A rack with no save folder (a test, a tool) writes nothing:
//! its saves report failure.

use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use kontra_engine::Text;
use kontra_engine::files::{self, ArrayKind, FileReply, FileRequest};

use super::{Command, Rack};

/// Reads the array files a script names: what it saved under `saves`,
/// else the library's, relative to its resource container folder `rc`
/// without case ([`read`]).
#[derive(Debug, Default)]
pub(crate) struct DiskFiles {
    pub(crate) saves: Option<PathBuf>,
    pub(crate) rc: Option<Text>,
}

impl files::Files for DiskFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        read(
            self.saves.as_deref(),
            self.rc.as_ref().map(Text::as_str),
            path,
        )
    }
}

/// The user's own save folder for scripts' arrays, for the plugin's rack.
#[must_use]
pub fn user_saves() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("kontra/arrays"))
}

/// Only array (`.nka`) and MIDI (`.mid`, `.midi`) files: a script names
/// nothing else to read or write.
fn nka(path: &str) -> Option<&Path> {
    let path = Path::new(path);
    let ext = path.extension()?;
    ["nka", "mid", "midi"]
        .iter()
        .any(|e| ext.eq_ignore_ascii_case(e))
        .then_some(path)
}

/// Where a save of `path` goes under `saves`.
pub(super) fn saved(saves: Option<&Path>, path: &Path) -> Option<PathBuf> {
    let parts = path.components().filter_map(|c| match c {
        Component::Normal(part) => Some(part),
        _ => None,
    });
    let mut at = saves?.to_path_buf();
    at.extend(parts);
    Some(at)
}

/// What a script reads at `path`: its save under `saves`, else the file.
/// Relative to the script's resource container folder `rc` (and its
/// library, the folder above), names match without case: Kontakt's Chords
/// tool asks for `../Library Data/Midi Files/` and ships `MIDI Files/`.
fn read(saves: Option<&Path>, rc: Option<&str>, path: &str) -> Option<Vec<u8>> {
    let path = nka(path)?;
    saved(saves, path)
        .and_then(|s| bounded(&s))
        .or_else(|| bounded(path))
        .or_else(|| {
            let rc = Path::new(rc?);
            let rest = path.strip_prefix(rc).ok()?.to_str()?;
            let library = rc.parent().unwrap_or(rc);
            let found =
                kontra_format::paths::resolve_case_insensitive_bounded(rc, library, rest, None)?;
            bounded(&found)
        })
}

/// The bytes of a regular file no larger than any resource a script loads
/// ([`kontra_format_kontakt::RESOURCE_LIMIT`]); nothing for a larger file,
/// a device or a pipe, so a script cannot make the host read without end.
pub(super) fn bounded(path: &Path) -> Option<Vec<u8>> {
    const LIMIT: u64 = kontra_format_kontakt::RESOURCE_LIMIT;
    if !std::fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    let file = std::fs::File::open(path).ok()?;
    file.take(LIMIT + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= LIMIT).then_some(bytes)
}

/// Writes a save's values under `saves`; whether it could.
fn write(saves: Option<&Path>, reply: &FileReply) -> bool {
    let r = reply.request;
    let Some(at) = nka(r.path.as_str()).and_then(|p| saved(saves, p)) else {
        return false;
    };
    let bytes = match (&reply.midi, r.midi) {
        (Some(smf), Some(_)) => kontra_engine::midi_file::write(smf),
        (_, Some(_)) => None,
        _ => Some(files::write(r.name.as_str(), r.kind, &reply.ints, &reply.texts).into_bytes()),
    };
    let made = at
        .parent()
        .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok());
    made && bytes.is_some_and(|b| std::fs::write(at, b).is_ok())
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
                reply.written = Some(write(self.saves.as_deref(), &reply));
                self.send(Command::File {
                    part,
                    generation,
                    reply,
                });
            }
        }
        while let Some((part, generation, request)) = self.channels.files.recv() {
            let live = self.parts.get_mut(part).and_then(Option::as_mut);
            let Some(p) = live.filter(|p| p.script.generation == generation) else {
                continue;
            };
            // fs_navigate: the file listed before or after the selector's,
            // picked as the player picks one, which runs its callback; at
            // either end the pick stays.
            if let Some(go) = request.navigate {
                let ext = kontra_ui_model::Files::of_type(go.files).extensions();
                let to = kontra_library::selector::step(request.path.as_str(), ext, go.next);
                let text = to.map_or(request.path, |p| Text::from_str_cut(&p.to_string_lossy()).0);
                self.send(Command::ControlText {
                    part,
                    generation,
                    ui_seq: 0,
                    slot: request.slot,
                    id: go.control,
                    cell: None,
                    text: Box::new(text),
                });
                continue;
            }
            // A detection reads the zone's own sample, not a named file.
            if let Some(job) = request.detect {
                let sample = (p.plan.as_ref())
                    .and_then(|plan| plan.zone_sample(job.zone))
                    .cloned();
                let detected = sample.and_then(|s| kontra_engine::detect_sample(&s, job.what));
                let reply = Box::new(FileReply {
                    request,
                    loaded: None,
                    ints: Vec::new(),
                    texts: Vec::new(),
                    written: None,
                    midi: None,
                    detected,
                });
                self.send(Command::File {
                    part,
                    generation,
                    reply,
                });
                continue;
            }
            // No path: the player picks the file in the editor first.
            if request.path.as_str().is_empty() {
                let names =
                    super::picks::names(self.saves.as_deref(), p.source.as_deref(), &request);
                self.picks.push_back(super::picks::Parked {
                    part,
                    generation,
                    request,
                    names,
                });
                continue;
            }
            let rc = (p.plan.as_ref())
                .and_then(|plan| plan.scripts.get(usize::from(request.slot)))
                .and_then(|s| s.rc_folder);
            let rc = rc.as_ref().map(Text::as_str);
            let reply = Box::new(answer(self.saves.as_deref(), rc, request));
            // Only a load that read its file names a preset.
            if reply.loaded.is_some() {
                p.named.loaded(request.path.as_str());
            }
            self.send(Command::File {
                part,
                generation,
                reply,
            });
        }
    }
}

/// A load's values, filled from the file ([`read`]), or a save's reserved room.
pub(super) fn answer(saves: Option<&Path>, rc: Option<&str>, request: FileRequest) -> FileReply {
    let len = request.len as usize;
    let text = request.kind == ArrayKind::Text;
    let (save, midi) = (request.save, request.midi.is_some());
    let mut reply = FileReply {
        request,
        loaded: None,
        ints: Vec::with_capacity(if save && !midi && !text { len } else { 0 }),
        texts: Vec::with_capacity(if save && !midi && text { len } else { 0 }),
        written: None,
        // A MIDI save's room: the engine copies its events in without
        // allocating on the audio thread.
        midi: (save && midi).then(|| kontra_engine::midi_file::Smf {
            events: Vec::with_capacity(len),
            division: 0,
            tracks: 0,
            area: kontra_engine::midi_file::Area::WHOLE,
        }),
        detected: None,
    };
    if request.save {
        return reply;
    }
    if request.midi.is_some() {
        let smf =
            read(saves, rc, request.path.as_str()).and_then(|b| kontra_engine::midi_file::read(&b));
        reply.loaded = smf.as_ref().map(|_| (Vec::new(), Vec::new()));
        reply.midi = smf;
        return reply;
    }
    let mut ints = vec![0; if text { 0 } else { len }];
    let mut texts = vec![Text::EMPTY; if text { len } else { 0 }];
    let bytes = read(saves, rc, request.path.as_str());
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

#[cfg(test)]
mod tests {
    /// Kontakt's Chords tool asks, from its container folder, for
    /// `../Library Data/Midi Files/…` and ships `MIDI Files/`: read without
    /// case, but only up to the container's library (an exact path reads
    /// as before).
    #[test]
    fn a_read_relative_to_the_container_folder_ignores_case() {
        let lib = std::env::temp_dir().join(format!("kontra-rc-read-{}", std::process::id()));
        let samples = lib.join("Chords/Samples");
        std::fs::create_dir_all(&samples).unwrap();
        std::fs::create_dir_all(lib.join("Chords/MIDI Files")).unwrap();
        std::fs::write(lib.join("Chords/MIDI Files/a.mid"), b"MThd").unwrap();
        std::fs::write(lib.join("B.mid"), b"MThd").unwrap();
        let rc = format!("{}/", samples.display());
        let asked = |rest: &str| super::read(None, Some(&rc), &format!("{rc}{rest}"));
        let found = asked("../Midi Files/a.mid");
        // The resolver itself, past the exact-path read a case-blind file
        // system (macOS, Windows) would answer.
        let above = kontra_format::paths::resolve_case_insensitive_bounded(
            &samples,
            &lib.join("Chords"),
            "../../b.mid",
            None,
        );
        let without = super::read(None, None, &format!("{rc}../Midi Files/a.mid"));
        let case_blind = lib.join("chords/midi files/a.mid").exists();
        let _ = std::fs::remove_dir_all(&lib);
        assert_eq!(found.as_deref(), Some(&b"MThd"[..]));
        assert_eq!(above, None);
        // Without a container folder only the exact path reads.
        assert!(case_blind || without.is_none());
    }
}
