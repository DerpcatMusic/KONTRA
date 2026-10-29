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
mod input;
mod look;
#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use kontra_host::rack::{Event, Rack, RackAudio, Store, StoreConfig};
use kontra_host::state::Saved;
use kontra_protocol::{Load, LoadCells};
use moose::core::LosslessEventRef;
use moose::prelude::*;

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
    /// The rack as JSON ([`Saved`]): parts, presets, mix.
    #[persist = "rack"]
    pub saved: RwLock<String>,
    /// The editor's view as JSON ([`kontra_host::state::SavedView`]).
    #[persist = "editor"]
    pub look: RwLock<String>,
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
    /// The editor's keyboard into the rack, without the rack's lock.
    player: Mutex<Option<kontra_host::rack::Player>>,
    requests: Mutex<Vec<Request>>,
    /// Opened on the first tick, so a plugin made and dropped by a host
    /// scanning plug-ins never scans libraries.
    browser: Mutex<Option<kontra_host::browser::Browser>>,
    /// The editor's copy of the rack, refreshed by [`Tick`].
    view: Mutex<editor::Snapshot>,
    dirty: std::sync::atomic::AtomicBool,
    /// Pictures the editor asked for and has not shown yet: it keeps
    /// drawing until they arrive.
    decoding: std::sync::atomic::AtomicBool,
    /// The latency to report, in frames; the audio thread reads it.
    latency: AtomicU32,
    /// Moves when the host loads a state: the editor reads its view again.
    restored: AtomicU32,
    /// The "Add folder" dialog; dropping it closes an open one.
    folder: folder::Dialog,
    /// The rack's [`Rack::loads`], for the editor while a load holds it.
    loads: Mutex<Arc<LoadCells>>,
}

/// What the host or the editor asks of the rack, carried out by [`Tick`].
pub enum Request {
    /// Load `path` into `part`, or into the first free part.
    Load {
        part: Option<usize>,
        path: std::path::PathBuf,
    },
    Restore(Saved),
    Ui(editor::Edit),
    /// The browser's own actions: load a preset, star, rescan.
    Browse(kontra_ui::app::Action),
    /// A folder the player dropped, to scan for libraries.
    AddRoot(std::path::PathBuf),
}

impl Request {
    /// The part this asks to load into, when it names one.
    fn part(&self) -> Option<usize> {
        use kontra_ui::app::Action::{Step, Switch};
        match self {
            Request::Load { part, .. } => *part,
            Request::Browse(Switch { part, .. } | Step { part, .. }) => Some(*part),
            _ => None,
        }
    }
}

impl Shared {
    pub fn request(&self, r: Request) {
        if let Some(part) = r.part() {
            lock(&self.loads).set(part, Load::Queued);
        }
        lock(&self.requests).push(r);
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
        // A tick holds the rack through a whole load (seconds on a big
        // program): the host's thread does not wait for it, and saves the
        // state the last tick that changed the rack left.
        let Ok(rack) = self.shared.rack.try_lock() else {
            return;
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
        requests.push(Request::Restore(Saved::read(&json)));
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
    kontra_host::browser::Browser::open(config.as_deref(), cache.as_deref())
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
        let mut report = Vec::new();
        let mut browser = lock(&p.shared.browser);
        let browser = browser.get_or_insert_with(browse);
        browser.poll();
        for r in requests {
            // Done, or not a load after all (a full turn, no such preset).
            let queued = r.part();
            match r {
                Request::Load { part, path } => {
                    let loaded = match part.or_else(|| rack.free()) {
                        Some(part) => rack.load(part, &path),
                        None => Err("every part is in use".into()),
                    };
                    if let Err(e) = loaded {
                        report.push(format!("{}: {e}", path.display()));
                    }
                }
                Request::Restore(saved) => {
                    for (part, e) in rack.restore(&saved) {
                        report.push(format!("part {}: {e}", part + 1));
                    }
                }
                Request::Ui(edit) => editor::apply(rack, edit),
                Request::Browse(action) => editor::browse(rack, browser, action, &mut report),
                Request::AddRoot(path) => browser.add_root(path),
            }
            if let Some(part) = queued {
                rack.loads().set(part, Load::Ready);
            }
        }
        if let Some(path) = p.shared.folder.picked() {
            browser.add_root(path);
        }
        let latency = align::update(p.align.value(), rack);
        p.shared.latency.store(latency, Ordering::Relaxed);
        rack.tick();
        // What a save while the next tick is busy writes (`capture`).
        if changed && !requests_restoring(p) {
            *p.saved.write().unwrap_or_else(PoisonError::into_inner) = rack.save().write();
        }
        if lock(&p.shared.view).refresh(rack, browser, report) {
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
        s.until_tick = 0;
        let rate = c.sample_rate.round() as u32;
        let Ok(store) = Store::new(StoreConfig::DEFAULT) else {
            s.audio = None;
            return;
        };
        let (rack, audio) = Rack::new(rate, store, c.max_block_size);
        *lock(&p.shared.player) = Some(rack.player());
        *lock(&p.shared.loads) = rack.loads();
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
            if let EventBody::Transport(t) = e.body {
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
        let chunk = CHUNK.min(audio.max_block()).max(1);
        for start in (0..frames).step_by(chunk) {
            let len = chunk.min(frames - start);
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
            audio.process(len, &s.chunk);
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
