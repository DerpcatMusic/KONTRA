//! Numeric-only actual V2 Auto head/onset diagnostics. Audio remains in RAM.
use kontakto::sound::{
    BlockInfo, Core, CoreLoader, LoadRequest,
    event::{Event, HostNote, HostPattern},
    v2::{V2Core, V2Loader},
};
use serde_json::{Value, json};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    path::PathBuf,
    time::{Duration, Instant},
};
struct Counting;
thread_local! {static ACTIVE:Cell<bool>=const{Cell::new(false)};static HEAP:Cell<u64>=const{Cell::new(0)};}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if ACTIVE.get() {
            HEAP.set(HEAP.get() + 1);
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if ACTIVE.get() {
            HEAP.set(HEAP.get() + 1);
        }
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
fn io() -> Value {
    let data = std::fs::read_to_string("/proc/self/io").unwrap();
    let field = |key: &str| {
        data.lines()
            .find_map(|l| l.strip_prefix(key)?.trim().parse::<u64>().ok())
            .unwrap_or(0)
    };
    json!({"read_bytes":field("read_bytes:"),"rchar":field("rchar:"),"syscr":field("syscr:")})
}
fn heads(stream: &kontakto::sound::Stream) -> Value {
    let mut resident = 0;
    let mut ready = 0;
    let mut source_frames = 0u64;
    for p in &stream.assets {
        let b = p.head_bytes();
        resident += b;
        ready += usize::from(b > 0);
        source_frames += p.frame_count() as u64;
    }
    json!({"assets":stream.assets.len(),"assets_with_any_head":ready,"head_bytes":resident,"source_frames":source_frames,"note":"any head is not proof every authored onset/loop window is ready"})
}
fn readiness(
    stream: &kontakto::sound::Stream,
    include: impl Fn(sampler_core::AssetId) -> bool,
) -> Value {
    let r = stream
        .streamer
        .head_readiness_matching(&stream.assets, include);
    json!({"assets":r.assets,"ranges":r.ranges,"source_frames":r.source_frames,"covered_assets":r.covered_assets,"covered_ranges":r.covered_ranges,"covered_source_frames":r.covered_source_frames,"minimum_packed_bytes":r.minimum_packed_bytes(),"decoded_bytes":r.decoded_bytes(),"packed_resident_bytes":r.packed_bytes,"configured_budget_bytes":r.budget_bytes,"complete":r.complete()})
}
fn stats(core: &V2Core) -> Value {
    let s = core.runtime_stats(0).unwrap();
    json!({"voices":s.voices,"cold_starts":s.cold_starts,"underruns":s.stream_underruns,"refused":s.refused_starts,"capacity_drops":s.voice_drops,"stream_capacity_refusals":s.stream_capacity_refusals})
}
fn phase(core: &mut V2Core, stream: &kontakto::sound::Stream, block: usize, round: i32) -> Value {
    let n = 120usize;
    let mut rows = Vec::new();
    let mut times = Vec::with_capacity(n);
    let mut misses = 0;
    let mut peak = 0f32;
    let mut first_signal = None;
    let mut event_us = 0u64;
    let before_heap = HEAP.get();
    let before_io = io();
    let before_stats = stats(core);
    let before_heads = heads(stream);
    let start = Instant::now();
    for index in 0..n {
        ACTIVE.set(true);
        let timer = Instant::now();
        core.begin_block(&BlockInfo {
            frames: block,
            ..Default::default()
        });
        if index == 0 {
            let event = Instant::now();
            core.event(0, Event::midi1(0xb0, 1, 110));
            core.event(0, Event::midi1(0xb0, 11, 127));
            for key in 60..68 {
                core.event(
                    0,
                    Event::NoteOn {
                        note: HostNote {
                            port: 0,
                            channel: 0,
                            key,
                            id: round * 1000 + i32::from(key),
                            clap: true,
                        },
                        velocity: 0.8,
                        tune: 0.,
                    },
                );
            }
            event_us = event.elapsed().as_micros() as u64;
        }
        let mut block_peak = 0f32;
        for offset in (0..block).step_by(128) {
            let len = (block - offset).min(128);
            let out = core.render(len);
            for sample in out.buses[0].iter().flat_map(|c| &c[..len]) {
                block_peak = block_peak.max(sample.abs());
            }
        }
        core.end_block(block, &mut |_| true);
        let ns = timer.elapsed().as_nanos() as u64;
        ACTIVE.set(false);
        times.push(ns);
        misses += usize::from(ns as f64 > block as f64 / 48000. * 1e9);
        peak = peak.max(block_peak);
        if block_peak > 1e-8 && first_signal.is_none() {
            first_signal = Some(index * block);
        }
        if [0, 1, 2, 4, 8, 16, 32, 64, 119].contains(&index) {
            rows.push(json!({"block":index,"engine_offset":index*block,"callback_us":ns as f64/1000.,"peak":block_peak,"runtime":stats(core),"heads":heads(stream)}));
        }
        let due = start + Duration::from_secs_f64((index + 1) as f64 * block as f64 / 48000.);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    times.sort_unstable();
    json!({"round":round,"before_runtime":before_stats,"after_runtime":stats(core),"before_heads":before_heads,"after_heads":heads(stream),"after_exact_head_coverage":readiness(stream, |_|true),"io_before":before_io,"io_after":io(),"event_us":event_us,"deadline_misses":misses,"heap_calls":HEAP.get()-before_heap,"callback_p50_us":times[n/2] as f64/1000.,"callback_p99_us":times[(n-1)*99/100] as f64/1000.,"callback_max_us":times[n-1] as f64/1000.,"first_nonzero_engine_offset_frames":first_signal,"peak":peak,"rows":rows})
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "PATH 64|256");
    let path = PathBuf::from(&args[1]);
    let block = args[2].parse::<usize>().unwrap();
    assert!([64, 256].contains(&block));
    let load_start = Instant::now();
    let loaded = V2Loader
        .prepare(
            &LoadRequest {
                path,
                sample_rate: 48000.,
                ..Default::default()
            },
            &mut |_| {},
            &|| false,
        )
        .unwrap();
    let load_ms = load_start.elapsed().as_secs_f64() * 1000.;
    let stream = loaded.stream.clone().expect("streamed instrument");
    let candidate_assets = |first_chord: bool| -> std::collections::HashSet<sampler_core::AssetId> {
        loaded
            .instrument
            .as_ref()
            .into_iter()
            .flat_map(|i| {
                i.zones.iter().filter(move |z| {
                    !z.group.is_some_and(|g| i.groups[g.0].muted)
                        && (!first_chord
                            || (z.keys.low <= 67
                                && z.keys.high >= 60
                                && z.velocities.low <= 102
                                && z.velocities.high >= 102))
                })
            })
            .filter_map(|z| {
                z.asset
                    .and_then(|a| stream.assets.get(a.0))
                    .map(|p| p.asset_id())
            })
            .collect()
    };
    let enabled = candidate_assets(false);
    let chord = candidate_assets(true);
    let source = json!({"load_ms":load_ms,"head_frames":stream.report.head_frames,"head_bytes_reported_at_load":stream.report.head_bytes,"pool_bytes":stream.report.pool_bytes,"full_bytes":stream.report.full_bytes,"read_latency_samples":stream.report.latency_samples,"read_latency_p95_us":stream.report.latency_p95.as_micros(),"heads_after_prepare":heads(&stream),"exact_head_coverage":readiness(&stream, |_|true),"saved_unmuted_asset_coverage":readiness(&stream, |id|enabled.contains(&id)),"static_first_chord_asset_candidates":readiness(&stream, |id|chord.contains(&id)),"candidate_scope":"saved group mute plus authored key/velocity; includes RR/articulation/trigger/controller alternatives, does not infer Lua mutations or exact first-burst reachability","io_after_prepare":io()});
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, loaded.part);
    let first = phase(&mut core, &stream, block, 1);
    for key in 60..68 {
        core.event(
            0,
            Event::Choke(HostPattern {
                port: 0,
                channel: 0,
                key: i32::from(key),
                id: 1000 + i32::from(key),
                clap: true,
            }),
        );
    }
    for _ in 0..20 {
        core.begin_block(&BlockInfo {
            frames: block,
            ..Default::default()
        });
        let _ = core.render(block.min(128));
        core.end_block(block, &mut |_| true);
        std::thread::sleep(Duration::from_millis(1));
    }
    let second = phase(&mut core, &stream, block, 2);
    println!(
        "{}",
        json!({"policy":"actual V2Loader default Auto","block":block,"cache_scope":"fresh process/source heads; OS/storage cache uncontrolled; no cache eviction requested","diagnostic":true,"source":source,"first_burst":first,"second_burst":second})
    );
}
