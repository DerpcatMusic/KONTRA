//! Single-instrument UI parameter probe. No source, saved values or assets are written.
//! ui_params_probe --probe PATH; corpus collection belongs to the shared scanner.
use ni_file::kontakt::{
    Chunk, StructuredObject,
    objects::{BParScript, Bank, Snapshot},
};
use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
};

fn words(source: &str) -> Vec<&str> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' | b'"' => {
                let end = if bytes[i] == b'{' { b'}' } else { b'"' };
                i += 1;
                while i < bytes.len() && bytes[i] != end {
                    i += 1;
                }
                i += usize::from(i < bytes.len());
            }
            b'$' | b'_' | b'a'..=b'z' | b'A'..=b'Z' => {
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                out.push(&source[start..i]);
            }
            _ => i += 1,
        }
    }
    out
}

fn count(counts: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *counts.entry(key.into()).or_default() += 1;
}

fn inspect(chunk: &Chunk, counts: &mut BTreeMap<String, usize>) -> Result<(), ()> {
    match chunk.id {
        6 => {
            let script = BParScript::try_from(chunk)
                .and_then(|s| s.params())
                .map_err(|_| ())?;
            count(counts, "script_records");
            let prefix = if script.bypass { "bypass:" } else { "" };
            if let Some(source) = script.text {
                count(counts, format!("{prefix}source_slots"));
                let mut hasher = DefaultHasher::new();
                source.hash(&mut hasher);
                count(counts, format!("source_hash:{:016x}", hasher.finish()));
                // ponytail: lexical mentions include inactive preprocessor branches;
                // use resolved HIR for exact reached-call counts in a later runtime census.
                for word in words(&source) {
                    let word = word.strip_prefix('_').unwrap_or(word);
                    if word.starts_with("$CONTROL_PAR_")
                        || word.starts_with("ui_")
                        || word.starts_with("set_control_")
                        || word.starts_with("get_control_")
                        || word.starts_with("set_ui_")
                        || word.starts_with("get_ui_")
                        || word.starts_with("set_key")
                        || word.starts_with("get_key")
                        || word.starts_with("set_nks_")
                        || [
                            "get_ui_id",
                            "make_perfview",
                            "hide_part",
                            "move_control",
                            "move_control_px",
                            "make_persistent",
                            "make_instr_persistent",
                            "read_persistent_var",
                            "set_snapshot_type",
                            "set_script_title",
                            "set_skin_offset",
                            "load_performance_view",
                            "load_native_ui",
                            "load_komplete_ui",
                            "reset_nks_nav",
                            "remove_keyrange",
                            "set_text",
                            "add_text_line",
                            "set_knob_label",
                            "set_knob_unit",
                            "set_knob_defval",
                            "add_menu_item",
                            "set_menu_item_str",
                            "set_menu_item_value",
                            "set_menu_item_visibility",
                            "get_menu_item_str",
                            "get_menu_item_value",
                            "get_menu_item_visibility",
                            "get_num_menu_items",
                            "get_font_id",
                            "set_table_steps_shown",
                            "attach_zone",
                            "attach_level_meter",
                            "fs_navigate",
                            "fs_get_filename",
                            "expose_controls",
                            "persistence_changed",
                            "set_listener",
                            "change_listener_par",
                        ]
                        .contains(&word)
                    {
                        count(counts, format!("{prefix}{word}"));
                    }
                }
            }
            for entry in script.persistent {
                if let Some(sigil) = entry.as_bytes().first() {
                    count(counts, format!("saved_tag:{}", *sigil as char));
                }
            }
        }
        0x28 | 0x29 => {
            let object = StructuredObject::try_from(chunk).map_err(|_| ())?;
            for child in &object.children {
                inspect(child, counts)?;
            }
        }
        3 => {
            let bank = Bank::try_from(chunk).map_err(|_| ())?;
            for child in bank.0.children.iter().filter(|c| c.id == 6 || c.id == 0x29) {
                inspect(child, counts)?;
            }
            for (_, slot) in bank.slot_list().map_err(|_| ())?.slots {
                for program in slot.program_list().map_err(|_| ())?.programs {
                    for child in &program.0.children {
                        inspect(child, counts)?;
                    }
                }
            }
        }
        0x4f => {
            let snapshot = Snapshot::try_from(chunk).map_err(|_| ())?;
            count(counts, "snapshot_records");
            for slot in snapshot.persistent {
                for entry in slot {
                    if let Some(sigil) = entry.as_bytes().first() {
                        count(counts, format!("snapshot_saved_tag:{}", *sigil as char));
                    }
                }
            }
        }
        _ => (),
    }
    Ok(())
}

fn main() {
    // Parser panic payloads may contain source or saved data; export status only.
    std::panic::set_hook(Box::new(|_| {}));
    let args: Vec<_> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--probe" {
        probe(Path::new(&args[2]));
        return;
    }
    eprintln!("usage: ui_params_probe --probe PATH");
    std::process::exit(2);
}

fn probe(path: &Path) {
    let mut metadata = BTreeMap::new();
    let chunks = sampler_kontakt::read_chunks(path).unwrap();
    for chunk in &chunks.0 {
        inspect(chunk, &mut metadata).unwrap();
    }
    for (key, value) in metadata {
        println!("metadata\t{key}\t{value}");
    }
    use sampler_ir::Saved;
    use sampler_ksp::model::Value;
    let kontakt = sampler_kontakt::read(path).unwrap();
    let mut resources = sampler_kontakt::Resources::of(path);
    for (index, behavior) in kontakt.instrument.behaviors.iter().enumerate() {
        let mut env = sampler_ksp::Environment {
            groups: kontakt
                .instrument
                .groups
                .iter()
                .map(|g| g.name.clone())
                .collect(),
            slot: behavior.slot.unwrap_or(index as u8),
            ..Default::default()
        };
        for (name, saved) in &behavior.state {
            match saved {
                Saved::Int(n) => {
                    env.persisted.insert(name.clone(), Value::Int(*n as i32));
                }
                Saved::Real(r) => {
                    env.persisted.insert(name.clone(), Value::Real(*r));
                }
                Saved::Text(t) => {
                    env.persisted.insert(name.clone(), Value::Text(t.clone()));
                }
                Saved::Ints(v) => {
                    env.persisted_arrays.insert(
                        name.clone(),
                        v.iter().map(|v| Value::Int(*v as i32)).collect(),
                    );
                }
                Saved::Reals(v) => {
                    env.persisted_arrays
                        .insert(name.clone(), v.iter().map(|v| Value::Real(*v)).collect());
                }
            }
        }
        if let Some(name) = sampler_ksp::nckp::view_name(&behavior.source) {
            if let Some(bytes) = resources.read(&format!("Resources/performance_view/{name}.nckp"))
            {
                env.performance_view = sampler_ksp::nckp::parse(&bytes).unwrap().0;
            }
        }
        let begun = std::time::Instant::now();
        match sampler_ksp::compile_with(
            &behavior.source,
            48000,
            sampler_ksp::Limits::LIBRARY,
            &[],
            &env,
        ) {
            Ok(script) => {
                let ui = script.ui(&|_| None).unwrap();
                println!(
                    "slot\t{}\tcompile_ms\t{}\twidgets\t{}\tcontrols\t{}\tcallbacks\t{}\tassets\t{}\tunsupported\t{}\twarnings\t{}",
                    env.slot,
                    begun.elapsed().as_millis(),
                    ui.widgets.len(),
                    script.controls().len(),
                    script
                        .entries()
                        .iter()
                        .filter(|e| matches!(e.kind, sampler_ksp::EntryKind::UiControl(_)))
                        .count(),
                    ui.assets.len(),
                    ui.unsupported.len(),
                    script.warnings().len()
                );
                let mut counts = BTreeMap::new();
                for unsupported in &ui.unsupported {
                    count(&mut counts, &unsupported.feature);
                }
                for (key, value) in counts {
                    println!("unsupported\t{key}\t{value}");
                }
                for (name, coverage, sites) in script.coverage() {
                    println!("coverage\t{name}\t{coverage:?}\t{sites}");
                }
            }
            Err(e) => println!(
                "slot\t{}\tcompile_error_line\t{}\tcolumn\t{}",
                env.slot, e.line, e.column
            ),
        }
    }
}

#[test]
fn probe_ignores_comments_and_strings_and_keeps_complete_names() {
    assert_eq!(
        words("{$CONTROL_PAR_BAD} \"ui_bad\" set_control_par($id,$CONTROL_PAR_VALUE,1)"),
        ["set_control_par", "$id", "$CONTROL_PAR_VALUE"]
    );
}
