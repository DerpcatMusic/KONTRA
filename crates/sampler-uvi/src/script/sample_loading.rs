//! One bounded decode worker and one dedicated core transaction producer.
//! Lua objects stay on their owner; source bytes and registrations retire there.
use super::{ScriptHost, Shared, script_directory};
mod metrics;
pub use metrics::{SampleLoadTimings, SampleSourceTimings};
use metrics::{WorkerTimings, elapsed};
use std::time::Instant;
use mlua::{
    AnyUserData, Lua, MultiValue, Thread, UserData, UserDataFields, UserDataMethods, Value,
    Variadic,
};
use sampler_core::{
    Direction, HeadBudget, Loop, LoopMode, LoopShape, Pcm, Playback, PreparedSampleRevision,
    ResidentBuffer, SampleControl, SampleTargetId, SampleTask,
};
use std::{
    cell::Cell,
    collections::BTreeMap,
    sync::{Arc, mpsc},
    thread::JoinHandle,
};

/// Owner-thread task accounting. Success is counted only after a real core
/// commit reply. Pending includes metadata, decode, ready and submitted work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SampleLoadStatus {
    pub requested: usize,
    pub successful: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub pending: usize,
    pub queued_metadata: usize,
    pub worker_active: usize,
    pub ready: usize,
    pub submitted: usize,
    pub timings: SampleLoadTimings,
}
#[derive(Default)]
struct Counts {
    requested: Cell<usize>,
    successful: Cell<usize>,
    failed: Cell<usize>,
    cancelled: Cell<usize>,
    timings: Cell<SampleLoadTimings>,
}
impl ScriptHost {
    pub fn sample_load_status(&self) -> SampleLoadStatus {
        self.shared.sample_loads.borrow().status()
    }
}

/// Keep the caller's raw path until the owning resource authority can apply
/// its ordered lookup rules. A module-relative rewrite loses later candidates.
#[derive(Clone, Copy, Debug)]
pub struct SamplePath<'a> {
    pub path: &'a str,
    pub script_directory: Option<&'a str>,
}
pub type SampleSource = Arc<dyn Fn(SamplePath<'_>, bool) -> Result<SampleResource, String> + Send + Sync>;
/// Numeric file facts plus the same immutable authority used by page decoding.
pub struct SampleResource {
    pub source: Arc<dyn sampler_kontakt::AssetSource>,
    /// Reuse the reader opened during origin/header validation on the worker.
    pub reader: Option<sampler_kontakt::SampleReader>,
    pub playback: Playback,
    pub base_note: u8,
    pub fine_tune: f64,
    pub geometry_error: Option<String>,
    pub format_code: u8,
    pub channels: u16,
    pub bits: u16,
    pub tempo: f64,
    pub timings: SampleSourceTimings,
}
#[derive(Clone)]
struct Info {
    rate: u32,
    frames: usize,
    base: u8,
    tune: f64,
    looping: Option<Loop>,
    geometry_error: Option<String>,
    name: String,
    format: u8,
    channels: u16,
    bits: u16,
    tempo: f64,
}
struct Task {
    id: u64,
    status: Arc<SampleTask>,
}
impl UserData for Task {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("id", |_, t| Ok(t.id));
        f.add_field_method_get("finished", |_, t| Ok(t.status.finished()));
        f.add_field_method_get("success", |_, t| Ok(t.status.success()));
        f.add_field_method_get("progress", |_, t| {
            Ok(if t.status.finished() { 1. } else { 0. })
        });
        f.add_field_method_get("state", |_, t| {
            Ok(if t.status.cancelled() {
                "cancelled"
            } else if t.status.finished() {
                "finished"
            } else {
                "pending"
            })
        });
    }
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("cancel", |_, t, ()| {
            t.status.cancel();
            Ok(())
        });
    }
}
const MAX_TARGETS: usize = 65_536;
// The core permits 64 incoming and 64 returned transactions; one additional
// worker/ready payload is independent. Metadata is not a 64-target work queue.
const TRANSACTION_CREDITS: usize = 2 * 64 + 1;
const MAX_PATH_BYTES: usize = 8 << 20;
const MAX_RESIDENT: usize = 256 << 20;
const HEAD: usize = 8192;
#[derive(Clone, Copy)]
struct Recipe {
    residency: sampler_ir::AssetResidency,
    playback: Option<Playback>,
    reverse: bool,
}
fn recipe(node: roxmltree::Node<'_, '_>) -> Result<Recipe, String> {
    let number = |n: roxmltree::Node<'_, '_>, key: &str, default: f64| -> Result<f64, String> {
        let value = n.attribute(key).map_or(Ok(default), |v| {
            v.parse::<f64>()
                .map_err(|_| format!("invalid sample playback {key}"))
        })?;
        if !value.is_finite() {
            return Err(format!("invalid sample playback {key}"));
        }
        Ok(value)
    };
    let frame = |n, key| -> Result<usize, String> {
        let v = number(n, key, 0.)?;
        if v < 0. || v.fract() != 0. || v > usize::MAX as f64 {
            return Err(format!("invalid sample frame {key}"));
        }
        Ok(v as usize)
    };
    let residency = crate::residency_policy::sample(node)?;
    let reverse = number(node, "Reverse", 0.)? != 0.;
    let Some(options) = node.children().find(|n| n.has_tag_name("PlaybackOptions")) else {
        return Ok(Recipe {
            residency,
            playback: None,
            reverse,
        });
    };
    if number(options, "PlayDirection", 0.)? != 0. {
        return Err("unsupported authored sample PlayDirection".into());
    }
    let mut p = Playback {
        start: frame(options, "Start")?,
        end: options
            .attribute("Stop")
            .map(|_| frame(options, "Stop"))
            .transpose()?,
        direction: if reverse {
            Direction::Reverse
        } else {
            Direction::Forward
        },
        ..Default::default()
    };
    let mut loops = options.children().filter(|n| n.has_tag_name("Loop"));
    if let Some(l) = loops.next() {
        if number(l, "Type", 0.)? != 0. || l.attribute("Crossfade").is_some_and(|v| v != "0") {
            return Err("unsupported authored sample loop type/crossfade".into());
        }
        p.loop_range = Some(Loop {
            start: frame(l, "Start")?,
            end: frame(l, "End")?,
            shape: LoopShape::Wrap,
            mode: LoopMode::Continuous,
            passes: None,
        });
    }
    if loops.next().is_some() {
        return Err("unsupported additional authored sample loops".into());
    }
    Ok(Recipe {
        residency,
        playback: Some(p),
        reverse,
    })
}
struct Pending {
    id: u64,
    target: u32,
    path: String,
    script_directory: Option<String>,
    queued_at: Instant,
    prepared_at: Option<Instant>,
    submitted_at: Option<Instant>,
    status: Arc<SampleTask>,
    task: Option<AnyUserData>,
    callback: Option<Thread>,
}
impl Pending {
    fn path_bytes(&self) -> usize {
        self.path.len() + self.script_directory.as_ref().map_or(0, String::len)
    }
}
struct Request {
    id: u64,
    target: u32,
    path: String,
    script_directory: Option<String>,
    queued_at: Instant,
    status: Arc<SampleTask>,
    source: SampleSource,
    recipe: Recipe,
    rate: u32,
    dynamic: Option<sampler_kontakt::DynamicSources>,
    head_budget: HeadBudget,
}
struct Response {
    id: u64,
    completed_at: Instant,
    timings: WorkerTimings,
    result: Result<(PreparedSampleRevision, Info), String>,
}
struct Worker {
    send: Option<mpsc::SyncSender<Request>>,
    receive: mpsc::Receiver<Response>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    fn new() -> Result<Self, String> {
        let (send, requests) = mpsc::sync_channel::<Request>(1);
        let (replies, receive) = mpsc::sync_channel(1);
        let owner = std::thread::current();
        let thread = std::thread::Builder::new()
            .name("uvi-sample-load".into())
            .spawn(move || {
                let mut page = vec![[0.; 2]; sampler_core::PAGE_FRAMES];
                while let Ok(r) = requests.recv() {
                    let id = r.id;
                    let mut timings = WorkerTimings::default();
                    let start = Instant::now();
                    let result = prepare(r, &mut page, &mut timings);
                    timings.prepare_ns = elapsed(start).saturating_sub(timings.resolve_ns);
                    if replies.send(Response { id, result, timings, completed_at: Instant::now() }).is_err() {
                        break;
                    }
                    owner.unpark();
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            send: Some(send),
            receive,
            thread: Some(thread),
        })
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.send.take(); /* Never join a blocked I/O read on the owner. */
        self.thread.take();
    }
}
fn prepare(
    r: Request,
    page: &mut [sampler_core::Frame],
    trace: &mut WorkerTimings,
) -> Result<(PreparedSampleRevision, Info), String> {
    if r.status.cancelled() {
        return Err("sample load cancelled".into());
    }
    trace.queue_ns = elapsed(r.queued_at);
    let started = Instant::now();
    let found = (r.source)(SamplePath { path: &r.path, script_directory: r.script_directory.as_deref() }, r.recipe.playback.is_some());
    trace.resolve_ns = elapsed(started);
    let mut resource = found.map_err(|e|format!("sample source resolution for '{}': {e}", r.path))?;
    trace.source = resource.timings;
    let mut reader = match resource.reader.take() {
        Some(reader) => reader,
        None => {
            let started = Instant::now();
            let opened = resource.source.open();
            trace.source.reader_open_ns = trace.source.reader_open_ns.saturating_add(elapsed(started));
            opened.map_err(|e|format!("sample reader open: {e}"))?
        },
    };
    let (rate, frames) = (reader.rate(), reader.frames());
    let info = Info {
        rate,
        frames,
        base: resource.base_note,
        tune: resource.fine_tune,
        looping: resource.playback.loop_range,
        geometry_error: resource.geometry_error,
        name: r.path.rsplit('/').next().unwrap_or(&r.path).to_owned(),
        format: resource.format_code,
        channels: resource.channels,
        bits: resource.bits,
        tempo: resource.tempo,
    };
    let mut playback = r.recipe.playback.unwrap_or(resource.playback);
    if r.recipe.reverse {
        playback.direction = Direction::Reverse;
    }
    PreparedSampleRevision::validate_geometry(frames, rate, playback, r.rate)
        .map_err(|e|format!("sample playback geometry: frames={frames} rate={rate} output={} start={} end={:?} loop={:?} direction={:?}: {e}",r.rate,playback.start,playback.end,playback.loop_range,playback.direction))?;
    let stream = r.dynamic.is_some() && r.recipe.residency != sampler_ir::AssetResidency::Resident;
    let pcm = if stream {
        Pcm::streamed(rate, frames).map_err(|e| e.to_string())?
    } else {
        let bytes = frames.checked_mul(8).ok_or("sample decode byte overflow")?;
        if bytes > MAX_RESIDENT {
            return Err("sample exceeds resident revision decode budget".into());
        }
        let mut out =
            ResidentBuffer::new(rate, frames, &r.head_budget).map_err(|e| e.to_string())?;
        for (index, block) in out.frames_mut().chunks_mut(HEAD).enumerate() {
            if r.status.cancelled() {
                return Err("sample load cancelled".into());
            }
            reader
                .read(index * HEAD, block)
                .map_err(|e| e.to_string())?;
            if r.status.cancelled() {
                return Err("sample load cancelled".into());
            }
        }
        out.finish().map_err(|e| e.to_string())?
    };
    let mut revision = PreparedSampleRevision::new(SampleTargetId(r.target), pcm, playback, r.rate)
        .map_err(|e| e.to_string())?;
    if let Some(dynamic) = r.dynamic.filter(|_| stream) {
        // Include the interpolation guards and both ends of a short loop.
        let end = playback.end.unwrap_or(frames);
        let guard = sampler_core::read_radius(16.).saturating_add(1);
        let first = if playback.direction == Direction::Reverse {
            end.saturating_sub(HEAD)
        } else {
            playback.start.saturating_sub(guard)
        };
        let last = if playback.direction == Direction::Reverse {
            end
        } else {
            (playback.start + HEAD).min(end)
        };
        let mut ranges = vec![first..last];
        if let Some(l) = playback.loop_range {
            ranges.push(l.start.saturating_sub(guard)..(l.start + HEAD).min(frames));
            ranges.push(l.end.saturating_sub(HEAD)..l.end);
        }
        ranges.sort_by_key(|r| r.start);
        let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
        for range in ranges {
            if range.is_empty() {
                continue;
            }
            if let Some(old) = merged.last_mut().filter(|old| old.end >= range.start) {
                old.end = old.end.max(range.end);
            } else {
                merged.push(range);
            }
        }
        revision
            .asset()
            .bind_head_budget(&r.head_budget)
            .map_err(|e| e.to_string())?;
        let mut builder = revision.asset().head_preparer().builder();
        for range in merged {
            let mut position = range.start;
            while position < range.end {
                if r.status.cancelled() {
                    return Err("sample load cancelled".into());
                }
                let frames = &mut page[..(range.end - position).min(sampler_core::PAGE_FRAMES)];
                reader.read(position, frames).map_err(|e| e.to_string())?;
                if r.status.cancelled() {
                    return Err("sample load cancelled".into());
                }
                builder.push(position, frames).map_err(|e| {
                    if e == sampler_core::Error::Capacity {
                        let budget = r.head_budget.status();
                        format!(
                            "dynamic sample head budget exceeded: limit={} held={} bytes",
                            budget.limit_bytes, budget.held_bytes
                        )
                    } else {
                        e.to_string()
                    }
                })?;
                position += frames.len();
            }
        }
        if r.status.cancelled() {
            return Err("sample load cancelled".into());
        }
        revision
            .asset()
            .publish_ranges(builder.finish())
            .map_err(|e| e.to_string())?;
        // Recompile the byte receipt after heads were published off audio.
        let pcm = revision.asset().clone();
        revision = PreparedSampleRevision::new(SampleTargetId(r.target), pcm, playback, r.rate)
            .map_err(|e| e.to_string())?
            .with_lifetime(
                dynamic
                    .register(revision.asset(), resource.source)
                    .map_err(|e| e.to_string())?,
            );
    }
    if r.status.cancelled() {
        return Err("sample load cancelled".into());
    }
    Ok((revision, info))
}

pub(super) struct SampleLoads {
    source: Option<SampleSource>,
    recipes: BTreeMap<u32, Result<Recipe, String>>,
    rate: u32,
    control: Option<SampleControl>,
    dynamic: Option<sampler_kontakt::DynamicSources>,
    worker: Option<Worker>,
    pending: BTreeMap<u64, Pending>,
    pending_targets: BTreeMap<u32, u64>,
    active: Option<Pending>,
    ready: Option<(Pending, PreparedSampleRevision, Info)>,
    submitted: BTreeMap<u64, (Pending, Info)>,
    paths: usize,
    committed: BTreeMap<u32, Info>,
    latest: BTreeMap<u32, Arc<SampleTask>>,
    next_collect: f64,
    counts: Counts,
}
impl SampleLoads {
    pub fn new(doc: &roxmltree::Document<'_>, source: Option<SampleSource>, rate: u32) -> Self {
        let recipes = doc
            .descendants()
            .filter(|n| n.has_tag_name("SamplePlayer"))
            .map(|n| (n.id().get_usize() as u32, recipe(n)))
            .collect();
        Self {
            source,
            recipes,
            rate,
            control: None,
            dynamic: None,
            worker: None,
            pending: BTreeMap::new(),
            pending_targets: BTreeMap::new(),
            active: None,
            ready: None,
            submitted: BTreeMap::new(),
            paths: 0,
            committed: BTreeMap::new(),
            latest: BTreeMap::new(),
            next_collect: 0.,
            counts: Counts::default(),
        }
    }
    pub fn refresh_initial(&mut self, doc: &roxmltree::Document<'_>) -> Result<(),String> {
        if self.control.is_some() || self.worker.is_some() || self.active.is_some() || self.ready.is_some() || !self.submitted.is_empty() {
            return Err("initial sample recipes cannot change after service binding".into());
        }
        let recipes: BTreeMap<_,_> = doc.descendants().filter(|n|n.has_tag_name("SamplePlayer"))
            .map(|n|(n.id().get_usize() as u32,recipe(n))).collect();
        if !self.recipes.keys().eq(recipes.keys()) {
            return Err("initial sample target identities changed".into());
        }
        self.recipes=recipes;
        Ok(())
    }
    pub fn bind(&mut self, mut control: SampleControl) -> Result<(), String> {
        if self.control.is_some() {
            return Err("sample control already bound".into());
        }
        control
            .set_wake(std::thread::current())
            .map_err(|e| e.to_string())?;
        self.worker = Some(Worker::new()?);
        self.control = Some(control);
        Ok(())
    }
    pub fn collection_timer(&self) -> Option<std::time::Duration> {
        (self.control.is_some() && !self.committed.is_empty())
            .then_some(std::time::Duration::from_millis(250))
    }
    pub fn stream(&mut self, dynamic: sampler_kontakt::DynamicSources) -> Result<(), String> {
        if self.control.is_some() {
            return Err("sample registry must bind before control".into());
        }
        self.dynamic = Some(dynamic);
        Ok(())
    }
    fn status(&self) -> SampleLoadStatus {
        let queued_metadata = self.pending.len();
        let worker_active = usize::from(self.active.is_some());
        let ready = usize::from(self.ready.is_some());
        let submitted = self.submitted.len();
        SampleLoadStatus {
            requested: self.counts.requested.get(),
            successful: self.counts.successful.get(),
            failed: self.counts.failed.get(),
            cancelled: self.counts.cancelled.get(),
            pending: queued_metadata + worker_active + ready + submitted,
            queued_metadata,
            worker_active,
            ready,
            submitted,
            timings: self.counts.timings.get(),
        }
    }
    fn finish(shared: &Shared, counts: &Counts, mut pending: Pending, error: Option<&str>) {
        let mut timings = counts.timings.get();
        timings.total.add(elapsed(pending.queued_at));
        counts.timings.set(timings);
        if let Some(error) = error {
            pending.status.fail();
            shared.find("lua loadSample failed", error);
        }
        let count = if pending.status.cancelled() {
            &counts.cancelled
        } else if pending.status.success() {
            &counts.successful
        } else {
            &counts.failed
        };
        count.set(count.get().saturating_add(1));
        if let (Some(thread), Some(task)) = (pending.callback.take(), pending.task.take()) {
            shared.deferred.borrow_mut().push((
                thread,
                MultiValue::from_vec(vec![Value::UserData(task)]),
                None,
            ));
        }
    }
    fn enqueue(&mut self, shared: &Shared, pending: Pending) {
        self.counts
            .requested
            .set(self.counts.requested.get().saturating_add(1));
        if self.source.is_none() || !self.recipes.contains_key(&pending.target) {
            Self::finish(
                shared,
                &self.counts,
                pending,
                Some("sample source authority/oscillator kind unavailable"),
            );
            return;
        }
        if pending.path.len() > 4096 || pending.script_directory.as_ref().is_some_and(|p| p.len() > 4096) {
            Self::finish(
                shared,
                &self.counts,
                pending,
                Some("sample load path capacity exceeded"),
            );
            return;
        }
        // Supersession precedes admission: canceled metadata must not consume
        // an authored target's slot or path bytes. Every old Task still receives
        // its callback, with the same canceled status and object identity.
        if let Some(older) = self.latest.get(&pending.target) {
            older.cancel();
        }
        if let Some(id) = self.pending_targets.remove(&pending.target) {
            let older = self
                .pending
                .remove(&id)
                .expect("pending target index is exact");
            older.status.cancel();
            self.paths -= older.path_bytes();
            Self::finish(shared, &self.counts, older, None);
        }
        let capacity = self
            .recipes
            .len()
            .min(MAX_TARGETS)
            .saturating_add(TRANSACTION_CREDITS);
        if self.pending.len()
            + self.submitted.len()
            + usize::from(self.active.is_some())
            + usize::from(self.ready.is_some())
            >= capacity
            || self.paths.saturating_add(pending.path_bytes()) > MAX_PATH_BYTES
        {
            Self::finish(
                shared,
                &self.counts,
                pending,
                Some("sample load request capacity exceeded"),
            );
            return;
        }
        self.latest.insert(pending.target, pending.status.clone());
        self.paths += pending.path_bytes();
        self.pending_targets.insert(pending.target, pending.id);
        self.pending.insert(pending.id, pending);
    }
    pub fn poll(&mut self, shared: &Shared) {
        if let Some(control) = self.control.as_mut() {
            for _ in 0..64 {
                let Some(reply) = control.reply() else {
                    break;
                };
                if let Some((pending, info)) = self.submitted.remove(&reply.sequence) {
                    let mut timings = self.counts.timings.get();
                    timings.commit_ack.add(elapsed(pending.submitted_at.expect("submitted clock")));
                    self.counts.timings.set(timings);
                    self.paths -= pending.path_bytes();
                    if reply.result.is_ok() {
                        if let Some(error) = &info.geometry_error {
                            shared.find("sampleInfo file geometry unsupported", error);
                        }
                        self.committed.insert(pending.target, info);
                        Self::finish(shared, &self.counts, pending, None);
                    } else if pending.status.cancelled() {
                        Self::finish(shared, &self.counts, pending, None);
                    } else {
                        Self::finish(
                            shared,
                            &self.counts,
                            pending,
                            Some(&format!("source commit {:?}", reply.result)),
                        );
                    }
                }
            }
            if shared.now.get() >= self.next_collect {
                let _ = control.collect();
                self.next_collect = shared.now.get() + 250.;
            }
        }
        if let Some(worker) = &self.worker {
            if let Ok(response) = worker.receive.try_recv() {
                let mut pending = self
                    .active
                    .take()
                    .expect("one active bounded sample decode");
                debug_assert_eq!(pending.id, response.id);
                let mut timings = self.counts.timings.get();
                timings.worker_delivery.add(elapsed(response.completed_at));
                timings.worker(response.timings);
                self.counts.timings.set(timings);
                pending.prepared_at = Some(Instant::now());
                match response.result {
                    Ok((revision, info)) => self.ready = Some((pending, revision, info)),
                    Err(error) => {
                        self.paths -= pending.path_bytes();
                        let canceled = pending.status.cancelled();
                        Self::finish(
                            shared,
                            &self.counts,
                            pending,
                            (!canceled).then_some(error.as_str()),
                        );
                    }
                }
            }
        }
        if let Some((mut pending, revision, info)) = self.ready.take() {
            if pending.status.cancelled() {
                self.paths -= pending.path_bytes();
                Self::finish(shared, &self.counts, pending, None);
            } else if let Some(control) = self.control.as_mut() {
                match control.submit(revision, pending.status.clone()) {
                    Ok(sequence) => {
                        let mut timings = self.counts.timings.get();
                        timings.ready_submit.add(elapsed(pending.prepared_at.expect("prepared clock")));
                        self.counts.timings.set(timings);
                        pending.submitted_at = Some(Instant::now());
                        self.submitted.insert(sequence, (pending, info));
                    }
                    Err((sampler_core::Error::Capacity, revision, _)) => {
                        self.ready = Some((pending, revision, info));
                    }
                    Err((error, _, _)) => {
                        self.paths -= pending.path_bytes();
                        Self::finish(
                            shared,
                            &self.counts,
                            pending,
                            Some(&format!("source submission {error:?}")),
                        );
                    }
                }
            } else {
                self.ready = Some((pending, revision, info));
            }
        }
        if self.control.is_none() || self.active.is_some() || self.ready.is_some() {
            return;
        }
        // One worker receives one request; metadata/decode/registry I/O never runs on Lua.
        while let Some((_, pending)) = self.pending.pop_first() {
            self.pending_targets.remove(&pending.target);
            if pending.status.cancelled() {
                self.paths -= pending.path_bytes();
                Self::finish(shared, &self.counts, pending, None);
                continue;
            }
            let recipe = match &self.recipes[&pending.target] {
                Ok(r) => *r,
                Err(e) => {
                    self.paths -= pending.path_bytes();
                    Self::finish(shared, &self.counts, pending, Some(e));
                    continue;
                }
            };
            let request = Request {
                id: pending.id,
                target: pending.target,
                path: pending.path.clone(),
                script_directory: pending.script_directory.clone(),
                queued_at: pending.queued_at,
                status: pending.status.clone(),
                source: self.source.as_ref().unwrap().clone(),
                recipe,
                rate: self.rate,
                dynamic: self.dynamic.clone(),
                head_budget: self.control.as_ref().unwrap().head_budget(),
            };
            match self
                .worker
                .as_ref()
                .unwrap()
                .send
                .as_ref()
                .unwrap()
                .try_send(request)
            {
                Ok(()) => self.active = Some(pending),
                Err(_) => {
                    self.paths -= pending.path_bytes();
                    Self::finish(
                        shared,
                        &self.counts,
                        pending,
                        Some("sample decode worker disconnected"),
                    );
                }
            }
            break;
        }
    }
    pub fn info(&self, lua: &Lua, target: u32) -> mlua::Result<Option<mlua::Table>> {
        let Some(info) = self.committed.get(&target) else {
            return Ok(None);
        };
        let t = lua.create_table()?;
        let ms = |frame: usize| frame as f64 * 1000. / f64::from(info.rate);
        t.set("name", info.name.clone())?;
        t.set("type", info.format)?;
        t.set("channels", info.channels)?;
        t.set("duration", ms(info.frames))?;
        t.set("samplerate", info.rate)?;
        t.set("bits", info.bits)?;
        t.set("tempo", info.tempo)?;
        t.set("basenote", info.base)?;
        t.set("finetune", info.tune)?;
        if info.geometry_error.is_none() {
            t.set("start", 0)?;
            t.set("end", ms(info.frames))?;
            t.set("looped", info.looping.is_some())?;
            if let Some(l) = info.looping {
                t.set("loopStart", ms(l.start))?;
                t.set("loopEnd", ms(l.end))?;
            }
        }
        Ok(Some(t))
    }
}
impl Drop for SampleLoads {
    fn drop(&mut self) {
        for p in self
            .pending
            .values()
            .chain(self.active.iter())
            .chain(self.ready.iter().map(|r| &r.0))
            .chain(self.submitted.values().map(|r| &r.0))
        {
            p.status.cancel();
        }
    }
}
pub(super) fn install(lua: &Lua, shared: &std::rc::Rc<Shared>) -> mlua::Result<()> {
    let s = shared.clone();
    lua.globals().raw_set(
        "loadSample",
        lua.create_function(move |lua, args: Variadic<Value>| {
            let id = match args.first() {
                Some(Value::Table(t)) => t.raw_get::<Option<usize>>("__id")?,
                _ => None,
            };
            let target = id.and_then(|id| s.nodes.borrow().get(id).and_then(|n| n.0).and_then(|n|u32::try_from(n).ok()));
            let path = match args.get(1) {
                Some(Value::String(p)) => p.to_str()?.to_owned(),
                _ => String::new(),
            };
            let callback = args.get(2).and_then(|v| {
                if let Value::Function(f) = v {
                    Some(f.clone())
                } else {
                    None
                }
            });
            let status = Arc::new(SampleTask::default());
            let next = s.next_id();
            let task = callback
                .as_ref()
                .map(|_| {
                    lua.create_userdata(Task {
                        id: next,
                        status: status.clone(),
                    })
                })
                .transpose()?;
            let thread = callback.map(|f| lua.create_thread(f)).transpose()?;
            let pending = Pending {
                id: next,
                target: target.unwrap_or(u32::MAX),
                path,
                script_directory: script_directory(lua),
                queued_at: Instant::now(),
                prepared_at: None,
                submitted_at: None,
                status,
                task: task.clone(),
                callback: thread,
            };
            s.sample_loads.borrow_mut().enqueue(&s, pending);
            Ok(task)
        })?,
    )?;
    Ok(())
}
pub(super) fn poll(host: &ScriptHost) {
    host.shared.sample_loads.borrow_mut().poll(&host.shared);
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Memory {
        cancel: Option<Arc<SampleTask>>,
        fail: bool,
    }
    impl sampler_kontakt::AssetSource for Memory {
        fn open(&self) -> std::io::Result<sampler_kontakt::SampleReader> {
            let cancel = self.cancel.clone();
            let fail = self.fail;
            Ok(sampler_kontakt::SampleReader::custom(
                48000,
                16,
                move |_, out| {
                    if fail {
                        return Err(std::io::Error::other("authored fixture source failure"));
                    }
                    out.fill([0.5; 2]);
                    if let Some(task) = &cancel {
                        task.cancel();
                    }
                    Ok(())
                },
            ))
        }
    }
    fn request(budget: HeadBudget, cancel_after_read: bool) -> Request {
        let status = Arc::new(SampleTask::default());
        let source: Arc<dyn sampler_kontakt::AssetSource> = Arc::new(Memory {
            cancel: cancel_after_read.then(|| status.clone()),
            fail: false,
        });
        let authority: SampleSource = Arc::new(move |_, _| {
            Ok(SampleResource {
                source: source.clone(),
                reader: None,
                playback: Playback::default(),
                base_note: 60,
                fine_tune: 0.,
                geometry_error: None,
                format_code: 1,
                channels: 2,
                bits: 16,
                tempo: 0.,
                timings: Default::default(),
            })
        });
        Request {
            id: 1,
            target: 7,
            path: "memory.wav".into(),
            script_directory: None,
            queued_at: Instant::now(),
            status,
            source: authority,
            recipe: Recipe {
                residency: Default::default(),
                playback: None,
                reverse: false,
            },
            rate: 48000,
            dynamic: Some(Default::default()),
            head_budget: budget,
        }
    }
    #[test]
    fn actual_source_worker_enforces_exact_physical_head_quota_before_publication() {
        for (limit, success) in [(0, false), (31, false), (32, true)] {
            let budget = HeadBudget::new(limit);
            let result = prepare(
                request(budget.clone(), false),
                &mut [[0.; 2]; sampler_core::PAGE_FRAMES],
                &mut WorkerTimings::default(),
            );
            assert_eq!(result.is_ok(), success);
            match result {
                Ok((revision, _)) => {
                    assert_eq!(revision.asset().head_bytes(), 32);
                    assert!(revision.asset().resident_frames().is_none());
                    assert_eq!(budget.status().held_bytes, 32);
                    drop(revision);
                }
                Err(error) => assert!(
                    error.contains("dynamic sample head budget exceeded"),
                    "{error}"
                ),
            }
            assert_eq!(budget.status().held_bytes, 0);
            assert_eq!(budget.status().peak_bytes, if success { 32 } else { 0 });
        }
    }
    #[test]
    fn cancellation_after_worker_read_releases_staging_without_source_registration_or_success() {
        let budget = HeadBudget::new(32);
        let request = request(budget.clone(), true);
        let task = request.status.clone();
        let error = prepare(request, &mut [[0.; 2]; sampler_core::PAGE_FRAMES], &mut WorkerTimings::default())
            .err()
            .unwrap();
        assert!(error.contains("cancelled"));
        assert!(task.cancelled() && !task.success());
        assert_eq!(budget.status().held_bytes, 0);
        assert_eq!(budget.status().peak_bytes, 0);
    }
    #[test]
    fn full_resident_worker_preflights_geometry_and_reserves_before_decode() {
        for (limit, success) in [(0, false), (127, false), (128, true)] {
            let budget = HeadBudget::new(limit);
            let mut r = request(budget.clone(), false);
            r.dynamic = None;
            let result = prepare(r, &mut [[0.; 2]; sampler_core::PAGE_FRAMES], &mut WorkerTimings::default());
            assert_eq!(result.is_ok(), success);
            if let Ok((revision, _)) = result {
                assert_eq!(
                    &*revision.asset().resident_frames().unwrap(),
                    &[[0.5; 2]; 16]
                );
                assert_eq!(budget.status().held_bytes, 128);
                drop(revision);
            }
            assert_eq!(budget.status().held_bytes, 0);
            assert_eq!(budget.status().peak_bytes, if success { 128 } else { 0 });
        }
        let budget = HeadBudget::new(128);
        let mut r = request(budget.clone(), false);
        r.dynamic = None;
        r.recipe.playback = Some(Playback {
            start: 17,
            ..Default::default()
        });
        assert!(prepare(r, &mut [[0.; 2]; sampler_core::PAGE_FRAMES], &mut WorkerTimings::default()).is_err());
        assert_eq!(
            budget.status().peak_bytes,
            0,
            "invalid geometry must fail before allocation"
        );
    }
    #[test]
    fn full_resident_source_error_and_cancel_release_the_actual_preallocated_buffer() {
        for cancel in [false, true] {
            let budget = HeadBudget::new(128);
            let mut r = request(budget.clone(), cancel);
            r.dynamic = None;
            if !cancel {
                let original = r.source.clone();
                r.source = Arc::new(move |path, explicit| {
                    let mut resource = original(path, explicit)?;
                    resource.source = Arc::new(Memory {
                        cancel: None,
                        fail: true,
                    });
                    Ok(resource)
                });
            }
            let task = r.status.clone();
            assert!(prepare(r, &mut [[0.; 2]; sampler_core::PAGE_FRAMES], &mut WorkerTimings::default()).is_err());
            assert!(!task.success());
            assert_eq!(task.cancelled(), cancel);
            assert_eq!(budget.status().held_bytes, 0);
            assert_eq!(budget.status().peak_bytes, 128);
        }
    }
}
