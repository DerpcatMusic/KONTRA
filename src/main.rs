use anyhow::{Context,Result,ensure};
use std::path::Path;
use kontakto::{import,engine::{Bank,Engine}};
fn main() -> Result<()> {
 let args:Vec<_>=std::env::args().collect();
 match args.get(1).map(String::as_str) {
  Some("scan") => {for p in import::presets(Path::new(args.get(2).map(String::as_str).unwrap_or(import::LIBRARY_ROOT)))? {println!("{}",p.display());}},
  Some("inspect") => {let p=args.get(2).context("inspect requires an NKI path")?; println!("{}",serde_json::to_string_pretty(&import::read(Path::new(p))?)?);},
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
  Some("render") => {
    let instrument=import::read(Path::new(args.get(2).context("render requires an NKI path")?))?;
    let output=args.get(3).context("render requires an output WAV path")?;
    ensure!(!Path::new(output).exists(),"Output already exists; choose a new output path");
    let group=args.get(4).map(|s|s.parse()).transpose()?.or_else(||instrument.first_playable_group()).context("No complete playable group: inspect the missing sample report")?;
    let bank=Bank::load(&instrument,group)?;
    let note=args.get(5).map(|s|s.parse::<u8>()).transpose()?.unwrap_or(bank.zones[0].root);
    ensure!(note<128,"MIDI note must be 0..127");
    let velocity=args.get(6).map(|s|s.parse::<u8>()).transpose()?.unwrap_or_else(|| bank.zones.iter().find(|z|note>=z.low_key && note<=z.high_key).map(|z|((z.low_velocity as u16+z.high_velocity as u16)/2).max(1) as u8).unwrap_or(100));
    let mut engine=Engine::default();engine.bank=Some(Box::new(bank));
    let spec=hound::WavSpec{channels:2,sample_rate:48000,bits_per_sample:32,sample_format:hound::SampleFormat::Float};
    let mut writer=hound::WavWriter::create(output,spec)?;
    engine.note_on(0,note,velocity);let mut peak=0f32;let mut square=0f64;
    for n in 0..(48000*4) {
       if n==48000*2 {engine.note_off(0,note);}
       for sample in engine.frame() {let sample=sample*0.25;ensure!(sample.is_finite(),"Nonfinite rendered audio");peak=peak.max(sample.abs());square+=sample as f64*sample as f64;writer.write_sample(sample)?;}
    }
    writer.finalize()?;
    ensure!(peak>0.00001,"Rendered silence; chosen key/velocity has no audible zone in this group");
    println!("{} · group {} ({}) · note {note} · peak {peak:.6} · RMS {:.6}",instrument.name,group,instrument.groups[group].name,(square/(48000.0*4.0*2.0)).sqrt());
    for warning in instrument.warnings {eprintln!("Compatibility: {warning}");}
  },
  Some("ksp-run") => ksp_run(Path::new(args.get(2).context("ksp-run requires an NKI path")?),&args[3..])?,
  _=> println!("kontakto scan [folder]\nkontakto inspect <instrument.nki>\nkontakto inspect-multi <multi.nkm>\nkontakto ui <instrument.nki>\nkontakto audit [folder]\nkontakto audit-structure [folder]\nkontakto audit-scripts [folder]\nkontakto audit-archives [folder]\nkontakto render <instrument.nki> <output.wav> [group=0] [note=first root] [velocity=zone midpoint]\nkontakto ksp-run <instrument.nki> [note[@on_ms[-off_ms]][:velocity]...]"),
 }
 Ok(())
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
