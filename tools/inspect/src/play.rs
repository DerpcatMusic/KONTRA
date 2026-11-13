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
const MAX_BLOCK: usize = 4096;
const TAIL_SECONDS: u64 = 20;

pub fn run(path: &Path, scenario_name: &str, oracle: bool) -> Result<(Value, String)> {
    let events = if scenario_name == "user" {
        scenario::user()
    } else {
        scenario::file(Path::new(scenario_name))?
    };
    run_events(path, scenario_name, events, oracle, false)
}

pub fn segment(path: &Path, part: u64, oracle: bool) -> Result<(Value, String)> {
    let names = [
        "legato slow",
        "legato fast",
        "chord slow",
        "chord fast",
        "repeat slow",
        "repeat fast",
    ];
    let name = names
        .get(usize::try_from(part)?)
        .context("invalid segment")?;
    run_events(path, name, scenario::user_segment(part), oracle, true)
}

fn run_events(
    path: &Path,
    scenario_name: &str,
    events: Vec<scenario::Input>,
    oracle: bool,
    first_onset: bool,
) -> Result<(Value, String)> {
    let end = events.last().map_or(0, |e| e.frame);
    ensure!(end <= u64::from(RATE) * 3600, "scenario exceeds one hour");
    let last_off = events
        .iter()
        .filter(|e| e.description.get("type").and_then(Value::as_str) == Some("note_off"))
        .map(|e| e.frame)
        .max()
        .context("scenario needs a note-off")?;
    let stop = end.max(last_off + TAIL_SECONDS * u64::from(RATE));
    let mut ir = kontra_host::instrument::read(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    // KONTRA_SCRIPT_SOURCE=<dir>: script n's source is <dir>/n.ksp when that file exists
    // (the oracle capture loads the same files, so both sides run the same edited script).
    // A file past the last script plays in the next empty slot (a probe).
    if let Some(dir) = std::env::var_os("KONTRA_SCRIPT_SOURCE") {
        for n in 0..5 {
            let Ok(source) = std::fs::read_to_string(Path::new(&dir).join(format!("{n}.ksp")))
            else {
                continue;
            };
            match ir.scripts.get_mut(n) {
                Some(script) => script.source = source,
                None => ir.scripts.push(kontra_engine::ir::Script {
                    name: format!("probe {n}"),
                    slot: n,
                    dialect: kontra_engine::ir::Dialect::Ksp,
                    source,
                    saved: Vec::new(),
                    data: Vec::new(),
                    links: Vec::new(),
                }),
            }
        }
    }
    let samples: Vec<_> = ir.samples.iter().map(|s| s.name.clone()).collect();
    let group_samples: Vec<Vec<usize>> = ir
        .groups
        .iter()
        .map(|g| g.zones.iter().map(|z| z.sample).collect())
        .collect();
    let mut store = Store::new(StoreConfig::DEFAULT).context("creating sample store")?;
    let loaded = kontra_host::instrument::build(ir, RATE, &mut store);
    // Each script slot's symbol names in id order, as the engine compiled
    // them: a traced action's argument `SYMBOL_BASE + n` is name n.
    let script_symbols = loaded.symbols.clone();
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
    // KONTRA_INSPECT_LEAN=1: no trace, no per-block JSON: only the audio
    // thread's own work, for timing and profiles.
    let lean = std::env::var_os("KONTRA_INSPECT_LEAN").is_some();
    let mut engine = if lean {
        Engine::new(Arc::clone(&plan), stream.clone(), RATE)
    } else {
        Engine::new_traced(Arc::clone(&plan), stream.clone(), RATE, 262144)
    };
    // Lean timing runs as the host does: no waiting for sample data inside a block.
    engine.set_offline_wait(!lean);
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
    let mut route_events = Vec::new();
    let mut missing_blocks = Vec::new();
    let mut settle_timeouts = 0u64;
    let mut peak = 0f32;
    let mut jump = 0f32;
    let mut clipped = 0u64;
    let mut prior = [0f32; 2];
    // KONTRA_INSPECT_BLOCK=N: the host's block size, frames (default 128).
    let block = std::env::var("KONTRA_INSPECT_BLOCK")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map_or(BLOCK, |n| n.clamp(1, MAX_BLOCK));
    let mut audio = vec![[0f32; 2]; block];
    // Each block's time in `Engine::process`, ns, with where it was.
    let mut block_times: Vec<(u64, u64, usize, [u64; 3])> = Vec::new();
    let mut rendered = Vec::new();
    let mut process_time = Duration::ZERO;
    while at < stop {
        let frames = usize::try_from((stop - at).min(block as u64))?;
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
        let script_before = engine.stats();
        let timer = (std::time::Instant::now(), on_cpu());
        engine.process(&mut audio[..frames], &timed);
        let spent = timer.0.elapsed();
        process_time += spent;
        // Time on the CPU when the kernel reports it: a block the machine
        // preempted is not a spike of ours.
        let cpu = on_cpu()
            .zip(timer.1)
            .map(|(end, start)| end.saturating_sub(start));
        block_times.push((
            cpu.unwrap_or_else(|| u64::try_from(spent.as_nanos()).unwrap_or(u64::MAX)),
            at,
            timed.len(),
            {
                let now = engine.stats();
                [
                    now.script_starts - script_before.script_starts,
                    now.script_resumes - script_before.script_resumes,
                    now.script_turns - script_before.script_turns,
                ]
            },
        ));
        stage_events.extend(
            engine
                .trace_events()
                .iter()
                .map(|event| super::trace_output::event(event, &plan, &zone_samples, RATE)),
        );
        route_events.extend(
            engine
                .trace_route_values()
                .map(|v| super::trace_output::route(&v, &plan, &zone_samples)),
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
        if !lean && (cc_in_block || (at >= next_tail && at >= last_off)) {
            let voices: Vec<_> = engine
                .inspect_voices()
                .map(|v| {
                    json!({
                        "zone":v.zone, "sample":zone_samples.get(v.zone), "position":v.position,
                        "released":v.released, "gain_before_sample_and_fx":v.gain, "last_render_peak_before_group_fx":v.last_peak,
                        "source_frames_per_output_frame":v.step, "envelope_stage":v.envelope_stage,
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
            let voice_effect_targets: Vec<_> = engine.inspect_voice_effects().iter().map(|v| json!({
                "voice_slot":v.voice_slot, "zone":v.zone, "insert":v.insert, "target":format!("{:?}",v.target),
            })).collect();
            let shared_effects: Vec<_> = engine.inspect_shared_effects().iter().map(|v| json!({
                "scope":v.scope, "owner":v.owner, "slot":v.slot, "effect":format!("{:?}",v.effect),
            })).collect();
            trace.push(json!({"frame":at, "cc1":engine.controller(1), "cc11":engine.controller(11), "voices":voices, "eq_routes":routes, "all_routes":all_routes, "bus_routes":bus_routes, "voice_effect_targets":voice_effect_targets, "shared_effects":shared_effects}));
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
        "gain_before_sample_and_fx":v.gain, "last_render_peak_before_group_fx":v.last_peak,
        "source_frames_per_output_frame":v.step, "envelope_stage":v.envelope_stage})
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
    let timing = block_timing(&block_times, block);
    let stats = engine.stats();
    let wait_timeouts = engine.offline_wait_timeouts() + settle_timeouts;
    let stream_stats = stream.stats();
    let dropped = engine.trace_dropped();
    let complete = dropped == 0
        && stats.missing_frames == 0
        && wait_timeouts == 0
        && stream_stats.read_errors == 0
        && stream_stats.requests_refused == 0;
    // KONTRA_INSPECT_WAV=<file>: also write what v3 rendered, for offline A/B.
    if let Some(out) = std::env::var_os("KONTRA_INSPECT_WAV") {
        super::oracle::wav(Path::new(&out), &rendered)?;
    }
    let reference = if oracle {
        if complete {
            Some(
                match super::oracle::run(path, &events, &rendered, first_onset) {
                    Ok(value) => value,
                    Err(error) => json!({"status":"error", "reason":format!("{error:#}")}),
                },
            )
        } else {
            Some(
                json!({"status":"error", "reason":"v3 render incomplete; oracle comparison skipped"}),
            )
        }
    } else {
        None
    };
    let mut summary = format!(
        "{}: scenario {scenario_name}, {} input events\n  peak {peak:.6}, clipped samples {clipped}, max sample jump {jump:.6}\n  {} voices active {:.3} s after last note-off (per-voice last-render peak before group FX in JSON)\n  loudest remaining voice peak {loudest_remaining:.8}; EQ route snapshots: {}; max CC-window HF energy {burst_peak:.8}\n  missing sample frames: {}; offline wait timeouts: {wait_timeouts}; render complete: {complete}; audio FNV-1a: {fingerprint}\n",
        loaded.name,
        events.len(),
        left.len(),
        (stop - last_off) as f64 / f64::from(RATE),
        trace.len(),
        stats.missing_frames
    );
    summary.push_str(&super::routes::summary(&route_events));
    summary.push_str(&format!(
        "  block timing ({} frames, budget {:.0} us): p50 {:.0} us, p99 {:.0} us, max {:.0} us\n",
        block,
        timing["budget_us"].as_f64().unwrap_or(0.0),
        timing["p50_us"].as_f64().unwrap_or(0.0),
        timing["p99_us"].as_f64().unwrap_or(0.0),
        timing["max_us"].as_f64().unwrap_or(0.0)
    ));
    if let Some(reference) = &reference {
        let status = reference
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("error");
        let detail = reference
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("");
        let difference = reference
            .get("comparison")
            .and_then(|r| r.get("level_diff_db"))
            .and_then(Value::as_f64);
        let peak_difference = reference
            .get("comparison")
            .and_then(|r| r.get("peak_diff_db"))
            .and_then(Value::as_f64);
        let native_jump = reference
            .get("native_metrics")
            .and_then(|r| r.get("max_sample_jump"))
            .and_then(Value::as_f64);
        let db =
            |v: Option<f64>| v.map_or_else(|| "unavailable".to_owned(), |x| format!("{x:+.2} dB"));
        let jump = native_jump.map_or_else(|| "unavailable".to_owned(), |x| format!("{x:.6}"));
        summary.push_str(&format!(
            "  oracle: {status}; native minus v3 RMS {}, peak {}; native max jump {jump} {detail}\n",
            db(difference), db(peak_difference)
        ));
    }
    Ok((
        json!({"schema_version":1,"command":"play","instrument":path,"engine":"v3",
        "result":{"scenario":scenario_name,"rate":RATE,"events":events.iter().map(|e|json!({"frame":e.frame,"event":e.description})).collect::<Vec<_>>(),
        "trace":trace,"stage_events":stage_events,"script_symbols":script_symbols,"symbol_base":kontra_ksp::SYMBOL_BASE,"route_events":route_events,"route_event_semantics":"values evaluated during CC or note-on processing before smoothing, combining and clamping; dependency flags are structural","trace_events_dropped":dropped,"remaining_voices":left,"metrics":{"peak":peak,"clipped_samples":clipped,"max_sample_jump":jump,"cc_hf_bursts":bursts,"octave_bands":octave_bands,"octave_time":octave_time},
        "oracle":reference,"diagnostics":diagnostics,"engine_stats":format!("{stats:?}"), "process_ms":process_time.as_secs_f64() * 1e3, "block_timing":timing, "audio_s":stop as f64 / f64::from(RATE), "stream_stats":format!("{stream_stats:?}"), "missing_blocks":missing_blocks, "settle_timeouts":wait_timeouts, "render_complete":complete,"audio_fnv1a64":fingerprint,"unavailable":["instantaneous per-route smoothed values", "controller dependencies hidden inside arbitrary script state"]}}),
        summary,
    ))
}

/// Max, p50 and p99 of the per-block process time, and the five slowest blocks.
fn block_timing(times: &[(u64, u64, usize, [u64; 3])], block: usize) -> Value {
    let mut sorted: Vec<u64> = times.iter().map(|t| t.0).collect();
    sorted.sort_unstable();
    let at = |q: f64| {
        let i = ((sorted.len() as f64 * q).ceil() as usize).saturating_sub(1);
        sorted.get(i).copied().unwrap_or(0) as f64 / 1e3
    };
    let mut worst: Vec<_> = times.to_vec();
    worst.sort_unstable_by_key(|t| std::cmp::Reverse(t.0));
    let top: Vec<_> = worst
        .iter()
        .take(5)
        .map(|(ns, frame, events, [starts, resumes, turns])| {
            json!({"us":*ns as f64 / 1e3, "frame":frame, "events":events,
                "script_starts":starts, "script_resumes":resumes, "script_loop_turns":turns})
        })
        .collect();
    let budget = block as f64 / f64::from(RATE) * 1e6;
    json!({"block_frames":block, "budget_us":budget, "blocks":times.len(),
        "p50_us":at(0.5), "p99_us":at(0.99), "max_us":at(1.0), "top":top})
}

/// This thread's CPU time so far, ns (`CLOCK_THREAD_CPUTIME_ID`).
fn on_cpu() -> Option<u64> {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `t` is a valid timespec the call fills in.
    let ok = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) } == 0;
    ok.then(|| {
        u64::try_from(t.tv_sec).unwrap_or(0) * 1_000_000_000 + u64::try_from(t.tv_nsec).unwrap_or(0)
    })
}
