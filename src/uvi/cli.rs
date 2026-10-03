//! Offline commands; reader constants and content state stay local.
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Seek, SeekFrom, Write};
use std::{collections::HashMap, rc::Rc};

struct ReaderNamespaces {
    metadata: Vec<u8>,
    program: Vec<u8>,
}

impl ReaderNamespaces {
    fn open(path: &Path) -> Result<Self> {
        let mut bytes = Vec::new();
        File::open(path)?
            .take((64 << 20) + 1)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 64 << 20, "UVI reader exceeds size limit");
        let digest = format!("{:x}", Sha256::digest(&bytes));
        ensure!(
            digest == "78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721",
            "Reader namespace layout is verified only for official UVI Workstation 4.0.9 x64"
        );
        Ok(Self {
            metadata: bytes[0x1ea4e58..0x1ea4e58 + 36].to_vec(),
            program: bytes[31_586_936..31_586_936 + 39].to_vec(),
        })
    }
}

#[derive(Deserialize)]
struct ContentState {
    key: u64,
    #[serde(default)]
    bank: Option<String>,
}

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
                let file = File::open(value)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    ensure!(
                        file.metadata()?.permissions().mode() & 0o077 == 0,
                        "Content-state file must be private (chmod 600)"
                    );
                }
                let mut bytes = Vec::new();
                file.take(4097).read_to_end(&mut bytes)?;
                ensure!(bytes.len() <= 4096, "Content-state file exceeds limit");
                state = Some(if bytes.len() == 8 {
                    ContentState {
                        key: u64::from_le_bytes(bytes.try_into().unwrap()),
                        bank: None,
                    }
                } else {
                    serde_json::from_slice(&bytes).context("Invalid local content-state file")?
                });
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

/// Validate the complete PNG, rather than trusting a guessed cipher prefix.
fn validate_png(bytes: &[u8]) -> Result<usize> {
    ensure!(
        bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "Invalid PNG signature"
    );
    let mut offset = 8usize;
    let (mut chunks, mut data) = (0usize, false);
    while offset < bytes.len() {
        let header = bytes
            .get(offset..offset + 8)
            .context("Truncated PNG chunk")?;
        let size = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let end = offset
            .checked_add(12)
            .and_then(|n| n.checked_add(size))
            .context("PNG chunk overflow")?;
        let chunk = bytes
            .get(offset + 4..end)
            .context("Truncated PNG chunk data")?;
        let crc = u32::from_be_bytes(chunk[chunk.len() - 4..].try_into().unwrap());
        ensure!(
            crc32fast::hash(&chunk[..chunk.len() - 4]) == crc,
            "PNG CRC mismatch"
        );
        let kind = &header[4..8];
        if chunks == 0 {
            ensure!(kind == b"IHDR" && size == 13, "PNG must start with IHDR");
            let ihdr = &chunk[4..17];
            ensure!(
                u32::from_be_bytes(ihdr[..4].try_into().unwrap()) != 0
                    && u32::from_be_bytes(ihdr[4..8].try_into().unwrap()) != 0,
                "Invalid PNG dimensions"
            );
            ensure!(
                ihdr[10] == 0 && ihdr[11] == 0 && ihdr[12] <= 1,
                "Invalid PNG encoding"
            );
        } else {
            ensure!(kind != b"IHDR", "Repeated PNG IHDR");
        }
        data |= kind == b"IDAT";
        chunks += 1;
        offset = end;
        if kind == b"IEND" {
            ensure!(
                size == 0 && data && offset == bytes.len(),
                "Invalid PNG end"
            );
            return Ok(chunks);
        }
    }
    anyhow::bail!("PNG has no IEND")
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
            let mut candidates: Vec<_> = directory
                .files
                .iter()
                .filter(|m| {
                    m.mode == 2
                        && m.size >= 45
                        && m.size <= 16 << 20
                        && m.name.to_ascii_lowercase().ends_with(".png")
                })
                .collect();
            candidates.sort_by_key(|m| m.size);
            ensure!(
                !candidates.is_empty(),
                "Bank has no bounded encrypted PNG suitable for known-plaintext recovery"
            );
            let mut verified = None;
            for member in candidates {
                let mut file = File::open(path)?;
                file.seek(SeekFrom::Start(member.offset))?;
                let mut bytes = vec![0; member.size as usize];
                file.read_exact(&mut bytes)?;
                let cipher: [u8; 16] = bytes[..16].try_into().unwrap();
                let plain = *b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
                if let Ok(key) = crypto::recover_key(&cipher, &plain, member.offset) {
                    crypto::transform_blocks(&mut bytes, key, member.offset);
                    if let Ok(chunks) = validate_png(&bytes) {
                        verified = Some((key, chunks));
                        break;
                    }
                }
            }
            let (key, chunks) =
                verified.context("No recovered content state passed full PNG CRC verification")?;
            let state = serde_json::json!({"key":key,"bank":std::fs::canonicalize(path)?.to_string_lossy()});
            write_private(Path::new(&args[2]), &serde_json::to_vec(&state)?)?;
            println!(
                "Saved a private bank-bound content state after verifying {chunks} PNG chunk CRCs"
            );
        }
        "uvi-check" | "uvi-play" => {
            let rendering = args[0] == "uvi-play";
            ensure!(
                args.len() >= if rendering { 6 } else { 5 },
                "Usage: uvi-check <bank.ufs> <member.uvip> --reader <exe> [--content-key-file <private-file>]\n       uvi-play <bank.ufs> <member.uvip> <output.wav> --reader <exe> [--content-key-file <private-file>] [--notes 60@0-500:100 | --events timeline.json] [--sample-rate 48000]"
            );
            let start = if rendering { 4 } else { 3 };
            let mut args = args.to_vec();
            let mut notes = "60@0-500:100".to_owned();
            let mut explicit_notes = false;
            let mut event_file = None;
            let mut rate = 48000u32;
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
            let (reader, library) = open_library(&args, start)?;
            let library = Rc::new(library);
            let loaded = library.program(&args[2], &reader.program)?;
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
            let resources = library::BankResources::new(
                library.clone(),
                &loaded.path,
                if rendering {
                    library.samples(&loaded)?
                } else {
                    HashMap::new()
                },
            )?;
            let mut processed = script::process_program_chain_at_rate(
                &loaded.program,
                library.modules()?,
                Some(resources.capability()),
                &inputs,
                end,
                rate,
            )?;
            let unsupported = playback::preflight(&loaded.program);
            if rendering {
                // A file's end is exclusive; commands at that boundary have no audio frame.
                processed.commands.retain(|command| command.frame < end);
                processed
                    .host_commands
                    .retain(|command| command.frame < end);
                let samples = resources.samples();
                let mut renderer = playback::Renderer::new(&loaded.program, samples, rate)?;
                let diagnostics = renderer.diagnostics();
                let frames = if renderer.requires_planned_segments() {
                    // Native source arrays are planned over complete 256-frame blocks.
                    // Render their final block, retaining only the requested file extent.
                    let padded_end = end
                        .checked_add(255)
                        .context("UVI padded timeline overflow")?
                        / 256
                        * 256;
                    let chunk_limit = u64::from(rate) * 60 / 256 * 256;
                    let mut output = Vec::with_capacity(end as usize);
                    let (mut frame, mut note_index, mut host_index) = (0, 0, 0);
                    while frame < padded_end {
                        let chunk_end = padded_end.min(frame + chunk_limit);
                        let next_note = processed
                            .commands
                            .partition_point(|command| command.frame < chunk_end);
                        let next_host = processed
                            .host_commands
                            .partition_point(|command| command.frame < chunk_end);
                        let chunk = renderer.render(
                            &processed.commands[note_index..next_note],
                            &processed.host_commands[host_index..next_host],
                            (chunk_end - frame) as usize,
                        )?;
                        output.extend(chunk.into_iter().take(end.saturating_sub(frame) as usize));
                        (frame, note_index, host_index) = (chunk_end, next_note, next_host);
                    }
                    output
                } else {
                    renderer.render(&processed.commands, &processed.host_commands, end as usize)?
                };
                let peak = write_wav(Path::new(&args[3]), rate, |write| {
                    for frame in frames {
                        write(frame)?;
                    }
                    Ok(())
                })?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"frames":end,"sample_rate":rate,"peak":peak,"event_commands":processed.commands.len(),"host_commands":processed.host_commands.len(),"diagnostics":diagnostics,"private_log_messages":processed.logs.len(),"dropped_logs":processed.dropped_logs})
                    )?
                );
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"program_nodes":loaded.program.nodes.len(),"sample_players":loaded.program.sample_zones.len(),"event_commands":processed.commands.len(),"host_commands":processed.host_commands.len(),"unsupported":unsupported,"private_log_messages":processed.logs.len(),"dropped_logs":processed.dropped_logs})
                    )?
                );
            }
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
                let text =
                    std::str::from_utf8(&bytes).context("Decoded UVI program is not UTF-8")?;
                let decoded = crypto::decode_program(text, &reader.program)?;
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
        assert_eq!(validate_png(&png).unwrap(), 3);
        let mut corrupt = png.clone();
        corrupt[45] ^= 1;
        assert!(validate_png(&corrupt).is_err());
        assert!(validate_png(&png[..png.len() - 1]).is_err());
        png.push(0);
        assert!(validate_png(&png).is_err());
    }
}
