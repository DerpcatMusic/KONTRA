//! KONTRA v3: the plugin shell (07-APP.md §3). moose's CLAP and VST3 glue
//! over [`kontra_host::rack`] and the [`kontra_ui`] editor. The shell stamps
//! host events with their frame offset, hands them to the rack, copies the
//! rack's output pairs to the host's ports and keeps the project state.
//! It holds no engine logic of its own.
//!
//! Threads: the host's audio thread owns [`Dsp`] (the rack's audio side);
//! [`Tick`] runs on moose's task pool and owns the rack's control side
//! behind a mutex the audio thread never sees.

mod align;
mod editor;
mod folder;
mod follow;
mod frames;
mod input;
mod look;
mod macros;
#[cfg(test)]
mod playtest;
mod readout;
#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, RwLock, TryLockError};

use kontra_host::rack::{Event, Rack, RackAudio, RenderMode, Store, StoreConfig};
use kontra_host::state::Saved;
use kontra_host::view::Interfaces;
use moose::core::LosslessEventRef;
use moose::prelude::*;

/// The build as About shows it (stamped by build.rs), with the changelog.
pub const BUILD: kontra_ui::app::Build = kontra_ui::app::Build {
    version: env!("KONTRA_VERSION"),
    revision: env!("KONTRA_REVISION"),
    built: env!("KONTRA_BUILT"),
    target: env!("KONTRA_TARGET"),
    formats: env!("KONTRA_FORMATS"),
    changelog: include_str!("../../CHANGELOG.md"),
};

/// Stereo outputs the plugin offers: one per rack part.
pub const OUTPUTS: usize = kontra_host::rack::OUTPUTS;

/// Host events one block takes; more in one block are dropped and counted.
const EVENTS: usize = 4096;

/// Frames rendered at once, well inside the engine's look-ahead.
const CHUNK: usize = 1024;

#[derive(Params)]
#[params(
    pre_save = "capture",
    post_load = "restore",
    presentation = "macro_presentation",
    presentation_revision = "macro_revision",
    presentation_change = "macro_renamed"
)]
pub struct KontraParams {
    #[param(
        id = 0,
        name = "Volume",
        range = "linear(-60, 6)",
        default = 0.0,
        unit = "dB",
        smooth = "exp(5)"
    )]
    pub volume: FloatParam,
    /// Holds the earlier parts so every part sounds on the beat, and
    /// reports the latest part's attack delay as the plugin's latency.
    #[param(id = 1, name = "Auto-align", default = false)]
    pub align: BoolParam,
    /// Host automation slots, Kontakt's: a script's control takes the slot
    /// its `AUTOMATION_ID` names, and a control the player moves takes a
    /// free one ([`macros`]). Slot n is host id 2 + n, so sessions saved
    /// with the earlier eight macros keep theirs.
    #[param(id = 2, name = "#{i:03}", range = "linear(0, 1)", default = 0.0)]
    pub slots: [FloatParam; 128],
    /// The rack as JSON ([`Saved`]): parts, presets, mix.
    #[persist = "rack"]
    pub saved: RwLock<String>,
    /// The editor's view as JSON ([`kontra_host::state::SavedView`]).
    #[persist = "editor"]
    pub look: RwLock<String>,
    /// Which panel control each macro moves ([`macros::write`]).
    #[persist = "macros"]
    pub macros: RwLock<String>,
    #[skip]
    pub shared: Shared,
    #[meter]
    pub level: MeterSlot,
}

use KontraParamsParamId as P;

/// Coordinates `KontraParams::capture` with [`Tick`] without spinning or
/// sleeping: `capture` blocks on `cv` only during short ticks and returns
/// immediately (keeping pre-load `saved`) while a slow load holds the rack.
#[derive(Default)]
struct RackGate {
    state: Mutex<GateState>,
    cv: Condvar,
}

#[derive(Default)]
struct GateState {
    busy: bool,
    holding: bool,
}

struct GateGuard<'a>(&'a RackGate);

impl Drop for GateGuard<'_> {
    fn drop(&mut self) {
        let mut s = lock(&self.0.state);
        s.busy = false;
        s.holding = false;
        self.0.cv.notify_all();
    }
}

impl RackGate {
    fn enter(&self) -> GateGuard<'_> {
        lock(&self.state).busy = true;
        GateGuard(self)
    }

    fn set_holding(&self, holding: bool) {
        let mut s = lock(&self.state);
        if s.holding != holding {
            s.holding = holding;
            self.cv.notify_all();
        }
    }

    /// Waits while a short tick is running; returns `false` if a slow load or
    /// restore is holding the rack (caller should keep the pre-load state).
    fn wait_for_rack(&self) -> bool {
        let mut s = lock(&self.state);
        while s.busy && !s.holding {
            s = self.cv.wait(s).unwrap_or_else(PoisonError::into_inner);
        }
        !s.holding
    }
}

/// What the control threads share. The audio thread reads only `latency` and `shown`.
#[derive(Default)]
pub struct Shared {
    rack: Mutex<Option<Rack>>,
    gate: RackGate,
    /// Each macro's host value as last applied to its control, as f64
    /// bits: the audio thread applies a host's in-block moves too.
    macro_values: macros::PerSlot<AtomicU64>,
    /// When the editor last moved each macro ([`macros::touch`]), in
    /// [`macros::now`] milliseconds.
    macro_touched: macros::PerSlot<AtomicU64>,
    /// What the audio side was last told each macro moves.
    /// Bumped when a macro's host name changes ([`macros::renamed`]), and
    /// the value the host was last asked to read the names at.
    macro_names: AtomicU64,
    macro_names_told: AtomicU64,
    macro_targets: Mutex<macros::PerSlot<Option<kontra_host::rack::MacroTarget>>>,
    /// The editor's keyboard into the rack, without the rack's lock.
    player: Mutex<Option<kontra_host::rack::Player>>,
    requests: Mutex<Vec<Request>>,
    /// The editor's edits asked for and carried out, in order: the
    /// editor's copy says how many it shows ([`editor::Snapshot::applied`]).
    ui_sent: AtomicU64,
    ui_applied: AtomicU64,
    /// Opened on the first tick, so a plugin made and dropped by a host
    /// scanning plug-ins never scans libraries.
    browser: Mutex<Option<kontra_host::browser::Browser>>,
    /// The editor's copy of the rack, refreshed by [`Tick`].
    view: Mutex<editor::Snapshot>,
    dirty: AtomicBool,
    /// Coalesced wakeup handle for the currently open editor window.
    waker: Mutex<Option<moose::mui::EditorWaker>>,
    progress: Arc<kontra_protocol::LoadProgress>,
    /// Loads asked for and loads carried out: the editor shows one loading
    /// while the first is ahead.
    asked: AtomicU64,
    done: AtomicU64,
    /// A host's state is loading (a project opened, an undo): shown as a
    /// load too.
    restoring: AtomicBool,
    /// Loads the rack's loader thread has not finished: shown as a load.
    in_flight: AtomicUsize,
    /// The latency to report, in frames; the audio thread reads it.
    latency: AtomicU32,
    /// Moves when the host loads a state: the editor reads its view again.
    restored: AtomicU32,
    /// The audio side a deactivation hands back, with its rate: the next
    /// activation at that rate and block size takes it, parts and all.
    /// Locked on the host's main thread only.
    parked: Arc<Mutex<Option<(f64, RackAudio)>>>,
    /// The "Add folder" dialog; dropping it closes an open one.
    folder: folder::Dialog,
    /// Task spawner captured from the editor context so UI actions still run
    /// [`Tick`] when the host suspends audio processing (e.g. Bitwig track sleep
    /// or stopped audio engine).
    /// Shared with the rack's loader thread, which wakes the tick when a
    /// load is ready.
    tasks: Arc<Mutex<Option<moose::prelude::TaskSpawner<Tick>>>>,
    /// Whether the library browser has a background scan running.
    scanning: AtomicBool,
    /// The editor is open: the tick keeps its copy of the rack current.
    shown: AtomicBool,
    /// What the last editor window left (its faces, pictures and view
    /// state), for the next one to take.
    editor: Mutex<Option<editor::Kept>>,
    /// The audio thread caught a panic in the rack's render: the block went
    /// silent. The tick tells the player, once per fault.
    fault: AtomicBool,
    /// Tests only: the next render panics, as a bug in it would.
    #[cfg(test)]
    inject_panic: AtomicBool,
}

/// What the host or the editor asks of the rack, carried out by [`Tick`].
pub enum Request {
    /// Load `path` into `part`, or into the first free part.
    Load {
        part: Option<usize>,
        path: std::path::PathBuf,
    },
    Restore(Saved),
    /// A message for the player, shown with the load errors.
    Notice(String),
    Ui(editor::Edit),
    /// The browser's own actions: load a preset, star, rescan.
    Browse(kontra_ui::app::Action),
    /// A folder the player dropped, to scan for libraries.
    AddRoot(std::path::PathBuf),
}

impl Shared {
    fn open_editor(&self) {
        {
            let mut view = lock(&self.view);
            view.open_epoch = view.open_epoch.wrapping_add(1);
            self.shown.store(true, Ordering::Release);
        }
        self.wake_tick();
    }

    pub fn set_waker(&self, waker: Option<moose::mui::EditorWaker>) {
        *lock(&self.waker) = waker;
    }

    /// Marks the editor dirty and immediately wakes any open editor window
    /// without waiting for a poll timer.
    pub fn wake_editor(&self) {
        self.dirty.store(true, Ordering::Release);
        if let Some(waker) = lock(&self.waker).as_ref() {
            waker.wake();
        }
    }

    pub fn set_tasks(&self, tasks: Option<moose::prelude::TaskSpawner<Tick>>) {
        if let Some(tasks) = tasks {
            *lock(&self.tasks) = Some(tasks);
        }
    }

    pub fn wake_tick(&self) {
        wake(&self.tasks);
    }

    pub fn has_pending_work(&self) -> bool {
        !lock(&self.requests).is_empty() || self.folder.has_result()
    }

    pub fn request(&self, r: Request) {
        if let Request::Ui(edit) = r {
            self.request_ui(edit);
            return;
        }
        if r.loads() {
            if !self.loading() {
                self.progress.begin();
            }
            self.asked.fetch_add(1, Ordering::AcqRel);
        }
        lock(&self.requests).push(r);
        self.wake_tick();
    }

    /// Asks for the editor's `edit`; its number in order, which the
    /// editor's copy of the rack shows once it is carried out.
    pub fn request_ui(&self, mut edit: editor::Edit) -> u64 {
        let seq = {
            let mut requests = lock(&self.requests);
            let seq = self.ui_sent.fetch_add(1, Ordering::AcqRel) + 1;
            edit.seq = seq;
            requests.push(Request::Ui(edit));
            seq
        };
        self.wake_tick();
        seq
    }

    /// Whether a load asked for is not carried out yet.
    fn loading(&self) -> bool {
        self.asked.load(Ordering::Acquire) > self.done.load(Ordering::Acquire)
            || self.restoring.load(Ordering::Acquire)
            || self.in_flight.load(Ordering::Acquire) > 0
    }
}

/// Loads on the rack's own thread from now on. A finished load wakes the
/// tick that installs it: nothing else need run (an input event, an audio
/// block) for it to land.
fn load_in_background(shared: &Shared, rack: &mut Rack) {
    let tasks = Arc::clone(&shared.tasks);
    rack.load_in_background(move || wake(&tasks));
}

/// Runs a [`Tick`] soon, if a spawner is known.
fn wake(tasks: &Mutex<Option<moose::prelude::TaskSpawner<Tick>>>) {
    if let Some(tasks) = lock(tasks).as_ref() {
        tasks.spawn_coalescing(Tick);
    }
}

impl Request {
    /// Whether it loads an instrument into a part.
    fn loads(&self) -> bool {
        use kontra_ui::app::Action;
        match self {
            Self::Load { .. } => true,
            Self::Browse(a) => matches!(
                a,
                Action::Load { .. }
                    | Action::Replace { .. }
                    | Action::Switch { .. }
                    | Action::Step { .. }
            ),
            _ => false,
        }
    }
}

impl KontraParams {
    /// Before the host saves: the rack as it is now. A state still waiting
    /// to load (the host saved before the first block) is kept as it came.
    fn capture(&self) {
        let waiting = lock(&self.shared.requests)
            .iter()
            .any(|r| matches!(r, Request::Restore(_)));
        if waiting {
            return;
        }
        // A drop not loaded yet is saved too: load it now.
        let loading = lock(&self.shared.requests)
            .iter()
            .any(|r| matches!(r, Request::Load { .. }));
        if loading {
            Tick.run(self);
        }
        // A load holds the rack for seconds: the host's thread does not
        // wait for it, and keeps the state the tick saved just before it.
        // Any other tick is over in milliseconds and notifies `gate.cv`.
        let rack = loop {
            match self.shared.rack.try_lock() {
                Ok(rack) => break rack,
                Err(TryLockError::Poisoned(e)) => break e.into_inner(),
                Err(TryLockError::WouldBlock) => {
                    if !self.shared.gate.wait_for_rack() {
                        return;
                    }
                }
            }
        };
        if let Some(rack) = rack.as_ref() {
            *self.saved.write().unwrap_or_else(PoisonError::into_inner) = rack.save().write();
        }
    }

    /// A learnt macro's host name, "Part: Control".
    fn macro_presentation(&self, id: u32) -> Option<moose::params::ParameterPresentation> {
        let n = macros::index(id)?;
        let text = self.macros.read().unwrap_or_else(PoisonError::into_inner);
        let label = macros::read(&text).into_iter().nth(n)??.label;
        (!label.is_empty()).then(|| moose::params::ParameterPresentation {
            name: label,
            ..Default::default()
        })
    }

    fn macro_revision(&self) -> u64 {
        self.shared.macro_names.load(Ordering::Acquire)
    }

    /// A macro's move asks the host to read the names again while one
    /// changed since it last did: a learn's first move, or the next move
    /// after a release. Atomics only: it runs on the audio thread too.
    fn macro_renamed(&self, id: u32) -> bool {
        let now = self.shared.macro_names.load(Ordering::Acquire);
        macros::index(id).is_some()
            && self.shared.macro_names_told.swap(now, Ordering::AcqRel) != now
    }

    /// After the host loads a state: reload its parts on the next tick.
    fn restore(&self) {
        let json = self
            .saved
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        self.shared.restored.fetch_add(1, Ordering::Release);
        // The restored macros' names.
        macros::renamed(self);
        let mut requests = lock(&self.shared.requests);
        requests.retain(|r| !matches!(r, Request::Restore(_)));
        // A damaged or foreign state loads an empty rack, and says so.
        let saved = Saved::parse(&json).unwrap_or_else(|message| {
            requests.push(Request::Notice(message));
            Saved::new(Vec::new())
        });
        requests.push(Request::Restore(saved));
    }
}

/// The player's library browser, its prefs in the user's settings; tests
/// keep nothing on disk.
fn browse() -> kontra_host::browser::Browser {
    let (config, cache) = if cfg!(test) {
        (None, None)
    } else {
        (dirs::config_dir(), dirs::cache_dir())
    };
    let mut browser = kontra_host::browser::Browser::open(config.as_deref(), cache.as_deref());
    // Library folders found on their own (not in tests): NI/UVI
    // registrations, default folders, a brief walk of home and the drives.
    if config.is_some() {
        browser.discover();
    }
    // The player's own presets, listed with their libraries' (not in tests).
    if let Some(user) = config.and_then(|_| kontra_host::presets::folder()) {
        browser.set_user(user);
    }
    // The saved colours, for every editor in this process.
    kontra_ui::theme::set_look(kontra_ui::theme::Look::from_tokens(&browser.theme()));
    browser
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The audio thread's state, built by [`Kontra`]'s `activate`.
#[derive(Default)]
pub struct Dsp {
    audio: Option<RackAudio>,
    /// [`Shared::parked`], where `deactivate` leaves `audio`.
    park: Arc<Mutex<Option<(f64, RackAudio)>>>,
    input: Vec<(u32, Event)>,
    /// This chunk's share of `input`.
    chunk: Vec<(u32, Event)>,
    /// This block's host changes that split it: macro moves and transport.
    moves: Vec<(u32, Change)>,
    gain: f32,
    rate: f64,
    until_tick: usize,
    was_active: bool,
    latency: u32,
}

/// A host change inside a block, applied at its frame.
#[derive(Clone, Copy)]
enum Change {
    /// Macro `n` to host value `v`.
    Macro(usize, f64),
    /// Tempo, play state, song position and signature.
    Transport(TransportInfo),
}

/// The control tick: carries out requests, frees what audio retired and
/// refreshes the editor's snapshot. Spawned from `process` about 20 times a
/// second; moose runs one at a time.
pub struct Tick;

/// Whether `widget` on `rack` part `slot` is a continuous control whose
/// consecutive identical-modifier drag moves can safely coalesce to
/// latest-wins (buttons, switches, and `native_ui` tap areas preserve every
/// press/release edge).
fn is_continuous_control(rack: &Rack, slot: usize, widget: usize) -> bool {
    let Some(w) = rack
        .parts()
        .get(slot)
        .and_then(Option::as_ref)
        .and_then(|p| p.view.as_ref())
        .and_then(|v| v.tree().widgets.get(widget))
    else {
        return false;
    };
    if w.name.starts_with("native_ui tap") {
        return false;
    }
    matches!(
        w.kind,
        kontra_ui_model::Kind::Knob { .. }
            | kontra_ui_model::Kind::Slider { .. }
            | kontra_ui_model::Kind::ValueEdit { .. }
            | kontra_ui_model::Kind::Table { .. }
            | kontra_ui_model::Kind::Xy { .. }
    )
}

/// Coalesces consecutive continuous `Request::Ui(Action::Control)` moves for
/// the exact same `(slot, widget, cell, keys)` to latest-wins (preserving the
/// highest `seq`), while keeping press/release edges and inter-control
/// ordering intact.
pub(crate) fn coalesce_requests(requests: Vec<Request>, rack: &Rack) -> Vec<Request> {
    use kontra_ui::app::Action;
    let mut out: Vec<Request> = Vec::with_capacity(requests.len());
    for req in requests {
        let same_continuous = match (out.last(), &req) {
            (
                Some(Request::Ui(editor::Edit {
                    slot: Some(s1),
                    action:
                        Action::Control {
                            widget: w1,
                            cell: c1,
                            keys: k1,
                            ..
                        },
                    ..
                })),
                Request::Ui(editor::Edit {
                    slot: Some(s2),
                    action:
                        Action::Control {
                            widget: w2,
                            cell: c2,
                            keys: k2,
                            ..
                        },
                    ..
                }),
            ) => (s1, w1, c1, k1) == (s2, w2, c2, k2) && is_continuous_control(rack, *s2, *w2),
            _ => false,
        };
        if same_continuous && let Some(last) = out.last_mut() {
            *last = req;
        } else {
            out.push(req);
        }
    }
    out
}

impl BackgroundTask for Tick {
    type Params = KontraParams;
    const SERIALIZED: bool = true;

    fn run(self, p: &KontraParams) {
        let _gate = p.shared.gate.enter();
        let mut guard = lock(&p.shared.rack);
        let Some(rack) = guard.as_mut() else {
            return;
        };
        // Capture the opening before taking requests, so a tick that began
        // before reopening cannot mark its older snapshot ready.
        let open_epoch = {
            let view = lock(&p.shared.view);
            p.shared
                .shown
                .load(Ordering::Acquire)
                .then_some(view.open_epoch)
        };
        let interfaces = if open_epoch.is_some() {
            Interfaces::Shown
        } else {
            Interfaces::Hidden
        };
        let raw_requests = std::mem::take(&mut *lock(&p.shared.requests));
        let changed = !raw_requests.is_empty();
        let requests = coalesce_requests(raw_requests, rack);
        let mut report = Vec::new();
        if p.shared.fault.swap(false, Ordering::AcqRel) {
            report.push(
                "the audio engine hit an internal error and a block was silenced; if it repeats, reload the project"
                    .to_string(),
            );
        }
        let mut browser = lock(&p.shared.browser);
        let browser = browser.get_or_insert_with(browse);
        browser.poll();
        rack.know(browser);
        // What the loads of this tick found missing comes after them.
        let mut notices = Vec::new();
        // A state the host restored in this batch is what `saved` holds: a
        // load beside it must not write the rack over it.
        let restores = requests.iter().any(|r| matches!(r, Request::Restore(_)));
        for r in requests {
            let loads = r.loads();
            let holds = loads || matches!(r, Request::Restore(_));
            if holds {
                // What a host save during the load writes (`capture`); a
                // state waiting to load is already what `saved` holds.
                if loads && !restores && !requests_restoring(p) {
                    *p.saved.write().unwrap_or_else(PoisonError::into_inner) = rack.save().write();
                }
                p.shared.gate.set_holding(true);
            }
            match r {
                Request::Load { part, path } => {
                    let loaded = match (part, rack.free()) {
                        (Some(part), _) => rack.load(part, &path),
                        (None, Some(new)) => rack.load(new, &path).map(|()| {
                            if !rack.place_later(new) {
                                editor::place(rack, browser, new);
                            }
                        }),
                        (None, None) => Err(kontra_host::Error::NoFreePart),
                    };
                    if let Err(e) = loaded {
                        report.push(format!("{}: {e}", path.display()));
                    }
                }
                Request::Restore(saved) => {
                    p.shared.restoring.store(true, Ordering::Release);
                    for (part, e) in rack.restore(&saved) {
                        report.push(format!("part {}: {e}", part + 1));
                    }
                    p.shared.restoring.store(false, Ordering::Release);
                }
                Request::Notice(message) => report.push(message),
                Request::Ui(edit) => {
                    let seq = edit.seq;
                    editor::apply(rack, edit);
                    if seq > 0 {
                        p.shared.ui_applied.fetch_max(seq, Ordering::AcqRel);
                    } else {
                        p.shared.ui_applied.fetch_add(1, Ordering::AcqRel);
                    }
                }
                Request::Browse(action) => editor::browse(rack, browser, action, &mut report),
                Request::AddRoot(path) => browser.add_root(path),
            }
            p.shared.gate.set_holding(false);
            // Counted before `done`, so the load never looks finished.
            p.shared
                .in_flight
                .store(rack.in_flight(), Ordering::Release);
            if loads {
                p.shared.done.fetch_add(1, Ordering::AcqRel);
            }
        }
        // The loads that finished on the loader thread go in now.
        let (placed, failed) = rack.finish_loads();
        for part in placed {
            editor::place(rack, browser, part);
        }
        report.extend(failed);
        p.shared
            .in_flight
            .store(rack.in_flight(), Ordering::Release);
        // A scan reading the same disk waits while a load runs.
        browser.pause(rack.in_flight() > 0);
        notices.extend(rack.take_notices());
        notices.append(&mut browser.notices);
        report.extend(notices);
        match p.shared.folder.picked() {
            Some(Ok((folder::Wanted::Libraries, path))) => browser.add_root(path),
            Some(Ok((folder::Wanted::Snapshots, path))) => browser.set_kontakt(path),
            Some(Ok((folder::Wanted::Locate(n), path))) => browser.locate(n, path),
            Some(Ok((folder::Wanted::ExportReport(contents), path))) => {
                match std::fs::write(&path, contents) {
                    Ok(()) => report.push(format!("Exported diagnostics report to {}", path.display())),
                    Err(err) => report.push(format!(
                        "Could not export diagnostics report to {}: {err}",
                        path.display()
                    )),
                }
            }
            Some(Err(err)) => report.push(format!(
                "Could not open folder picker ({err}). You can also drag and drop a library folder onto KONTRA."
            )),
            None => {}
        }
        let latency = align::update(p.align.value(), rack);
        p.shared.latency.store(latency, Ordering::Relaxed);
        rack.show_interfaces(interfaces);
        // The host's automation of a macro moves its control, as the
        // player's move does.
        if !p.shared.loading() {
            macros::release(p, |n| {
                let part = rack.parts().get(n).and_then(Option::as_ref);
                part.map(|p| macros::instrument(p.path.as_deref()))
            });
            // A script's controls with automation ids take their slots.
            macros::declare(
                p,
                (rack.parts().iter().enumerate()).filter_map(|(n, part)| {
                    let part = part.as_ref()?;
                    let widgets = part.view.as_ref()?.tree().widgets.as_slice();
                    let path = macros::instrument(part.path.as_deref());
                    Some((n, part.name.as_str(), path, widgets))
                }),
            );
        }
        macros::send_targets(p, rack);
        for (part, name, value) in macros::moved(p) {
            let tree = rack.parts().get(part).and_then(Option::as_ref);
            let tree = tree.and_then(|p| p.view.as_ref()).map(|v| v.tree());
            let widget = tree.and_then(|t| t.widgets.iter().position(|w| w.name == name));
            if let Some(widget) = widget {
                rack.control(part, widget, None, value, Default::default());
            }
        }
        // The looks the scripts sent (a native_ui rebuild among them).
        frames::time("tick", || rack.tick());
        follow::follow(rack, browser);
        p.shared
            .scanning
            .store(browser.scanning(), Ordering::Release);
        let mut view = lock(&p.shared.view);
        view.applied = p.shared.ui_applied.load(Ordering::Acquire);
        // A closed editor's copy only keeps the report; it catches up on
        // the first tick after the editor opens.
        let refreshed = match open_epoch {
            Some(epoch) if epoch == view.open_epoch => {
                let opened = !view.ready();
                let changed = frames::time("copy", || view.refresh(rack, browser, report));
                view.ready_epoch = epoch;
                opened || changed
            }
            _ => view.report(report),
        };
        // What the requests did shows immediately via the coalesced editor
        // waker instead of waiting on a poll timer.
        if refreshed | changed {
            p.shared.wake_editor();
        }
    }
}

/// A host state is waiting to load: what `saved` holds is that state.
fn requests_restoring(p: &KontraParams) -> bool {
    lock(&p.shared.requests)
        .iter()
        .any(|r| matches!(r, Request::Restore(_)))
}

pub struct Kontra;

impl Plugin for Kontra {
    type Shared = KontraParams;
    type MainThread = ();
    type Processor = Dsp;

    fn bus_layouts() -> Vec<BusLayout> {
        const NAMES: [&str; OUTPUTS] = [
            "st.1", "st.2", "st.3", "st.4", "st.5", "st.6", "st.7", "st.8", "st.9", "st.10",
            "st.11", "st.12", "st.13", "st.14", "st.15", "st.16",
        ];
        vec![NAMES.into_iter().fold(BusLayout::new(), |l, name| {
            l.with_output(name, ChannelConfig::Stereo)
        })]
    }

    fn main_thread(_: &Arc<KontraParams>) {}

    /// The audio side at the host's rate (allocates; main thread). The one
    /// the last deactivation parked is kept whenever its sample rate matches
    /// (resizing audio scratch/mix buffers on the main thread if the host's
    /// `max_block_size` grew); otherwise a new rack is built and the previous
    /// parts reload on the next tick.
    fn activate(
        p: &KontraParams,
        c: &AudioConfig,
        cx: &InitContext,
    ) -> Result<Dsp, PreparationError> {
        // The tick runs without an editor or an audio block (a load landing).
        p.shared.set_tasks(cx.tasks::<Tick>());
        let parked = lock(&p.shared.parked).take();
        let mut audio = match parked {
            Some((rate, mut audio)) if rate == c.sample_rate => {
                audio.resize_max_block(c.max_block_size);
                Some(audio)
            }
            _ => build_rack(p, c),
        };
        if let Some(audio) = audio.as_mut() {
            audio.panic();
        }
        Ok(Dsp {
            audio,
            park: Arc::clone(&p.shared.parked),
            input: Vec::with_capacity(EVENTS),
            chunk: Vec::with_capacity(EVENTS),
            moves: Vec::with_capacity(EVENTS),
            gain: db_to_linear(p.volume.value()),
            rate: c.sample_rate,
            until_tick: 0,
            was_active: false,
            latency: p.shared.latency.load(Ordering::Relaxed),
        })
    }

    /// Parks the audio side for the next activation (main thread).
    fn deactivate(s: Dsp) {
        if let Some(audio) = s.audio {
            *lock(&s.park) = Some((s.rate, audio));
        }
    }

    /// May run on the audio thread (CLAP's plain reset, a host restarting
    /// its stream): keeps the loaded parts and only ends their notes.
    fn reset(s: &mut Dsp, _p: &KontraParams) {
        if let Some(audio) = s.audio.as_mut() {
            audio.panic();
        }
    }

    fn process(
        s: &mut Dsp,
        p: &KontraParams,
        b: &mut AudioBuffer,
        events: &EventList,
        cx: &mut ProcessContext,
    ) -> ProcessStatus {
        let frames = b.num_samples();
        let Some(audio) = s.audio.as_mut() else {
            for ch in 0..b.num_output_channels() {
                b.output(ch).fill(0.0);
            }
            return ProcessStatus::Normal;
        };
        s.latency = p.shared.latency.load(Ordering::Relaxed);
        s.input.clear();
        s.moves.clear();
        audio.set_render_mode(if cx.process_mode.is_offline() {
            RenderMode::Offline
        } else {
            RenderMode::Live
        });
        // VST3 hosts give the tempo only here; CLAP may also send events.
        audio.set_tempo(cx.transport.tempo as f32);
        let t = cx.transport;
        audio.set_transport(
            t.playing,
            t.position_beats,
            (t.time_sig_num, t.time_sig_den),
        );
        for e in events.lossless_iter() {
            let e = match e {
                LosslessEventRef::Typed(e) => e,
                LosslessEventRef::Exact(x) => {
                    if let Some(event) = input::exact(x) {
                        if s.input.len() < s.input.capacity() {
                            s.input.push((x.sample_offset(), event));
                        }
                        continue;
                    }
                    // Else its typed form, when the host gave one.
                    let Some(e) = x.fallback() else { continue };
                    e
                }
            };
            let change = match e.body {
                EventBody::ParamChange { id, value } => {
                    macros::index(id).map(|n| Change::Macro(n, value))
                }
                EventBody::Transport(t) => Some(Change::Transport(t)),
                _ => None,
            };
            if let Some(change) = change {
                if s.moves.len() < s.moves.capacity() {
                    s.moves.push((e.sample_offset, change));
                }
            } else if let Some(event) = input::event(e)
                && s.input.len() < s.input.capacity()
            {
                s.input.push((e.sample_offset, event));
            }
        }
        // The master ramps from the last block's gain to this one's.
        let target = db_to_linear(p.volume.value());
        let step = (target - s.gain) / frames.max(1) as f32;
        let mut peak = 0.0f32;
        for ch in 0..b.num_output_channels() {
            b.output(ch).fill(0.0);
        }
        // Rendered in chunks, so a long offline block never reads past what
        // the engine looked ahead for.
        // Host macro moves and transport changes split it too: each
        // applies at its frame.
        let chunk = CHUNK.min(audio.max_block()).max(1);
        let mut moves = s.moves.iter().peekable();
        let mut start = 0;
        while start < frames {
            while let Some(&(_, change)) = moves.next_if(|m| m.0 as usize <= start) {
                match change {
                    // A host's echo of the editor's own move (one from
                    // before the latest) would pull the control back.
                    Change::Macro(n, _) if macros::held(p, n) => {}
                    Change::Macro(n, value) => {
                        if audio.move_macro(n, value)
                            && let Some(last) = p.shared.macro_values.get(n)
                        {
                            last.store(value.to_bits(), Ordering::Relaxed);
                        }
                    }
                    Change::Transport(t) => {
                        audio.set_tempo(t.tempo as f32);
                        let signature = (t.time_sig_num, t.time_sig_den);
                        audio.set_transport(t.playing, t.position_beats, signature);
                    }
                }
            }
            let next = moves.peek().map_or(frames, |m| (m.0 as usize).min(frames));
            let len = chunk.min(frames - start).min(next - start);
            let end = (start + len) as u32;
            s.chunk.clear();
            s.chunk.extend(
                (s.input.iter())
                    .filter(|(at, _)| (start as u32..end).contains(at))
                    .map(|&(at, e)| (at - start as u32, e)),
            );
            // A bounce waits for the disk; a live block never does.
            if cx.process_mode.is_offline() {
                audio.settle();
            }
            // A panic in the render is silence and a message, never an unwind
            // into the host (catching costs nothing while nothing panics).
            let chunk_events = &s.chunk;
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                #[cfg(test)]
                assert!(
                    !p.shared.inject_panic.swap(false, Ordering::AcqRel),
                    "injected render panic"
                );
                audio.process(len, chunk_events);
            }))
            .is_err()
            {
                p.shared.fault.store(true, Ordering::Release);
                if let Some(tasks) = cx.tasks::<Tick>() {
                    tasks.spawn_coalescing(Tick);
                }
                for ch in 0..b.num_output_channels() {
                    b.output(ch).fill(0.0);
                }
                return ProcessStatus::Normal;
            }
            for ch in 0..b.num_output_channels() {
                let pair = audio.output(ch / 2, len);
                let Some(out) = b.output(ch).get_mut(start..start + len) else {
                    continue;
                };
                for (n, (o, frame)) in out.iter_mut().zip(pair).enumerate() {
                    let x = frame.get(ch % 2).copied().unwrap_or(0.0);
                    *o = x * (s.gain + step * (start + n) as f32);
                    peak = peak.max(o.abs());
                }
            }
            start += len;
        }
        s.gain = target;
        cx.set_meter(&p.level, peak.min(1.0));
        // Event-driven wake: whenever audio returned a LookFrame/Capture/
        // request or advanced a part's UI/persist epoch, wake `Tick`
        // immediately (≤ 1 block round-trip). While actively producing sound
        // with an open editor, keep a 20 Hz housekeeping tick for meters/stats;
        // when completely silent and idle, spawn 0 ticks.
        let active = peak > 1e-6 || audio.active_voices() > 0;
        let urgent = audio.take_wake();
        let meter_tick = (active || s.was_active) && p.shared.shown.load(Ordering::Relaxed);
        s.was_active = active;
        if urgent {
            if let Some(tasks) = cx.tasks::<Tick>() {
                tasks.spawn_coalescing(Tick);
            }
            s.until_tick = (s.rate / 20.0) as usize;
        } else if meter_tick {
            if s.until_tick <= frames {
                if let Some(tasks) = cx.tasks::<Tick>() {
                    tasks.spawn_coalescing(Tick);
                }
                s.until_tick = (s.rate / 20.0) as usize;
            } else {
                s.until_tick -= frames;
            }
        }
        ProcessStatus::Normal
    }

    fn latency(s: &Dsp) -> u32 {
        s.latency
    }

    fn editor(params: Arc<KontraParams>) -> Box<dyn Editor> {
        editor::editor(params)
    }
}

moose::plugin! { plugin: Kontra, params: KontraParams, tasks: [Tick] }

/// Build a fresh rack and its audio side for `c` (allocates; main thread);
/// `None` when the sample store can't open.
fn build_rack(p: &KontraParams, c: &AudioConfig) -> Option<RackAudio> {
    let rate = c.sample_rate.round() as u32;
    let mut store = Store::new(StoreConfig::DEFAULT).ok()?;
    // Other instances in this host share this one's pinned pages.
    store.share_pages_in_process();
    store.set_progress(Arc::clone(&p.shared.progress));
    // A part plays as soon as its plan exists; the sample heads follow.
    store.defer_heads(true);
    let (mut rack, audio) = Rack::new(rate, store, c.max_block_size);
    // Reads and sample loads run on the rack's own thread, never on the
    // shared task pool or under the rack lock. Tests load where they ask.
    if !cfg!(test) {
        load_in_background(&p.shared, &mut rack);
    }
    if let Some(saves) = kontra_host::rack::user_saves() {
        rack.set_saves(saves);
    }
    *lock(&p.shared.player) = Some(rack.player());
    let previous = lock(&p.shared.rack).replace(rack).map(|r| r.save());
    if let Some(saved) = previous.filter(|s| !s.parts.is_empty()) {
        let mut requests = lock(&p.shared.requests);
        if !requests.iter().any(|r| matches!(r, Request::Restore(_))) {
            requests.insert(0, Request::Restore(saved));
        }
    }
    // An editor opened before the host prepared audio is waiting for its
    // first snapshot. The host need not process an audio block to get it.
    if p.shared.shown.load(Ordering::Acquire) {
        p.shared.wake_tick();
    }
    Some(audio)
}

moose::enable_rt_paranoid!();

#[cfg(all(test, feature = "rt-paranoid"))]
mod reset_realtime_tests {
    #[test]
    fn reset_is_realtime_after_prepare() {
        moose_test::assert_reset_realtime::<moose::Lifecycle<super::Kontra>, f32>(
            &super::KontraParams::default(),
            &moose::prelude::AudioConfig::new(48_000.0, 512),
        );
    }
}
