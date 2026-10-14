//! Numeric-only experimental complete-attack preparation. No audio is written.
//! Uses the production Kontakt streamed loader with explicit policy overrides;
//! this is not a receipt for the default V2 Auto policy.
use serde_json::json;
use std::{path::Path, time::Instant};
fn io() -> serde_json::Value {
    let s = std::fs::read_to_string("/proc/self/io").unwrap_or_default();
    let n = |key: &str| {
        s.lines()
            .find_map(|l| l.strip_prefix(key)?.trim().parse::<u64>().ok())
            .unwrap_or(0)
    };
    json!({"read_bytes":n("read_bytes:"),"rchar":n("rchar:"),"syscr":n("syscr:")})
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 4, "PATH HEAD_BUDGET_BYTES DECODERS");
    let policy = sampler_kontakt::StreamPolicy {
        head_budget: args[2].parse().unwrap(),
        decoders: args[3].parse().unwrap(),
        max_step: 16.,
        block_frames: kontakto::sound::MAX_BLOCK,
        voices: 1024,
        lazy: false,
        ..Default::default()
    };
    let before = io();
    let start = Instant::now();
    let result = sampler_kontakt::load_streamed(
        Path::new(&args[1]),
        &sampler_kontakt::Options {
            mpe: None,
            ..Default::default()
        },
        &policy,
        |_| {},
    );
    let ms = start.elapsed().as_secs_f64() * 1000.;
    match result {
        Ok(loaded) => {
            let r = loaded.streamer.head_readiness(&loaded.assets);
            println!(
                "{}",
                json!({"policy":"experimental production Kontakt eager heads; default V2 Auto unchanged","load_ms":ms,"head_budget_bytes":policy.head_budget,"decoders":policy.decoders,"assets":r.assets,"ranges":r.ranges,"covered_assets":r.covered_assets,"covered_ranges":r.covered_ranges,"head_ready":r.complete(),"minimum_packed_bytes":r.minimum_packed_bytes(),"decoded_head_bytes":r.decoded_bytes(),"actual_packed_head_bytes":r.packed_bytes,"full_sample_bytes":loaded.report.full_bytes,"pool_bytes":loaded.report.pool_bytes,"head_output_frames":loaded.report.head_frames,"read_latency_samples":loaded.report.latency_samples,"read_latency_p95_us":loaded.report.latency_p95.as_micros(),"io_before":before,"io_after":io(),"cache_scope":"process heads fresh; OS/storage caches uncontrolled; no eviction","audio_or_source_written":false})
            );
        }
        Err(error) => {
            eprintln!("eager preparation failed: {error}");
            std::process::exit(1);
        }
    }
}
