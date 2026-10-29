//! Loads an instrument, plays one note and writes what the engine renders to
//! a WAV file, for listening and A/B against other samplers. Prints load time,
//! gaps and the script actions the engine does not answer yet.
//! `cargo run --release -p kontra-host --example render -- <file> <key> <out.wav>`
//! `<file>` is a Kontakt instrument, or a UVI program as `bank.ufs/Name.uvip`.

use std::path::PathBuf;
use std::sync::Arc;

use kontra_engine::{Engine, Event, HostNote, HostPattern, Plan, Prepared, SampleTime, Timed};
use kontra_stream::{Store, StoreConfig};

const RATE: u32 = 48_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path: PathBuf = args
        .next()
        .ok_or("usage: render <file> <key> <out.wav>")?
        .into();
    let key: u8 = args
        .next()
        .and_then(|k| k.to_str()?.parse().ok())
        .unwrap_or(60);
    let out: PathBuf = args.next().unwrap_or_else(|| "render.wav".into()).into();
    // Optional: the mod wheel moves to this value one second in.
    let wheel: Option<u8> = args.next().and_then(|a| a.to_str()?.parse().ok());

    // KONTRA_SCRIPT_DUMP=<dir> writes each script's source, data and saved values there.
    if let Some(dir) = std::env::var_os("KONTRA_SCRIPT_DUMP") {
        for (n, s) in kontra_host::instrument::read(&path)?
            .scripts
            .iter()
            .enumerate()
        {
            std::fs::write(
                std::path::Path::new(&dir).join(format!("{n}.ksp")),
                &s.source,
            )?;
            for (name, bytes) in &s.data {
                std::fs::write(
                    // A UVI module's name is a path in its bank.
                    std::path::Path::new(&dir).join(format!("{n}-{}.nka", name.replace('/', "_"))),
                    bytes,
                )?;
            }
            // Its saved persistent values, one per line.
            std::fs::write(
                std::path::Path::new(&dir).join(format!("{n}-saved.txt")),
                s.saved.join("\n"),
            )?;
            let data: Vec<_> = s.data.iter().map(|(name, b)| (name, b.len())).collect();
            println!("  script {n} data files: {data:?}");
        }
    }
    // KONTRA_SAMPLES=<part of a name>: those samples' peaks, decoded directly.
    if let Ok(part) = std::env::var("KONTRA_SAMPLES") {
        let ir = kontra_host::instrument::read(&path)?;
        for s in ir
            .samples
            .iter()
            .filter(|s| s.name.contains(&part))
            .take(12)
        {
            let Ok((source, fmt)) = kontra_stream::open(s) else {
                println!("  {}: does not open", s.name);
                continue;
            };
            let mut bytes = vec![0u8; s.frames as usize * fmt.bytes_per_frame()];
            let read = source.read(0, &mut bytes);
            let width = fmt.encoding.bytes();
            let peak = bytes.chunks_exact(width).fold(0f32, |p, b| {
                let x = match b {
                    [a, b] => f32::from(i16::from_le_bytes([*a, *b])) / 32768.0,
                    [a, b, c] => (i32::from_le_bytes([0, *a, *b, *c]) >> 8) as f32 / 8_388_608.0,
                    [a, b, c, d] => f32::from_le_bytes([*a, *b, *c, *d]),
                    _ => 0.0,
                };
                p.max(x.abs())
            });
            println!(
                "  {} {:?} {} frames: peak {peak:.4} {read:?}",
                s.name, fmt, s.frames
            );
        }
    }
    let t = std::time::Instant::now();
    let mut store = Store::new(StoreConfig::DEFAULT)?;
    let mut ir = kontra_host::instrument::read(&path)?;
    // KONTRA_SCRIPT_SOURCE=<dir>: script n's source is replaced by <dir>/n.ksp
    // when that file exists (edit a dump, add messages, render again).
    if let Some(dir) = std::env::var_os("KONTRA_SCRIPT_SOURCE") {
        for (n, s) in ir.scripts.iter_mut().enumerate() {
            if let Ok(source) =
                std::fs::read_to_string(std::path::Path::new(&dir).join(format!("{n}.ksp")))
            {
                s.source = source;
            }
        }
        // <dir>/append.ksp plays as one more script slot (a probe the oracle runs too).
        if let Ok(source) = std::fs::read_to_string(std::path::Path::new(&dir).join("append.ksp")) {
            ir.scripts.push(kontra_engine::ir::Script {
                name: "append".into(),
                dialect: kontra_engine::ir::Dialect::Ksp,
                source,
                saved: Vec::new(),
                data: Vec::new(),
            });
        }
    }
    let scripts: Vec<String> = ir.scripts.iter().map(|s| s.source.clone()).collect();
    let loaded = kontra_host::instrument::build(ir, RATE, &mut store);
    let pages = loaded.pages.clone();
    println!(
        "{}: {} zones, loaded in {:?}",
        loaded.name,
        loaded.plan.zones.len(),
        t.elapsed()
    );
    println!(
        "  memory: {}, pinned samples {} MB",
        memory(),
        store.pinned_bytes() / 1_000_000
    );
    if !loaded.plan.switches.is_empty() {
        println!(
            "  switches {:?}, default {:?}",
            loaded.plan.switches, loaded.plan.default_keyswitch
        );
    }
    for d in &loaded.diagnostics {
        println!("  {:?} {}: {}", d.impact, d.feature, d.detail);
    }
    // The zones that could answer this key at the played velocity (101), and what they wait on.
    let candidates: Vec<_> = loaded
        .plan
        .zones
        .iter()
        .filter(|z| z.keys.contains(&key) && z.velocities.contains(&101))
        .collect();
    println!("  key {key}: {} zones", candidates.len());
    let mut kinds = std::collections::BTreeMap::new();
    for z in &candidates {
        let mods: Vec<_> = loaded
            .plan
            .groups
            .get(z.group as usize)
            .map(|g| g.mods.iter().map(|m| format!("{:?}", m.kind)).collect())
            .unwrap_or_default();
        let what = format!(
            "{:?} {:?} gain {:.2} mods {mods:?}",
            z.trigger, z.start, z.gain
        );
        *kinds.entry(what).or_insert(0) += 1;
    }
    for (what, n) in kinds.iter().take(8) {
        println!("    {n}× {what}");
    }
    // KONTRA_FX=1 prints the effect racks that reach the voices.
    if std::env::var_os("KONTRA_FX").is_some() {
        let plan = &loaded.plan;
        println!("instrument inserts {:?}", plan.inserts);
        println!("sends {:?}", plan.sends);
        let own = plan
            .slot_shapes
            .shapes
            .iter()
            .filter(|s| !s.laws.is_empty())
            .count();
        println!(
            "slot laws {}, shapes {} ({own} with laws) for {} slots",
            plan.slot_laws.len(),
            plan.slot_shapes.shapes.len(),
            plan.slot_shapes.slots.len()
        );
        for (n, b) in plan.buses.iter().enumerate() {
            println!("bus {n} {b:?}");
        }
        for (n, i) in plan.impulses.iter().enumerate() {
            let frames = i.as_ref().map(|r| r.left.len());
            println!("impulse {n}: {frames:?} frames");
        }
        let mut seen = Vec::new();
        for (n, g) in plan.groups.iter().enumerate() {
            if !g.inserts.is_empty() && !seen.contains(&&g.inserts) {
                seen.push(&g.inserts);
                println!("group {n} inserts {:?}", g.inserts);
            }
        }
    }
    // KONTRA_MODS=1 prints each distinct group modulation.
    if std::env::var_os("KONTRA_MODS").is_some() {
        let mut seen = Vec::new();
        for m in loaded.plan.groups.iter().flat_map(|g| &g.mods) {
            if !seen.contains(&m.kind) {
                seen.push(m.kind);
                println!("mod {:?}", m.kind);
            }
        }
        let mut timed = Vec::new();
        for g in &loaded.plan.groups {
            let line = format!("{:?} {:?} {:?}", g.envelopes, g.lfos, g.envelope);
            if !timed.contains(&line) && (!g.envelopes.is_empty() || !g.lfos.is_empty()) {
                println!("timed {line} amp {:?}", g.envelope);
                timed.push(line);
            }
        }
    }
    if let Ok(zone) = std::env::var("KONTRA_ZONE") {
        let plan = &loaded.plan;
        let z = zone.parse::<usize>().ok().and_then(|z| plan.zones.get(z));
        if let Some((z, g)) = z.and_then(|z| Some((z, plan.groups.get(z.group as usize)?))) {
            println!(
                "  zone {zone}: keys {:?} root {} keytrack {} tune {} group {} bus {} {:?} envelope {:?}",
                z.keys,
                z.root,
                z.keytrack,
                z.tune,
                z.group,
                g.bus,
                plan.buses.get(g.bus),
                g.envelope
            );
            for m in &g.mods {
                println!("    mod {m:?}");
            }
            for (n, b) in g.blends.iter().enumerate() {
                println!("    blend {n} {b:?}");
            }
            for (n, s) in g.shapers.iter().enumerate() {
                println!(
                    "    shaper {n} {:?} len {} mid {:?} end {:?}",
                    s.source,
                    s.table.len(),
                    s.table.get(s.table.len() / 2),
                    s.table.last()
                );
            }
        }
    }
    let stream = store.client();
    if let Some(z) = std::env::var("KONTRA_ZONE")
        .ok()
        .and_then(|z| loaded.plan.zones.get(z.parse::<usize>().ok()?))
    {
        let r = &z.region;
        let mut frames = vec![[0f32; 2]; 48_000];
        let read = r.sample.read(&stream, r.start, &mut frames);
        stream.settle(std::time::Duration::from_secs(5));
        let read = (read, r.sample.read(&stream, r.start, &mut frames));
        let peak = frames.iter().flatten().fold(0f32, |p, s| p.max(s.abs()));
        println!(
            "  region {}..{} of {} frames, loop {:?}: first second peak {peak:.4}, {read:?}",
            r.start,
            r.end,
            r.sample.frames(),
            r.loops
        );
    }
    let plan = Arc::new(loaded.plan);
    let mut engine = Engine::new(Arc::clone(&plan), store.client(), RATE);
    println!("init: unanswered {:?}", engine.unhandled_actions());
    if std::env::var_os("KONTRA_MODS").is_some() {
        let mut seen = Vec::new();
        for m in engine.mods().filter(|m| !m.bypass) {
            let line = format!("{} {:?}", m.name, m.kind);
            if !seen.contains(&line) {
                println!("live mod {line}");
                seen.push(line);
            }
        }
    }
    // The program as its scripts leave it, without playing it.
    if std::env::var_os("KONTRA_INIT_ONLY").is_some() {
        return Ok(());
    }
    let (filtered, cutoffs) = engine.voice_filters();
    if filtered > 0 {
        println!("voice filters: {filtered} groups {cutoffs:?}");
    }
    let (chains, chained) = engine.eq_chains();
    if chains > 0 {
        println!("eq: {chains} chains over {chained} zones");
    }
    let purged: Vec<usize> = engine.purged().collect();
    if !purged.is_empty() {
        let zones = (plan.zones.iter())
            .filter(|z| purged.contains(&(z.group as usize)))
            .count();
        println!(
            "purged: {} groups, {zones} of {} zones",
            purged.len(),
            plan.zones.len()
        );
    }
    // A quarter second after load before the first note, as a player gives
    // a program: a script may wait while it loads (UVI `onLoad`).
    let mut silent = vec![[0.0f32; 2]; 512];
    for _ in 0..RATE as usize / 4 / 512 {
        engine.process(&mut silent, &[]);
        install_requests(&mut engine, &plan);
    }
    // KONTRA_CONTROL=name=value,...: each script control (a KSP one named
    // without its type prefix, a UVI widget or macro by its name) moves there
    // in turn and its callbacks run, as a user's edit would; its effect loads
    // install over a quarter second. "name=value@1.5" moves it 1.5 s into
    // the render instead, at the start of that block.
    let mut timed_controls = Vec::new();
    for entry in std::env::var("KONTRA_CONTROL")
        .iter()
        .flat_map(|s| s.split(','))
    {
        let Some((name, rest)) = entry.split_once('=') else {
            continue;
        };
        let (value, at) = rest.split_once('@').unwrap_or((rest, "0"));
        let (Some((slot, id, value)), Ok(at)) =
            (control(&plan, &scripts, name, value), at.parse::<f32>())
        else {
            println!("control {name}: no such control or value {rest}");
            continue;
        };
        if at > 0.0 {
            timed_controls.push(((at * RATE as f32) as usize / 512, slot, id, value));
            continue;
        }
        println!("control {name} (script {slot}, id {id}) := {value}");
        engine.set_control(slot, id, value);
        for _ in 0..RATE as usize / 4 / 512 {
            engine.process(&mut silent, &[]);
            install_requests(&mut engine, &plan);
        }
    }
    // KONTRA_WIDGET=name=value,...: a Lua script's own widget moves there,
    // as the editor moves it, and its `changed` runs.
    for pair in std::env::var("KONTRA_WIDGET")
        .iter()
        .flat_map(|s| s.split(','))
    {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let Ok(value) = value.parse::<f64>() else {
            continue;
        };
        let found = pages.iter().enumerate().find_map(|(slot, page)| {
            let n = page.as_ref()?.widgets.iter().position(|w| w.name == name)?;
            Some((u8::try_from(slot).ok()?, u32::try_from(n).ok()?))
        });
        let Some((slot, n)) = found else {
            println!("widget {name}: no such widget");
            continue;
        };
        println!("widget {name} (script {slot}, number {n}) := {value}");
        engine.set_widget(slot, n, value);
        for _ in 0..RATE as usize / 4 / 512 {
            engine.process(&mut silent, &[]);
            install_requests(&mut engine, &plan);
        }
    }
    let before: std::collections::HashMap<_, _> = engine.action_calls().into_iter().collect();

    let host = HostNote {
        port: 0,
        channel: 0,
        key,
        id: -1,
        clap: true,
    };
    // By default the oracle's note: velocity 100 held 1.5 s (librender2.py: 1440 ticks at
    // 480 PPQ and 120 bpm), so a comparison never sets its release against a held note.
    // KONTRA_VELOCITY (1..=127) and KONTRA_HOLD (seconds) override it.
    let env = |name: &str| std::env::var(name).ok()?.parse::<f32>().ok();
    let velocity = env("KONTRA_VELOCITY").unwrap_or(100.0) / 127.0;
    let hold_s = env("KONTRA_HOLD").unwrap_or(1.5);
    let hold = (hold_s * RATE as f32) as usize / 512;
    let on = Timed {
        at: SampleTime(0),
        body: Event::NoteOn {
            note: host,
            velocity: velocity.into(),
            tune: 0.0,
        },
    };
    let pattern = HostPattern {
        port: 0,
        channel: 0,
        key: i32::from(key),
        id: -1,
        clap: true,
    };
    let off = Timed {
        at: SampleTime(0),
        body: Event::NoteOff(pattern),
    };
    let live = std::env::var_os("KONTRA_LIVE").is_some();
    // KONTRA_WATCH=102,104: the controllers the block lines print (110, 111).
    let watched: Vec<usize> = std::env::var("KONTRA_WATCH")
        .unwrap_or_else(|_| "110,111".into())
        .split(',')
        .filter_map(|c| c.trim().parse().ok())
        .collect();
    let mut audio = Vec::new();
    let mut block = vec![[0.0f32; 2]; 512];
    // Time inside `Engine::process` alone (not disk waits): the render's CPU.
    let mut busy = std::time::Duration::ZERO;
    // KONTRA_BEND=0: pitch bend (its high 7 bits) sent with the wheel at 1 s.
    let bend = std::env::var("KONTRA_BEND")
        .ok()
        .and_then(|b| b.parse::<u32>().ok());
    let cc: Vec<_> = (wheel.map(|v| 0x20B0_0100 | u32::from(v)).into_iter())
        .chain(bend.map(|b| 0x20E0_0000 | (b & 127)))
        .map(|word| Timed {
            at: SampleTime(0),
            body: Event::Ump([word, 0]),
        })
        .collect();
    // KONTRA_CC="11=127,1=100": controllers sent with the note-on;
    // "7:64@1.5" (or "7=64@1.5") sends one 1.5 s into the render, on its frame;
    // "pb:16383@1" is a 14-bit pitch bend.
    let rides: Vec<(usize, Event)> = std::env::var("KONTRA_CC")
        .unwrap_or_default()
        .split(',')
        .filter_map(|entry| {
            let (cc, rest) = entry.split_once(['=', ':'])?;
            let (v, at) = rest.split_once('@').unwrap_or((rest, "0"));
            let v: u32 = v.trim().parse().ok()?;
            let at: f32 = at.trim().parse().ok()?;
            let word = match cc.trim() {
                "pb" => 0x20E0_0000 | (v & 127) << 8 | (v >> 7 & 127),
                cc => 0x20B0_0000 | (cc.parse::<u32>().ok()? & 127) << 8 | (v & 127),
            };
            Some(((at * RATE as f32) as usize, Event::Ump([word, 0])))
        })
        .collect();
    let mut first: Vec<Timed<Event>> = (rides.iter().filter(|r| r.0 == 0))
        .map(|&(_, body)| Timed {
            at: SampleTime(0),
            body,
        })
        .collect();
    first.push(on);
    // KONTRA_CHORDS=1: instead of the one note, 64 steps of 4-note chords
    // 128 ms apart across the keyboard, each held two steps (contracts C2/C8;
    // with KONTRA_LIVE=1 and a cold page cache, late pages show as missing).
    let cc_off: Vec<_> = cc.iter().copied().chain([off]).collect();
    let chords = std::env::var_os("KONTRA_CHORDS").is_some();
    let hold = if chords { 64 * 12 + 24 } else { hold };
    let keys = |step: usize| (0..4).map(move |i| 36 + ((step * 7 + i * 5) % 60) as u8);
    for n in 0..(hold + 2 * RATE as usize / 512) {
        let step = n / 12;
        let chord: Vec<Timed<Event>> = if chords && n % 12 == 0 && step < 64 {
            let offs = (step.checked_sub(2).into_iter().flat_map(keys)).map(|k| {
                Event::NoteOff(HostPattern {
                    key: i32::from(k),
                    ..pattern
                })
            });
            let ons = keys(step).map(|key| Event::NoteOn {
                note: HostNote { key, ..host },
                velocity: velocity.into(),
                tune: 0.0,
            });
            (offs.chain(ons))
                .map(|body| Timed {
                    at: SampleTime(0),
                    body,
                })
                .collect()
        } else {
            Vec::new()
        };
        let input: &[Timed<Event>] = match n {
            _ if chords => &chord,
            0 => &first,
            // A one-second hold lands on the controllers' block: send both.
            n if n == RATE as usize / 512 && n == hold => cc_off.as_slice(),
            n if n == RATE as usize / 512 => cc.as_slice(),
            n if n == hold => std::slice::from_ref(&off),
            _ => &[],
        };
        let now = engine.now().0;
        let ridden: Vec<Timed<Event>> = (rides.iter())
            .filter(|r| r.0 > 0 && r.0 / 512 == n)
            .map(|&(frame, body)| Timed {
                at: SampleTime(now + (frame % 512) as u64),
                body,
            })
            .chain(input.iter().copied())
            .collect();
        let input = ridden.as_slice();
        // A bounce waits for the disk; KONTRA_LIVE=1 plays in real time
        // without waiting, as a host would, so late pages show as missing frames.
        let started = std::time::Instant::now();
        if !live {
            stream.settle(std::time::Duration::from_secs(5));
        }
        for &(_, slot, id, value) in timed_controls.iter().filter(|c| c.0 == n) {
            engine.set_control(slot, id, value);
        }
        let processing = std::time::Instant::now();
        engine.process(&mut block, input);
        busy += processing.elapsed();
        install_requests(&mut engine, &plan);
        if n == 0 || n == 3 * RATE as usize / 512 {
            let during: Vec<_> = engine
                .action_calls()
                .into_iter()
                .filter_map(|(a, c)| {
                    let d = c - before.get(&a).copied().unwrap_or(0);
                    (d > 0).then_some((a, d))
                })
                .collect();
            println!("  calls by block {n}: {during:?}");
        }
        if n == 1 || n == RATE as usize / 512 + 1 || n == 2 * RATE as usize / 512 + 20 {
            println!(
                "  block {n}: {} voices, {} playing, controllers {:?}",
                engine.active_voices(),
                engine.playing_voices(),
                watched
                    .iter()
                    .map(|&cc| (cc, engine.controller(cc)))
                    .collect::<Vec<_>>()
            );
            for (zone, at, gain) in engine.voices() {
                println!("    zone {zone} at {at:.0} gain {gain:?}");
            }
            let (filtered, cutoffs) = engine.voice_filters();
            if filtered > 0 {
                println!("    voice filter cutoffs {cutoffs:?}");
            }
        }
        if live {
            let period = std::time::Duration::from_secs_f64(512.0 / f64::from(RATE));
            std::thread::sleep(period.saturating_sub(started.elapsed()));
        }
        audio.extend_from_slice(&block);
    }
    let peak = audio.iter().flatten().fold(0f32, |p, s| p.max(s.abs()));
    let rms = |from: usize, to: usize| {
        let s = audio.get(from..to).unwrap_or_default();
        (s.iter().flatten().map(|x| x * x).sum::<f32>() / (2 * s.len().max(1)) as f32).sqrt()
    };
    // The last half second held, and 0.1..0.6 s after the key-up.
    let (r, up) = (RATE as usize, hold * 512);
    let at = |frame: usize| frame as f32 / RATE as f32;
    let (held, gone) = (up.saturating_sub(r / 2), up + r / 10);
    println!(
        "rms 0.5-1 s {:.4}, {:.1}-{:.1} s {:.4}, after release {:.1}-{:.1} s {:.4}",
        rms(r / 2, r),
        at(held),
        at(up),
        rms(held, up),
        at(gone),
        at(gone + r / 2),
        rms(gone, gone + r / 2)
    );
    println!("peak {peak:.3}, {:?}", engine.stats());
    let seconds = audio.len() as f64 / f64::from(RATE);
    println!(
        "render cpu {:.1} ms for {seconds:.1} s of audio: rtf {:.5}",
        busy.as_secs_f64() * 1e3,
        busy.as_secs_f64() / seconds.max(1e-9)
    );
    println!("stream: {:?}", stream.stats());
    println!("all actions: {:?}", engine.unhandled_actions());
    println!("last message: {}", engine.last_message());
    println!("script faults: {:?}", engine.script_faults());
    println!(
        "script arena spills: {}, memory after render: {}",
        kontra_lua::alloc::spills(),
        memory()
    );
    write_wav(&out, &audio)?;
    println!("wrote {}", out.display());
    Ok(())
}

/// Resident and peak memory from `/proc/self/status`.
fn memory() -> String {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .filter(|l| l.starts_with("VmRSS") || l.starts_with("VmHWM"))
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join(", ")
}

fn write_wav(path: &std::path::Path, frames: &[[f32; 2]]) -> std::io::Result<()> {
    let data = (frames.len() * 8) as u32;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend(b"RIFF");
    b.extend((36 + data).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(3u16.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(RATE.to_le_bytes());
    b.extend((RATE * 8).to_le_bytes());
    b.extend(8u16.to_le_bytes());
    b.extend(32u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data.to_le_bytes());
    for s in frames.iter().flatten() {
        b.extend(s.to_le_bytes());
    }
    std::fs::write(path, b)
}

/// Script control `name` and `value` as `Engine::set_control` takes them:
/// a UVI script's by its name among its controls (the last of that name, a
/// widget before a macro), its value an `f64`; a KSP one's UI id is the first
/// one plus its variable's index (persist.rs), its value an integer.
fn control(plan: &Plan, scripts: &[String], name: &str, value: &str) -> Option<(u8, i32, i64)> {
    plan.scripts
        .iter()
        .zip(scripts)
        .enumerate()
        .find_map(|(n, (script, source))| {
            let slot = u8::try_from(n).ok()?;
            if let Some(foreign) = &script.foreign {
                let id = foreign.controls().iter().rposition(|c| c == name)?;
                let value = value.parse::<f64>().ok()?.to_bits() as i64;
                return Some((slot, i32::try_from(id).ok()?, value));
            }
            let compiled = kontra_ksp::compile(source, &[]).ok()?;
            let bare = |v: &str| v.trim_start_matches(['$', '%', '~', '?', '@', '!']) == name;
            let at = compiled.variables.iter().position(|v| bare(&v.name))?;
            let id = kontra_ksp::FIRST_UI_ID + i32::try_from(at).ok()?;
            Some((slot, id, value.parse().ok()?))
        })
}

/// Builds and installs the effects and responses scripts asked for, between
/// blocks, as the host's rack does off the audio thread.
fn install_requests(engine: &mut Engine, plan: &Plan) {
    let mut asked = Vec::new();
    engine.drain_requests(|r| asked.push(r));
    for r in asked {
        if std::env::var_os("KONTRA_FX").is_some() {
            let kind = r.effect.as_ref().map(|e| &e.kind);
            println!(
                "installs {:?} slot {} type {:?}: {kind:?}",
                r.at, r.slot, r.ty
            );
        }
        engine.install(Prepared::new(r, Some(plan), RATE));
    }
}
