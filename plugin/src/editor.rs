//! The editor: [`kontra_ui::app::view`] in a moose window. It draws from a
//! [`Snapshot`] of the rack that [`crate::Tick`] refreshes, so a frame never
//! waits on a load, and it hands what the user does back as [`Edit`]s for
//! the tick to carry out.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};

use kontra_host::browser::Browser;
use kontra_host::rack::{Event, HostNote, HostPattern, Mix, Rack};
use kontra_host::state::SavedView;
use kontra_host::ui::Library;
use kontra_protocol::Stats;
use kontra_ui::app::{self, Action, App, Face, Part};
use kontra_ui::{Pictures, layout};
use kontra_ui_model::Interface;
use moose::mui::MuiEditor;
use moose::prelude::*;

use crate::readout::Readout;
use crate::{KontraParams, P, Request, lock};

/// Report lines kept; older ones scroll away.
const REPORT: usize = 200;

/// One rack part as the editor shows it.
#[derive(Clone, Debug)]
struct PartView {
    slot: usize,
    /// Which load of the part this is.
    generation: u64,
    path: Option<PathBuf>,
    /// Its library's interface; copied when the part loads and whenever
    /// its script changes it ([`kontra_host::view::View::revision`]).
    tree: Option<Arc<Interface>>,
    revision: u64,
    /// Each group's sound, copied when an edit moves it
    /// ([`kontra_host::sound::Sounds::revision`]).
    sounds: (u64, Arc<Vec<app::Sound>>),
    name: String,
    /// Its library's name in the browser, if the browser lists it.
    library: String,
    mix: Mix,
    solo: bool,
    mute: bool,
    /// Its output's peaks, then each instrument bus's.
    level: Vec<[f32; 2]>,
}

/// The rack as the editor last saw it.
#[derive(Debug, Default)]
pub struct Snapshot {
    parts: Vec<PartView>,
    /// The browser as lane D's app lists it, and which change of it.
    libraries: (u64, Arc<Vec<app::Library>>),
    recent: Vec<(usize, usize)>,
    folders: Vec<String>,
    /// [`kontra_library::Prefs::next_channel`] and `own_output`.
    new_parts: (bool, bool),
    level: f32,
    stats: Stats,
    readout: Readout,
    report: Vec<String>,
}

impl Snapshot {
    /// Copies what the editor shows from `rack`, with this tick's `report`;
    /// whether any of it changed, so an idle editor draws no frames.
    pub fn refresh(&mut self, rack: &Rack, browser: &Browser, report: Vec<String>) -> bool {
        let before = self.shown();
        let reported = !report.is_empty();
        let rescanned = self.libraries.0 != browser.generation;
        if rescanned {
            self.libraries = (browser.generation, Arc::new(libraries(browser)));
            self.recent = browser.recent();
            self.folders = (browser.prefs.roots.iter())
                .map(|r| r.path.display().to_string())
                .collect();
            self.new_parts = (browser.prefs.next_channel, browser.prefs.own_output);
        }
        let meters = rack.meters();
        let level = |slot: usize| {
            let out = meters.parts.get(slot).copied().unwrap_or_default();
            let buses = meters.buses.get(slot).into_iter().flatten().copied();
            std::iter::once(out).chain(buses).collect()
        };
        let old = std::mem::take(&mut self.parts);
        self.parts = (rack.parts().iter().enumerate())
            .filter_map(|(slot, p)| {
                let p = p.as_ref()?;
                let generation = p.script.generation();
                let same = (old.iter()).find(|v| v.slot == slot && v.generation == generation);
                let revision = p.view.as_ref().map_or(0, |v| v.revision);
                let tree = match same {
                    Some(v) if v.revision == revision => v.tree.clone(),
                    _ => p.view.as_ref().map(|v| Arc::new(v.tree().clone())),
                };
                let sounds = match same {
                    Some(v) if v.sounds.0 == p.sounds.revision => v.sounds.clone(),
                    _ => (p.sounds.revision, Arc::new(sounds(&p.sounds))),
                };
                let library = match same {
                    Some(v) if !rescanned => v.library.clone(),
                    _ => (p.path.as_deref())
                        .and_then(|path| browser.find(path))
                        .and_then(|(l, _)| browser.catalog.libraries.get(l))
                        .map(|l| l.name.clone())
                        .unwrap_or_default(),
                };
                Some(PartView {
                    slot,
                    generation,
                    path: p.path.clone(),
                    tree,
                    revision,
                    sounds,
                    name: p.name.clone(),
                    library,
                    mix: p.mix,
                    solo: p.solo,
                    mute: p.mute,
                    level: level(slot),
                })
            })
            .collect();
        self.level = meters.master[0].max(meters.master[1]);
        if let Some(s) = rack.stats() {
            self.stats = s;
        }
        self.readout.sample(&self.stats, rack);
        self.report.extend(report);
        let over = self.report.len().saturating_sub(REPORT);
        self.report.drain(..over);
        reported || self.shown() != before
    }

    /// What a frame draws of this, as it reads on screen: the CPU load in
    /// whole percent; a part's interface by its load and revision.
    fn shown(&self) -> impl PartialEq + use<> {
        let parts: Vec<_> = (self.parts.iter())
            .map(|p| {
                let mix = (
                    p.mix,
                    p.solo,
                    p.mute,
                    p.level
                        .iter()
                        .map(|l| l.map(f32::to_bits))
                        .collect::<Vec<_>>(),
                );
                (
                    p.slot,
                    p.generation,
                    p.revision,
                    p.name.clone(),
                    p.library.clone(),
                    mix,
                )
            })
            .collect();
        (
            self.libraries.0,
            parts,
            self.level.to_bits(),
            self.readout.shown(),
            self.stats.voices,
        )
    }

    /// Fills `app` for a frame, keeping each part's own view state; `shown`
    /// is which load of each part the editor's faces are of.
    fn fill(&self, app: &mut App, shown: &mut Shown) {
        if shown.browser != self.libraries.0 {
            let fresh = self.libraries.1.as_ref();
            // A load or a star changes the list, not its art: the covers
            // decoded so far stay (they are keyed by library and art).
            let same = (app.libraries.iter().map(|l| &l.art)).eq(fresh.iter().map(|l| &l.art));
            if app.covers.is_none() || !same {
                app.covers = Some(Pictures::new(Box::new(cover)));
                app.banners = Some(Pictures::with_look(
                    Box::new(cover),
                    kontra_ui::artwork::banner,
                ));
            }
            app.libraries = fresh.clone();
            app.recent.clone_from(&self.recent);
            app.folders.clone_from(&self.folders);
            (app.next_channel, app.own_output) = self.new_parts;
            shown.browser = self.libraries.0;
        }
        for pictures in [&mut app.covers, &mut app.banners].into_iter().flatten() {
            pictures.poll();
        }
        let Shown {
            parts: shown,
            sounds: sounds_shown,
            ..
        } = shown;
        let heard = std::mem::take(sounds_shown);
        let mut old = std::mem::take(&mut app.parts);
        let parts = self.parts.iter().enumerate().map(|(n, p)| {
            let mut old = old.get_mut(n);
            let (open, tab, look) = old
                .as_ref()
                .map_or((true, Default::default(), None), |old| {
                    (old.open, old.tab, old.look)
                });
            let now = (p.generation, p.revision);
            let sound_now = (p.generation, p.sounds.0);
            // The editor's own edit shows at once; the rack's copy once it moved.
            let sounds = match old.as_mut() {
                Some(old) if heard.get(n) == Some(&sound_now) => std::mem::take(&mut old.sounds),
                _ => p.sounds.1.as_ref().clone(),
            };
            let face = match old {
                Some(old) if shown.get(n) == Some(&now) => old.face.take(),
                // The script changed its look: a new tree, the same pictures.
                Some(old) if shown.get(n).is_some_and(|s| s.0 == now.0) => {
                    let tree = p.tree.as_deref();
                    old.face.take().zip(tree).map(|(mut f, tree)| {
                        f.tree = resolved(tree);
                        f
                    })
                }
                _ => p.tree.as_deref().map(|tree| face(tree, p.path.clone())),
            };
            Part {
                face,
                name: p.name.clone(),
                library: p.library.clone(),
                channel: p.mix.channel,
                output: p.mix.output + 1,
                volume: f64::from(linear_to_db(p.mix.gain)),
                pan: f64::from(p.mix.pan),
                level: p.level.iter().map(|l| l.map(f64::from)).collect(),
                solo: p.solo,
                mute: p.mute,
                keys: (0, 127),
                open,
                tab,
                look,
                sounds,
                ..Part::default()
            }
        });
        app.parts = parts.collect();
        // What the picture workers decoded since the last frame.
        for face in app.parts.iter_mut().filter_map(|p| p.face.as_mut()) {
            face.pictures.poll();
        }
        *shown = self
            .parts
            .iter()
            .map(|p| (p.generation, p.revision))
            .collect();
        let heard = self.parts.iter().map(|p| (p.generation, p.sounds.0));
        sounds_shown.extend(heard);
        app.level = f64::from(self.level);
        let r = &self.readout;
        app.stats = app::Stats {
            cpu: r.cpu,
            voices: self.stats.voices,
            ram_mb: r.process_mb.unwrap_or(0),
            samples_mb: r.samples_mb,
            disk_mbs: r.disk_mbs,
            cache_mb: r.cache_mb,
        };
        app.report.clone_from(&self.report);
    }
}

/// Which change of the rack's parts and of the browser the editor shows.
#[derive(Default)]
struct Shown {
    /// By part: its load and its interface's revision.
    parts: Vec<(u64, u64)>,
    /// By part: its load and its sound's revision.
    sounds: Vec<(u64, u64)>,
    browser: u64,
}

/// The catalog as the browser lists it: presets by path inside their
/// library, without the extension.
fn libraries(browser: &Browser) -> Vec<app::Library> {
    (browser.catalog.libraries.iter())
        .map(|l| app::Library {
            name: l.name.clone(),
            format: match l.format {
                kontra_library::Format::Kontakt => app::Format::Kontakt,
                kontra_library::Format::Uvi => app::Format::Uvi,
            },
            presets: (l.presets.iter())
                .map(|p| {
                    let p = p.strip_prefix(&l.dir).unwrap_or(p).with_extension("");
                    p.to_string_lossy().into_owned()
                })
                .collect(),
            starred: browser.prefs.starred(&l.dir),
            art: art(&l.art),
        })
        .collect()
}

/// Pictures the library holds inside a resource container, best first:
/// its library-browser banner (stored with the dot), then its artwork.
const HELD: [&str; 2] = [".LibBrowser.png", "MST_artwork.png"];

/// The cover keys of a library's ranked pictures and containers: a PNG's
/// path, or a container's path, a NUL, and the picture's name inside it.
/// Any container reads the whole library's, so only the first is asked.
pub(crate) fn art(paths: &[PathBuf]) -> Vec<String> {
    let mut keys = Vec::new();
    let mut held = false;
    for path in paths {
        let ext = path.extension().map(|e| e.to_ascii_lowercase());
        let name = path.to_string_lossy();
        match ext.as_ref().and_then(|e| e.to_str()) {
            Some("png" | "jpg" | "jpeg") => keys.push(name.into_owned()),
            Some("nicnt" | "nkr") if !held => {
                held = true;
                keys.extend(HELD.iter().map(|n| format!("{name}\0{n}")));
            }
            _ => {}
        }
    }
    keys
}

/// The bytes of the cover picture `key` names (see [`art`]).
pub(crate) fn cover(key: &str) -> Option<Vec<u8>> {
    match key.split_once('\0') {
        Some((container, name)) => Library::open(Path::new(container)).read(name),
        None => {
            let file = std::fs::File::open(key).ok()?;
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut std::io::Read::take(file, COVER_BYTES), &mut bytes)
                .ok()?;
            Some(bytes)
        }
    }
}

/// Places a part the browser added where the player's prefs say: the
/// lowest MIDI channel no other part has, its own output pair.
pub fn place(rack: &mut Rack, browser: &Browser, part: usize) {
    let parts = rack.parts();
    let Some(mut mix) = parts.get(part).and_then(Option::as_ref).map(|p| p.mix) else {
        return;
    };
    if browser.prefs.next_channel {
        let taken = |c: &u8| {
            (parts.iter().enumerate())
                .any(|(n, p)| n != part && p.as_ref().is_some_and(|p| p.mix.channel == *c))
        };
        mix.channel = (1..=16).find(|c| !taken(c)).unwrap_or(0);
    }
    if browser.prefs.own_output {
        mix.output = u8::try_from(part.min(crate::OUTPUTS - 1)).unwrap_or(0);
    }
    rack.set_mix(part, mix);
}

/// The largest cover file read.
const COVER_BYTES: u64 = 32 << 20;

/// Carries out a browser action: a preset loads into the first free part.
pub fn browse(rack: &mut Rack, browser: &mut Browser, action: Action, report: &mut Vec<String>) {
    match action {
        Action::Load { library, preset } => {
            let Some(path) = browser.preset(library, preset) else {
                return;
            };
            let loaded = match rack.free() {
                Some(part) => rack.load(part, &path).map(|()| place(rack, browser, part)),
                None => Err("every part is in use".into()),
            };
            match loaded {
                Ok(()) => browser.loaded(&path),
                Err(e) => report.push(format!("{}: {e}", path.display())),
            }
        }
        // Replaces the part's instrument with another of its library's.
        Action::Switch { part, preset } => {
            let library = (rack.parts().get(part).and_then(Option::as_ref))
                .and_then(|p| browser.find(p.path.as_deref()?));
            let Some(path) = library.and_then(|(l, _)| browser.preset(l, preset)) else {
                return;
            };
            match rack.load(part, &path) {
                Ok(()) => browser.loaded(&path),
                Err(e) => report.push(format!("{}: {e}", path.display())),
            }
        }
        Action::Replace {
            part,
            library,
            preset,
        } => {
            let Some(path) = browser.preset(library, preset) else {
                return;
            };
            match rack.load(part, &path) {
                Ok(()) => browser.loaded(&path),
                Err(e) => report.push(format!("{}: {e}", path.display())),
            }
        }
        Action::Step { part, by } => {
            let found = (rack.parts().get(part).and_then(Option::as_ref))
                .and_then(|p| browser.find(p.path.as_deref()?));
            let Some((library, preset)) = found else {
                return;
            };
            let count = browser
                .catalog
                .libraries
                .get(library)
                .map_or(0, |l| l.presets.len());
            if count == 0 {
                return;
            }
            let step = by.unsigned_abs() as usize % count;
            // A full turn (or a one-preset library) stays: no reload.
            if step == 0 {
                return;
            }
            let preset = if by < 0 {
                preset + count - step
            } else {
                preset + step
            } % count;
            let switch = Action::Switch { part, preset };
            browse(rack, browser, switch, report);
        }
        Action::Star(library, on) => browser.star(library, on),
        Action::Refresh => browser.rescan(),
        Action::RemoveFolder(n) => browser.remove_root(n),
        Action::NewParts {
            next_channel,
            own_output,
        } => browser.new_parts(next_channel, own_output),
        _ => {}
    }
}

/// A library's interface for the editor, laid out, its pictures read from
/// the library at `path` off the UI thread.
fn face(tree: &Interface, path: Option<PathBuf>) -> Face {
    let tree = resolved(tree);
    let library = Mutex::new(None);
    let pictures = Pictures::new(Box::new(move |p: &str| {
        let path = path.as_deref()?;
        let mut library = library.lock().ok()?;
        library.get_or_insert_with(|| Library::open(path)).read(p)
    }));
    Face {
        tree,
        pictures,
        gesture: Default::default(),
    }
}

/// `tree` laid out.
fn resolved(tree: &Interface) -> Interface {
    let mut tree = tree.clone();
    let all = 0..tree.widgets.len();
    layout::resolve(&mut tree, all);
    tree
}

/// A user's edit to one rack part, or to the whole rack.
pub struct Edit {
    pub(crate) slot: Option<usize>,
    pub(crate) action: Action,
}

/// Carries out `edit` on the rack.
pub fn apply(rack: &mut Rack, edit: Edit) {
    let part = edit
        .slot
        .and_then(|s| Some((s, rack.parts().get(s)?.as_ref()?)));
    let mix = |(slot, p): (usize, &kontra_host::rack::Part)| (slot, p.mix, p.solo, p.mute);
    let Some((slot, mut m, solo, mute)) = part.map(mix) else {
        if edit.action == Action::Panic {
            rack.panic();
        }
        return;
    };
    match edit.action {
        Action::Volume(_, db) => m.gain = db_to_linear(db as f32),
        Action::Pan(_, pan) => m.pan = pan as f32,
        Action::Channel(_, c) => m.channel = c.min(16),
        // Output 0 lets the host pick, which is the first pair.
        Action::Output(_, o) => m.output = o.saturating_sub(1),
        Action::Solo(_, on) => return rack.set_solo_mute(slot, on, mute),
        Action::Mute(_, on) => return rack.set_solo_mute(slot, solo, on),
        Action::Remove(_) => return rack.remove(slot),
        Action::Control {
            widget,
            cell,
            value,
            keys,
            ..
        } => return rack.control(slot, widget, cell, value, keys),
        Action::ControlText { widget, text, .. } => return rack.control_text(slot, widget, &text),
        Action::Sound {
            group,
            param,
            value,
            ..
        } => return rack.set_sound(slot, group, param_of(param), value),
        _ => return,
    }
    rack.set_mix(slot, m);
}

/// Each group's sound as the Sound tab draws it.
fn sounds(s: &kontra_host::sound::Sounds) -> Vec<app::Sound> {
    use kontra_host::sound::BandKind;
    let filter = |b: kontra_host::sound::Band| app::Filter {
        pass: match b.kind {
            BandKind::Low => app::Pass::Low,
            BandKind::High => app::Pass::High,
            BandKind::Band => app::Pass::Band,
            BandKind::Notch => app::Pass::Notch,
            BandKind::Peak => app::Pass::Peak,
            BandKind::LowShelf => app::Pass::LowShelf,
            BandKind::HighShelf => app::Pass::HighShelf,
        },
        hz: b.hz,
        q: b.q,
        gain_db: b.gain_db,
        poles: b.poles,
    };
    (0..s.len())
        .map(|g| app::Sound {
            envelope: s
                .envelope(g)
                .map(|[attack, hold, decay, sustain, release]| app::Envelope {
                    attack,
                    hold,
                    decay,
                    sustain,
                    release,
                }),
            filters: s.bands(g).into_iter().map(filter).collect(),
            ..app::Sound::default()
        })
        .collect()
}

fn param_of(p: app::Param) -> kontra_host::sound::Param {
    use kontra_host::sound::Param as H;
    match p {
        app::Param::Attack => H::Attack,
        app::Param::Hold => H::Hold,
        app::Param::Decay => H::Decay,
        app::Param::Sustain => H::Sustain,
        app::Param::Release => H::Release,
        app::Param::Hz(n) => H::Hz(n),
        app::Param::Q(n) => H::Q(n),
        app::Param::Gain(n) => H::Gain(n),
    }
}

/// The rack part an action names, by its place in the editor's list.
fn part_of(action: &Action) -> Option<usize> {
    match *action {
        Action::Volume(n, _)
        | Action::Pan(n, _)
        | Action::Channel(n, _)
        | Action::Output(n, _)
        | Action::Solo(n, _)
        | Action::Mute(n, _)
        | Action::Remove(n)
        | Action::Control { part: n, .. }
        | Action::ControlText { part: n, .. }
        | Action::Sound { part: n, .. } => Some(n),
        _ => None,
    }
}

pub fn editor(params: Arc<KontraParams>) -> Box<dyn Editor> {
    let p = Arc::clone(&params);
    let watched = Arc::clone(&params);
    let dropping = Arc::clone(&params);
    let owner = Arc::clone(&params);
    let mut app = App {
        build: crate::BUILD.into(),
        ..App::default()
    };
    let mut shown = Shown::default();
    let mut look = SavedView::default();
    let mut waiting = Vec::new();
    let mut restored = None;
    // Where the last load the editor asked for lands.
    let mut target = app::Loading::New;
    let mut buttons = None;
    MuiEditor::new(params, kontra_ui::ui(), (1180, 780), move |ui, bridge| {
        // On opening, and whenever the host loads a state: its view.
        let now = p.shared.restored.load(Ordering::Acquire);
        if restored != Some(now) {
            restored = Some(now);
            look = SavedView::read(&p.look.read().unwrap_or_else(PoisonError::into_inner));
            crate::look::apply(&mut app, &look);
            waiting.clone_from(&look.parts);
        }
        let slots: Vec<usize> = {
            let view = lock(&p.shared.view);
            view.fill(&mut app, &mut shown);
            view.parts.iter().map(|v| v.slot).collect()
        };
        crate::look::parts(&mut app, &slots, &mut waiting);
        app.master = f64::from(p.volume.value());
        app.loading = p.shared.loading().then_some(target);
        let response = kontra_ui::app::view(ui, &mut app);
        // A frame reads its input as it builds, so what a press or release
        // changes (a switch applied to the face, a row selected) shows in
        // the frame after it: ask for that one, or it waits for more input.
        let held = ui.pointer().buttons;
        if buttons.replace(held) != Some(held) || !response.changed.is_empty() {
            p.shared.dirty.store(true, Ordering::Release);
        }
        for action in response.changed {
            let asked = match action {
                Action::Load { .. } => Some(app::Loading::New),
                Action::Replace { part, .. }
                | Action::Switch { part, .. }
                | Action::Step { part, .. } => Some(app::Loading::Part(part)),
                _ => None,
            };
            if let Some(t) = asked {
                // One more frame, so the chip shows before the load ends.
                target = t;
                p.shared.dirty.store(true, Ordering::Release);
            }
            if let Action::Master(db) = action {
                bridge.drag_gesture(P::Volume, db);
                continue;
            }
            if action == Action::AddFolder {
                // The browser's first folder, from the editor's own copy.
                let from = app.folders.first().map(PathBuf::from);
                p.shared.folder.ask(from);
                continue;
            }
            if let Some(event) = played(&action) {
                if let Some(player) = lock(&p.shared.player).as_ref() {
                    player.play(event);
                }
                continue;
            }
            if matches!(
                action,
                Action::Load { .. }
                    | Action::Star(..)
                    | Action::Refresh
                    | Action::RemoveFolder(_)
                    | Action::NewParts { .. }
            ) {
                p.shared.request(Request::Browse(action));
                continue;
            }
            // The browser's part actions name the rack slot, not the row.
            let row = |n: usize| slots.get(n).copied();
            let slotted = match action {
                Action::Switch { part, preset } => {
                    row(part).map(|part| Action::Switch { part, preset })
                }
                Action::Step { part, by } => row(part).map(|part| Action::Step { part, by }),
                Action::Replace {
                    part,
                    library,
                    preset,
                } => row(part).map(|part| Action::Replace {
                    part,
                    library,
                    preset,
                }),
                _ => None,
            };
            if let Some(action) = slotted {
                p.shared.request(Request::Browse(action));
                continue;
            }
            let slot = part_of(&action).and_then(|n| slots.get(n).copied());
            p.shared.request(Request::Ui(Edit { slot, action }));
        }
        // Parts not loaded yet keep how they were saved.
        let mut now = crate::look::saved(&app, &slots);
        now.parts.extend_from_slice(&waiting);
        if now != look {
            *p.look.write().unwrap_or_else(PoisonError::into_inner) = now.write();
            look = now;
        }
        response.el
    })
    .resizable((900, 600))
    .on_files(move |_, _, paths, dropped| {
        let instruments: Vec<_> = paths.iter().filter(|p| is_instrument(p)).cloned().collect();
        let folders: Vec<_> = paths.iter().filter(|p| p.is_dir()).cloned().collect();
        if dropped {
            for path in &folders {
                dropping.shared.request(Request::AddRoot(path.clone()));
            }
            for path in &instruments {
                dropping.shared.request(Request::Load {
                    part: None,
                    path: path.clone(),
                });
            }
        }
        !instruments.is_empty() || !folders.is_empty()
    })
    .changed(move || {
        // Only events draw: a change the tick or the view marked, or a
        // picture that arrived. Nothing polled (pictures pending, a load
        // under way) asks for frames, so nothing stuck can spin them.
        watched.shared.dirty.swap(false, Ordering::AcqRel) | kontra_ui::pictures::arrived()
    })
    // Add folder's picker is owned by the host's window (Windows).
    .on_win32_parent(move |hwnd| owner.shared.folder.set_owner(hwnd))
    .into_editor()
}

/// The port the editor's notes come from: no host port, so a host note-off
/// never ends a key the editor holds, nor the other way round.
const EDITOR_PORT: u8 = 255;

/// What the editor's keyboard and wheels play, on channel 1 of every part.
fn played(action: &Action) -> Option<Event> {
    Some(match *action {
        Action::Note(key, true) => Event::NoteOn {
            note: HostNote {
                port: EDITOR_PORT,
                channel: 0,
                key,
                id: -1,
                clap: false,
            },
            velocity: 100.0 / 127.0,
            tune: 0.0,
        },
        Action::Note(key, false) => Event::NoteOff(HostPattern {
            port: i32::from(EDITOR_PORT),
            channel: 0,
            key: i32::from(key),
            id: -1,
            clap: false,
        }),
        // -1..1 onto MIDI 1.0's 14-bit bend, centred on 8192.
        Action::Pitch(x) => {
            let x = x.clamp(-1.0, 1.0);
            let bend = (8192.0 + x * if x < 0.0 { 8192.0 } else { 8191.0 }).round() as u16;
            Event::midi1(0xE0, (bend & 127) as u8, (bend >> 7) as u8)
        }
        Action::Modulation(x) => Event::midi1(0xB0, 1, (x.clamp(0.0, 1.0) * 127.0).round() as u8),
        _ => return None,
    })
}

/// Whether a dropped file is an instrument the rack loads: a Kontakt
/// instrument or snapshot, or a UVI program inside its bank (`bank.ufs/Name.uvip`).
fn is_instrument(path: &std::path::Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    matches!(ext.as_deref(), Some("nki" | "nksn" | "uvip"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keyboard_and_wheels_play_midi() {
        let on = played(&Action::Note(60, true));
        assert!(matches!(on, Some(Event::NoteOn { note, .. }) if note.key == 60));
        assert!(matches!(played(&Action::Note(60, false)), Some(Event::NoteOff(p)) if p.key == 60));
        // Centre, top and bottom of the bend.
        let bend = |x| match played(&Action::Pitch(x)) {
            Some(Event::Ump([w, _])) => (w >> 8 & 127) | (w & 127) << 7,
            other => panic!("{other:?}"),
        };
        assert_eq!((bend(0.0), bend(1.0), bend(-1.0)), (8192, 16383, 0));
        assert!(played(&Action::Panic).is_none());
        assert!(is_instrument(std::path::Path::new("/a/B.NKI")));
        assert!(!is_instrument(std::path::Path::new("/a/b.wav")));
    }
}
