//! Offline source and conversion inspection. No plugin, Runtime or audio audition.
use ni_file::kontakt::objects::{BParScript, Bank as NiBank, GroupList, Program, ZoneList};
use sampler_parity::{Lang, ScriptReport};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

const XML_BYTES: u64 = 32 << 20;

#[derive(Default, Debug)]
struct Options {
    path: PathBuf,
    program: Option<String>,
    initialize: bool,
    source: bool,
    summary: bool,
}

fn options(args: &[String]) -> Result<Options, &'static str> {
    let mut opts = Options::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--program" => {
                opts.program = Some(args.next().ok_or("--program needs a value")?.clone())
            }
            "--initialize" => opts.initialize = true,
            "--ephemeral-source" => opts.source = true,
            "--summary" => opts.summary = true,
            flag if flag.starts_with('-') => return Err("unknown inspector flag"),
            _ if opts.path.as_os_str().is_empty() => opts.path = arg.into(),
            _ => return Err("inspect accepts one input path"),
        }
    }
    if opts.path.as_os_str().is_empty() {
        return Err("inspect needs an input path");
    }
    Ok(opts)
}

fn stage(report: &mut Value, name: &str, status: &str, scope: &str, owner: &str) {
    report["stages"].as_array_mut().unwrap().push(json!({
        "name": name, "status": status, "scope": scope, "owner": owner
    }));
    if status == "failed" {
        report["status"] = json!("failed");
    } else if status == "partial" && report["status"] == "ok" {
        report["status"] = json!("partial");
    }
}

fn diagnostic(report: &mut Value, stage: &str, category: &str, detail: &str, owner: &str) {
    report["diagnostics"].as_array_mut().unwrap().push(json!({
        "stage": stage, "category": category, "feature": category,
        "location": null, "detail": detail, "owner": owner, "spec": null
    }));
}

fn fail(report: &mut Value, name: &str, category: &str, detail: &str, owner: &str) {
    stage(report, name, "failed", detail, owner);
    diagnostic(report, name, category, detail, owner);
    report["status"] = json!("failed");
}

fn kontakt_error(report: &mut Value, error: &sampler_kontakt::LoadError) {
    let name = format!(
        "{:?}",
        error.stage().unwrap_or(sampler_kontakt::Stage::Container)
    );
    let owner = error
        .location()
        .map(|at| format!("{}:{}", at.file(), at.line()))
        .unwrap_or_else(|| "crates/sampler-kontakt/src/container.rs".into());
    fail(
        report,
        &name,
        &format!("{:?}", error.kind()),
        "Kontakt source/conversion failed; authored error values are omitted",
        &owner,
    );
    let d = report["diagnostics"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    d["stage_origin"] = json!(if error.location().is_some() {
        "tagged"
    } else {
        "kind_fallback"
    });
    if let sampler_kontakt::LoadError::Io { error, .. } = error.cause() {
        d["io_kind"] = json!(format!("{:?}", error.kind()));
    }
    ephemeral_error(report, error);
}

fn ephemeral_error(report: &mut Value, error: &impl std::fmt::Display) {
    if report["mode"]["ephemeral_source"] == true {
        if let Some(d) = report["diagnostics"].as_array_mut().unwrap().last_mut() {
            d["detail"] = json!(error.to_string());
            d["detail_scope"] =
                json!("ephemeral authored diagnostic; omitted from source-free reports");
        }
    }
}

fn add_script(
    report: &mut Value,
    script: ScriptReport,
    source: Option<&str>,
    program: Option<usize>,
    slot: Option<usize>,
    bypassed: Option<bool>,
) {
    let mut script = serde_json::to_value(script).unwrap();
    script["program"] = json!(program);
    script["slot"] = json!(slot);
    script["bypassed"] = json!(bypassed);
    if let Some(source) = source {
        script["source"] = json!(source);
    }
    report["scripts"].as_array_mut().unwrap().push(script);
}

fn base(opts: &Options, kind: &str) -> Value {
    json!({
        "format": "kontra-library-inspector", "schema": 1,
        "item": {"path": opts.path, "kind": kind, "program": opts.program},
        "mode": {"initialize": opts.initialize, "ephemeral_source": opts.source},
        "status": "ok", "stages": [], "metadata": {}, "scripts": [],
        "converted": {"status": "not_run", "scope": "No product IR translation requested"},
        "ui": {"status": "not_evaluated", "scope": "Source frontends do not execute authored UI initialization", "interfaces": []},
        "diagnostics": [],
        "evidence": {"runtime": "not_run", "audio": "not_run", "native_equivalence": "unknown",
            "reference_policy":"Native host behavior and official scripting contracts; implementation agreement is not native proof",
            "native_reference":{"host":if kind.starts_with("kontakt") {"Native Instruments Kontakt"} else {"UVI Falcon/Workstation"},"measurement":"not_run"},
            "implementation_history": {"kontakt_v1": "0cb7a8a0", "uvi_v1": "4bffbb18"},
            "implementation_comparison":"Historical commits only; v1 conversion was not executed"}
    })
}

fn source_program(
    report: &mut Value,
    path: &Path,
    program: &Program,
    index: usize,
    rack_slot: Option<(u16, usize)>,
    opts: &Options,
) -> Value {
    let mut record = json!({"program": index, "rack_slot": rack_slot.map(|r| r.0), "program_within_slot": rack_slot.map(|r| r.1), "version": program.version(),
        "records": program.0.children.iter().enumerate().map(|(n,c)| json!({"ordinal": n,"record_id": c.id,"bytes": c.data.len()})).collect::<Vec<_>>()});
    if let Ok(params) = program.params() {
        record["name"] = json!(params.name);
        record["key_range"] = json!([params.low_key, params.high_key]);
        record["velocity_range"] = json!([params.low_velocity, params.high_velocity]);
        record["unknown_public_bytes"] = json!(params.unknown_tail.len());
    } else {
        diagnostic(
            report,
            "parse",
            "ProgramParameters",
            "Program public parameters were rejected",
            "vendor/ni-file/src/kontakt/objects/program.rs",
        );
    }
    if let Some(chunk) = program.0.find_first(0x33) {
        match GroupList::try_from(chunk) {
            Ok(groups) => record["groups"] = json!(groups.groups.iter().enumerate().map(|(n,g)| {
                let params = g.params().ok();
                json!({"source_group": n,"name": params.as_ref().map(|p| &p.name),
                    "muted": params.as_ref().map(|p| p.muted),"soloed": params.as_ref().map(|p| p.soloed),
                    "parameter_decode": if params.is_some() {"ok"} else {"failed"}})
            }).collect::<Vec<_>>()),
            Err(_) => diagnostic(report,"parse","GroupList","Typed group records were rejected","vendor/ni-file/src/kontakt/objects/group_list.rs"),
        }
    }
    if let Some(chunk) = program.0.find_first(0x34) {
        match ZoneList::try_from(chunk) {
            Ok(zones) => {
                record["zones"] =
                    json!({"count":zones.zones().len(),"source_group_ids":zones.group_ids})
            }
            Err(_) => diagnostic(
                report,
                "parse",
                "ZoneList",
                "Typed zone records were rejected",
                "vendor/ni-file/src/kontakt/objects/zone_list.rs",
            ),
        }
    }
    let mut resources = sampler_kontakt::Resources::of(path);
    for (slot, chunk) in program.0.children.iter().filter(|c| c.id == 6).enumerate() {
        let Ok(params) = BParScript::try_from(chunk).and_then(|s| s.params()) else {
            diagnostic(
                report,
                "parse",
                "ScriptSlot",
                "A physical script slot could not be decoded",
                "vendor/ni-file/src/kontakt/objects/bpar_script.rs",
            );
            continue;
        };
        let mut source = params.text.unwrap_or_default();
        if let Some(link) = params
            .textfile_name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            if let Some(linked) = resources.script(link) {
                source = linked;
            }
        }
        // The source-only Kontakt frontend below compiles each script with its
        // physical slot and performance-view environment. Keep only display
        // metadata here so the script is not compiled twice.
        let id = if let Some((rack, within)) = rack_slot {
            format!("ksp:program{rack}:{within}:slot{slot}")
        } else {
            format!("ksp:slot{slot}")
        };
        let frontend = ScriptReport {
            id: id.clone(),
            title: format!("Script slot {slot}"),
            lang: Lang::Ksp,
            source_len: source.len().min(u32::MAX as usize) as u32,
            tokens: Vec::new(),
            constructs: Vec::new(),
            lowered: Vec::new(),
        };
        add_script(
            report,
            frontend,
            opts.source.then_some(source.as_str()),
            Some(index),
            Some(slot),
            Some(params.bypass),
        );
        if opts.source {
            if let Some(name) = sampler_ksp::nckp::view_name(&source) {
                let path = format!("Resources/performance_view/{name}.nckp");
                let mut view = json!({
                    "script_id": id,
                    "slot": slot,
                    "path": path,
                    "format": "NCKP JSON",
                    "provenance": "literal load_performance_view in the authored KSP on init"
                });
                match resources.read_result(&path) {
                    Ok(Some(bytes)) => {
                        view["bytes"] = json!(bytes.len());
                        match sampler_ksp::nckp::parse(&bytes) {
                            Ok((_, skipped)) => {
                                view["parse_status"] = json!("ok");
                                let omitted_fields = skipped.iter().filter(|warning|
                                    sampler_ksp::nckp::omitted_field(warning).is_some()).count();
                                view["skipped"] = json!({
                                    "total": skipped.len(),
                                    "omitted_source_fields": omitted_fields,
                                    "unsupported_control_types": skipped.len() - omitted_fields
                                });
                            }
                            Err(error) => {
                                view["parse_status"] = json!("failed");
                                view["parse_detail"] = json!(error);
                            }
                        }
                        match String::from_utf8(bytes) {
                            Ok(text) => view["source"] = json!(text),
                            Err(_) => view["text_status"] = json!("non_utf8"),
                        }
                    }
                    Ok(None) => view["parse_status"] = json!("missing"),
                    Err(error) => {
                        view["parse_status"] = json!("unavailable");
                        view["parse_detail"] = json!(error.to_string());
                    }
                }
                if record["ui_resources"].is_null() {
                    record["ui_resources"] = json!([]);
                }
                record["ui_resources"].as_array_mut().unwrap().push(view);
            }
        }
    }
    record
}

fn instrument(report: &mut Value, ir: &sampler_ir::Instrument, scope: &str) {
    let ownership: Vec<_> = ir.source_indices.modulators.iter().map(|m| json!({
        "source_group": m.group,"source_slot":m.slot,"external":m.external,"name":m.name,"runtime":m.runtime.map(|r|r.0)
    })).collect();
    report["converted"] = json!({
        "status":"ok","scope":scope,
        "summary":{"groups":ir.groups.len(),"zones":ir.zones.len(),"assets":ir.assets.len(),"buses":ir.buses.len(),"chains":ir.chains.len(),"modulators":ir.modulators.len(),"routes":ir.routes.len(),"controls":ir.controls.len(),"scripts":ir.behaviors.len(),"impulses":ir.impulses.len()},
        "groups":ir.groups,"zones":ir.zones,"buses":ir.buses,"chains":ir.chains,
        "modulators":ir.modulators,"routes":ir.routes,"controls":ir.controls,"processor_controls":ir.processor_controls,
        "ownership":ownership,"physical_control_aliases":ir.source_indices.control_aliases,
        "source_group_mapping":ir.source_indices.groups,"source_zone_mapping":ir.source_indices.zones,
        "source_records":ir.source_parameters.iter().map(|r|json!({"location":r.location,"object_id":r.object_id,"version":r.version,"fields":r.fields.iter().map(|f|f.name).collect::<Vec<_>>()})).collect::<Vec<_>>()
    });
    for omitted in &ir.unsupported {
        let detail = if report["mode"]["ephemeral_source"] == true {
            omitted.value.as_str()
        } else {
            "Source feature is unsupported or its law is unverified; authored value omitted"
        };
        report["diagnostics"].as_array_mut().unwrap().push(json!({
            "stage":"translate","category":format!("{:?}",omitted.reason),"feature":omitted.feature,
            "location":omitted.location,"detail":detail,
            "owner":null,"spec":null
        }));
    }
}

fn interfaces(faces: &[sampler_ui_ir::Interface]) -> Value {
    json!(faces.iter().enumerate().map(|(index, face)|json!({
        "index":index,"source":format!("{:?}",face.source),"performance":face.performance,
        "pages":face.pages.iter().enumerate().map(|(n,p)|json!({"index":n,"name":p.name})).collect::<Vec<_>>(),
        "widgets":face.widgets.iter().enumerate().map(|(n,w)|json!({"index":n,"name":w.name,"source_id":w.source_id,"page":w.page.0,"parent":w.parent.map(|p|p.0),"kind":format!("{:?}",w.kind),"rect":{"x":w.rect.x,"y":w.rect.y,"width":w.rect.width,"height":w.rect.height},"binding":format!("{:?}",w.binding),"components":w.components.iter().map(|id|id.0.to_string()).collect::<Vec<_>>(),"hidden":w.hidden,"enabled":w.enabled,"text":w.text})).collect::<Vec<_>>(),
        "assets":face.assets.iter().map(|a|json!({"path":a.path,"kind":format!("{:?}",a.kind)})).collect::<Vec<_>>(),
        "unsupported":face.unsupported.iter().map(|u|json!({"feature":u.feature,"widget":u.widget.map(|w|w.0),"detail":"UI projection does not model this source feature; authored value omitted"})).collect::<Vec<_>>()
    })).collect::<Vec<_>>())
}

fn uvi_interface(report: &mut Value, face: &sampler_ui_ir::Interface) {
    report["ui"] = json!({
        "status":if face.unsupported.is_empty() {"ok"} else {"partial"},
        "scope":"Actual Lua initialization/interface projection only; asynchronous/live callbacks and rendering not run",
        "interfaces":interfaces(std::slice::from_ref(face))
    });
    for gap in &face.unsupported {
        let ephemeral = report["mode"]["ephemeral_source"] == true;
        diagnostic(
            report,
            "ui_projection",
            "NotModeled",
            "UI projection does not model this source feature; authored value omitted",
            "crates/sampler-uvi/src/script/ui.rs",
        );
        let entry = report["diagnostics"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap();
        entry["feature"] = json!(gap.feature);
        entry["location"] = json!(gap.widget.map(|w| format!("interface 0 widget {}", w.0)));
        if ephemeral {
            entry["detail"] = json!(gap.value);
            entry["detail_scope"] =
                json!("ephemeral authored diagnostic; omitted from source-free reports");
        }
    }
    stage(
        report,
        "ui_projection",
        if face.unsupported.is_empty() {
            "ok"
        } else {
            "partial"
        },
        "Initialized UI model; renderer/engine services that were omitted are retained as diagnostics",
        "crates/sampler-uvi/src/script/ui.rs",
    );
}

fn uvi_initialization(
    report: &mut Value,
    host: &sampler_uvi::script::ScriptHost,
    source_lines: &BTreeMap<String, Vec<usize>>,
) {
    uvi_interface(report, &host.interface());
    let counts = host.fault_counts();
    let failed = counts.init.values().any(|count| *count != 0);
    let coverage = host.coverage();
    let at = |script: &str, offset: u32| {
        let line = source_lines
            .get(script)
            .map(|starts| starts.partition_point(|start| *start <= offset as usize));
        json!({"start":offset,"end":offset,"line":line})
    };
    report["evidence"]["initialization"] = json!({
        "status":if failed {"failed"} else {"ok"},
        "scope":"Actual bounded Luau script bodies/onInit only; no musical or gesture callbacks",
        "fault_counts":counts.init,
        "line_observations":coverage.iter().map(|c|json!({
            "script_id":c.script_id,
            "hits":c.hits.iter().map(|(offset,count)|json!({"span":at(&c.script_id,*offset),"count":count})).collect::<Vec<_>>(),
            "fault_count":c.faults.len()
        })).collect::<Vec<_>>()
    });
    let mut located_faults = false;
    for script in &coverage {
        let mut faults = BTreeMap::<u32, (u64, &str)>::new();
        for (offset, message) in &script.faults {
            let entry = faults.entry(*offset).or_insert((0, message));
            entry.0 += 1;
        }
        for (offset, (count, message)) in faults {
            located_faults = true;
            diagnostic(
                report,
                "initialize",
                "LuaInitializationFault",
                "The initializer faulted at this source line; authored error payload omitted",
                "crates/sampler-uvi/src/script.rs",
            );
            let entry = report["diagnostics"]
                .as_array_mut()
                .unwrap()
                .last_mut()
                .unwrap();
            entry["feature"] = json!("lua error");
            entry["script_id"] = json!(script.script_id);
            entry["span"] = at(&script.script_id, offset);
            entry["count"] = json!(count);
            ephemeral_error(report, &message);
        }
    }
    for finding in host.findings() {
        if finding.feature == "lua error" && located_faults {
            continue;
        }
        diagnostic(
            report,
            "initialize",
            "LuaFinding",
            "The initializer reported an unsupported or failed operation; authored value omitted",
            "crates/sampler-uvi/src/script.rs",
        );
        let entry = report["diagnostics"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap();
        entry["feature"] = json!(finding.feature);
        entry["count"] = json!(finding.count);
        ephemeral_error(report, &finding.value);
    }
    if failed {
        report["ui"]["status"] = json!("partial");
        report["ui"]["scope"] = json!(
            "Model retained after an initialization fault; it may be incomplete. No rendering or live gestures evaluated"
        );
    }
    stage(
        report,
        "initialize",
        if failed { "failed" } else { "ok" },
        "Actual bounded Lua initialization; typed faults and line observations are retained, native equivalence unverified",
        "crates/sampler-uvi/src/script.rs",
    );
}

fn placeholder_plan(
    report: &mut Value,
    ir: &sampler_ir::Instrument,
    scripts: Vec<sampler_ksp::Script>,
) {
    // Metadata-only streamed assets allocate no sample payload. Their length/rate
    // are deliberately fictitious and cannot establish real sample admission.
    let pcm = ir
        .assets
        .iter()
        .map(|_| sampler_core::Pcm::streamed(48000, 1usize << 32).unwrap())
        .collect();
    match sampler_core::lower::lower_with(
        ir,
        48000,
        pcm,
        &sampler_core::lower::Options {
            mpe: None,
            ..Default::default()
        },
        |_, plan| {
            sampler_ksp::bind_modules(scripts, plan).map_err(|e| {
                sampler_core::lower::LowerError::Behavior {
                    module: "inspected KSP".into(),
                    message: e.to_string(),
                }
            })
        },
    ) {
        Ok(plan) => {
            report["converted"]["placeholder_plan"] = json!({"status":"ok","sample_payload_bytes":0,"sample_metadata":"fictitious 48kHz/2^32frames","controls":plan.controls().len(),"engine_bindings":plan.engine_parameter_bindings().len(),"engine_lookups":plan.engine_lookups().len(),"runtime":"not_run"})
        }
        Err(_) => {
            report["converted"]["placeholder_plan"] = json!({"status":"failed","sample_metadata":"fictitious; failure is not a real-audio load verdict"});
            diagnostic(
                report,
                "placeholder_binding",
                "Lowering",
                "IR could not bind with fictitious streamed assets",
                "crates/sampler-core/src/lower.rs",
            );
        }
    }
}

fn kontakt(opts: &Options, report: &mut Value) {
    let chunks = match sampler_kontakt::read_chunks(&opts.path) {
        Ok(c) => c,
        Err(e) => {
            kontakt_error(report, &e);
            return;
        }
    };
    stage(
        report,
        "container",
        "ok",
        "Decoded Kontakt chunk metadata only",
        "crates/sampler-kontakt/src/container.rs",
    );
    let mut programs = Vec::new();
    if let Some(chunk) = chunks.find_first(0x28) {
        match Program::try_from(chunk) {
            Ok(p) => programs.push((None, p)),
            Err(_) => {
                fail(
                    report,
                    "parse",
                    "ProgramDecode",
                    "Kontakt program record rejected",
                    "vendor/ni-file/src/kontakt/objects/program.rs",
                );
                return;
            }
        }
    } else if let Some(chunk) = chunks.find_first(3) {
        let Ok(bank) = NiBank::try_from(chunk) else {
            fail(
                report,
                "parse",
                "MultiDecode",
                "Kontakt multi bank rejected",
                "vendor/ni-file/src/kontakt/objects/bank.rs",
            );
            return;
        };
        let Ok(slots) = bank.slot_list() else {
            fail(
                report,
                "parse",
                "SlotList",
                "Kontakt multi slots rejected",
                "vendor/ni-file/src/kontakt/objects/bank.rs",
            );
            return;
        };
        let mut slots: Vec<_> = slots.slots.into_iter().collect();
        slots.sort_by_key(|s| s.0);
        for (slot, container) in slots {
            match container.program_list() {
                Ok(list) => programs.extend(
                    list.programs
                        .into_iter()
                        .enumerate()
                        .map(|(within, p)| (Some((slot, within)), p)),
                ),
                Err(_) => diagnostic(
                    report,
                    "parse",
                    "ProgramList",
                    "Multi program list rejected",
                    "vendor/ni-file/src/kontakt/objects/bank.rs",
                ),
            }
        }
    } else {
        fail(
            report,
            "parse",
            "MissingProgram",
            "Input has no instrument or multi program",
            "crates/sampler-kontakt/src/container.rs",
        );
        return;
    }
    let records: Vec<_> = programs
        .iter()
        .enumerate()
        .map(|(i, (slot, p))| source_program(report, &opts.path, p, i, *slot, opts))
        .collect();
    report["metadata"] = json!({"program_count":programs.len(),"programs":records});
    let mut frontend_ok = true;
    let frontends = if opts.summary {
        sampler_kontakt::read_parity_reports_summary(&opts.path)
    } else {
        sampler_kontakt::read_parity_reports(&opts.path)
    };
    match frontends {
        Ok(frontends) => {
            for frontend in frontends {
                if let Some(existing) = report["scripts"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|s| s["id"] == frontend.id)
                {
                    let rejected = frontend.constructs.iter().any(|construct| {
                        construct.name.starts_with("@syntax")
                            && matches!(construct.status, sampler_parity::Status::Unsupported | sampler_parity::Status::Fault)
                    });
                    frontend_ok &= !rejected;
                    let mut frontend = serde_json::to_value(frontend).unwrap();
                    existing["compiler"] = json!({
                        "status": if rejected {"failed"} else {"ok"},
                        "scope": "KSP source frontend and callback lowering without on init execution"
                    });
                    for key in ["tokens", "constructs", "lowered"] {
                        existing[key] = frontend.as_object_mut().unwrap().remove(key).unwrap();
                    }
                }
            }
        }
        Err(e) => {
            frontend_ok = false;
            kontakt_error(report, &e);
        }
    }
    stage(
        report,
        "source_frontend",
        if frontend_ok { "ok" } else { "failed" },
        "Physical scripts parsed/lowered without initialization; bypassed/empty slots retained",
        "crates/sampler-kontakt/src/parity.rs",
    );
    if !opts.initialize {
        return;
    }
    let index = match opts.program.as_deref().unwrap_or("0").parse::<usize>() {
        Ok(i) if i < programs.len() => i,
        _ => {
            fail(
                report,
                "select",
                "ProgramIndex",
                "Requested program index is unavailable",
                "tools/corpus-health/src/inspect.rs",
            );
            return;
        }
    };
    report["item"]["program"] = json!(index);
    let converted = if programs[0].0.is_some() {
        sampler_kontakt::read_program(&opts.path, index)
    } else {
        sampler_kontakt::read(&opts.path)
    };
    let mut converted = match converted {
        Ok(c) => c,
        Err(e) => {
            kontakt_error(report, &e);
            return;
        }
    };
    let init_faults = converted.script_init_faults();
    let script_id = |slot: usize| -> Value {
        report["scripts"]
            .as_array()
            .and_then(|scripts| scripts.iter().find(|script| {
                script["program"].as_u64() == Some(index as u64)
                    && script["slot"].as_u64() == Some(slot as u64)
            }))
            .map_or(Value::Null, |script| script["id"].clone())
    };
    let init_records: Vec<_> = init_faults.iter().map(|fault| {
        let slot = fault.slot.map_or(fault.behavior_index, usize::from);
        (slot, script_id(slot))
    }).collect();
    for (fault, (slot, script_id)) in init_faults.iter().zip(&init_records) {
        diagnostic(report, "initialize", "KspInitializationFault",
            "KONTRA KSP initialization rejected this script; authored error payload omitted",
            "crates/sampler-ksp/src/lib.rs");
        let entry = report["diagnostics"].as_array_mut().unwrap().last_mut().unwrap();
        entry["feature"] = json!(fault.error.builtin.unwrap_or("KSP initialization"));
        entry["location"] = json!(format!("script slot {slot}"));
        entry["script_id"] = script_id.clone();
        entry["program"] = json!(index);
        entry["slot"] = json!(slot);
        entry["behavior_index"] = json!(fault.behavior_index);
        entry["span"] = json!({
            "start": fault.error.offset,
            "end": fault.error.offset,
            "line": fault.error.line,
            "column": fault.error.column
        });
        entry["kind"] = json!(format!("{:?}", fault.error.kind));
        ephemeral_error(report, &fault.error);
    }
    report["evidence"]["initialization"] = json!({
        "status": if init_faults.is_empty() {"ok"} else {"failed"},
        "scope": "KONTRA KSP on init of the selected translated program; native behavior unverified",
        "script_count": converted.instrument.behaviors.len(),
        "fault_count": init_faults.len()
    });
    stage(
        report,
        "initialize",
        if init_faults.is_empty() { "ok" } else { "failed" },
        "Product translation executes KSP init; may read/decode convolution impulse resources, no sample playback",
        "crates/sampler-kontakt/src/library.rs",
    );
    let expected_scripts = converted.instrument.behaviors.len();
    let unsupported_before = converted.instrument.unsupported.len();
    let (scripts, faces, _) = converted.compile_ui(&sampler_kontakt::Options {
        library: Some(opts.path.clone()),
        mpe: None,
        ..Default::default()
    });
    let new_unsupported = &converted.instrument.unsupported[unsupported_before..];
    let projection_faults = new_unsupported.iter().filter(|gap| {
        gap.feature == "script" || gap.feature == "script interface"
    }).count();
    let projection_gaps = new_unsupported.iter().filter(|gap| {
        gap.feature.starts_with("script interface:") || gap.feature == "native interface"
    }).count();
    let resource_gaps = converted.instrument.unsupported.iter().filter(|gap| {
        gap.feature == "performance view" || gap.feature == "performance view control"
    }).count();
    for gap in new_unsupported.iter().filter(|gap| {
        gap.feature == "script" || gap.feature.starts_with("script interface")
            || gap.feature == "native interface"
    }) {
        diagnostic(report, "ui_projection",
            if gap.feature == "script" || gap.feature == "script interface" {"ProjectionFault"} else {"NotModeled"},
            "KONTRA UI projection omitted an authored feature or script; authored value omitted",
            "crates/sampler-kontakt/src/load.rs");
        let entry = report["diagnostics"].as_array_mut().unwrap().last_mut().unwrap();
        entry["feature"] = json!(gap.feature);
        entry["location"] = json!(gap.location);
        if opts.source {
            entry["detail"] = json!(gap.value);
            entry["detail_scope"] = json!("ephemeral authored diagnostic; omitted from source-free reports");
        }
    }
    for gap in converted.instrument.unsupported.iter().filter(|gap| {
        gap.feature == "performance view" || gap.feature == "performance view control"
    }) {
        diagnostic(report, "ui_projection", "NotModeled",
            "Native performance view resource was unavailable or had omitted controls or fields; authored value omitted",
            "crates/sampler-kontakt/src/load.rs");
        let entry = report["diagnostics"].as_array_mut().unwrap().last_mut().unwrap();
        entry["feature"] = json!(gap.feature);
        entry["location"] = json!(gap.location);
        if opts.source {
            entry["detail"] = json!(gap.value);
            entry["detail_scope"] = json!("ephemeral authored diagnostic; omitted from source-free reports");
        }
    }
    let lost_projection = scripts.len() < expected_scripts || faces.len() < scripts.len();
    let ui_status = if expected_scripts != 0 && faces.is_empty() && lost_projection {
        "failed"
    } else if lost_projection || projection_faults != 0 || projection_gaps != 0 || resource_gaps != 0
        || faces.iter().any(|face| !face.unsupported.is_empty()) {
        "partial"
    } else {
        "ok"
    };
    stage(
        report,
        "ui_projection",
        ui_status,
        "Product UI projection consumes translated KSP init state; deferred live persistence callbacks are not activated",
        "crates/sampler-kontakt/src/load.rs",
    );
    report["ui"] = json!({"status":ui_status,"scope":"Initialized model only; no native renderer, resource pixels, gestures or live callbacks","interfaces":interfaces(&faces)});
    instrument(
        report,
        &converted.instrument,
        "Product Kontakt IR after initializer; selected program only",
    );
    placeholder_plan(report, &converted.instrument, scripts);
}

fn uvi(opts: &Options, report: &mut Value) {
    let bank = if opts
        .path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ufs"))
    {
        match super::bank(&opts.path) {
            Ok(b) => Some(b),
            Err(e) => {
                fail(
                    report,
                    "container",
                    "BankAccess",
                    &e.to_string(),
                    "crates/sampler-uvi/src/bank.rs",
                );
                return;
            }
        }
    } else {
        None
    };
    let (xml, files, program) = if let Some(bank) = &bank {
        let Some(program) = opts.program.as_deref() else {
            report["metadata"] = json!({"programs":bank.programs()});
            stage(
                report,
                "container",
                "ok",
                "Bank directory only; choose --program BANK_MEMBER to inspect source",
                "crates/sampler-uvi/src/bank.rs",
            );
            return;
        };
        let (xml, _) = match bank.program(program) {
            Ok(v) => v,
            Err(_) => {
                fail(
                    report,
                    "parse",
                    "ProgramMember",
                    "Bank program could not be read",
                    "crates/sampler-uvi/src/bank.rs",
                );
                return;
            }
        };
        (xml, bank.scripts(), Some(program.to_owned()))
    } else {
        let mut bytes = Vec::new();
        match std::fs::File::open(&opts.path)
            .and_then(|f| f.take(XML_BYTES + 1).read_to_end(&mut bytes))
        {
            Ok(_) if bytes.len() as u64 <= XML_BYTES => {}
            _ => {
                fail(
                    report,
                    "container",
                    "FileRead",
                    "Loose program unreadable or exceeds 32MiB",
                    "tools/corpus-health/src/inspect.rs",
                );
                return;
            }
        }
        let xml = match String::from_utf8(bytes) {
            Ok(x) => x,
            Err(_) => {
                fail(
                    report,
                    "parse",
                    "Utf8",
                    "Loose program is not UTF-8",
                    "tools/corpus-health/src/inspect.rs",
                );
                return;
            }
        };
        (xml, sampler_uvi::script::Scripts::default(), None)
    };
    stage(
        report,
        "container",
        "ok",
        "Source text retained in process memory; no decoded samples",
        "crates/sampler-uvi/src/bank.rs",
    );
    let doc = match sampler_uvi::parse_program_xml(&xml) {
        Ok(d) => d,
        Err(e) => {
            let position = match &e {
                sampler_uvi::Translate::Xml(e) => Some(e.pos()),
                _ => None,
            };
            fail(
                report,
                "parse",
                match &e {
                    sampler_uvi::Translate::Xml(_) => "Xml",
                    _ => "XmlBounds",
                },
                "Program XML rejected by production parser",
                "crates/sampler-uvi/src/lib.rs",
            );
            if let Some(pos) = position {
                report["diagnostics"]
                    .as_array_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()["span"] =
                    json!({"line":pos.row,"column":pos.col,"start":null,"end":null});
            }
            ephemeral_error(report, &e);
            return;
        }
    };
    if opts.summary {
        let mut counts = BTreeMap::<&str, usize>::new();
        for node in doc.descendants().filter(|n| n.is_element()) {
            *counts.entry(node.tag_name().name()).or_default() += 1;
        }
        report["metadata"] = json!({"xml_bytes":xml.len(),"element_counts":counts});
    } else {
        report["metadata"] = json!({"xml_bytes":xml.len(),"elements":doc.descendants().filter(|n|n.is_element()).map(|n|{let p=doc.text_pos_at(n.range().start);json!({"node":n.id().get_usize(),"kind":n.tag_name().name(),"name":n.attribute("Name"),"parent":n.parent_element().map(|p|p.id().get_usize()),"span":{"start":n.range().start,"end":n.range().end,"line":p.row,"column":p.col},"attribute_names":n.attributes().map(|a|a.name()).collect::<Vec<_>>()})}).collect::<Vec<_>>()});
    }
    if opts.source && !opts.summary {
        report["metadata"]["xml"] = json!(xml);
    }
    let mut syntax_ok = true;
    let mut source_lines = BTreeMap::new();
    for (frontend, source) in sampler_uvi::parity::report_sources(&xml, &files) {
        if opts.initialize {
            source_lines.insert(
                frontend.id.clone(),
                std::iter::once(0)
                    .chain(
                        source
                            .bytes()
                            .enumerate()
                            .filter_map(|(i, b)| (b == b'\n').then_some(i + 1)),
                    )
                    .collect(),
            );
        }
        let syntax = sampler_uvi::parity::check_source_syntax(&source);
        if let Err(error) = &syntax {
            syntax_ok = false;
            diagnostic(
                report,
                "source_frontend",
                "LuaSyntax",
                "The executable Luau compiler rejected this Falcon Lua source; authored error payload omitted",
                "crates/sampler-uvi/src/parity.rs",
            );
            let entry = report["diagnostics"]
                .as_array_mut()
                .unwrap()
                .last_mut()
                .unwrap();
            entry["script_id"] = json!(frontend.id);
            entry["span"] = error.line.map_or(Value::Null, |line| {
                let start = if line == 1 {
                    Some(0)
                } else {
                    source
                        .bytes()
                        .enumerate()
                        .filter(|(_, b)| *b == b'\n')
                        .nth(line.saturating_sub(2) as usize)
                        .map(|(i, _)| i + 1)
                };
                json!({"line":line,"start":start,"end":start})
            });
            ephemeral_error(report, error);
        }
        add_script(
            report,
            frontend,
            opts.source.then_some(source.as_ref()),
            None,
            None,
            None,
        );
        report["scripts"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap()["compiler"] = json!({
            "source_language":"Falcon Lua","executable_frontend":"Luau",
            "status":if syntax.is_ok() {"ok"} else {"failed"},
            "scope":"Actual compiler-only syntax validation; bytecode discarded, no execution or native Lua semantic proof"
        });
    }
    stage(
        report,
        "source_frontend",
        if syntax_ok { "ok" } else { "failed" },
        "Falcon Lua source/modules classified and compiled for Luau syntax without execution; lowered entries are source mappings, native Lua semantics unverified",
        "crates/sampler-uvi/src/parity.rs",
    );
    if opts.initialize {
        let origin = if let Some(program) = program.as_deref() {
            sampler_uvi::ScriptedOrigin::Bank(program)
        } else {
            sampler_uvi::ScriptedOrigin::Loose(&opts.path)
        };
        match sampler_uvi::inspect_decoded_scripted_program(xml, files, origin) {
            Ok((ir, host)) => {
                instrument(
                    report,
                    &ir,
                    "Production Falcon XML translation after bounded Lua onLoad overrides; no sample decode",
                );
                stage(
                    report,
                    "translate",
                    "ok",
                    "Converted the initialized XML hierarchy and admitted processor/control graph",
                    "crates/sampler-uvi/src/lib.rs",
                );
                uvi_initialization(report, &host, &source_lines);
            }
            Err(sampler_uvi::ScriptedTranslationFailure::Initialize(error)) => {
                fail(
                    report,
                    "initialize",
                    "LuaInitialization",
                    "Lua initialization rejected; authored payload omitted",
                    "crates/sampler-uvi/src/script.rs",
                );
                ephemeral_error(report, &error);
            }
            Err(sampler_uvi::ScriptedTranslationFailure::Translate(error)) => {
                fail(
                    report,
                    "translate",
                    "Translation",
                    "Production initialized XML translation rejected source; authored error payload omitted",
                    "crates/sampler-uvi/src/lib.rs",
                );
                ephemeral_error(report, &error);
            }
        }
        return;
    }
    let ir = if let Some(bank) = &bank {
        sampler_uvi::translate_decoded_program(bank, program.as_ref().unwrap(), xml)
    } else {
        sampler_uvi::translate_path(&opts.path).map(|p| p.instrument)
    };
    match ir {
        Ok(ir) => {
            instrument(
                report,
                &ir,
                "Production Falcon XML translation, no Lua initialization or sample decode",
            );
            stage(
                report,
                "translate",
                "ok",
                "Converted XML hierarchy and admitted processor/control graph",
                "crates/sampler-uvi/src/lib.rs",
            );
        }
        Err(e) => {
            fail(
                report,
                "translate",
                "Translation",
                "Production XML translation rejected source; authored error payload omitted",
                "crates/sampler-uvi/src/lib.rs",
            );
            ephemeral_error(report, &e);
            return;
        }
    }
}

fn inspect(opts: &Options) -> Value {
    let kind = match opts
        .path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "nki" => "kontakt",
        "nkm" => "kontakt-multi",
        "ufs" => "uvi-bank",
        "uvip" => "uvi-loose",
        _ => "unknown",
    };
    let mut report = base(opts, kind);
    if opts.initialize
        && (std::env::var("KONTRA_INSPECT_READ_ONLY_INIT").as_deref() != Ok("1")
            || std::env::var_os("KONTRA_SCAN_ACTIVE").is_none())
    {
        fail(
            &mut report,
            "initialize_guard",
            "ReadOnlyRequired",
            "Use the read-only tools/library-inspector.py launcher for --initialize",
            "tools/corpus-health/src/inspect.rs",
        );
        return report;
    }
    match kind {
        "kontakt" | "kontakt-multi" => kontakt(opts, &mut report),
        "uvi-loose" | "uvi-bank" => uvi(opts, &mut report),
        _ => fail(
            &mut report,
            "select",
            "Format",
            "Expected .nki, .nkm, .uvip or .ufs input",
            "tools/corpus-health/src/inspect.rs",
        ),
    }
    let mut source_gaps = false;
    let mut diagnostics = Vec::new();
    for script in report["scripts"].as_array().unwrap() {
        for construct in script["constructs"].as_array().unwrap() {
            source_gaps |= construct["status"] != "Implemented";
            if matches!(
                construct["status"].as_str(),
                Some("Unsupported" | "Stub" | "Fault")
            ) {
                diagnostics.push(json!({"stage":"source_frontend","category":construct["status"],"feature":construct["name"],"location":null,"script_id":script["id"],"span":construct["span"],"detail":construct["detail"],"owner":construct["owner"],"spec":construct["spec"]}));
            }
        }
    }
    report["diagnostics"]
        .as_array_mut()
        .unwrap()
        .extend(diagnostics);
    if report["status"] == "ok"
        && (source_gaps || !report["diagnostics"].as_array().unwrap().is_empty())
    {
        report["status"] = json!("partial");
    }
    for (name, scope) in [
        (
            "runtime",
            "Callbacks, note dispatch and gestures were not executed",
        ),
        (
            "audio",
            "Samples were not auditioned or compared to native hosts",
        ),
    ] {
        stage(
            &mut report,
            name,
            "skipped",
            scope,
            "tools/corpus-health/src/inspect.rs",
        );
    }
    report
}

fn compact(mut report: Value) -> Value {
    report["mode"]["summary"] = json!(true);
    let scripts=report["scripts"].as_array().unwrap().iter().map(|script| {
        let mut counts=std::collections::BTreeMap::<String,usize>::new();
        for construct in script["constructs"].as_array().unwrap(){*counts.entry(construct["status"].as_str().unwrap_or("unknown").to_owned()).or_default()+=1;}
        json!({"id":script["id"],"title":script["title"],"lang":script["lang"],"source_len":script["source_len"],"program":script["program"],"slot":script["slot"],"bypassed":script["bypassed"],"compiler":script["compiler"],"construct_count":script["constructs"].as_array().unwrap().len(),"status_counts":counts,"tokens":[],"constructs":[],"lowered":[]})
    }).collect::<Vec<_>>();
    report["scripts"] = json!(scripts);
    if let Some(elements) = report["metadata"]["elements"].as_array() {
        let mut counts = std::collections::BTreeMap::<String, usize>::new();
        for node in elements {
            *counts
                .entry(node["kind"].as_str().unwrap_or("unknown").to_owned())
                .or_default() += 1;
        }
        report["metadata"] =
            json!({"xml_bytes":report["metadata"]["xml_bytes"],"element_counts":counts});
    } else if let Some(programs) = report["metadata"]["programs"].as_array() {
        if programs.iter().all(|p| p.is_object()) {
            report["metadata"] = json!({"program_count":programs.len(),"programs":programs.iter().map(|p|json!({"program":p["program"],"rack_slot":p["rack_slot"],"program_within_slot":p["program_within_slot"],"name":p["name"],"version":p["version"],"group_count":p["groups"].as_array().map(Vec::len),"zone_count":p["zones"]["count"],"record_count":p["records"].as_array().map(Vec::len)})).collect::<Vec<_>>()});
        }
    }
    if let Some(converted) = report["converted"].as_object_mut() {
        converted.retain(|key, _| {
            matches!(
                key.as_str(),
                "status" | "scope" | "summary" | "placeholder_plan"
            )
        });
    }
    let faces=report["ui"]["interfaces"].as_array().unwrap().iter().map(|f|json!({"index":f["index"],"source":f["source"],"pages":f["pages"].as_array().map(Vec::len),"widgets":f["widgets"].as_array().map(Vec::len),"assets":f["assets"].as_array().map(Vec::len),"unsupported":f["unsupported"].as_array().map(Vec::len)})).collect::<Vec<_>>();
    report["ui"]["interfaces"] = json!(faces);
    report
}

fn manifest_item(kind: &str, id: &str) -> Result<Options, &'static str> {
    match kind {
        "uvi-program" => {
            let (path, program) = id
                .split_once("::")
                .ok_or("UVI manifest item needs BANK::MEMBER")?;
            Ok(Options {
                path: path.into(),
                program: Some(program.into()),
                summary: true,
                ..Default::default()
            })
        }
        "kontakt" | "kontakt-multi" | "uvi-loose" => {
            let (path, program) = if kind == "kontakt-multi" {
                id.rsplit_once('#')
                    .map_or((id, None), |(p, n)| (p, Some(n.to_owned())))
            } else {
                (id, None)
            };
            Ok(Options {
                path: path.into(),
                program,
                summary: true,
                ..Default::default()
            })
        }
        _ => Err("unknown inspector manifest kind"),
    }
}

pub(super) fn run_list(args: &[String]) -> i32 {
    let [manifest] = args else {
        eprintln!("inspect-list needs a kind<TAB>id manifest; output is source-free compact JSONL");
        return 2;
    };
    let text = match std::fs::read_to_string(manifest) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("manifest read failed: {:?}", e.kind());
            return 2;
        }
    };
    let mut failed = false;
    for (index, line) in text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
    {
        let opts = match line
            .split_once('\t')
            .ok_or("manifest line needs kind<TAB>id")
            .and_then(|(kind, id)| manifest_item(kind, id))
        {
            Ok(o) => o,
            Err(e) => {
                eprintln!("manifest line {}: {e}", index + 1);
                failed = true;
                continue;
            }
        };
        let report = compact(inspect(&opts));
        failed |= report["status"] == "failed";
        println!("{}", serde_json::to_string(&report).unwrap());
    }
    if failed { 1 } else { 0 }
}

pub(super) fn run(args: &[String]) -> i32 {
    let opts = match options(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let report = inspect(&opts);
    let report = if opts.summary {
        compact(report)
    } else {
        report
    };
    let failed = report["status"] == "failed";
    println!("{}", serde_json::to_string(&report).unwrap());
    if failed { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "kontra-inspect-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture(temp: &Temp, xml: &str, source: bool) -> Options {
        let path = temp.0.join("Authored.uvip");
        std::fs::write(&path, xml).unwrap();
        Options {
            path,
            source,
            ..Default::default()
        }
    }

    fn scripted_nki(temp: &Temp, source: &str) -> PathBuf {
        use ni_file::{
            kontakt::{Chunk, KontaktChunks},
            nis::{ItemContainer, ItemData, ItemDataHeader, ItemHeader},
        };
        let item = |domain_id: [u8; 4], item_id: u32, data: Vec<u8>| {
            let base = ItemData {
                header: ItemDataHeader { length: 0, domain_id: *b"NISD", item_id: 1, version: 1 },
                inner: None,
                data: 1u32.to_le_bytes().to_vec(),
            };
            ItemContainer {
                header: ItemHeader {
                    length: 0, magic: b"hsin".to_vec(), header_flags: 0,
                    reserved: 0, uuid: vec![0; 16],
                },
                data: ItemData {
                    header: ItemDataHeader { length: 0, domain_id, item_id, version: 1 },
                    inner: (item_id != 1).then_some(Box::new(base)),
                    data,
                },
                children: vec![], child_headers: vec![], trailing_data: vec![],
            }
        };
        let mut script = vec![0, 0x50, 0];
        script.extend((source.len() as u32).to_le_bytes());
        script.extend(source.as_bytes());
        script.extend([0; 3]);
        script.extend(0u32.to_le_bytes());
        script.extend(u32::MAX.to_le_bytes());
        script.extend(0u32.to_le_bytes());
        script.extend(0u32.to_le_bytes()); // persistent-table count
        let mut children = Vec::new();
        for child in [
            Chunk { id: 0x33, data: vec![0; 4] },
            Chunk { id: 0x34, data: vec![0; 4] },
            Chunk { id: 6, data: script },
        ] {
            child.write(&mut children).unwrap();
        }
        let mut program = vec![1];
        program.extend(0xafu16.to_le_bytes());
        program.extend(0u32.to_le_bytes());
        program.extend(70u32.to_le_bytes());
        program.extend([0; 70]);
        program.extend((children.len() as u32).to_le_bytes());
        program.extend(children);
        let mut table = 2u16.to_le_bytes().to_vec();
        table.extend([0; 12]);
        let mut preset = Vec::new();
        KontaktChunks(vec![Chunk { id: 0x28, data: program }, Chunk { id: 0x4b, data: table }])
            .write(&mut preset).unwrap();
        let mut chunk = 1u32.to_le_bytes().to_vec();
        chunk.extend(0u32.to_le_bytes());
        chunk.extend(1u32.to_le_bytes());
        chunk.extend((preset.len() as u64).to_le_bytes());
        chunk.extend(preset);
        let mut chunk_item = Vec::new();
        item(*b"NISD", 0x6d, chunk).write(&mut chunk_item).unwrap();
        let mut subtree = 1u32.to_le_bytes().to_vec();
        subtree.push(0);
        subtree.extend(chunk_item);
        let mut encryption = item(*b"NISD", 0x74, vec![1, 0, 0, 0, 0]);
        encryption.data.inner = Some(Box::new(item(*b"NISD", 0x73, subtree).data));
        let mut root = item(*b"NIK4", 3, vec![]);
        root.children.push(encryption);
        root.child_headers.push([0; 12]);
        let mut repository = item(*b"NISD", 0x76, vec![]);
        repository.children.push(root);
        repository.child_headers.push([0; 12]);
        let path = temp.0.join("Scripted.nki");
        let mut bytes = Vec::new();
        repository.write(&mut bytes).unwrap();
        std::fs::write(&path, bytes).unwrap();
        sampler_kontakt::read_chunks(&path).unwrap();
        path
    }

    const XML: &str = r#"<Program Name="Authored"><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="missing.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers><Inserts><UnmodeledFixture/></Inserts><EventProcessors><ScriptProcessor><script><![CDATA[function onInit() error("PRIVATE_DO_NOT_EXECUTE") end]]></script></ScriptProcessor></EventProcessors></Program>"#;

    #[test]
    fn summary_ksp_frontend_matches_full_report_after_compaction() {
        let source = "on init declare $x := abs(-7) end on\non note set_global_key(1) end on";
        let full = sampler_ksp::parity_report(source, "ksp:slot0", "Script slot 0");
        let summary = sampler_ksp::parity_report_summary_with(
            source,
            "ksp:slot0",
            "Script slot 0",
            sampler_ksp::Limits::LIBRARY,
            &sampler_ksp::Environment::default(),
        );
        assert_eq!(summary.constructs, full.constructs);
        assert!(summary.tokens.is_empty());
        assert!(summary.lowered.is_empty());
        let opts = Options {
            path: "Synthetic.nki".into(),
            summary: true,
            ..Default::default()
        };
        let mut full_report = base(&opts, "kontakt");
        let mut summary_report = base(&opts, "kontakt");
        add_script(&mut full_report, full, None, Some(0), Some(0), Some(false));
        add_script(
            &mut summary_report,
            summary,
            None,
            Some(0),
            Some(0),
            Some(false),
        );
        assert_eq!(compact(full_report), compact(summary_report));
    }

    #[test]
    fn static_inspection_converts_metadata_without_running_lua_or_reading_samples() {
        let temp = Temp::new();
        let opts = fixture(&temp, XML, false);
        let report = inspect(&opts);
        assert_ne!(report["status"], "failed");
        assert_eq!(
            report["converted"]["summary"]["zones"], 0,
            "a missing sample is an omitted source zone, not admitted audio"
        );
        assert!(
            report["metadata"]["elements"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n["kind"] == "SamplePlayer")
        );
        assert_eq!(report["ui"]["status"], "not_evaluated");
        assert_eq!(report["evidence"]["runtime"], "not_run");
        assert_eq!(report["evidence"]["audio"], "not_run");
        assert_eq!(report["evidence"]["native_equivalence"], "unknown");
        assert!(
            report["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["stage"] == "translate")
        );
        assert!(!report.to_string().contains("PRIVATE_DO_NOT_EXECUTE"));
        assert!(report["scripts"][0].get("source").is_none());
        assert!(report["metadata"].get("xml").is_none());
        assert_eq!(
            std::fs::read_dir(&temp.0).unwrap().count(),
            1,
            "inspection must not create cache/output files"
        );
    }

    #[test]
    fn ephemeral_source_is_exact_and_source_maps_keep_owner_and_span() {
        let temp = Temp::new();
        let opts = fixture(&temp, XML, true);
        let report = inspect(&opts);
        assert_eq!(report["metadata"]["xml"], XML);
        assert_eq!(
            report["scripts"][0]["source"],
            "function onInit() error(\"PRIVATE_DO_NOT_EXECUTE\") end"
        );
        assert!(
            report["metadata"]["elements"]
                .as_array()
                .unwrap()
                .iter()
                .all(|n| n["span"]["line"].as_u64().unwrap() > 0)
        );
        assert!(report["scripts"][0]["tokens"].as_array().is_some());
        assert!(
            report["scripts"][0]["constructs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["owner"].is_string())
        );
    }

    #[test]
    fn malformed_xml_has_a_typed_parse_failure_without_payload() {
        let temp = Temp::new();
        let opts = fixture(&temp, "<Program><PRIVATE_BAD_ELEMENT>", false);
        let report = inspect(&opts);
        assert_eq!(report["status"], "failed");
        assert_eq!(report["diagnostics"][0]["stage"], "parse");
        assert_eq!(report["diagnostics"][0]["category"], "Xml");
        assert!(!report.to_string().contains("PRIVATE_BAD_ELEMENT"));
    }

    #[test]
    fn malformed_lua_is_rejected_by_the_executable_compiler_without_running_init() {
        let temp = Temp::new();
        let xml = "<Program><EventProcessors><ScriptProcessor><script><![CDATA[function onInit(\nend]]></script></ScriptProcessor></EventProcessors></Program>";
        let report = inspect(&fixture(&temp, xml, false));
        assert_eq!(report["status"], "failed");
        assert_eq!(report["scripts"][0]["compiler"]["status"], "failed");
        assert_eq!(
            report["scripts"][0]["compiler"]["source_language"],
            "Falcon Lua"
        );
        assert_eq!(
            report["scripts"][0]["compiler"]["executable_frontend"],
            "Luau"
        );
        let finding = report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["category"] == "LuaSyntax")
            .unwrap();
        assert_eq!(finding["script_id"], report["scripts"][0]["id"]);
        assert!(
            finding["span"]["line"]
                .as_u64()
                .is_some_and(|line| line > 0)
        );
        assert_eq!(
            report["converted"]["status"], "ok",
            "script rejection is separate from XML translation"
        );
        assert_eq!(report["ui"]["status"], "not_evaluated");
        assert_eq!(report["evidence"]["runtime"], "not_run");
        assert_eq!(report["evidence"]["native_equivalence"], "unknown");
        assert!(report["scripts"][0].get("source").is_none());
    }

    #[test]
    fn uvi_projected_ui_omissions_are_visible_in_problems_with_widget_coordinates() {
        let host = sampler_uvi::script::ScriptHost::new(
            "<Program><EventProcessors><ScriptProcessor><script>XY('AuthoredX','PRIVATE_UNBOUND_AXIS')</script></ScriptProcessor></EventProcessors></Program>",
            (), Default::default(),
        ).unwrap();
        let face = host.interface();
        assert!(face.unsupported.iter().any(|u| u.feature == "XY paramY"));
        let opts = Options {
            path: "Authored.uvip".into(),
            ..Default::default()
        };
        let mut report = base(&opts, "uvi-loose");
        uvi_interface(&mut report, &face);
        assert_eq!(report["ui"]["status"], "partial");
        let finding = report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["feature"] == "XY paramY")
            .unwrap();
        assert_eq!(finding["stage"], "ui_projection");
        assert_eq!(finding["location"], "interface 0 widget 0");
        assert_eq!(finding["category"], "NotModeled");
        assert_eq!(
            finding["detail"],
            "UI projection does not model this source feature; authored value omitted"
        );
        assert_eq!(report["evidence"]["native_equivalence"], "unknown");
    }

    #[test]
    fn retained_lua_init_faults_fail_the_init_stage_and_preserve_safe_line_evidence() {
        let source = "function onInit()\n error(\"PRIVATE_INIT_ERROR\")\nend";
        let xml = format!(
            "<Program><EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors></Program>"
        );
        let host =
            sampler_uvi::script::ScriptHost::new_inspected(&xml, (), Default::default()).unwrap();
        let coverage = host.coverage();
        assert_eq!(coverage.len(), 1);
        let starts = std::iter::once(0)
            .chain(
                source
                    .bytes()
                    .enumerate()
                    .filter_map(|(i, b)| (b == b'\n').then_some(i + 1)),
            )
            .collect::<Vec<_>>();
        let source_lines = BTreeMap::from([(coverage[0].script_id.clone(), starts)]);
        let opts = Options {
            path: "Authored.uvip".into(),
            initialize: true,
            ..Default::default()
        };
        let mut report = base(&opts, "uvi-loose");
        uvi_initialization(&mut report, &host, &source_lines);
        assert_eq!(report["evidence"]["initialization"]["status"], "failed");
        assert_eq!(
            report["evidence"]["initialization"]["fault_counts"]["Lua"],
            1
        );
        assert_eq!(report["ui"]["status"], "partial");
        assert!(
            report["stages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["name"] == "initialize" && s["status"] == "failed")
        );
        let faults = report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["feature"] == "lua error")
            .collect::<Vec<_>>();
        assert_eq!(
            faults.len(),
            1,
            "a located fault must not also appear as the host's generic duplicate"
        );
        assert_eq!(faults[0]["category"], "LuaInitializationFault");
        assert_eq!(faults[0]["script_id"], coverage[0].script_id);
        assert_eq!(faults[0]["span"]["line"], 2);
        assert_eq!(faults[0]["count"], 1);
        assert!(!report.to_string().contains("PRIVATE_INIT_ERROR"));
        assert!(
            !report["evidence"]["initialization"]["line_observations"][0]["hits"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(report["evidence"]["runtime"], "not_run");
        assert_eq!(report["evidence"]["native_equivalence"], "unknown");
        let mut ephemeral = base(
            &Options {
                source: true,
                ..opts
            },
            "uvi-loose",
        );
        uvi_initialization(&mut ephemeral, &host, &source_lines);
        assert!(ephemeral.to_string().contains("PRIVATE_INIT_ERROR"));
    }

    #[test]
    fn initialized_uvi_reports_the_post_onload_gain_in_production_ir() {
        let temp = Temp::new();
        let xml = r#"<Program><Inserts><Gain Volume="1"/></Inserts><EventProcessors><ScriptProcessor><script><![CDATA[local gain=Program.inserts[1]
function onInit() gain:setParameter('Volume',0.25) end]]></script></ScriptProcessor></EventProcessors></Program>"#;
        let mut opts = fixture(&temp, xml, true);
        let mut static_report = base(&opts, "uvi-loose");
        uvi(&opts, &mut static_report);
        opts.initialize = true;
        let mut initialized = base(&opts, "uvi-loose");
        uvi(&opts, &mut initialized);
        let gain = |report: &Value| {
            report["converted"]["chains"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|chain| {
                    ["pre_amplitude", "post_amplitude"]
                        .into_iter()
                        .flat_map(move |lane| chain[lane].as_array().into_iter().flatten())
                })
                .find_map(|processor| processor["Gain"]["Linear"].as_f64())
                .unwrap()
        };
        assert_eq!(gain(&static_report), 1.0);
        assert_eq!(gain(&initialized), 0.25);
        assert_eq!(initialized["evidence"]["initialization"]["status"], "ok");
        assert_eq!(initialized["metadata"]["xml"], xml);
        assert!(initialized["scripts"][0]["source"].is_string());
        assert_eq!(initialized["evidence"]["native_equivalence"], "unknown");
    }

    #[test]
    fn initialized_uvi_pipeline_keeps_located_faults_and_source_opt_in() {
        let temp = Temp::new();
        let xml = r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[function onInit()
 error('SYNTHETIC_PRIVATE_FAULT')
end]]></script></ScriptProcessor></EventProcessors></Program>"#;
        let mut opts = fixture(&temp, xml, false);
        opts.initialize = true;
        let mut report = base(&opts, "uvi-loose");
        uvi(&opts, &mut report);
        assert_eq!(report["evidence"]["initialization"]["status"], "failed");
        assert_eq!(report["converted"]["status"], "ok");
        assert!(report["metadata"].get("xml").is_none());
        assert!(report["scripts"][0].get("source").is_none());
        let fault = report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["category"] == "LuaInitializationFault")
            .unwrap();
        assert_eq!(fault["span"]["line"], 2);
        assert!(!report.to_string().contains("SYNTHETIC_PRIVATE_FAULT"));
    }

    #[test]
    fn initialized_mode_requires_the_read_only_launcher() {
        if std::env::var("KONTRA_INSPECT_READ_ONLY_INIT").as_deref() == Ok("1") {
            return;
        }
        let opts = Options {
            path: "not-read.nki".into(),
            initialize: true,
            ..Default::default()
        };
        let report = inspect(&opts);
        assert_eq!(report["status"], "failed");
        assert_eq!(report["diagnostics"][0]["category"], "ReadOnlyRequired");
    }

    #[test]
    fn scripted_nki_frontend_rejection_marks_compiler_stage_failed() {
        let temp = Temp::new();
        let path = scripted_nki(&temp, "on init\n declare $broken(\nend on");
        let opts = Options { path, ..Default::default() };
        let report = inspect(&opts);
        let frontend = report["stages"].as_array().unwrap().iter()
            .find(|stage| stage["name"] == "source_frontend").unwrap();
        assert_eq!(frontend["status"], "failed");
        assert_eq!(report["status"], "failed");
        assert_eq!(report["scripts"][0]["compiler"]["status"], "failed");
        assert!(report["scripts"][0]["constructs"].as_array().unwrap().iter()
            .any(|construct| construct["name"].as_str().unwrap_or("").starts_with("@syntax")));
    }

    #[test]
    fn scripted_nki_init_fault_keeps_physical_source_location_and_failed_ui() {
        let temp = Temp::new();
        let path = scripted_nki(&temp, "on init\n declare $signal := 1\n declare $value := -1\n change_listener_par($signal,$value)\nend on");
        let opts = Options { path, initialize: true, ..Default::default() };
        let mut report = base(&opts, "kontakt");
        kontakt(&opts, &mut report);
        let stage_status = |name| report["stages"].as_array().unwrap().iter()
            .find(|stage| stage["name"] == name).unwrap()["status"].clone();
        assert_eq!(stage_status("source_frontend"), "ok", "{:?}", report["scripts"][0]["constructs"]);
        assert_eq!(stage_status("initialize"), "failed");
        assert_eq!(stage_status("ui_projection"), "failed");
        assert_eq!(report["status"], "failed");
        assert_eq!(report["ui"]["status"], "failed");
        assert!(report["ui"]["interfaces"].as_array().unwrap().is_empty());
        let fault = report["diagnostics"].as_array().unwrap().iter()
            .find(|entry| entry["category"] == "KspInitializationFault").unwrap();
        assert_eq!(fault["script_id"], "ksp:slot0");
        assert_eq!(fault["program"], 0);
        assert_eq!(fault["slot"], 0);
        assert_eq!(fault["span"]["line"], 4);
        assert!(fault["span"]["start"].as_u64().unwrap() > 0);
        assert!(fault["span"]["column"].as_u64().unwrap() > 0);
        assert_eq!(report["evidence"]["initialization"]["fault_count"], 1);
        assert!(!report.to_string().contains("invalid listener"));
    }

    #[test]
    fn scripted_nki_ui_omission_marks_projection_partial() {
        let temp = Temp::new();
        let source = "on init\n declare ui_knob $Cut(0,100,1)\n set_control_par(get_ui_id($Cut),$CONTROL_PAR_HIDE,$HIDE_PART_MOD_LIGHT)\nend on";
        let path = scripted_nki(&temp, source);
        let opts = Options { path, initialize: true, ..Default::default() };
        let mut report = base(&opts, "kontakt");
        kontakt(&opts, &mut report);
        assert_eq!(report["ui"]["status"], "partial");
        assert_eq!(report["status"], "partial");
        assert!(report["diagnostics"].as_array().unwrap().iter().any(|entry|
            entry["stage"] == "ui_projection" && entry["feature"] == "script interface: $HIDE_PART_MOD_LIGHT"));
    }

    #[test]
    fn skipped_nckp_fields_mark_initialized_ui_partial() {
        let temp = Temp::new();
        let view_dir = temp.0.join("Resources/performance_view");
        std::fs::create_dir_all(&view_dir).unwrap();
        let view = r#"{"value":{"performanceView":{"controls":[{"index":3,"value":{"common":{"id":"Cut"},"text":{"font":{"custom":"PRIVATE_CUSTOM_FONT"}}}},{"index":999,"value":{"common":{"id":"Unknown"}}}]}}}"#;
        std::fs::write(view_dir.join("Piano.nckp"), view).unwrap();
        let path = scripted_nki(&temp, "on init\n load_performance_view(\"Piano\")\nend on");
        let opts = Options { path, initialize: true, ..Default::default() };
        let mut report = base(&opts, "kontakt");
        kontakt(&opts, &mut report);
        assert_eq!(report["evidence"]["initialization"]["status"], "ok");
        assert_eq!(report["ui"]["status"], "partial");
        assert_eq!(report["status"], "partial");
        assert!(report["diagnostics"].as_array().unwrap().iter().any(|entry|
            entry["stage"] == "ui_projection" && entry["feature"] == "performance view control"));
        // The font is a resolved bitmap-font asset now; skipped values must still stay
        // out of the diagnostics.
        assert!(!report["diagnostics"].to_string().contains("PRIVATE_CUSTOM_FONT"));
    }

    #[test]
    fn physical_empty_and_bypassed_slots_have_unique_multi_coordinates() {
        use ni_file::kontakt::{Chunk, StructuredObject};
        let temp = Temp::new();
        let source = "on init declare $x end on";
        let chunk = |source: &str, bypassed: bool| {
            let mut data = vec![0, 0x50, 0];
            data.extend((source.len() as u32).to_le_bytes());
            data.extend(source.as_bytes());
            data.extend([0, 0, u8::from(bypassed)]);
            data.extend(0u32.to_le_bytes());
            data.extend(u32::MAX.to_le_bytes());
            data.extend(0u32.to_le_bytes());
            data.extend(0u32.to_le_bytes()); // persistent-table count
            Chunk { id: 6, data }
        };
        let program = Program(StructuredObject {
            version: 0xaf,
            public_data: vec![0; 70],
            private_data: vec![],
            children: vec![chunk("", false), chunk(source, true)],
        });
        let opts = Options {
            path: temp.0.join("Authored.nkm"),
            source: true,
            ..Default::default()
        };
        let mut report = base(&opts, "kontakt-multi");
        source_program(&mut report, &opts.path, &program, 0, Some((4, 0)), &opts);
        source_program(&mut report, &opts.path, &program, 1, Some((4, 1)), &opts);
        let scripts = report["scripts"].as_array().unwrap();
        assert_eq!(scripts.len(), 4);
        assert_eq!(scripts[0]["id"], "ksp:program4:0:slot0");
        assert_eq!(scripts[2]["id"], "ksp:program4:1:slot0");
        assert_eq!(scripts[0]["source_len"], 0);
        assert_eq!(scripts[1]["bypassed"], true);
        assert_eq!(scripts[1]["source"], source);
    }

    #[test]
    fn ephemeral_kontakt_view_source_has_slot_path_and_parse_provenance() {
        use ni_file::kontakt::{Chunk, StructuredObject};
        let temp = Temp::new();
        let view_dir = temp.0.join("Resources/performance_view");
        std::fs::create_dir_all(&view_dir).unwrap();
        let view =
            r#"{"value":{"performanceView":{"size":{"width":800,"height":500},"controls":[{"index":3,"value":{"common":{"id":"Cut"},"text":{"font":{"custom":"PRIVATE_CUSTOM_FONT"}}}},{"index":999,"value":{"common":{"id":"Unknown"}}}]}}}"#;
        std::fs::write(view_dir.join("Piano.nckp"), view).unwrap();
        let source = "on init\n load_performance_view(\"Piano\")\nend on";
        let mut data = vec![0, 0x50, 0];
        data.extend((source.len() as u32).to_le_bytes());
        data.extend(source.as_bytes());
        data.extend([0; 3]);
        data.extend(0u32.to_le_bytes());
        data.extend(u32::MAX.to_le_bytes());
        data.extend(0u32.to_le_bytes());
        data.extend(0u32.to_le_bytes()); // persistent-table count
        let program = Program(StructuredObject {
            version: 0xaf,
            public_data: vec![0; 70],
            private_data: vec![],
            children: vec![Chunk { id: 6, data }],
        });
        let path = temp.0.join("Authored.nki");
        let opts = Options {
            path: path.clone(),
            source: true,
            ..Default::default()
        };
        let mut report = base(&opts, "kontakt");
        let record = source_program(&mut report, &path, &program, 0, None, &opts);
        let original = &record["ui_resources"][0];
        assert_eq!(original["script_id"], "ksp:slot0");
        assert_eq!(original["slot"], 0);
        assert_eq!(original["path"], "Resources/performance_view/Piano.nckp");
        assert_eq!(original["parse_status"], "ok");
        assert_eq!(original["skipped"], json!({"total":1,"omitted_source_fields":0,"unsupported_control_types":1}));
        assert_eq!(original["source"], view);

        let opts = Options {
            source: false,
            ..opts
        };
        let mut report = base(&opts, "kontakt");
        let record = source_program(&mut report, &path, &program, 0, None, &opts);
        assert!(record.get("ui_resources").is_none());
        assert!(!report.to_string().contains(view));
    }

    #[test]
    fn cli_rejects_missing_values_and_extra_paths() {
        let strings = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(options(&strings(&["a.nki", "--program"])).is_err());
        assert!(options(&strings(&["a.nki", "b.nki"])).is_err());
        assert!(options(&strings(&["a.nki", "--write-source"])).is_err());
        let parsed = options(&strings(&["a.nkm", "--program", "1", "--ephemeral-source"])).unwrap();
        assert_eq!(parsed.program.as_deref(), Some("1"));
        assert!(parsed.source);
        assert!(!parsed.initialize);
    }

    #[test]
    fn compact_reports_keep_evidence_and_gap_counts_without_source_payload() {
        let temp = Temp::new();
        let mut opts = fixture(&temp, XML, false);
        let full = inspect(&opts);
        let summary = compact(full.clone());
        opts.summary = true;
        assert_eq!(compact(inspect(&opts)), summary);
        assert_eq!(summary["status"], full["status"]);
        assert_eq!(summary["evidence"], full["evidence"]);
        assert_eq!(
            summary["scripts"][0]["compiler"],
            full["scripts"][0]["compiler"]
        );
        assert_eq!(summary["diagnostics"], full["diagnostics"]);
        assert_eq!(
            summary["scripts"][0]["source_len"],
            full["scripts"][0]["source_len"]
        );
        assert_eq!(
            summary["scripts"][0]["construct_count"],
            full["scripts"][0]["constructs"].as_array().unwrap().len()
        );
        assert_eq!(summary["metadata"]["element_counts"]["SamplePlayer"], 1);
        assert!(summary["converted"].get("zones").is_none());
        assert!(!summary.to_string().contains("PRIVATE_DO_NOT_EXECUTE"));
    }

    #[test]
    fn manifest_program_identifiers_are_unambiguous() {
        let uvi = manifest_item("uvi-program", "/owned/Bank.ufs::Presets/A.uvip").unwrap();
        assert_eq!(uvi.path, Path::new("/owned/Bank.ufs"));
        assert_eq!(uvi.program.as_deref(), Some("Presets/A.uvip"));
        let multi = manifest_item("kontakt-multi", "/owned/Rack.nkm#2").unwrap();
        assert_eq!(multi.path, Path::new("/owned/Rack.nkm"));
        assert_eq!(multi.program.as_deref(), Some("2"));
        assert!(manifest_item("uvi-program", "/owned/Bank.ufs").is_err());
        assert!(!multi.initialize);
        assert!(!uvi.source);
    }
}
