//! `render-kontakt`: a real Kontakt instrument, through the IR and the native
//! core, played by a MIDI 1.0 note sequence into a WAV file.

use crate::wave;
use sampler_core::{Limits, Outcome, Runtime};
use sampler_midi::{Applied, Ingress, Packets, TimedPacket, Version};
use std::{
    fs::OpenOptions,
    io::{self, BufWriter, Write},
    path::Path,
};

/// A held note: key, velocity, start and length in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub key: u8,
    pub velocity: u8,
    pub start: f64,
    pub length: f64,
}

/// A C major arpeggio resolving onto its chord.
pub const DEFAULT_SEQUENCE: &str = "60:100:0:0.9,64:90:0.5:0.9,67:80:1:0.9,60:110:1.5:1.5,64:100:1.5:1.5,67:100:1.5:1.5";

/// `key:velocity:start:length` notes, comma separated.
pub fn parse(sequence: &str) -> Result<Vec<Note>, String> {
    sequence
        .split(',')
        .map(|note| {
            let fields: Vec<_> = note.trim().split(':').collect();
            let [key, velocity, start, length] = fields.as_slice() else {
                return Err(format!("{note:?}: expected key:velocity:start:length"));
            };
            let number = |text: &str| text.parse::<f64>().map_err(|e| format!("{note:?}: {e}"));
            let note = Note {
                key: key.parse().map_err(|e| format!("{note:?}: {e}"))?,
                velocity: velocity.parse().map_err(|e| format!("{note:?}: {e}"))?,
                start: number(start)?,
                length: number(length)?,
            };
            let valid = note.key < 128 && (1..128).contains(&note.velocity) && note.start >= 0.0 && note.length > 0.0;
            if valid { Ok(note) } else { Err(format!("{note:?} is out of range")) }
        })
        .collect()
}

pub fn run(instrument: &Path, output: &Path, notes: &[Note], scripts: bool) -> io::Result<()> {
    let keys = notes.iter().map(|n| n.key);
    let options = sampler_kontakt::Options {
        keys: keys.clone().min().unwrap_or(0)..=keys.max().unwrap_or(127),
        scripts,
        ..Default::default()
    };
    let loaded = sampler_kontakt::load(instrument, &options, |progress| {
        if let sampler_kontakt::Progress::Translated { zones, assets } = progress {
            eprintln!("translated: {zones} zones over {assets} samples in the played key range");
        }
    })
    .map_err(|e| io::Error::other(e.to_string()))?;
    let ir = &loaded.instrument;
    eprintln!(
        "{:?}: {} groups, {} zones, {} envelopes, {} scripts, {} unsupported",
        ir.name,
        ir.groups.len(),
        ir.zones.len(),
        ir.modulators.len(),
        ir.behaviors.len(),
        ir.unsupported.len()
    );
    // One line per feature: how often, and where it first occurs.
    let mut features: Vec<(&str, usize, &sampler_ir::Unsupported)> = Vec::new();
    for item in &ir.unsupported {
        match features.iter_mut().find(|(feature, ..)| *feature == item.feature) {
            Some((_, count, _)) => *count += 1,
            None => features.push((&item.feature, 1, item)),
        }
    }
    for (feature, count, first) in features {
        let value: String = first.value.chars().take(160).collect();
        eprintln!("  unsupported x{count}: {feature} (first: {}: {value})", first.location);
    }
    let plan = loaded.plan;
    let rate = plan.sample_rate();
    let limits = Limits {
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
        behavior_cells: plan.behavior_local_count().saturating_mul(16),
        note_cells: plan.note_cell_count().saturating_mul(64),
    };
    let mut rt = Runtime::new(plan, limits).map_err(|e| io::Error::other(format!("native core: {e}")))?;
    let frame = |seconds: f64| (seconds * f64::from(rate)).round() as usize;
    let mut events: Vec<(usize, u32)> = notes
        .iter()
        .flat_map(|n| {
            let word = 0x2000_0000 | u32::from(n.key) << 8 | u32::from(n.velocity);
            [(frame(n.start), word | 0x0090_0000), (frame(n.start + n.length), word | 0x0080_0000)]
        })
        .collect();
    events.sort_by_key(|&(at, word)| (at, word & 0x0010_0000)); // Offs before ons at one instant.
    let words: Vec<[u32; 1]> = events.iter().map(|&(_, word)| [word]).collect();
    let packets = events
        .iter()
        .zip(&words)
        .map(|(&(offset, _), word)| {
            let packet = Packets::new(word).next().expect("one word").map_err(|e| io::Error::other(format!("{e:?}")))?;
            Ok(TimedPacket { offset, packet })
        })
        .collect::<io::Result<Vec<_>>>()?;
    let count = events.last().map_or(0, |e| e.0) + frame(3.0);
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi1);
    let ingress = Ingress::new(0, groups);
    let mut out = BufWriter::new(OpenOptions::new().write(true).create_new(true).open(output)?);
    wave::header(&mut out, rate, count)?;
    let (mut buffer, mut next, mut peak) = ([[0.0f32; 2]; 256], 0, 0.0f32);
    for begin in (0..count).step_by(buffer.len()) {
        let len = buffer.len().min(count - begin);
        let mut batch = Vec::new();
        while next < packets.len() && packets[next].offset < begin + len {
            batch.push(TimedPacket { offset: packets[next].offset - begin, ..packets[next] });
            next += 1;
        }
        let mut failure = None;
        ingress
            .render(&mut rt, &mut buffer[..len], &batch, batch.len(), |index, result| {
                if !matches!(result, Ok(Applied::Started(_) | Applied::Released { .. })) {
                    failure.get_or_insert((index, result));
                }
            })
            .map_err(|e| io::Error::other(format!("MIDI block: {e:?}")))?;
        if let Some((index, result)) = failure {
            eprintln!("note event at frame {}: {result:?}", begin + batch[index].offset);
        }
        rt.flush_behaviors(|_, _, outcome| {
            if !matches!(outcome, Outcome::Finished | Outcome::Cancelled) {
                eprintln!("script outcome: {outcome:?}");
            }
            true
        });
        rt.flush_ended(|_| true);
        peak = buffer[..len].iter().flatten().fold(peak, |p, x| p.max(x.abs()));
        wave::frames(&mut out, &buffer[..len])?;
    }
    out.flush()?;
    println!("rendered {count} frames at {rate} Hz, peak {peak:.3}");
    if peak == 0.0 {
        return Err(io::Error::other("the render is silent"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequences_parse_and_reject_out_of_range_notes() {
        let notes = parse(DEFAULT_SEQUENCE).unwrap();
        assert_eq!(notes.len(), 6);
        assert_eq!(notes[1], Note { key: 64, velocity: 90, start: 0.5, length: 0.9 });
        assert!(parse("128:1:0:1").is_err());
        assert!(parse("60:0:0:1").is_err());
        assert!(parse("60:1:0").is_err());
    }
}
