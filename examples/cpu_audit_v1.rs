//! Source-adapter audit; PCM stays in RAM. Product source and Cargo.lock stay pinned.
use anyhow::{Context, Result};
use kontakto::engine::MAX_BLOCK;
const RATE: f64 = 48_000.;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "bench-stream") {
        assert_eq!(args.len(), 5, "bench-stream PATH NOTES SECONDS");
        let notes: usize = args[3].parse().unwrap();
        let seconds: f64 = args[4].parse().unwrap();
        assert!(notes > 0 && seconds.is_finite() && seconds > 0.);
        bench_stream(Path::new(&args[2]), notes, seconds).unwrap();
    } else {
        audit_main();
    }
}
use kontakto::{
    engine::{Bank, Engine, MEMORY_LIMIT, Streaming, effects, load_scripts},
    import,
};
use serde_json::{Value, json};
use std::path::Path;
struct Player(Engine);
fn cpu_checks() {}
impl Player {
    fn load(path: &Path) -> (Self, Value) {
        let i = import::read(path).unwrap();
        let (script, errors) = load_scripts(&i, i.script_state.clone(), 48000.);
        let controllers = script
            .as_deref()
            .map_or(&[][..], |s| &s.init_controllers[..]);
        let bank = Bank::load_counting(
            &i,
            MEMORY_LIMIT,
            Streaming::Auto,
            controllers,
            &Default::default(),
        )
        .unwrap();
        let info = json!({"engine":"v1","preload_frames":bank.preload,"head_bytes":bank.bytes,"samples":bank.sample_count(),"script_errors":errors.len()});
        let fx = effects(&i, script.as_deref(), 48000.);
        let mut e = Engine::default();
        e.set_bank(Some(Box::new(bank)));
        e.set_fx(fx);
        e.set_script(script);
        (Self(e), info)
    }
    fn event(&mut self, s: u8, a: u8, b: u8) {
        match s {
            0x90 => self.0.note_on(0, a, b),
            0x80 => self.0.note_off(0, a),
            0xb0 => self.0.cc(0, a, b),
            _ => panic!("event"),
        }
    }
    fn render(&mut self, frames: usize) -> f32 {
        self.0.begin_audio_block(frames, 1, false);
        let mut peak = 0.0f32;
        for n in (0..frames).step_by(128) {
            let len = (frames - n).min(128);
            let (mut l, mut r) = ([0.; 128], [0.; 128]);
            self.0.render(&mut l[..len], &mut r[..len]);
            peak = peak.max(
                l[..len]
                    .iter()
                    .chain(&r[..len])
                    .fold(0.0f32, |a, b| a.max(b.abs())),
            );
        }
        peak
    }
    fn voices(&self) -> usize {
        self.0.active_voices()
    }
    fn problems(&self) -> Value {
        json!({"underruns":self.0.underruns(),"dropped_commands":self.0.dropped_commands()})
    }
}
include!("../tools/cpu-audit-common.rs");

// Exact CLI load/bench-stream path from 0cb7a8a0:src/main.rs.
type Scripts = (Option<Box<kontakto::ksp::Runtime>>, Vec<String>);

/// Load the scripts, then the bank, as the plugin does: the bank keeps
/// resident the start offsets the scripts' `on init` controllers select.
fn load(instrument: &import::Instrument) -> Result<(Bank, Scripts)> {
    let scripts = load_scripts(instrument, instrument.script_state.clone(), RATE);
    let controllers = scripts
        .0
        .as_deref()
        .map_or(&[][..], |rt| &rt.init_controllers[..]);
    let bank = Bank::load_counting(
        instrument,
        kontakto::engine::MEMORY_LIMIT,
        kontakto::engine::Streaming::Auto,
        controllers,
        &Default::default(),
    )?;
    Ok((bank, scripts))
}

/// Install [`load`]ed scripts in `engine`, reporting slot errors.
fn install_scripts(engine: &mut Engine, (script, errors): Scripts) {
    for error in errors {
        eprintln!("{error}");
    }
    engine.set_script(script);
}

fn bench_stream(path: &Path, notes: usize, seconds: f64) -> Result<()> {
    let instrument = import::read(path)?;
    let (bank, scripts) = load(&instrument)?;
    let low = bank
        .zones()
        .iter()
        .map(|z| z.low_key)
        .min()
        .context("Instrument has no playable zones")?;
    let high = bank.zones().iter().map(|z| z.high_key).max().unwrap_or(low);
    let preload = bank.preload;
    let mut engine = Engine::default();
    engine.set_bank(Some(Box::new(bank)));
    install_scripts(&mut engine, scripts);
    let keys: Vec<u8> = (low..=high).collect();
    let (mut left, mut right) = ([0f32; MAX_BLOCK], [0f32; MAX_BLOCK]);
    let block = std::time::Duration::from_secs_f64(MAX_BLOCK as f64 / RATE);
    let every = (RATE / notes as f64) as u64;
    let blocks = (seconds * RATE) as u64 / MAX_BLOCK as u64;
    let (mut held, mut started, mut voice_blocks, mut late, mut peak) =
        (std::collections::VecDeque::new(), 0, 0, 0, 0);
    let (mut pace, render_start, process_start) = (
        kontakto::engine::Pace::start(),
        cpu_time(THREAD),
        cpu_time(PROCESS),
    );
    let mut render_cpu = 0.0;
    for b in 0..blocks {
        let frame = b * MAX_BLOCK as u64;
        while started * every < frame + MAX_BLOCK as u64 {
            if held.len() >= notes.min(keys.len())
                && let Some(key) = held.pop_front()
            {
                engine.note_off(0, key);
            }
            let key = keys[(started as usize * 7) % keys.len()];
            engine.note_on(0, key, 100);
            held.push_back(key);
            started += 1;
        }
        let before = cpu_time(THREAD);
        engine.render(&mut left, &mut right);
        render_cpu += cpu_time(THREAD) - before;
        voice_blocks += engine.active_voices();
        peak = peak.max(engine.active_voices());
        if !pace.until(block * (b + 1) as u32).is_zero() {
            late += 1;
        }
    }
    let underruns = engine.underruns();
    // Everything but the rendering thread is the streamer (and script timers).
    let other = cpu_time(PROCESS) - process_start - (cpu_time(THREAD) - render_start);
    println!(
        "{}: preload {preload} · {started} notes over {seconds} s ({notes} held) · {} voices mean, {peak} peak · {underruns} underruns ({:.3}%) · {late} late blocks · render {:.1}% of a core, streamer {:.1}%",
        instrument.name,
        voice_blocks / blocks as usize,
        underruns as f64 * 100.0 / voice_blocks.max(1) as f64,
        render_cpu * 100.0 / seconds,
        other * 100.0 / seconds,
    );
    Ok(())
}

const PROCESS: i32 = 2;
const THREAD: i32 = 3;

/// CPU seconds of this process or thread (`CLOCK_*_CPUTIME_ID`); 0 off Linux.
fn cpu_time(clock: i32) -> f64 {
    #[cfg(target_os = "linux")]
    {
        #[repr(C)]
        struct Timespec {
            s: i64,
            ns: i64,
        }
        unsafe extern "C" {
            fn clock_gettime(clock: i32, t: *mut Timespec) -> i32;
        }
        let mut t = Timespec { s: 0, ns: 0 };
        // SAFETY: clock_gettime writes one timespec for these clock ids.
        unsafe { clock_gettime(clock, &mut t) };
        t.s as f64 + t.ns as f64 * 1e-9
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = clock;
        0.0
    }
}
