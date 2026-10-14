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
    edit: Option<(sampler_ui_ir::ControlId, f64)>,
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
            if !menu && !["load","stream","mic","purge","select"].iter().any(|key|name.contains(key)) {return None;}
            let items=if let sampler_ui_ir::Kind::Menu{items}=&w.kind {items.iter().take(12).map(|i|json!({"text":i.text,"value":i.value,"visible":i.visible})).collect::<Vec<_>>()}else{Vec::new()};
            Some(json!({"name":w.name,"text":w.text,"control_id_hex":format!("{:032x}",id.0),"value":values.iter().find(|(c,_)|c==id).map(|(_,v)|*v),"enabled":w.enabled,"hidden":w.hidden,"is_menu":menu,"menu_items_first12":items}))
        }).collect();
        return Ok(
            json!({"scope":"read-only authored control metadata discovery; no dynamic loading trigger executed","prebind_tasks":tasks(prebind),"controls":rows,"payload_written":false}),
        );
    }

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
    if let Some((id, value)) = edit {
        if !driver.set_control(id, value) {
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
        json!({"scope":"production promotion, attach, static streamed assembly, real script owner and core render commits; no MIDI notes; not native audio parity or deadline margin","stage_ms":{"translate":translated_ms,"attach":attached_ms,"assemble":assembled_ms,"total":start.elapsed().as_secs_f64()*1000.},"stop_reason":stopped,"deadline_seconds":limit.as_secs(),"deadline_reached":start.elapsed()>=limit,"prebind_tasks":tasks(prebind),"final_tasks":tasks(last),"targets":ids.len(),"initial_static_sources":initial_sources,"committed_source_target_coverage":covered,"targets_with_replaced_source_identity":replaced,"distinct_current_source_ids":distinct.len(),"current_source_frames_sum":source_frames,"current_source_rate_min":if distinct.is_empty(){None}else{Some(rate_min)},"current_source_rate_max":rate_max,"current_source_head_bytes":head_bytes,"current_sources_fully_resident":full_resident,"dynamic_physical_head_quota":{"limit":physical.limit_bytes,"held":physical.held_bytes,"peak":physical.peak_bytes},"initial_static_head_quota":{"limit":static_quota.limit_bytes,"held":static_quota.held_bytes,"peak":static_quota.peak_bytes,"head_assets":initial_readiness.assets,"covered_assets_at_publish":initial_readiness.covered_assets},"source_failure_classes":failure_classes,"script_fault_counts":{"init":faults.init,"runtime":faults.runtime},"core_wake_errors":wake_errors,"core_render_errors":render_errors,"rendered_empty_blocks":blocks,"sounding_voices":runtime.voice_count(),"stream_underruns":runtime.stats().stream_underruns,"observations":observations,"truncated_observations":observations.len()==64,"payload_written":false,"os_cache_scope":"uncontrolled; no global cache eviction","measurement_scope":"unisolated correctness witness; future authored timers beyond the settled observation window are not proven quiescent"}),
    )
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.len() == 3 || args.len() == 4 || args.len() == 5,
        "PATH TOTAL_DEADLINE_SECONDS [--controls | CONTROL_ID_HEX VALUE]"
    );
    let limit = Duration::from_secs(args[2].parse::<u64>().unwrap().clamp(1, 120));
    let controls = args.get(3).is_some_and(|s| s == "--controls");
    let edit = (args.len() == 5).then(|| {
        (
            sampler_ui_ir::ControlId(
                u128::from_str_radix(args[3].trim_start_matches("0x"), 16).unwrap(),
            ),
            args[4].parse::<f64>().unwrap(),
        )
    });
    match run(Path::new(&args[1]), limit, controls, edit) {
        Ok(result) => println!("{result}"),
        Err(stage) => {
            println!("{}", json!({"failed_stage":stage,"payload_written":false}));
            std::process::exit(1);
        }
    }
}
