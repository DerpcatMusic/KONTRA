use crate::render_kontakt::Note;
use std::{io, path::Path};

#[cfg(feature = "library-access")]
pub fn run(bank: &Path, program: &str, output: &Path, notes: &[Note]) -> io::Result<()> {
    let bank = sampler_uvi::Bank::open(bank).map_err(io::Error::other)?;
    // Scripts may play any key: every zone is present, its samples stream.
    let rate = sampler_kontakt::Options::default().rate;
    let program = sampler_uvi::load_program_scripted_streamed(&bank, program, rate, &Default::default())
        .map_err(|e| io::Error::other(e.to_string()))?;
    let ir = &program.instrument;
    eprintln!(
        "{:?}: {} groups, {} zones, {} scripts, {} unsupported",
        ir.name,
        ir.groups.len(),
        ir.zones.len(),
        ir.behaviors.len(),
        ir.unsupported.len()
    );
    for item in ir.unsupported.iter().take(12) {
        let value: String = item.value.chars().take(120).collect();
        eprintln!(
            "  unsupported: {} ({}: {value})",
            item.feature, item.location
        );
    }
        let limits = sampler_core::Limits {
        notes: 64,
        channels: 16,
        performances: 1,
        expressions: 64,
        families: 64,
        decisions: 256,
        voices: 512,
        commands: 256,
        behaviors: 16,
        behavior_fuel: 1 << 20,
        behavior_cells: 0,
        note_cells: 0,
    };
    let mut player = sampler_uvi::scripted::Player::new(program, limits, rate)
        .map_err(|e| io::Error::other(format!("native core: {e}")))?;
    let frame = |seconds: f64| (seconds * f64::from(rate)).round() as usize;
    // (frame, is_on, note); offs sort before ons at one instant.
    let mut events: Vec<(usize, bool, &Note)> = notes
        .iter()
        .flat_map(|n| {
            [
                (frame(n.start), true, n),
                (frame(n.start + n.length), false, n),
            ]
        })
        .collect();
    events.sort_by_key(|e| (e.0, e.1));
    let count = events.last().map_or(0, |e| e.0) + frame(3.0);
    let mut out = io::BufWriter::new(
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?,
    );
    crate::wave::header(&mut out, rate, count)?;
    let (mut buffer, mut next, mut peak) = ([[0.0f32; 2]; 256], 0, 0.0f32);
    let fail = |e: sampler_core::Error| io::Error::other(format!("native core: {e}"));
    let mut at = 0;
    while at < count {
        while next < events.len() && events[next].0 <= at {
            let (_, on, n) = events[next];
            if on {
                player
                    .note_on(n.key, f64::from(n.velocity) / 127.0)
                    .map_err(fail)?;
            } else {
                player.note_off(n.key).map_err(fail)?;
            }
            next += 1;
        }
        let stop = events.get(next).map_or(count, |e| e.0.min(count));
        let len = buffer.len().min(stop - at);
        player.render(&mut buffer[..len]).map_err(fail)?;
        peak = buffer[..len]
            .iter()
            .flatten()
            .fold(peak, |p, x| p.max(x.abs()));
        crate::wave::frames(&mut out, &buffer[..len])?;
        at += len;
    }
    io::Write::flush(&mut out)?;
    for item in player.unmodeled() {
        eprintln!("  unmodeled script request: {item}");
    }
    drop(player);
    if !sampler_core::trace_report::flush(std::time::Duration::from_secs(10)) { return Err(io::Error::other("signal trace report flush timed out")); }
    println!("rendered {count} frames at {rate} Hz, peak {peak:.3}");
    if peak == 0.0 {
        return Err(io::Error::other("the render is silent"));
    }
    Ok(())
}

#[cfg(not(feature = "library-access"))]
pub fn run(_: &Path, _: &str, _: &Path, _: &[Note]) -> io::Result<()> {
    Err(io::Error::other(
        "UVI banks need the library-access feature",
    ))
}
