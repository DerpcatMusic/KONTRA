//! Offline performance and between-block engine observations.
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use kontra_engine::{Engine, Prepared, SampleTime, Timed};
use kontra_stream::{Store, StoreConfig};
use serde_json::{Value, json};

use super::{RATE, scenario};

const BLOCK: usize = 128;
const TAIL_SECONDS: u64 = 20;

pub fn run(path: &Path, scenario_name: &str) -> Result<(Value, String)> {
    let events = if scenario_name == "user" {
        scenario::user()
    } else {
        scenario::file(Path::new(scenario_name))?
    };
    let end = events.last().map_or(0, |e| e.frame);
    ensure!(end <= u64::from(RATE) * 3600, "scenario exceeds one hour");
    let last_off = events
        .iter()
        .filter(|e| e.description.get("type").and_then(Value::as_str) == Some("note_off"))
        .map(|e| e.frame)
        .max()
        .context("scenario needs a note-off")?;
    let stop = end.max(last_off + TAIL_SECONDS * u64::from(RATE));
    let ir = kontra_host::instrument::read(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    let samples: Vec<_> = ir.samples.iter().map(|s| s.name.clone()).collect();
    let group_samples: Vec<Vec<usize>> = ir
        .groups
        .iter()
        .map(|g| g.zones.iter().map(|z| z.sample).collect())
        .collect();
    let mut store = Store::new(StoreConfig::DEFAULT).context("creating sample store")?;
    let loaded = kontra_host::instrument::build(ir, RATE, &mut store);
    let diagnostics: Vec<_> = loaded
        .diagnostics
        .iter()
        .map(|d| json!({"impact":format!("{:?}",d.impact), "feature":d.feature, "detail":d.detail}))
        .collect();
    let plan = Arc::new(loaded.plan);
    let zone_samples: Vec<_> = plan
        .zones
        .iter()
        .map(|z| {
            usize::try_from(z.group)
                .ok()
                .and_then(|g| group_samples.get(g))
                .and_then(|g| g.get(usize::from(z.member)))
                .and_then(|id| samples.get(*id))
                .cloned()
        })
        .collect();
    let stream = store.client();
    let mut engine = Engine::new_traced(Arc::clone(&plan), stream.clone(), RATE, 65536);
    engine.set_offline_wait(true);
    let mut at = 0u64;
    let mut next = 0usize;
    let mut next_tail = last_off;
    let mut trace = Vec::new();
    let mut stage_events: Vec<Value> = engine
        .trace_events()
        .iter()
        .map(|event| super::trace_output::event(event, &plan, &zone_samples, RATE))
        .collect();
    engine.clear_trace();
    let mut missing_blocks = Vec::new();
    let mut settle_timeouts = 0u64;
    let mut peak = 0f32;
    let mut jump = 0f32;
    let mut clipped = 0u64;
    let mut prior = [0f32; 2];
    let mut audio = [[0f32; 2]; BLOCK];
    let mut rendered = Vec::new();
    while at < stop {
        let frames = usize::try_from((stop - at).min(BLOCK as u64))?;
        let until = at + frames as u64;
        let mut timed = Vec::new();
        let mut cc_in_block = false;
        while let Some(e) = events.get(next).filter(|e| e.frame < until) {
            cc_in_block |= e.description.get("type").and_then(Value::as_str) == Some("cc");
            timed.push(Timed {
                at: SampleTime(e.frame),
                body: e.event,
            });
            next += 1;
        }
        if !stream.settle(Duration::from_secs(5)) {
            settle_timeouts += 1;
        }
        let before = engine.stats().missing_frames;
        let stream_before = stream.stats();
        engine.process(&mut audio[..frames], &timed);
        stage_events.extend(
            engine
                .trace_events()
                .iter()
                .map(|event| super::trace_output::event(event, &plan, &zone_samples, RATE)),
        );
        engine.clear_trace();
        let missed = engine.stats().missing_frames.saturating_sub(before);
        if missed > 0 {
            missing_blocks.push(json!({"start_frame":at, "missing_frames":missed,
                "events":format!("{timed:?}"), "stream_before":format!("{stream_before:?}"),
                "stream_after":format!("{:?}",stream.stats()),
                "voices":engine.inspect_voices().filter(|v| v.missing_frames > 0).map(|v| json!({"zone":v.zone,"sample":zone_samples.get(v.zone),"position":v.position,"missing_frames":v.missing_frames})).collect::<Vec<_>>() }));
        }
        let mut requests = Vec::new();
        engine.drain_requests(|r| requests.push(r));
        for request in requests {
            engine.install(Prepared::new(request, Some(&plan), RATE));
        }
        rendered.extend_from_slice(&audio[..frames]);
        for frame in &audio[..frames] {
            for (sample, previous) in frame.iter().zip(&mut prior) {
                peak = peak.max(sample.abs());
                jump = jump.max((sample - *previous).abs());
                clipped += u64::from(sample.abs() > 1.0);
                *previous = *sample;
            }
        }
        at = until;
        if cc_in_block || (at >= next_tail && at >= last_off) {
            let voices: Vec<_> = engine
                .inspect_voices()
                .map(|v| {
                    json!({
                        "zone":v.zone, "sample":zone_samples.get(v.zone), "position":v.position,
                        "released":v.released, "gain_before_sample_and_fx":v.gain, "last_render_peak_before_group_fx":v.last_peak,
                    })
                })
                .collect();
            let routes: Vec<_> = engine.inspect_eq_routes().iter().map(|r| json!({
                "voice_slot":r.voice_slot, "zone":r.zone, "name":r.name, "insert":r.insert, "band":r.band,
                "routed_source":r.source, "contribution_db":r.contribution_db, "band_gain_db":r.band_gain_db,
            })).collect();
            let all_routes: Vec<_> = engine
                .inspect_routes()
                .iter()
                .map(|r| {
                    json!({
                        "voice_slot":r.voice_slot, "zone":r.zone, "name":r.name, "route":super::routes::route(r.kind),
                        "current_source":r.source, "current_routed_source":r.routed,
                    })
                })
                .collect();
            let bus_routes: Vec<_> = engine
                .inspect_bus_routes()
                .map(|r| {
                    json!({"bus":r.bus,
                "route":super::routes::route(r.kind), "last_applied_amount":r.amount})
                })
                .collect();
            trace.push(json!({"frame":at, "cc1":engine.controller(1), "cc11":engine.controller(11), "voices":voices, "eq_routes":routes, "all_routes":all_routes, "bus_routes":bus_routes}));
            if at >= next_tail && at >= last_off {
                next_tail += u64::from(RATE);
            }
        }
    }
    let left: Vec<_> = engine
        .inspect_voices()
        .map(|v| {
            json!({"zone":v.zone,
        "sample":zone_samples.get(v.zone), "position":v.position, "released":v.released,
        "gain_before_sample_and_fx":v.gain, "last_render_peak_before_group_fx":v.last_peak})
        })
        .collect();
    // Five milliseconds around each controller event, using the second
    // difference as a reproducible high-frequency burst measure.
    let window = usize::try_from(RATE / 200).unwrap_or(0);
    let bursts: Vec<_> = events.iter().filter(|e| e.description.get("type").and_then(Value::as_str) == Some("cc"))
        .filter_map(|e| {
            let frame = usize::try_from(e.frame).ok()?;
            let end = frame.saturating_add(window).min(rendered.len());
            let samples = rendered.get(frame.saturating_sub(window)..end)?;
            let energy: f64 = samples.windows(3).map(|w| {
                let [a, b, c] = w else { return 0.0 };
                a.iter().zip(b).zip(c).map(|((&x, &y), &z)| {
                    let d = f64::from(z - 2.0 * y + x);
                    d * d
                }).sum::<f64>()
            }).sum();
            Some(json!({"frame": e.frame, "event": e.description, "hf_second_difference_energy": energy}))
        }).collect();
    // Stable FNV-1a of the exact little-endian stereo f32 sample bits.
    let mut fingerprint = 0xcbf29ce484222325u64;
    for sample in rendered.iter().flatten() {
        for byte in sample.to_bits().to_le_bytes() {
            fingerprint = (fingerprint ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    let fingerprint = format!("{fingerprint:016x}");
    let loudest_remaining = engine
        .inspect_voices()
        .map(|v| v.last_peak)
        .fold(0.0_f32, f32::max);
    let burst_peak = bursts
        .iter()
        .filter_map(|b| b.get("hf_second_difference_energy")?.as_f64())
        .fold(0.0_f64, f64::max);
    let octave_bands = super::bands::octave_levels(&rendered, RATE);
    let octave_time: Vec<_> = rendered
        .chunks(RATE as usize)
        .enumerate()
        .map(|(n, audio)| {
            json!({
                "start_frame": n * RATE as usize, "frames": audio.len(),
                "bands":super::bands::octave_levels(audio, RATE),
            })
        })
        .collect();
    let stats = engine.stats();
    let wait_timeouts = engine.offline_wait_timeouts() + settle_timeouts;
    let stream_stats = stream.stats();
    let dropped = engine.trace_dropped();
    let complete = dropped == 0
        && stats.missing_frames == 0
        && wait_timeouts == 0
        && stream_stats.read_errors == 0
        && stream_stats.requests_refused == 0;
    let summary = format!(
        "{}: scenario {scenario_name}, {} input events\n  peak {peak:.6}, clipped samples {clipped}, max sample jump {jump:.6}\n  {} voices active {:.3} s after last note-off (per-voice last-render peak before group FX in JSON)\n  loudest remaining voice peak {loudest_remaining:.8}; EQ route snapshots: {}; max CC-window HF energy {burst_peak:.8}\n  missing sample frames: {}; offline wait timeouts: {wait_timeouts}; render complete: {complete}; audio FNV-1a: {fingerprint}\n",
        loaded.name,
        events.len(),
        left.len(),
        (stop - last_off) as f64 / f64::from(RATE),
        trace.len(),
        stats.missing_frames
    );
    Ok((
        json!({"schema_version":1,"command":"play","instrument":path,"engine":"v3",
        "result":{"scenario":scenario_name,"rate":RATE,"events":events.iter().map(|e|json!({"frame":e.frame,"event":e.description})).collect::<Vec<_>>(),
        "trace":trace,"stage_events":stage_events,"trace_events_dropped":dropped,"remaining_voices":left,"metrics":{"peak":peak,"clipped_samples":clipped,"max_sample_jump":jump,"cc_hf_bursts":bursts,"octave_bands":octave_bands,"octave_time":octave_time},
        "diagnostics":diagnostics,"engine_stats":format!("{stats:?}"), "stream_stats":format!("{stream_stats:?}"), "missing_blocks":missing_blocks, "settle_timeouts":wait_timeouts, "render_complete":complete,"audio_fnv1a64":fingerprint,"unavailable":["non-EQ applied route values"]}}),
        summary,
    ))
}
