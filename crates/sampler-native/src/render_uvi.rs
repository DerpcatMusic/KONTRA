#[cfg(feature = "library-access")]
use crate::render_kontakt;
use crate::render_kontakt::Note;
use std::{io, path::Path};

#[cfg(feature = "library-access")]
pub fn run(bank: &Path, program: &str, output: &Path, notes: &[Note]) -> io::Result<()> {
    let bank = sampler_uvi::Bank::open(bank).map_err(io::Error::other)?;
    let keys = notes.iter().map(|note| note.key);
    let options = sampler_kontakt::Options {
        keys: keys.clone().min().unwrap_or(0)..=keys.max().unwrap_or(127),
        ..Default::default()
    };
    let loaded = sampler_uvi::load_program_with_options(&bank, program, &options)
        .map_err(|e| io::Error::other(e.to_string()))?;
    render_kontakt::render(loaded, output, notes)
}

#[cfg(not(feature = "library-access"))]
pub fn run(_: &Path, _: &str, _: &Path, _: &[Note]) -> io::Result<()> {
    Err(io::Error::other(
        "UVI banks need the library-access feature",
    ))
}
