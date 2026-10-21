//! Held-note CPU audit using the same loader, ingress and render as the existing harness.
use super::*;

const PATCHES: &[(&str, &str)] = &[
    (
        "afflatus",
        "Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/4 Horns KS.nki",
    ),
    (
        "conflux",
        "Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki",
    ),
    (
        "conflux-multi",
        "Conflux 1.1.0 [Native Instruments]/Multis/Hybrid Walk.nkm",
    ),
];

pub(super) fn metadata(name: &str, ir: &sampler_ir::Instrument) {
    // Only structural metadata: never serialize sample PCM, scripts or resource payloads.
    eprintln!(
        "{name}: groups {} zones {} modulators {} impulses {} unsupported {}",
        ir.groups.len(),
        ir.zones.len(),
        ir.modulators.len(),
        ir.impulses.len(),
        ir.unsupported.len()
    );
    for (i, group) in ir.groups.iter().enumerate() {
        eprintln!("group {i}: {}", group.name);
    }
}

fn voices(player: &Player) -> usize {
    match player {
        Player::Midi { rt, .. } => rt.voice_count(),
        Player::Uvi(p) => p.runtime().voice_count(),
    }
}

fn summary(mut times: Vec<u64>, block: usize) -> Value {
    times.sort_unstable();
    let deadline = block as f64 / RATE as f64 * 1e9;
    json!({"blocks": times.len(), "p50_us": percentile(&times, 0.5) as f64 / 1e3,
        "p99_us": percentile(&times, 0.99) as f64 / 1e3,
        "p999_us": percentile(&times, 0.999) as f64 / 1e3,
        "p50_percent_core": percentile(&times, 0.5) as f64 / deadline * 100.,
        "p99_percent_core": percentile(&times, 0.99) as f64 / deadline * 100.,
        "mean_percent_core": times.iter().sum::<u64>() as f64 / times.len() as f64 / deadline * 100.,
        "max_us": times.last().unwrap() / 1000,
        "deadline_misses": times.iter().filter(|&&ns| ns as f64 > deadline).count()})
}

/// Paced sampling: sleeping and metadata reads stay outside the timed section.
#[cfg(target_os = "linux")]
#[allow(
    unsafe_code,
    reason = "Read the thread CPU clock; no user/library data is exposed"
)]
fn cpu_ns(process: bool) -> u64 {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let clock = if process { libc::CLOCK_PROCESS_CPUTIME_ID } else { libc::CLOCK_THREAD_CPUTIME_ID };
    let result = unsafe { libc::clock_gettime(clock, &mut time) };
    assert_eq!(result, 0);
    time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64
}

#[cfg(not(target_os = "linux"))]
fn cpu_ns(_: bool) -> u64 {
    0
}

fn phase(players: &mut [Loaded], block: usize, seconds: f64) -> Value {
    let blocks = (seconds * RATE as f64 / block as f64).ceil() as usize;
    let mut times = Vec::with_capacity(blocks);
    let mut cpu_times = Vec::with_capacity(blocks);
    let mut process_times = Vec::with_capacity(blocks);
    let mut voice_min = usize::MAX;
    let mut voice_max = 0;
    let mut peak = 0.0f32;
    let mut energy = 0.0f64;
    let mut fingerprint = 0xcbf29ce484222325u64;
    let mut buffers = vec![vec![[0.0; 2]; block]; players.len()];
    let before = CALLS.get();
    let underruns_before: u64 = players.iter().map(|p| stats(&p.player).stream_underruns).sum();
    let phase_cpu_start = cpu_ns(true);
    let start = Instant::now();
    for i in 0..blocks {
        COUNTING.set(true);
        let p = cpu_ns(true);
        let c = cpu_ns(false);
        let t = Instant::now();
        for (loaded, buffer) in players.iter_mut().zip(&mut buffers) {
            render(&mut loaded.player, buffer, &[], &[], 0, &mut 0);
        }
        times.push(t.elapsed().as_nanos() as u64);
        cpu_times.push(cpu_ns(false) - c);
        process_times.push(cpu_ns(true) - p);
        COUNTING.set(false);
        let n = players.iter().map(|p| voices(&p.player)).sum();
        voice_min = voice_min.min(n);
        voice_max = voice_max.max(n);
        for buffer in &buffers {
            for x in buffer.iter().flatten() {
                peak = peak.max(x.abs());
                energy += f64::from(*x).powi(2);
                fingerprint = (fingerprint ^ u64::from(x.to_bits())).wrapping_mul(0x100000001b3);
            }
        }
        let due = start + Duration::from_secs_f64((i + 1) as f64 * block as f64 / RATE as f64);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    let phase_cpu = cpu_ns(true) - phase_cpu_start;
    let mut result = summary(times, block);
    result["thread_cpu"] = summary(cpu_times, block);
    result["all_workers_cpu_during_render"] = summary(process_times, block);
    result["whole_phase_process_cpu_percent_core"] = json!(phase_cpu as f64 / (blocks * block) as f64 * RATE as f64 / 1e9 * 100.);
    result["voices_min"] = json!(voice_min);
    result["voices_max"] = json!(voice_max);
    result["peak"] = json!(peak);
    result["rms"] = json!((energy / (blocks * block * 2 * players.len()) as f64).sqrt());
    result["pcm_fingerprint"] = json!(format!("{fingerprint:016x}"));
    result["audio_thread_heap_calls"] = json!(CALLS.get() - before);
    let underruns_after: u64 = players.iter().map(|p| stats(&p.player).stream_underruns).sum();
    result["stream_underruns_during_phase"] = json!(underruns_after - underruns_before);
    result["stream_underruns_before_phase"] = json!(underruns_before);
    result
}

fn events(players: &mut [Loaded], keys: &[u8], on: bool) {
    let mut messages = if on { expression() } else { Vec::new() };
    messages.extend(keys.iter().map(|&key| {
        (
            0.,
            [if on { 0x90 } else { 0x80 }, key, if on { 100 } else { 0 }],
        )
    }));
    for loaded in players {
        // Use the existing MIDI ingress and effect drain; event blocks are outside the steady-phase timings.
        play(&mut loaded.player, &messages, 64, 64);
    }
}

pub(super) fn run(args: &[String]) -> Result<(), String> {
    let mut only = None;
    let mut block = None;
    let mut seconds = 3.;
    let mut notes = None;
    let mut threads = 1;
    let mut out = None;
    for pair in args.chunks(2) {
        let [key, value] = pair else {
            return Err("sustain options need values".into());
        };
        match key.as_str() {
            "--only" => only = Some(value.as_str()),
            "--block" => block = Some(value.parse::<usize>().map_err(fail)?),
            "--seconds" => seconds = value.parse::<f64>().map_err(fail)?,
            "--notes" => notes = Some(value.parse::<usize>().map_err(fail)?),
            "--threads" => threads = value.parse::<usize>().map_err(fail)?,
            "--out" => out = Some(value),
            _ => return Err(format!("unknown sustain option {key}")),
        }
    }
    if !seconds.is_finite()
        || seconds <= 0.
        || block.is_some_and(|b| ![64, 256].contains(&b))
        || notes.is_some_and(|n| ![1, 8].contains(&n))
        || ![1, 2, 4].contains(&threads)
    {
        return Err("seconds > 0, block 64|256, notes 1|8, threads 1|2|4 required".into());
    }
    if only.is_some_and(|n| !PATCHES.iter().any(|(name, _)| *name == n)) {
        return Err("unknown sustain scenario".into());
    }
    let mut cells = Vec::new();
    let mut failures = Vec::new();
    for &(name, path) in PATCHES.iter().filter(|(n, _)| only.is_none_or(|o| o == *n)) {
        for b in [64, 256]
            .into_iter()
            .filter(|b| block.is_none_or(|n| n == *b))
        {
            for n in [1, 8].into_iter().filter(|n| notes.is_none_or(|v| v == *n)) {
                let keys = &[60, 62, 64, 65, 67, 69, 71, 72][..n];
                let messages: Vec<_> = keys.iter().map(|&k| (0., [0x90, k, 100])).collect();
                let count = if name == "conflux-multi" {
                    sampler_kontakt::read_multi(
                        &root(KONTAKT, "/mnt/MAIN_STORAGE/Libraries/Kontakt").join(path),
                    )
                    .map_err(fail)?
                    .programs
                    .len()
                } else {
                    1
                };
                if count == 0 { return Err(format!("{name}: multi has no physical programs")); }
                EVENT_ERRORS.set(0);
                BEHAVIOR_ERRORS.set(0);
                SERVICE_ERRORS.set(0);
                SERVICE_MASK.set(0);
                STREAM_SERVICE_ERRORS.set(0);
                let mut players = Vec::new();
                for program in 0..count {
                    let scenario = Scenario {
                        name,
                        source: if name == "conflux-multi" {
                            Source::KontaktProgram(path, program)
                        } else {
                            Source::Kontakt(path)
                        },
                        seconds,
                        notes: expression,
                    };
                    players.push(load(&scenario, &messages, b)?);
                }
                for loaded in &mut players {
                    match &mut loaded.player {
                        Player::Midi { rt, .. } => rt.set_threads(Threads::Fixed(threads)),
                        Player::Uvi(_) => return Err("Kontakt sustain requires a MIDI runtime".into()),
                    }
                }
                eprintln!("{name} block {b} notes {n}: loaded {count} programs");
                // Drain finite bootstrap; recurring timers continue normally.
                let bootstrap = phase(&mut players, b, 1.);
                let idle = phase(&mut players, b, seconds);
                events(&mut players, keys, true);
                let attack = phase(&mut players, b, 1.);
                if let Ok(ready) = std::env::var("CPU_AUDIT_READY") {
                    std::fs::write(ready, std::process::id().to_string()).map_err(fail)?;
                    std::thread::sleep(Duration::from_millis(400));
                }
                let held = phase(&mut players, b, seconds);
                if let Ok(done) = std::env::var("CPU_AUDIT_FINISHED") {
                    std::fs::write(done, "done").map_err(fail)?;
                }
                events(&mut players, keys, false);
                let release = phase(&mut players, b, seconds);
                phase(&mut players, b, 8.);
                let post_release = phase(&mut players, b, seconds);
                let st: Vec<_> = players.iter().map(|p| stats(&p.player)).collect();
                let errors = EVENT_ERRORS.replace(0);
                let behavior_errors = BEHAVIOR_ERRORS.replace(0);
                let service_errors = SERVICE_ERRORS.replace(0);
                let service_mask = SERVICE_MASK.replace(0);
                let stream_service_errors = STREAM_SERVICE_ERRORS.replace(0);
                let underruns: u64 = st.iter().map(|s| s.stream_underruns).sum();
                if held["peak"].as_f64().unwrap_or(0.) < SILENT as f64
                    || errors > 0
                    || behavior_errors > 0
                    || service_errors > 0
                    || stream_service_errors > 0
                    || underruns > 0
                {
                    failures.push(format!(
                        "{name}/{b}/{n}: held peak {}, errors {errors}, behavior errors {behavior_errors}, external service refusals {service_errors} (mask {service_mask}), stream service errors {stream_service_errors}, underruns {underruns}",
                        held["peak"]
                    ));
                }
                cells.push(json!({"scenario":name,"block":b,"notes":n,"programs":count,
                    "bootstrap":bootstrap,"idle":idle,"attack":attack,"held":held,"release":release,"post_release":post_release,
                    "cold_starts":st.iter().map(|s| s.cold_starts).sum::<u64>(),
                    "voice_drops":st.iter().map(|s| s.voice_drops).sum::<u64>(),
                    "refused_starts":st.iter().map(|s| s.refused_starts).sum::<u64>(),
                    "stream_capacity_refusals":st.iter().map(|s| s.stream_capacity_refusals).sum::<u64>(),
                    "event_errors":errors,"behavior_errors":behavior_errors,"external_service_refusals":service_errors,
                    "external_service_mask":service_mask,"stream_service_errors":stream_service_errors,"stream_underruns":underruns}));
                eprintln!(
                    "{}",
                    serde_json::to_string(cells.last().unwrap()).map_err(fail)?
                );
            }
        }
    }
    let result = json!({"format":"sampler-perf-sustain-2","rate":RATE,"threads":threads,
        "seconds_per_phase":seconds,"release_settle_seconds":8,"cells":cells,"failures":failures,
        "external_service_mask_bits":{"1":"MIDI service","2":"array file service"},
        "ui_effects":"presentation effects drained without a UI; file/MIDI jobs retained and reported as failure",
        "key_range":"only sample zones intersecting held MIDI keys are loaded; script-generated out-of-range notes are not a full-library audit",
        "multi_routing":"all physical programs auditioned on MIDI channel 1; rack channel/fader routing is not modeled",
        "cpu_metrics":"wall and thread_cpu are callback deadline percentages; all_workers_cpu_during_render sums process CPU while render is bracketed; whole_phase_process_cpu_percent_core includes decoder workers, pacing, clocks, PCM fingerprinting and other harness overhead across the entire phase",
        "loadavg":std::fs::read_to_string("/proc/loadavg").ok()});
    let text = serde_json::to_string_pretty(&result).map_err(fail)?;
    if let Some(out) = out {
        std::fs::write(out, text).map_err(fail)?;
    } else {
        println!("{text}");
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percentages_use_one_core_real_time_at_48k() {
        let result = summary(vec![53333; 100], 256);
        assert!((result["p50_percent_core"].as_f64().unwrap() - 1.).abs() < 0.00001);
        assert_eq!(result["p99_us"], 53.333);
        assert_eq!(result["blocks"], 100);
    }
}
