use super::*;

fn table() -> Part {
    let script = sampler_ksp::compile(
        "on init declare ui_table %table[32768](1,1,127) end on",
        48000, sampler_ksp::Limits::LIBRARY, &[],
    ).unwrap();
    let view = script.view();
    let plan = script.bind(Prepared::new(48000, vec![], vec![], 1).unwrap()).unwrap();
    let limits = Limits::for_plan(&plan, 128, 8);
    let mut part = Part::new(Runtime::new(plan, limits).unwrap(), MixTree::instrument("capture")).unwrap();
    part.prepare_persistence(&[view], "").unwrap();
    part
}

#[test]
fn typed_readback_retains_at_most_one_transfer_chunk() {
    let part = table();
    let ingress = part.ui_controls.as_ref().unwrap();
    let retained: usize = ingress.captures.iter().map(Vec::capacity).sum();
    assert!(retained <= sampler_core::WIDGET_EDIT_CAPACITY,
        "typed readback retains {retained} Text-sized cells; its transfer must be bounded");
}

#[test]
fn unchanged_typed_readback_does_not_keep_queueing_captures() {
    let mut part = table();
    let mut ingress = part.ui_controls.take().unwrap();
    // Allow an initial refresh to finish, then observe the same generation again.
    for _ in 0..16 {
        ingress.refresh();
        while part.runtime.poll_control_update().unwrap().is_some() {}
        ingress.settle();
    }
    ingress.refresh();
    assert!(part.runtime.poll_control_update().unwrap().is_none(),
        "unchanged typed readback must not enqueue another audio-owner capture");
}

/// Numeric/count-only counterpart to W1/W3's real editor probe. No authored
/// names, values, resource bytes or sample data are emitted.
#[test]
#[ignore]
#[cfg(target_os = "linux")]
fn probe_conflux_capture_idle_and_reopen() {
    let path = PathBuf::from(std::env::var_os("PROBE_PATH").expect("PROBE_PATH"));
    let proc_value = |key: &str| -> u64 {
        std::fs::read_to_string("/proc/self/status").unwrap().lines()
            .find_map(|line| line.strip_prefix(key)?.split_whitespace().next()?.parse().ok()).unwrap_or(0)
    };
    let cpu = || -> f64 {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        // SAFETY: getrusage initializes the supplied record on success.
        assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) }, 0);
        let usage = unsafe { usage.assume_init() };
        usage.ru_utime.tv_sec as f64 + usage.ru_utime.tv_usec as f64 / 1e6
            + usage.ru_stime.tv_sec as f64 + usage.ru_stime.tv_usec as f64 / 1e6
    };
    let mut rows = Vec::new();
    for reopen in 0..4 {
        let mut loaded = V2Loader.prepare(&LoadRequest { path: path.clone(), sample_rate: 48000., ..Default::default() }, &mut |_| {}, &|| false).unwrap();
        let zones = loaded.instrument.as_ref().map_or(0, |i| i.zones.len());
        let part = loaded.part.as_mut().unwrap();
        part.prepare_persistence(&loaded.scripts.views, "").unwrap();
        let persistence = part.persistence.as_ref().unwrap();
        let persistent_values = persistence.values_len();
        let persistent_bytes = persistence.values_bytes();
        let mut ingress = part.ui_controls.take().unwrap();
        let transfer_bytes: usize = ingress.captures.iter().map(|c| c.capacity() * std::mem::size_of::<sampler_core::WidgetEdit>()).sum();
        let typed_cells: usize = ingress.widget_values.values().map(|v| match v {
            sampler_ui_ir::Value::Integers(v) => v.len(), sampler_ui_ir::Value::Reals(v) => v.len(), _ => 1,
        }).sum();
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, loaded.part.take());
        core.onset_audit();
        let started = std::time::Instant::now();
        let cpu_start = cpu();
        let mut samples = Vec::new();
        let mut queued = 0;
        let mut readback_us = 0u128;
        let mut changed = 0;
        let mut phases = Vec::new();
        let mut phase_cpu = cpu_start;
        for tick in 0..100 {
            let at = std::time::Instant::now();
            // Isolate render idle, readback polling, then unchanged editor reads.
            if (33..66).contains(&tick) {
                changed += usize::from(ingress.refresh());
                queued += usize::from(ingress.capturing);
            }
            if tick >= 33 {
                for face in &loaded.interfaces { std::hint::black_box(ingress.values(face)); }
            }
            readback_us += at.elapsed().as_micros();
            for _ in 0..38 {
                core.begin_block(&BlockInfo { frames: 128, ..Default::default() });
                core.render(128);
            }
            if tick % 10 == 0 { samples.push(serde_json::json!({"tick":tick,"rss_kb":proc_value("VmRSS:"),"threads":proc_value("Threads:")})); }
            let due = started + std::time::Duration::from_millis((tick + 1) * 100);
            if let Some(left) = due.checked_duration_since(std::time::Instant::now()) { std::thread::sleep(left); }
            if [32, 65, 99].contains(&tick) {
                let now = cpu();
                phases.push(serde_json::json!({"end_tick":tick,"cpu_seconds":now-phase_cpu,
                    "readback_us":readback_us,"rss_kb":proc_value("VmRSS:"),"stages":core.onset_audit()}));
                phase_cpu = now;
                readback_us = 0;
            }
        }
        rows.push(serde_json::json!({"reopen":reopen,"zones":zones,"persistent_values":persistent_values,
            "persistent_bytes":persistent_bytes,"typed_cells":typed_cells,"transfer_bytes":transfer_bytes,
            "capture_ticks":queued,"changed_ticks":changed,"readback_us":readback_us,
            "cpu_seconds":cpu()-cpu_start,"wall_seconds":started.elapsed().as_secs_f64(),
            "phases":phases,"samples":samples}));
        drop(core); drop(ingress); drop(loaded);
    }
    println!("CAPTURE_PROBE {}", serde_json::to_string(&rows).unwrap());
}
