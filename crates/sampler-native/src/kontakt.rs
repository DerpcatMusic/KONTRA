//! Source inspection only: decoding never silently admits an instrument for audio.
use sampler_kontakt::{Chunks, Limits, Nks42, Script, nis::Item};
use std::{
    io::{self, Read, Write},
    path::Path,
};

fn source(error: sampler_kontakt::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

pub enum Format {
    Chunks,
    Nks,
    Nis,
}

pub fn inspect(path: &Path, format: Format) -> io::Result<()> {
    let limits = Limits {
        bytes: 256 * 1024 * 1024,
        records: 1_000_000,
    };
    let mut bytes = Vec::new();
    let input_limit = if matches!(format, Format::Chunks) {
        limits.bytes
    } else {
        128 * 1024 * 1024
    };
    std::fs::File::open(path)?
        .take(input_limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    let mut out = io::stdout().lock();
    match format {
        Format::Chunks => report(&bytes, limits, &mut out),
        Format::Nks => {
            let expanded = Nks42::parse(&bytes, input_limit)
                .map_err(source)?
                .expand(limits.bytes)
                .map_err(source)?;
            report(&expanded, limits, &mut out)
        }
        Format::Nis => {
            let root = Item::parse(
                &bytes,
                Limits {
                    bytes: input_limit,
                    ..limits
                },
            )
            .map_err(source)?;
            let first = root.layers().next().expect("validated NIS layers");
            let preset = match (first.domain, first.id) {
                (domain, 0x76) if domain == *b"NISD" => child(root, *b"NIK4", 3)?,
                (domain, 3) if domain == *b"NIK4" => root,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Unsupported NIS preset root",
                    ));
                }
            };
            let encryption = child(preset, *b"NISD", 0x74)?;
            let expanded = encryption
                .unencrypted_subtree(limits.bytes)
                .map_err(source)?;
            let inner = Item::parse(&expanded, limits).map_err(source)?;
            let layer = inner.layers().next().expect("validated NIS layers");
            let chunks = layer.preset_chunks().map_err(source)?;
            report(chunks.data(), limits, &mut out)
        }
    }
}

fn child<'a>(parent: Item<'a>, domain: [u8; 4], id: u32) -> io::Result<Item<'a>> {
    let mut found = None;
    for candidate in parent.children() {
        let candidate = candidate.map_err(source)?.item;
        let layer = candidate.layers().next().expect("validated NIS layers");
        if layer.domain == domain && layer.id == id {
            if found.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Ambiguous NIS preset child",
                ));
            }
            found = Some(candidate);
        }
    }
    found.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Missing NIS preset child"))
}

fn report(bytes: &[u8], limits: Limits, out: &mut impl Write) -> io::Result<()> {
    let chunks = Chunks::parse(bytes, limits).map_err(source)?;
    writeln!(
        out,
        "Expanded Kontakt source; inspection only, playback not admitted"
    )?;
    for chunk in chunks.iter() {
        writeln!(
            out,
            "chunk 0x{:04x} at {}: {} bytes",
            chunk.id,
            chunk.raw().offset(),
            chunk.body.data().len()
        )?;
        if chunk.id != 0x28 {
            continue;
        }
        let program = chunk.structured().map_err(source)?;
        writeln!(
            out,
            "  program version 0x{:04x}, private {} bytes",
            program.version,
            program.private.data().len()
        )?;
        for child in program.children(limits).map_err(source)?.iter() {
            match child.id {
                0x33 | 0x34 => {
                    let records = child.records(limits).map_err(source)?;
                    writeln!(
                        out,
                        "  {}: {} records",
                        if child.id == 0x33 { "groups" } else { "zones" },
                        records.len()
                    )?;
                }
                0x06 => {
                    let script = Script::parse(child, limits).map_err(source)?;
                    writeln!(
                        out,
                        "  script at {}: text {:?} bytes, bypass {}, linked {}, saved {:?}, extension {} bytes",
                        child.raw().offset(),
                        script.text.map(|b| b.data().len()),
                        script.bypass,
                        script.textfile_name.is_some(),
                        script.persistent.map(|p| p.len()),
                        script.extension.data().len()
                    )?;
                }
                _ => writeln!(
                    out,
                    "  unmodeled child 0x{:04x} at {}: {} bytes retained",
                    child.id,
                    child.raw().offset(),
                    child.body.data().len()
                )?,
            }
        }
    }
    Ok(())
}
