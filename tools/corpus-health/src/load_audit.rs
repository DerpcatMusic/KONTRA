//! Source-free production loader evidence. One input per fresh process permits
//! RSS attribution; the caller shards processes through kontakto-heavy.
use sampler_ir as ir;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Instant,
};

fn counts(instrument: &ir::Instrument) -> Value {
    let mut features = BTreeMap::<String, usize>::new();
    for gap in &instrument.unsupported {
        *features
            .entry(format!("{:?}: {}", gap.reason, gap.feature))
            .or_default() += 1;
    }
    let mut slots = BTreeMap::<String, usize>::new();
    let mut targets = BTreeMap::<String, usize>::new();
    if let Some(records) = &instrument.dsp_slots {
        for slot in records {
            *slots
                .entry(format!(
                    "{:?}/{} enabled={} {:?}",
                    slot.kind, slot.module, slot.enabled, slot.disposition
                ))
                .or_default() += 1;
            for target in &slot.targets {
                *targets
                    .entry(format!(
                        "{} enabled={} {:?}",
                        target.parameter, target.enabled, target.disposition
                    ))
                    .or_default() += 1;
            }
        }
    }
    let source = instrument.kontakt_objects.as_ref().map(|objects| {
        let sampleless = objects.zones.iter().filter(|z| z.filename_id < 0).count();
        let muted = objects.groups.iter().filter(|g| g.muted).count();
        let tails = objects.program.unknown_tail.len()
            + objects.groups.iter().map(|g| g.unknown_tail.len() + g.criteria_unknown_tail.len()
                + g.source.as_ref().map_or(0, |s| s.private_tail.len())).sum::<usize>()
            + objects.zones.iter().map(|z| z.unknown_tail.len()).sum::<usize>();
        let loops = objects.zones.iter().flat_map(|z| &z.loops).filter(|l| l.mode != 0).count();
        let modes: BTreeSet<_> = objects.groups.iter().filter_map(|g| g.source.as_ref().map(|s| s.mode)).collect();
        json!({"groups":objects.groups.len(),"muted_groups":muted,"zones":objects.zones.len(),
            "sampleless_zones":sampleless,"active_loops":loops,"source_modes":modes,
            "retained_unclassified_tail_bytes":tails,
            "program_key_range":[objects.program.low_key, objects.program.high_key],
            "program_velocity_range":[objects.program.low_velocity, objects.program.high_velocity],
            "nondefault_zone_tune":objects.zones.iter().filter(|z|z.zone_tune != 1.).count(),
            "nondefault_loop_tune":objects.zones.iter().flat_map(|z|&z.loops).filter(|l|l.mode != 0 && l.loop_tuning != 1.).count(),
            "counted_loops":objects.zones.iter().flat_map(|z|&z.loops).filter(|l|l.mode != 0 && l.loop_count != 0).count(),
            "group_midi_channels":objects.groups.iter().filter(|g|g.midi_channel >= 0).count(),
            "soloed_groups":objects.groups.iter().filter(|g|g.soloed).count()})
    });
    json!({"groups":instrument.groups.len(),"zones":instrument.zones.len(),
        "assets":instrument.assets.len(),"chains":instrument.chains.len(),"buses":instrument.buses.len(),
        "modulators":instrument.modulators.len(),"routes":instrument.routes.len(),
        "scripts":instrument.behaviors.len(),"source":source,"features":features,
        "dsp_slots":instrument.dsp_slots.as_ref().map(Vec::len),"slot_outcomes":slots,"target_outcomes":targets,
        "unmapped_zones":instrument.source_indices.zones.iter().filter(|z|z.is_none()).count(),
        "unmapped_groups":instrument.source_indices.groups.iter().filter(|g|g.is_none()).count()})
}

pub(super) fn run(args: &[String]) -> i32 {
    let Some(input) = args.first() else {
        eprintln!("load-audit INPUT [--program INDEX] [--stream]");
        return 2;
    };
    let mut program = None;
    let mut streamed = false;
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--program" => match rest.next().and_then(|s| s.parse::<usize>().ok()) {
                Some(p) => program = Some(p),
                None => return 2,
            },
            "--stream" => streamed = true,
            _ => return 2,
        }
    }
    let path = Path::new(input);
    let multi = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("nkm"));
    let programs = match (multi, program) {
        (_, Some(p)) => vec![p],
        (true, None) => match sampler_kontakt::read_multi(path) {
            Ok(m) => (0..m.programs.len()).collect(),
            Err(e) => {
                println!(
                    "{}",
                    json!({"path":input,"ok":false,"stage":format!("{:?}",e.stage())})
                );
                return 1;
            }
        },
        (false, None) => vec![0],
    };
    let mut failed = false;
    for index in programs {
        super::heap::job_start();
        let start = Instant::now();
        let read = if multi {
            sampler_kontakt::read_program(path, index)
        } else {
            sampler_kontakt::read(path)
        };
        let mut record = json!({"format":"kontra-load-audit","schema":1,"path":input,"program":index,
            "streamed":streamed,"native_equivalence":"unknown"});
        match read {
            Ok(k) => {
                let read_ms = start.elapsed().as_secs_f64() * 1000.;
                record["counts"] = counts(&k.instrument);
                record["script_init_faults"] = json!(k.script_init_faults().len());
                record["read_ms"] = json!(read_ms);
                record["rss_after_read_kib"] =
                    json!(super::proc_field("/proc/self/status", "VmRSS:"));
                record["ok"] = json!(true);
                if streamed {
                    let options = sampler_kontakt::Options {
                        library: Some(path.into()),
                        ..Default::default()
                    };
                    let policy = sampler_kontakt::StreamPolicy {
                        lazy: true,
                        head_budget: 8 << 20,
                        decoders: 2,
                        ..Default::default()
                    };
                    let bind_start = Instant::now();
                    match sampler_kontakt::load_read_streamed(k, &options, &policy, |_| {}) {
                        Ok(s) => {
                            record["ready_ms"] =
                                json!(read_ms + bind_start.elapsed().as_secs_f64() * 1000.);
                            record["rss_ready_kib"] =
                                json!(super::proc_field("/proc/self/status", "VmRSS:"));
                            record["head_bytes"] = json!(s.report.head_bytes);
                            record["pool_bytes"] = json!(s.report.pool_bytes);
                            record["sample_full_bytes"] = json!(s.report.full_bytes);
                            record["bound_scripts"] = json!(s.loaded.scripts.len());
                        }
                        Err(e) => {
                            record["ok"] = json!(false);
                            record["stage"] = json!(format!("{:?}", e.stage()));
                            failed = true;
                        }
                    }
                }
            }
            Err(e) => {
                record["ok"] = json!(false);
                record["stage"] = json!(format!("{:?}", e.stage()));
                failed = true;
            }
        }
        record["peak_rss_kib"] = json!(super::proc_field("/proc/self/status", "VmHWM:"));
        record["peak_thread_heap_bytes"] = json!(super::heap::peak());
        record["loadavg"] = json!(std::fs::read_to_string("/proc/loadavg").ok());
        println!("{}", record);
    }
    i32::from(failed)
}
