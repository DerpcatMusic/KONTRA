//! KONTRA v3: the plugin shell (07-APP.md §3). moose's CLAP and VST3 glue
//! over [`kontra_host::rack`] and the [`kontra_ui`] editor. The shell stamps
//! host events with their frame offset, hands them to the rack, copies the
//! rack's output pairs to the host's ports and keeps the project state.
//! It holds no engine logic of its own.
//!
//! Threads: the host's audio thread owns [`Dsp`] (the rack's audio side);
//! [`Tick`] runs on moose's task pool and owns the rack's control side
//! behind a mutex the audio thread never sees.

mod editor;
mod input;
#[cfg(test)]
mod tests;

use std::sync::{Mutex, MutexGuard, PoisonError, RwLock};

use kontra_host::rack::{Event, Rack, RackAudio, Store, StoreConfig};
use kontra_host::state::Saved;
use moose::prelude::*;

/// Stereo outputs the plugin offers: one per rack part.
pub const OUTPUTS: usize = kontra_host::rack::OUTPUTS;

/// Host events one block takes; more in one block are dropped and counted.
const EVENTS: usize = 4096;

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
    /// The rack as JSON ([`Saved`]): parts, presets, mix.
    #[persist = "rack"]
    pub saved: RwLock<String>,
    #[skip]
    pub shared: Shared,
    #[meter]
    pub level: MeterSlot,
}

use KontraParamsParamId as P;

/// What the control threads share. The audio thread never touches it.
#[derive(Default)]
pub struct Shared {
    rack: Mutex<Option<Rack>>,
    requests: Mutex<Vec<Request>>,
    /// The editor's copy of the rack, refreshed by [`Tick`].
    view: Mutex<editor::Snapshot>,
    dirty: std::sync::atomic::AtomicBool,
}

/// What the host or the editor asks of the rack, carried out by [`Tick`].
pub enum Request {
    Load {
        part: usize,
        path: std::path::PathBuf,
    },
    Restore(Saved),
    Ui(editor::Edit),
}

impl Shared {
    pub fn request(&self, r: Request) {
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
        if let Some(rack) = lock(&self.shared.rack).as_ref() {
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
        let mut requests = lock(&self.shared.requests);
        requests.retain(|r| !matches!(r, Request::Restore(_)));
        requests.push(Request::Restore(Saved::read(&json)));
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The audio thread's state.
#[derive(Default)]
pub struct Dsp {
    audio: Option<RackAudio>,
    input: Vec<(u32, Event)>,
    gain: f32,
    rate: f64,
    until_tick: usize,
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
        let mut report = Vec::new();
        for r in requests {
            match r {
                Request::Load { part, path } => {
                    if let Err(e) = rack.load(part, &path) {
                        report.push(format!("{}: {e}", path.display()));
                    }
                }
                Request::Restore(saved) => {
                    for (part, e) in rack.restore(&saved) {
                        report.push(format!("part {}: {e}", part + 1));
                    }
                }
                Request::Ui(edit) => editor::apply(rack, edit),
            }
        }
        rack.tick();
        lock(&p.shared.view).refresh(rack, report);
        p.shared
            .dirty
            .store(true, std::sync::atomic::Ordering::Release);
    }
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
        s.rate = c.sample_rate;
        s.gain = db_to_linear(p.volume.value());
        s.input = Vec::with_capacity(EVENTS);
        s.until_tick = 0;
        let rate = c.sample_rate.round() as u32;
        let Ok(store) = Store::new(StoreConfig::DEFAULT) else {
            s.audio = None;
            return;
        };
        let (rack, audio) = Rack::new(rate, store, c.max_block_size);
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
        s.input.clear();
        for e in events.iter() {
            if let EventBody::Transport(t) = e.body {
                audio.set_tempo(t.tempo as f32);
            } else if let Some(event) = input::event(e)
                && s.input.len() < s.input.capacity()
            {
                s.input.push((e.sample_offset, event));
            }
        }
        audio.process(frames, &s.input);
        // The master ramps from the last block's gain to this one's.
        let target = db_to_linear(p.volume.value());
        let step = (target - s.gain) / frames.max(1) as f32;
        let mut peak = 0.0f32;
        for ch in 0..b.num_output_channels() {
            let pair = audio.output(ch / 2, frames);
            let out = b.output(ch);
            out.fill(0.0);
            for (n, (o, frame)) in out.iter_mut().zip(pair).enumerate() {
                let x = frame.get(ch % 2).copied().unwrap_or(0.0);
                *o = x * (s.gain + step * n as f32);
                peak = peak.max(o.abs());
            }
        }
        s.gain = target;
        cx.set_meter(&p.level, peak.min(1.0));
        ProcessStatus::Normal
    }

    fn editor(params: std::sync::Arc<KontraParams>) -> Box<dyn Editor> {
        editor::editor(params)
    }
}

moose::plugin! { logic: Kontra, params: KontraParams, tasks: [Tick] }
