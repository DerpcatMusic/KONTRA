use anyhow::{Context,Result,ensure};
use std::path::Path;
use kontakto::{audio::Sample,engine::{Bank,Engine,MAX_BLOCK,MAX_VOICES,NoteEvent,load_scripts},import::{self,Group,Loop,Zone}};
fn main() -> Result<()> {
 let args:Vec<_>=std::env::args().collect();
 match args.get(1).map(String::as_str) {
  Some("scan") => {for p in import::presets(Path::new(args.get(2).map(String::as_str).unwrap_or(import::LIBRARY_ROOT)))? {println!("{}",p.display());}},
  Some("inspect") => {let p=args.get(2).context("inspect requires an NKI path")?; println!("{}",serde_json::to_string_pretty(&import::read(Path::new(p))?)?);},
  Some("inspect-fx") => {let path=args.get(2).context("inspect-fx requires an NKI/NKM path")?;println!("{}",serde_json::to_string_pretty(&inspect_fx(Path::new(path))?)?);},
  Some("audit-fx") => {println!("{}",serde_json::to_string_pretty(&audit_fx(Path::new(args.get(2).map(String::as_str).unwrap_or(import::LIBRARY_ROOT)))?)?);},
  Some("inspect-mods") => {let p=args.get(2).context("inspect-mods requires an NKI path")?;println!("{}",serde_json::to_string_pretty(&inspect_mods(Path::new(p))?)?);},
  Some("inspect-multi") => {let path=args.get(2).context("inspect-multi requires an NKM path")?;println!("{}",serde_json::to_string_pretty(&import::read_multi(Path::new(path))?)?);},
  Some("ui") => {let p=args.get(2).context("ui requires an NKI path")?;let i=import::read(Path::new(p))?;let mut host=kontakto::ksp::HostState::default();let report:Vec<_>=i.scripts.iter().map(|s|match kontakto::ksp::initialize_with_host(s,i.groups.len(),8,&mut host){Ok(ui)=>serde_json::json!({"interface":ui}),Err(e)=>serde_json::json!({"error":format!("{e:#}")})}).collect();println!("{}",serde_json::to_string_pretty(&report)?);},
  Some("audit-archives") => {
    let root=Path::new(args.get(2).map(String::as_str).unwrap_or(import::LIBRARY_ROOT));let mut report=Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
      let entry=entry?;let path=entry.path();if !entry.file_type().is_file() || !path.extension().is_some_and(|s|s.eq_ignore_ascii_case("nkx") || s.eq_ignore_ascii_case("nkr")){continue;}
      eprintln!("Archive {}",path.display());let result=std::fs::File::open(path).map_err(anyhow::Error::from).and_then(|f|ni_file::nkr::Archive::read(f).map_err(anyhow::Error::from));
      report.push(match result{Ok(a)=>{let mut damage=std::collections::BTreeMap::<&str,usize>::new();for entry in a.entries.values(){if let Some(issue)=entry.issue{*damage.entry(issue).or_default()+=1;}}serde_json::json!({"path":path,"members":a.entries.len(),"invalid_headers":a.entries.values().filter(|e|!e.valid).count(),"member_issues":damage,"issues":a.issues,"payloads_verified":false})},Err(e)=>serde_json::json!({"path":path,"error":format!("{e:#}")})});
    }
    report.sort_by(|a,b|a["path"].as_str().cmp(&b["path"].as_str()));println!("{}",serde_json::to_string_pretty(&report)?);
  },
  Some("audit-scripts") => {
    let root=Path::new(args.get(2).map(String::as_str).unwrap_or(import::LIBRARY_ROOT));let mut report=Vec::new();
    for path in import::presets(root)? {eprintln!("Scripts {}",path.display());let item=match import::script_inventory(&path){Ok(v)=>serde_json::json!({"path":path,"programs":v}),Err(e)=>serde_json::json!({"path":path,"error":format!("{e:#}")})};report.push(item);}
    println!("{}",serde_json::to_string_pretty(&report)?);
  },
  Some("audit-structure") => {
    let root=Path::new(args.get(2).map(String::as_str).unwrap_or(import::LIBRARY_ROOT));let mut report=Vec::new();
    for path in import::presets(root)? {eprintln!("Inventory {}",path.display());let item=match import::source_inventory(&path){Ok(v)=>serde_json::json!({"path":path,"inventory":v}),Err(e)=>serde_json::json!({"path":path,"error":format!("{e:#}")})};report.push(item);}
    println!("{}",serde_json::to_string_pretty(&report)?);
  },
  Some("audit") => {
    let root=Path::new(args.get(2).map(String::as_str).unwrap_or(import::LIBRARY_ROOT));
    let mut report=Vec::new();
    for path in import::presets(root)? {
       eprintln!("Inspecting {}",path.display());
       let programs=if import::is_multi(&path){import::read_multi(&path).map(|m|m.parts.into_iter().map(|p|p.program).collect::<Vec<_>>())}else{Ok(vec![0])};
       match programs {
         Err(e)=>report.push(serde_json::json!({"path":path,"error":format!("{e:#}")})),
         Ok(programs)=>for program in programs {
           let item=match import::read_program(&path,program) {
             Ok(i)=>{
               let mut host=kontakto::ksp::HostState::default();let scripts:Vec<_>=i.scripts.iter().enumerate().map(|(slot,s)|{
                 let mut report=kontakto::ksp::inspect(s,i.groups.len(),&mut host);report["slot"]=serde_json::json!(slot);report
               }).collect();
               serde_json::json!({"path":path,"program":program,"name":i.name,"groups":i.groups.len(),"zones":i.zones.len(),"complete_groups":i.groups.iter().enumerate().filter(|(n,_)|{let mut z=i.zones.iter().filter(|z|z.group==*n).peekable();z.peek().is_some() && z.all(|z|z.available)}).count(),"missing_samples":i.missing_samples,"warnings":i.warnings,"scripts":scripts,"audio_decode_verified":false,"kontakt_behavior_verified":false})
             },
             Err(e)=>serde_json::json!({"path":path,"program":program,"error":format!("{e:#}")}),
           };report.push(item);
         }
       }
    }
    println!("{}",serde_json::to_string_pretty(&report)?);
  },
  Some("render") => render(&args[2..])?,
  Some("ksp-run") => ksp_run(Path::new(args.get(2).context("ksp-run requires an NKI path")?),&args[3..])?,
  Some("bench") => bench(args.get(2).map(|s|s.parse()).transpose()?.unwrap_or(1000),args.get(3).map(|s|s.parse()).transpose()?.unwrap_or(24))?,
  Some("bench-load") => for p in &args[2..] {bench_load(Path::new(p))?},
  Some("bench-stream") => bench_stream(Path::new(args.get(2).context("bench-stream requires an NKI path")?),args.get(3).map(|s|s.parse()).transpose()?.unwrap_or(64),args.get(4).map(|s|s.parse()).transpose()?.unwrap_or(10.0))?,
  Some("bench-script") => bench_script(Path::new(args.get(2).context("bench-script requires an NKI path")?),args.get(3).map(|s|s.parse()).transpose()?.unwrap_or(20.0))?,
  _=> println!("kontakto scan [folder]\nkontakto inspect <instrument.nki>\nkontakto inspect-multi <multi.nkm>\nkontakto inspect-mods <instrument.nki>\nkontakto inspect-fx <instrument.nki>\nkontakto audit-fx [folder]\nkontakto ui <instrument.nki>\nkontakto audit [folder]\nkontakto audit-structure [folder]\nkontakto audit-scripts [folder]\nkontakto audit-archives [folder]\nkontakto render [--dry] [--no-script] [--notes 60@0-600,62@500-1100:90] [--cc 11@0:40,11@500:127] <instrument.nki> <output.wav> [group=all] [note=first root] [velocity=zone midpoint]\nkontakto ksp-run <instrument.nki> [note[@on_ms[-off_ms]][:velocity]...]\nkontakto bench [voices=1000] [bits=24|16|32]\nkontakto bench-script <instrument.nki> [seconds=20]\nkontakto bench-load <instrument.nki>...\nkontakto bench-stream <instrument.nki> [notes=64] [seconds=10]"),
 }
 Ok(())
}

fn inspect_fx(path: &Path) -> Result<serde_json::Value> {
    let programs = import::read_fx(path)?
        .into_iter()
        .map(|(program, fx)| {
            serde_json::json!({"program": program, "warnings": fx.warnings(), "effects": fx})
        })
        .collect();
    Ok(serde_json::Value::Array(programs))
}

/// Instance counts per effect kind and rack, split by bypass state.
fn audit_fx(root: &Path) -> Result<serde_json::Value> {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<String, BTreeMap<String, [usize; 2]>> = BTreeMap::new();
    let (mut presets, mut failures) = (0, Vec::new());
    for path in import::presets(root)? {
        eprintln!("Effects {}", path.display());
        match import::read_fx(&path) {
            Ok(programs) => {
                presets += 1;
                for (_, fx) in programs {
                    for (location, effect) in fx.effects() {
                        let rack = if location.starts_with("bus") { "bus".into() } else { location };
                        let row = counts.entry(effect.kind.name()).or_default();
                        row.entry(rack).or_default()[usize::from(effect.bypass)] += 1;
                    }
                }
            }
            Err(e) => failures.push(serde_json::json!({"path": path, "error": format!("{e:#}")})),
        }
    }
    let kinds: BTreeMap<_, _> = counts
        .into_iter()
        .map(|(kind, racks)| {
            let racks: BTreeMap<_, _> = racks
                .into_iter()
                .map(|(rack, [active, bypassed])| {
                    (rack, serde_json::json!({"active": active, "bypassed": bypassed}))
                })
                .collect();
            (kind, racks)
        })
        .collect();
    Ok(serde_json::json!({"presets": presets, "failures": failures, "kinds": kinds}))
}

const RATE: f64 = 48_000.0;

/// One MIDI note input: frame, note-on, note, velocity.
type NoteInput = (u64, bool, u8, u8);

/// Parse `note[@on_ms[-off_ms]][:velocity]` specs into time-ordered note input.
/// Without times, notes are 400 ms apart and overlap by 100 ms (legato); without
/// an end, a note lasts 400 ms.
fn parse_notes<'a>(specs: impl IntoIterator<Item = &'a str>) -> Result<Vec<NoteInput>> {
    let ms = |s: &str| s.parse::<f64>().map(|ms| (ms * RATE / 1e3) as u64);
    let mut input = Vec::new();
    for (i, spec) in specs.into_iter().enumerate() {
        let (spec, velocity) = spec
            .split_once(':')
            .map_or((spec, Ok(100)), |(s, v)| (s, v.parse::<u8>()));
        let (note, times) = spec
            .split_once('@')
            .map_or((spec, None), |(n, t)| (n, Some(t)));
        let note: u8 = note.parse().with_context(|| format!("Bad note {spec}"))?;
        let velocity = velocity.with_context(|| format!("Bad velocity in {spec}"))?;
        ensure!(
            note < 128 && (1..128).contains(&velocity),
            "Notes and velocities must be MIDI values"
        );
        let (on, off) =
            match times.map(|t| t.split_once('-').map_or((t, None), |(a, b)| (a, Some(b)))) {
                Some((on, off)) => {
                    let on = ms(on)?;
                    (
                        on,
                        off.map(ms).transpose()?.unwrap_or(on + (0.4 * RATE) as u64),
                    )
                }
                None => {
                    let on = (i as f64 * 0.4 * RATE) as u64;
                    (on, on + (0.5 * RATE) as u64)
                }
            };
        ensure!(off > on, "Note-off must follow note-on in {spec}");
        input.push((on, true, note, velocity));
        input.push((off, false, note, 0));
    }
    // Note-offs sort before note-ons at the same frame.
    input.sort();
    Ok(input)
}

/// Parse `cc@ms:value` specs (`11@0:40,11@500:127`) into time-ordered
/// controller input: frame, controller, value.
fn parse_ccs(list: &str) -> Result<Vec<(u64, u8, u8)>> {
    let mut input = list
        .split(',')
        .map(|spec| {
            let parsed = spec.split_once('@').and_then(|(cc, rest)| {
                let (ms, value) = rest.split_once(':')?;
                let frame = (ms.parse::<f64>().ok()? * RATE / 1e3) as u64;
                Some((frame, cc.parse::<u8>().ok()?, value.parse::<u8>().ok()?))
            });
            parsed
                .filter(|&(_, cc, value)| cc < 128 && value < 128)
                .with_context(|| format!("Bad controller {spec}; expected cc@ms:value"))
        })
        .collect::<Result<Vec<_>>>()?;
    input.sort_by_key(|e| e.0);
    Ok(input)
}

fn play(engine: &mut Engine, &(_, on, note, velocity): &NoteInput) {
    if on {
        engine.note_on(0, note, velocity);
    } else {
        engine.note_off(0, note);
    }
}

/// Load an instrument's scripts for `engine`, reporting slot errors.
fn install_scripts(engine: &mut Engine, instrument: &import::Instrument) {
    let (script, errors) = load_scripts(instrument, instrument.script_state.clone(), engine.rate());
    for error in errors {
        eprintln!("{error}");
    }
    engine.set_script(script);
}

/// Render notes through the instrument scripts (`--no-script` plays MIDI
/// directly), every playable group (or one group, unscripted) and the
/// instrument effects (`--dry` skips them) to a WAV file, with the tail.
/// `--notes 60@0-600,62@500-1100:90` plays a sequence (times in ms); otherwise
/// one note is held for 2 s. `--cc 11@0:40,11@500:127` sends controllers on
/// channel 1 (times in ms), before notes at the same time.
fn render(args: &[String]) -> Result<()> {
    let flag = |name: &str| args.iter().any(|a| a == name);
    let (dry, no_script) = (flag("--dry"), flag("--no-script"));
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .map(|i| args.get(i + 1).with_context(|| format!("{name} requires a list")))
            .transpose()
    };
    let (notes, ccs) = (option("--notes")?, option("--cc")?);
    let args: Vec<_> = args
        .iter()
        .filter(|a| !a.starts_with("--") && Some(*a) != notes && Some(*a) != ccs)
        .cloned()
        .collect();
    let ccs = ccs.map(|list| parse_ccs(list)).transpose()?.unwrap_or_default();
    let instrument = import::read(Path::new(
        args.first().context("render requires an NKI path")?,
    ))?;
    let output = args.get(1).context("render requires an output WAV path")?;
    ensure!(
        !Path::new(output).exists(),
        "Output already exists; choose a new output path"
    );
    let group: Option<usize> = args
        .get(2)
        .filter(|s| *s != "all")
        .map(|s| s.parse())
        .transpose()?;
    let bank = Bank::load(&instrument)?;
    if bank.skipped_zones > 0 {
        eprintln!(
            "Skipped {} zones: {}",
            bank.skipped_zones,
            bank.issues.join("; ")
        );
    }
    let first = bank
        .zones()
        .iter()
        .find(|z| group.is_none_or(|g| z.group == g))
        .context("Group has no playable zones")?;
    let note = args
        .get(3)
        .map(|s| s.parse::<u8>())
        .transpose()?
        .unwrap_or(first.root);
    ensure!(note < 128, "MIDI note must be 0..127");
    let velocity = args
        .get(4)
        .map(|s| s.parse::<u8>())
        .transpose()?
        .unwrap_or_else(|| {
            let zone = bank
                .zones()
                .iter()
                .find(|z| (z.low_key..=z.high_key).contains(&note));
            zone.map_or(100, |z| {
                ((u16::from(z.low_velocity) + u16::from(z.high_velocity)) / 2).max(1) as u8
            })
        });
    let streamed = bank.streamed_samples();
    let mut engine = Engine::default();
    engine.blocking_streams = true;
    engine.set_bank(Some(Box::new(bank)));
    if !dry {
        engine.set_fx(instrument.fx.processor(engine.rate() as f32, MAX_BLOCK));
    }
    if let Some(g) = group {
        engine.set_all_groups_allowed(false);
        engine.set_group_allowed(g, true);
    }
    if !no_script {
        install_scripts(&mut engine, &instrument);
    }
    let input = match notes {
        Some(list) => parse_notes(list.split(','))?,
        None => vec![
            (0, true, note, velocity),
            ((2.0 * RATE) as u64, false, note, 0),
        ],
    };
    let last_off = input.last().map_or(0, |e| e.0).max(ccs.last().map_or(0, |e| e.0));
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(output, spec)?;
    let (mut peak, mut square) = (0f32, 0f64);
    // Largest sample-to-sample step: clicks at note transitions show up here.
    let (mut jump, mut jump_at, mut previous) = (0f32, 0, [0f32; 2]);
    let (mut left, mut right) = ([0f32; MAX_BLOCK], [0f32; MAX_BLOCK]);
    // Render until the last note-off, then until voices end and the effect tails
    // fall below -120 dB, for at least 4 s and at most 60 s. Rendering splits at
    // note times so input lands on its exact frame.
    let (min_frames, max_frames) = ((4.0 * RATE) as u64, (60.0 * RATE) as u64);
    let (mut frame, mut next, mut voices_end, mut last_audible) = (0u64, 0, None, 0);
    let mut next_cc = 0;
    loop {
        while let Some(&(_, cc, value)) = ccs.get(next_cc).filter(|e| e.0 <= frame) {
            engine.cc(0, cc, value);
            next_cc += 1;
        }
        while let Some(event) = input.get(next).filter(|e| e.0 <= frame) {
            play(&mut engine, event);
            next += 1;
        }
        let until = |t: Option<u64>| t.map_or(MAX_BLOCK as u64, |t| t - frame);
        let len = until(input.get(next).map(|e| e.0))
            .min(until(ccs.get(next_cc).map(|e| e.0)))
            .min(MAX_BLOCK as u64) as usize;
        engine.render(&mut left[..len], &mut right[..len]);
        let mut block_peak = 0f32;
        for (i, (l, r)) in left[..len].iter().zip(&right[..len]).enumerate() {
            let pair = [l * 0.25, r * 0.25];
            for (sample, before) in pair.iter().zip(&previous) {
                ensure!(sample.is_finite(), "Nonfinite rendered audio");
                block_peak = block_peak.max(sample.abs());
                square += f64::from(*sample) * f64::from(*sample);
                writer.write_sample(*sample)?;
                if (sample - before).abs() > jump {
                    (jump, jump_at) = ((sample - before).abs(), frame + i as u64);
                }
            }
            previous = pair;
        }
        peak = peak.max(block_peak);
        frame += len as u64;
        if block_peak > peak * 1e-3 {
            last_audible = frame;
        }
        if frame > last_off && engine.active_voices() == 0 {
            voices_end.get_or_insert(frame);
        }
        if frame >= max_frames
            || (frame >= min_frames && voices_end.is_some() && block_peak <= 1e-6)
        {
            break;
        }
    }
    writer.finalize()?;
    let seconds = |frames: u64| frames as f64 / RATE;
    let tail = voices_end.map_or("voices still playing at 60 s".into(), |end| {
        format!(
            "{:.2} s to -60 dB re peak after the last voice",
            seconds(last_audible.saturating_sub(end))
        )
    });
    ensure!(
        peak > 0.00001,
        "Rendered silence; chosen key/velocity has no audible zone"
    );
    let groups = group.map_or_else(
        || "all groups".to_string(),
        |g| format!("group {g} ({})", instrument.groups[g].name),
    );
    let rms = (square / (frame * 2) as f64).sqrt();
    let fx = if dry { "dry" } else { "with effects" };
    let scripts = match engine.script() {
        Some(rt) => format!("{} script slots", rt.slots()),
        None => "no scripts".into(),
    };
    let mut played = notes.map_or_else(|| format!("note {note}"), |n| format!("notes {n}"));
    if !ccs.is_empty() {
        played += &format!(" · {} CC changes", ccs.len());
    }
    println!(
        "{} · {groups} · {played} · {scripts} · {fx} · {:.2} s · peak {peak:.6} · RMS {rms:.6} · max step {jump:.6} at {:.3} s · sound until {:.2} s (last note-off {:.2} s) · {tail} · {streamed} streamed samples · {} underruns",
        instrument.name,
        seconds(frame),
        seconds(jump_at),
        seconds(last_audible),
        seconds(last_off),
        engine.underruns()
    );
    if let Some(rt) = engine.script() {
        for line in rt.diagnostics() {
            eprintln!("Script: {line}");
        }
    }
    for warning in instrument.warnings {
        eprintln!("Compatibility: {warning}");
    }
    Ok(())
}

/// Voices one core renders in real time: 48 kHz, 128-frame blocks, stereo,
/// pitched looping voices with loop crossfades, playing a sample of `bits`
/// resolution (resident as 16-bit, 24-bit or f32).
fn bench(voices: usize, bits: i32) -> Result<()> {
    ensure!(
        (1..=MAX_VOICES).contains(&voices),
        "voices must be 1..={MAX_VOICES}"
    );
    let frames: Vec<[f32; 2]> = (0..96000)
        .map(|i| {
            let scale = 2f32.powi(bits - 1);
            let x = (i as f32 * 0.013).sin();
            [x * 0.9, x * 0.6].map(|v| (v * scale).round() / scale)
        })
        .collect();
    let group = Group {
        name: "bench".into(),
        ..Group::default()
    };
    let zone = Zone {
        low_velocity: 1,
        loop_range: Some(Loop {
            start: 20000,
            end: 90000,
            until_release: false,
            crossfade: 2000,
        }),
        ..Zone::default()
    };
    let mut bank = Bank::from_samples(
        vec![group],
        vec![zone],
        vec![(
            Default::default(),
            Sample {
                rate: 44100,
                frames,
            },
        )],
    )?;
    bank.set_polyphony(MAX_VOICES);
    let mut engine = Engine::default();
    engine.set_bank(Some(Box::new(bank)));
    for i in 0..voices {
        let mut event = NoteEvent::new((i % 16) as u8, 36 + (i % 48) as u8, 100);
        event.tune = (i % 7) as f64 * 0.013;
        engine.start_event(&event);
    }
    ensure!(
        engine.active_voices() == voices,
        "only {} voices started",
        engine.active_voices()
    );
    let (mut left, mut right) = ([0f32; MAX_BLOCK], [0f32; MAX_BLOCK]);
    // Best of several runs: the minimum is the least disturbed by other load.
    let seconds = 2.0;
    let blocks = (48000.0 * seconds / MAX_BLOCK as f64) as usize;
    let mut checksum = 0f32;
    let mut cpu = f64::MAX;
    for _ in 0..7 {
        let started = std::time::Instant::now();
        for _ in 0..blocks {
            engine.render(&mut left, &mut right);
            checksum += left[0] + right[MAX_BLOCK - 1];
        }
        cpu = cpu.min(started.elapsed().as_secs_f64());
    }
    ensure!(
        engine.active_voices() == voices && checksum.is_finite(),
        "voices ended during the benchmark"
    );
    let realtime = seconds / cpu;
    println!(
        "{voices} voices · {seconds} s audio in {cpu:.3} s (best of 7) · {realtime:.1}x real time · {:.0} voices per core",
        voices as f64 * realtime
    );
    Ok(())
}

/// Import, bank load and script init time plus resident memory of each instrument.
fn bench_load(path: &Path) -> Result<()> {
    let name = path.file_stem().unwrap_or_default().to_string_lossy();
    let started = std::time::Instant::now();
    let instrument = import::read(path)?;
    let import_ms = started.elapsed().as_secs_f64() * 1e3;
    let started = std::time::Instant::now();
    let bank = match Bank::load(&instrument) {
        Ok(bank) => bank,
        Err(e) => {
            println!("{name}: load failed after {:.0} ms: {e:#}", started.elapsed().as_secs_f64() * 1e3);
            return Ok(());
        }
    };
    let load_ms = started.elapsed().as_secs_f64() * 1e3;
    let (mib, preload, samples, streamed, zones, skipped) = (
        bank.bytes as f64 / (1 << 20) as f64,
        bank.preload,
        bank.sample_count(),
        bank.streamed_samples(),
        bank.zones().len(),
        bank.skipped_zones,
    );
    let mut engine = Engine::default();
    engine.set_bank(Some(Box::new(bank)));
    let started = std::time::Instant::now();
    install_scripts(&mut engine, &instrument);
    let init_ms = started.elapsed().as_secs_f64() * 1e3;
    let status = |key: &str| {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                let line = s.lines().find(|l| l.starts_with(key))?;
                line.split_whitespace().nth(1)?.parse::<f64>().ok()
            })
            .map_or(0.0, |kib| kib / 1024.0)
    };
    let (rss, peak) = (status("VmRSS:"), status("VmHWM:"));
    println!(
        "{name}: {mib:.1} MiB resident (preload {preload}) · RSS {rss:.0} MiB (peak {peak:.0}) · {samples} samples ({streamed} streamed) · {zones} zones ({skipped} skipped) · import {import_ms:.0} ms · load {load_ms:.0} ms · scripts {init_ms:.0} ms · Kontakt stores {:.0} MiB of samples, preload override {}",
        instrument.kontakt_sample_bytes / (1 << 20) as f64,
        instrument.kontakt_preload
    );
    Ok(())
}

/// Time an instrument with and without its scripts on a dense stream: a legato
/// line (150 ms steps, 30 ms overlap) plus a three-note chord every second.
/// 48 kHz, 128-frame blocks, effects on. Streams are not awaited, so late disk
/// data plays as silence; the per-voice work is the same.
fn bench_script(path: &Path, seconds: f64) -> Result<()> {
    let instrument = import::read(path)?;
    ensure!(!instrument.scripts.is_empty(), "Instrument has no scripts");
    let bank = Bank::load(&instrument)?;
    let low = bank
        .zones()
        .iter()
        .map(|z| z.low_key)
        .min()
        .context("Instrument has no playable zones")?;
    let high = bank.zones().iter().map(|z| z.high_key).max().unwrap_or(low);
    let mid = (u16::from(low) + u16::from(high)) / 2;
    let mut specs = Vec::new();
    for i in 0..(seconds / 0.15) as usize {
        let (on, step) = (i as f64 * 150.0, [0, 2, 4, 5, 7, 5, 4, 2][i % 8]);
        specs.push(format!("{}@{on}-{}", mid + step, on + 180.0));
    }
    for s in 0..seconds as usize {
        let on = s as f64 * 1000.0 + 500.0;
        for interval in [0, 4, 7] {
            specs.push(format!(
                "{}@{on}-{}:90",
                mid.saturating_sub(12) + interval,
                on + 800.0
            ));
        }
    }
    let input = parse_notes(specs.iter().map(String::as_str))?;
    let mut engine = Engine::default();
    engine.set_bank(Some(Box::new(bank)));
    engine.set_fx(instrument.fx.processor(RATE as f32, MAX_BLOCK));
    let started = std::time::Instant::now();
    install_scripts(&mut engine, &instrument);
    let init_ms = started.elapsed().as_secs_f64() * 1e3;
    let slots = engine.script().map_or(0, |rt| rt.slots());
    let scripted = time_blocks(&mut engine, &input);
    let underruns = engine.underruns();
    engine.set_script(None);
    engine.reset(RATE);
    let direct = time_blocks(&mut engine, &input);
    let fresh = || {
        let (script, errors) = load_scripts(&instrument, instrument.script_state.clone(), RATE);
        ensure!(errors.is_empty(), "Scripts failed to load: {errors:?}");
        script.context("Instrument has no scripts")
    };
    let runtime = time_runtime(fresh, &input)?;
    let block_ns = MAX_BLOCK as f64 / RATE * 1e9;
    let report = |name: &str, (mean, p99, max, voices): (f64, f64, f64, usize)| {
        println!(
            "{name}: {mean:.0} ns/block mean · p99 {p99:.0} · max {max:.0} · {:.1}x real time · peak {voices} voices",
            block_ns / mean
        );
    };
    println!(
        "{} · {slots} script slots · init {init_ms:.1} ms · {} notes over {seconds} s · {underruns} underruns",
        instrument.name,
        input.len() / 2
    );
    report("runtime only (best of 5 runs)", runtime);
    report("scripts + engine             ", scripted);
    report("no scripts, every group plays", direct);
    Ok(())
}

/// Streaming under real-time pacing: `notes` held notes across the key
/// range, one replaced every `1 s / notes`, so new streams keep starting.
/// Blocks are rendered at their wall-clock deadline (48 kHz, 128 frames) and
/// never wait for the disk; a voice block missing streamed data is an underrun.
/// Scripts run, so articulation scripts pick the groups that sound.
fn bench_stream(path: &Path, notes: usize, seconds: f64) -> Result<()> {
    let instrument = import::read(path)?;
    let bank = Bank::load(&instrument)?;
    let low = bank.zones().iter().map(|z| z.low_key).min().context("Instrument has no playable zones")?;
    let high = bank.zones().iter().map(|z| z.high_key).max().unwrap_or(low);
    let preload = bank.preload;
    let mut engine = Engine::default();
    engine.set_bank(Some(Box::new(bank)));
    install_scripts(&mut engine, &instrument);
    let keys: Vec<u8> = (low..=high).collect();
    let (mut left, mut right) = ([0f32; MAX_BLOCK], [0f32; MAX_BLOCK]);
    let block = std::time::Duration::from_secs_f64(MAX_BLOCK as f64 / RATE);
    let every = (RATE / notes as f64) as u64;
    let blocks = (seconds * RATE) as u64 / MAX_BLOCK as u64;
    let (mut held, mut started, mut voice_blocks, mut late, mut peak) =
        (std::collections::VecDeque::new(), 0, 0, 0, 0);
    let start = std::time::Instant::now();
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
        engine.render(&mut left, &mut right);
        voice_blocks += engine.active_voices();
        peak = peak.max(engine.active_voices());
        let deadline = start + block * (b + 1) as u32;
        match deadline.checked_duration_since(std::time::Instant::now()) {
            Some(wait) => std::thread::sleep(wait),
            None => late += 1,
        }
    }
    let underruns = engine.underruns();
    println!(
        "{}: preload {preload} · {started} notes over {seconds} s ({notes} held) · {} voices mean, {peak} peak · {underruns} underruns ({:.3}%) · {late} late blocks",
        instrument.name,
        voice_blocks / blocks as usize,
        underruns as f64 * 100.0 / voice_blocks.max(1) as f64
    );
    Ok(())
}

/// Engine stand-in that plays nothing, so timing isolates the runtime.
struct NullEngine(u32);

impl kontakto::ksp::KspEngine for NullEngine {
    fn play_note(
        &mut self,
        _: u32,
        _: &kontakto::ksp::NoteSpec<'_>,
    ) -> Option<kontakto::ksp::EventId> {
        self.0 += 1;
        Some(kontakto::ksp::EventId(self.0))
    }
    fn note_off(&mut self, _: u32, _: kontakto::ksp::EventId, _: &kontakto::ksp::NoteSpec<'_>) {}
    fn fade(&mut self, _: u32, _: kontakto::ksp::EventId, _: kontakto::ksp::Fade) {}
    fn set_par(&mut self, _: u32, _: kontakto::ksp::EventId, _: kontakto::ksp::VoicePar, _: i32) {}
    fn controller(&mut self, _: u32, _: u8, _: i32) {}
    fn group_count(&self) -> usize {
        0
    }
    fn group_name(&self, _: usize) -> &str {
        ""
    }
    fn sample_rate(&self) -> f64 {
        RATE
    }
    fn set_engine_par(&mut self, _: u32, _: kontakto::ksp::EnginePar, _: i32) -> bool {
        false
    }
    fn engine_par(&self, _: kontakto::ksp::EnginePar) -> Option<i32> {
        None
    }
}

/// Like [`time_blocks`], for the runtime alone against a [`NullEngine`]. The
/// runtime is deterministic, so each block keeps its fastest of several runs:
/// preemption by other processes drops out and real spikes remain.
fn time_runtime(
    fresh: impl Fn() -> Result<Box<kontakto::ksp::Runtime>>,
    input: &[NoteInput],
) -> Result<(f64, f64, f64, usize)> {
    let end = input.last().map_or(0, |e| e.0) + (2.0 * RATE) as u64;
    let mut best = vec![f64::MAX; end.div_ceil(MAX_BLOCK as u64) as usize];
    for _ in 0..5 {
        let (mut rt, mut engine, mut next) = (fresh()?, NullEngine(0), 0);
        for (block, frame) in (0..end).step_by(MAX_BLOCK).enumerate() {
            let started = std::time::Instant::now();
            while let Some(&(time, on, note, velocity)) =
                input.get(next).filter(|e| e.0 < frame + MAX_BLOCK as u64)
            {
                let at = (time - frame) as u32;
                if on {
                    rt.note_on(&mut engine, at, note, velocity);
                } else {
                    rt.note_off(&mut engine, at, note);
                }
                next += 1;
            }
            rt.process(&mut engine, MAX_BLOCK as u32);
            best[block] = best[block].min(started.elapsed().as_nanos() as f64);
        }
    }
    Ok(summarize(best, 0))
}

fn summarize(mut times: Vec<f64>, voices: usize) -> (f64, f64, f64, usize) {
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    times.sort_by(f64::total_cmp);
    (
        mean,
        times[times.len() * 99 / 100],
        times[times.len() - 1],
        voices,
    )
}

/// Render `input` (quantized to 128-frame blocks) plus a 2 s tail, timing
/// each block with its MIDI input: mean, p99 and max ns, and peak voices.
fn time_blocks(engine: &mut Engine, input: &[NoteInput]) -> (f64, f64, f64, usize) {
    let (mut left, mut right) = ([0f32; MAX_BLOCK], [0f32; MAX_BLOCK]);
    let end = input.last().map_or(0, |e| e.0) + (2.0 * RATE) as u64;
    let (mut times, mut voices, mut next) = (Vec::new(), 0, 0);
    for frame in (0..end).step_by(MAX_BLOCK) {
        let started = std::time::Instant::now();
        while let Some(event) = input.get(next).filter(|e| e.0 < frame + MAX_BLOCK as u64) {
            play(engine, event);
            next += 1;
        }
        engine.render(&mut left, &mut right);
        times.push(started.elapsed().as_nanos() as f64);
        voices = voices.max(engine.active_voices());
    }
    summarize(times, voices)
}
/// Group modulation, envelopes and zone crossfade summary; no sample data or paths.
fn inspect_mods(path: &Path) -> Result<serde_json::Value> {
    let instrument = import::read(path)?;
    let groups: Vec<_> = instrument
        .groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            let zones: Vec<_> = instrument.zones.iter().filter(|z| z.group == index).collect();
            let crossfaded = zones
                .iter()
                .filter(|z| {
                    [z.fade_low_velocity, z.fade_high_velocity, z.fade_low_key, z.fade_high_key]
                        .iter()
                        .any(|fade| *fade != 0)
                })
                .count();
            serde_json::json!({
                "name": group.name,
                "gain": group.gain,
                "tune": group.tune,
                "voice_group": group.voice_group,
                "interp_quality": group.interp_quality,
                "volume_env": group.volume_env,
                "velocity_to_volume": group.velocity_to_volume(),
                "pitch_bend_range": group.pitch_bend_range(),
                "cc_volume": group.cc_volume().map(|(cc, _)| cc),
                "mods": group.mods,
                "modulators": group.modulators,
                "zones": zones.len(),
                "crossfaded_zones": crossfaded,
                "start_mod_zones": zones.iter().filter(|z| z.start_mod.is_some_and(|frames| frames != 0)).count(),
            })
        })
        .collect();
    Ok(serde_json::json!({
        "name": instrument.name,
        "groups": groups,
        "warnings": instrument.warnings,
        "kontakt_behavior_verified": false,
    }))
}

/// Run an instrument's scripts against a logging engine: `on init`, then each note as
/// `note[@on_ms[-off_ms]][:velocity]`. Without times, notes are 400 ms apart and overlap
/// by 100 ms (legato). Prints the engine calls as JSON; script source is never printed.
fn ksp_run(path:&Path,notes:&[String])->Result<()> {
    use kontakto::ksp::{EngineCall,LogEngine,Runtime};
    const BLOCK:u32=128;
    let instrument=import::read(path)?;
    let mut engine=LogEngine::new(instrument.groups.iter().map(|g|g.name.clone()).collect(),RATE);
    engine.modulators=instrument.groups.iter().map(|g|g.modulators.iter().map(|m|(m.name.clone(),m.targets.clone())).collect()).collect();
    let start=std::time::Instant::now();
    let (mut rt,init_errors)=Runtime::with_scripts(&instrument.scripts,&mut engine,8,instrument.script_state.clone());
    let init_ms=start.elapsed().as_secs_f64()*1e3;
    let init_engine_pars=engine.calls.iter().filter(|c|matches!(c,EngineCall::SetEnginePar{..})).count();
    engine.calls.clear();
    let input=parse_notes(notes.iter().map(String::as_str))?;
    let end=input.last().map_or(0,|e|e.0)+(2.0*RATE) as u64;
    let mut next=input.iter().peekable();
    while rt.now()<end {
        engine.block_start=rt.now();
        while let Some(&&(time,on,note,velocity))=next.peek().filter(|e|e.0<rt.now()+u64::from(BLOCK)) {
            let at=(time-rt.now()) as u32;
            if on {rt.note_on(&mut engine,at,note,velocity);} else {rt.note_off(&mut engine,at,note);}
            next.next();
        }
        rt.process(&mut engine,BLOCK);
    }
    let report=serde_json::json!({
        "instrument":instrument.name,"groups":instrument.groups.len(),"slots":rt.slots(),
        "init_errors":init_errors,"init_ms":(init_ms*10.0).round()/10.0,"init_engine_pars":init_engine_pars,
        "diagnostics":rt.diagnostics(),"sample_rate":RATE,"calls":engine.calls,
    });
    println!("{}",serde_json::to_string_pretty(&report)?);
    Ok(())
}
