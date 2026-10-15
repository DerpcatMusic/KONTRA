//! Bounded reductions between blocks. No PCM is retained or exported.
use super::Part;
use sampler_core::{
    PlanId,
    trace::{
        TraceReader, TraceRecord,
        onset::{Birth, Record, Window},
    },
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

type Voice = (u64, usize, u64);
fn voice(v: sampler_core::trace::onset::PhysicalVoice) -> Voice {
    (v.runtime, v.slot, v.generation)
}
fn id(v: Voice) -> [u64; 3] {
    [v.0, v.1 as u64, v.2]
}

#[derive(Default)]
struct Metric {
    nonzero: [Option<u64>; 2],
    threshold: [Option<u64>; 2],
    peak: [f64; 2],
    source_nonzero: [Option<u64>; 2],
    source_threshold: [Option<u64>; 2],
    first: Option<TraceRecord>,
    blocks: u64,
}
impl Metric {
    fn add(&mut self, row: TraceRecord) {
        for channel in 0..2 {
            let update = |target: &mut Option<u64>, offset: Option<u32>| {
                if let Some(offset) = offset {
                    let frame = row.at + u64::from(offset);
                    if target.is_none_or(|old| frame < old) {
                        *target = Some(frame);
                    }
                }
            };
            update(
                &mut self.nonzero[channel],
                row.output.onset.nonzero[channel],
            );
            update(
                &mut self.threshold[channel],
                row.output.onset.above_threshold[channel],
            );
            update(
                &mut self.source_nonzero[channel],
                row.identity.source_metrics.onset.nonzero[channel],
            );
            update(
                &mut self.source_threshold[channel],
                row.identity.source_metrics.onset.above_threshold[channel],
            );
            self.peak[channel] = self.peak[channel].max(row.output.peak[channel]);
        }
        self.blocks += 1;
        if self.first.is_none() {
            self.first = Some(row);
        }
    }
}

pub(super) struct Capture {
    capacity: usize,
    births: Vec<Birth>,
    windows: Vec<Window>,
    metrics: BTreeMap<(Voice, usize), Metric>,
    sums: BTreeMap<usize, Metric>,
    reduced_overflow: u64,
    signal_dropped: u64,
    onset_dropped: u64,
    first_trial: Option<(usize, usize)>,
    records: u64,
    nodes: BTreeMap<usize, (&'static str, &'static str)>,
    birth_graph: BTreeMap<Voice, Option<usize>>,
    graph_scope: Option<sampler_core::trace::SignalTraceScope>,
    graph_nodes: usize,
}
impl Capture {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            births: Vec::new(),
            windows: Vec::new(),
            metrics: BTreeMap::new(),
            sums: BTreeMap::new(),
            reduced_overflow: 0,
            signal_dropped: 0,
            onset_dropped: 0,
            first_trial: None,
            records: 0,
            nodes: BTreeMap::new(),
            birth_graph: BTreeMap::new(),
            graph_scope: None,
            graph_nodes: 0,
        }
    }
    pub fn birth_count(&self) -> usize {
        self.births.len()
    }
    pub fn drain(&mut self, reader: &TraceReader) -> Result<(), &'static str> {
        self.graph_scope = reader.graph.scope;
        self.graph_nodes = reader.graph.nodes.len();
        for row in reader
            .onset_reader()
            .drain()
            .map_err(|_| "Owned onset trace drain failed")?
        {
            if self.births.len() + self.windows.len() >= self.capacity {
                self.reduced_overflow += 1;
                continue;
            }
            match row {
                Record::Birth(row) => {
                    self.birth_graph
                        .insert(voice(row.voice), reader.graph.contribution_node(row.zone));
                    self.births.push(row);
                }
                Record::Window(row) => self.windows.push(row),
            }
        }
        for row in reader.drain() {
            self.records += 1;
            if row.node >= reader.graph.nodes.len() {
                return Err("Trace node identity is outside its graph");
            }
            if self.nodes.len() < self.capacity || self.nodes.contains_key(&row.node) {
                let node = &reader.graph.nodes[row.node];
                self.nodes.insert(row.node, (node.kind, node.processor));
            }
            // Coherent node sums retain a last-contributor identity; only the
            // explicit physical contribution row can establish voice coverage.
            if row.contribution {
                let physical = row
                    .identity
                    .voice
                    .ok_or("Contribution row has no physical owner")?;
                if reader.graph.contribution_node(row.identity.zone) != Some(row.node) {
                    return Err("Contribution row is outside its declared graph");
                }
                let key = (voice(physical), row.node);
                if self.metrics.len() + self.sums.len() >= self.capacity
                    && !self.metrics.contains_key(&key)
                {
                    self.reduced_overflow += 1;
                    continue;
                }
                self.metrics.entry(key).or_default().add(row);
            } else {
                if self.metrics.len() + self.sums.len() >= self.capacity
                    && !self.sums.contains_key(&row.node)
                {
                    self.reduced_overflow += 1;
                    continue;
                }
                self.sums.entry(row.node).or_default().add(row);
            }
        }
        self.signal_dropped = reader.dropped();
        self.onset_dropped = reader.onset_reader().dropped();
        if self.signal_dropped != 0 || self.onset_dropped != 0 || self.reduced_overflow != 0 {
            return Err("Trace or bounded reduction lost rows; capture is partial");
        }
        Ok(())
    }
    pub fn mark_first_trial(&mut self, start: usize) {
        self.first_trial = Some((start, self.births.len()));
    }
    fn first_windows(&self) -> Result<Vec<Window>, &'static str> {
        let (start, end) = self
            .first_trial
            .ok_or("No first trial window witness is available")?;
        if start == end {
            return Err("First trial selected no physical births");
        }
        let mut windows = Vec::new();
        for birth in &self.births[start..end] {
            let mut count = 0;
            for window in &self.windows {
                if voice(window.voice) == voice(birth.voice) {
                    if window.asset != birth.asset {
                        return Err("Birth and window physical assets disagree");
                    }
                    windows.push(*window);
                    count += 1;
                }
            }
            if count == 0 {
                return Err("A physical first-trial birth has no complete first-window witness");
            }
        }
        Ok(windows)
    }
    /// The exact original immutable asset/window is used. No group is reloaded
    /// or unpurged; a missing/dynamic asset identity refuses the intervention.
    pub fn warm_first_windows(
        &self,
        part: &mut Part,
        plan: PlanId,
        request: bool,
    ) -> Result<Value, &'static str> {
        if part.runtime.active_plan() != plan {
            return Err("Warm-window plan owner was replaced");
        }
        let windows = self.first_windows()?;
        let mut ready = 0;
        let mut requested = 0;
        let at = part.runtime.now();
        for window in &windows {
            let pcm = part
                ._stream
                .as_ref()
                .and_then(|s| s.assets.iter().find(|p| p.asset_id() == window.asset))
                .or_else(|| {
                    part.waveform_sources
                        .values()
                        .find(|s| s.pcm.asset_id() == window.asset)
                        .map(|s| &s.pcm)
                })
                .cloned()
                .ok_or("No exact current asset mapping for a historical warm window")?;
            if window.frames[0] >= window.frames[1] || window.frames[1] > pcm.frame_count() {
                return Err("Historical first-window range is invalid for its actual asset");
            }
            if pcm.resident_covers(window.frames[0]..window.frames[1]) {
                ready += 1;
                continue;
            }
            let cache = part
                .runtime
                .stream_cache_mut()
                .ok_or("Historical window is cold without a streaming cache")?;
            if request {
                cache
                    .request(&pcm, window.page, at)
                    .map_err(|_| "Exact-window request was refused")?;
                requested += 1;
            }
            if cache.status(sampler_core::PageKey {
                asset: window.asset,
                index: window.page,
            }) == sampler_core::PageStatus::Ready
                && cache
                    .protect(&pcm, window.frames[0]..window.frames[1])
                    .map_err(|_| "Exact-window protection failed")?
            {
                ready += 1;
            }
        }
        if request && requested != 0 {
            // Ordinary service publishes/polls and wakes the existing workers.
            // This is off audio and advances no sample clock.
            part.runtime
                .service_streaming(0)
                .map_err(|_| "Exact-window production service failed")?;
        }
        Ok(
            json!({"declared_intervention":true,"requested":request,"requested_pages":requested,
            "windows":windows.len(),"ready_windows":ready,"verified_ready":ready==windows.len(),
            "at_frame":at,"identity":"historical_actual_asset_id_and_interpolation_ranges"}),
        )
    }
    pub fn compare_trials(&self, trials: &[Value]) -> Value {
        let Some((first, last)) = trials.first().zip(trials.last()) else {
            return Value::Null;
        };
        let selection = |trial: &Value| -> Option<Vec<Value>> {
            let start = usize::try_from(trial["birth_start"].as_u64()?).ok()?;
            let end = usize::try_from(trial["birth_end"].as_u64()?).ok()?;
            let mut rows: Vec<_> = self
                .births
                .get(start..end)?
                .iter()
                .map(|b| {
                    json!([
                        b.group,
                        b.zone,
                        b.asset,
                        b.used_start_offset,
                        b.source_offset_micros,
                        b.cursor.base_start,
                        b.cursor.traversal,
                        b.cursor.frame,
                        b.cursor.fraction,
                        b.cursor.step,
                        b.key,
                        b.velocity
                    ])
                })
                .collect();
            // Numeric serialization is only an order key; no identities are invented.
            rows.sort_by_cached_key(Value::to_string);
            Some(rows)
        };
        let parameters = |trial: &Value| -> Option<Vec<Value>> {
            let start = usize::try_from(trial["birth_start"].as_u64()?).ok()?;
            let end = usize::try_from(trial["birth_end"].as_u64()?).ok()?;
            let mut voices = Vec::new();
            for birth in self.births.get(start..end)? {
                let physical = voice(birth.voice);
                let rows: Vec<_> = self
                    .metrics
                    .iter()
                    .filter_map(|((v, node), metric)| {
                        if *v != physical {
                            return None;
                        }
                        let row = metric.first?;
                        Some(json!([
                            node,
                            row.gain,
                            row.values,
                            row.normalized,
                            row.identity.region_gain,
                            row.identity.velocity_gain,
                            row.identity.xfade_weight,
                            row.identity.script_gain,
                            row.identity.note_gain,
                            row.identity.amplifier_control_gain,
                            row.identity.ratio
                        ]))
                    })
                    .collect();
                if rows.is_empty() {
                    return None;
                }
                voices.push(json!([birth.group, birth.zone, birth.asset, rows]));
            }
            voices.sort_by_cached_key(Value::to_string);
            Some(voices)
        };
        let gain_match = parameters(first)
            .zip(parameters(last))
            .map(|(a, b)| !a.is_empty() && a == b);
        match selection(first).zip(selection(last)) {
            Some((first, last)) => {
                json!({"births_and_cursors_match":!first.is_empty()&&first==last,
                "observed_gain_and_signal_parameters_match":gain_match,
                "first_voices":first.len(),"last_voices":last.len(),"full_effective_parameters_verified":false})
            }
            None => Value::Null,
        }
    }
    pub fn report(&self) -> Value {
        let rows:Vec<_> = self.metrics.iter().map(|((physical,node),m)| json!({"voice":id(*physical),"node":node,
            "nonzero":m.nonzero,"above_threshold":m.threshold,"peak":m.peak,"blocks":m.blocks,
            "post_resampler_source_nonzero":m.source_nonzero,"post_resampler_source_above_threshold":m.source_threshold,
            "first_row":m.first})).collect();
        let sums: Vec<_> = self
            .sums
            .iter()
            .map(|(node, m)| {
                json!({"node":node,"nonzero":m.nonzero,
            "above_threshold":m.threshold,"peak":m.peak,"blocks":m.blocks})
            })
            .collect();
        let no_source = self
            .metrics
            .values()
            .all(|m| m.source_nonzero.iter().all(Option::is_none));
        let no_output = self
            .metrics
            .values()
            .chain(self.sums.values())
            .all(|m| m.nonzero.iter().all(Option::is_none));
        let physical: BTreeMap<_, _> = self.births.iter().map(|b| (voice(b.voice), b)).collect();
        let coverage: Vec<_> = physical.iter().map(|(v,b)| {
            let graph_node = self.birth_graph.get(v).copied().flatten();
            let observed = graph_node.is_some_and(|node| self.metrics.contains_key(&(*v,node)));
            json!({"voice":id(*v),"zone":b.zone,"actual_birth_key":b.key,
                "contribution_node":graph_node,"contribution_observed":observed,
                "status":if observed { "covered" } else { "partial" },
                "reason":if graph_node.is_none() { Some("selected_birth_has_no_contribution_graph") }
                    else if !observed { Some("selected_birth_has_no_observed_contribution") } else { None }})
        }).collect();
        let missing_graph = coverage
            .iter()
            .filter(|r| r["contribution_node"].is_null())
            .count();
        let missing = coverage
            .iter()
            .filter(|r| r["contribution_observed"] != true)
            .count();
        let loss =
            self.signal_dropped != 0 || self.onset_dropped != 0 || self.reduced_overflow != 0;
        let coverage_status = if loss || missing != 0 {
            "partial"
        } else if physical.is_empty() {
            "unobserved"
        } else {
            "covered_observed_births"
        };
        let all_zero = if missing != 0 || loss || (self.metrics.is_empty() && self.sums.is_empty())
        {
            None
        } else {
            Some(no_source && no_output)
        };
        let nodes: Vec<_> = self
            .nodes
            .iter()
            .map(|(id, (kind, processor))| json!({"node":id,"kind":kind,"processor":processor}))
            .collect();
        json!({"births":self.births,"first_windows":self.windows,"voice_metrics":rows,"node_metrics":sums,"nodes":nodes,
            "signal_rows_reduced":self.records,"signal_dropped":self.signal_dropped,"onset_dropped":self.onset_dropped,
            "reduction_overflow":self.reduced_overflow,"births_without_signal_rows":missing,
            "graph_scope":self.graph_scope,"graph_nodes":self.graph_nodes,
            "birth_graph_coverage":coverage,"graph_coverage_status":coverage_status,
            "births_without_contribution_graph":missing_graph,
            "scope_semantics":"immutable_prepared_region_interval_overlap_not_input_key",
            "source_and_output_all_zero":all_zero,"threshold":sampler_core::trace::onset::THRESHOLD,
            "perceptual_attack_detector":false,"window_scope":"selection_time_prediction"})
    }
}
