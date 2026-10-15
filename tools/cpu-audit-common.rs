include!("cpu-audit-schedules.rs");
// Attribute callback deadline outliers without counting a sleeping Lua owner as audio CPU.
#[cfg(target_os = "linux")]
fn thread_cpu_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) } != 0 {
        return 0;
    }
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}
#[cfg(not(target_os = "linux"))]
fn thread_cpu_ns() -> u64 {
    0
}

// Adapt W13 d0c89ffb's callback counters; pacing sleeps stay outside the sample.
#[cfg(target_os = "linux")]
fn thread_switches() -> Option<[u64; 2]> {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_THREAD, &mut usage) } != 0 {
        return None;
    }
    Some([
        usage.ru_nvcsw.try_into().ok()?,
        usage.ru_nivcsw.try_into().ok()?,
    ])
}
#[cfg(not(target_os = "linux"))]
fn thread_switches() -> Option<[u64; 2]> {
    None
}

fn switch_delta(before: Option<[u64; 2]>, after: Option<[u64; 2]>) -> Option<[u64; 2]> {
    let (before, after) = (before?, after?);
    Some([
        after[0].checked_sub(before[0])?,
        after[1].checked_sub(before[1])?,
    ])
}

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    time::{Duration, Instant},
};
struct Counting;
thread_local! {static COUNT:Cell<bool>=const{Cell::new(false)};static CALLS:Cell<u64>=const{Cell::new(0)};}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if COUNT.get() {
            CALLS.set(CALLS.get() + 1)
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if COUNT.get() {
            CALLS.set(CALLS.get() + 1)
        }
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
fn proc_value(file: &str, key: &str) -> u64 {
    std::fs::read_to_string(file)
        .unwrap()
        .lines()
        .find_map(|s| {
            s.strip_prefix(key)?
                .trim()
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
        .unwrap_or(0)
}
fn quantiles(mut times: Vec<u64>) -> Value {
    assert!(!times.is_empty());
    times.sort_unstable();
    let n = times.len();
    json!({"blocks":n,"p50_us":times[n/2] as f64/1000.,"p99_us":times[(n-1)*99/100] as f64/1000.,"p999_us":times[(n-1)*999/1000] as f64/1000.,"max_us":times[n-1] as f64/1000.})
}
#[cfg(target_os = "linux")]
fn process_cpu_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    assert_eq!(unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut ts) }, 0);
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}
#[cfg(not(target_os = "linux"))]
fn process_cpu_ns() -> u64 { 0 }
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--check") {
        assert_eq!(quantiles(vec![1000, 2000, 3000])["p50_us"], 2.);
        assert_eq!(switch_delta(Some([3, 5]), Some([4, 9])), Some([1, 4]));
        assert_eq!(switch_delta(Some([3, 5]), Some([3, 5])), Some([0, 0]));
        assert_eq!(switch_delta(None, Some([0, 0])), None);
        assert_eq!(switch_delta(Some([0, 0]), None), None);
        assert_eq!(switch_delta(Some([3, 5]), Some([2, 9])), None);
        assert_eq!(switch_delta(Some([3, 5]), Some([4, 4])), None);
        #[cfg(target_os = "linux")]
        assert!(
            switch_delta(thread_switches(), thread_switches()).is_some(),
            "thread switch counters must be available and monotonic"
        );
        #[cfg(target_os = "linux")]
        assert!(
            thread_cpu_ns() > 0,
            "audio-thread CPU clock must be available for miss attribution"
        );
        cpu_checks();
        println!("ok");
        return;
    }
    assert_eq!(
        args.len(),
        4,
        "PATH BLOCK piano|strings|fx|fast-repeat|legato|cold-jump|held-stress|host-300"
    );
    let block: usize = args[2].parse().unwrap();
    assert!([32, 64, 256].contains(&block));
    let schedule = audit_schedule(&args[3]);
    let scheduling_diagnostic = std::env::var("KONTRA_HOST_SCHED_DIAGNOSTIC").as_deref() == Ok("1");
    if scheduling_diagnostic {
        assert!(
            thread_switches().is_some(),
            "thread switch diagnostic unavailable on this platform"
        );
    }
    let start = Instant::now();
    let (mut p, mut info) = Player::load(Path::new(&args[1]));
    if args[3] == "host-300" {
        assert_eq!(info["engine"], "v2", "exact-host-ID schedule requires the typed V2 event adapter");
    }
    info["schedule"] = json!(&args[3]);
    info["requested_host_note_ids"] = json!(if args[3] == "host-300" { 300 } else { 0 });
    info["load_seconds"] = json!(start.elapsed().as_secs_f64());
    info["rss_kb_loaded"] = json!(proc_value("/proc/self/status", "VmRSS:"));
    let mut idle = Vec::with_capacity(1000);
    for _ in 0..schedule.idle_blocks {
        let t = Instant::now();
        p.begin(block);
        std::hint::black_box(p.render(block));
        idle.push(t.elapsed().as_nanos() as u64)
    }
    let events = schedule.events;
    let blocks = schedule.frames.div_ceil(block).max(6000);
    let (mut all, mut steady, mut per_voice) = (
        Vec::with_capacity(blocks),
        Vec::with_capacity(blocks),
        Vec::with_capacity(blocks),
    );
    let (mut next, mut peak, mut voice_sum, mut voice_peak, mut misses) =
        (0, 0.0f32, 0usize, 0usize, 0usize);
    let (mut steady_voices_min, mut steady_voices_max, mut steady_audible_blocks) = (usize::MAX, 0, 0);
    let (mut process_cpu, mut caller_cpu, mut event_times, mut render_times) = (
        Vec::with_capacity(blocks), Vec::with_capacity(blocks), Vec::with_capacity(blocks), Vec::with_capacity(blocks),
    );
    let (mut allocations, mut event_allocations) = (0u64, 0u64);
    let mut miss_detail = Vec::with_capacity(6000);
    // At least one event belongs to every recorded callback, so the input
    // schedule bounds this control-side allocation. Store after timing ends.
    let mut event_detail = Vec::with_capacity(events.len());
    let (io0, rchar0) = (
        proc_value("/proc/self/io", "read_bytes:"),
        proc_value("/proc/self/io", "rchar:"),
    );
    if let Ok(path) = std::env::var("CPU_AUDIT_READY") {
        std::fs::write(path, std::process::id().to_string()).unwrap();
        std::thread::sleep(Duration::from_millis(300))
    }
    if std::env::var_os("CPU_AUDIT_READY").is_some() {
        info["profile_pace_unix_ns"] = json!(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                .to_string()
        );
    }
    // Optional metadata outside the timed/allocator-counted section. Diagnostic
    // runs are tagged separately from the original performance cells.
    let trace_stream = std::env::var_os("CPU_AUDIT_TRACE_STREAM").is_some();
    let mut stream_trace = Vec::with_capacity(if trace_stream {
        schedule.frames.div_ceil(block)
    } else {
        0
    });
    let mut previous_underruns = 0;
    let pace = Instant::now();
    let phase_cpu_start = process_cpu_ns();
    for begin in (0..schedule.frames).step_by(block) {
        let before = CALLS.get();
        COUNT.set(true);
        let switches_before = if scheduling_diagnostic {
            thread_switches()
        } else {
            None
        };
        let cpu_start = thread_cpu_ns();
        let process_start = process_cpu_ns();
        let t = Instant::now();
        let events_before = next;
        p.begin(block);
        while next < events.len() && events[next].0 < begin + block {
            let (_, s, a, b) = events[next];
            p.event(s, a, b);
            next += 1
        }
        let after_events = CALLS.get();
        let event_ns = t.elapsed().as_nanos() as u64;
        event_allocations += after_events - before;
        let v = p.voices();
        let got = p.render(block);
        if schedule.steady.contains(&begin) {
            steady_voices_min = steady_voices_min.min(v);
            steady_voices_max = steady_voices_max.max(v);
            steady_audible_blocks += usize::from(got > 0.);
        }
        let ns = t.elapsed().as_nanos() as u64;
        let cpu_ns = thread_cpu_ns().saturating_sub(cpu_start);
        caller_cpu.push(cpu_ns);
        process_cpu.push(process_cpu_ns().saturating_sub(process_start));
        event_times.push(event_ns);
        render_times.push(ns.saturating_sub(event_ns));
        let switches_after = if scheduling_diagnostic {
            thread_switches()
        } else {
            None
        };
        COUNT.set(false);
        if next != events_before {
            event_detail.push((begin, next - events_before, event_ns, ns, cpu_ns, v));
        }
        if ns > block as u64 * 1_000_000_000 / 48000 {
            miss_detail.push((
                begin,
                ns,
                cpu_ns,
                v,
                switch_delta(switches_before, switches_after),
            ));
        }
        allocations += CALLS.get() - after_events;
        if trace_stream {
            let underruns = p.problems()["underruns"].as_u64().unwrap();
            if underruns != previous_underruns {
                stream_trace.push((begin, underruns - previous_underruns, ns));
                previous_underruns = underruns;
            }
        }
        peak = peak.max(got);
        voice_sum += v;
        voice_peak = voice_peak.max(v);
        misses += usize::from(ns > block as u64 * 1_000_000_000 / 48000);
        all.push(ns);
        if schedule.steady.contains(&begin) && v > 0 {
            steady.push(ns);
            per_voice.push(ns / v as u64)
        }
        if let Some(wait) = (pace + Duration::from_secs_f64((begin + block) as f64 / 48000.))
            .checked_duration_since(Instant::now())
        {
            std::thread::sleep(wait)
        }
    }
    if let Ok(path) = std::env::var("CPU_AUDIT_FINISHED") {
        // Stop instruction sampling before JSON construction and Player teardown.
        std::fs::write(path, "done").unwrap();
        std::thread::sleep(Duration::from_millis(300));
    }
    info["path"] = json!(args[1]);
    info["block"] = json!(block);
    info["scenario"] = json!(args[3]);
    info["schedule_frames"] = json!(schedule.frames);
    if !idle.is_empty() {
        info["idle"] = quantiles(idle);
    }
    info["all"] = quantiles(all.clone());
    let audio_ns = schedule.frames as f64 / 48000. * 1e9;
    info["whole_phase_process_cpu_percent_core"] = json!(process_cpu_ns().saturating_sub(phase_cpu_start) as f64 / audio_ns * 100.);
    info["all_workers_cpu"] = quantiles(process_cpu.clone());
    info["caller_cpu"] = quantiles(caller_cpu);
    info["all_workers_cpu_mean_percent_core"] = json!(process_cpu.iter().map(|n| *n as f64).sum::<f64>() / audio_ns * 100.);
    info["begin_and_events"] = quantiles(event_times);
    info["render_and_finish"] = quantiles(render_times);
    info["event_callback_detail"] = json!(event_detail.into_iter().map(|(frame, count, events_ns, callback_ns, caller_ns, voices)| json!({
        "frame":frame, "events":count, "begin_and_events_wall_us":events_ns as f64 / 1000.,
        "callback_wall_us":callback_ns as f64 / 1000., "callback_caller_cpu_us":caller_ns as f64 / 1000., "voices":voices,
    })).collect::<Vec<_>>());
    info["timing_scope"] = json!(format!("{} core adapter callback including witness PCM observation/probes; excludes outer plugin process, UI and host; whole-phase CPU includes pacing/harness/decoder work", info["engine"].as_str().unwrap_or("unknown")));
    if !steady.is_empty() {
        info["steady_voices_min"] = json!(steady_voices_min);
        info["steady_voices_max"] = json!(steady_voices_max);
        info["steady_audible_blocks"] = json!(steady_audible_blocks);
        info["steady"] = quantiles(steady);
        info["amortized_per_voice"] = quantiles(per_voice)
    }
    info["peak"] = json!(peak);
    info["voices_mean"] = json!(voice_sum as f64 / all.len() as f64);
    info["voices_peak"] = json!(voice_peak);
    info["deadline_misses"] = json!(misses);
    info["thread_cpu_clock"] = json!(if cfg!(target_os = "linux") {
        "CLOCK_THREAD_CPUTIME_ID"
    } else {
        "unavailable"
    });
    info["scheduling_diagnostic"] = json!(scheduling_diagnostic);
    if scheduling_diagnostic {
        info["timing_status"] = json!("DIAGNOSTIC-NOT-ACCEPTANCE");
    }
    info["deadline_miss_detail"] = json!(miss_detail.iter().map(|&(frame, wall_ns, cpu_ns, voices, switches)| json!({
        "frame":frame, "wall_us":wall_ns as f64 / 1000., "thread_cpu_us":cpu_ns as f64 / 1000., "voices":voices,
        "voluntary_switches":switches.map(|s| s[0]), "involuntary_switches":switches.map(|s| s[1]),
        "phase":if frame == 0 { "first_block" } else if frame < 12000 { "startup" } else if frame < 48000 { "steady" } else if frame < 144000 { "sustain" } else { "release" },
    })).collect::<Vec<_>>());
    info["render_heap_calls"] = json!(allocations);
    info["event_heap_calls"] = json!(event_allocations);
    info["problems"] = p.problems();
    if trace_stream {
        info["diagnostic_stream_trace"] = json!(stream_trace);
    }
    info["rss_kb_final"] = json!(proc_value("/proc/self/status", "VmRSS:"));
    info["disk_read_bytes"] = json!(proc_value("/proc/self/io", "read_bytes:") - io0);
    info["rchar_bytes"] = json!(proc_value("/proc/self/io", "rchar:") - rchar0);
    println!("{}", info);
}
