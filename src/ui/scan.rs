//! Shared scanner's pinned-v1 adapter; no decrypted source or library assets are persisted.
use super::*;
use crate::{
    artwork,
    engine::{Bank, Engine},
    scan_metrics as metrics,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn original(part: &PartView, load_started: Instant, out: &Path, prefix: &str) -> anyhow::Result<Value> {
    use moose::mui::mui::vello::{
        self,
        vello_cpu::{Pixmap, RenderContext, Resources},
    };
    let p = Arc::new(SamplerParams::new());
    let i = part.instrument.as_ref().unwrap();
    p.selection.write().unwrap().parts.push(Part {
        path: i.path.to_string_lossy().into(),
        group: i.first_playable_group().unwrap_or(0) as u32,
        view: 1,
        ..Default::default()
    });
    {
        let mut view = p.shared.view.lock().unwrap();
        view.files = Arc::new(vec![i.path.clone()]);
        view.parts[0] = part.clone();
    }
    let mut ui = theme::ui();
    let mut build = build(
        &p,
        Arc::default(),
        Arc::default(),
        Arc::default(),
        Arc::default(),
    );
    let mut bridge = Bridge::new(p.clone());
    for _ in 0..4 {
        let root = build(&mut ui, &mut bridge);
        ui.frame(
            root,
            Some(Size::new(1180., 900.)),
            Input::default(),
            1. / 60.,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    let mut ctx = RenderContext::new(1180, 900);
    let mut resources = Resources::default();
    vello::paint(
        &mut vello::Cpu {
            ctx: &mut ctx,
            resources: &mut resources,
            cache: &mut vello::Cache::default(),
        },
        ui.scene().ok_or_else(|| anyhow::anyhow!("no scene"))?,
        vello::kurbo::Affine::IDENTITY,
    )
    .map_err(|_| anyhow::anyhow!("paint failed"))?;
    ctx.flush();
    let mut pix = Pixmap::new(1180, 900);
    ctx.render(&mut pix, &mut resources);
    let first_frame_ms=load_started.elapsed().as_secs_f64()*1000.;
    let rgba: Vec<_> = pix
        .take_unpremultiplied()
        .iter()
        .flat_map(|p| [p.r, p.g, p.b, p.a])
        .collect();
    let mut record = metrics::pixels(&rgba);
    record["ui_first_frame_ms"]=json!(first_frame_ms);
    record["extent"] =
        json!("full v1 editor; blank uses authored visibility, not shell pixel uniformity");
    if std::env::var_os("KONTRA_SCAN_SHOTS").is_some() {
        let path = out.join(format!("{prefix}-original.png"));
        moose::core::screenshot::save_png(&path, &rgba, 1180, 900);
        record["shot"] = json!(path);
    }
    Ok(record)
}

pub fn one(id: &str, out: &Path) -> Value {
    let mut first_audio_ms=None;
    let path = Path::new(id);
    let mut result = json!({"loads":"no","ui":"error","plays_note":"no","controls_bound":"0/0","stage":"parse","programs":[],"cache_state":"cold","cache_state_basis":"scanner disables product parsed/header cache reads and writes; OS page cache uncontrolled"});
    PHASES.with(|p| p.borrow_mut().clear());
    result["metadata"] = match import::scan_chunks(path) {
        Ok(c) => metrics::metadata::inspect(&c.0),
        Err(e) => metrics::error("metadata", e),
    };
    metrics::checkpoint(out, &result);
    let programs = if import::is_multi(path) {
        match import::read_multi(path) {
            Ok(m) => m.parts.into_iter().map(|p| p.program).collect(),
            Err(e) => {
                result["failure"] = metrics::error("multi parse", e);
                result["reason"] = json!("multi parse failed");
                return result;
            }
        }
    } else {
        vec![0]
    };
    let (
        mut all_load,
        mut bound,
        mut interactive,
        mut missing,
        mut error,
        mut blank,
        mut has_ui,
        mut any_heard,
    ) = (true, 0, 0, false, false, false, false, false);
    let mut load_ms = 0.;
    let load_started=Instant::now();
    result["onset_basis"]=json!("monotonic from first production program import; shared collector paints Original and auditions concurrently; first output excludes lexical metadata prepass");
    for program in &programs {
        let start = Instant::now();
        result["stage"] = json!(format!("parse program {program}"));
        metrics::checkpoint(out, &result);
        ADMISSIONS.with(|a| a.borrow_mut().clear());
        let instrument = match import::read_program(path, *program) {
            Ok(i) => Arc::new(i),
            Err(e) => {
                all_load = false;
                error = true;
                result["programs"]
                    .as_array_mut()
                    .unwrap()
                    .push(metrics::error("parse", e));
                continue;
            }
        };
        let mut symbols = BTreeMap::<String, usize>::new();
        for s in &instrument.scripts {
            for (k, v) in metrics::symbols(s) {
                *symbols.entry(k).or_default() += v;
            }
        }
        result["stage"] = json!(format!("script init program {program}"));
        metrics::checkpoint(out, &result);
        let (mut rt, script_errors) =
            crate::engine::load_scripts(&instrument, instrument.script_state.clone(), 48_000.);
        if let Some(rt) = rt.as_mut() {
            let mut setup = crate::engine::ScriptSetup::new(&instrument, 48_000.);
            for _ in 0..10 {
                rt.process(&mut setup, 480);
            }
        }
        let mut ksp = phase_report();
        if let Some(rt) = rt.as_ref() {
            ksp["load_fault_records"]=json!(rt.faults().map(|f|json!({"runtime_slot":f.slot.saturating_sub(1),"line":f.line,"category":"retained-runtime-fault","builtin":match f.context{Some(crate::ksp::FaultContext::MidiNote{builtin,..})=>Some(builtin),_=>None},"count":f.count})).collect::<Vec<_>>());
        }
        load_ms += start.elapsed().as_secs_f64() * 1000.;
        let mut views = Vec::new();
        let mut paint_jobs=Vec::new();
        if let Some(rt) = rt.as_ref() {
            for (slot, _) in rt.performance_slots() {
                let face = Arc::new(rt.interface(slot));
                if !face.performance || face.controls.is_empty() {
                    continue;
                }
                has_ui = true;
                let names: Vec<_> = artwork::picture_names(&face)
                    .map(|s| s.into_owned())
                    .collect();
                let (pictures, picture_errors) =
                    artwork::pictures_report(path, names.iter().map(String::as_str));
                let wallpaper = artwork::performance(&instrument, Some(&face));
                let missing_images = names
                    .iter()
                    .filter(|n| !pictures.contains_key(n.as_str()))
                    .count()
                    + usize::from(wallpaper.is_err());
                missing |= missing_images > 0;
                let shown = super::perf_view::layout(&face, &pictures);
                let mut kinds = BTreeMap::<String, usize>::new();
                let mut geometries = BTreeMap::<String, usize>::new();
                let mut live = 0;
                let mut live_bound = 0;
                for s in &shown {
                    let c = &face.controls[s.control];
                    *kinds.entry(c.kind.clone()).or_default() += 1;
                    if !matches!(
                        s.kind,
                        super::perf_view::Kind::Label
                            | super::perf_view::Kind::Waveform
                            | super::perf_view::Kind::Meter
                    ) {
                        live += 1;
                        live_bound += usize::from(c.id > 0);
                    }
                    if s.x < 0.
                        || s.y < 0.
                        || s.x + s.w > face.width as f64
                        || s.y + s.h > face.height as f64
                    {
                        *geometries
                            .entry("outside authored page candidate".into())
                            .or_default() += 1;
                    }
                }
                bound += live_bound;
                interactive += live;
                blank |= shown.is_empty();
                let part = PartView {
                    pictures: Arc::new(pictures),
                    wallpaper: wallpaper.ok().flatten(),
                    interface: Some(face.clone()),
                    instrument: Some(instrument.clone()),
                    active: instrument.name.clone(),
                    ..Default::default()
                };
                result["stage"] = json!(format!("Original UI program {program} slot {slot}"));
                metrics::checkpoint(out, &result);
                paint_jobs.push((part,format!("program-{program}-slot-{slot}")));
                let declared = face
                    .controls
                    .iter()
                    .filter(|c| {
                        !matches!(
                            c.kind.as_str(),
                            "ui_label" | "ui_panel" | "ui_waveform" | "ui_level_meter"
                        )
                    })
                    .count();
                let declared_bound = face
                    .controls
                    .iter()
                    .filter(|c| {
                        c.id > 0
                            && !matches!(
                                c.kind.as_str(),
                                "ui_label" | "ui_panel" | "ui_waveform" | "ui_level_meter"
                            )
                    })
                    .count();
                views.push(json!({"controls_declared":declared,"controls_bound_declared":declared_bound,
                    "bound_typed":shown.iter().filter(|s|matches!(face.controls[s.control].kind.as_str(),"ui_table"|"ui_xy"|"ui_text_edit")&&face.controls[s.control].id>0).count(),"typed_binding_basis":"visible source UI ID; live typed edit/readback unmeasured","phantom_free_controls":null,"slot":slot,"widgets":face.controls.len(),"visible":shown.len(),"interactive":live,"bound":live_bound,"kinds":kinds,"geometry":geometries,"missing_images":missing_images,"asset_errors":picture_errors.len(),"render":null}));
            }
        }
        let paint_out=out.to_path_buf();
        let paint=std::thread::Builder::new().stack_size(32 << 20).spawn(move ||paint_jobs.into_iter().map(|(part,prefix)|match original(&part,load_started,&paint_out,&prefix) {
            Ok(mut r)=>{r["ok"]=json!(true);r},
            Err(e)=>{let message=e.to_string();json!({"ok":false,"budget_hit":metrics::budget(&message),"reason":metrics::message(&message)})}
        }).collect::<Vec<_>>()).expect("paint worker start");
        // v1's initial playable bank validates/resolves sample headers and streams on demand.
        result["stage"] = json!(format!("sample load program {program}"));
        metrics::checkpoint(out, &result);
        let bank_started = Instant::now();
        let bank = match Bank::load_bare(&instrument) {
            Ok(b) => b,
            Err(e) => {
                let _=paint.join();
                all_load = false;
                result["programs"]
                    .as_array_mut()
                    .unwrap()
                    .push(metrics::error("sample load", e));
                continue;
            }
        };
        load_ms += bank_started.elapsed().as_secs_f64() * 1000.;
        let native = rt.as_ref().map(|r| &r.host().keyboard);
        let public = |value: &crate::ksp::Value| match value {
            crate::ksp::Value::Int(n) => *n as i32,
            crate::ksp::Value::Text(t) => match t.as_str() {
                "$NI_KEY_TYPE_DEFAULT"=>0,"$NI_KEY_TYPE_CONTROL"=>1,"$NI_KEY_TYPE_NONE"=>2,
                "$KEY_COLOR_DEFAULT"=>16,"$KEY_COLOR_INACTIVE"=>17,"$KEY_COLOR_NONE"=>18,"$KEY_COLOR_WHITE"=>19,
                _=>-1,
            },
            _=>-1,
        };
        let (valid, invalid, keyboard_reason_counts) = metrics::ksp_keyboard(native.into_iter()
            .flat_map(|keys| keys.iter()).map(|(key,k)|(*key,k.kind.as_ref().map(&public),k.color.as_ref().map(&public))));
        let keyboard_reason_counts=if native.is_some(){keyboard_reason_counts}else{serde_json::Value::Null};
        let candidate = (0..=127u8)
            .filter(|k| {
                !invalid.contains(k)
                    && bank.zones().iter().any(|z| {
                        (z.low_key..=z.high_key).contains(k)
                            && (z.low_velocity..=z.high_velocity).contains(&64)
                    })
            })
            .max_by_key(|k| (valid.contains(k), std::cmp::Reverse(k.abs_diff(60))))
            .map(|k| (k, 64));
        let pick = metrics::note(*program).filter(|(key,_)|!invalid.contains(key)).or(candidate).or_else(||metrics::fallback_note(&invalid));
        let pick_source = match pick {
            Some((k, 64)) if candidate==pick && valid.contains(&k) => "native_declared",
            Some((_, 64)) if candidate==pick => "zone_coverage",
            _ => "fallback",
        };
        let declared_switch=invalid.iter().copied().next();
        let keyswitch=metrics::planned_keyswitch(*program).unwrap_or(declared_switch);
        let skipped = bank.skipped_zones;
        let sample_zone_count=bank.zones().len();
        let mut engine = Engine::default();
        engine.set_bank(Some(Box::new(bank)));
        engine.set_fx(crate::engine::effects(&instrument, rt.as_deref(), 48_000.));
        engine.set_script(rt);
        let mut heard = false;
        result["stage"] = json!(format!("play program {program}"));
        metrics::checkpoint(out, &result);
        if let Some((key, vel)) = pick {
            engine.cc(0, 1, 100);
            engine.cc(0, 11, 127);
            if let Some(switch)=keyswitch {
                engine.note_on(0,switch,64);
                let (mut l,mut r)=([0f32;128],[0f32;128]);engine.render(&mut l,&mut r);
                if first_audio_ms.is_none() && metrics::nonzero(l.iter().chain(&r).copied()) {first_audio_ms=Some(load_started.elapsed().as_secs_f64()*1000.);}
                engine.note_off(0,switch);
            }
            engine.note_on(0, key, vel);
            for _ in 0..180 {
                std::thread::sleep(Duration::from_millis(3));
                let (mut l, mut r) = ([0f32; 128], [0f32; 128]);
                engine.render(&mut l, &mut r);
                if first_audio_ms.is_none() && metrics::nonzero(l.iter().chain(&r).copied()) {first_audio_ms=Some(load_started.elapsed().as_secs_f64()*1000.);}
                if l.iter().chain(&r).any(|x| x.is_finite() && x.abs() > 1e-5) {
                    heard = true;
                }
            }
        }
        result["stage"]=json!(format!("Original paint join program {program}"));
        metrics::checkpoint(out,&result);
        let painted=paint.join().unwrap_or_else(|_|vec![json!({"ok":false,"budget_hit":false,"reason":"paint worker panicked"});views.len()]);
        for (view,render) in views.iter_mut().zip(painted) {error |= render["ok"]!=true;view["render"]=render;}
        any_heard |= heard;
        // Script failures are recorded separately from successful import/sample-bank construction.
        error |= !script_errors.is_empty();
        result["programs"].as_array_mut().unwrap().push(json!({"ksp":ksp,"keyboard_reason_counts":keyboard_reason_counts,"keyswitch":keyswitch,"fallback_note":pick_source=="fallback","zero_zone_reason":if sample_zone_count==0 {Some("unknown")} else {None},"sample_zone_count":sample_zone_count,"sample_resident_bytes":crate::engine::resident_bytes(),"underruns":engine.underruns(),"pick_source":pick_source,"native_valid_keys":valid,"native_key_conflicts":0,"native_preferred_note":candidate.filter(|(k,_)|valid.contains(k)),"admitted_saved_entries_by_sigil":instrument.script_state.iter().flat_map(|s|s.keys()).fold(BTreeMap::<String,usize>::new(),|mut m,n|{*m.entry(metrics::metadata::sigil(n.as_bytes()).into()).or_default()+=1;m}),"load_path":"kontakt-v1-loader","program":program,"loaded":true,"source":"kontakt","symbols":symbols,"views":views,"script_error_count":script_errors.len(),"missing_samples":instrument.missing_samples.len(),"skipped_zones":skipped,"plays_note":if heard{"yes"}else{"silent"},"pick":pick}));
    }
    result["loads"] = json!(if all_load && !programs.is_empty() {
        "yes"
    } else {
        "no"
    });
    result["ui"] = json!(if error {
        "error"
    } else if blank {
        "blank"
    } else if missing {
        "missing-images"
    } else if !has_ui {
        "no-ui"
    } else {
        "original-ok"
    });
    result["plays_note"] = json!(if any_heard {
        "yes"
    } else if all_load {
        "silent"
    } else {
        "no"
    });
    result["controls_bound"] = json!(format!("{bound}/{interactive}"));
    result["load_ms"] = json!(load_ms);
    result["first_audio_ms"]=json!(first_audio_ms);
    result["reason"] = json!(format!(
        "{} programs; Original only; bound {bound}/{interactive}; initial streaming bank; audio {}",
        programs.len(),
        if any_heard {
            "audible"
        } else {
            "not audible in 0.5s probe"
        }
    ));
    result["stage"] = json!("complete");
    result
}

thread_local! {
    static PHASES: std::cell::RefCell<BTreeMap<u8,Value>> = const { std::cell::RefCell::new(BTreeMap::new()) };
    static ADMISSIONS: std::cell::RefCell<BTreeMap<u8,Value>> = const { std::cell::RefCell::new(BTreeMap::new()) };
    static SOURCE_KIND: std::cell::Cell<&'static str> = const {std::cell::Cell::new("none")};
}
pub(crate) fn source_kind(kind: &'static str) {
    SOURCE_KIND.set(kind);
}
pub(crate) fn admission(wire: usize, runtime: usize) {
    ADMISSIONS.with(|a|{a.borrow_mut().insert(runtime as u8,json!({"wire_slot":wire,"runtime_slot":runtime,"effective_source_kind":SOURCE_KIND.get()}));});
}
pub(crate) fn compile_phase(
    slot: u8,
    admitted: bool,
    disabled: usize,
    diagnostics: usize,
    persist_disabled: bool,
) {
    if !cfg!(test) && std::env::var_os("KONTRA_SCAN_ACTIVE").is_none() {
        return;
    }
    PHASES.with(|p|{let mapping=ADMISSIONS.with(|a|a.borrow().get(&slot).cloned()).unwrap_or(json!({"runtime_slot":slot,"wire_slot":null,"effective_source_kind":"unknown"}));
        let mut record=json!({"slot":slot,"runtime_slot":slot,"wire_slot":mapping["wire_slot"],"effective_source_kind":mapping["effective_source_kind"],"compile_ok":admitted,"compile_admitted":admitted,"compile_clean":admitted&&disabled==0,"disabled_block_errors":disabled,"compile_diagnostic_count":diagnostics,
        "init":{"completion":"not_reached","status":"not_started"},"persistence_changed":{"completion":"not_reached","status":if persist_disabled{"compile_disabled"}else{"not_started"}}});
        if !admitted {record["compile_fault"]=json!({"phase":"compile","category":"compiler-rejection","builtin":null,"inactive_region":"unknown"});}p.borrow_mut().insert(slot,record);});
}
pub(crate) fn phase_event(slot: u8, name: &'static str, status: &'static str) {
    if !cfg!(test) && std::env::var_os("KONTRA_SCAN_ACTIVE").is_none() {
        return;
    }
    PHASES.with(|p|{if let Some(r)=p.borrow_mut().get_mut(&slot){
        if r[name]["status"]=="compile_disabled"&&status=="absent" {return}
        r[name]["status"]=json!(status);r[name]["completion"]=json!(match status{"completed"=>"completed","absent"=>"not_present","faulted"|"budget_stopped"|"dropped"=>"failed",_=>"not_reached"});
        r[name]["present"]=json!(status!="absent");
        if matches!(status,"faulted"|"budget_stopped"|"dropped"){r[name]["fault"]=json!({"phase":name,"category":match status{"budget_stopped"=>"fuel-budget","dropped"=>"callback-pool-exhausted",_=>"runtime-fault"},"builtin":null});}
    }});
}
fn phase_report() -> Value {
    let obs: Vec<_> = PHASES.with(|p| std::mem::take(&mut *p.borrow_mut()).into_values().collect());
    let compile = if obs.is_empty() {
        "no-scripts"
    } else if obs.iter().all(|s| s["compile_admitted"] == true) {
        "yes"
    } else {
        "no"
    };
    let init = if obs.is_empty() {
        "no-scripts"
    } else if obs.iter().any(|s| {
        matches!(
            s["init"]["status"].as_str(),
            Some("faulted" | "budget_stopped")
        )
    }) {
        "no"
    } else if obs
        .iter()
        .all(|s| matches!(s["init"]["status"].as_str(), Some("completed" | "absent")))
    {
        "yes"
    } else {
        "unknown"
    };
    let first = obs
        .iter()
        .find_map(|s| {
            s.get("compile_fault")
                .or(s["init"].get("fault"))
                .or(s["persistence_changed"].get("fault"))
        })
        .map(|e| {
            format!(
                "{} {}",
                e["phase"].as_str().unwrap_or("unknown"),
                e["category"].as_str().unwrap_or("unknown")
            )
        });
    json!({"compile_ok":compile,"init_ok":init,"first_error":first,"scripts":obs.len(),"slots":obs})
}

#[cfg(test)]
mod tests {
    #[test]
    fn scanner_phase_completion_follows_actual_yields() {
        let mut engine = crate::ksp::LogEngine::new(vec![], 48000.);
        let source = "on init\ndeclare $x\nend on\non persistence_changed\nwait(10000000)\nend on";
        let (_, errors) = crate::ksp::Runtime::with_scripts(&[source], &mut engine, 8, vec![]);
        assert!(errors[0].is_none());
        let r = super::phase_report();
        assert_eq!(r["slots"][0]["init"]["status"], "completed");
        assert_eq!(r["slots"][0]["persistence_changed"]["status"], "waiting");
        let source =
            "on init\ndeclare $x\nend on\non persistence_changed\nunsupported private syntax\nend on";
        let (_, errors) = crate::ksp::Runtime::with_scripts(&[source], &mut engine, 8, vec![]);
        assert!(errors[0].is_none());
        let r = super::phase_report();
        assert_eq!(r["slots"][0]["compile_admitted"], true);
        assert_eq!(r["slots"][0]["compile_clean"], false);
        assert_eq!(
            r["slots"][0]["persistence_changed"]["status"],
            "compile_disabled"
        );
        assert!(!r.to_string().contains("private syntax"));
    }
}
