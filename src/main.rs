use anyhow::{Context,Result,ensure};
use std::path::Path;
use kontakto::{audio::Sample,engine::{Bank,Engine,MAX_BLOCK,MAX_VOICES,NoteEvent},import::{self,Group,Loop,Zone}};
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
  Some("bench") => bench(args.get(2).map(|s|s.parse()).transpose()?.unwrap_or(1000))?,
  _=> println!("kontakto scan [folder]\nkontakto inspect <instrument.nki>\nkontakto inspect-multi <multi.nkm>\nkontakto inspect-mods <instrument.nki>\nkontakto inspect-fx <instrument.nki>\nkontakto audit-fx [folder]\nkontakto ui <instrument.nki>\nkontakto audit [folder]\nkontakto audit-structure [folder]\nkontakto audit-scripts [folder]\nkontakto audit-archives [folder]\nkontakto render <instrument.nki> <output.wav> [group=all] [note=first root] [velocity=zone midpoint]\nkontakto ksp-run <instrument.nki> [note[@on_ms[-off_ms]][:velocity]...]\nkontakto bench [voices=1000]"),
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

/// Render a note through every playable group (or one group) to a WAV file.
fn render(args: &[String]) -> Result<()> {
  let instrument = import::read(Path::new(args.first().context("render requires an NKI path")?))?;
  let output = args.get(1).context("render requires an output WAV path")?;
  ensure!(!Path::new(output).exists(), "Output already exists; choose a new output path");
  let group: Option<usize> = args.get(2).filter(|s| *s != "all").map(|s| s.parse()).transpose()?;
  let bank = Bank::load(&instrument)?;
  if bank.skipped_zones > 0 {
    eprintln!("Skipped {} zones: {}", bank.skipped_zones, bank.issues.join("; "));
  }
  let first = bank.zones().iter().find(|z| group.is_none_or(|g| z.group == g)).context("Group has no playable zones")?;
  let note = args.get(3).map(|s| s.parse::<u8>()).transpose()?.unwrap_or(first.root);
  ensure!(note < 128, "MIDI note must be 0..127");
  let velocity = args.get(4).map(|s| s.parse::<u8>()).transpose()?.unwrap_or_else(|| {
    let zone = bank.zones().iter().find(|z| (z.low_key..=z.high_key).contains(&note));
    zone.map_or(100, |z| ((u16::from(z.low_velocity) + u16::from(z.high_velocity)) / 2).max(1) as u8)
  });
  let streamed = bank.streamed_samples();
  let mut engine = Engine::default();
  engine.blocking_streams = true;
  engine.set_bank(Some(Box::new(bank)));
  if let Some(g) = group {
    engine.set_all_groups_allowed(false);
    engine.set_group_allowed(g, true);
  }
  let spec = hound::WavSpec { channels: 2, sample_rate: 48000, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
  let mut writer = hound::WavWriter::create(output, spec)?;
  engine.note_on(0, note, velocity);
  let (mut peak, mut square) = (0f32, 0f64);
  let (mut left, mut right) = ([0f32; MAX_BLOCK], [0f32; MAX_BLOCK]);
  let blocks = 48000 * 4 / MAX_BLOCK;
  for block in 0..blocks {
    if block == blocks / 2 {
      engine.note_off(0, note);
    }
    engine.render(&mut left, &mut right);
    for (l, r) in left.iter().zip(&right) {
      for sample in [l * 0.25, r * 0.25] {
        ensure!(sample.is_finite(), "Nonfinite rendered audio");
        peak = peak.max(sample.abs());
        square += f64::from(sample) * f64::from(sample);
        writer.write_sample(sample)?;
      }
    }
  }
  writer.finalize()?;
  ensure!(peak > 0.00001, "Rendered silence; chosen key/velocity has no audible zone");
  let groups = group.map_or_else(|| "all groups".to_string(), |g| format!("group {g} ({})", instrument.groups[g].name));
  let rms = (square / (blocks * MAX_BLOCK * 2) as f64).sqrt();
  println!("{} · {groups} · note {note} · peak {peak:.6} · RMS {rms:.6} · {streamed} streamed samples · {} underruns", instrument.name, engine.underruns());
  for warning in instrument.warnings {
    eprintln!("Compatibility: {warning}");
  }
  Ok(())
}

/// Voices one core renders in real time: 48 kHz, 128-frame blocks, stereo,
/// pitched looping voices with loop crossfades.
fn bench(voices: usize) -> Result<()> {
  ensure!((1..=MAX_VOICES).contains(&voices), "voices must be 1..={MAX_VOICES}");
  let frames: Vec<[f32; 2]> = (0..96000).map(|i| { let x = (i as f32 * 0.013).sin(); [x, x * 0.7] }).collect();
  let group = Group { name: "bench".into(), ..Group::default() };
  let zone = Zone { low_velocity: 1, loop_range: Some(Loop { start: 20000, end: 90000, until_release: false, crossfade: 2000 }), ..Zone::default() };
  let mut bank = Bank::from_samples(vec![group], vec![zone], vec![(Default::default(), Sample { rate: 44100, frames })])?;
  bank.set_polyphony(MAX_VOICES);
  let mut engine = Engine::default();
  engine.set_bank(Some(Box::new(bank)));
  for i in 0..voices {
    let mut event = NoteEvent::new((i % 16) as u8, 36 + (i % 48) as u8, 100);
    event.tune = (i % 7) as f64 * 0.013;
    engine.start_event(&event);
  }
  ensure!(engine.active_voices() == voices, "only {} voices started", engine.active_voices());
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
  ensure!(engine.active_voices() == voices && checksum.is_finite(), "voices ended during the benchmark");
  let realtime = seconds / cpu;
  println!("{voices} voices · {seconds} s audio in {cpu:.3} s (best of 7) · {realtime:.1}x real time · {:.0} voices per core", voices as f64 * realtime);
  Ok(())
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
    const RATE:f64=48_000.0;const BLOCK:u32=128;
    let instrument=import::read(path)?;
    let mut engine=LogEngine::new(instrument.groups.iter().map(|g|g.name.clone()).collect(),RATE);
    let start=std::time::Instant::now();
    let (mut rt,init_errors)=Runtime::with_scripts(&instrument.scripts,&mut engine,8,instrument.script_state.clone());
    let init_ms=start.elapsed().as_secs_f64()*1e3;
    let init_engine_pars=engine.calls.iter().filter(|c|matches!(c,EngineCall::SetEnginePar{..})).count();
    engine.calls.clear();
    let ms=|s:&str|s.parse::<f64>().map(|ms|(ms*RATE/1e3) as u64);
    let mut input=Vec::new();
    for (i,spec) in notes.iter().enumerate() {
        let (spec,velocity)=spec.split_once(':').map_or((spec.as_str(),Ok(100)),|(s,v)|(s,v.parse::<u8>()));
        let (note,times)=spec.split_once('@').map_or((spec,None),|(n,t)|(n,Some(t)));
        let note:u8=note.parse().with_context(||format!("Bad note {spec}"))?;
        ensure!(note<128,"MIDI note must be 0..127");
        let (on,off)=match times.map(|t|t.split_once('-').map_or((t,None),|(a,b)|(a,Some(b)))) {
            Some((on,off))=>{let on=ms(on)?;(on,off.map(ms).transpose()?.unwrap_or(on+(0.4*RATE) as u64))},
            None=>{let on=(i as f64*0.4*RATE) as u64;(on,on+(0.5*RATE) as u64)},
        };
        ensure!(off>on,"Note-off must follow note-on");
        input.push((on,1,note,velocity?));input.push((off,0,note,0));
    }
    input.sort();
    let end=input.last().map_or(0,|e|e.0)+(2.0*RATE) as u64;
    let mut next=input.iter().peekable();
    while rt.now()<end {
        engine.block_start=rt.now();
        while let Some(&&(time,on,note,velocity))=next.peek().filter(|e|e.0<rt.now()+u64::from(BLOCK)) {
            let at=(time-rt.now()) as u32;
            if on==1 {rt.note_on(&mut engine,at,note,velocity);} else {rt.note_off(&mut engine,at,note);}
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
