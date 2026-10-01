//! KSP interpreter throughput on an instrument's own scripts, after loading:
//! 4-note chords every 150 ms. `cargo run --release --example ksp_bench -- <nki> [chords=100]`
//! prints script instructions run and user-space cycles per instruction.

use kontakto::ksp::{LogEngine, Runtime};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: ksp_bench <nki> [chords]");
    let chords: u64 = args.get(2).map_or(Ok(100), |s| s.parse())?;
    let instrument = kontakto::import::read(Path::new(path))?;
    let groups = instrument.groups.iter().map(|g| g.name.clone()).collect();
    let mut engine = LogEngine::new(groups, 48_000.0);
    let (mut rt, _) =
        Runtime::with_scripts(&instrument.scripts, &mut engine, 8, instrument.script_state.clone());
    let (cycles, instructions) = (Counter::open(0), Counter::open(1));
    // Rounds of `chords / 10`; the quietest round counts.
    let (mut best, mut total) = ((f64::MAX, 0.0), 0.0);
    for round in 0..10 {
        rt.env.spent = 0;
        let start = (cycles.read(), instructions.read());
        for c in round * chords / 10..(round + 1) * chords / 10 {
            let keys = [0, 4, 7, 11].map(|k| 55 + (c * 5 % 12) as u8 + k);
            for key in keys {
                rt.note_on(&mut engine, 0, key, 100);
            }
            // 150 ms of 128-frame blocks, releasing after 120 ms.
            for block in 0..56 {
                if block == 45 {
                    for key in keys {
                        rt.note_off(&mut engine, 0, key);
                    }
                }
                rt.process(&mut engine, 128);
                engine.calls.clear();
            }
        }
        let ops = rt.env.spent as f64;
        total += ops;
        let spent = (cycles.read() - start.0) as f64 / ops;
        if spent < best.0 {
            best = (spent, (instructions.read() - start.1) as f64 / ops);
        }
    }
    println!(
        "{}: {:.0} script instructions per chord · {:.2} cycles and {:.1} CPU instructions each",
        instrument.name,
        total / chords as f64,
        best.0,
        best.1
    );
    Ok(())
}

/// A user-space hardware counter on this thread (perf_event_open); 0 = cycles.
struct Counter(i32);

impl Counter {
    fn open(config: u64) -> Self {
        unsafe extern "C" {
            fn syscall(n: i64, ...) -> i64;
        }
        let mut attr = [0u64; 14];
        attr[0] = 112 << 32;
        attr[1] = config;
        attr[5] = (1 << 5) | (1 << 6);
        // SAFETY: perf_event_open(attr, this thread, any CPU, no group, no flags).
        let fd = unsafe { syscall(298, attr.as_ptr(), 0i32, -1i32, -1i32, 0u64) };
        assert!(fd >= 0, "perf_event_open failed");
        Self(fd as i32)
    }

    fn read(&self) -> u64 {
        unsafe extern "C" {
            fn read(fd: i32, buf: *mut u64, n: usize) -> isize;
        }
        let mut value = 0;
        // SAFETY: reads one u64 count from the counter's descriptor.
        unsafe { read(self.0, &mut value, 8) };
        value
    }
}
