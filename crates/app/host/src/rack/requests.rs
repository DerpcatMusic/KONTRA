//! What the parts' scripts ask the control side to build, off the audio
//! thread: effect units, and plans with their zone edits applied.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use kontra_engine::{Engine, Loop, Prepared, PreparedRacks, Region, Zone, ZoneEdit, ir};
use kontra_format::io::{ReadAt, SliceReader, TransformedReader};
use kontra_lua::{
    EffectOp, EffectTarget, KeygroupParamKind, MAX_INLINE_SLICES, OscParamKind, RuntimeChannel,
    RuntimeReply, RuntimeRequest,
};
use kontra_protocol::Ring;
use kontra_stream::{BlobKey, Files, LoadRequest, Sample, StreamClient};

use super::{Command, Plan, Rack};

/// Cached runtime-loaded sample and metadata for one `(file, offset)` member.
#[derive(Clone)]
pub(super) struct CachedSample {
    pub(super) sample: Arc<Sample>,
    pub(super) frames: u64,
    pub(super) rate: u32,
    pub(super) loops: Vec<ir::Loop>,
    pub(super) slices: Vec<(u64, u64)>,
}

/// Control-thread runtime swap state for one rack part's UVI Lua scripts.
#[derive(Default)]
pub(super) struct PartLuaRuntime {
    channels: Vec<(Arc<RuntimeChannel>, String)>,
    library: Option<kontra_format_uvi::RuntimeLibrary>,
    sample_cache: HashMap<(PathBuf, u64), CachedSample>,
    slot_sample: HashMap<(u32, u16), (PathBuf, u64)>,
    pending_replies: Vec<(usize, RuntimeReply)>,
}

impl PartLuaRuntime {
    pub(super) fn reset(&mut self) {
        for (ch, _) in &self.channels {
            ch.disconnect();
        }
        self.channels.clear();
        self.library = None;
        self.sample_cache.clear();
        self.slot_sample.clear();
        self.pending_replies.clear();
    }

    pub(super) fn attach(
        &mut self,
        channels: Vec<(Arc<RuntimeChannel>, String)>,
        slot_samples: HashMap<(u32, u16), (PathBuf, u64)>,
    ) {
        self.reset();
        for (ch, _) in &channels {
            ch.connect();
        }
        self.channels = channels;
        self.slot_sample = slot_samples;
    }
}

/// A part's plan with its scripts' zone edits applied, each with whether it
/// took. It comes back from the audio side holding the plan it replaced.
pub struct ZoneSwap {
    pub plan: Arc<Plan>,
    pub edits: Box<[(ZoneEdit, bool)]>,
    /// Optional off-thread pre-built rack/chain replacements swapped in
    /// atomically alongside `plan`.
    pub racks: Option<PreparedRacks>,
}

/// One or more purged groups whose heads were read in a batch.
pub struct Woken {
    pub part: usize,
    pub generation: u64,
    pub unpurges: Vec<kontra_engine::Unpurge>,
}

/// A part's (index, load) edits since the last tick, each with whether it took.
type Batch = (usize, u64, Vec<(ZoneEdit, bool)>);

/// The zone-edit channels between the rack and its audio side.
pub struct ZoneRings {
    /// Zone edits the parts' scripts made: (part, its load, edit).
    pub edits: Ring<(usize, u64, ZoneEdit)>,
    /// Swaps done or refused, to free here.
    pub done: Ring<Box<ZoneSwap>>,
}

impl ZoneRings {
    pub fn new(parts: usize) -> Self {
        Self {
            // A callback's edits arrive in one block: an engine queues 1024.
            edits: Ring::new(1024 * parts),
            done: Ring::new(4 * parts),
        }
    }
}

impl Rack {
    /// Builds what scripts asked for since the last tick and sends it in.
    pub(super) fn script_requests(&mut self) {
        self.effect_requests();
        self.zone_edits();
        self.unpurges();
        self.user_zones();
        self.lua_runtime_requests();
    }

    /// Flushes queued Lua runtime replies once all pending audio commands
    /// (including `Command::Zones`) have been accepted into the command ring.
    pub(super) fn flush_lua_replies(&mut self) {
        if !self.pending.is_empty() {
            return;
        }
        for rt in &mut self.lua_runtime {
            if rt.pending_replies.is_empty() {
                continue;
            }
            let queued = std::mem::take(&mut rt.pending_replies);
            for (idx, reply) in queued {
                let sent = rt
                    .channels
                    .get(idx)
                    .is_some_and(|(ch, _)| ch.push_reply(reply));
                if !sent {
                    rt.pending_replies.push((idx, reply));
                }
            }
        }
    }

    /// Services lock-free UVI Lua runtime swap requests (`loadSample`,
    /// `loadMapping`, `setPlaybackOptions`, live oscillator/keygroup parameter
    /// writes, and dynamic `insertEffect`/`removeEffect`/`moveEffect`) off the
    /// audio thread and atomically swaps the updated `Plan` onto audio.
    fn lua_runtime_requests(&mut self) {
        let part_count = self.parts.len().min(self.lua_runtime.len());
        for part in 0..part_count {
            let mut drained: Vec<(usize, RuntimeRequest)> = Vec::new();
            if let Some(rt) = self.lua_runtime.get(part) {
                for (rt_idx, (ch, _)) in rt.channels.iter().enumerate() {
                    while let Some(req) = ch.pop_request() {
                        drained.push((rt_idx, req));
                    }
                }
            }
            if drained.is_empty() {
                continue;
            }
            let Some((generation, source_path, base_plan)) =
                self.parts.get(part).and_then(Option::as_ref).and_then(|p| {
                    Some((
                        p.script.generation,
                        p.source.clone().or_else(|| p.path.clone()),
                        p.plan.clone()?,
                    ))
                })
            else {
                continue;
            };

            let Some(rt) = self.lua_runtime.get_mut(part) else {
                continue;
            };
            if rt.library.is_none()
                && let Some(path) = source_path.as_deref()
            {
                rt.library = kontra_format_uvi::RuntimeLibrary::open(path);
            }

            // Deduplicate repeated LoadSample calls to the same (group, slot)
            // within a single tick: only the last sample needs disk/plan work.
            let mut last_sample_idx: HashMap<(u32, u16), usize> = HashMap::new();
            for (i, (_, req)) in drained.iter().enumerate() {
                if let RuntimeRequest::LoadSample { group, slot, .. } = *req {
                    last_sample_idx.insert((group, slot), i);
                }
            }

            // Step 1: Resolve unique LoadSample paths once.
            let mut resolved_samples: HashMap<
                (usize, String),
                Result<kontra_format_uvi::RuntimeSample, String>,
            > = HashMap::new();
            for (i, (rt_idx, req)) in drained.iter().enumerate() {
                let RuntimeRequest::LoadSample {
                    group, slot, path, ..
                } = req
                else {
                    continue;
                };
                if last_sample_idx.get(&(*group, *slot)) != Some(&i) {
                    continue;
                }
                let key = (*rt_idx, path.as_str().to_owned());
                if resolved_samples.contains_key(&key) {
                    continue;
                }
                let script_dir = rt.channels.get(*rt_idx).map_or("", |(_, dir)| dir.as_str());
                let res = match rt.library.as_ref() {
                    Some(lib) => lib.sample(script_dir, path.as_str()),
                    None => resolve_disk_fallback(path.as_str()),
                };
                resolved_samples.insert(key, res);
            }

            // Step 2: Determine which resolved samples actually need loading
            // into `self.store` (skipping cached samples and no-op echoes).
            let mut to_load: Vec<(PathBuf, u64, ir::Sample, Vec<ir::Loop>)> = Vec::new();
            let mut to_load_seen: HashMap<(PathBuf, u64), ()> = HashMap::new();
            for (i, (rt_idx, req)) in drained.iter().enumerate() {
                let RuntimeRequest::LoadSample {
                    group,
                    slot,
                    path,
                    root,
                    sample_start,
                    reverse,
                    coarse_tune,
                    fine_tune,
                    gain,
                    ..
                } = req
                else {
                    continue;
                };
                if last_sample_idx.get(&(*group, *slot)) != Some(&i) {
                    continue;
                }
                let Some(Ok(rs)) = resolved_samples.get(&(*rt_idx, path.as_str().to_owned()))
                else {
                    continue;
                };
                let sample_key = (rs.sample.file.clone(), rs.sample.offset);
                let is_noop = is_load_sample_noop(
                    &base_plan,
                    &rt.slot_sample,
                    (*group, *slot),
                    &sample_key,
                    *root,
                    *sample_start,
                    *reverse,
                    *coarse_tune,
                    *fine_tune,
                    *gain,
                );
                if is_noop {
                    continue;
                }
                if !rt.sample_cache.contains_key(&sample_key)
                    && !to_load_seen.contains_key(&sample_key)
                {
                    to_load_seen.insert(sample_key.clone(), ());
                    to_load.push((
                        sample_key.0,
                        sample_key.1,
                        rs.sample.clone(),
                        rs.loops.clone(),
                    ));
                }
            }

            if !to_load.is_empty() {
                load_samples_into_cache(&mut self.store, &to_load, &mut rt.sample_cache);
            }

            // Step 3: Apply all requests to a lazily cloned `Plan`, collecting
            // replies and tracking which effect racks/chains need live swaps.
            let mut edited_plan: Option<Plan> = None;
            let mut program_dirty = false;
            let mut dirty_buses: Vec<usize> = Vec::new();
            let mut dirty_groups: Vec<usize> = Vec::new();
            let mut replies: Vec<(usize, RuntimeReply)> = Vec::new();
            let mut superseded_replies: Vec<(usize, u32, u32, u16)> = Vec::new();

            for (i, (rt_idx, req)) in drained.into_iter().enumerate() {
                match req {
                    RuntimeRequest::LoadSample {
                        id,
                        group,
                        slot,
                        path,
                        root,
                        sample_start,
                        reverse,
                        coarse_tune,
                        fine_tune,
                        gain,
                    } => {
                        if last_sample_idx.get(&(group, slot)) != Some(&i) {
                            superseded_replies.push((rt_idx, id, group, slot));
                            continue;
                        }
                        let Some(Ok(rs)) =
                            resolved_samples.get(&(rt_idx, path.as_str().to_owned()))
                        else {
                            replies.push((rt_idx, RuntimeReply::simple(id, false, 0, 0, 0)));
                            continue;
                        };
                        let sample_key = (rs.sample.file.clone(), rs.sample.offset);
                        let active_plan = edited_plan.as_ref().unwrap_or(&base_plan);
                        if is_load_sample_noop(
                            active_plan,
                            &rt.slot_sample,
                            (group, slot),
                            &sample_key,
                            root,
                            sample_start,
                            reverse,
                            coarse_tune,
                            fine_tune,
                            gain,
                        ) {
                            let slices = rt
                                .sample_cache
                                .get(&sample_key)
                                .map(|c| c.slices.as_slice())
                                .unwrap_or(&[]);
                            replies.push((
                                rt_idx,
                                make_reply(id, true, rs.sample.frames, rs.sample.rate, 1, slices),
                            ));
                            continue;
                        }
                        if !rt.sample_cache.contains_key(&sample_key) {
                            let single = vec![(
                                sample_key.0.clone(),
                                sample_key.1,
                                rs.sample.clone(),
                                rs.loops.clone(),
                            )];
                            load_samples_into_cache(&mut self.store, &single, &mut rt.sample_cache);
                        }
                        let Some(cached) = rt.sample_cache.get(&sample_key).cloned() else {
                            replies.push((rt_idx, RuntimeReply::simple(id, false, 0, 0, 0)));
                            continue;
                        };
                        let plan = edited_plan.get_or_insert_with(|| (*base_plan).clone());
                        apply_load_sample(
                            plan,
                            group,
                            slot,
                            &cached,
                            root,
                            sample_start,
                            reverse,
                            coarse_tune,
                            fine_tune,
                            gain,
                        );
                        rt.slot_sample.insert((group, slot), sample_key);
                        replies.push((
                            rt_idx,
                            make_reply(id, true, cached.frames, cached.rate, 1, &cached.slices),
                        ));
                    }
                    RuntimeRequest::LoadMapping {
                        id,
                        group,
                        slot,
                        path,
                    } => {
                        let script_dir = rt
                            .channels
                            .get(rt_idx)
                            .map_or("", |(_, dir)| dir.as_str())
                            .to_owned();
                        let mapped =
                            resolve_mapping(rt.library.as_ref(), &script_dir, path.as_str());
                        if mapped.is_empty() {
                            replies.push((rt_idx, RuntimeReply::simple(id, false, 0, 0, 0)));
                            continue;
                        }
                        let mut missing_cache = Vec::new();
                        let mut seen = HashMap::new();
                        for m in &mapped {
                            let key = (m.sample.sample.file.clone(), m.sample.sample.offset);
                            if !rt.sample_cache.contains_key(&key) && !seen.contains_key(&key) {
                                seen.insert(key.clone(), ());
                                missing_cache.push((
                                    key.0,
                                    key.1,
                                    m.sample.sample.clone(),
                                    m.sample.loops.clone(),
                                ));
                            }
                        }
                        if !missing_cache.is_empty() {
                            load_samples_into_cache(
                                &mut self.store,
                                &missing_cache,
                                &mut rt.sample_cache,
                            );
                        }
                        let mut loaded_entries = Vec::with_capacity(mapped.len());
                        for m in mapped {
                            let key = (m.sample.sample.file.clone(), m.sample.sample.offset);
                            if let Some(cached) = rt.sample_cache.get(&key).cloned() {
                                loaded_entries.push((m, cached));
                            }
                        }
                        if loaded_entries.is_empty() {
                            replies.push((rt_idx, RuntimeReply::simple(id, false, 0, 0, 0)));
                            continue;
                        }
                        let first_frames = loaded_entries.first().map_or(0, |(_, c)| c.frames);
                        let first_rate = loaded_entries.first().map_or(0, |(_, c)| c.rate);
                        let count = u16::try_from(loaded_entries.len()).unwrap_or(u16::MAX);
                        let plan = edited_plan.get_or_insert_with(|| (*base_plan).clone());
                        apply_load_mapping(plan, group, slot, &loaded_entries);
                        rt.slot_sample.remove(&(group, slot));
                        replies.push((
                            rt_idx,
                            RuntimeReply::simple(id, true, first_frames, first_rate, count),
                        ));
                    }
                    RuntimeRequest::OscParam {
                        group,
                        slot,
                        param,
                        value,
                    } => {
                        let plan = edited_plan.get_or_insert_with(|| (*base_plan).clone());
                        apply_osc_param(plan, group, slot, param, value);
                    }
                    RuntimeRequest::KeygroupParam {
                        group,
                        param,
                        value,
                    } => {
                        let plan = edited_plan.get_or_insert_with(|| (*base_plan).clone());
                        apply_keygroup_param(plan, group, param, value);
                    }
                    RuntimeRequest::PlaybackOptions {
                        id,
                        group,
                        slot,
                        sample_start,
                        loop_start,
                        loop_end,
                        loop_mode,
                        reverse,
                    } => {
                        let plan = edited_plan.get_or_insert_with(|| (*base_plan).clone());
                        let (ok, frames, rate) = apply_playback_options(
                            plan,
                            group,
                            slot,
                            sample_start,
                            loop_start,
                            loop_end,
                            loop_mode,
                            reverse,
                        );
                        replies.push((rt_idx, RuntimeReply::simple(id, ok, frames, rate, 1)));
                    }
                    RuntimeRequest::EffectChain { target, op } => {
                        let plan = edited_plan.get_or_insert_with(|| (*base_plan).clone());
                        apply_effect_chain(
                            plan,
                            target,
                            op,
                            &mut program_dirty,
                            &mut dirty_buses,
                            &mut dirty_groups,
                        );
                    }
                    RuntimeRequest::EffectParam {
                        target,
                        slot,
                        name,
                        value,
                    } => {
                        let plan = edited_plan.get_or_insert_with(|| (*base_plan).clone());
                        if apply_effect_param(plan, target, slot, name.as_str(), value) {
                            mark_effect_target_dirty(
                                target,
                                &mut program_dirty,
                                &mut dirty_buses,
                                &mut dirty_groups,
                            );
                        }
                    }
                }
            }

            // Answer any earlier LoadSample calls superseded within this batch.
            for (rt_idx, id, group, slot) in superseded_replies {
                let (frames, rate) = rt
                    .slot_sample
                    .get(&(group, slot))
                    .and_then(|k| rt.sample_cache.get(k))
                    .map_or((0, 0), |c| (c.frames, c.rate));
                replies.push((rt_idx, RuntimeReply::simple(id, true, frames, rate, 1)));
            }

            rt.pending_replies.extend(replies);

            let plan_swapped = edited_plan.is_some();
            if let Some(plan) = edited_plan {
                let racks = (program_dirty || !dirty_buses.is_empty() || !dirty_groups.is_empty())
                    .then(|| {
                        PreparedRacks::build(
                            &plan,
                            self.rate,
                            program_dirty,
                            &dirty_buses,
                            &dirty_groups,
                        )
                    })
                    .filter(|r| !r.is_empty());
                let plan = Arc::new(plan);
                if let Some(Some(p)) = self.parts.get_mut(part)
                    && p.script.generation == generation
                {
                    p.plan = Some(Arc::clone(&plan));
                    p.script.look_needed = true;
                    p.script.capture_needed = true;
                }
                let swap = Box::new(ZoneSwap {
                    plan,
                    edits: Box::default(),
                    racks,
                });
                self.send(Command::Zones {
                    part,
                    generation,
                    swap,
                });
            } else if let Some(Some(p)) = self.parts.get_mut(part)
                && p.script.generation == generation
            {
                p.script.look_needed = true;
                p.script.capture_needed = true;
            }

            if plan_swapped || self.pending.is_empty() {
                self.flush_lua_replies();
            }
        }
    }

    /// User zones whose sample a script set (in `on init`, or by a
    /// `set_sample` already applied): built here, off the audio thread, and
    /// swapped in between blocks.
    fn user_zones(&mut self) {
        for part in 0..self.parts.len() {
            let Some(Some(p)) = self.parts.get_mut(part) else {
                continue;
            };
            let Some(plan) = p.plan.as_deref().filter(|p| !p.user_zones.is_empty()) else {
                continue;
            };
            let Some((grown, diagnostics)) =
                kontra_prepare::user_zones(plan, &mut self.store, &|s| kontra_stream::open(s))
            else {
                continue;
            };
            let name = p.name.clone();
            self.notices.extend(
                diagnostics
                    .iter()
                    .map(|d| format!("part {} ({name}): {}: {}", part + 1, d.feature, d.detail)),
            );
            p.diagnostics.extend(diagnostics);
            let generation = p.script.generation;
            let plan = Arc::new(grown);
            p.plan = Some(Arc::clone(&plan));
            let swap = Box::new(ZoneSwap {
                plan,
                edits: Box::default(),
                racks: None,
            });
            self.send(Command::Zones {
                part,
                generation,
                swap,
            });
        }
    }

    /// Purged groups scripts asked for: pending unpurges for each part are
    /// batched into a single `Command::Zones` swap and a single background
    /// head-reading job instead of one-by-one roundtrips.
    fn unpurges(&mut self) {
        let mut batches: Vec<(usize, u64, Vec<kontra_engine::Unpurge>)> = Vec::new();
        while let Some((part, generation, u)) = self.channels.unpurges.recv() {
            match batches
                .iter_mut()
                .find(|b| (b.0, b.1) == (part, generation))
            {
                Some(b) => b.2.push(u),
                None => batches.push((part, generation, vec![u])),
            }
        }
        for (part, generation, requests) in batches {
            let Some(Some(p)) = self.parts.get_mut(part) else {
                continue;
            };
            let Some(base_plan) = p
                .plan
                .as_deref()
                .filter(|_| p.script.generation == generation)
            else {
                continue;
            };
            let mut wanted_groups: Vec<u32> = requests.iter().map(|u| u.group).collect();
            wanted_groups.sort_unstable();
            wanted_groups.dedup();
            let samples: Vec<_> = (base_plan.dormant.iter())
                .filter(|d| wanted_groups.binary_search(&d.group).is_ok())
                .flat_map(|d| d.samples.iter())
                .collect();
            let files = kontra_stream::Files::new(samples.iter().map(|s| s.file.as_path()));

            let mut grown_plan: Option<Plan> = None;
            let mut woken_groups: Vec<u32> = Vec::new();
            let mut pending_woken: Vec<kontra_engine::Unpurge> = Vec::new();
            let mut ready_now: Vec<kontra_engine::Unpurge> = Vec::new();

            for u in requests {
                let current = grown_plan.as_ref().unwrap_or(base_plan);
                let waked =
                    kontra_prepare::wake(current, u.group, &mut self.store, &|s| files.open(s));
                if let Some((grown, diagnostics, _count)) = waked {
                    for d in &diagnostics {
                        if d.impact != kontra_engine::ir::Impact::Latent {
                            self.notices.push(format!(
                                "part {} ({}): {}: {}",
                                part + 1,
                                p.name,
                                d.feature,
                                d.detail
                            ));
                        }
                    }
                    p.diagnostics.extend(diagnostics);
                    grown_plan = Some(grown);
                    woken_groups.push(u.group);
                    pending_woken.push(u);
                } else if woken_groups.contains(&u.group) {
                    // Duplicate request for a group woken earlier in this same batch:
                    // wait for the shared head-read job before completing.
                    pending_woken.push(u);
                } else {
                    // Already resident before this batch: complete immediately.
                    ready_now.push(u);
                }
            }

            if let Some(grown) = grown_plan {
                let grown = Arc::new(grown);
                p.plan = Some(Arc::clone(&grown));
                let swap = Box::new(ZoneSwap {
                    plan: grown,
                    edits: Box::default(),
                    racks: None,
                });
                self.send(Command::Zones {
                    part,
                    generation,
                    swap,
                });
                let done = self.woken.0.clone();
                let heads = self.store.take_heads();
                let lanes = std::thread::available_parallelism().map_or(4, |n| n.get() * 3);
                let fallback = pending_woken.clone();
                let read = move || {
                    if let Some(heads) = heads {
                        heads.load(lanes);
                    }
                    let _ = done.send(Woken {
                        part,
                        generation,
                        unpurges: pending_woken,
                    });
                };
                // A failed spawn leaves the heads to stream on demand: the groups
                // still play.
                if std::thread::Builder::new()
                    .name("kontra-unpurge".into())
                    .spawn(read)
                    .is_err()
                {
                    ready_now.extend(fallback);
                }
            }

            for u in ready_now {
                self.send(Command::Unpurged {
                    part,
                    generation,
                    unpurge: u,
                    took: true,
                });
            }
        }
        while let Ok(w) = self.woken.1.try_recv() {
            for u in w.unpurges {
                self.send(Command::Unpurged {
                    part: w.part,
                    generation: w.generation,
                    unpurge: u,
                    took: true,
                });
            }
        }
    }

    /// Effect types scripts loaded.
    fn effect_requests(&mut self) {
        while let Some((part, generation, request)) = self.channels.requests.recv() {
            let Some(Some(p)) = self.parts.get_mut(part) else {
                continue;
            };
            if p.script.generation != generation {
                continue;
            }
            let here = (request.at, request.slot);
            let (asked, ty) = (request.response.is_some(), request.ty);
            let unit = Prepared::new(request, p.plan.as_deref(), self.rate);
            // A response that fails to decode leaves the slot playing what it
            // had, so the save keeps what it had too.
            if let Some(name) = unit.response_name() {
                p.slot_responses.retain(|&(at, slot, _)| (at, slot) != here);
                p.slot_responses.push((here.0, here.1, name.to_owned()));
            } else if let Some(ty) = ty.filter(|_| !asked) {
                // None: the slot's own effect built again (a reverb's
                // network), which saves no type.
                p.slot_types.retain(|&(at, slot, _)| (at, slot) != here);
                p.slot_types.push((here.0, here.1, ty));
                p.slot_responses.retain(|&(at, slot, _)| (at, slot) != here);
            }
            let unit = Box::new(Some(unit));
            self.send(Command::Effect {
                part,
                generation,
                unit,
            });
        }
    }

    /// Each part's zone edits since the last tick, applied together to a
    /// copy of its plan (KSP manual: a callback's `set_zone_par` calls run
    /// together after it), which becomes the part's plan.
    fn zone_edits(&mut self) {
        while self.channels.zones.done.recv().is_some() {}
        let mut batches: Vec<Batch> = Vec::new();
        while let Some((part, generation, edit)) = self.channels.zones.edits.recv() {
            match batches
                .iter_mut()
                .find(|b| (b.0, b.1) == (part, generation))
            {
                Some(b) => b.2.push((edit, false)),
                None => batches.push((part, generation, vec![(edit, false)])),
            }
        }
        for (part, generation, mut edits) in batches {
            let Some(Some(p)) = self.parts.get_mut(part) else {
                continue;
            };
            let base = p
                .plan
                .as_deref()
                .filter(|_| p.script.generation == generation);
            let Some(mut plan) = base.cloned() else {
                continue;
            };
            for (e, took) in &mut edits {
                *took = e.apply(&mut plan);
            }
            // A user zone that got a sample plays it from this swap.
            if let Some((grown, diagnostics)) =
                kontra_prepare::user_zones(&plan, &mut self.store, &|s| kontra_stream::open(s))
            {
                plan = grown;
                p.diagnostics.extend(diagnostics);
            }
            // A zone moved into a wavetable group plays its table.
            let plan =
                Arc::new(kontra_prepare::tables(&plan, &self.store.client()).unwrap_or(plan));
            p.plan = Some(Arc::clone(&plan));
            let swap = Box::new(ZoneSwap {
                plan,
                edits: edits.into(),
                racks: None,
            });
            self.send(Command::Zones {
                part,
                generation,
                swap,
            });
        }
    }
}

/// Gives the zones `on init` moved into a wavetable group their tables,
/// before the engine plays.
pub(super) fn init_tables(engine: &mut Engine, client: &StreamClient) {
    if let Some(plan) = kontra_prepare::tables(engine.plan(), client) {
        engine.swap_plan(&mut Arc::new(plan), &[]);
    }
}

struct MappingZoneSpec {
    sample: kontra_format_uvi::RuntimeSample,
    keys: (u8, u8),
    velocities: (u8, u8),
    root: u8,
    tune: f64,
    gain: f32,
    start: u64,
    reverse: bool,
}

fn ir_loops_to_voice(loops: &[ir::Loop]) -> Arc<[Loop]> {
    loops
        .iter()
        .map(|l| Loop {
            start: l.start,
            end: l.end,
            crossfade: l.crossfade,
            count: l.count,
            alternating: l.alternating,
            until_release: l.until_release,
            tune_ratio: l.tune_ratio,
        })
        .collect::<Vec<_>>()
        .into()
}

fn make_reply(
    id: u32,
    ok: bool,
    frames: u64,
    sample_rate: u32,
    num_zones: u16,
    slices: &[(u64, u64)],
) -> RuntimeReply {
    let mut slice_starts = [0u32; MAX_INLINE_SLICES];
    let count = slices.len().min(MAX_INLINE_SLICES);
    for (dst, &(start, _)) in slice_starts.iter_mut().zip(slices.iter()).take(count) {
        *dst = u32::try_from(start).unwrap_or(u32::MAX);
    }
    RuntimeReply {
        id,
        ok,
        frames,
        sample_rate,
        num_zones,
        num_slices: u16::try_from(count).unwrap_or(0),
        slice_starts,
    }
}

fn resolve_disk_fallback(reference: &str) -> Result<kontra_format_uvi::RuntimeSample, String> {
    let path = PathBuf::from(reference);
    if !path.is_file() {
        return Err(format!("{reference}: file not found"));
    }
    let hdr =
        kontra_format::header::probe_header(&path).map_err(|e| format!("{reference}: {e}"))?;
    let channels = u8::try_from(hdr.channels)
        .map_err(|_| format!("{reference}: channel count exceeds 255"))?;
    if hdr.frames == 0 {
        return Err(format!("{reference}: sample has no frames"));
    }
    let loops = hdr.loops.into_iter().map(|l| l.to_ir_loop(false)).collect();
    Ok(kontra_format_uvi::RuntimeSample {
        sample: ir::Sample {
            name: reference.to_owned(),
            file: path,
            offset: 0,
            len: None,
            channels,
            frames: hdr.frames,
            rate: hdr.sample_rate,
            keystream: None,
            raw: None,
        },
        loops,
    })
}

fn resolve_mapping(
    lib: Option<&kontra_format_uvi::RuntimeLibrary>,
    script_dir: &str,
    reference: &str,
) -> Vec<MappingZoneSpec> {
    let is_audio = reference.rsplit_once('.').is_some_and(|(_, ext)| {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "wav" | "flac" | "aif" | "aiff" | "ogg"
        )
    });
    let plain = |sample: kontra_format_uvi::RuntimeSample| MappingZoneSpec {
        sample,
        keys: (0, 127),
        velocities: (1, 127),
        root: 60,
        tune: 0.0,
        gain: 1.0,
        start: 0,
        reverse: false,
    };
    if is_audio {
        let res = match lib {
            Some(l) => l.sample(script_dir, reference),
            None => resolve_disk_fallback(reference),
        };
        return res.into_iter().map(plain).collect();
    }
    let member = match lib {
        Some(l) => l.read_member(script_dir, reference),
        None => {
            let p = PathBuf::from(reference);
            std::fs::read(&p).ok().map(|b| {
                let dir = p
                    .parent()
                    .map(|d| d.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (dir, b)
            })
        }
    };
    if let Some((base_dir, bytes)) = member
        && let Some(text) = kontra_format_uvi::decode_program_text(&bytes)
    {
        let entries = kontra_format_uvi::parse_mapping_xml(&text);
        let zones: Vec<MappingZoneSpec> = entries
            .into_iter()
            .filter_map(|e| {
                let sample = match lib {
                    Some(l) => l
                        .sample(&base_dir, &e.sample_path)
                        .or_else(|_| l.sample(script_dir, &e.sample_path))
                        .or_else(|_| l.sample("", &e.sample_path))
                        .ok()?,
                    None => {
                        let joined = if base_dir.is_empty() {
                            e.sample_path.clone()
                        } else {
                            format!("{base_dir}/{}", e.sample_path)
                        };
                        resolve_disk_fallback(&joined)
                            .or_else(|_| resolve_disk_fallback(&e.sample_path))
                            .ok()?
                    }
                };
                let start = e.start.min(sample.sample.frames);
                Some(MappingZoneSpec {
                    sample,
                    keys: (e.low_key, e.high_key),
                    velocities: (e.low_velocity, e.high_velocity),
                    root: e.base_note,
                    tune: e.tune,
                    gain: e.gain,
                    start,
                    reverse: e.reverse,
                })
            })
            .collect();
        if !zones.is_empty() {
            return zones;
        }
    }
    if let Some(l) = lib {
        let mut files = l.audio_under(script_dir, reference);
        files.sort();
        return files
            .into_iter()
            .filter_map(|p| Some(plain(l.sample("", &p).ok()?)))
            .collect();
    }
    Vec::new()
}

fn load_samples_into_cache(
    store: &mut kontra_stream::Store,
    items: &[(PathBuf, u64, ir::Sample, Vec<ir::Loop>)],
    cache: &mut HashMap<(PathBuf, u64), CachedSample>,
) {
    struct PendingMeta {
        file: PathBuf,
        offset: u64,
        frames: u64,
        rate: u32,
        loops: Vec<ir::Loop>,
        slices: Vec<(u64, u64)>,
    }
    let files = Files::new(items.iter().map(|(_, _, s, _)| s.file.as_path()));
    let mut load_reqs: Vec<LoadRequest> = Vec::with_capacity(items.len());
    let mut meta: Vec<PendingMeta> = Vec::with_capacity(items.len());
    for (file, offset, ir_sample, loops) in items {
        let Ok((source, fmt)) = files.open(ir_sample) else {
            continue;
        };
        let key = BlobKey {
            file: Arc::from(ir_sample.file.to_string_lossy()),
            offset: ir_sample.offset,
            fmt,
        };
        // Pin at least the attack head (up to 32k frames) so notes sound
        // immediately after swap with zero streaming miss.
        let pin_end = ir_sample.frames.min(32_768);
        let pin = (pin_end > 0).then_some(0..pin_end).into_iter().collect();
        let slices = probe_wav_cues(ir_sample);
        load_reqs.push(LoadRequest {
            key,
            source,
            frames: ir_sample.frames,
            pin,
        });
        meta.push(PendingMeta {
            file: file.clone(),
            offset: *offset,
            frames: ir_sample.frames,
            rate: ir_sample.rate,
            loops: loops.clone(),
            slices,
        });
    }
    if load_reqs.is_empty() {
        return;
    }
    let loaded = store.load_many(load_reqs);
    if let Some(heads) = store.take_heads() {
        let lanes = std::thread::available_parallelism().map_or(4, |n| n.get() * 3);
        heads.load(lanes);
    }
    for (m, res) in meta.into_iter().zip(loaded) {
        if let Ok(sample) = res {
            cache.insert(
                (m.file, m.offset),
                CachedSample {
                    sample,
                    frames: m.frames,
                    rate: m.rate,
                    loops: m.loops,
                    slices: m.slices,
                },
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn is_load_sample_noop(
    plan: &Plan,
    slot_sample: &HashMap<(u32, u16), (PathBuf, u64)>,
    target: (u32, u16),
    sample_key: &(PathBuf, u64),
    root: Option<u8>,
    sample_start: Option<u64>,
    reverse: Option<bool>,
    coarse_tune: Option<f32>,
    fine_tune: Option<f32>,
    gain: Option<f32>,
) -> bool {
    if slot_sample.get(&target) != Some(sample_key) {
        return false;
    }
    let mut matched = false;
    for z in plan
        .zones
        .iter()
        .filter(|z| z.group == target.0 && z.slot == target.1)
    {
        matched = true;
        if let Some(r) = root
            && z.root != r
        {
            return false;
        }
        if let Some(s) = sample_start
            && z.region.start != s.min(z.region.end)
        {
            return false;
        }
        if let Some(rev) = reverse
            && z.region.reverse != rev
        {
            return false;
        }
        if coarse_tune.is_some() || fine_tune.is_some() {
            let want =
                f64::from(coarse_tune.unwrap_or(0.0)) + f64::from(fine_tune.unwrap_or(0.0)) * 0.01;
            if (z.tune - want).abs() > 1e-4 {
                return false;
            }
        }
        if let Some(g) = gain
            && (z.gain - g.clamp(0.0, 16.0)).abs() > 1e-4
        {
            return false;
        }
    }
    matched
}

#[allow(clippy::too_many_arguments)]
fn apply_load_sample(
    plan: &mut Plan,
    group: u32,
    slot: u16,
    cached: &CachedSample,
    root: Option<u8>,
    sample_start: Option<u64>,
    reverse: Option<bool>,
    coarse_tune: Option<f32>,
    fine_tune: Option<f32>,
    gain: Option<f32>,
) {
    let loops = (!cached.loops.is_empty()).then(|| ir_loops_to_voice(&cached.loops));
    let start = sample_start.unwrap_or(0).min(cached.frames);
    let mut updated = false;
    for z in plan
        .zones
        .iter_mut()
        .filter(|z| z.group == group && z.slot == slot)
    {
        updated = true;
        z.region.sample = Arc::clone(&cached.sample);
        z.sample_rate = cached.rate;
        z.region.start = start;
        z.region.end = cached.frames;
        z.region.loops = loops.clone();
        if let Some(r) = root {
            z.root = r;
        }
        if let Some(rev) = reverse {
            z.region.reverse = rev;
        }
        if coarse_tune.is_some() || fine_tune.is_some() {
            z.tune =
                f64::from(coarse_tune.unwrap_or(0.0)) + f64::from(fine_tune.unwrap_or(0.0)) * 0.01;
        }
        if let Some(g) = gain {
            z.gain = g.clamp(0.0, 16.0);
        }
    }
    if !updated {
        let mut zone = plan
            .zones
            .iter()
            .find(|z| z.group == group)
            .cloned()
            .unwrap_or_else(|| {
                let region = Region {
                    sample: Arc::clone(&cached.sample),
                    start,
                    end: cached.frames,
                    loops: loops.clone(),
                    mix: None,
                    table: None,
                    reverse: reverse.unwrap_or(false),
                };
                Zone::new(region, root.unwrap_or(60), cached.rate)
            });
        zone.group = group;
        zone.slot = slot;
        zone.region.sample = Arc::clone(&cached.sample);
        zone.sample_rate = cached.rate;
        zone.region.start = start;
        zone.region.end = cached.frames;
        zone.region.loops = loops;
        if let Some(r) = root {
            zone.root = r;
        }
        if let Some(rev) = reverse {
            zone.region.reverse = rev;
        }
        if coarse_tune.is_some() || fine_tune.is_some() {
            zone.tune =
                f64::from(coarse_tune.unwrap_or(0.0)) + f64::from(fine_tune.unwrap_or(0.0)) * 0.01;
        }
        if let Some(g) = gain {
            zone.gain = g.clamp(0.0, 16.0);
        }
        plan.zones.push(zone);
    }
}

fn apply_load_mapping(
    plan: &mut Plan,
    group: u32,
    slot: u16,
    entries: &[(MappingZoneSpec, CachedSample)],
) {
    let existing: Vec<usize> = plan
        .zones
        .iter()
        .enumerate()
        .filter_map(|(i, z)| (z.group == group && z.slot == slot).then_some(i))
        .collect();
    let template = existing
        .first()
        .and_then(|&i| plan.zones.get(i))
        .or_else(|| plan.zones.iter().find(|z| z.group == group))
        .cloned();
    let apply_one = |z: &mut Zone, spec: &MappingZoneSpec, cached: &CachedSample| {
        z.group = group;
        z.slot = slot;
        z.keys = spec.keys.0..=spec.keys.1;
        z.velocities = spec.velocities.0..=spec.velocities.1;
        z.root = spec.root;
        z.tune = spec.tune;
        z.gain = spec.gain;
        z.sample_rate = cached.rate;
        z.region.sample = Arc::clone(&cached.sample);
        z.region.start = spec.start.min(cached.frames);
        z.region.end = cached.frames;
        z.region.reverse = spec.reverse;
        z.region.loops = (!cached.loops.is_empty()).then(|| ir_loops_to_voice(&cached.loops));
        z.range_crossfades();
    };
    for (idx, (spec, cached)) in entries.iter().enumerate() {
        if let Some(&zone_idx) = existing.get(idx)
            && let Some(z) = plan.zones.get_mut(zone_idx)
        {
            apply_one(z, spec, cached);
        } else {
            let mut z = template.clone().unwrap_or_else(|| {
                let region = Region {
                    sample: Arc::clone(&cached.sample),
                    start: 0,
                    end: cached.frames,
                    loops: None,
                    mix: None,
                    table: None,
                    reverse: false,
                };
                Zone::new(region, spec.root, cached.rate)
            });
            apply_one(&mut z, spec, cached);
            plan.zones.push(z);
        }
    }
    // Keep `plan.zones.len() >= old_len` (Engine::swap_plan invariant) by
    // disabling any excess previously-mapped slots with an empty range.
    for &zone_idx in existing.iter().skip(entries.len()) {
        if let Some(z) = plan.zones.get_mut(zone_idx) {
            z.keys = std::ops::RangeInclusive::new(1, 0);
            z.velocities = std::ops::RangeInclusive::new(1, 0);
        }
    }
}

fn apply_osc_param(plan: &mut Plan, group: u32, slot: u16, param: OscParamKind, value: f64) {
    for z in plan
        .zones
        .iter_mut()
        .filter(|z| z.group == group && z.slot == slot)
    {
        match param {
            OscParamKind::BaseNote => z.root = value.round().clamp(0.0, 127.0) as u8,
            OscParamKind::CoarseTune | OscParamKind::Pitch => z.tune = value.clamp(-96.0, 96.0),
            OscParamKind::FineTune => z.tune = (value * 0.01).clamp(-12.0, 12.0),
            OscParamKind::Gain => z.gain = (value as f32).clamp(0.0, 16.0),
            OscParamKind::Reverse => z.region.reverse = value != 0.0,
            OscParamKind::SampleStart => {
                z.region.start = (value.max(0.0) as u64).min(z.region.end);
            }
            OscParamKind::LoopStart => {
                let start = (value.max(0.0) as u64).min(z.region.end);
                let mut loops: Vec<Loop> = z
                    .region
                    .loops
                    .as_deref()
                    .map(<[Loop]>::to_vec)
                    .unwrap_or_default();
                if let Some(first) = loops.first_mut() {
                    first.start = start.min(first.end);
                } else if start < z.region.end {
                    loops.push(Loop {
                        start,
                        end: z.region.end,
                        ..Loop::default()
                    });
                }
                z.region.loops = (!loops.is_empty()).then(|| loops.into());
            }
            OscParamKind::LoopEnd => {
                let end = (value.max(0.0) as u64).min(z.region.end);
                let mut loops: Vec<Loop> = z
                    .region
                    .loops
                    .as_deref()
                    .map(<[Loop]>::to_vec)
                    .unwrap_or_default();
                if let Some(first) = loops.first_mut() {
                    first.end = end.max(first.start);
                } else if end > 0 {
                    loops.push(Loop {
                        start: 0,
                        end,
                        ..Loop::default()
                    });
                }
                z.region.loops = (!loops.is_empty()).then(|| loops.into());
            }
            OscParamKind::Bypass => {
                if value != 0.0 {
                    z.gain = 0.0;
                } else if z.gain == 0.0 {
                    z.gain = 1.0;
                }
            }
        }
    }
}

fn apply_keygroup_param(plan: &mut Plan, group: u32, param: KeygroupParamKind, value: f64) {
    let key = value.round().clamp(0.0, 127.0) as u8;
    for z in plan.zones.iter_mut().filter(|z| z.group == group) {
        match param {
            KeygroupParamKind::LowKey => {
                let hi = (*z.keys.end()).max(key);
                z.keys = key..=hi;
                z.range_crossfades();
            }
            KeygroupParamKind::HighKey => {
                let lo = (*z.keys.start()).min(key);
                z.keys = lo..=key;
                z.range_crossfades();
            }
            KeygroupParamKind::LowVelocity => {
                let lo = key.max(1);
                let hi = (*z.velocities.end()).max(lo);
                z.velocities = lo..=hi;
                z.range_crossfades();
            }
            KeygroupParamKind::HighVelocity => {
                let hi = key.max(1);
                let lo = (*z.velocities.start()).min(hi);
                z.velocities = lo..=hi;
                z.range_crossfades();
            }
            KeygroupParamKind::Tune | KeygroupParamKind::CoarseTune => {
                z.tune = value.clamp(-96.0, 96.0);
            }
            KeygroupParamKind::FineTune => z.tune = (value * 0.01).clamp(-12.0, 12.0),
            KeygroupParamKind::Gain => z.gain = (value as f32).clamp(0.0, 16.0),
            KeygroupParamKind::Pan => z.pan = (value as f32).clamp(-1.0, 1.0),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_playback_options(
    plan: &mut Plan,
    group: u32,
    slot: u16,
    sample_start: Option<u64>,
    loop_start: Option<u64>,
    loop_end: Option<u64>,
    loop_mode: Option<u8>,
    reverse: Option<bool>,
) -> (bool, u64, u32) {
    let mut ok = false;
    let mut frames = 0u64;
    let mut rate = 0u32;
    for z in plan
        .zones
        .iter_mut()
        .filter(|z| z.group == group && z.slot == slot)
    {
        ok = true;
        frames = z.region.end;
        rate = z.sample_rate;
        if let Some(s) = sample_start {
            z.region.start = s.min(z.region.end);
        }
        if let Some(rev) = reverse {
            z.region.reverse = rev;
        }
        match loop_mode {
            Some(0) => z.region.loops = None,
            Some(mode) => {
                let start = loop_start.unwrap_or(0).min(z.region.end);
                let end = loop_end.unwrap_or(z.region.end).min(z.region.end);
                if end > start {
                    z.region.loops = Some(
                        vec![Loop {
                            start,
                            end,
                            alternating: mode == 2,
                            until_release: mode == 3,
                            ..Loop::default()
                        }]
                        .into(),
                    );
                }
            }
            None if loop_start.is_some() || loop_end.is_some() => {
                let prev = z.region.loops.as_deref().and_then(|l| l.first()).copied();
                let start = loop_start
                    .unwrap_or_else(|| prev.map_or(0, |l| l.start))
                    .min(z.region.end);
                let end = loop_end
                    .unwrap_or_else(|| prev.map_or(z.region.end, |l| l.end))
                    .min(z.region.end);
                if end > start {
                    let mut lp = prev.unwrap_or_default();
                    lp.start = start;
                    lp.end = end;
                    z.region.loops = Some(vec![lp].into());
                }
            }
            None => {}
        }
    }
    (ok, frames, rate)
}

fn build_uvi_effect(effect_type: &str) -> (ir::Effect, Option<ir::KnobModel>) {
    if let Some(pair) = kontra_format_uvi::build_effect_by_type(effect_type) {
        return pair;
    }
    let kind = match effect_type.to_ascii_lowercase().as_str() {
        "onepole" => ir::EffectKind::Filter(ir::Filter {
            pass: ir::Pass::Low,
            hz: 20_000.0,
            stages: 0,
            ladder: Some(ir::Ladder::OnePole),
            damping: [2.0; 3],
            gain: 1.0,
            octaves: 9.965_784,
            q_octaves: 0.0,
        }),
        "ms20" => ir::EffectKind::Filter(ir::Filter {
            pass: ir::Pass::Low,
            hz: 20_000.0,
            stages: 0,
            ladder: Some(ir::Ladder::SallenKey {
                morph: 1.0,
                curve: [2.065_561, 1.749_079],
            }),
            damping: [2.0; 3],
            gain: 1.0,
            octaves: 9.965_784,
            q_octaves: 0.0,
        }),
        "combfilter" => ir::EffectKind::Comb(ir::Comb {
            hz: 440.0,
            feedback: 0.0,
            forward: false,
            octaves: 9.965_784,
        }),
        "gain" | "gainmatrix" => ir::EffectKind::Gain(1.0),
        _ => ir::EffectKind::Other,
    };
    (
        ir::Effect {
            kind,
            bypass: false,
            output: 1.0,
            dry: 0.0,
        },
        None,
    )
}

fn target_rack_at(target: EffectTarget) -> Option<ir::RackAt> {
    match target {
        EffectTarget::Keygroup(g) => Some(ir::RackAt::Group(g)),
        EffectTarget::Bus(b) => b
            .checked_sub(1)
            .and_then(|v| u8::try_from(v).ok())
            .map(ir::RackAt::Bus),
        EffectTarget::Program => Some(ir::RackAt::Inserts),
    }
}

fn mark_effect_target_dirty(
    target: EffectTarget,
    program_dirty: &mut bool,
    dirty_buses: &mut Vec<usize>,
    dirty_groups: &mut Vec<usize>,
) {
    match target {
        EffectTarget::Keygroup(g) => {
            let idx = g as usize;
            if !dirty_groups.contains(&idx) {
                dirty_groups.push(idx);
            }
        }
        EffectTarget::Bus(b) => {
            let idx = b as usize;
            if !dirty_buses.contains(&idx) {
                dirty_buses.push(idx);
            }
        }
        EffectTarget::Program => *program_dirty = true,
    }
}

fn sync_slot_models(
    models: &mut Vec<(ir::RackAt, u8, ir::KnobModel)>,
    at: ir::RackAt,
    op: &EffectOp,
    inserted: Option<(usize, ir::KnobModel)>,
    prev_len: usize,
) {
    match *op {
        EffectOp::Insert { index, .. } => {
            let pos = usize::from(index).min(prev_len);
            for (m_at, slot, _) in models.iter_mut() {
                if *m_at == at && usize::from(*slot) >= pos {
                    *slot = slot.saturating_add(1);
                }
            }
            if let Some((slot_idx, model)) = inserted
                && let Ok(slot_u8) = u8::try_from(slot_idx)
            {
                models.push((at, slot_u8, model));
            }
        }
        EffectOp::Remove { index } => {
            let pos = usize::from(index);
            if pos < prev_len {
                models.retain(|(m_at, slot, _)| !(*m_at == at && usize::from(*slot) == pos));
                for (m_at, slot, _) in models.iter_mut() {
                    if *m_at == at && usize::from(*slot) > pos {
                        *slot = slot.saturating_sub(1);
                    }
                }
            }
        }
        EffectOp::Move { from, to } => {
            let (f, t) = (usize::from(from), usize::from(to));
            if f < prev_len && t < prev_len && f != t {
                for (m_at, slot, _) in models.iter_mut() {
                    if *m_at != at {
                        continue;
                    }
                    let s = usize::from(*slot);
                    let next = if s == f {
                        t
                    } else if f < t && s > f && s <= t {
                        s - 1
                    } else if t < f && s >= t && s < f {
                        s + 1
                    } else {
                        s
                    };
                    if let Ok(next_u8) = u8::try_from(next) {
                        *slot = next_u8;
                    }
                }
            }
        }
    }
}

fn mutate_insert_vec(
    inserts: &mut Vec<Option<ir::Effect>>,
    op: EffectOp,
) -> (bool, Option<(usize, ir::KnobModel)>) {
    match op {
        EffectOp::Insert { index, effect_type } => {
            let pos = usize::from(index).min(inserts.len());
            let (effect, model) = build_uvi_effect(effect_type.as_str());
            inserts.insert(pos, Some(effect));
            (true, model.map(|m| (pos, m)))
        }
        EffectOp::Remove { index } => {
            let pos = usize::from(index);
            if pos < inserts.len() {
                inserts.remove(pos);
                (true, None)
            } else {
                (false, None)
            }
        }
        EffectOp::Move { from, to } => {
            let (f, t) = (usize::from(from), usize::from(to));
            if f < inserts.len() && t < inserts.len() && f != t {
                let item = inserts.remove(f);
                inserts.insert(t, item);
                (true, None)
            } else {
                (false, None)
            }
        }
    }
}

fn apply_effect_chain(
    plan: &mut Plan,
    target: EffectTarget,
    op: EffectOp,
    program_dirty: &mut bool,
    dirty_buses: &mut Vec<usize>,
    dirty_groups: &mut Vec<usize>,
) {
    let (prev_len, changed, inserted) = match target {
        EffectTarget::Keygroup(g) => {
            let Some(group) = plan.groups.get_mut(g as usize) else {
                return;
            };
            let prev = group.inserts.len();
            let (changed, inserted) = mutate_insert_vec(&mut group.inserts, op);
            (prev, changed, inserted)
        }
        EffectTarget::Bus(b) => {
            let Some(bus) = plan.buses.get_mut(b as usize) else {
                return;
            };
            let prev = bus.inserts.len();
            let (changed, inserted) = mutate_insert_vec(&mut bus.inserts, op);
            (prev, changed, inserted)
        }
        EffectTarget::Program => {
            let prev = plan.inserts.len();
            let (changed, inserted) = mutate_insert_vec(&mut plan.inserts, op);
            (prev, changed, inserted)
        }
    };
    if changed {
        if let Some(at) = target_rack_at(target) {
            sync_slot_models(&mut plan.slot_models, at, &op, inserted, prev_len);
        }
        mark_effect_target_dirty(target, program_dirty, dirty_buses, dirty_groups);
    }
}

/// Writes UVI attribute `name` into `model`'s saved knobs and rebuilds
/// `effect` through its registered model; `false` when either refuses.
fn set_model_param(
    model: &mut ir::KnobModel,
    name: &str,
    value: f32,
    effect: &mut ir::Effect,
) -> bool {
    let Some(idx) = kontra_format_uvi::attr_index(model.id, name) else {
        return false;
    };
    let mut saved = model.saved.to_vec();
    let Some(slot) = saved.get_mut(idx) else {
        return false;
    };
    *slot = value;
    let Some(built) = ir::model(model.id).and_then(|m| m.build_effect(&saved, effect)) else {
        return false;
    };
    model.saved = saved.into();
    *effect = built;
    true
}

fn apply_effect_param(
    plan: &mut Plan,
    target: EffectTarget,
    slot: u16,
    name: &str,
    value: f64,
) -> bool {
    let slot_idx = usize::from(slot);
    let effect = match target {
        EffectTarget::Keygroup(g) => plan
            .groups
            .get_mut(g as usize)
            .and_then(|grp| grp.inserts.get_mut(slot_idx))
            .and_then(Option::as_mut),
        EffectTarget::Bus(b) => plan
            .buses
            .get_mut(b as usize)
            .and_then(|bus| bus.inserts.get_mut(slot_idx))
            .and_then(Option::as_mut),
        EffectTarget::Program => plan.inserts.get_mut(slot_idx).and_then(Option::as_mut),
    };
    let Some(effect) = effect else {
        return false;
    };
    if name == "Bypass" {
        effect.bypass = value != 0.0;
        return true;
    }
    if name == "Volume"
        && let ir::EffectKind::Gain(g) = &mut effect.kind
    {
        *g = (value as f32).max(0.0);
        return true;
    }
    if let Some(at) = target_rack_at(target)
        && let Ok(slot_u8) = u8::try_from(slot_idx)
        && let Some((_, _, model)) = plan
            .slot_models
            .iter_mut()
            .find(|(m_at, s, _)| *m_at == at && *s == slot_u8)
        && set_model_param(model, name, value as f32, effect)
    {
        return true;
    }
    match &mut effect.kind {
        ir::EffectKind::Filter(f) => match name {
            "Freq" => {
                f.hz = (value as f32).clamp(20.0, 20_000.0);
                true
            }
            "Mode" => {
                f.pass = if (value - 1.0).abs() < 0.5 {
                    ir::Pass::High
                } else {
                    ir::Pass::Low
                };
                true
            }
            "Q" => {
                let q = (value as f32).clamp(0.0, 1.0);
                f.damping = [2.0 * (1.0 - q); 3];
                true
            }
            "Morph" => {
                if let Some(ir::Ladder::SallenKey { morph, .. }) = &mut f.ladder {
                    *morph = (value as f32).clamp(0.0, 1.0);
                    return true;
                }
                false
            }
            "ReferenceVoltage" => {
                f.gain = (value as f32).max(0.01);
                true
            }
            _ => false,
        },
        ir::EffectKind::Comb(c) => match name {
            "Freq" => {
                c.hz = (value as f32).clamp(ir::COMB_LOWEST_HZ, 20_000.0);
                true
            }
            "Q" => {
                c.feedback = (value as f32).clamp(0.0, 0.999);
                true
            }
            "Mode" => {
                c.forward = (value - 1.0).abs() < 0.5;
                true
            }
            _ => false,
        },
        _ => false,
    }
}

/// Scans RIFF/RF64 WAV chunk headers (bounded, never reading sample audio) for
/// a `cue ` chunk and returns `(start_frame, length_frames)` slice intervals.
fn probe_wav_cues(sample: &ir::Sample) -> Vec<(u64, u64)> {
    let Ok(file) = std::fs::File::open(&sample.file) else {
        return Vec::new();
    };
    let Ok(meta) = file.metadata() else {
        return Vec::new();
    };
    let available = meta.len().saturating_sub(sample.offset);
    let len = sample.len.unwrap_or(available).min(available);
    if len < 12 {
        return Vec::new();
    }
    let slice = SliceReader::new(file, sample.offset, len);
    let reader = TransformedReader::new(slice, sample.offset, sample.keystream.as_ref());
    let mut head = [0u8; 12];
    if reader.read_exact_at(0, &mut head).is_err()
        || !matches!(&head[..4], b"RIFF" | b"RF64")
        || &head[8..12] != b"WAVE"
    {
        return Vec::new();
    }
    let mut pos = 12u64;
    let mut hdr = [0u8; 8];
    for _ in 0..256 {
        if pos.saturating_add(8) > len || reader.read_exact_at(pos, &mut hdr).is_err() {
            break;
        }
        let size = u64::from(u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]));
        let data_pos = pos + 8;
        if &hdr[..4] == b"cue " && size >= 4 && data_pos.saturating_add(size) <= len {
            let read_len = usize::try_from(size)
                .unwrap_or(0)
                .min(4 + MAX_INLINE_SLICES * 24);
            let mut buf = vec![0u8; read_len];
            if reader.read_exact_at(data_pos, &mut buf).is_ok()
                && let Some(c_bytes) = buf.get(0..4).and_then(|s| <[u8; 4]>::try_from(s).ok())
            {
                let count = u32::from_le_bytes(c_bytes) as usize;
                let max = count.min(buf.len().saturating_sub(4) / 24);
                let mut pts: Vec<u64> = (0..max)
                    .filter_map(|i| {
                        let off = 4 + i * 24 + 20;
                        let bytes: [u8; 4] = buf.get(off..off + 4)?.try_into().ok()?;
                        Some(u64::from(u32::from_le_bytes(bytes)).min(sample.frames))
                    })
                    .collect();
                pts.sort_unstable();
                pts.dedup();
                return pts
                    .iter()
                    .enumerate()
                    .map(|(i, &start)| {
                        let end = pts.get(i + 1).copied().unwrap_or(sample.frames);
                        (start, end.saturating_sub(start))
                    })
                    .collect();
            }
            break;
        }
        let step = size.saturating_add(size & 1);
        match data_pos.checked_add(step) {
            Some(next) if next > pos => pos = next,
            _ => break,
        }
    }
    Vec::new()
}
