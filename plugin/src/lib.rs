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

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock, TryLockError};
use std::time::Duration;

use kontra_host::rack::{Event, Rack, RackAudio, RenderMode, Store, StoreConfig};
use kontra_host::state::Saved;
use kontra_host::view::Interfaces;
use moose::core::LosslessEventRef;
use moose::prelude::*;

/// The build as About shows it: the version and its source revision.
pub const BUILD: &str = concat!(
    "Version ",
    env!("KONTRA_VERSION"),
    " · ",
    env!("KONTRA_REVISION")
);

/// Stereo outputs the plugin offers: one per rack part.
pub const OUTPUTS: usize = kontra_host::rack::OUTPUTS;

/// Host events one block takes; more in one block are dropped and counted.
const EVENTS: usize = 4096;

/// Frames rendered at once, well inside the engine's look-ahead.
const CHUNK: usize = 1024;

#[derive(Params)]
#[params(pre_save = "capture", post_load = "restore")]
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
    /// Host automation of panel controls, each assigned when the player
    /// first moves one ([`macros`]).
    #[param(id = 2, name = "Macro 1", range = "linear(0, 1)", default = 0.0)]
    pub macro_1: FloatParam,
    #[param(id = 3, name = "Macro 2", range = "linear(0, 1)", default = 0.0)]
    pub macro_2: FloatParam,
    #[param(id = 4, name = "Macro 3", range = "linear(0, 1)", default = 0.0)]
    pub macro_3: FloatParam,
    #[param(id = 5, name = "Macro 4", range = "linear(0, 1)", default = 0.0)]
    pub macro_4: FloatParam,
    #[param(id = 6, name = "Macro 5", range = "linear(0, 1)", default = 0.0)]
    pub macro_5: FloatParam,
    #[param(id = 7, name = "Macro 6", range = "linear(0, 1)", default = 0.0)]
    pub macro_6: FloatParam,
    #[param(id = 8, name = "Macro 7", range = "linear(0, 1)", default = 0.0)]
    pub macro_7: FloatParam,
    #[param(id = 9, name = "Macro 8", range = "linear(0, 1)", default = 0.0)]
    pub macro_8: FloatParam,
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

/// What the control threads share. The audio thread reads only `latency`.
#[derive(Default)]
pub struct Shared {
    rack: Mutex<Option<Rack>>,
    /// Each macro's host value as last applied to its control, as f64
    /// bits: the audio thread applies a host's in-block moves too.
    macro_values: [AtomicU64; macros::COUNT],
    /// What the audio side was last told each macro moves.
    macro_targets: Mutex<[Option<kontra_host::rack::MacroTarget>; macros::COUNT]>,
    /// The editor's keyboard into the rack, without the rack's lock.
    player: Mutex<Option<kontra_host::rack::Player>>,
    requests: Mutex<Vec<Request>>,
    /// Opened on the first tick, so a plugin made and dropped by a host
    /// scanning plug-ins never scans libraries.
    browser: Mutex<Option<kontra_host::browser::Browser>>,
    /// The editor's copy of the rack, refreshed by [`Tick`].
    view: Mutex<editor::Snapshot>,
    dirty: std::sync::atomic::AtomicBool,
    progress: Arc<kontra_protocol::LoadProgress>,
    /// Loads asked for and loads carried out: the editor shows one loading
    /// while the first is ahead.
    asked: AtomicU64,
    done: AtomicU64,
    /// A host's state is loading (a project opened, an undo): shown as a
    /// load too.
    restoring: AtomicBool,
    /// The latency to report, in frames; the audio thread reads it.
    latency: AtomicU32,
    /// Moves when the host loads a state: the editor reads its view again.
    restored: AtomicU32,
    /// The "Add folder" dialog; dropping it closes an open one.
    folder: folder::Dialog,
    /// The editor is open: the tick keeps its copy of the rack current.
    shown: AtomicBool,
    /// A load holds the rack; `saved` is the state from just before it.
    holding: AtomicBool,
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
    pub fn request(&self, r: Request) {
        if r.loads() {
            if !self.loading() {
                self.progress.begin();
            }
            self.asked.fetch_add(1, Ordering::AcqRel);
        }
        lock(&self.requests).push(r);
    }

    /// Whether a load asked for is not carried out yet.
    fn loading(&self) -> bool {
        self.asked.load(Ordering::Acquire) > self.done.load(Ordering::Acquire)
            || self.restoring.load(Ordering::Acquire)
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
        // Any other tick is over in milliseconds.
        let rack = loop {
            match self.shared.rack.try_lock() {
                Ok(rack) => break rack,
                Err(TryLockError::Poisoned(e)) => break e.into_inner(),
                Err(TryLockError::WouldBlock) if self.shared.holding.load(Ordering::Acquire) => {
                    return;
                }
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(1)),
            }
        };
        if let Some(rack) = rack.as_ref() {
            *self.saved.write().unwrap_or_else(PoisonError::into_inner) = rack.save().write();
        }
    }

    /// After the host loads a state: reload its parts on the next tick.
    fn restore(&self) {
        let json = self
            .saved
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        self.shared.restored.fetch_add(1, Ordering::Release);
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
    // The player's own presets, listed with their libraries' (not in tests).
    if let Some(user) = config.and_then(|_| kontra_host::presets::folder()) {
        browser.set_user(user);
    }
    browser
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The audio thread's state.
#[derive(Default)]
pub struct Dsp {
    audio: Option<RackAudio>,
    input: Vec<(u32, Event)>,
    /// This chunk's share of `input`.
    chunk: Vec<(u32, Event)>,
    /// This block's host macro moves: (frame, macro, host value).
    moves: Vec<(u32, usize, f64)>,
    gain: f32,
    rate: f64,
    until_tick: usize,
    latency: u32,
}

/// The control tick: carries out requests, frees what audio retired and
/// refreshes the editor's snapshot. Spawned from `process` about 20 times a
/// second; moose runs one at a time.
pub struct Tick;

impl BackgroundTask for Tick {
    type Params = KontraParams;
    const SERIALIZED: bool = true;

    fn run(self, p: &KontraParams) {
        let mut guard = lock(&p.shared.rack);
        let Some(rack) = guard.as_mut() else {
            return;
        };
        let requests = std::mem::take(&mut *lock(&p.shared.requests));
        let changed = !requests.is_empty();
        let interfaces = if p.shared.shown.load(Ordering::Acquire) {
            Interfaces::Shown
        } else {
            Interfaces::Hidden
        };
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
                p.shared.holding.store(true, Ordering::Release);
            }
            match r {
                Request::Load { part, path } => {
                    let loaded = match (part, rack.free()) {
                        (Some(part), _) => rack.load(part, &path),
                        (None, Some(new)) => {
                            (rack.load(new, &path)).map(|()| editor::place(rack, browser, new))
                        }
                        (None, None) => Err("every part is in use".into()),
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
                Request::Ui(edit) => editor::apply(rack, edit),
                Request::Browse(action) => editor::browse(rack, browser, action, &mut report),
                Request::AddRoot(path) => browser.add_root(path),
            }
            p.shared.holding.store(false, Ordering::Release);
            if loads {
                p.shared.done.fetch_add(1, Ordering::AcqRel);
            }
        }
        notices.extend(rack.take_notices());
        report.extend(notices);
        match p.shared.folder.picked() {
            Some((folder::Wanted::Libraries, path)) => browser.add_root(path),
            Some((folder::Wanted::Snapshots, path)) => browser.set_kontakt(path),
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
        let mut view = lock(&p.shared.view);
        // A closed editor's copy only keeps the report; it catches up on
        // the first tick after the editor opens.
        let refreshed = match interfaces {
            Interfaces::Shown => frames::time("copy", || view.refresh(rack, browser, report)),
            Interfaces::Hidden => view.report(report),
        };
        // What the requests did shows, whether or not the editor's copy
        // of the rack noticed (a script's answer to a click).
        if refreshed | changed {
            p.shared
                .dirty
                .store(true, std::sync::atomic::Ordering::Release);
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

impl PluginLogic for Kontra {
    type Params = KontraParams;
    type DspState = Dsp;

    fn bus_layouts() -> Vec<BusLayout> {
        const NAMES: [&str; OUTPUTS] = [
            "st.1", "st.2", "st.3", "st.4", "st.5", "st.6", "st.7", "st.8", "st.9", "st.10",
            "st.11", "st.12", "st.13", "st.14", "st.15", "st.16",
        ];
        vec![NAMES.into_iter().fold(BusLayout::new(), |l, name| {
            l.with_output(name, ChannelConfig::Stereo)
        })]
    }

    /// A new rack at the host's rate. The parts of the one it replaces (a
    /// rate change) load again into it on the next tick.
    fn reset(s: &mut Dsp, p: &KontraParams, c: &AudioConfig) {
        // A reset at the same rate (CLAP's plain reset, a host restarting
        // its stream) keeps the loaded parts and only ends their notes.
        if let Some(audio) = &s.audio
            && s.rate == c.sample_rate
            && audio.max_block() >= c.max_block_size
            && let Some(rack) = lock(&p.shared.rack).as_mut()
        {
            rack.panic();
            return;
        }
        s.rate = c.sample_rate;
        s.gain = db_to_linear(p.volume.value());
        s.input = Vec::with_capacity(EVENTS);
        s.chunk = Vec::with_capacity(EVENTS);
        s.moves = Vec::with_capacity(EVENTS);
        s.until_tick = 0;
        let rate = c.sample_rate.round() as u32;
        let Ok(mut store) = Store::new(StoreConfig::DEFAULT) else {
            s.audio = None;
            return;
        };
        // Other instances in this host share this one's pinned pages.
        store.share_pages_in_process();
        store.set_progress(Arc::clone(&p.shared.progress));
        // A part plays as soon as its plan exists; the sample heads follow.
        store.defer_heads(true);
        let (mut rack, audio) = Rack::new(rate, store, c.max_block_size);
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
        s.audio = Some(audio);
    }

    fn process(
        s: &mut Dsp,
        p: &KontraParams,
        b: &mut AudioBuffer,
        events: &EventList,
        cx: &mut ProcessContext,
    ) -> ProcessStatus {
        let frames = b.num_samples();
        if s.until_tick <= frames {
            if let Some(tasks) = cx.tasks::<Tick>() {
                tasks.spawn_coalescing(Tick);
            }
            s.until_tick = (s.rate / 20.0) as usize;
        } else {
            s.until_tick -= frames;
        }
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
            if let EventBody::ParamChange { id, value } = e.body
                && let Some(n) = macros::index(id)
            {
                if s.moves.len() < s.moves.capacity() {
                    s.moves.push((e.sample_offset, n, value));
                }
            } else if let EventBody::Transport(t) = e.body {
                audio.set_tempo(t.tempo as f32);
                audio.set_transport(
                    t.playing,
                    t.position_beats,
                    (t.time_sig_num, t.time_sig_den),
                );
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
        // Host macro moves split it too: each applies at its frame.
        let chunk = CHUNK.min(audio.max_block()).max(1);
        let mut moves = s.moves.iter().peekable();
        let mut start = 0;
        while start < frames {
            while let Some(&(_, n, value)) = moves.next_if(|m| m.0 as usize <= start) {
                if audio.move_macro(n, value)
                    && let Some(last) = p.shared.macro_values.get(n)
                {
                    last.store(value.to_bits(), Ordering::Relaxed);
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
        ProcessStatus::Normal
    }

    fn latency(s: &Dsp) -> u32 {
        s.latency
    }

    fn editor(params: std::sync::Arc<KontraParams>) -> Box<dyn Editor> {
        editor::editor(params)
    }
}

moose::plugin! { logic: Kontra, params: KontraParams, tasks: [Tick] }
