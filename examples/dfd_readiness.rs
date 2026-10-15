//! Numeric-only actual V2 Auto head/onset diagnostics. Audio remains in RAM.
use kontakto::sound::{
    BlockInfo, Core, CoreLoader, LoadRequest,
    event::{Event, HostNote, HostPattern},
    v2::{V2Core, V2Loader},
};
use serde_json::{Value, json};
use sha2::{Digest,Sha256};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    path::PathBuf,
    time::{Duration, Instant},
};
struct Counting;
thread_local! {static ACTIVE:Cell<bool>=const{Cell::new(false)};static HEAP:Cell<u64>=const{Cell::new(0)};}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if ACTIVE.get() {
            HEAP.set(HEAP.get() + 1);
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if ACTIVE.get() {
            HEAP.set(HEAP.get() + 1);
        }
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
fn io() -> Value {
    let data = std::fs::read_to_string("/proc/self/io").unwrap();
    let field = |key: &str| {
        data.lines()
            .find_map(|l| l.strip_prefix(key)?.trim().parse::<u64>().ok())
            .unwrap_or(0)
    };
    json!({"read_bytes":field("read_bytes:"),"rchar":field("rchar:"),"syscr":field("syscr:")})
}
fn heads(stream: &kontakto::sound::Stream) -> Value {
    let mut resident = 0;
    let mut ready = 0;
    let mut source_frames = 0u64;
    for p in &stream.assets {
        let b = p.head_bytes();
        resident += b;
        ready += usize::from(b > 0);
        source_frames += p.frame_count() as u64;
    }
    let coverage = |r: sampler_kontakt::HeadReadiness| json!({
        "assets":r.assets,"ranges":r.ranges,"source_frames":r.source_frames,
        "covered_ranges":r.covered_ranges,"covered_source_frames":r.covered_source_frames,
        "complete":r.complete()
    });
    let hints = json!({
        "requested":stream.streamer.hint_readiness(&stream.assets).map(coverage),
        "selected":stream.streamer.selected_hint_readiness(&stream.assets).map(coverage),
        "selected_output_horizon":stream.streamer.selected_hint_head_frames(),
        "scope":"Current resident snapshot, queried outside callback; separate from historical loader report"
    });
    json!({"assets":stream.assets.len(),"assets_with_any_head":ready,"head_bytes":resident,"source_frames":source_frames,
        "current_constant_onsets":hints,"note":"any head is not proof every authored onset/loop window is ready"})
}
fn readiness(
    stream: &kontakto::sound::Stream,
    include: impl Fn(sampler_core::AssetId) -> bool,
) -> Value {
    let r = stream
        .streamer
        .head_readiness_matching(&stream.assets, include);
    json!({"assets":r.assets,"ranges":r.ranges,"source_frames":r.source_frames,"covered_assets":r.covered_assets,"covered_ranges":r.covered_ranges,"covered_source_frames":r.covered_source_frames,"minimum_packed_bytes":r.minimum_packed_bytes(),"decoded_bytes":r.decoded_bytes(),"packed_resident_bytes":r.packed_bytes,"configured_budget_bytes":r.budget_bytes,"complete":r.complete()})
}
fn stats(core: &V2Core) -> Value {
    let s = core.runtime_stats(0).unwrap();
    json!({"voices":s.voices,"voice_capacity":s.voice_capacity,"voice_growths":s.voice_growths,
        "growth_failures":s.growth_failures,"cold_starts":s.cold_starts,"underruns":s.stream_underruns,"nonfinite_frames":s.nonfinite_frames,
        "refused":s.refused_starts,"capacity_drops":s.voice_drops,"stream_capacity_refusals":s.stream_capacity_refusals,
        "stream_disconnected_refusals":s.stream_disconnected_refusals,
        "voice_pool_refusals":s.voice_pool_refusals,"waiting_voice_replacements":s.waiting_voice_replacements,
        "held_key_60":core.key_held(0,60),"problems":core.problems(0)})
}

fn guards(core: &V2Core, views: &[sampler_ksp::ScriptView], targets: Option<&Value>) -> Value {
    let Some(targets)=targets else {return Value::Null};
    let generation=core.ui_lifecycle_progress(0)["audio_generation"].as_u64();
    let mut rows=Vec::new();
    let mut matches=0usize;
    for (instance,view) in views.iter().enumerate().filter(|(_,v)|Some(u64::from(v.slot()))==targets["physical_slot"].as_u64()) {
        let source=view.setup().identity.source.iter().map(|b|format!("{b:02x}")).collect::<String>();
        if Some(source.as_str())!=targets["script_sha256"].as_str() {
            rows.push(json!({"physical_slot":view.slot(),"instance":instance,"source_match":false}));continue;
        }
        matches+=1;
        for target in targets["fields"].as_array().into_iter().flatten() {
            if target["storage_role"]=="system" {continue;}
            let Some(id)=target["id"].as_str() else {continue};
            if id.len()!=12 || !id.bytes().all(|b|b.is_ascii_hexdigit()) {continue;}
            let field=view.setup().fields.iter().find(|field|format!("{:x}",Sha256::digest(field.name.as_bytes())).starts_with(id));
            let Some(field)=field else {
                rows.push(json!({"physical_slot":view.slot(),"instance":instance,"id":id,"storage_role":target["storage_role"],"setup_field_present":false}));continue;
            };
            match field.storage {
                sampler_ksp::model::Location::Control(control)=>rows.push(json!({"physical_slot":view.slot(),"instance":instance,
                    "id":id,"storage":"control","value":core.control_value(0,sampler_ui_ir::ControlId(control.0))})),
                sampler_ksp::model::Location::Cells{offset,len}=>{
                    let mut indices:Vec<u32>=target["literal_indices"].as_array().into_iter().flatten()
                        .filter_map(|n|n.as_u64().and_then(|n|u32::try_from(n).ok())).filter(|&n|n<len).take(8).collect();
                    if indices.is_empty() && len!=0 {indices.push(0);}
                    for index in indices {
                        let value=generation.ok_or(sampler_core::Error::StaleHandle).and_then(|epoch|
                            core.diagnostic_script_cell(0,epoch,instance as u16,offset+index));
                        rows.push(json!({"physical_slot":view.slot(),"instance":instance,"id":id,"storage":"cell",
                            "array_index":index,"raw_integer_or_real_bits":value.as_ref().ok(),
                            "error_kind":value.err().and_then(|e|sampler_core::Error::ALL.iter().position(|v|*v==e))}));
                    }
                }
                sampler_ksp::model::Location::Texts{..}=>{}
            }
        }
    }
    json!({"source_bound":matches!=0,"matching_instances":matches,"generation":generation,"rows":rows,
        "scope":"Exact SetupLayout storage; SHA12 names and source SHA gate, no source strings or guessed cell packing"})
}

fn activate(core: &mut V2Core, views: &mut [sampler_ksp::ScriptView], targets: Option<&Value>) -> Value {
    // Same read-only effect host as the production offline lifecycle witness.
    // Fixed bounds are evidence, never permission to call an unfinished plan ready.
    let started=Instant::now();
    let mut effects=0usize;
    let mut frames=0usize;
    let mut rows=Vec::new();
    let mut failure=None;
    let mut completed=std::collections::BTreeMap::new();
    while frames<96_000 && started.elapsed()<Duration::from_secs(5) && effects<=65_536 {
        match core.service_ui_read_only(0,views) {
            Ok(count)=>effects+=count,
            Err(error)=>{failure=Some(format!("{:?}",error.kind()));break;}
        }
        let progress=core.ui_lifecycle_progress(0);
        if rows.len()<4 {rows.push(progress.clone());}
        if frames!=0 && progress["pending_plan_callbacks"].as_u64()==Some(0) {break;}
        if ["live_callback_failures","queued_faults","dropped_faults"].iter()
            .any(|k|progress[*k].as_u64().is_some_and(|n|n!=0)) {break;}
        core.begin_block(&BlockInfo{frames:128,offline:true,..Default::default()});
        let _=core.render(128);
        // end_block consumes terminal callback outcomes. Observe exact retained
        // roles before that production drain without consuming outcomes here.
        let progress=core.ui_lifecycle_progress(0);
        for row in progress["callbacks"].as_array().into_iter().flatten() {
            let Some(program)=row["program"].as_u64() else {continue};
            if row["outcome"]=="pending" {continue;}
            let mut local=program as usize;
            for view in views.iter() {
                if local<view.program_count() {
                    completed.insert(program,json!({"physical_slot":view.slot(),"role":view.program_role(local),
                        "outcome":row["outcome"],"error_kind":row["error_kind"]}));break;
                }
                local-=view.program_count();
            }
        }
        core.end_block(128,&mut |_|true);
        frames+=128;
    }
    json!({"frames":frames,"effects_applied":effects,"service_failure":failure,
        "initial_progress":rows,"final_progress":core.ui_lifecycle_progress(0),"runtime":stats(core),
        "completed_callback_programs_observed":completed,
        "guard_state":guards(core,views,targets),
        "elapsed_ms":started.elapsed().as_secs_f64()*1000.,
        "scope":"Bounded96k offline activation frames with real read-only UI-effect drains; pending listener work is not proof finite activation failed or completed"})
}
fn demand(core: &V2Core) -> Value {
    // One output frame identifies current interpolation reads. Bounded numeric
    // snapshot runs outside the callback and is not a historical miss trace.
    let mut rows = Vec::new();
    let complete = core.visit_stream_demand(0, 1, |d| {
        if rows.len() == 1024 { return false; }
        rows.push(json!({"voice":format!("{:?}",d.voice),"asset":format!("{:?}",d.key.asset),
            "zone_source":d.source_zone,"script_source_offset_micros":d.source_offset_micros,"source_frame":d.source_frame,
            "page":d.key.index,"start":d.frames.start,"end":d.frames.end,"deadline":d.deadline,
            "starved":d.starved,"cold":d.cold,"resident":d.resident,"status":format!("{:?}",d.status)}));
        true
    }).unwrap().unwrap();
    json!({"complete":complete,"horizon":1,"rows":rows})
}
fn phase(core: &mut V2Core, views: &mut [sampler_ksp::ScriptView], targets: Option<&Value>, stream: &kontakto::sound::Stream, block: usize, round: i32) -> Value {
    let n = 120usize;
    let mut rows = Vec::new();
    let mut times = Vec::with_capacity(n);
    let mut misses = 0;
    let mut peak = 0f32;
    let mut first_signal = None;
    let mut event_us = 0u64;
    let mut ui_effects=0usize;
    let mut ui_errors=0usize;
    let before_heap = HEAP.get();
    let before_io = io();
    let before_stats = stats(core);
    let before_heads = heads(stream);
    let mut prior_underruns = core.runtime_stats(0).unwrap().stream_underruns;
    let start = Instant::now();
    for index in 0..n {
        match core.service_ui_read_only(0,views) {Ok(count)=>ui_effects+=count,Err(_)=>ui_errors+=1}
        ACTIVE.set(true);
        let timer = Instant::now();
        core.begin_block(&BlockInfo {
            frames: block,
            ..Default::default()
        });
        if index == 0 {
            let event = Instant::now();
            core.event(0, Event::midi1(0xb0, 1, 110));
            core.event(0, Event::midi1(0xb0, 11, 127));
            for key in 60..68 {
                core.event(
                    0,
                    Event::NoteOn {
                        note: HostNote {
                            port: 0,
                            channel: 0,
                            key,
                            id: round * 1000 + i32::from(key),
                            clap: true,
                        },
                        velocity: 0.8,
                        tune: 0.,
                    },
                );
            }
            event_us = event.elapsed().as_micros() as u64;
        }
        let mut block_peak = 0f32;
        for offset in (0..block).step_by(128) {
            let len = (block - offset).min(128);
            let out = core.render(len);
            for sample in out.buses[0].iter().flat_map(|c| &c[..len]) {
                block_peak = block_peak.max(sample.abs());
            }
        }
        core.end_block(block, &mut |_| true);
        let ns = timer.elapsed().as_nanos() as u64;
        ACTIVE.set(false);
        times.push(ns);
        misses += usize::from(ns as f64 > block as f64 / 48000. * 1e9);
        peak = peak.max(block_peak);
        if block_peak > 1e-8 && first_signal.is_none() {
            first_signal = Some(index * block);
        }
        let underruns = core.runtime_stats(0).unwrap().stream_underruns;
        if [0, 1, 2, 4, 8, 16, 32, 64, 119].contains(&index) || underruns != prior_underruns {
            rows.push(json!({"block":index,"engine_offset":index*block,"callback_us":ns as f64/1000.,"peak":block_peak,"runtime":stats(core),"callback_progress":core.ui_lifecycle_progress(0),"guard_state":guards(core,views,targets),"heads":heads(stream),"current_demand_after_callback":demand(core)}));
        }
        prior_underruns = underruns;
        let due = start + Duration::from_secs_f64((index + 1) as f64 * block as f64 / 48000.);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    times.sort_unstable();
    json!({"round":round,"before_runtime":before_stats,"after_runtime":stats(core),"before_heads":before_heads,"after_heads":heads(stream),"after_exact_head_coverage":readiness(stream, |_|true),"io_before":before_io,"io_after":io(),"event_us":event_us,"ui_effects":ui_effects,"ui_service_errors":ui_errors,"deadline_misses":misses,"heap_calls":HEAP.get()-before_heap,"callback_p50_us":times[n/2] as f64/1000.,"callback_p99_us":times[(n-1)*99/100] as f64/1000.,"callback_max_us":times[n-1] as f64/1000.,"first_nonzero_engine_offset_frames":first_signal,"peak":peak,"rows":rows})
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "PATH 64|256");
    let path = PathBuf::from(&args[1]);
    let block = args[2].parse::<usize>().unwrap();
    assert!([64, 256].contains(&block));
    let targets:Option<Value>=std::env::var_os("KONTRA_GUARD_TARGETS")
        .map(|p|serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap());
    let load_start = Instant::now();
    let loaded = V2Loader
        .prepare(
            &LoadRequest {
                path,
                sample_rate: 48000.,
                ..Default::default()
            },
            &mut |_| {},
            &|| false,
        )
        .unwrap();
    let load_ms = load_start.elapsed().as_secs_f64() * 1000.;
    let stream = loaded.stream.clone().expect("streamed instrument");
    let candidate_assets = |first_chord: bool| -> std::collections::HashSet<sampler_core::AssetId> {
        loaded
            .instrument
            .as_ref()
            .into_iter()
            .flat_map(|i| {
                i.zones.iter().filter(move |z| {
                    !z.group.is_some_and(|g| i.groups[g.0].muted)
                        && (!first_chord
                            || (z.keys.low <= 67
                                && z.keys.high >= 60
                                && z.velocities.low <= 102
                                && z.velocities.high >= 102))
                })
            })
            .filter_map(|z| {
                z.asset
                    .and_then(|a| stream.assets.get(a.0))
                    .map(|p| p.asset_id())
            })
            .collect()
    };
    let enabled = candidate_assets(false);
    let chord = candidate_assets(true);
    let source = json!({"load_ms":load_ms,"stream_startup":loaded.report.stream_startup,"head_frames":stream.report.head_frames,"head_bytes_reported_at_load":stream.report.head_bytes,"pool_bytes":stream.report.pool_bytes,"full_bytes":stream.report.full_bytes,"read_latency_samples":stream.report.latency_samples,"read_latency_p95_us":stream.report.latency_p95.as_micros(),"heads_after_prepare":heads(&stream),"exact_head_coverage":readiness(&stream, |_|true),"saved_unmuted_asset_coverage":readiness(&stream, |id|enabled.contains(&id)),"static_first_chord_asset_candidates":readiness(&stream, |id|chord.contains(&id)),"candidate_scope":"saved group mute plus authored key/velocity; includes RR/articulation/trigger/controller alternatives, does not infer Lua mutations or exact first-burst reachability","io_after_prepare":io()});
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, loaded.part);
    let mut scripts=loaded.scripts;
    let before_activation_guards=guards(&core,&scripts.views,targets.as_ref());
    let activation=activate(&mut core,&mut scripts.views,targets.as_ref());
    let first = phase(&mut core, &mut scripts.views, targets.as_ref(), &stream, block, 1);
    for key in 60..68 {
        core.event(
            0,
            Event::Choke(HostPattern {
                port: 0,
                channel: 0,
                key: i32::from(key),
                id: 1000 + i32::from(key),
                clap: true,
            }),
        );
    }
    let warm_start = Instant::now();
    for index in 0..20 {
        let _=core.service_ui_read_only(0,&mut scripts.views);
        core.begin_block(&BlockInfo {
            frames: block,
            ..Default::default()
        });
        for offset in (0..block).step_by(128) {
            let _ = core.render((block - offset).min(128));
        }
        core.end_block(block, &mut |_| true);
        let due = warm_start + Duration::from_secs_f64((index + 1) as f64 * block as f64 / 48000.);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    let second = phase(&mut core, &mut scripts.views, targets.as_ref(), &stream, block, 2);
    println!(
        "{}",
        json!({"policy":"actual V2Loader default Auto","block":block,"cache_scope":"fresh process/source heads; OS/storage cache uncontrolled; no cache eviction requested","diagnostic":true,"source":source,"before_activation_guards":before_activation_guards,"activation":activation,"first_burst":first,"second_burst":second})
    );
}
