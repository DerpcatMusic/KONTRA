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
    json!({"blocks":n,"p50_us":times[n/2] as f64/1000.,"p99_us":times[(n-1)*99/100] as f64/1000.,"max_us":times[n-1] as f64/1000.})
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--check") {
        assert_eq!(quantiles(vec![1000, 2000, 3000])["p50_us"], 2.);
        cpu_checks();
        println!("ok");
        return;
    }
    assert_eq!(args.len(), 4, "PATH BLOCK piano|strings|fx");
    let block: usize = args[2].parse().unwrap();
    assert!([32, 64, 256].contains(&block));
    let start = Instant::now();
    let (mut p, mut info) = Player::load(Path::new(&args[1]));
    info["load_seconds"] = json!(start.elapsed().as_secs_f64());
    info["rss_kb_loaded"] = json!(proc_value("/proc/self/status", "VmRSS:"));
    let mut idle = Vec::with_capacity(1000);
    for _ in 0..1000 {
        let t = Instant::now();
        std::hint::black_box(p.render(block));
        idle.push(t.elapsed().as_nanos() as u64)
    }
    let keys: Vec<u8> = if args[3] == "piano" {
        vec![48, 52, 55, 60, 64, 67, 72, 76]
    } else {
        (48..60).collect()
    };
    let mut events = vec![(0, 0xb0, 1, 110), (0, 0xb0, 11, 127), (0, 0xb0, 64, 127)];
    for &key in &keys {
        events.push((0, 0x90, key, 100));
        events.push((48000, 0x80, key, 0))
    }
    events.push((144000, 0xb0, 64, 0));
    events.sort_by_key(|e| e.0);
    let (mut all, mut steady, mut per_voice) = (
        Vec::with_capacity(6000),
        Vec::with_capacity(6000),
        Vec::with_capacity(6000),
    );
    let (mut next, mut peak, mut voice_sum, mut voice_peak, mut misses) =
        (0, 0.0f32, 0usize, 0usize, 0usize);
    let (mut allocations, mut event_allocations) = (0u64, 0u64);
    let (io0, rchar0) = (
        proc_value("/proc/self/io", "read_bytes:"),
        proc_value("/proc/self/io", "rchar:"),
    );
    if let Ok(path) = std::env::var("CPU_AUDIT_READY") {
        std::fs::write(path, std::process::id().to_string()).unwrap();
        std::thread::sleep(Duration::from_millis(300))
    }
    // Optional metadata outside the timed/allocator-counted section. Diagnostic
    // runs are tagged separately from the original performance cells.
    let trace_stream = std::env::var_os("CPU_AUDIT_TRACE_STREAM").is_some();
    let mut stream_trace = Vec::with_capacity(if trace_stream { 6000 } else { 0 });
    let mut previous_underruns = 0;
    let pace = Instant::now();
    for begin in (0..192000).step_by(block) {
        let before = CALLS.get();
        COUNT.set(true);
        let t = Instant::now();
        while next < events.len() && events[next].0 < begin + block {
            let (_, s, a, b) = events[next];
            p.event(s, a, b);
            next += 1
        }
        let after_events = CALLS.get();
        event_allocations += after_events - before;
        let v = p.voices();
        let got = p.render(block);
        let ns = t.elapsed().as_nanos() as u64;
        COUNT.set(false);
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
        if (12000..48000).contains(&begin) && v > 0 {
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
    info["idle"] = quantiles(idle);
    info["all"] = quantiles(all.clone());
    if !steady.is_empty() {
        info["steady"] = quantiles(steady);
        info["amortized_per_voice"] = quantiles(per_voice)
    }
    info["peak"] = json!(peak);
    info["voices_mean"] = json!(voice_sum as f64 / all.len() as f64);
    info["voices_peak"] = json!(voice_peak);
    info["deadline_misses"] = json!(misses);
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
