//! `render-kontakt`: a real Kontakt instrument, through the IR and the native
//! core, played by a MIDI 1.0 note sequence into a WAV file.

use crate::wave;
use sampler_core::{Limits, Outcome, Runtime};
use sampler_midi::{Ingress, Packets, TimedPacket, Version};
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
pub const DEFAULT_SEQUENCE: &str =
    "60:100:0:0.9,64:90:0.5:0.9,67:80:1:0.9,60:110:1.5:1.5,64:100:1.5:1.5,67:100:1.5:1.5";

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
            let valid = note.key < 128
                && (1..128).contains(&note.velocity)
                && note.start >= 0.0
                && note.length > 0.0;
            if valid {
                Ok(note)
            } else {
                Err(format!("{note:?} is out of range"))
            }
        })
        .collect()
}

/// A MIDI 1.0 channel message at `seconds`.
pub type Message = (f64, [u8; 3]);

/// The channel-voice messages of a Standard MIDI File (format 0 or 1), in
/// time order, with tempo changes honoured. Meta and system messages are
/// skipped.
pub fn midi_file(smf: &[u8]) -> Result<Vec<Message>, String> {
    let be16 = |at: usize| smf.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let be32 = |at: usize| smf.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    if smf.get(..4) != Some(b"MThd") {
        return Err("not a Standard MIDI File".into());
    }
    let (tracks, division) = (be16(10).ok_or("short header")?, be16(12).ok_or("short header")?);
    if division & 0x8000 != 0 {
        return Err("SMPTE time division is not supported".into());
    }
    let mut at = 8 + be32(4).ok_or("short header")? as usize;
    let mut raw: Vec<(u64, usize, [u8; 3])> = Vec::new(); // (tick, order, message)
    let mut tempos: Vec<(u64, u32)> = vec![(0, 500_000)];
    for _ in 0..tracks {
        if smf.get(at..at + 4) != Some(b"MTrk") {
            return Err("missing track".into());
        }
        let end = at + 8 + be32(at + 4).ok_or("short track")? as usize;
        let mut i = at + 8;
        let (mut tick, mut running) = (0u64, 0u8);
        let byte = |i: usize| smf.get(i).copied().ok_or("truncated track");
        while i < end {
            let mut delta = 0u64;
            loop {
                let b = byte(i)?;
                i += 1;
                delta = delta << 7 | u64::from(b & 0x7f);
                if b & 0x80 == 0 {
                    break;
                }
            }
            tick += delta;
            let mut status = byte(i)?;
            if status < 0x80 {
                status = running;
            } else {
                i += 1;
            }
            match status {
                0xff => {
                    let kind = byte(i)?;
                    let len = byte(i + 1)? as usize;
                    if kind == 0x51 && len == 3 {
                        let t = smf.get(i + 2..i + 5).ok_or("truncated tempo")?;
                        tempos.push((tick, u32::from(t[0]) << 16 | u32::from(t[1]) << 8 | u32::from(t[2])));
                    }
                    i += 2 + len;
                }
                0xf0 | 0xf7 => {
                    let len = byte(i)? as usize;
                    i += 1 + len;
                }
                0x80..=0xef => {
                    running = status;
                    let data = if matches!(status & 0xf0, 0xc0 | 0xd0) { 1 } else { 2 };
                    let d1 = byte(i)?;
                    let d2 = if data == 2 { byte(i + 1)? } else { 0 };
                    i += data;
                    raw.push((tick, raw.len(), [status, d1, d2]));
                }
                _ => return Err(format!("unexpected status byte {status:#x}")),
            }
        }
        at = end;
    }
    tempos.sort_by_key(|t| t.0);
    let seconds = |tick: u64| {
        let (mut t, mut last, mut us) = (0.0f64, 0u64, 500_000u32);
        for &(at, tempo) in &tempos {
            if at >= tick {
                break;
            }
            t += (at - last) as f64 * f64::from(us) / 1e6 / f64::from(division);
            (last, us) = (at, tempo);
        }
        t + (tick - last) as f64 * f64::from(us) / 1e6 / f64::from(division)
    };
    // Offs before ons at one tick, otherwise file order.
    raw.sort_by_key(|&(tick, order, m)| (tick, m[0] & 0xf0 == 0x90 && m[2] > 0, order));
    Ok(raw.into_iter().map(|(tick, _, m)| (seconds(tick), m)).collect())
}

/// `notes` as note-on and note-off messages.
pub fn note_messages(notes: &[Note]) -> Vec<Message> {
    let mut m: Vec<Message> = notes
        .iter()
        .flat_map(|n| [(n.start, [0x90, n.key, n.velocity]), (n.start + n.length, [0x80, n.key, n.velocity])])
        .collect();
    m.sort_by(|a, b| a.0.total_cmp(&b.0).then((a.1[0] & 0xf0 == 0x90).cmp(&(b.1[0] & 0xf0 == 0x90))));
    m
}

pub fn run(instrument: &Path, output: &Path, messages: &[Message], scripts: bool) -> io::Result<()> {
    let keys: Vec<u8> = messages.iter().filter(|m| matches!(m.1[0] & 0xf0, 0x80 | 0x90)).map(|m| m.1[1]).collect();
    let options = sampler_kontakt::Options {
        keys: keys.iter().copied().min().unwrap_or(0)..=keys.iter().copied().max().unwrap_or(127),
        scripts,
        ..Default::default()
    };
    let loaded = sampler_kontakt::load(instrument, &options, |progress| {
        if let sampler_kontakt::Progress::Translated { zones, assets } = progress {
            eprintln!("translated: {zones} zones over {assets} samples in the played key range");
        }
    })
    .map_err(|e| io::Error::other(e.to_string()))?;
    render(loaded, output, messages)
}

pub fn render(loaded: sampler_kontakt::Loaded, output: &Path, messages: &[Message]) -> io::Result<()> {
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
        match features
            .iter_mut()
            .find(|(feature, ..)| *feature == item.feature)
        {
            Some((_, count, _)) => *count += 1,
            None => features.push((&item.feature, 1, item)),
        }
    }
    for (feature, count, first) in features {
        let value: String = first.value.chars().take(160).collect();
        eprintln!(
            "  unsupported x{count}: {feature} (first: {}: {value})",
            first.location
        );
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
    let mut rt =
        Runtime::new(plan, limits).map_err(|e| io::Error::other(format!("native core: {e}")))?;
    rt.set_voice_stealing(Some(sampler_core::Stealing::for_limits(
        rt.sample_rate(),
        limits.voices,
    )))
    .map_err(|e| io::Error::other(format!("native core: {e}")))?;
    let frame = |seconds: f64| (seconds * f64::from(rate)).round() as usize;
    // MIDI 1.0 channel voice messages as UMP words, in the order given (a
    // stable sort keeps simultaneous messages in file order).
    let mut events: Vec<(usize, u32)> = messages
        .iter()
        .map(|&(at, [status, d1, d2])| {
            (frame(at), 0x2000_0000 | u32::from(status) << 16 | u32::from(d1) << 8 | u32::from(d2))
        })
        .collect();
    events.sort_by_key(|&(at, _)| at);
    let words: Vec<[u32; 1]> = events.iter().map(|&(_, word)| [word]).collect();
    let packets = events
        .iter()
        .zip(&words)
        .map(|(&(offset, _), word)| {
            let packet = Packets::new(word)
                .next()
                .expect("one word")
                .map_err(|e| io::Error::other(format!("{e:?}")))?;
            Ok(TimedPacket { offset, packet })
        })
        .collect::<io::Result<Vec<_>>>()?;
    let count = events.last().map_or(0, |e| e.0) + frame(3.0);
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi1);
    let mut ingress = Ingress::new(0, groups);
    let mut out = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?,
    );
    wave::header(&mut out, rate, count)?;
    let (mut buffer, mut next, mut peak) = ([[0.0f32; 2]; 256], 0, 0.0f32);
    for begin in (0..count).step_by(buffer.len()) {
        let len = buffer.len().min(count - begin);
        let mut batch = Vec::new();
        while next < packets.len() && packets[next].offset < begin + len {
            batch.push(TimedPacket {
                offset: packets[next].offset - begin,
                ..packets[next]
            });
            next += 1;
        }
        let mut failure = None;
        ingress
            .render(
                &mut rt,
                &mut buffer[..len],
                &batch,
                batch.len(),
                |index, result| {
                    if result.is_err() {
                        failure.get_or_insert((index, result));
                    }
                },
            )
            .map_err(|e| io::Error::other(format!("MIDI block: {e:?}")))?;
        if let Some((index, result)) = failure {
            eprintln!(
                "MIDI event at frame {}: {result:?}",
                begin + batch[index].offset
            );
        }
        rt.flush_behaviors(|_, _, outcome| {
            if !matches!(outcome, Outcome::Finished | Outcome::Cancelled) {
                eprintln!("script outcome: {outcome:?}");
            }
            true
        });
        rt.flush_ended(|_| true);
        peak = buffer[..len]
            .iter()
            .flatten()
            .fold(peak, |p, x| p.max(x.abs()));
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
        assert_eq!(
            notes[1],
            Note {
                key: 64,
                velocity: 90,
                start: 0.5,
                length: 0.9
            }
        );
        assert!(parse("128:1:0:1").is_err());
        assert!(parse("60:0:0:1").is_err());
        assert!(parse("60:1:0").is_err());
    }
}
