//! One actual own-plugin instance per process, using the bench_host loader/callback path.
use super::*;
use anyhow::{Context, Result, ensure};
use moose::core::{
    bus_routing::{BusActivation, BusRouting},
    events::RawMidi1,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    sync::Arc,
    time::Duration,
};

const RATE: u64 = 48_000;
const FRAMES: usize = 512;
const CHANNELS: usize = 2 * BUSES;
#[derive(Deserialize)]
struct Item {
    item_id: String,
    #[serde(alias = "format")]
    kind: String,
    path: PathBuf,
    #[serde(default)]
    program: u32,
    member: Option<String>,
    reader: Option<PathBuf>,
    #[serde(default)]
    playable_keys: Vec<u8>,
    key_range: Option<[u8; 2]>,
}
#[derive(Clone, Deserialize)]
struct TraceEvent {
    frame: u64,
    kind: String,
    #[serde(default)]
    port: u8,
    #[serde(default)]
    channel: u8,
    key: Option<u8>,
    key_index: Option<usize>,
    key_role: Option<String>,
    midi: Option<[u8; 3]>,
    #[serde(default)]
    on: bool,
    #[serde(default)]
    velocity: u8,
    #[serde(default)]
    note_id: i32,
    #[serde(default)]
    value: f64,
    tag: String,
}
#[derive(Deserialize)]
struct Trace {
    schema: String,
    sample_rate: u64,
    block_frames: usize,
    playing_frames: u64,
    total_frames: u64,
    events: Vec<TraceEvent>,
    windows: Vec<Value>,
}
fn ns(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn write_json(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)
        .with_context(|| format!("write {}", path.display()))
}
fn arg<'a>(args: &'a [String], key: &str) -> Result<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1].as_str())
        .with_context(|| format!("{key} required"))
}
fn add_event(output: &mut EventList, event: &TraceEvent, origin: u64) -> Result<()> {
    let offset = u32::try_from(event.frame.checked_sub(origin).context("unsorted trace")?)?;
    ensure!(
        offset < FRAMES as u32 && event.channel < 16 && event.port < 4,
        "invalid event address/offset"
    );
    ensure!(
        event.key.is_none_or(|key| key < 128) && event.velocity < 128 && event.value.is_finite(),
        "invalid MIDI key/value"
    );
    ensure!(
        event.kind != "exact_note" || ((0. ..=1.).contains(&event.value) && event.note_id >= -1),
        "invalid exact identity/velocity"
    );
    ensure!(
        event.kind != "poly_pressure"
            || ((0. ..=127.).contains(&event.value) && event.value.fract() == 0.),
        "invalid pressure"
    );
    let body = match event.kind.as_str() {
        "exact_note" => ExactEventBody::Note {
            kind: if event.on {
                ExactNoteKind::On
            } else {
                ExactNoteKind::Off
            },
            address: ExactNoteAddress::from_raw_signed(
                i16::from(event.port),
                i16::from(event.channel),
                i16::from(event.key.context("resolved key required")?),
                event.note_id,
            ),
            velocity: event.value,
        },
        "midi" | "midi_note" | "poly_pressure" => {
            let data = match event.kind.as_str() {
                "midi" => event.midi.context("raw MIDI required")?,
                "midi_note" => [
                    if event.on { 0x90 } else { 0x80 } | event.channel,
                    event.key.context("resolved key required")?,
                    event.velocity,
                ],
                _ => [
                    0xa0 | event.channel,
                    event.key.context("resolved key required")?,
                    event.value as u8,
                ],
            };
            let length = if data[0] >> 4 == 0xc || data[0] >> 4 == 0xd {
                2
            } else {
                3
            };
            ExactEventBody::Midi1 {
                port: u16::from(event.port),
                message: RawMidi1::new(data, length).context("invalid raw MIDI bytes")?,
            }
        }
        other => anyhow::bail!("unknown trace event kind {other}"),
    };
    output
        .try_push_exact(ExactEvent::new(offset, body))
        .map_err(|error| anyhow::anyhow!("input capacity {error:?}"))?;
    Ok(())
}
fn process(
    dsp: &mut Dsp,
    params: &SamplerParams,
    events: &EventList,
    output: &mut EventList,
    data: &mut [[f32; FRAMES]; 2 * BUSES],
    meter: fn(bool) -> [u64; 4],
) -> (ProcessStatus, u64, u64, [u64; 4]) {
    let mut channels: Vec<_> = data
        .iter_mut()
        .map(|channel| channel.as_mut_slice())
        .collect();
    let mut buffer = AudioBuffer::from_slices_checked(&[], &mut channels, FRAMES);
    let transport = TransportInfo::default();
    let mut routing = BusRouting::new();
    for _ in 0..BUSES {
        routing.push_output(2, BusActivation::Active);
    }
    output.clear();
    let mut cx =
        ProcessContext::new(&transport, RATE as f64, FRAMES, output).with_bus_routing(routing);
    let cpu = thread_cpu();
    let before = meter(true);
    let started = Instant::now();
    let status = Sampler::process(dsp, params, &mut buffer, events, &mut cx);
    let wall = ns(started.elapsed());
    let after = meter(false);
    (
        status,
        wall,
        ((thread_cpu() - cpu).max(0.) * 1e9) as u64,
        std::array::from_fn(|index| after[index] - before[index]),
    )
}
fn owners(dsp: &Dsp) -> Value {
    let core = dsp.rack.parts.iter().enumerate().filter_map(|(slot, part)| {
        let engine = part.instrument()?;
        let census = engine.voice_census();
        let cc = engine.cc_state();
        let mut roots = Vec::new();
        let mut index = 0;
        while let Some((note, pending)) = part.host_note_at(index) {
            roots.push(json!({"port":note.port,"channel":note.channel,"key":note.key,"id":note.id,
                "clap":note.clap,"pending":pending})); index += 1;
        }
        Some(json!({"slot":slot,"voices":engine.active_voices(),"audible_voices":engine.audible_voices(),
            "underruns":engine.underruns(),"dropped_commands":engine.dropped_commands(),
            "host_note_drops":engine.host_note_drops(),"pending_commands_writes_releases":engine.pending_work(),
            "held_keys":(0..16).map(|channel|(0..128).filter(|&key| engine.key_down(channel,key)).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "controllers":(0..16).map(|channel|json!({"channel":channel,"cc1":cc[channel][1],"cc11":cc[channel][11],
                "cc64":cc[channel][64],"cc66":cc[channel][66],"cc67":cc[channel][67],"cc74":cc[channel][74],"cc91":cc[channel][91]})).collect::<Vec<_>>(),
            "voice_identity":census.iter().map(|voice|json!({"event":voice.event.0,"note":voice.note,
                "channel":voice.channel,"physical_channel":voice.input_channel,"owner":voice.owner,
                "held":voice.held,"released":voice.released,"release_trigger":voice.release_trigger,
                "group":voice.group,"phase":format!("{:?}",voice.phase),"gain":voice.gain,"envelope":voice.envelope})).collect::<Vec<_>>(),
            "host_roots":roots,
            "script":engine.script().map(|runtime| crate::diagnostics::script_runtime_report(runtime,&[])),
        }))
    }).collect::<Vec<_>>();
    let native = dsp.uvi.iter().enumerate().filter_map(|(slot, audio)| audio.as_ref().map(|audio| {
        let endpoint = audio.slot();
        json!({"slot":slot,"frame":endpoint.frame(),"latency_frames":endpoint.latency_frames(),
            "voices":endpoint.active_voices(),"underruns":endpoint.underruns(),"error":endpoint.error().map(|error|format!("{error:?}"))})
    })).collect::<Vec<_>>();
    json!({"core":core,"native":native,"pending_terminal_notes":dsp.pending_host_note_ends.len(),
        "unsupported_host_expression":dsp.unsupported_host_expression,"unsupported_note_brightness":dsp.unsupported_note_brightness,
        "host_note_end_rejections":dsp.host_note_end_rejections,"alignment_overflows":dsp.align.overflows(),
        "resident_pcm_bytes":crate::engine::resident_bytes(),"memory_budget_bytes":crate::engine::memory_budget()})
}
fn full_reports(params: &SamplerParams) -> Value {
    json!({"plugin":params.diagnostic_report(),"uvi_workers_full":params.shared.uvi_controls.lock().unwrap()
        .runtime_diagnostic_report(Duration::from_secs(2)),"journal":crate::diagnostics::snapshot()})
}
/// Runs one externally frozen item/trace and retains partial artifacts on failures.
pub fn run(args: &[String], meter: fn(bool) -> [u64; 4]) -> Result<()> {
    let item_bytes = fs::read(arg(args, "--item")?)?;
    let item: Item = serde_json::from_slice(&item_bytes)?;
    let trace_bytes = fs::read(arg(args, "--trace")?)?;
    let mut trace_value: Value = serde_json::from_slice(&trace_bytes)?;
    let output = PathBuf::from(arg(args, "--output")?);
    fs::create_dir_all(&output)?;
    ensure!(
        !output.join("result.json").exists(),
        "result already exists"
    );
    let mut trace: Trace = serde_json::from_slice(&trace_bytes)?;
    ensure!(
        trace.schema == "universal-jam.trace.v1"
            && trace.sample_rate == RATE
            && trace.block_frames == FRAMES
            && trace.playing_frames == RATE * 60
            && trace.total_frames >= trace.playing_frames
            && trace.total_frames <= RATE * 180,
        "bounded 60s/48k/512 trace required"
    );
    ensure!(
        trace
            .events
            .windows(2)
            .all(|pair| pair[0].frame <= pair[1].frame)
            && trace
                .events
                .iter()
                .all(|event| event.frame < trace.total_frames),
        "trace event time invalid"
    );
    let params = Arc::new(SamplerParams::new());
    params.shared.ensure_parts(1);
    let mut part = Part {
        path: item.path.to_string_lossy().into_owned(),
        program: item.program,
        channel: -1,
        port: 0,
        ..Default::default()
    };
    match item.kind.as_str() {
        "kontakt" => {}
        "uvi" => {
            let bank = crate::uvi::ufs::Ufs::open(&item.path)?;
            part.path.clear();
            part.uvi = Some(library::UviSource {
                bank: item.path.clone(),
                bank_uuid: bank.header.uuid,
                member: item.member.clone().context("UVI member required")?,
            });
            if let Some(reader) = &item.reader {
                params
                    .shared
                    .libraries
                    .configure_uvi_reader(Some(reader.clone()));
            }
        }
        other => anyhow::bail!("unknown item kind {other}"),
    }
    params.selection.write().unwrap().parts = vec![part];
    let mut dsp = Dsp::default();
    Sampler::reset(&mut dsp, &params, &AudioConfig::new(RATE as f64, FRAMES));
    let stop = Arc::new(AtomicBool::new(false));
    let passes = Arc::new(AtomicU64::new(0));
    let loader = {
        let (params, stop, passes, path) = (
            params.clone(),
            stop.clone(),
            passes.clone(),
            output.join("loader-service.jsonl"),
        );
        std::thread::spawn(move || -> Result<()> {
            let mut writer = BufWriter::new(File::create(path)?);
            while !stop.load(Ordering::Acquire) {
                let began = Instant::now();
                let cpu = thread_cpu();
                Load.run(&params);
                params.shared.publish_live(false);
                let row = json!({"pass":passes.fetch_add(1,Ordering::AcqRel),"wall_ns":ns(began.elapsed()),
                    "cpu_ns":((thread_cpu()-cpu).max(0.)*1e9) as u64,"scope":"existing Load service plus publish_live; no UI paint"});
                serde_json::to_writer(&mut writer, &row)?;
                writeln!(writer)?;
                std::thread::sleep(Duration::from_millis(50));
            }
            writer.flush()?;
            Ok(())
        })
    };
    let mut data = Box::new([[0f32; FRAMES]; 2 * BUSES]);
    let none = EventList::with_capacity(0);
    let mut outgoing = EventList::with_capacity(256);
    let mut pace = crate::engine::Pace::start();
    let load_start = Instant::now();
    let mut load_blocks = 0u32;
    let mut loaded = false;
    while load_start.elapsed() < Duration::from_secs(120) {
        process(&mut dsp, &params, &none, &mut outgoing, &mut data, meter);
        loaded = if item.kind == "uvi" {
            dsp.uvi[0].is_some()
        } else {
            dsp.rack.parts[0]
                .instrument()
                .is_some_and(|engine| engine.bank().is_some())
        };
        if loaded {
            break;
        }
        if passes.load(Ordering::Acquire) > 0 {
            let view = params.shared.view.lock().unwrap();
            if view.parts.first().is_some_and(|part| {
                !part.loading
                    && (part
                        .load_report
                        .as_deref()
                        .and_then(|report| report.get("status"))
                        .and_then(Value::as_str)
                        == Some("failed")
                        || item.kind == "uvi"
                            && part
                                .uvi_activation
                                .as_ref()
                                .is_some_and(|activation| !activation.published))
            }) {
                break;
            }
        }
        load_blocks += 1;
        pace.until(Duration::from_secs_f64(
            f64::from(load_blocks) * FRAMES as f64 / RATE as f64,
        ));
    }
    let load_elapsed = ns(load_start.elapsed());
    let before = full_reports(&params);
    write_json(&output.join("before.json"), &before)?;
    let mut keys = item.playable_keys.clone();
    if keys.is_empty() {
        if let Some(bank) = dsp.rack.parts[0].instrument().and_then(Engine::bank) {
            keys = (0..128u8)
                .filter(|key| {
                    bank.zones()
                        .iter()
                        .any(|zone| zone.low_key <= *key && *key <= zone.high_key)
                })
                .collect();
        } else {
            let view = params.shared.view.lock().unwrap();
            if let Some(part) = view.parts.first() {
                // Existing UI sentinel means authored valid input keys, not arbitrary highlight colors.
                if let Some(ui) = &part.uvi_ui {
                    keys = ui
                        .key_colours
                        .colours
                        .iter()
                        .filter_map(|(&key, colour)| {
                            colour.eq_ignore_ascii_case("#00FFFFFF").then_some(key)
                        })
                        .collect();
                }
                if keys.is_empty()
                    && let Some(mapping) = part
                        .uvi_activity
                        .as_ref()
                        .and_then(|activity| activity.mapping.as_ref())
                {
                    keys = mapping
                        .sample_keys
                        .iter()
                        .enumerate()
                        .filter_map(|(key, &sampled)| sampled.then_some(key as u8))
                        .collect();
                }
            }
            if keys.is_empty()
                && let Some([low, high]) = item.key_range
            {
                keys = (low..=high).collect();
            }
        }
    }
    if let Some([low, high]) = item.key_range {
        keys.retain(|key| low <= *key && *key <= high);
    }
    keys.sort_unstable();
    keys.dedup();
    let musical_first = keys
        .iter()
        .copied()
        .filter(|key| keys.iter().any(|&next| next >= key.saturating_add(2)))
        .min_by_key(|key| key.abs_diff(60))
        .or_else(|| keys.first().copied());
    let musical_next = musical_first
        .and_then(|first| {
            keys.iter()
                .copied()
                .filter(|&key| key > first)
                .min_by_key(|key| key.abs_diff(first.saturating_add(2)))
        })
        .or(musical_first);
    let mut failure = if loaded && !keys.is_empty() {
        None
    } else {
        Some(
            if loaded {
                "authored playable keys unavailable"
            } else {
                "actual plugin source failed to load"
            }
            .to_owned(),
        )
    };
    if failure.is_none() {
        for (index, event) in trace.events.iter_mut().enumerate() {
            if let Some(role) = event.key_role.as_deref() {
                event.key = Some(match role {
                    "spine" => musical_first.context("spine key")?,
                    "spine_nearby" => musical_next.context("nearby spine key")?,
                    _ => anyhow::bail!("unknown key role {role}"),
                });
                trace_value["events"][index]["key"] = json!(event.key);
                trace_value["events"][index]
                    .as_object_mut()
                    .context("trace event object")?
                    .remove("key_role");
                trace_value["events"][index]
                    .as_object_mut()
                    .context("trace event object")?
                    .remove("key_index");
            } else if let Some(key_index) = event.key_index {
                event.key = Some(keys[key_index % keys.len()]);
                trace_value["events"][index]["key"] = json!(event.key);
                trace_value["events"][index]
                    .as_object_mut()
                    .context("trace event object")?
                    .remove("key_index");
            }
        }
    }
    write_json(&output.join("trace-resolved.json"), &trace_value)?;
    let resolved_bytes = fs::read(output.join("trace-resolved.json"))?;
    let blocks = trace.total_frames.div_ceil(FRAMES as u64) as usize;
    let mut captured_frames = 0usize;
    let mut callback_writer = BufWriter::new(File::create(output.join("callbacks.jsonl"))?);
    let mut raw = BufWriter::new(File::create(output.join("audio.f32le"))?);
    let mut output_writer = BufWriter::new(File::create(output.join("output-events.jsonl"))?);
    let mut processed_blocks = 0usize;
    let mut checkpoints = Vec::with_capacity(blocks / 90 + 2);
    let mut events = EventList::with_capacity(256);
    let mut index = 0;
    let began = Instant::now();
    let before_owners = owners(&dsp);
    if failure.is_none() {
        for block in 0..blocks {
            let frame = block as u64 * FRAMES as u64;
            let target = began + Duration::from_nanos(frame * 1_000_000_000 / RATE);
            if let Some(wait) = target.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
            let start = Instant::now();
            let late = ns(start.saturating_duration_since(target));
            events.clear();
            let first = index;
            while index < trace.events.len() && trace.events[index].frame < frame + FRAMES as u64 {
                add_event(&mut events, &trace.events[index], frame)?;
                index += 1;
            }
            let prepared_ns = ns(start.elapsed());
            let usage_before = thread_usage();
            let (status, wall, cpu, heap) =
                process(&mut dsp, &params, &events, &mut outgoing, &mut data, meter);
            let usage_after = thread_usage();
            let end_late = ns(Instant::now().saturating_duration_since(
                target + Duration::from_nanos(FRAMES as u64 * 1_000_000_000 / RATE),
            ));
            let valid_frames = (trace.total_frames - frame).min(FRAMES as u64) as usize;
            for at in 0..valid_frames {
                for channel in data.iter() {
                    let sample = channel[at];
                    raw.write_all(&sample.to_le_bytes())?;
                }
            }
            captured_frames += valid_frames;
            processed_blocks += 1;
            for (ordinal, event) in outgoing.exact_iter().enumerate() {
                let identity = if let ExactEventBody::Note {
                    kind,
                    address,
                    velocity,
                } = event.body()
                {
                    json!({"kind":format!("{kind:?}"),"port":address.port.raw_i32(),"channel":address.channel.raw_i32(),
                        "key":address.key.raw_i32(),"note_id":address.note_id.raw_i32(),"velocity":velocity})
                } else {
                    json!({"kind":"other_exact","body":format!("{:?}",event.body())})
                };
                serde_json::to_writer(
                    &mut output_writer,
                    &json!({"callback_frame":frame,"offset":event.sample_offset(),
                    "absolute_frame":frame+u64::from(event.sample_offset()),"ordinal":ordinal,"identity":identity}),
                )?;
                writeln!(output_writer)?;
            }
            let native = params.shared.uvi_controls.lock().unwrap().jam_metrics();
            let row = json!({"frame":frame,"frames":FRAMES,"valid_frames":valid_frames,"phase":if frame<trace.playing_frames {"playing"} else {"tail"},
                "event_tags":trace.events[first..index].iter().map(|event|&event.tag).collect::<Vec<_>>(),
                "wall_ns":wall,"cpu_ns":cpu,"off_cpu_ns":wall.saturating_sub(cpu),"host_prepare_ns":prepared_ns,
                "scheduler_lateness_ns":late,"deadline_ns":FRAMES as u64*1_000_000_000/RATE,"deadline_completion_lateness_ns":end_late,
                "deadline_miss":wall>FRAMES as u64*1_000_000_000/RATE,"status":format!("{status:?}"),
                "minor_faults":usage_after[0]-usage_before[0],"voluntary_context_switches":usage_after[1]-usage_before[1],
                "involuntary_context_switches":usage_after[2]-usage_before[2],
                "heap_alloc":heap[0],"heap_alloc_zeroed":heap[1],"heap_free":heap[2],"heap_realloc":heap[3],"heap_ops":heap.iter().sum::<u64>(),
                "voices":dsp.rack.parts.iter().map(|part|part.active_voices()).sum::<usize>(),
                "audible_voices":dsp.rack.parts.iter().map(|part|part.audible_voices()).sum::<usize>(),
                "dropouts":params.shared.dropouts.load(Ordering::Relaxed),"resident_pcm_bytes":crate::engine::resident_bytes(),
                "rss_plus_swap_mib":rss_mib(),"process_cpu_seconds":cpu_clock(2),"worker":native,
                "pending_terminal_notes":dsp.pending_host_note_ends.len(),"output_exact_records":outgoing.exact_len(),
                "live_diagnostic_dropped":params.shared.diagnostic_dropped.load(Ordering::Relaxed)});
            serde_json::to_writer(&mut callback_writer, &row)?;
            writeln!(callback_writer)?;
            if block % 94 == 0 {
                callback_writer.flush()?;
                raw.flush()?;
                output_writer.flush()?;
            }
            if block % 94 == 0 {
                checkpoints.push(json!({"frame":frame,"owners":owners(&dsp)}));
            }
            if status == ProcessStatus::Error {
                failure = Some(format!("plugin process Error at frame {frame}"));
                break;
            }
        }
    }
    stop.store(true, Ordering::Release);
    let service_result = loader
        .join()
        .map_err(|_| anyhow::anyhow!("loader thread panicked"))?;
    if let Err(error) = service_result {
        failure = Some(format!("loader service: {error:#}"));
    }
    let after_owners = owners(&dsp);
    let after = full_reports(&params);
    write_json(&output.join("after.json"), &after)?;
    write_json(
        &output.join("ownership-checkpoints.json"),
        &json!({"before":before_owners,"during":checkpoints,"after":after_owners}),
    )?;
    callback_writer.flush()?;
    output_writer.flush()?;
    raw.flush()?;

    // After the last PCM/owner observations, join the existing workers off callback.
    // This preserves an early typed failure and makes its final trace durable.
    params
        .shared
        .uvi_controls
        .lock()
        .unwrap()
        .jam_join_workers();
    write_json(&output.join("worker-final.json"), &full_reports(&params))?;
    let flush_error = crate::diagnostics::flush(Duration::from_secs(5)).err();
    let diagnostic_snapshot = crate::diagnostics::snapshot();
    let result = json!({"schema":"universal-jam.result.v1","item_id":item.item_id,"trace_sha256":hash(&trace_bytes),
        "resolved_trace_sha256":hash(&resolved_bytes),"input_identity":serde_json::from_slice::<Value>(&item_bytes)?,
        "load":{"status":if loaded {"loaded"} else {"failed"},"elapsed_ns":load_elapsed,"error":if loaded {None} else {failure.as_deref()},"warnings":before},
        "runtime":{"status":if failure.is_some(){"failed"}else{"completed"},"error":failure,"processed_blocks":processed_blocks,"dispatched_events":index},
        "diagnostics":diagnostic_snapshot,"diagnostics_full_paths":["before.json","after.json","worker-final.json","raw-journal"],
        "observations_jsonl":"callbacks.jsonl","pcm":{"path":"audio.f32le","wav":null,"format":"interleaved-f32le", "sample_rate":RATE,
            "frames":captured_frames,"channels":CHANNELS,"channel_mapping":(0..CHANNELS).map(|channel|json!({"channel":channel,"bus":channel/2,"side":if channel%2==0{"left"}else{"right"}})).collect::<Vec<_>>(),"latency_frames":dsp.uvi_latency},"coverage":{"windows":trace.windows,"playable_keys":keys,
            "pedal":"requested in saved trace; observed state in ownership checkpoints", "mpe":"requested RPN6/member controls; unsupported delivery diagnostics retained","ui":"loader UI service only; separate parent paint harness required","effect_segments":"CC91; authored graph unchanged"},
        "availability":{"callback_heap":"System allocator alloc/zeroed/free/realloc on invoking callback thread only",
            "callback_cpu":"existing bench_host thread CPU clock; platform implementation provenance",
            "worker":"existing atomic Worker stats and shared queues; sampled highwater lower bound",
            "core_ownership":"existing root refs, voice census, cc_state sampled outside callback",
            "unavailable":["per-core disk bytes/decode nanoseconds/stream depth","per-KSP callback counts and GC","native UI paint/GPU counters","exact host NOTE_END refusal (own ProcessContext capture only)"]},
        "snapshot":{"before":"before.json","after":"after.json","ownership":"ownership-checkpoints.json","output_events":"output-events.jsonl"},"log_flush_error":flush_error,
        "source_build":crate::diagnostics::build_identity(),"clock":{"trace_origin":"first playing callback; source activation/load blocks precede frame0","callback_extent":FRAMES,"pcm_extent":"exact total_frames, last callback cropped","pacing":"absolute host trace clock; observer overhead affects subsequent lateness"},
        "scope":"one own Sampler+Load path, realtime process mode; no vendor player or audio device"});
    let mut result = result;
    let measured = [
        (
            "cpu",
            "invoking thread CPU clock; worker atomic CPU samples separately",
        ),
        ("wall", "Sampler::process only"),
        ("scheduler", "absolute scheduled callback start/completion"),
        (
            "heap",
            "invoking callback thread System allocator operations",
        ),
        (
            "rss",
            "existing RSS+swap process sample; includes harness retention",
        ),
        ("resident_pcm", "own core and native resource atomic census"),
        (
            "voices",
            "core active/audible and native historical worker census",
        ),
        (
            "stream_underruns",
            "actual core/native counters; polling emptiness labeled separately",
        ),
    ];
    for (name, scope) in measured {
        result["availability"][name] = json!({"status":"measured","scope":scope});
    }
    let partial = [
        ("cache", "native initialization cache/decode report"),
        (
            "decode",
            "native loaded paths and unique decode counts; no core timing",
        ),
        (
            "physical_held",
            "core key/root checkpoints; native worker runtime reports",
        ),
        ("generated", "core voice provenance/native emitted counts"),
        (
            "pedal_held",
            "controller+voice/root snapshots; no universal per-pedal root census",
        ),
        (
            "completions",
            "captured own output End events; no real CLAP acceptance adapter",
        ),
        (
            "queues",
            "native shared queue depth samples plus core pending work; not every local queue",
        ),
        ("routing", "physical/logical voice provenance checkpoints"),
        (
            "wait_tasks",
            "native ScriptActivity suspended_tasks; KSP unavailable",
        ),
        (
            "gc",
            "native explicit prune timing only; automatic/authored GC excluded",
        ),
        (
            "script_faults",
            "full journal and runtime reports; retention/drop counters preserved",
        ),
        (
            "effects",
            "authored FX graph preserved; CC91 stimulus, no arbitrary GUI FX edits",
        ),
        (
            "tails",
            "full PCM plus final owner checkpoint, authored release duration may exceed tail",
        ),
        (
            "mpe",
            "raw RPN/member stimulus and unsupported diagnostics; no universal implementation claim",
        ),
        ("pedal", "raw64/66/67 stimulus and checkpoints"),
    ];
    for (name, scope) in partial {
        result["availability"][name] = json!({"status":"partial","scope":scope});
    }
    for (name, reason) in [
        ("disk_read", "no existing per-core disk-byte observer"),
        (
            "ui",
            "loader/publish service measured; native paint/GPU requires separate context",
        ),
    ] {
        result["availability"][name] = json!({"status":"unavailable","reason":reason});
    }
    write_json(&output.join("result.json"), &result)?;
    if let Some(error) = failure {
        anyhow::bail!("{error}; partial artifacts retained");
    }
    Ok(())
}
