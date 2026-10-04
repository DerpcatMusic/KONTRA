//! Offline commands; reader constants and content state stay local.
use super::*;
use std::io::Write;
use std::{collections::HashMap, rc::Rc};

pub(crate) use super::access::ReaderNamespaces;

pub(crate) use super::access::ContentState;

fn options(args: &[String], start: usize) -> Result<(ReaderNamespaces, Option<ContentState>)> {
    let (mut reader, mut state) = (None, None);
    let mut i = start;
    while i < args.len() {
        let value = args.get(i + 1).context("Missing UVI option value")?;
        match args[i].as_str() {
            "--reader" => {
                ensure!(reader.is_none(), "Repeated --reader");
                reader = Some(ReaderNamespaces::open(Path::new(value))?);
            }
            "--content-key-file" => {
                ensure!(state.is_none(), "Repeated --content-key-file");
                state = Some(ContentState::open(Path::new(value))?);
            }
            other => anyhow::bail!("Unknown UVI option {other}"),
        }
        i += 2;
    }
    Ok((
        reader.context("UFS requires --reader <official UVIWorkstationx64.exe>")?,
        state,
    ))
}

fn open_library(args: &[String], start: usize) -> Result<(ReaderNamespaces, library::Library)> {
    let (reader, state) = options(args, start)?;
    let path = Path::new(&args[1]);
    let library = library::Library::open(path, &reader.metadata, state.as_ref().map(|s| s.key))?;
    if let Some(identity) = state.as_ref().and_then(|s| s.bank.as_ref()) {
        ensure!(
            identity == &library.bank.header.bank_name
                || std::fs::canonicalize(identity).ok() == Some(std::fs::canonicalize(path)?),
            "Content-state file belongs to a different bank"
        );
    }
    Ok((reader, library))
}

fn member_bytes(args: &[String], start: usize) -> Result<(ReaderNamespaces, Vec<u8>)> {
    let (reader, library) = open_library(args, start)?;
    let member = library::resolve_member(&library.directory, &args[2])?;
    if args[0] == "uvi-program" || args[2].to_ascii_lowercase().ends_with(".uvip") {
        ensure!(
            member.size <= crypto::PROGRAM_XML_LIMIT as u64,
            "UVI Program member exceeds 32 MiB limit"
        );
    }
    let bytes = library.read(member)?;
    Ok((reader, bytes))
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = std::fs::remove_file(path);
        return Err(error.into());
    }
    Ok(())
}

fn processing_end(end: u64) -> Result<u64> {
    Ok(end
        .checked_add(255)
        .context("UVI padded timeline overflow")?
        / 256
        * 256)
}

/// An explicit control-thread census. Counts do not imply painted controls or
/// exercised callbacks, and no captions, resource paths or Lua values are emitted.
#[cfg(any(feature = "plugin", test))]
fn ui_panel_report(snapshot: &host::UiSnapshot, stamp: worker::Stamp) -> serde_json::Value {
    serde_json::json!({"processor":snapshot.processor,"outcome":"captured",
        "stamp":{"epoch":stamp.epoch,"generation":stamp.generation,"frame":stamp.frame},
        "width":snapshot.root.width,"height":snapshot.root.height,
        "performance_view":snapshot.root.performance_view,"widgets":snapshot.widgets.len(),
        "visible_widgets":snapshot.widgets.iter().filter(|w|w.effective_visible).count(),
        "widgets_with_changed_callback":snapshot.widgets.iter().filter(|w|w.has_changed_callback).count()})
}

#[cfg(feature = "plugin")]
fn diagnose_ui(
    worker: &worker::Worker,
    assets: Result<super::ui_assets::UiAssets>,
) -> serde_json::Value {
    use std::time::{Duration, Instant};
    let processors = worker.ui_processors();
    let total = processors.len();
    // Bound snapshot waiting; artwork has its own byte/reference limits. The
    // worker retains only one coalesced request/reply.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut panels = Vec::new();
    let mut captured = 0usize;
    let mut assets = assets;
    for processor in processors.into_iter().take(64) {
        if Instant::now() >= deadline {
            break;
        }
        let request = match worker.request_ui_snapshot(processor) {
            Ok(request) => request,
            Err(error) => {
                panels.push(
                    serde_json::json!({"processor":processor,"outcome":"rejected",
                    "reason":format!("{error:?}")}),
                );
                continue;
            }
        };
        loop {
            if let Some(reply) = worker.poll_ui_snapshot() {
                if reply.request != request
                    || reply.processor != processor
                    || reply.stamp.epoch != 1
                    || reply.stamp.generation != 1
                {
                    continue;
                }
                match reply.snapshot {
                    Ok(snapshot) => {
                        if let Ok(assets) = &mut assets {
                            assets.refresh(std::slice::from_ref(&snapshot));
                        }
                        panels.push(ui_panel_report(&snapshot, reply.stamp));
                        captured += 1;
                    }
                    Err(error) => panels.push(serde_json::json!({"processor":processor,
                        "outcome":"unavailable","reason":format!("{error:?}")})),
                }
                break;
            }
            if Instant::now() >= deadline || worker.status() != worker::Status::Ready {
                panels.push(serde_json::json!({"processor":processor,"outcome":"no_reply"}));
                break;
            }
            std::thread::park_timeout(Duration::from_millis(1));
        }
    }
    let artwork = match &mut assets {
        Ok(assets) => {
            let diagnostics = assets.diagnostics();
            serde_json::json!({"outcome":if diagnostics.failed == 0 && diagnostics.limited == 0 {
                "complete"
            } else {"partial"},"pictures":assets.refresh(&[]).len(),
                "fonts":assets.fonts().len(),"failed_font_references":diagnostics.font_failed,
                "failed_references":diagnostics.failed,"limited_references":diagnostics.limited,
                "resident_bytes":assets.resident_bytes()})
        }
        Err(error) => serde_json::json!({"outcome":"open_failed","reason":error.to_string()}),
    };
    serde_json::json!({"outcome":if captured == total && artwork["outcome"] == "complete" {
        "complete"
    } else {"partial"},"requested_processors":total,"captured_processors":captured,
        "unattempted_processors":total - panels.len(),"processor_limit":64,"panels":panels,
        "artwork":artwork,"drawn":false,"interactions_exercised":false,"native_compared":false})
}

/// Exercises the same fixed packet API intended for the live audio handoff.
/// Waiting, preparation and controller destruction here are explicitly offline.
fn play_worker(
    args: &[String],
    start: usize,
    inputs: &[script::Input],
    end: u64,
    rate: u32,
) -> Result<()> {
    use std::time::{Duration, Instant};
    use worker::{PacketError, Request, Stamp, StartConfig, Worker};
    let (reader, state) = options(args, start)?;
    let mut worker = Worker::start(
        StartConfig {
            bank: PathBuf::from(&args[1]),
            expected_bank_uuid: None,
            member: args[2].clone(),
            metadata_namespace: reader.metadata,
            program_namespace: reader.program,
            content_key: state.as_ref().map(|state| state.key),
            content_bank: state.and_then(|state| state.bank),
            sample_rate: rate,
        },
        1,
        1,
    )?;
    worker.wait_ready(Duration::from_secs(60))?;
    let diagnostics = worker.diagnostics();
    let padded_end = processing_end(end)?;
    let (mut input_index, mut commands, mut host_commands, mut logs, mut dropped_logs) =
        (0, 0u64, 0u64, 0u64, 0u64);
    let peak = write_wav(Path::new(&args[3]), rate, |write| {
        for frame in (0..padded_end).step_by(worker::BLOCK_FRAMES) {
            let stamp = Stamp {
                epoch: 1,
                generation: 1,
                frame,
            };
            let next_input =
                inputs.partition_point(|input| input.frame < frame + worker::BLOCK_FRAMES as u64);
            let request = Request::new(stamp, &inputs[input_index..next_input])
                .map_err(|error| anyhow::anyhow!("Invalid UVI worker packet: {error:?}"))?;
            worker.realtime().try_submit(request).map_err(|rejected| {
                anyhow::anyhow!("UVI worker packet rejected: {:?}", rejected.reason)
            })?;
            let started = Instant::now();
            let output = loop {
                match worker.realtime().try_receive(stamp) {
                    Ok(output) => break output,
                    Err(PacketError::Underrun) => {
                        ensure!(
                            started.elapsed() < Duration::from_secs(60),
                            "UVI worker output timed out"
                        );
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => anyhow::bail!(
                        "UVI worker output failed: {error:?}; {}",
                        worker.private_failure().unwrap_or_default()
                    ),
                }
            };
            commands += u64::from(output.commands);
            host_commands += u64::from(output.host_commands);
            logs += u64::from(output.logs);
            dropped_logs += u64::from(output.dropped_logs);
            for sample in output
                .audio
                .into_iter()
                .take(end.saturating_sub(frame) as usize)
            {
                write(sample)?;
            }
            input_index = next_input;
        }
        Ok(())
    })?;
    let mut runtime_report = worker.runtime_diagnostic_report(Duration::from_millis(500));
    worker.stop();
    let stats = worker.stats();
    // The retained node snapshot precedes stop; these aggregate fields show
    // the worker's current lifecycle after the endpoint has been destroyed.
    runtime_report["status"] = serde_json::to_value(worker.status())?;
    runtime_report["stats"] = serde_json::to_value(stats)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"frames":end,"processed_frames":padded_end,"sample_rate":rate,"block_frames":worker::BLOCK_FRAMES,"worker":true,"runtime_report":runtime_report,"peak":peak,"event_commands":commands,"host_commands":host_commands,"diagnostics":diagnostics,"private_log_messages":logs,"dropped_logs":dropped_logs,"worker_initialization_ns":stats.initialization_ns,"worker_render_ns":stats.render_ns,"worker_max_render_ns":stats.max_render_ns,"worker_render_deadline_misses":stats.render_deadline_misses,"worker_backpressure":stats.backpressure,"worker_packet_polls":stats.underruns,"worker_errors":stats.errors})
        )?
    );
    Ok(())
}

pub fn run(args: &[String]) -> Result<()> {
    let path = Path::new(args.get(1).context("UVI command requires a source path")?);
    match args[0].as_str() {
        "uvi-key" => {
            ensure!(
                args.len() == 5 && args[3] == "--reader",
                "Usage: uvi-key <bank.ufs> <new-private-state.json> --reader <official-exe>"
            );
            let (reader, state) = options(args, 3)?;
            ensure!(
                state.is_none(),
                "Key recovery does not accept a content state"
            );
            let bank = ufs::Ufs::open(path)?;
            let directory = bank.decode_directory(&reader.metadata)?;
            let state = super::access::recover_content_state(path, &bank, &directory)?;
            super::access::save_content_state(Path::new(&args[2]), &bank, &state, false)?;
            println!(
                "Saved a private UUID-bound content state after complete PNG CRC verification"
            );
        }
        "uvi-check" | "uvi-play" => {
            let rendering = args[0] == "uvi-play";
            ensure!(
                args.len() >= if rendering { 6 } else { 5 },
                "Usage: uvi-check <bank.ufs> <member.uvip> --reader <exe> [--content-key-file <private-file>]\n       uvi-play <bank.ufs> <member.uvip> <output.wav> --reader <exe> [--content-key-file <private-file>] [--notes 60@0-500:100 | --events timeline.json] [--sample-rate 48000] [--block-frames 256] [--worker]"
            );
            let start = if rendering { 4 } else { 3 };
            let mut args = args.to_vec();
            let dedicated_worker = if let Some(index) = args.iter().position(|s| s == "--worker") {
                ensure!(rendering && index >= start, "Misplaced --worker option");
                args.remove(index);
                ensure!(!args.iter().any(|s| s == "--worker"), "Repeated --worker");
                true
            } else {
                false
            };
            let mut notes = "60@0-500:100".to_owned();
            let mut explicit_notes = false;
            let mut event_file = None;
            let mut rate = 48000u32;
            let mut block_frames = None;
            if let Some(index) = args.iter().position(|s| s == "--sample-rate") {
                ensure!(index >= start, "Misplaced --sample-rate option");
                rate = args
                    .get(index + 1)
                    .context("Missing --sample-rate value")?
                    .parse()?;
                ensure!((8000..=192000).contains(&rate), "Invalid UVI sample rate");
                args.drain(index..=index + 1);
                ensure!(
                    !args.iter().any(|s| s == "--sample-rate"),
                    "Repeated --sample-rate"
                );
            }
            if let Some(index) = args.iter().position(|s| s == "--block-frames") {
                ensure!(
                    rendering && index >= start,
                    "Misplaced --block-frames option"
                );
                let frames: usize = args
                    .get(index + 1)
                    .context("Missing --block-frames value")?
                    .parse()?;
                ensure!(
                    frames > 0 && frames <= rate as usize * 60,
                    "Invalid UVI block length"
                );
                block_frames = Some(frames);
                args.drain(index..=index + 1);
                ensure!(
                    !args.iter().any(|s| s == "--block-frames"),
                    "Repeated --block-frames"
                );
            }
            if let Some(index) = args.iter().position(|s| s == "--notes") {
                ensure!(index >= start, "Misplaced --notes option");
                explicit_notes = true;
                notes = args
                    .get(index + 1)
                    .context("Missing --notes value")?
                    .clone();
                args.drain(index..=index + 1);
                ensure!(!args.iter().any(|s| s == "--notes"), "Repeated --notes");
            }
            if let Some(index) = args.iter().position(|s| s == "--events") {
                ensure!(index >= start, "Misplaced --events option");
                event_file = Some(
                    args.get(index + 1)
                        .context("Missing --events value")?
                        .clone(),
                );
                args.drain(index..=index + 1);
                ensure!(!args.iter().any(|s| s == "--events"), "Repeated --events");
                ensure!(!explicit_notes, "Use either --notes or --events");
            }
            let inputs: Vec<script::Input> = if let Some(file) = event_file {
                serde_json::from_str(&read_text(Path::new(&file))?)
                    .context("Invalid UVI event timeline JSON")?
            } else {
                script::parse_notes_at_rate(&notes, rate)?
            };
            let end = inputs
                .iter()
                .map(|i| i.frame)
                .max()
                .unwrap_or(0)
                .checked_add(u64::from(rate))
                .context("UVI event timeline overflow")?;
            script::validate_sequence(&inputs, end - 1)?;
            if rendering {
                ensure!(
                    inputs.iter().all(player::input_is_valid),
                    "UVI audio transport tempo must be between 1 and 1000 BPM"
                );
            }
            if dedicated_worker {
                ensure!(
                    block_frames.is_none_or(|frames| frames == worker::BLOCK_FRAMES),
                    "The UVI worker uses fixed 256-frame blocks"
                );
                return play_worker(&args, start, &inputs, end, rate);
            }
            let (reader, library) = open_library(&args, start)?;
            let library = Rc::new(library);
            let loaded = library.program(&args[2], &reader.program)?;
            let unsupported = playback::preflight(&loaded.program);
            if rendering {
                ensure!(
                    unsupported.is_empty(),
                    "Native UVI graph preflight failed: {}",
                    serde_json::to_string(&unsupported)?
                );
            }
            let resources = library::BankResources::new(
                library.clone(),
                &loaded.path,
                if rendering {
                    library.samples(&loaded)?
                } else {
                    HashMap::new()
                },
            )?;
            if rendering {
                let mut player =
                    player::Player::new(&loaded.program, library.modules()?, resources, rate)?;
                let diagnostics = player.diagnostics();
                let planned = player.requires_planned_segments();
                // Bulk and smaller blocks use the same VM/DSP processing horizon.
                // Planned-source lookahead can affect audible samples before a
                // callback in the padded tail; do not truncate those commands.
                let block_frames = block_frames.unwrap_or_else(|| {
                    let limit = rate as usize * 60;
                    if planned { limit / 256 * 256 } else { limit }
                });
                ensure!(
                    !planned || block_frames.is_multiple_of(256),
                    "This UVI graph requires --block-frames to be a multiple of 256"
                );
                let padded_end = processing_end(end)?;
                let (mut input_index, mut commands, mut host_commands, mut logs, mut dropped_logs) =
                    (0, 0, 0, 0, 0);
                let peak = write_wav(Path::new(&args[3]), rate, |write| {
                    while player.current_frame() < padded_end {
                        let frame = player.current_frame();
                        let chunk_end = padded_end.min(frame + block_frames as u64);
                        let next_input = inputs.partition_point(|input| input.frame < chunk_end);
                        let rendered = player.render(
                            &inputs[input_index..next_input],
                            (chunk_end - frame) as usize,
                        )?;
                        commands += rendered.commands;
                        host_commands += rendered.host_commands;
                        logs += rendered.logs.len();
                        dropped_logs += rendered.dropped_logs;
                        for sample in rendered
                            .audio
                            .into_iter()
                            .take(end.saturating_sub(frame) as usize)
                        {
                            write(sample)?;
                        }
                        input_index = next_input;
                    }
                    Ok(())
                })?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"frames":end,"processed_frames":padded_end,"sample_rate":rate,"block_frames":block_frames,"peak":peak,"event_commands":commands,"host_commands":host_commands,"diagnostics":diagnostics,"private_log_messages":logs,"dropped_logs":dropped_logs})
                    )?
                );
                return Ok(());
            }
            let processed = script::process_program_chain_at_rate(
                &loaded.program,
                library.modules()?,
                Some(resources.capability()),
                &inputs,
                end,
                rate,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"program_nodes":loaded.program.nodes.len(),"sample_players":loaded.program.sample_zones.len(),"event_commands":processed.commands.len(),"host_commands":processed.host_commands.len(),"unsupported":unsupported,"private_log_messages":processed.logs.len(),"dropped_logs":processed.dropped_logs})
                )?
            );
        }
        "uvi-diagnose" => {
            ensure!(
                args.len() >= 5,
                "Usage: uvi-diagnose <bank.ufs> <member.uvip> --reader <official-exe> [--content-key-file <private-file>] [--sample-rate 48000] [--ui]"
            );
            let mut args = args.to_vec();
            let include_ui = if let Some(index) = args.iter().position(|arg| arg == "--ui") {
                ensure!(index >= 3, "Misplaced --ui");
                args.remove(index);
                ensure!(!args.iter().any(|arg| arg == "--ui"), "Repeated --ui");
                true
            } else {
                false
            };
            let mut rate = 48000u32;
            if let Some(index) = args.iter().position(|arg| arg == "--sample-rate") {
                ensure!(index >= 3, "Misplaced --sample-rate");
                rate = args
                    .get(index + 1)
                    .context("Missing --sample-rate value")?
                    .parse()?;
                ensure!((8000..=192000).contains(&rate), "Invalid UVI sample rate");
                args.drain(index..=index + 1);
                ensure!(
                    !args.iter().any(|arg| arg == "--sample-rate"),
                    "Repeated --sample-rate"
                );
            }
            let (reader, state) = options(&args, 3)?;
            let config = worker::StartConfig {
                bank: PathBuf::from(&args[1]),
                expected_bank_uuid: None,
                member: args[2].clone(),
                metadata_namespace: reader.metadata,
                program_namespace: reader.program,
                content_key: state.as_ref().map(|state| state.key),
                content_bank: state.and_then(|state| state.bank),
                sample_rate: rate,
            };
            #[cfg(feature = "plugin")]
            let assets = include_ui.then(|| super::ui_assets::UiAssets::open(&config));
            let mut worker = worker::Worker::start(config, 1, 1)?;
            let ready = worker.wait_ready(std::time::Duration::from_secs(60));
            let mut report =
                worker.runtime_diagnostic_report(std::time::Duration::from_millis(500));
            if include_ui {
                let ui = if ready.is_err() {
                    serde_json::json!({"outcome":"initialization_unavailable",
                        "drawn":false,"interactions_exercised":false,"native_compared":false})
                } else {
                    #[cfg(feature = "plugin")]
                    {
                        diagnose_ui(&worker, assets.unwrap())
                    }
                    #[cfg(not(feature = "plugin"))]
                    {
                        serde_json::json!({"outcome":"unavailable_in_build","reason":"UI assets require the plugin feature",
                        "drawn":false,"interactions_exercised":false,"native_compared":false})
                    }
                };
                report["ui_inspection"] = ui;
            }
            println!("{}", serde_json::to_string_pretty(&report)?);
            worker.stop();
            // Machine-readable stdout remains available on failed initialization.
            ready.context("UVI diagnosis found an initialization failure")?;
            ensure!(
                !include_ui || report["ui_inspection"]["outcome"] == "complete",
                "UVI UI diagnosis is incomplete; see ui_inspection in stdout"
            );
        }
        "uvi-bank" => {
            let (reader, _) = options(args, 2)?;
            let bank = ufs::Ufs::open(path)?;
            let directory = bank.decode_directory(&reader.metadata)?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"header":bank.header,"directory":directory})
                )?
            );
        }
        "uvi-program" | "uvi-decode" => {
            let extracting = args[0] == "uvi-decode";
            ensure!(
                args.len() >= if extracting { 6 } else { 5 },
                "Usage: uvi-program <bank.ufs> <member.uvip> --reader <exe> [--content-key-file <private.json>]\n       uvi-decode <bank.ufs> <member> <new-output> --reader <exe> [--content-key-file <private.json>]"
            );
            let (reader, bytes) = member_bytes(args, if extracting { 4 } else { 3 })?;
            let is_program = args[2].to_ascii_lowercase().ends_with(".uvip");
            if is_program || !extracting {
                let decoded = crypto::decode_program_bytes(&bytes, &reader.program)?;
                let program = program::parse_program(&decoded)?;
                if extracting {
                    write_private(Path::new(&args[3]), decoded.as_bytes())?;
                    println!(
                        "Decoded {} program nodes into a private local file",
                        program.nodes.len()
                    );
                } else {
                    println!("{}", serde_json::to_string_pretty(&program)?);
                }
            } else {
                if args[2].to_ascii_lowercase().ends_with(".lua") {
                    let source = std::str::from_utf8(&bytes).context("Decoded Lua is not UTF-8")?;
                    let lua = mlua::Lua::new_with(mlua::StdLib::NONE, mlua::LuaOptions::default())?;
                    lua.set_memory_limit(32 << 20)?;
                    lua.load(source)
                        .into_function()
                        .context("Decoded Lua syntax is invalid")?;
                }
                write_private(Path::new(&args[3]), &bytes)?;
                println!("Decoded {} bytes into a private local file", bytes.len());
            }
        }
        "uvi-inspect" => {
            ensure!(
                args.len() == 2,
                "Usage: uvi-inspect <preset.uvip|mapping.dmap>"
            );
            let text = read_text(path)?;
            let report = if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("uvip"))
            {
                serde_json::to_value(inspect_preset(&text)?)?
            } else {
                serde_json::to_value(parse_mapping(path, &text)?)?
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        "uvi-sample" => {
            ensure!(
                args.len() == 5,
                "Usage: uvi-sample <bank.ufs> <offset> <length> <output.wav>"
            );
            let mut reader = open_member(path, args[2].parse()?, args[3].parse()?)?;
            let header = reader.header();
            ensure!(
                header.frames <= u64::from(header.rate) * 600,
                "UVI sample exceeds ten-minute offline limit"
            );
            let peak = write_wav(Path::new(&args[4]), header.rate, |write| {
                let mut block = [[0.; 2]; 4096];
                let mut at = 0;
                while at < header.frames {
                    let count = (header.frames - at).min(block.len() as u64) as usize;
                    reader.read(at, &mut block[..count])?;
                    for &frame in &block[..count] {
                        write(frame)?;
                    }
                    at += count as u64;
                }
                Ok(())
            })?;
            println!(
                "{} frames at {} Hz; peak {peak:.6}; bounded clear FLAC member",
                header.frames, header.rate
            );
        }
        "uvi-run" | "uvi-render" => {
            let rendering = args[0] == "uvi-render";
            ensure!(
                args.len() <= if rendering { 5 } else { 3 },
                "Too many UVI command arguments"
            );
            let output = rendering
                .then(|| {
                    args.get(2)
                        .context("uvi-render requires an output WAV path")
                })
                .transpose()?;
            let list = args
                .get(if rendering { 3 } else { 2 })
                .map_or("60@0-500:100", String::as_str);
            let inputs = script::parse_notes(list)?;
            let end = inputs.iter().map(|i| i.frame).max().unwrap_or(0) + 48000;
            let source = if rendering {
                args.get(4)
                    .map(|p| read_text(Path::new(p)))
                    .transpose()?
                    .unwrap_or_default()
            } else {
                read_text(path)?
            };
            let name = if rendering {
                args.get(4).map_or(path, |p| Path::new(p))
            } else {
                path
            };
            let commands = script::process(&source, &name.to_string_lossy(), &inputs, end)?;
            if let Some(output) = output {
                let mapping = read_mapping(path)?;
                let peak = write_wav(Path::new(output), 48000, |write| {
                    render(&mapping, &commands, end, write)
                })?;
                println!(
                    "{} commands; peak {peak:.6}; offline UVI mapping render",
                    commands.len()
                );
                ensure!(
                    peak > 0.00001,
                    "Rendered silence: no matching audible mapping zone"
                );
            } else {
                println!("{}", serde_json::to_string_pretty(&commands)?);
            }
        }
        _ => anyhow::bail!("Unknown UVI command"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ui_census_counts_initialized_controls_without_callbacks_or_private_values() {
        let program = program::parse_program(
            r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
            function onInit()
                visible=Knob{name='private-name',displayName='private-label',value=0.25}
                visible.changed=function()error('census ran callback')end
                hidden=Label('private-label');hidden.visible=false
            end
            function onSave()error('census ran onSave')end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer/></Layers></Program>"#,
        )
        .unwrap();
        let session =
            script::Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
        let processor = program
            .nodes
            .iter()
            .position(|node| node.kind == "ScriptProcessor")
            .unwrap();
        let snapshot = session.ui_snapshot(processor).unwrap();
        let report = ui_panel_report(
            &snapshot,
            worker::Stamp {
                epoch: 1,
                generation: 1,
                frame: 0,
            },
        );
        assert_eq!(report["widgets"], 2);
        assert_eq!(report["visible_widgets"], 1);
        assert_eq!(report["widgets_with_changed_callback"], 1);
        assert_eq!(report["stamp"]["frame"], 0);
        let serialized = report.to_string();
        assert!(!serialized.contains("private-name") && !serialized.contains("private-label"));
        assert!(!serialized.contains("0.25"));
    }
    #[test]
    fn recovered_png_requires_every_crc_and_complete_end() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut chunk = |kind: &[u8; 4], data: &[u8]| {
            png.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let start = png.len();
            png.extend_from_slice(kind);
            png.extend_from_slice(data);
            let crc = crc32fast::hash(&png[start..]);
            png.extend_from_slice(&crc.to_be_bytes());
        };
        chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]);
        // Authored one-pixel grayscale PNG: uncompressed zlib filter byte + pixel.
        chunk(b"IDAT", &[0x78, 1, 1, 2, 0, 0xfd, 0xff, 0, 0, 0, 2, 0, 1]);
        chunk(b"IEND", &[]);
        assert_eq!(super::super::access::validate_png(&png).unwrap(), 3);
        let mut corrupt = png.clone();
        corrupt[45] ^= 1;
        assert!(super::super::access::validate_png(&corrupt).is_err());
        assert!(super::super::access::validate_png(&png[..png.len() - 1]).is_err());
        png.push(0);
        assert!(super::super::access::validate_png(&png).is_err());
    }
}
