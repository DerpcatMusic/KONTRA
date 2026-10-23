//! Capacity overhead: the same held voices against several voice-pool capacities.
//! Playing 16 voices must cost the same whatever the pool holds; any gap is
//! per-capacity work in the render path.
use super::*;

/// `keys` notes held for the whole run: all voices sound, none is stolen.
fn chord(keys: u8, seconds: f64) -> Vec<Message> {
    let mut m = expression();
    for key in 48..48 + keys {
        on_off(&mut m, key, 100, 0.0, seconds);
    }
    m
}

fn option(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
}

pub(super) fn run(args: &[String]) -> Result<(), String> {
    let caps: Vec<usize> = option(args, "--caps")
        .unwrap_or_else(|| "64,256,4096".into())
        .split(',')
        .map(|c| c.parse().map_err(fail))
        .collect::<Result<_, _>>()?;
    let name = option(args, "--only").unwrap_or_else(|| "dense-strings".into());
    let block: usize = option(args, "--block").map_or(Ok(64), |v| v.parse().map_err(fail))?;
    let seconds: f64 = option(args, "--seconds").map_or(Ok(4.0), |v| v.parse().map_err(fail))?;
    let repeats: usize = option(args, "--repeats").map_or(Ok(3), |v| v.parse().map_err(fail))?;
    let s = SCENARIOS
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("unknown scenario {name}"))?;
    let keys: u8 = option(args, "--keys").map_or(Ok(16), |v| v.parse().map_err(fail))?;
    let mut messages = chord(keys, seconds);
    messages.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then((a.1[0] & 0xf0 == 0x90).cmp(&(b.1[0] & 0xf0 == 0x90)))
    });
    let frames = (seconds * f64::from(RATE)) as usize;
    // Half a second of warm-up: the attack transients are not steady state.
    let warm = (0.5 * f64::from(RATE)) as usize / block;
    for &cap in &caps {
        for rep in 0..repeats {
            VOICE_CAP.store(cap as u64, Ordering::Relaxed);
            let mut loaded = load(s, &messages, block)?;
            if let Player::Midi { rt, .. } = &mut loaded.player {
                rt.set_threads(Threads::Fixed(1));
            }
            let cell = play(&mut loaded.player, &messages, block, frames);
            let mut times: Vec<u64> = cell.times.iter().skip(warm).copied().collect();
            let voices = cell.recs.iter().map(|r| r.voices).max().unwrap_or(0);
            let mean_us = times.iter().sum::<u64>() as f64 / times.len().max(1) as f64 / 1e3;
            times.sort_unstable();
            let deadline = block as f64 / f64::from(RATE) * 1e9;
            let line = json!({
                "scenario": s.name, "keys": keys, "capacity": cap, "rep": rep, "block": block,
                "voices_max": voices, "blocks": times.len(), "mean_us": mean_us,
                "p50_us": percentile(&times, 0.5) as f64 / 1e3,
                "p99_us": percentile(&times, 0.99) as f64 / 1e3,
                "p999_us": percentile(&times, 0.999) as f64 / 1e3,
                "max_us": times.last().copied().unwrap_or(0) as f64 / 1e3,
                "deadline_misses": times.iter().filter(|&&ns| ns as f64 > deadline).count(),
            });
            println!("{line}");
        }
    }
    Ok(())
}

/// `sampler-perf spike`: one instrument, one schedule, one thread. Prints the
/// per-block timing and the spike report (which counters move in the slow blocks).
/// `--kontakt PATH` or `--uvi BANK::PRESET`; `--notes dense|pad|legato|sweep`.
pub(super) fn spike(args: &[String]) -> Result<(), String> {
    let source = match (option(args, "--kontakt"), option(args, "--uvi")) {
        (Some(path), None) => Source::Kontakt(Box::leak(path.into_boxed_str())),
        (None, Some(v)) => {
            let (bank, preset) = v.split_once("::").ok_or("--uvi BANK::PRESET")?;
            Source::Uvi(
                Box::leak(bank.to_string().into_boxed_str()),
                Box::leak(preset.to_string().into_boxed_str()),
            )
        }
        _ => return Err("spike needs --kontakt PATH or --uvi BANK::PRESET".into()),
    };
    let notes: fn() -> Vec<Message> = match option(args, "--notes").as_deref() {
        None | Some("dense") => dense,
        Some("pad") => pad,
        Some("legato") => legato,
        Some("sweep") => sweep,
        Some(other) => return Err(format!("unknown --notes {other}")),
    };
    let block: usize = option(args, "--block").map_or(Ok(64), |v| v.parse().map_err(fail))?;
    let seconds: f64 = option(args, "--seconds").map_or(Ok(8.0), |v| v.parse().map_err(fail))?;
    if let Some(cap) = option(args, "--voices") {
        VOICE_CAP.store(cap.parse::<u64>().map_err(fail)?, Ordering::Relaxed);
    }
    let s = Scenario {
        name: "spike",
        source,
        seconds,
        notes,
    };
    // `--key K --vel V`: one note held for the run (the corpus survey's probe note).
    let mut messages = match option(args, "--key") {
        Some(key) => {
            let key: u8 = key.parse().map_err(fail)?;
            let vel: u8 = option(args, "--vel").map_or(Ok(64), |v| v.parse().map_err(fail))?;
            let mut m = expression();
            on_off(&mut m, key, vel, 0.0, seconds);
            m
        }
        None => (s.notes)(),
    };
    messages.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then((a.1[0] & 0xf0 == 0x90).cmp(&(b.1[0] & 0xf0 == 0x90)))
    });
    let frames = (seconds * f64::from(RATE)) as usize;
    let mut loaded = load(&s, &messages, block)?;
    if let Player::Midi { rt, .. } = &mut loaded.player {
        rt.set_threads(Threads::Fixed(1));
    }
    let cell = if matches!(loaded.player, Player::Uvi(_)) {
        play_uvi(&mut loaded.player, &messages, block, frames)
    } else {
        play(&mut loaded.player, &messages, block, frames)
    };
    let mut sorted = cell.times.clone();
    sorted.sort_unstable();
    let deadline = block as f64 / f64::from(RATE) * 1e9;
    // Thread CPU time next to wall time: wall far above CPU means the thread was not running
    // (preempted or blocked), so the spike is not engine work.
    let mut cpu = cell.cpu.clone();
    cpu.sort_unstable();
    let over_1ms = cell
        .times
        .iter()
        .zip(&cell.cpu)
        .filter(|(w, _)| **w > 1_000_000)
        .count();
    let blocked = cell
        .times
        .iter()
        .zip(&cell.cpu)
        .filter(|(w, c)| **w > 1_000_000 && **c * 2 < **w)
        .count();
    let mut worst: Vec<(u64, u64, usize)> = cell
        .times
        .iter()
        .zip(&cell.cpu)
        .enumerate()
        .map(|(i, (w, c))| (*w, *c, i))
        .collect();
    worst.sort_by(|a, b| b.0.cmp(&a.0));
    let worst: Vec<Value> = worst
        .iter()
        .take(8)
        .map(|&(w, c, i)| json!({"block": i, "wall_us": w as f64 / 1e3, "cpu_us": c as f64 / 1e3}))
        .collect();
    let out = json!({
        "cpu_p50_us": percentile(&cpu, 0.5) as f64 / 1e3,
        "cpu_p99_us": percentile(&cpu, 0.99) as f64 / 1e3,
        "cpu_max_us": cpu.last().copied().unwrap_or(0) as f64 / 1e3,
        "blocks_over_1ms": over_1ms,
        "over_1ms_cpu_below_half_wall": blocked,
        "worst_blocks": worst,
        "source": option(args, "--kontakt").or(option(args, "--uvi")),
        "block": block,
        "p50_us": percentile(&sorted, 0.5) as f64 / 1e3,
        "p99_us": percentile(&sorted, 0.99) as f64 / 1e3,
        "p999_us": percentile(&sorted, 0.999) as f64 / 1e3,
        "max_us": sorted.last().copied().unwrap_or(0) as f64 / 1e3,
        "deadline_us": deadline / 1e3,
        "deadline_misses": sorted.iter().filter(|&&ns| ns as f64 > deadline).count(),
        "spikes": spike_report(&cell.times, &cell.recs),
    });
    println!("{out}");
    Ok(())
}
