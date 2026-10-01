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
  Some("voices") => voices(Path::new(args.get(2).context("voices requires an NKI path")?), args.get(3).map_or("60@0-3000,64@0-3000,67@0-3000", String::as_str))?,
  Some("ksp-run") => ksp_run(Path::new(args.get(2).context("ksp-run requires an NKI path")?),&args[3..])?,
  Some("bench") => bench(&args[2..])?,
  Some("bench-load") => for p in &args[2..] {bench_load(Path::new(p))?},
  Some("audit-libraries") => audit_libraries(&args[2..])?,
  #[cfg(feature = "plugin")]
  Some("audit-latency") => for p in &args[2..] {match kontakto::timing::audit(Path::new(p)) {Ok(v)=>println!("{}",serde_json::to_string(&v)?),Err(e)=>eprintln!("{p}: {e:#}")}},
  Some("audit-patch") => audit_patch(Path::new(args.get(2).context("audit-patch requires an NKI path")?))?,
  Some("bench-stream") => bench_stream(Path::new(args.get(2).context("bench-stream requires an NKI path")?),args.get(3).map(|s|s.parse()).transpose()?.unwrap_or(64),args.get(4).map(|s|s.parse()).transpose()?.unwrap_or(10.0))?,
  #[cfg(feature = "plugin")]
  Some("bench-host") => kontakto::bench_host(&args[4..],args.get(2).context("bench-host <seconds> <notes> <instrument.nki>...")?.parse()?,args.get(3).context("bench-host <seconds> <notes> <instrument.nki>...")?.parse()?)?,
  #[cfg(not(feature = "plugin"))]
  Some("bench-host" | "audit-latency") => anyhow::bail!("This command requires the plugin feature"),
  Some("bench-script") => bench_script(Path::new(args.get(2).context("bench-script requires an NKI path")?),args.get(3).map(|s|s.parse()).transpose()?.unwrap_or(20.0))?,
  _=> println!("kontakto scan [folder]\nkontakto inspect <instrument.nki>\nkontakto inspect-multi <multi.nkm>\nkontakto inspect-mods <instrument.nki>\nkontakto inspect-fx <instrument.nki>\nkontakto audit-fx [folder]\nkontakto ui <instrument.nki>\nkontakto audit [folder]\nkontakto audit-structure [folder]\nkontakto audit-scripts [folder]\nkontakto audit-archives [folder]\nkontakto render [--dry] [--no-script] [--realtime] [--notes 60@0-600,62@500-1100:90] [--cc 11@0:40,11@500:127] <instrument.nki> <output.wav> [group=all] [note=first root] [velocity=zone midpoint]\nkontakto ksp-run <instrument.nki> [note[@on_ms[-off_ms]][:velocity]...]\nkontakto bench [voices=1000] [bits=24|16|32] [layers=1] [--root] [--no-lanes]\nkontakto bench-script <instrument.nki> [seconds=20]\nkontakto audit-libraries [root] [--out audits/LIBRARIES.md]\nkontakto bench-load <instrument.nki>...\nkontakto bench-stream <instrument.nki> [notes=64] [seconds=10]"),
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

fn seconds_of(frames: u64) -> f64 {
    frames as f64 / RATE
}

/// Scripts, as the plugin loads them.
type Scripts = (Option<Box<kontakto::ksp::Runtime>>, Vec<String>);

/// Load the scripts, then the bank, as the plugin does: the bank keeps
/// resident the start offsets the scripts' `on init` controllers select.
fn load(instrument: &import::Instrument) -> Result<(Bank, Scripts)> {
    let scripts = load_scripts(instrument, instrument.script_state.clone(), RATE);
    let controllers = scripts.0.as_deref().map_or(&[][..], |rt| &rt.init_controllers[..]);
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

/// Render notes through the instrument scripts (`--no-script` plays MIDI
/// directly), every playable group (or one group, unscripted) and the
/// instrument effects (`--dry` skips them) to a WAV file, with the tail.
/// `--notes 60@0-600,62@500-1100:90` plays a sequence (times in ms); otherwise
/// one note is held for 2 s. `--cc 11@0:40,11@500:127` sends controllers on
/// channel 1 (times in ms), before notes at the same time.
fn render(args: &[String]) -> Result<()> {
    let flag = |name: &str| args.iter().any(|a| a == name);
    let (dry, no_script, realtime) = (flag("--dry"), flag("--no-script"), flag("--realtime"));
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
    let (bank, scripts) = load(&instrument)?;
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
    // `--realtime` plays like the plugin: blocks at their wall-clock deadline,
    // late disk data as silence.
    engine.blocking_streams = !realtime;
    engine.set_bank(Some(Box::new(bank)));
    if !dry {
        engine.set_fx(instrument.fx.processor(engine.rate() as f32, MAX_BLOCK));
    }
    if let Some(g) = group {
        engine.set_all_groups_allowed(false);
        engine.set_group_allowed(g, true);
    }
    if !no_script {
        install_scripts(&mut engine, scripts);
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
    let mut pace = kontakto::engine::Pace::start();
    loop {
        if realtime {
            pace.until(std::time::Duration::from_secs_f64(seconds_of(frame)));
        }
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
        "{} · {groups} · {played} · {scripts} · {fx} · {:.2} s · peak {peak:.6} · RMS {rms:.6} · max step {jump:.6} at {:.3} s · sound until {:.2} s (last note-off {:.2} s) · {tail} · {streamed} streamed samples · {} underruns · {} dropped script commands",
        instrument.name,
        seconds(frame),
        seconds(jump_at),
        seconds(last_audible),
        seconds(last_off),
        engine.underruns(),
        engine.dropped_commands()
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

/// Which voices a chord keeps alive, through the scripts and effects as the
/// plugin plays it: per group, how many, how loud (channel gain times
/// envelope), how many are inaudible (below -96 dB) and how many stream.
fn voices(path: &Path, notes: &str) -> Result<()> {
    let instrument = import::read(path)?;
    let (bank, scripts) = load(&instrument)?;
    let mut engine = Engine::default();
    engine.blocking_streams = true;
    engine.set_bank(Some(Box::new(bank)));
    engine.set_fx(instrument.fx.processor(engine.rate() as f32, MAX_BLOCK));
    install_scripts(&mut engine, scripts);
    let input = parse_notes(notes.split(','))?;
    let last = input.last().map_or(0, |e| e.0);
    let (mut left, mut right) = ([0f32; MAX_BLOCK], [0f32; MAX_BLOCK]);
    let (mut frame, mut next) = (0u64, 0);
    let mut reports = [0.1, 0.5, 1.0, 2.0].map(|s| (s * RATE) as u64).to_vec();
    reports.extend([0.2, 1.0, 3.0].map(|s| last + (s * RATE) as u64));
    for at in reports {
        while frame < at {
            while let Some(event) = input.get(next).filter(|e| e.0 <= frame) {
                play(&mut engine, event);
                next += 1;
            }
            let len = input
                .get(next)
                .map_or(MAX_BLOCK as u64, |e| e.0 - frame)
                .min(at - frame)
                .min(MAX_BLOCK as u64) as usize;
            engine.render(&mut left[..len], &mut right[..len]);
            frame += len as u64;
        }
        let census = engine.voice_census();
        let level = |v: &kontakto::engine::VoiceInfo| v.gain * v.envelope;
        let silent = census.iter().filter(|v| level(v) < 1.6e-5).count();
        println!(
            "{:.2} s: {} voices · {} below -96 dB · {} streaming",
            seconds_of(frame),
            census.len(),
            silent,
            census.iter().filter(|v| v.streams).count()
        );
        let mut groups: Vec<u32> = census.iter().map(|v| v.group).collect();
        groups.sort_unstable();
        groups.dedup();
        for g in groups {
            let of: Vec<_> = census.iter().filter(|v| v.group == g).collect();
            let loudest = of.iter().map(|v| level(v)).fold(0f32, f32::max);
            println!(
                "  {:>3} × group {g} {:?}: loudest {:.1} dB · {} below -96 dB · {} released · {} release-triggered · gains 0: {}",
                of.len(),
                instrument.groups[g as usize].name,
                20. * loudest.max(1e-12).log10(),
                of.iter().filter(|v| level(v) < 1.6e-5).count(),
                of.iter().filter(|v| v.released).count(),
                of.iter().filter(|v| v.release_trigger).count(),
                of.iter().filter(|v| v.gain == 0.).count(),
            );
        }
    }
    Ok(())
}

/// Voices one core renders in real time: 48 kHz, 128-frame blocks, stereo,
/// pitched looping voices with loop crossfades, playing a sample of `bits`
/// resolution (resident as 16-bit, 24-bit or f32). Every note plays `layers`
/// zones, one per group and sample, as mic positions and stacked sections
/// do; more voices than one engine holds spread over several, as a rack's
/// parts. `--root` plays every note on its root at 48 kHz (no resampling).
fn bench(args: &[String]) -> Result<()> {
    let root = args.iter().any(|a| a == "--root");
    let lanes = !args.iter().any(|a| a == "--no-lanes");
    let mut args = args.iter().filter(|a| !a.starts_with("--"));
    let mut next = |default: usize| -> Result<usize> { Ok(args.next().map(|s| s.parse()).transpose()?.unwrap_or(default)) };
    let (voices, bits, layers) = (next(1000)?, next(24)? as i32, next(1)?.max(1));
    ensure!(voices >= layers && voices % layers == 0, "voices must be a multiple of layers");
    let sample = |layer: usize| {
        let frames: Vec<[f32; 2]> = (0..96000)
            .map(|i| {
                let scale = 2f32.powi(bits - 1);
                let x = (i as f32 * (0.013 + 0.001 * layer as f32)).sin();
                [x * 0.9, x * 0.6].map(|v| (v * scale).round() / scale)
            })
            .collect();
        (std::path::PathBuf::from(format!("layer {layer}")), Sample { rate: if root { 48000 } else { 44100 }, frames })
    };
    let groups: Vec<Group> = (0..layers).map(|l| Group { name: format!("bench {l}"), ..Group::default() }).collect();
    let zones: Vec<Zone> = (0..layers)
        .map(|l| Zone {
            group: l,
            sample: format!("layer {l}").into(),
            low_velocity: 1,
            loop_range: Some(Loop { start: 20000, end: 90000, until_release: false, crossfade: 2000 }),
            ..Zone::default()
        })
        .collect();
    let mut engines: Vec<Engine> = Vec::new();
    let notes = voices / layers;
    let per_engine = MAX_VOICES / layers * layers;
    for i in 0..notes {
        if i * layers % per_engine == 0 {
            let mut bank = Bank::from_samples(groups.clone(), zones.clone(), (0..layers).map(sample).collect())?;
            bank.set_polyphony(MAX_VOICES);
            let mut engine = Engine::default();
            engine.set_bank(Some(Box::new(bank)));
            engine.set_lanes(lanes);
            engines.push(engine);
        }
        let key = if root { 60 } else { 36 + (i % 48) as u8 };
        let mut event = NoteEvent::new((i % 16) as u8, key, 100);
        if !root {
            event.tune = (i % 7) as f64 * 0.013;
        }
        engines.last_mut().unwrap().start_event(&event);
    }
    let started: usize = engines.iter().map(Engine::active_voices).sum();
    ensure!(started == voices, "only {started} voices started");
    let (mut left, mut right) = ([0f32; MAX_BLOCK], [0f32; MAX_BLOCK]);
    // Best of several runs: the minimum is the least disturbed by other load.
    let seconds = 2.0;
    let blocks = (48000.0 * seconds / MAX_BLOCK as f64) as usize;
    let mut checksum = 0f32;
    let mut cpu = f64::MAX;
    for _ in 0..7 {
        let started = std::time::Instant::now();
        for _ in 0..blocks {
            for engine in &mut engines {
                engine.render(&mut left, &mut right);
                checksum += left[0] + right[MAX_BLOCK - 1];
            }
        }
        cpu = cpu.min(started.elapsed().as_secs_f64());
    }
    let playing: usize = engines.iter().map(Engine::active_voices).sum();
    ensure!(playing == voices && checksum.is_finite(), "voices ended during the benchmark");
    let realtime = seconds / cpu;
    println!(
        "{voices} voices ({layers} layers, {} engines) · {seconds} s audio in {cpu:.3} s (best of 7) · {realtime:.1}x real time · {:.1}% of a core · {:.0} voices per core",
        engines.len(),
        100.0 / realtime,
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
    let scripts = load_scripts(&instrument, instrument.script_state.clone(), RATE);
    let init_ms = started.elapsed().as_secs_f64() * 1e3;
    let controllers = scripts.0.as_deref().map_or(&[][..], |rt| &rt.init_controllers[..]);
    let started = std::time::Instant::now();
    let bank = match Bank::load_counting(
        &instrument,
        kontakto::engine::MEMORY_LIMIT,
        kontakto::engine::Streaming::Auto,
        controllers,
        &Default::default(),
    ) {
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
    if let Some(warning) = &bank.warning {
        eprintln!("{name}: {warning}");
    }
    let mut engine = Engine::default();
    engine.set_bank(Some(Box::new(bank)));
    install_scripts(&mut engine, scripts);
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
    let started = std::time::Instant::now();
    let (bank, scripts) = load(&instrument)?;
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
    install_scripts(&mut engine, scripts);
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
    let (bank, scripts) = load(&instrument)?;
    let low = bank.zones().iter().map(|z| z.low_key).min().context("Instrument has no playable zones")?;
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
    let (mut pace, render_start, process_start) = (kontakto::engine::Pace::start(), cpu_time(THREAD), cpu_time(PROCESS));
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
                "flex_env": group.flex_env,
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
    // Writes per parameter name: which engine parameters the scripts drive.
    let mut init_engine_pars=std::collections::BTreeMap::<&str,usize>::new();
    for call in &engine.calls {if let EngineCall::SetEnginePar{par,..}=call {*init_engine_pars.entry(par).or_default()+=1;}}
    let init_engine_pars=serde_json::json!(init_engine_pars);
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

/// A library folder: a Kontakt library (`Instruments` or a `.nicnt`) or,
/// for anything else, why it cannot be audited.
enum Library {
    Kontakt(std::path::PathBuf),
    Other(std::path::PathBuf, String),
}

/// Libraries under `root`: folders holding a Kontakt library, and folders
/// of other formats named by their commonest file types. Folders without a
/// library recurse one level (`Libraries/Kontakt/<library>`).
fn libraries(root: &Path, depth: usize, out: &mut Vec<Library>) -> Result<()> {
    let mut dirs: Vec<_> = std::fs::read_dir(root)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let names: Vec<_> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        let kontakt = names.iter().any(|p| {
            p.file_name().is_some_and(|n| n == "Instruments")
                || p.extension().is_some_and(|x| x.eq_ignore_ascii_case("nicnt"))
        });
        if kontakt {
            out.push(Library::Kontakt(dir));
        } else if depth == 0 && names.iter().any(|p| p.is_dir()) {
            libraries(&dir, depth + 1, out)?;
        } else {
            let mut kinds = std::collections::BTreeMap::<String, usize>::new();
            for e in walkdir::WalkDir::new(&dir).max_depth(4).into_iter().flatten() {
                if let Some(x) = e.path().extension().filter(|_| e.file_type().is_file()) {
                    *kinds.entry(x.to_string_lossy().to_lowercase()).or_default() += 1;
                }
            }
            let mut kinds: Vec<_> = kinds.into_iter().collect();
            kinds.sort_by_key(|k| std::cmp::Reverse(k.1));
            let format = |x: &str| match x {
                "ascf" | "cf" => "Ample Sound",
                "ufs" | "r2ruvi" => "UVI",
                "pak" => "IK Multimedia",
                "s20" | "obw" => "Toontrack",
                _ => "",
            };
            let vendor = kinds.iter().map(|(x, _)| format(x)).find(|v| !v.is_empty());
            let files: Vec<_> = kinds.iter().take(3).map(|(x, n)| format!("{n} .{x}")).collect();
            let reason = format!(
                "not a Kontakt library{} ({}): no NKI instruments to load",
                vendor.map_or(String::new(), |v| format!(": {v} format")),
                files.join(", ")
            );
            out.push(Library::Other(dir, reason));
        }
    }
    Ok(())
}

/// One command for every library under `root`: load its heaviest instrument
/// (most available zones, then most zones) through the engine in a child
/// process, play notes at real-time pace and tabulate load time, memory,
/// zones, script errors, underruns and loudness. Writes `out` as Markdown
/// with names and numbers only.
fn audit_libraries(args: &[String]) -> Result<()> {
    let at = args.iter().position(|a| a == "--out");
    let out = at.and_then(|i| args.get(i + 1)).map_or("audits/LIBRARIES.md", String::as_str);
    let root = args
        .iter()
        .enumerate()
        .find(|&(i, a)| !a.starts_with("--") && at.is_none_or(|o| i != o + 1))
        .map_or_else(
            || Path::new(import::LIBRARY_ROOT).parent().unwrap_or(Path::new("/")).to_path_buf(),
            |(_, a)| std::path::PathBuf::from(a),
        );
    let mut found = Vec::new();
    libraries(&root, 0, &mut found)?;
    let exe = std::env::current_exe()?;
    let mut rows = Vec::new();
    for library in found {
        let (dir, row) = match library {
            Library::Other(dir, reason) => (dir, serde_json::json!({"status": "skipped", "reason": reason})),
            Library::Kontakt(dir) => {
                let row = match heaviest(&dir) {
                    Err(e) => serde_json::json!({"status": "FAIL", "reason": format!("{e:#}")}),
                    Ok((path, candidates)) => {
                        eprintln!("Auditing {}", path.display());
                        let child = std::process::Command::new(&exe).arg("audit-patch").arg(&path).output()?;
                        let line = String::from_utf8_lossy(&child.stdout);
                        let mut row = serde_json::from_str::<serde_json::Value>(line.trim()).unwrap_or_else(|_| {
                            let err = String::from_utf8_lossy(&child.stderr);
                            let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no output");
                            serde_json::json!({"status": "FAIL", "reason": format!("child exited {}: {last}", child.status)})
                        });
                        row["patch"] = path.file_stem().map(|s| s.to_string_lossy().into_owned()).into();
                        row["candidates"] = candidates.into();
                        row
                    }
                };
                (dir, row)
            }
        };
        let mut row = row;
        row["library"] = dir.file_name().map(|s| s.to_string_lossy().into_owned()).into();
        eprintln!("{row}");
        rows.push(row);
    }
    let table = libraries_table(&rows);
    println!("{table}");
    let date = std::process::Command::new("date").arg("+%Y-%m-%d").output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let doc = format!(
        "# Library audit, {date}\n\nGenerated by `kontakto audit-libraries {}`. For every library, the NKI with the most available zones (then most zones) is loaded through the engine path the plugin uses (import, bank within the 1 GiB part budget, scripts, effects) in its own process. It then plays three single notes and a three-note chord across the mapped keys at velocity 100 with CC1 = 100 and CC11 = 127, rendered at real-time pace (48 kHz, 128-frame blocks) without waiting for the disk, so late streams count as underruns. No audio is saved. Names and numbers only.\n\nLoad = import + bank + script init. RSS is the child process after loading (peak in brackets). RMS is over the whole render. Underruns count voice blocks missing streamed data.\n\n{table}\n",
        root.display()
    );
    std::fs::write(out, doc)?;
    eprintln!("Wrote {out}");
    Ok(())
}

/// The NKI under `dir` with the most available zones, then most zones, and
/// how many instruments were compared. Imports run on every core.
fn heaviest(dir: &Path) -> Result<(std::path::PathBuf, usize)> {
    let paths: Vec<_> = import::presets(dir)?.into_iter().filter(|p| !import::is_multi(p)).collect();
    ensure!(!paths.is_empty(), "no NKI instruments");
    let next = std::sync::Mutex::new(paths.iter());
    let weights = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..std::thread::available_parallelism().map_or(4, |n| n.get()) {
            s.spawn(|| {
                // A call, so the lock is released before the import.
                let take = || next.lock().unwrap().next();
                while let Some(path) = take() {
                    if let Ok(i) = import::read(path) {
                        let weight = (i.zones.iter().filter(|z| z.available).count(), i.zones.len());
                        weights.lock().unwrap().push((weight, path));
                    }
                }
            });
        }
    });
    let weights = weights.into_inner().unwrap();
    let (_, path) = weights.iter().max().context("no NKI instrument imports")?;
    Ok(((*path).clone(), paths.len()))
}

fn libraries_table(rows: &[serde_json::Value]) -> String {
    let mut t = String::from("| Library | Heaviest patch | Zones avail/total | Samples (streamed) | Resident MiB | Preload | RSS MiB | Load ms | RMS dBFS | Underruns | Script errors | Warnings | Status |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|\n");
    let s = |v: &serde_json::Value| match v {
        serde_json::Value::Null => "-".to_string(),
        serde_json::Value::String(s) => s.replace('|', "/"),
        v => v.to_string(),
    };
    for r in rows {
        let mut status = match r["status"].as_str() {
            Some("ok") => "ok".to_string(),
            _ => format!("{}: {}", s(&r["status"]), s(&r["reason"])),
        };
        if let Some(ms) = r["stall_ms"].as_u64().filter(|&ms| ms > kontakto::engine::Pace::CATCH_UP.as_millis() as u64) {
            status += &format!(" (render thread held off {ms} ms)");
        }
        let pair = |a: &str, b: &str| if r[a].is_null() { "-".into() } else { format!("{}/{}", s(&r[a]), s(&r[b])) };
        t += &format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            s(&r["library"]),
            s(&r["patch"]),
            pair("zones_available", "zones_total"),
            if r["samples"].is_null() { "-".into() } else { format!("{} ({})", r["samples"], r["streamed"]) },
            s(&r["resident_mib"]),
            s(&r["preload"]),
            if r["rss_mib"].is_null() { "-".into() } else { format!("{} ({})", r["rss_mib"], r["rss_peak_mib"]) },
            s(&r["load_ms"]),
            s(&r["rms_db"]),
            s(&r["underruns"]),
            s(&r["script_errors"]),
            s(&r["warnings"]),
            status
        );
    }
    t
}

/// `audit-libraries`' child: load and play one instrument, one JSON line.
fn audit_patch(path: &Path) -> Result<()> {
    let started = std::time::Instant::now();
    let instrument = import::read(path)?;
    let (bank, (script, errors)) = load(&instrument)?;
    let (zones_total, zones_available) = (
        instrument.zones.len(),
        instrument.zones.iter().filter(|z| z.available).count(),
    );
    let mut keys: Vec<u8> = (0..128u8)
        .filter(|&k| bank.zones().iter().any(|z| (z.low_key..=z.high_key).contains(&k)))
        .collect();
    keys.dedup();
    ensure!(!keys.is_empty(), "no playable zones");
    let mut row = serde_json::json!({
        "zones_total": zones_total,
        "zones_available": zones_available,
        "zones_playable": bank.zones().len(),
        "samples": bank.sample_count(),
        "streamed": bank.streamed_samples(),
        "resident_mib": (bank.bytes as f64 / 1048576.0).round(),
        "preload": bank.preload,
    });
    let mut warnings = instrument.warnings.len() + usize::from(bank.warning.is_some());
    let mut engine = Engine::default();
    engine.set_bank(Some(Box::new(bank)));
    engine.set_fx(instrument.fx.processor(RATE as f32, MAX_BLOCK));
    engine.set_script(script);
    row["load_ms"] = (started.elapsed().as_millis() as u64).into();
    row["script_errors"] = errors.len().into();
    let status = |key: &str| {
        std::fs::read_to_string("/proc/self/status").ok().and_then(|s| {
            let line = s.lines().find(|l| l.starts_with(key))?;
            line.split_whitespace().nth(1)?.parse::<f64>().ok()
        }).map_or(0.0, |kib| (kib / 1024.0).round())
    };
    row["rss_mib"] = status("VmRSS:").into();
    // Three notes across the mapped keys, then them as a chord; 0.3 s tail.
    let picks: Vec<u8> = [1, 2, 3].iter().map(|q| keys[keys.len() * q / 4]).collect();
    let mut specs: Vec<String> = picks.iter().enumerate()
        .map(|(i, k)| format!("{k}@{}-{}", i * 800, i * 800 + 700)).collect();
    specs.extend(picks.iter().map(|k| format!("{k}@2400-3400")));
    let input = parse_notes(specs.iter().map(String::as_str))?;
    for (cc, value) in [(1, 100), (11, 127)] {
        engine.cc(0, cc, value);
    }
    let (mut left, mut right) = ([0f32; MAX_BLOCK], [0f32; MAX_BLOCK]);
    let blocks = (3.7 * RATE) as usize / MAX_BLOCK;
    let block = std::time::Duration::from_secs_f64(MAX_BLOCK as f64 / RATE);
    let (mut square, mut nonfinite, mut next) = (0f64, 0usize, 0);
    let mut pace = kontakto::engine::Pace::start();
    let mut stalled = std::time::Duration::ZERO;
    for b in 0..blocks {
        let frame = (b * MAX_BLOCK) as u64;
        while let Some(event) = input.get(next).filter(|e| e.0 < frame + MAX_BLOCK as u64) {
            play(&mut engine, event);
            next += 1;
        }
        engine.render(&mut left, &mut right);
        for x in left.iter().chain(&right) {
            if x.is_finite() { square += f64::from(*x) * f64::from(*x) } else { nonfinite += 1 }
        }
        stalled = stalled.max(pace.until(block * (b as u32 + 1)));
    }
    let rms = (square / (blocks * MAX_BLOCK * 2) as f64).sqrt();
    if let Some(rt) = engine.script() {
        warnings += rt.diagnostics().len();
    }
    row["rss_peak_mib"] = status("VmHWM:").into();
    row["rms_db"] = ((20.0 * rms.max(1e-12).log10() * 10.0).round() / 10.0).into();
    row["underruns"] = engine.underruns().into();
    // The machine, not the engine: a render thread held off longer than a
    // device buffer would be a host dropout. Reported, not caught up.
    row["stall_ms"] = (stalled.as_millis() as u64).into();
    row["nonfinite"] = nonfinite.into();
    row["warnings"] = warnings.into();
    let failure = if nonfinite > 0 {
        Some("non-finite output".to_string())
    } else if rms <= 1e-6 {
        Some("silent".to_string())
    } else if engine.underruns() > 0 {
        Some("underruns".to_string())
    } else {
        errors.first().map(|_| "script errors".to_string())
    };
    row["status"] = failure.as_ref().map_or("ok", |_| "FAIL").into();
    row["reason"] = failure.into();
    println!("{row}");
    Ok(())
}
