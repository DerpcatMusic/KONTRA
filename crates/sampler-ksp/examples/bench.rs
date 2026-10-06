//! Per-callback cost of real library scripts (kept outside the repo).
//! Compiles each script, binds it to a one-region instrument and plays a
//! fixed workload in 64-frame blocks at 48 kHz: a note-on every block, the
//! note-off two blocks later, and a UI control edit every 8th block. Every
//! script callback runs inside the block that dispatched it, so block time
//! is the audio-thread cost. The deadline for 64 frames is 1333 us.
//! Usage: cargo run -p sampler-ksp --release --example bench [DIR]
//! KSP_FUEL sets the per-resume instruction budget (default 100000).
use sampler_core::*;
use std::time::{Duration, Instant};

const BLOCK: usize = 64;
const BLOCKS: usize = 2000;

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| format!("{}/.cache/ksp-corpus", std::env::var("HOME").unwrap()));
    let fuel: usize = std::env::var("KSP_FUEL")
        .ok()
        .and_then(|f| f.parse().ok())
        .unwrap_or(100_000);
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("corpus directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ksp"))
        .collect();
    paths.sort();
    let limits = sampler_ksp::Limits {
        source_bytes: usize::MAX,
        instructions: usize::MAX,
        variables: usize::MAX,
        array_cells: usize::MAX,
    };
    println!("fuel {fuel} per resume; {BLOCKS} blocks of {BLOCK} frames; deadline 1333 us");
    println!(
        "{:<10} {:>10} {:>8} {:>8} {:>8} {:>8} {:>6} {:>6}",
        "script", "compile", "mean us", "p99 us", "max us", "over", "fuel!", "fault"
    );
    let mut all = Vec::new();
    let mut fault_kinds = std::collections::BTreeMap::<String, usize>::new();
    let (mut total_over, mut total_exhausted) = (0, 0);
    for path in &paths {
        let source = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
        let file = path.file_stem().unwrap().to_string_lossy();
        let mut compile = Duration::MAX;
        let mut script = None;
        for _ in 0..3 {
            let t = Instant::now();
            let s = sampler_ksp::compile(&source, 48000, limits, &[]);
            compile = compile.min(t.elapsed());
            script = s.ok();
        }
        let Some(script) = script else {
            println!("{file:<10} compile failed");
            continue;
        };
        let region = Region {
            sample: 0,
            key_low: 0,
            key_high: 127,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        };
        let note_cells = script.note_cells();
        let controls: Vec<_> = script.controls().iter().map(|c| c.definition).collect();
        let pcm = Pcm::new(48000, vec![[0.1f32; 2]; 48000].into_boxed_slice()).unwrap();
        // Libraries address groups by index; give the scripts room.
        let plan = match Prepared::new(48000, vec![pcm], vec![region], 128)
            .and_then(|p| p.with_groups(512, vec![Some(0)]))
            .and_then(|p| script.bind(p))
        {
            Ok(p) => p,
            Err(e) => {
                println!("{file:<10} bind failed: {e:?}");
                continue;
            }
        };
        let cells = plan.behavior_local_count() * 128;
        let mut rt = match Runtime::new(
            plan,
            sampler_core::Limits {
                notes: 64,
                channels: 0,
                performances: 1,
                families: 64,
                expressions: 64,
                voices: 64,
                decisions: 0,
                commands: 256,
                behaviors: 128,
                behavior_fuel: fuel,
                behavior_cells: cells,
                note_cells: note_cells * 64,
            },
        ) {
            Ok(rt) => rt,
            Err(e) => {
                println!("{file:<10} runtime failed: {e:?}");
                continue;
            }
        };
        let generation = rt.active_plan();
        let mut out = [[0f32; 2]; BLOCK];
        let mut held = std::collections::VecDeque::new();
        let mut times = Vec::with_capacity(BLOCKS);
        let (mut exhausted, mut faults) = (0, 0);
        for i in 0..BLOCKS {
            let t = Instant::now();
            let key = 36 + (i * 7 % 60) as u8;
            let input = Input {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key,
                external_id: None,
            };
            if let Ok(note) = rt.trigger(input, key, 0.3 + (i % 7) as f64 / 10.) {
                held.push_back(note);
            }
            if held.len() > 2 {
                let _ = rt.key_up(held.pop_front().unwrap(), None);
            }
            if i % 8 == 0 && !controls.is_empty() {
                let c = controls[i / 8 % controls.len()];
                let value = match c.domain {
                    ControlDomain::Integer { min, max } => {
                        ControlValue::Integer(min + (max - min) / 3)
                    }
                    _ => c.default,
                };
                let _ = rt.edit_controls(generation, None, &[ControlWrite { id: c.id, value }]);
            }
            let _ = rt.render(&mut out);
            rt.flush_behaviors(|_, owner, outcome| {
                match outcome {
                    Outcome::FuelExhausted => exhausted += 1,
                    Outcome::Fault(e) => {
                        faults += 1;
                        let on = if matches!(owner, BehaviorOwner::Note(_)) {
                            "note"
                        } else {
                            "plan"
                        };
                        *fault_kinds
                            .entry(format!("{e:?} in {on} callback"))
                            .or_default() += 1;
                    }
                    _ => {}
                }
                true
            });
            rt.drain_effects(|_| true);
            times.push(t.elapsed());
        }
        times.sort();
        let mean = times.iter().sum::<Duration>() / times.len() as u32;
        let p99 = times[times.len() * 99 / 100];
        let max = *times.last().unwrap();
        let over = times.iter().filter(|t| t.as_micros() > 1333).count();
        total_over += over;
        total_exhausted += exhausted;
        all.extend_from_slice(&times);
        println!(
            "{file:<10} {:>8.1}ms {:>8.1} {:>8.1} {:>8.1} {over:>8} {exhausted:>6} {faults:>6}",
            compile.as_secs_f64() * 1e3,
            mean.as_secs_f64() * 1e6,
            p99.as_secs_f64() * 1e6,
            max.as_secs_f64() * 1e6,
        );
    }
    if !fault_kinds.is_empty() {
        println!("callback faults by error: {fault_kinds:?}");
    }
    all.sort();
    if !all.is_empty() {
        println!(
            "all blocks: p50 {:.1} us, p99 {:.1} us, p99.9 {:.1} us, max {:.1} us; {total_over} over deadline; {total_exhausted} callbacks hit the fuel budget",
            all[all.len() / 2].as_secs_f64() * 1e6,
            all[all.len() * 99 / 100].as_secs_f64() * 1e6,
            all[all.len() * 999 / 1000].as_secs_f64() * 1e6,
            all.last().unwrap().as_secs_f64() * 1e6,
        );
    }
}
