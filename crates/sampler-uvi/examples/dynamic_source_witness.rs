//! Numeric-only production source-service witness. No instrument, script,
//! image, sample or rendered audio payload is written. Run with an outer process
//! deadline too: a filesystem read itself cannot be canceled synchronously.
use sampler_core::{Limits, Runtime, SampleTargetId};
use sampler_uvi::script::SampleLoadStatus;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
    time::{Duration, Instant},
};
fn tasks(s: SampleLoadStatus) -> Value {
    json!({"requested":s.requested,"successful_after_owner_ack":s.successful,"failed":s.failed,"cancelled":s.cancelled,"pending":s.pending,"queued_metadata":s.queued_metadata,"worker_active":s.worker_active,"ready":s.ready,"submitted":s.submitted,"conserved":s.requested==s.successful+s.failed+s.cancelled+s.pending})
}
fn run(
    path: &Path,
    limit: Duration,
    controls: bool,
    edit: Option<(String, f64)>,
) -> Result<Value, &'static str> {
    let start = Instant::now();
    let mut translated = sampler_uvi::translate_path(path).map_err(|_| "translate")?;
    let translated_ms = start.elapsed().as_secs_f64() * 1000.;
    let stage = Instant::now();
    let mut attached = translated
        .attach_script(48000, sampler_uvi::script::Config::realtime())
        .map_err(|_| "attach")?
        .ok_or("no_script")?;
    let attached_ms = stage.elapsed().as_secs_f64() * 1000.;
    let mut meter_reasons = BTreeMap::<String, usize>::new();
    let meter_requests = attached.driver.ui().interface().widgets.iter().filter_map(|w|w.uvi_meter_request).collect::<Vec<_>>();
    let mut meter_resolved = 0;
    for request in &meter_requests {
        match sampler_uvi::meters::resolve(&attached.meter_buses, *request) {
            Ok(_) => meter_resolved += 1,
            Err(reason) => *meter_reasons.entry(format!("{reason:?}")).or_default() += 1,
        }
    }
    let meter_owners = json!({"requests":meter_requests.len(),"resolved":meter_resolved,"unbound_reasons":meter_reasons,"native_display_law_supported":false});
    let prebind = attached
        .driver
        .sample_load_status()
        .map_err(|_| "prebind_status")?;
    if controls {
        let face = attached.driver.ui().interface();
        let values = attached.driver.ui().values();
        let rows:Vec<_>=face.widgets.iter().filter_map(|w| {
            let sampler_ui_ir::Binding::Control(id)=&w.binding else {return None;};
            let name=w.name.to_ascii_lowercase();
            let menu=matches!(&w.kind,sampler_ui_ir::Kind::Menu{..});
            if !menu && !["load","stream","mic","purge","select","layer"].iter().any(|key|name.contains(key)) {return None;}
            let items=if let sampler_ui_ir::Kind::Menu{items}=&w.kind {items.iter().take(12).map(|i|json!({"text":i.text,"value":i.value,"visible":i.visible})).collect::<Vec<_>>()}else{Vec::new()};
            Some(json!({"name":w.name,"text":w.text,"control_id_hex":format!("{:032x}",id.0),"value":values.iter().find(|(c,_)|c==id).map(|(_,v)|*v),"enabled":w.enabled,"hidden":w.hidden,"is_menu":menu,"menu_items_first12":items}))
        }).collect();
        return Ok(
            json!({"scope":"read-only authored control metadata discovery; no dynamic loading trigger executed","required_resident_assets":translated.instrument.assets.iter().filter(|a|a.residency==sampler_ir::AssetResidency::Resident).count(),"prebind_tasks":tasks(prebind),"controls":rows,"payload_written":false}),
        );
    }

    // Resolve source identity against this fresh owner's authentic interface;
    // identifiers from a different seeded host are never reused blindly.
    let resolved = if let Some((selector, value)) = edit {
        if !value.is_finite() {
            return Err("nonfinite_control_value");
        }
        let face = attached.driver.ui().interface();
        let matches: Vec<_> = face
            .widgets
            .iter()
            .enumerate()
            .filter(|(_, w)| {
                if let Some(source) = selector.strip_prefix("source:") {
                    let Some((number, name)) = source.split_once(':') else {
                        return false;
                    };
                    number.parse::<i32>().ok() == w.source_id && name == w.name
                } else {
                    let id = u128::from_str_radix(selector.trim_start_matches("0x"), 16).ok();
                    matches!(w.binding,sampler_ui_ir::Binding::Control(c) if Some(c.0)==id)
                }
            })
            .collect();
        if matches.len() != 1 {
            return Err("authored_control_identity_not_unique");
        }
        let (index, w) = matches[0];
        let mut current = index;
        for _ in 0..=face.widgets.len() {
            let item = &face.widgets[current];
            if item.hidden || !item.enabled {
                return Err("authored_control_not_visible_or_enabled");
            }
            if let Some(parent) = item.parent {
                current = parent.0;
            } else {
                break;
            }
        }
        let sampler_ui_ir::Binding::Control(id) = w.binding else {
            return Err("authored_control_unbound");
        };
        let previous = attached
            .driver
            .ui()
            .values()
            .iter()
            .find(|(c, _)| *c == id)
            .map(|(_, v)| *v);
        Some((
            id,
            value,
            json!({"source_id":w.source_id,"widget_index":index,"control_id_hex":format!("{:032x}",id.0),"previous_value":previous,"submitted_value":value,"visible_enabled_ancestry_validated":true}),
        ))
    } else {
        None
    };
    let ids: Vec<_> = translated
        .instrument
        .source_indices
        .sample_targets
        .iter()
        .map(|t| SampleTargetId(t.node))
        .collect();
    // Explicit equivalent of the then-current V2 Auto policy. This witness
    // never changes the product default and does not measure note onset margin.
    let policy = sampler_kontakt::StreamPolicy {
        lazy: true,
        head_budget: 8 << 20,
        max_step: 16.,
        block_frames: 128,
        voices: 1024,
        ..Default::default()
    };
    let stage = Instant::now();
    let streamed = sampler_uvi::assemble_translated_streamed(translated, 48000, &policy)
        .map_err(|_| "assemble")?;
    let assembled_ms = stage.elapsed().as_secs_f64() * 1000.;
    let initial_readiness = streamed.streamer.head_readiness(&streamed.assets);
    let initial_sources = streamed.loaded.plan.sample_count();
    let limits = Limits::for_plan(&streamed.loaded.plan, 4096, 16);
    let (mut runtime, control) = Runtime::new(streamed.loaded.plan, limits)
        .map_err(|_| "runtime")?
        .with_stream_cache(streamed.cache)
        .with_sample_updates(64)
        .map_err(|_| "sample_control")?;
    let initial: Vec<_> = ids
        .iter()
        .map(|&id| runtime.sample_target_asset(id).ok().map(|p| p.asset_id()))
        .collect();
    let mut driver = attached.driver;
    driver
        .bind_sample_stream(streamed.streamer.dynamic_sources())
        .map_err(|_| "bind_stream")?;
    driver
        .bind_sample_control(control)
        .map_err(|_| "bind_control")?;
    #[cfg(feature="scan")]
    let owner_audit_before=json!(driver.ui().owner_audit());
    #[cfg(not(feature="scan"))]
    let owner_audit_before=Value::Null;
    if let Some((id, value, _)) = &resolved {
        if !driver.set_control(*id, *value) {
            return Err("authored_control_edit_rejected");
        }
    }

    let mut observations = Vec::new();
    let mut previous = None;
    let mut stable_since = None;
    let mut last = prebind;
    let mut next_query = Instant::now();
    let mut wake_errors = 0usize;
    let mut render_errors = 0usize;
    let mut blocks = 0usize;
    let mut buffer = [[0.; 2]; 128];
    let mut stopped = "deadline";
    while start.elapsed() < limit {
        if driver.wake(&mut runtime).is_err() {
            wake_errors += 1;
        }
        if runtime.render(&mut buffer).is_err() {
            render_errors += 1;
        }
        blocks += 1;
        if Instant::now() >= next_query {
            last = driver.sample_load_status().map_err(|_| "status_reply")?;
            if previous != Some(last) {
                stable_since = Some(Instant::now());
                previous = Some(last);
            }
            if observations.len() < 64 {
                observations.push(json!({"at_ms":start.elapsed().as_secs_f64()*1000.,"tasks":tasks(last),"dynamic_head_held_bytes":runtime.sample_head_budget_status().map(|s|s.held_bytes)}));
            }
            if std::env::var_os("KONTRA_SAMPLE_WITNESS_STOP_ON_FAILURE").is_some() && last.failed>0 {
                stopped="first_failure_debug_stop";
                break;
            }
            if last.pending == 0
                && stable_since.is_some_and(|t| t.elapsed() >= Duration::from_millis(500))
            {
                stopped = "task_counts_stable_500ms";
                break;
            }
            next_query = Instant::now() + Duration::from_millis(100);
        }
        std::thread::sleep(Duration::from_secs_f64(128. / 48000.));
    }
    #[cfg(feature="scan")]
    let owner_audit_after=json!(driver.ui().owner_audit());
    #[cfg(not(feature="scan"))]
    let owner_audit_after=Value::Null;
    let verification_started = Instant::now();
    let mut verified_assets=0usize;
    let mut verified_windows=0usize;
    let mut verified_frames=0usize;
    let mut verification_failures=0usize;
    let mut missing_registry=0usize;
    let mut header_mismatches=0usize;
    let mut frame_mismatches=0usize;
    let mut copied_hash=0xcbf29ce484222325u64;
    let mut decoded_hash=0xcbf29ce484222325u64;
    let mut verification_truncated=false;
    let mut covered = 0usize;
    let mut replaced = 0usize;
    let mut distinct = HashSet::new();
    let mut head_bytes = 0usize;
    let mut full_resident = 0usize;
    let mut source_frames = 0u64;
    let mut rate_min = u32::MAX;
    let mut rate_max = 0u32;
    for (&target, initial) in ids.iter().zip(initial) {
        if let Ok(pcm) = runtime.sample_target_asset(target) {
            covered += 1;
            replaced += usize::from(initial != Some(pcm.asset_id()));
            if distinct.insert(pcm.asset_id()) {
                head_bytes += pcm.head_bytes();
                full_resident += usize::from(pcm.resident_frames().is_some());
                if initial != Some(pcm.asset_id()) {
                    if start.elapsed()>=limit || verified_assets>=4096 {
                        verification_truncated=true;
                    } else if let Some(source)=streamed.streamer.source(pcm.asset_id()) {
                        match source.open() {
                            Ok(mut reader) => {
                                if reader.rate()!=pcm.sample_rate() || reader.frames()!=pcm.frame_count() { header_mismatches+=1; }
                                let len=pcm.frame_count().min(256);
                                let mut installed=[[0.;2];256];
                                let mut decoded=[[0.;2];256];
                                let mut checked=false;
                                let mut starts=vec![0,pcm.frame_count().saturating_sub(len),pcm.frame_count().saturating_sub(len)/2];
                                starts.sort_unstable(); starts.dedup();
                                for offset in starts {
                                    if pcm.copy_resident_range(offset,&mut installed[..len]).is_err() {continue;}
                                    if reader.read(offset,&mut decoded[..len]).is_err() {verification_failures+=1;continue;}
                                    checked=true; verified_windows+=1; verified_frames+=len;
                                    for (a,b) in installed[..len].iter().zip(&decoded[..len]) {
                                        frame_mismatches+=usize::from(a.map(f32::to_bits)!=b.map(f32::to_bits));
                                        for (a,b) in a.iter().zip(b) {
                                            copied_hash=(copied_hash ^ u64::from(a.to_bits())).wrapping_mul(0x100000001b3);
                                            decoded_hash=(decoded_hash ^ u64::from(b.to_bits())).wrapping_mul(0x100000001b3);
                                        }
                                    }
                                }
                                if checked {verified_assets+=1;} else {verification_failures+=1;}
                            },
                            Err(_)=>verification_failures+=1,
                        }
                    } else {missing_registry+=1;}
                }
                source_frames = source_frames.saturating_add(pcm.frame_count() as u64);
                rate_min = rate_min.min(pcm.sample_rate());
                rate_max = rate_max.max(pcm.sample_rate());
            }
        }
    }
    let physical = runtime
        .sample_head_budget_status()
        .ok_or("missing_dynamic_quota")?;
    let mut failure_classes = BTreeMap::<&'static str, usize>::new();
    if std::env::var_os("KONTRA_SAMPLE_WITNESS_STOP_ON_FAILURE").is_some() {
        for finding in driver.ui().findings().into_iter().filter(|f|f.feature=="lua loadSample failed").take(3) {
            eprintln!("sample_failure_preview {}",finding.value);
        }
    }
    for f in driver
        .ui()
        .findings()
        .into_iter()
        .filter(|f| f.feature == "lua loadSample failed")
    {
        let text = f.value.to_ascii_lowercase();
        let class = if text.contains("budget exceeded") {
            "physical_budget"
        } else if text.contains("unsupported") {
            "unsupported"
        } else if text.contains("no such file")
            || text.contains("not found")
            || text.contains("missing")
        {
            "missing"
        } else if text.contains("metadata") {
            "metadata"
        } else if text.contains("source commit") || text.contains("source submission") {
            "transaction"
        } else {
            "other"
        };
        *failure_classes.entry(class).or_default() += f.count;
    }
    let faults = driver.ui().fault_counts();
    let static_quota = streamed.streamer.head_budget_status();
    Ok(
        json!({"scope":"production promotion, attach, static streamed assembly, real script owner and core render commits; no MIDI notes; not native audio parity or deadline margin","stage_ms":{"translate":translated_ms,"attach":attached_ms,"assemble":assembled_ms,"total":start.elapsed().as_secs_f64()*1000.},"stop_reason":stopped,"deadline_seconds":limit.as_secs(),"deadline_reached":start.elapsed()>=limit,"prebind_tasks":tasks(prebind),"authored_control_edit":resolved.map(|(_,_,metadata)|metadata),"static_policy":{"lazy":true,"head_budget":8<<20,"max_step":16,"block_frames":128},"owner_audit_before":owner_audit_before,"owner_audit_after":owner_audit_after,"source_pcm_verification":{"assets":verified_assets,"windows":verified_windows,"frames":verified_frames,"failures":verification_failures,"missing_registry":missing_registry,"header_mismatches":header_mismatches,"frame_mismatches":frame_mismatches,"installed_fnv64":copied_hash,"independent_reopen_fnv64":decoded_hash,"truncated":verification_truncated,"duration_ms":verification_started.elapsed().as_secs_f64()*1000.,"scope":"up to4096changedassets,three256framewindows each when resident; fresh admitted authority reader; no full sample/native audio equivalence"},"sample_load_timings":last.timings,"initial_meter_owners":meter_owners,"final_tasks":tasks(last),"targets":ids.len(),"initial_static_sources":initial_sources,"committed_source_target_coverage":covered,"targets_with_replaced_source_identity":replaced,"distinct_current_source_ids":distinct.len(),"current_source_frames_sum":source_frames,"current_source_rate_min":if distinct.is_empty(){None}else{Some(rate_min)},"current_source_rate_max":rate_max,"current_source_head_bytes":head_bytes,"current_sources_fully_resident":full_resident,"dynamic_physical_head_quota":{"limit":physical.limit_bytes,"held":physical.held_bytes,"peak":physical.peak_bytes},"initial_static_head_quota":{"limit":static_quota.limit_bytes,"held":static_quota.held_bytes,"peak":static_quota.peak_bytes,"head_assets":initial_readiness.assets,"covered_assets_at_publish":initial_readiness.covered_assets},"source_failure_classes":failure_classes,"script_fault_counts":{"init":faults.init,"runtime":faults.runtime},"core_wake_errors":wake_errors,"core_render_errors":render_errors,"rendered_empty_blocks":blocks,"sounding_voices":runtime.voice_count(),"stream_underruns":runtime.stats().stream_underruns,"observations":observations,"truncated_observations":observations.len()==64,"payload_written":false,"os_cache_scope":"uncontrolled; no global cache eviction","measurement_scope":"unisolated correctness witness; future authored timers beyond the settled observation window are not proven quiescent"}),
    )
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.len() == 3 || args.len() == 4 || args.len() == 5,
        "PATH TOTAL_DEADLINE_SECONDS [--controls | CONTROL_ID_HEX VALUE | source:ID:NAME VALUE]"
    );
    let limit = Duration::from_secs(args[2].parse::<u64>().unwrap().clamp(1, 120));
    let controls = args.get(3).is_some_and(|s| s == "--controls");
    let edit = (args.len() == 5).then(|| (args[3].clone(), args[4].parse::<f64>().unwrap()));
    match run(Path::new(&args[1]), limit, controls, edit) {
        Ok(result) => println!("{result}"),
        Err(stage) => {
            println!("{}", json!({"failed_stage":stage,"payload_written":false}));
            std::process::exit(1);
        }
    }
}
