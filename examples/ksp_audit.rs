//! In-memory whole-container census. Emits counts and source hashes, never source or saved text.
//! ksp_audit FILE_LIST START COUNT [V1_BRIDGE]; FILE_LIST is path<TAB>status.
use ni_file::kontakt::{
    Chunk, StructuredObject,
    objects::{Bank, GroupList},
};
use sampler_ksp::{Environment, Limits, model::Value};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{DefaultHasher, Hash, Hasher},
    io::{BufRead, Write},
    path::Path,
    process::{Command, Stdio},
};

fn tokens(source: &str) -> BTreeSet<String> {
    let b = source.as_bytes();
    let mut i = 0;
    let mut out = BTreeSet::new();
    while i < b.len() {
        if b[i] == b'{' {
            while i < b.len() && b[i] != b'}' {
                i += 1;
            }
            i += usize::from(i < b.len());
        } else if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                } else if b[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else if b[i].is_ascii_alphabetic() || b[i] == b'_' || b[i] == b'$' {
            let start = i;
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.insert(source[start..i].to_ascii_lowercase());
        } else {
            i += 1;
        }
    }
    out
}
fn saved(entries: &[String], slot: u8, groups: &[String]) -> Environment {
    let mut e = Environment {
        slot,
        groups: groups.to_vec(),
        ..Default::default()
    };
    for s in entries {
        let Some((name, text)) = s.split_once(' ') else {
            continue;
        };
        match name.as_bytes()[0] {
            b'$' => {
                if let Ok(n) = text.trim().parse() {
                    e.persisted.insert(name.into(), Value::Int(n));
                }
            }
            b'~' => {
                if let Ok(n) = text.trim().parse() {
                    e.persisted.insert(name.into(), Value::Real(n));
                }
            }
            b'@' => {
                e.persisted.insert(name.into(), Value::Text(text.into()));
            }
            b'%' | b'?' => {
                let values = text
                    .split_whitespace()
                    .map(|s| {
                        if name.starts_with('%') {
                            s.parse().map(Value::Int).ok()
                        } else {
                            s.parse().map(Value::Real).ok()
                        }
                    })
                    .collect::<Option<Vec<_>>>();
                if let Some(v) = values {
                    e.persisted_arrays.insert(name.into(), v);
                }
            }
            _ => (),
        }
    }
    e
}
fn group_names(children: &[Chunk], fallback: &[String]) -> Vec<String> {
    children
        .iter()
        .find(|c| c.id == 0x33)
        .and_then(|c| GroupList::try_from(c).ok())
        .map(|g| {
            g.groups
                .iter()
                .filter_map(|g| g.params().ok())
                .filter(|g| !g.muted)
                .map(|g| g.name)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| fallback.to_vec())
}
fn scripts(
    c: &Chunk,
    groups: &[String],
    slot: &mut u8,
    origin: &'static str,
    out: &mut Vec<(String, Vec<String>, u8, Vec<String>, &'static str)>,
) -> Result<(), String> {
    match c.id {
        6 => {
            let mut wire = c.id.to_le_bytes().to_vec();
            wire.extend((c.data.len() as u32).to_le_bytes());
            wire.extend(&c.data);
            let limits = sampler_kontakt::Limits {
                bytes: 128 << 20,
                records: 1 << 20,
            };
            let chunks =
                sampler_kontakt::Chunks::parse(&wire, limits).map_err(|_| "script framing")?;
            let s = sampler_kontakt::Script::parse(chunks.iter().next().unwrap(), limits)
                .map_err(|_| "script record")?;
            if !s.bypass
                && let Some(text) = s.text
                && !std::str::from_utf8(text.data())
                    .unwrap_or("")
                    .trim()
                    .is_empty()
            {
                let source = std::str::from_utf8(text.data())
                    .map_err(|_| "source encoding")?
                    .to_owned();
                let saved = s
                    .persistent
                    .into_iter()
                    .flat_map(|t| t.iter())
                    .map(|x| String::from_utf8_lossy(x.data()).into_owned())
                    .collect();
                out.push((source, saved, *slot, groups.to_vec(), origin));
            }
            *slot += 1;
        }
        0x28 | 0x29 => {
            let o = StructuredObject::try_from(c).map_err(|_| "structured program")?;
            let names = group_names(&o.children, groups);
            let mut slot = 0;
            for child in &o.children {
                scripts(child, &names, &mut slot, origin, out)?;
            }
        }
        3 => {
            let bank = Bank::try_from(c).map_err(|_| "bank")?;
            for c in bank.0.children.iter().filter(|c| c.id == 6 || c.id == 0x29) {
                scripts(
                    c,
                    groups,
                    slot,
                    if c.id == 6 {
                        "bank-global"
                    } else {
                        "bank-container"
                    },
                    out,
                )?;
            }
            for (_, s) in bank.slot_list().map_err(|_| "bank slots")?.slots {
                for p in s.program_list().map_err(|_| "program list")?.programs {
                    let names = group_names(&p.0.children, groups);
                    let mut slot = 0;
                    for c in &p.0.children {
                        scripts(c, &names, &mut slot, "program", out)?;
                    }
                }
            }
        }
        _ => (),
    }
    Ok(())
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|x| x == "--check") {
        assert_eq!(
            tokens("{wait(1)} \"async_complete\" wait(10) $ENGINE_PAR_VOLUME"),
            BTreeSet::from(["wait".into(), "$engine_par_volume".into()])
        );
        let e = saved(&["%a 7 -3 0 ".into()], 0, &[]);
        assert_eq!(e.persisted_arrays["%a"].len(), 3);
        return;
    }
    let files = std::fs::read_to_string(&args[1]).unwrap();
    let start: usize = args[2].parse().unwrap();
    let count: usize = args[3].parse().unwrap();
    let mut child = args.get(4).map(|p| {
        Command::new(p)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap()
    });
    let mut input = child.as_mut().map(|c| c.stdin.take().unwrap());
    let mut output = child
        .as_mut()
        .map(|c| std::io::BufReader::new(c.stdout.take().unwrap()));
    let mut cache = BTreeMap::<String, Json>::new();
    for path in files
        .lines()
        .skip(1 + start)
        .take(count)
        .map(|s| s.split('\t').next().unwrap())
    {
        let started = std::time::Instant::now();
        let mut slots = Vec::new();
        let result = sampler_kontakt::read_chunks(Path::new(path))
            .map_err(|_| "read container".to_owned())
            .and_then(|cs| {
                let is_multi = cs.0.iter().any(|c| c.id == 3);
                for c in &cs.0 {
                    if is_multi && c.id != 3 {
                        continue;
                    }
                    scripts(c, &[], &mut 0, "root", &mut slots)?;
                }
                Ok(())
            });
        let mut records = Vec::new();
        for (source, entries, slot, groups, origin) in slots {
            let mut h = DefaultHasher::new();
            source.hash(&mut h);
            let source_hash = format!("{:016x}", h.finish());
            (slot, &groups, &entries).hash(&mut h);
            if std::env::var_os("KSP_AUDIT_RESOURCES").is_some() {
                path.hash(&mut h);
            }
            let key = format!("{:016x}", h.finish());
            if !cache.contains_key(&key) {
                let mut data = json!({"hash":source_hash,"slot":slot,"groups":groups.len(),"saved_entries":entries.len()});
                let ts = tokens(&source);
                let prefixes = [
                    "$engine_par_",
                    "$ni_signal_",
                    "$duration_",
                    "$ni_callback_",
                    "ui_",
                    "mf_",
                    "$event_par_",
                    "$num_",
                    "%groups_",
                ];
                let tracked = [
                    "subscribe_async",
                    "$current_script_slot",
                    "$all_events",
                    "%poly_at",
                    "pgs_get_key_val",
                    "get_event_ids",
                    "event_status",
                    "make_persistent",
                    "make_instr_persistent",
                    "read_persistent_var",
                    "wait",
                    "wait_ticks",
                    "wait_async",
                    "stop_wait",
                    "listen",
                    "set_listener",
                    "async_complete",
                    "persistence_changed",
                    "load_ir_sample",
                    "load_array",
                    "load_array_str",
                    "save_array",
                    "save_array_str",
                    "get_engine_par",
                    "get_engine_par_disp",
                    "get_engine_par_disp_ext",
                    "set_engine_par",
                    "find_mod",
                    "find_target",
                    "reset_rls_trig_counter",
                    "set_knob_label",
                    "set_text",
                    "set_menu_item_visibility",
                    "ms_to_ticks",
                    "ticks_to_ms",
                    "play_note",
                    "note_off",
                    "get_purge_state",
                    "purge_group",
                    "set_control_par_arr",
                    "get_control_par_arr",
                    "$num_output_channels",
                    "$ni_song_position",
                    "$event_par_allow_group",
                ];
                data["tokens"] = json!(
                    ts.iter()
                        .filter(|s| prefixes.iter().any(|p| s.starts_with(p))
                            || tracked.contains(&s.as_str()))
                        .collect::<Vec<_>>()
                );
                data["saved_tags"] = json!(entries.iter().fold(
                    BTreeMap::<String, usize>::new(),
                    |mut m, s| {
                        *m.entry(s.chars().next().unwrap_or('\0').to_string())
                            .or_default() += 1;
                        m
                    }
                ));
                let mut env = saved(&entries, slot, &groups);
                if std::env::var_os("KSP_AUDIT_RESOURCES").is_some() {
                    if let Some(name) = sampler_ksp::nckp::view_name(&source) {
                        let bytes = sampler_kontakt::Resources::of(Path::new(path))
                            .read(&format!("Resources/performance_view/{name}.nckp"));
                        if let Some(bytes) = bytes {
                            if let Ok((view, _)) = sampler_ksp::nckp::parse(&bytes) {
                                env.performance_view = view;
                            }
                        }
                    }
                    data["resource_controls"] = json!(env.performance_view.controls.len());
                }
                let t = std::time::Instant::now();
                data["init_frontend"] =
                    json!(sampler_ksp::init_engine_pars(&source, Limits::LIBRARY, &env).is_ok());
                data["init_frontend_us"] = json!(t.elapsed().as_micros());
                let t = std::time::Instant::now();
                match sampler_ksp::compile_with(&source, 48000, Limits::LIBRARY, &[], &env) {
                    Ok(s) => {
                        data["compile"] = json!(true);
                        data["coverage"] = json!(
                            s.coverage()
                                .iter()
                                .map(|(name, c, n)| json!([name, format!("{c:?}"), n]))
                                .collect::<Vec<_>>()
                        );
                        data["warnings"] = json!(s.warnings().iter().fold(
                            BTreeMap::<String, usize>::new(),
                            |mut m, w| {
                                *m.entry(format!(
                                    "{:?}:{}",
                                    w.kind,
                                    w.builtin.unwrap_or("init_or_ui")
                                ))
                                .or_default() += 1;
                                m
                            }
                        ));
                        let mut widgets = BTreeMap::<String, usize>::new();
                        let mut bad_menu = 0;
                        for w in &s.model().interface.widgets {
                            *widgets.entry(format!("{:?}", w.kind)).or_default() += 1;
                            if w.kind == sampler_ksp::model::WidgetKind::Menu
                                && w.menu.iter().enumerate().any(|(i, m)| m.value != i as i32)
                            {
                                bad_menu += 1;
                            }
                        }
                        data["widgets"] = json!(widgets);
                        data["nonidentity_menus"] = json!(bad_menu);
                        match s.ui(&|_| None) {
                            Ok(ui) => {
                                data["ui"] = json!(true);
                                data["wallpaper"] =
                                    json!(ui.pages.iter().any(|p| p.background.image.is_some()));
                                data["ui_unsupported"] = json!(
                                    ui.unsupported
                                        .iter()
                                        .map(|u| u.feature.clone())
                                        .collect::<BTreeSet<_>>()
                                );
                            }
                            Err(_) => data["ui"] = json!(false),
                        }
                        data["bind"] = json!(
                            s.bind(sampler_core::Prepared::new(48000, vec![], vec![], 0).unwrap())
                                .is_ok()
                        );
                    }
                    Err(e) => {
                        data["compile"] = json!(false);
                        data["failure"] = json!({"message":e.message,"line":e.line,"column":e.column,"kind":format!("{:?}",e.kind),"builtin":e.builtin, "category":if e.message.contains("budget") {"budget"} else if e.message.contains("fuel") {"init fuel"} else if e.message.contains("unknown") {"unknown symbol"} else {"frontend"}});
                    }
                }
                data["compile_us"] = json!(t.elapsed().as_micros());
                if let (Some(input), Some(output)) = (&mut input, &mut output) {
                    writeln!(
                        input,
                        "{}",
                        json!({"source":source,"saved":entries,"groups":groups,"path":std::env::var_os("KSP_AUDIT_RESOURCES").map(|_|path)})
                    )
                    .unwrap();
                    input.flush().unwrap();
                    let mut line = String::new();
                    output.read_line(&mut line).unwrap();
                    data["v1"] = serde_json::from_str(&line).expect("v1 response");
                }
                cache.insert(key.clone(), data);
            }
            let mut record = cache[&key].clone();
            record["origin"] = json!(origin);
            records.push(record);
        }
        println!(
            "{}",
            json!({"path":path,"read":result.is_ok(),"read_error":result.err(),"scripts":records,"elapsed_us":started.elapsed().as_micros()})
        );
    }
    drop(input);
    if let Some(mut c) = child {
        assert!(c.wait().unwrap().success());
    }
}
