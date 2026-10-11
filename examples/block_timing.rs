//! Real-time paced block timing through the plugin sound seam (no audio kept).
//! `block_timing PATH [PROGRAM] [BLOCK] [SECONDS] [KEY]` plays one note for half a second,
//! then idles, and prints per-block percentiles and where the slow blocks fall.
use kontakto::sound::{
    BlockInfo, Core, CoreLoader, LoadRequest,
    event::Event,
    v2::{V2Core, V2Loader},
};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let num = |i: usize, default: usize| args.get(i).and_then(|v| v.parse().ok()).unwrap_or(default);
    let (program, block, seconds, key) = (num(2, 0), num(3, 128), num(4, 5), num(5, 60) as u8);
    let loaded = V2Loader
        .prepare(
            &LoadRequest {
                path: args[1].clone().into(),
                sample_rate: 48000.,
                program: program as u32,
                ..Default::default()
            },
            &mut |_| {},
            &|| false,
        )
        .unwrap_or_else(|_| panic!("load failed; authored diagnostics omitted"));
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, loaded.part);
    let period = Duration::from_secs_f64(block as f64 / 48000.);
    let blocks = seconds * 48000 / block;
    let mut times = Vec::with_capacity(blocks);
    let start = Instant::now();
    for n in 0..blocks {
        let due = start + period * n as u32;
        while Instant::now() < due {
            std::thread::sleep(Duration::from_micros(200));
        }
        if n == 0 {
            core.event(0, Event::midi1(0xb0, 1, 100));
            core.event(0, Event::midi1(0xb0, 11, 127));
            core.event(0, Event::midi1(0x90, key, 100));
        }
        if n == 24000 / block {
            core.event(0, Event::midi1(0x80, key, 0));
        }
        let t = Instant::now();
        core.begin_block(&BlockInfo {
            frames: block,
            ..Default::default()
        });
        for offset in (0..block).step_by(128) {
            core.render((block - offset).min(128));
        }
        core.end_block(block, &mut |_| true);
        times.push(t.elapsed().as_secs_f64() * 1e6);
    }
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    let q = |p: f64| sorted[((sorted.len() - 1) as f64 * p) as usize];
    let deadline = block as f64 / 48000. * 1e6;
    let slow: Vec<usize> = (0..times.len()).filter(|&i| times[i] > deadline / 4.).collect();
    println!(
        "blocks {} p50 {:.1}us p99 {:.1}us max {:.1}us over_deadline {} over_quarter {}",
        times.len(),
        q(0.5),
        q(0.99),
        sorted[sorted.len() - 1],
        times.iter().filter(|&&t| t > deadline).count(),
        slow.len()
    );
    println!(
        "slow block times (ms): {:?}",
        slow.iter()
            .take(40)
            .map(|&i| (i as f64 * block as f64 / 48.).round())
            .collect::<Vec<_>>()
    );
}
