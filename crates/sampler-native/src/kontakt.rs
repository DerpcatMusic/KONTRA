//! Source inspection only: decoding never silently admits an instrument for audio.
use sampler_kontakt::{Chunks, Limits, Nks42, Script};
use std::{
    io::{self, Read, Write},
    path::Path,
};

fn source(error: sampler_kontakt::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

pub fn inspect(path: &Path, nks: bool) -> io::Result<()> {
    let limits = Limits {
        bytes: 256 * 1024 * 1024,
        records: 1_000_000,
    };
    let mut bytes = Vec::new();
    let input_limit = if nks { 128 * 1024 * 1024 } else { limits.bytes };
    std::fs::File::open(path)?
        .take(input_limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if nks {
        bytes = Nks42::parse(&bytes, input_limit)
            .map_err(source)?
            .expand(limits.bytes)
            .map_err(source)?;
    }
    report(&bytes, limits, &mut io::stdout().lock())
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
