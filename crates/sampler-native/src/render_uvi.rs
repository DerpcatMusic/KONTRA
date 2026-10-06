use crate::render_kontakt::{self, Note};
use std::{io, path::Path};

#[cfg(feature = "library-access")]
pub fn run(bank: &Path, program: &str, output: &Path, notes: &[Note]) -> io::Result<()> {
    let bank = sampler_uvi::Bank::open(bank).map_err(io::Error::other)?;
    let loaded = sampler_uvi::load_program(&bank, program, 48000)
        .map_err(|e| io::Error::other(e.to_string()))?;
    render_kontakt::render(loaded, output, notes)
}

#[cfg(not(feature = "library-access"))]
pub fn run(_: &Path, _: &str, _: &Path, _: &[Note]) -> io::Result<()> {
    Err(io::Error::other(
        "UVI banks need the library-access feature",
    ))
}
