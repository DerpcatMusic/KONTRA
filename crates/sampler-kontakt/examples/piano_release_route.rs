//! Script routing only: authored source stays in RAM, PCM is synthetic.
use anyhow::{Result, anyhow};
use sampler_core::{Input, Limits, Pcm, Protocol, Runtime, ScriptInstanceId};
use sampler_ir as ir;
use std::path::Path;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod heap;

fn main() -> Result<()> {
    let path = std::env::args().nth(1).ok_or_else(|| anyhow!("NKI path required"))?;
    let mut library = sampler_kontakt::read(Path::new(&path))
        .map_err(|_| anyhow!("read failed; authored diagnostics omitted"))?;
    let slot = library.instrument.behaviors.len();
    assert_eq!(slot, 4);
    if std::env::args().nth(2).as_deref() == Some("--calls") {
        let selected=std::env::args().nth(3).and_then(|s|s.parse::<usize>().ok()).unwrap_or(0);
        let b = &library.instrument.behaviors[selected];
        let (start,len)=if selected==0 {(4870,80)} else {(300,290)};
        for (line, text) in b.source.lines().enumerate().skip(start).take(len) {
            if let Some(case)=text.trim().strip_prefix("case ") {
                if let Ok(n)=case.trim().parse::<i32>() {println!("intent line={line} case={n}");}
            }
            for name in ["play_note", "set_event_par", "set_event_par_arr", "ignore_event", "allow_group", "disallow_group", "change_vol", "fade_out", "note_off", "get_event_par"] {
                if let Some((_, tail)) = text.split_once(&format!("{name}(")) {
                    let facts = tail.split(')').next().unwrap().split(',').map(|a| {
                        let a=a.trim();
                        if let Ok(n)=a.parse::<i64>() { n.to_string() }
                        else if ["$EVENT_ID","$EVENT_NOTE","$EVENT_VELOCITY","$ALL_GROUPS","$EVENT_PAR_0","$EVENT_PAR_1","$EVENT_PAR_2","$EVENT_PAR_3","$EVENT_PAR_SOURCE","$EVENT_PAR_ALLOW_GROUP"].contains(&a) {a.to_string()}
                        else {"dynamic".into()}
                    }).collect::<Vec<_>>();
                    println!("intent line={line} builtin={name} facts={facts:?}");
                }
            }
        }
        return Ok(());
    }
    // Export routing decisions only; never persist authored source or PCM.
    let b = &mut library.instrument.behaviors[1];
    let lines = b.source.lines().collect::<Vec<_>>();
    assert_eq!(lines[362].trim(), "case 3");
    let gate = lines[546];
    assert!(gate.contains("allow_group") && gate.contains("disallow_group"));
    let condition = gate.split_once('(').unwrap().1.split_once(')').unwrap().0;
    let mut in_note = false;
    let mut traced = Vec::new();
    for (line, text) in lines.iter().enumerate() {
        let token = text.trim();
        if (145..165).contains(&line) {
            let kind = ["on note", "if", "exit", "ignore_event", "end if", "end on", "select", "case"].into_iter().find(|kind| token.starts_with(kind)).unwrap_or("other");
            println!("statement line={line} class={kind}");
        }
        if token == "on note" { in_note = true; }
        if token == "end on" { in_note = false; }
        if in_note && line < 160 && (token.strip_prefix("if").is_some_and(|rest| rest.starts_with(char::is_whitespace) || rest.starts_with('('))) {
            let condition = token.split_once('(').unwrap().1.rsplit_once(')').unwrap().0;
            traced.push(format!("%w15_early[$EVENT_ID] := {condition}"));
            let mut names = Vec::new();
            let bytes = condition.as_bytes();
            let mut at = 0;
            while at < bytes.len() {
                if bytes[at] == b'$' {
                    let start = at;
                    at += 1;
                    while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') { at += 1; }
                    let name = &condition[start..at];
                    if !names.contains(&name) { names.push(name); }
                } else { at += 1; }
            }
            assert!(names.len() <= 8);
            for (n, name) in names.iter().enumerate() {
                traced.push(format!("%w15_value{n}[$EVENT_ID] := {name}"));
                println!("early_operand line={line} index={n} native_constant={}", name.starts_with("$EVENT_") || name.starts_with("$CURRENT_") || name.starts_with("$NI_"));
            }
            let integers = condition.split(|c: char| !c.is_ascii_digit() && c != '-').filter_map(|x| x.parse::<i64>().ok()).collect::<Vec<_>>();
            println!("early_integer_tokens line={line} values={integers:?}");
            println!("early line={line} builtins={:?} constants={:?} has_compare={} has_logical={}",
                ["get_event_par", "get_event_par_arr", "event_status", "in_range"].into_iter().filter(|name|condition.contains(name)).collect::<Vec<_>>(),
                ["$EVENT_ID", "$EVENT_NOTE", "$EVENT_VELOCITY", "$CURRENT_SCRIPT_SLOT", "$EVENT_PAR_SOURCE", "$EVENT_PAR_0", "$NI_MAIN_SCRIPT", "$ENGINE_UPTIME"].into_iter().filter(|name|condition.contains(name)).collect::<Vec<_>>(),
                condition.contains('=') || condition.contains('#') || condition.contains('<') || condition.contains('>'),
                condition.contains("and") || condition.contains("or"));
        }
        if in_note && ((token.strip_prefix("if").is_some_and(|rest| rest.starts_with(char::is_whitespace) || rest.starts_with('('))) || token == "exit" || token.starts_with("select ")) {
            traced.push(format!("%w15_path[$EVENT_ID] := {line}"));
        }
        if line == 157 {
            let expression = token.strip_prefix("select").unwrap().trim();
            traced.push(format!("%w15_selector[$EVENT_ID] := {expression}"));
            println!("selector_uses_legacy_event_array={}", expression.contains("%EVENT_PAR"));
            for name in ["get_event_par", "get_event_par_arr"] {
                if let Some((_, tail)) = expression.split_once(name) {
                    let Some(args) = tail.trim_start().strip_prefix('(') else { continue; };
                    let args=args.split(')').next().unwrap().split(',').map(|a| {
                        let a=a.trim();
                        if let Ok(v)=a.parse::<i64>() {v.to_string()}
                        else if a.starts_with("$EVENT_") {a.to_string()} else {"dynamic".into()}
                    }).collect::<Vec<_>>();
                    println!("selector_builtin={name} facts={args:?}");
                    let raw = tail.trim_start().strip_prefix('(').unwrap().split(')').next().unwrap().split(',').collect::<Vec<_>>();
                    if raw.len() >= 2 { traced.push(format!("%w15_value0[$EVENT_ID] := {}", raw[1])); }
                    traced.push(format!("%w15_value1[$EVENT_ID] := {}", raw[0]));
                }
            }
        }
        if line == 546 {
            traced.push(format!("%w15_before[$EVENT_ID] := get_event_par_arr($EVENT_ID,$EVENT_PAR_ALLOW_GROUP,39)"));
            traced.push(format!("%w15_condition[$EVENT_ID] := {condition}"));
        }
        traced.push(text.to_string());
        if token == "on init" {
            for name in ["before", "condition", "after", "entry", "tag", "source", "case3", "path", "early", "value0", "value1", "value2", "value3", "value4", "value5", "value6", "value7", "selector"] {
                traced.push(format!("declare %w15_{name}[64] make_persistent(%w15_{name})"));
            }
        }
        if token == "on note" {
            traced.push("%w15_entry[$EVENT_ID] := 1".into());
            traced.push("%w15_tag[$EVENT_ID] := get_event_par($EVENT_ID,0)".into());
            traced.push("%w15_source[$EVENT_ID] := get_event_par($EVENT_ID,$EVENT_PAR_SOURCE)".into());
        }
        if line == 362 { traced.push("%w15_case3[$EVENT_ID] := 1".into()); }
        if line == 546 {
            traced.push("%w15_after[$EVENT_ID] := get_event_par_arr($EVENT_ID,$EVENT_PAR_ALLOW_GROUP,39)".into());
        }
    }
    b.source = traced.join("\n");
    for b in &mut library.instrument.behaviors[2..4] {
        b.source = b.source.lines().flat_map(|text| {
            let extra = match text.trim() {
                "on init" => Some("declare %w15_start[64] make_persistent(%w15_start)"),
                "on note" => Some("%w15_start[$EVENT_ID] := 1 + get_event_par_arr($EVENT_ID,$EVENT_PAR_ALLOW_GROUP,39)"),
                _ => None,
            };
            std::iter::once(text).chain(extra)
        }).collect::<Vec<_>>().join("\n");
    }
    for b in &library.instrument.behaviors {
        let mut callback = "outside";
        for (line, text) in b.source.lines().enumerate() {
            let text = text.trim();
            for kind in ["init", "note", "release", "controller", "ui_control"] {
                if text.starts_with(&format!("on {kind}")) { callback = kind; }
            }
            if text == "end on" { callback = "outside"; }
            if text.contains("play_note(") && text.contains("1000") {
                println!("saved_call slot={} line={} callback={} duration_1000=true group_write_next_8={}",
                    b.slot.unwrap_or(255), line, callback,
                    b.source.lines().skip(line+1).take(8).any(|s|s.contains("set_event_par_arr")));
            }
        }
    }
    library.instrument.behaviors.push(ir::Behavior {
        name: "Numeric routing witness".into(), language: ir::Language::Ksp,
        source: "on init
            declare $count
            declare %ids[64] declare %source[64] declare %time[64]
            declare %kind[64] declare %group[64] declare %volume[64] declare %tag[64]
          end on
          on note
            %ids[$count] := $EVENT_ID
            %source[$count] := get_event_par($EVENT_ID,$EVENT_PAR_SOURCE)
            %time[$count] := $ENGINE_UPTIME
            %kind[$count] := 0
            %group[$count] := get_event_par_arr($EVENT_ID,$EVENT_PAR_ALLOW_GROUP,39)
            %volume[$count] := get_event_par($EVENT_ID,$EVENT_PAR_VOLUME)
            %tag[$count] := get_event_par($EVENT_ID,0)
            inc($count)
          end on
          on release
            %ids[$count] := $EVENT_ID
            %source[$count] := get_event_par($EVENT_ID,$EVENT_PAR_SOURCE)
            %time[$count] := $ENGINE_UPTIME
            %kind[$count] := 1
            %group[$count] := get_event_par_arr($EVENT_ID,$EVENT_PAR_ALLOW_GROUP,39)
            %volume[$count] := get_event_par($EVENT_ID,$EVENT_PAR_VOLUME)
            %tag[$count] := get_event_par($EVENT_ID,0)
            inc($count)
          end on".into(),
        slot: Some(4), state: vec![], requires: vec![],
    });
    for zone in &mut library.instrument.zones { zone.playback = Default::default(); }
    let pcm = Pcm::new(48000, vec![[0.001;2];96000].into_boxed_slice())?;
    let count = library.instrument.assets.len();
    let loaded = sampler_kontakt::finish(library.instrument, vec![pcm;count], vec!["RAM".into();count],
        &sampler_kontakt::Options { library: Some(path.into()), mpe: None, ..Default::default() })
        .map_err(|_| anyhow!("prepare failed; authored diagnostics omitted"))?;
    assert_eq!(loaded.scripts.len(), 5);
    let markers=loaded.scripts.iter().enumerate().flat_map(|(slot,s)|s.model().persistent.iter().filter(|p|p.name.starts_with("%w15_")).cloned().map(move |p|(slot,p))).collect::<Vec<_>>();
    let limits = Limits::for_plan(&loaded.plan, 128, 256);
    let mut rt = Runtime::new(loaded.plan, limits)?;
    let input = Input {protocol:Protocol::Native, port:0,group:0,channel:0,key:60,external_id:Some(60)};
    heap::without_heap(|| {
        rt.trigger(input, 60, 100./127.).unwrap();
        let mut block = [[0.;2];64];
        for _ in 0..375 {rt.render(&mut block).unwrap(); rt.flush_behaviors(|_,_,_,|true);}
        rt.note_off(input,None).unwrap();
        for _ in 0..375 {rt.render(&mut block).unwrap(); rt.flush_behaviors(|_,_,_,|true);}
    });
    let plan=rt.active_plan(); let instance=ScriptInstanceId(slot as u16);
    let n=rt.script_cell(plan,instance,0)?;
    println!("synthetic_pcm=true records={n} nonfinite={} preemptions={}",rt.nonfinite_frames(),rt.preemptions());
    for i in 0..n as usize {
        let values=(0..7).map(|a|rt.script_cell(plan,instance,(1+a*64+i) as u32).unwrap()).collect::<Vec<_>>();
        println!("event={} creator_slot={} time_ms={} release={} group39={} volume_mdb={} tag0={}",values[0],values[1],values[2],values[3],values[4],values[5],values[6]);
    }
    for (slot,marker) in markers {
        if let sampler_ksp::model::Location::Cells{offset,..}=marker.location {
            let values=(1..8).map(|i|rt.script_cell(plan,ScriptInstanceId(slot as u16),offset+i).unwrap()).collect::<Vec<_>>();
            println!("marker slot={slot} {} events1_7={values:?}",marker.name);
        }
    }
    Ok(())
}
