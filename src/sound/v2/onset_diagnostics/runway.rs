//! Shots-only, between-callback numeric witnesses. Never services or waits for pages.
use super::Part;
use sampler_core::{PageStatus, Runtime, trace::onset::PhysicalVoice};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::Instant;

const BLOCK_ROWS: usize = 256;
const DEMAND_SCAN_ROWS: usize = 4096;
const MISSING_EXAMPLES: usize = 8;

pub(super) fn loader(part: &Part, startup: Option<&crate::sound::report::StreamStartup>) -> Value {
    let Some(stream) = &part._stream else {
        return Value::Null;
    };
    let report = &stream.report;
    let budget = stream.streamer.head_budget_status();
    json!({
        "at_runtime_frame":part.runtime.now(),
        "requested_output_horizon_frames":report.head_frames,
        "selected_hint_output_horizon_frames":stream.streamer.selected_hint_head_frames(),
        "service_output_horizon_frames":part.horizon,
        "configured_payload_limit_bytes":budget.limit_bytes,
        "held_payload_bytes":budget.held_bytes,"peak_payload_bytes":budget.peak_bytes,
        "initial_report_head_bytes":report.head_bytes,
        "rejected_full_candidate":report.head_fallback.map(|failure|json!({
            "budget_bytes":failure.budget_bytes,
            "required_at_least_bytes":failure.required_at_least_bytes})),
        "startup_coverage":startup.map(|s|json!({
            "ranges":s.ranges,"covered_ranges":s.covered_ranges,
            "onset_ranges":s.onset_ranges,"covered_onset_ranges":s.covered_onset_ranges,
            "hint_ranges":s.hint_ranges,"covered_hint_ranges":s.covered_hint_ranges,
            "hint_source_frames":s.hint_source_frames,"covered_hint_source_frames":s.covered_hint_source_frames,
            "selected_hint_ranges":s.selected_hint_ranges,"covered_selected_hint_ranges":s.covered_selected_hint_ranges,
            "selected_hint_source_frames":s.selected_hint_source_frames,
            "covered_selected_hint_source_frames":s.covered_selected_hint_source_frames,
            "physical_payload_bytes_at_loader":s.physical_payload_bytes,
            "scope":"existing loader-owned numeric snapshot; startup predictions only; no new metadata scan"
        })),
        "page_pool_bytes":report.pool_bytes,"page_pool_pages":report.pool_pages,
        "latency_samples":report.latency_samples,
        "latency_p95_us":report.latency_p95.as_micros(),
        "scope":"initial source payload snapshot before first render; head budget covers required full PCM and packed staged/current/retired starts; page pool and decoder workspace are separate; requested horizon and rejected lower bound are not admitted payload",
        "live_controller_or_acoustic_readiness":false
    })
}

#[derive(Serialize)]
struct Missing {
    voice: PhysicalVoice,
    zone: u32,
    asset: sampler_core::AssetId,
    page: usize,
    source_frame: u64,
    frames: [usize; 2],
    deadline: u64,
    status: u8,
    cold: bool,
    starved: bool,
}
#[derive(Serialize)]
pub(super) struct Demand {
    at: u64,
    horizon_frames: u32,
    visited_rows: usize,
    resident_rows: usize,
    ready_page_rows: usize,
    unavailable_rows: usize,
    complete: bool,
    missing_examples_omitted: usize,
    missing: Vec<Missing>,
    probe_us: u128,
}
fn demand(runtime: &Runtime, frames: u32) -> Result<Demand, &'static str> {
    let started = Instant::now();
    let mut sample = Demand {
        at: runtime.now(),
        horizon_frames: frames,
        visited_rows: 0,
        resident_rows: 0,
        ready_page_rows: 0,
        unavailable_rows: 0,
        complete: false,
        missing_examples_omitted: 0,
        missing: Vec::with_capacity(MISSING_EXAMPLES),
        probe_us: 0,
    };
    sample.complete = runtime
        .visit_stream_demand(frames, |row| {
            if sample.visited_rows == DEMAND_SCAN_ROWS {
                return false;
            }
            sample.visited_rows += 1;
            sample.resident_rows += usize::from(row.resident);
            sample.ready_page_rows += usize::from(!row.resident && row.status == PageStatus::Ready);
            if !row.resident && row.status != PageStatus::Ready {
                sample.unavailable_rows += 1;
                if sample.missing.len() < MISSING_EXAMPLES {
                    sample.missing.push(Missing {
                        voice: row.voice.into(),
                        zone: row.source_zone,
                        asset: row.key.asset,
                        page: row.key.index,
                        source_frame: row.source_frame,
                        frames: [row.frames.start, row.frames.end],
                        deadline: row.deadline,
                        status: match row.status {
                            PageStatus::Missing => 0,
                            PageStatus::Pending => 1,
                            PageStatus::Ready => 2,
                            PageStatus::Failed(sampler_core::DecodeFailure::Unavailable) => 3,
                            PageStatus::Failed(sampler_core::DecodeFailure::InvalidSamples) => 4,
                        },
                        cold: row.cold,
                        starved: row.starved,
                    });
                } else {
                    sample.missing_examples_omitted += 1;
                }
            }
            true
        })
        .map_err(|_| "Read-only stream runway query failed")?;
    sample.probe_us = started.elapsed().as_micros();
    Ok(sample)
}

#[derive(Serialize)]
struct Block {
    interval: [u64; 2],
    trial: Option<u16>,
    note_on_frame: Option<u64>,
    underruns_before: u64,
    underruns_after: u64,
    lateness_us: u128,
    callback_us: u128,
    before: Option<Demand>,
    after: Option<Demand>,
}
pub(super) struct Audit {
    attack: Option<(u16, u64)>,
    rows: Vec<Block>,
    omitted_blocks: u64,
}
impl Audit {
    pub(super) fn new() -> Self {
        Self {
            attack: None,
            rows: Vec::with_capacity(BLOCK_ROWS),
            omitted_blocks: 0,
        }
    }
    pub(super) fn attack(&mut self, trial: u16, frame: u64) {
        self.attack = Some((trial, frame));
    }
    fn probing(&self, frame: u64) -> bool {
        self.attack
            .is_some_and(|(_, on)| frame >= on && frame - on < 512)
    }
    pub(super) fn before(
        &self,
        runtime: &Runtime,
        frames: u32,
    ) -> Result<Option<Demand>, &'static str> {
        if self.rows.len() < BLOCK_ROWS && self.probing(runtime.now()) {
            demand(runtime, frames).map(Some)
        } else {
            Ok(None)
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn record(
        &mut self,
        runtime: &Runtime,
        start: u64,
        frames: u32,
        underruns_before: u64,
        late: std::time::Duration,
        tolerance: std::time::Duration,
        callback: std::time::Duration,
        before: Option<Demand>,
    ) -> Result<(), &'static str> {
        let after_count = runtime.stream_underruns();
        if after_count < underruns_before {
            return Err("Stream underrun counter regressed");
        }
        let changed = after_count != underruns_before;
        if !(changed || late > tolerance || self.probing(start)) {
            return Ok(());
        }
        if self.rows.len() == BLOCK_ROWS {
            self.omitted_blocks = self.omitted_blocks.saturating_add(1);
            return Ok(());
        }
        let after = if changed || self.probing(start) {
            Some(demand(runtime, frames)?)
        } else {
            None
        };
        self.rows.push(Block {
            interval: [start, runtime.now()],
            trial: self.attack.map(|a| a.0),
            note_on_frame: self.attack.map(|a| a.1),
            underruns_before,
            underruns_after: after_count,
            lateness_us: late.as_micros(),
            callback_us: callback.as_micros(),
            before,
            after,
        });
        Ok(())
    }
    pub(super) fn report(&self) -> Value {
        json!({"blocks":self.rows,"omitted_blocks":self.omitted_blocks,
            "block_limit":BLOCK_ROWS,"demand_row_scan_limit":DEMAND_SCAN_ROWS,
            "missing_example_limit":MISSING_EXAMPLES,
            "scope":"read-only current demand before/after callbacks; first 512 output frames per attack, any underrun increment, and any pacing miss; no exact missed-read or worker completion claim; counts are demand rows, not distinct pages; probe time can affect pacing"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_core::*;

    // Source-free deterministic storage control. No decoder, I/O, sleep or bank.
    #[test]
    fn resident_first_window_does_not_certify_next_block_runway() {
        let pcm = Pcm::streamed(48000, PAGE_FRAMES * 3).unwrap();
        drop(
            pcm.set_ranges(vec![(5952, vec![[0.25; 2]; 160].into_boxed_slice())])
                .unwrap(),
        );
        let plan = Prepared::new(
            48000,
            vec![pcm],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::default(),
                playback: Playback {
                    start: 5952,
                    ..Default::default()
                },
            }],
            1,
        )
        .unwrap();
        let limits = Limits::for_plan(&plan, 8, 8);
        let (cache, mut worker) = StreamCache::new(8).unwrap();
        let mut runtime = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
        runtime.set_cold_starts(true);
        runtime.set_stream_horizon(128).unwrap();
        runtime
            .trigger(
                Input {
                    protocol: Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 60,
                    external_id: Some(1),
                },
                60,
                1.,
            )
            .unwrap();
        let first = demand(&runtime, 1).unwrap();
        assert!(first.complete);
        assert_eq!((first.resident_rows, first.unavailable_rows), (1, 0));
        let mut output = [[0.; 2]; 128];
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                runtime.service_streaming(128).unwrap();
                runtime.render(&mut output).unwrap();
            }),
            0
        );
        assert_eq!(runtime.stream_underruns(), 0);
        let next = demand(&runtime, 128).unwrap();
        assert!(next.complete);
        assert_eq!((next.resident_rows, next.unavailable_rows), (32, 96));
        assert_eq!(next.missing[0].status, 0);
        assert_eq!(next.missing[0].frames, [6112, 6113]);
        let clock = runtime.now();
        assert_eq!(demand(&runtime, 128).unwrap().unavailable_rows, 96);
        assert_eq!(runtime.now(), clock, "diagnostics cannot advance the clock");
        assert!(
            worker.next_job().is_none(),
            "diagnostics cannot enqueue decode work"
        );
        runtime.service_streaming(128).unwrap();
        let mut held = worker.next_job().unwrap();
        assert_eq!(demand(&runtime, 128).unwrap().missing[0].status, 1);
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                runtime.render(&mut output).unwrap();
            }),
            0
        );
        assert_eq!(runtime.stats().cold_starts, 0);
        assert_eq!(
            runtime.stream_underruns(),
            1,
            "a warm onset can still starve during playback"
        );
        held.frames_mut().fill([0.25; 2]);
        worker.complete(held, Ok(())).unwrap();
        assert!(runtime.service_streaming(128).unwrap());
        assert_eq!(demand(&runtime, 128).unwrap().unavailable_rows, 0);
    }
}
