//! Callback timing for the engine: every plan the plug-in can select, at 48 kHz
//! with 512-sample stereo blocks, under the defaults plus a realistic load (depth
//! 8, detail 5, band 1 a bell at 1 kHz). Each block is timed on its own, through
//! the same calls the plug-in makes (plan selection, every sample, and the plot
//! publish at 60 Hz).
//!
//! Run it in release, which is the only build whose numbers mean anything:
//!
//! ```text
//! build-slot cargo run -p smooth-core --release --example rt_timing
//! ```
//!
//! Output is one line per plan, then the plot and plan-switch costs. Timing on a
//! shared machine is noisy, so repeat the run and take the median of the medians.

use smooth_core::EngineBank;
use smooth_core::dsp::Settings;
use smooth_core::plot::Telemetry;
use smooth_core::weighting::{
    BandRecord, NormalizedRecord, normalized_from_center, normalized_from_q, normalized_from_slope,
};
use std::time::Instant;

const RATE: f32 = 48_000.0;
const BLOCK: usize = 512;
/// About four seconds of audio before anything is timed, so every plan has run its frames.
const WARM_BLOCKS: usize = 400;
/// About 85 seconds of audio per plan.
const TIMED_BLOCKS: usize = 8_000;
const SWITCHES: usize = 40;
const PLOT_CALLS: usize = 4_000;
/// The plot is published about sixty times a second: every 800 samples at 48 kHz.
const PUBLISH_EVERY: usize = 800;

/// The plug-in's plans, by selection index, with their names.
const PLANS: [(&str, usize); 7] = [
    ("low latency", 0),
    ("q0 normal", 1),
    ("q1 high", 2),
    ("q2 ultra", 3),
    ("q0 normal lp", 4),
    ("q1 high lp", 5),
    ("q2 ultra lp", 6),
];

/// The defaults, plus the realistic load: depth 8, detail 5, and band 1 a bell at
/// 1 kHz (the plug-in's band 1 is active by default).
fn realistic() -> Settings {
    let mut settings = Settings {
        depth: 0.8,
        detail: 0.5,
        attack: 0.5,
        release: 0.3,
        ..Settings::default()
    };
    settings.bands[0] = BandRecord::from_normalized(NormalizedRecord {
        used: 1.0,
        enabled: 1.0,
        frequency: normalized_from_center(1_000.0),
        depth: 0.7,
        q: normalized_from_q(2.0),
        slope: normalized_from_slope(12.0),
        focus: 0.5,
        selector: 0.0,
        shape: 2.0 / 7.0,
    })
    .expect("finite benchmark band");
    settings
}

/// A tone pair and a noise floor, generated a block at a time outside the timer.
struct Source {
    seed: u32,
    at: usize,
}

impl Source {
    fn fill(&mut self, block: &mut [[f32; 2]]) {
        for frame in block {
            self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (self.seed >> 8) as f32 / (1 << 24) as f32 - 0.5;
            let t = self.at as f64 / f64::from(RATE);
            let tone = (std::f64::consts::TAU * 1_000.0 * t).sin()
                + 0.5 * (std::f64::consts::TAU * 3_100.0 * t).sin();
            *frame = [
                (0.2 * tone as f32 + 0.05 * noise),
                (0.2 * tone as f32 - 0.03 * noise),
            ];
            self.at += 1;
        }
    }
}

/// One block, as the plug-in's `process` runs it: select the plan, run every sample
/// through the active engine, and publish the plot when a frame is due.
fn block(
    bank: &mut EngineBank,
    plot: &Telemetry,
    since: &mut usize,
    selection: usize,
    input: &[[f32; 2]],
    output: &mut [[f32; 2]],
) {
    let settings = realistic();
    bank.select(selection, false);
    for (frame, out) in input.iter().zip(output.iter_mut()) {
        *out = bank
            .active_mut()
            .process_sample_with_detector(*frame, *frame, &settings);
    }
    *since += input.len();
    if *since >= PUBLISH_EVERY {
        *since = 0;
        plot.publish(&bank.active().plot_frame());
    }
}

/// The nearest-rank percentile of sorted nanoseconds, in microseconds.
fn percentile_us(sorted: &[u64], p: f64) -> f64 {
    let rank = ((sorted.len() as f64) * p).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1] as f64 / 1_000.0
}

/// The calling thread's CPU time in nanoseconds (Linux). Wall-clock time includes
/// every preemption; this one does not, so it shows the cost of the work itself.
#[cfg(target_os = "linux")]
fn thread_cpu_nanos() -> Option<u64> {
    #[repr(C)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }
    const CLOCK_THREAD_CPUTIME_ID: i32 = 3;
    unsafe extern "C" {
        fn clock_gettime(clock: i32, time: *mut Timespec) -> i32;
    }
    let mut time = Timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `clock_gettime` writes one `timespec` through the valid pointer it is given.
    let status = unsafe { clock_gettime(CLOCK_THREAD_CPUTIME_ID, &mut time) };
    (status == 0).then(|| time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64)
}

#[cfg(not(target_os = "linux"))]
fn thread_cpu_nanos() -> Option<u64> {
    None
}

fn summarize(name: &str, mut nanos: Vec<u64>, mut cpu: Vec<u64>) {
    nanos.sort_unstable();
    cpu.sort_unstable();
    let budget_us = BLOCK as f64 / f64::from(RATE) * 1.0e6;
    let p50 = percentile_us(&nanos, 0.50);
    let p99 = percentile_us(&nanos, 0.99);
    println!(
        "{name:<14} blocks={} p50_us={p50:.1} p99_us={p99:.1} p999_us={:.1} max_us={:.1} \
         cpu_p50_pct={:.2} cpu_p99_pct={:.2} | thread_cpu_p50_us={:.1} thread_cpu_p99_us={:.1} \
         thread_cpu_max_us={:.1} thread_cpu_p50_pct={:.2}",
        nanos.len(),
        percentile_us(&nanos, 0.999),
        nanos.last().copied().unwrap_or(0) as f64 / 1_000.0,
        100.0 * p50 / budget_us,
        100.0 * p99 / budget_us,
        percentile_us(&cpu, 0.50),
        percentile_us(&cpu, 0.99),
        cpu.last().copied().unwrap_or(0) as f64 / 1_000.0,
        100.0 * percentile_us(&cpu, 0.50) / budget_us,
    );
}

fn load_average() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .map(|text| text.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|_| "unknown".into())
}

fn main() {
    let cpus = std::thread::available_parallelism().map_or(0, usize::from);
    println!(
        "rt_timing: rate={RATE} block={BLOCK} budget_us={:.1} cpus={cpus} load_before=[{}]",
        BLOCK as f64 / f64::from(RATE) * 1.0e6,
        load_average()
    );
    let plot = Telemetry::default();
    let mut source = Source { seed: 0x2545_f491, at: 0 };
    let mut input = vec![[0.0_f32; 2]; BLOCK];
    let mut output = vec![[0.0_f32; 2]; BLOCK];

    for (name, selection) in PLANS {
        let mut bank = EngineBank::new(RATE, selection, false).expect("supported rate");
        let mut since = 0;
        for _ in 0..WARM_BLOCKS {
            source.fill(&mut input);
            block(&mut bank, &plot, &mut since, selection, &input, &mut output);
        }
        let mut nanos = Vec::with_capacity(TIMED_BLOCKS);
        let mut cpu = Vec::with_capacity(TIMED_BLOCKS);
        for _ in 0..TIMED_BLOCKS {
            source.fill(&mut input);
            let cpu_start = thread_cpu_nanos().unwrap_or(0);
            let start = Instant::now();
            block(&mut bank, &plot, &mut since, selection, &input, &mut output);
            nanos.push(start.elapsed().as_nanos() as u64);
            cpu.push(thread_cpu_nanos().unwrap_or(0).saturating_sub(cpu_start));
        }
        summarize(name, nanos, cpu);
    }

    // The plot publish, timed on its own: the frame it copies and the atomic stores.
    let mut bank = EngineBank::new(RATE, 1, false).expect("supported rate");
    let mut since = 0;
    for _ in 0..WARM_BLOCKS {
        source.fill(&mut input);
        block(&mut bank, &plot, &mut since, 1, &input, &mut output);
    }
    let mut frame_nanos = Vec::with_capacity(PLOT_CALLS);
    let mut publish_nanos = Vec::with_capacity(PLOT_CALLS);
    for _ in 0..PLOT_CALLS {
        let start = Instant::now();
        let frame = bank.active().plot_frame();
        let built = Instant::now();
        plot.publish(&frame);
        publish_nanos.push(built.elapsed().as_nanos() as u64);
        frame_nanos.push((built - start).as_nanos() as u64);
    }
    frame_nanos.sort_unstable();
    publish_nanos.sort_unstable();
    println!(
        "plot_frame    calls={PLOT_CALLS} p50_us={:.1} p99_us={:.1} max_us={:.1}  (per frame, ~60 Hz)",
        percentile_us(&frame_nanos, 0.50),
        percentile_us(&frame_nanos, 0.99),
        frame_nanos.last().copied().unwrap_or(0) as f64 / 1_000.0,
    );
    println!(
        "plot_publish  calls={PLOT_CALLS} p50_us={:.2} p99_us={:.2} max_us={:.2}",
        percentile_us(&publish_nanos, 0.50),
        percentile_us(&publish_nanos, 0.99),
        publish_nanos.last().copied().unwrap_or(0) as f64 / 1_000.0,
    );

    // A plan switch: `select` resets the engine, and the switch block carries it.
    let mut switch_nanos = Vec::with_capacity(SWITCHES);
    let mut bank = EngineBank::new(RATE, 1, false).expect("supported rate");
    let mut since = 0;
    for i in 0..SWITCHES {
        let selection = i % PLANS.len();
        source.fill(&mut input);
        let start = Instant::now();
        block(&mut bank, &plot, &mut since, selection, &input, &mut output);
        switch_nanos.push(start.elapsed().as_nanos() as u64);
    }
    switch_nanos.sort_unstable();
    println!(
        "plan_switch   blocks={SWITCHES} (each block changes plan) p50_us={:.1} max_us={:.1}",
        percentile_us(&switch_nanos, 0.50),
        switch_nanos.last().copied().unwrap_or(0) as f64 / 1_000.0,
    );
    println!("load_after=[{}]", load_average());
}
