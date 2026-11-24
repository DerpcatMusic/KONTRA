//! A multi (`.nkm`): its bank's slots, each an instrument of its own.
//!
//! The bank (chunk 0x03) holds a slot list of up to 64 program containers;
//! each holds a program list whose programs are the slot's instrument, as an
//! `.nki` stores it. Each slot's program is cut out as its own chunk and
//! translated as an instrument, with the multi's file table beside it.
//!
//! Where a slot listens and plays (measured with the native oracle: a multi
//! Kontakt 8 built with known channels, outputs, volumes and pans, saved and
//! read back; `tools/kontakt-oracle/probes/multi_slots.lua`):
//! - its MIDI channel is the first u16 of its entry in the bank's slot table
//!   (chunk 0x48, 64 entries of 24 or 28 bytes by version): 0 is Omni, 1 to
//!   64 channels 1 to 16 of ports A to D;
//! - its output is the low u16 of the last u32 of its container's private
//!   record (Kontakt's reader resolves it through the output table);
//! - its volume and pan are its program's own (2^(dB/6) and percent/100),
//!   which the instrument translation already reads.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use kontra_ir as ir;
use ni_file::kontakt::objects::{Bank, FNTableImpl, Program};
use ni_file::kontakt::{Chunk, KontaktChunks, StructuredObject};

use crate::{Error, container, translate};

const PROGRAM: u16 = 0x28;
const PROGRAM_LIST: u16 = 0x36;
const FILE_TABLE: u16 = 0x4b;
const OLD_FILE_TABLE: u16 = 0x3d;
const SLOT_TABLE: u16 = 0x48;
const SCRIPT: u16 = 0x06;

/// One slot of a multi: where it sits and the instrument it holds.
#[derive(Debug)]
pub struct Slot {
    /// Its place in the multi, 0 to 63.
    pub index: u16,
    pub instrument: ir::Instrument,
    /// The MIDI channel it listens on: 0 for every channel, else 1 to 64
    /// (channels 1 to 16 of ports A to D).
    pub channel: u16,
    /// The stereo output it plays into, from 0.
    pub output: u16,
    /// Whether it sounds itself: a zone plays something other than digital
    /// silence. One that does not is a MIDI player ([`fold_players`]).
    pub sounds: bool,
}

/// A multi's decoded slots, multi scripts, and per-slot load failures, read
/// from the `.nkm` in a single open/decrypt pass.
#[derive(Debug)]
pub struct Multi {
    pub slots: Vec<Slot>,
    pub scripts: Vec<ir::Script>,
    pub failed: Vec<String>,
}

/// Reads both slots and multi scripts of the multi at `path` in one open/decrypt
/// pass; with `only`, keeps only that slot. `installs`: paths inside Kontakt
/// installations (their executables), where a slot's own resource container
/// may be ([`slots`]).
///
/// # Errors
/// When the file is not a multi this can read.
pub fn load(path: &Path, only: Option<u16>, installs: &[PathBuf]) -> Result<Multi, Error> {
    let (chunks, monolith, encrypted) = container::opened(path)?;
    let decode = |what| move |e: ni_file::Error| Error::new(path, what, e);
    let bank = chunks
        .find_first(ni_file::kontakt::objects::KONTAKT_BANK_ID)
        .ok_or_else(|| Error::new(path, "multi", "no bank"))?;
    let bank = Bank::try_from(bank).map_err(decode("bank"))?;
    let scripts = scripts_from_bank(path, &bank)?;
    let (slots, failed) =
        slots_from_bank(path, &chunks, &bank, monolith, encrypted, only, installs)?;
    Ok(Multi {
        slots,
        scripts,
        failed,
    })
}

/// The slots of the multi at `path`, in slot order; with `only`, that slot
/// alone.
///
/// A slot whose resource container the multi's file table names under a
/// Kontakt installation (a factory tool such as Chords, which Conflux's
/// multis put in front of the synth) is translated against that container,
/// not the multi's library: its scripts' data comes from there, and each
/// keeps that container's folder ([`ir::Script::container`]).
/// `installs`: paths inside installations (their executables).
///
/// # Errors
/// When the file is not a multi this can read; a slot whose instrument
/// cannot be translated is left out of the list with its reason.
pub fn slots(
    path: &Path,
    only: Option<u16>,
    installs: &[PathBuf],
) -> Result<(Vec<Slot>, Vec<String>), Error> {
    let (chunks, monolith, encrypted) = container::opened(path)?;
    let decode = |what| move |e: ni_file::Error| Error::new(path, what, e);
    let bank = chunks
        .find_first(ni_file::kontakt::objects::KONTAKT_BANK_ID)
        .ok_or_else(|| Error::new(path, "multi", "no bank"))?;
    let bank = Bank::try_from(bank).map_err(decode("bank"))?;
    slots_from_bank(path, &chunks, &bank, monolith, encrypted, only, installs)
}

fn slots_from_bank(
    path: &Path,
    chunks: &KontaktChunks,
    bank: &Bank,
    monolith: Option<ni_file::file_container::NIFileContainer>,
    encrypted: Option<bool>,
    only: Option<u16>,
    installs: &[PathBuf],
) -> Result<(Vec<Slot>, Vec<String>), Error> {
    let decode = |what| move |e: ni_file::Error| Error::new(path, what, e);
    let list = bank.slot_list().map_err(decode("slot list"))?;
    let table = (chunks.find_first(FILE_TABLE)).or_else(|| chunks.find_first(OLD_FILE_TABLE));
    let special = (chunks.find_first(FILE_TABLE))
        .and_then(|t| FNTableImpl::try_from(t).ok())
        .map(|t| t.special_filetable)
        .unwrap_or_default();
    let slot_table = bank.0.find_first(SLOT_TABLE);
    let stride = slot_table.map_or(0, |t| t.data.len() / 64);
    let channel = |index: u16| -> u16 {
        let at = usize::from(index) * stride;
        let bytes = slot_table.and_then(|t| t.data.get(at..at + 2));
        bytes
            .and_then(|b| b.try_into().ok())
            .map_or(0, u16::from_le_bytes)
    };
    // ponytail: `only` still translates every slot, since a MIDI player
    // slot folds into the one asked for; skip sounding others if rereads
    // of big multis get slow.
    let mut indices: Vec<u16> = list.slots.keys().copied().collect();
    indices.sort_unstable();
    let (mut slots, mut failed) = (Vec::new(), Vec::new());
    for index in indices {
        let Some(container) = list.slots.get(&index) else {
            continue;
        };
        let program = container
            .0
            .find_first(PROGRAM_LIST)
            .and_then(|l| first_program(&l.data));
        let Some(program) = program else {
            failed.push(format!("slot {}: no program", index + 1));
            continue;
        };
        let tool = tool_container(&program, &special, installs);
        let at = tool.as_ref().map_or(path, PathBuf::as_path);
        let table = table.map(|t| Chunk {
            id: t.id,
            data: t.data.clone(),
        });
        let stream = KontaktChunks([Some(program), table].into_iter().flatten().collect());
        match translate::instrument(at, &stream, monolith.clone(), &[], encrypted, None) {
            Ok(mut instrument) => {
                let folder = tool.as_deref().and_then(Path::parent);
                for s in &mut instrument.scripts {
                    s.container = folder.map(Path::to_path_buf);
                }
                slots.push(Slot {
                    index,
                    sounds: sounds(&instrument),
                    instrument,
                    channel: channel(index),
                    output: output(&container.0.private_data),
                });
            }
            Err(e) => failed.push(format!("slot {}: {e}", index + 1)),
        }
    }
    fold_players(&mut slots);
    slots.retain(|s| only.is_none_or(|o| o == s.index));
    Ok((slots, failed))
}

/// Slot `program`'s resource container when the multi's file table
/// (`special`) names it relative to a Kontakt installation found above one
/// of `installs` (`Content/Tools/Chords/Samples/Chords 1.0.nkr`).
///
/// A container named relative to the multi's library (`Samples/x.nkr`) is
/// no file under an installation and gives `None`.
fn tool_container(
    program: &Chunk,
    special: &HashMap<u32, String>,
    installs: &[PathBuf],
) -> Option<PathBuf> {
    let record = Program::try_from(program).ok()?.public_record().ok()?;
    let at = u32::try_from(record.resource_container_filename_ref?).ok()?;
    let relative = Path::new(special.get(&at)?);
    if !(relative.components()).all(|c| matches!(c, std::path::Component::Normal(_))) {
        return None;
    }
    (installs.iter())
        .flat_map(|p| p.ancestors().skip(1))
        .map(|root| root.join(relative))
        .find(|at| at.is_file())
}

/// Whether a zone of `instrument` plays something other than digital
/// silence. Kontakt's Chords tool has one zone, on `ghost sample.wav`: half
/// a second of zeros.
fn sounds(instrument: &ir::Instrument) -> bool {
    (instrument.groups.iter()).flat_map(|g| &g.zones).any(|z| {
        instrument
            .samples
            .get(z.sample)
            .is_none_or(|s| !silent_wav(s))
    })
}

/// Whether `sample` is a plain WAV file of at most 1 MiB whose sample data
/// is all zero bytes in an encoding where zero is silence (PCM of 16 bits
/// or more, or float; 8-bit PCM is unsigned, its silence 0x80). Anything
/// else (encrypted, inside an archive, compressed, larger) counts as
/// sound, unread.
fn silent_wav(sample: &ir::Sample) -> bool {
    const LIMIT: u64 = 1 << 20;
    let plain = sample.keystream.is_none() && sample.raw.is_none() && sample.offset == 0;
    let wav = (sample.file.extension()).is_some_and(|e| e.eq_ignore_ascii_case("wav"));
    let small = std::fs::metadata(&sample.file).is_ok_and(|m| m.is_file() && m.len() <= LIMIT);
    if !(plain && sample.len.is_none() && wav && small) {
        return false;
    }
    let Ok(bytes) = std::fs::read(&sample.file) else {
        return false;
    };
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return false;
    }
    let mut at = 12;
    let mut zero_is_silence = false;
    while let (Some(id), Some(size)) = (bytes.get(at..at + 4), bytes.get(at + 4..at + 8)) {
        let size = size.try_into().map_or(0, u32::from_le_bytes) as usize;
        let body = at + 8;
        if id == b"fmt " {
            let u16_at = |n: usize| {
                (bytes.get(body + n..body + n + 2))
                    .and_then(|b| b.try_into().ok())
                    .map(u16::from_le_bytes)
            };
            // Format tag 1 is PCM, 3 IEEE float; bits per sample at 14.
            zero_is_silence = match (u16_at(0), u16_at(14)) {
                (Some(1), Some(bits)) => bits > 8,
                (Some(3), _) => true,
                _ => false,
            };
        }
        if id == b"data" {
            let data = bytes.get(body..body.saturating_add(size).min(bytes.len()));
            return zero_is_silence && data.is_some_and(|d| d.iter().all(|b| *b == 0));
        }
        // Chunks are padded to an even size.
        at = body.saturating_add(size).saturating_add(size & 1);
    }
    false
}

/// Whether a slot on channel `a` hears notes made by one on `b` (0 is
/// every channel).
fn hears(a: u16, b: u16) -> bool {
    a == 0 || b == 0 || a == b
}

/// Notes a slot's scripts make (`play_note`) reach the slots that listen
/// on its channel. A slot that sounds nothing (a MIDI player, as Conflux's
/// factory multis put in front of the synth) is that: its scripts run in
/// front of each sounding slot on its channel, in slot order, where the
/// notes they make reach the slots after them as the slot's own; it then
/// is no slot of its own. A chain only feeds the slots after it, so no
/// slot ever hears its own notes.
///
/// A player no sounding slot hears stays, as does every slot of a multi
/// that sounds nothing at all.
fn fold_players(slots: &mut Vec<Slot>) {
    if slots.iter().all(|s| !s.sounds) {
        return;
    }
    let heard = |s: &Slot, slots: &[Slot]| {
        !s.sounds && (slots.iter()).any(|t| t.sounds && hears(t.channel, s.channel))
    };
    let players: Vec<(u16, Vec<ir::Script>)> = (slots.iter())
        .filter(|s| heard(s, slots))
        .map(|s| (s.channel, s.instrument.scripts.clone()))
        .collect();
    let folded: Vec<u16> = (slots.iter())
        .filter(|s| heard(s, slots))
        .map(|s| s.index)
        .collect();
    slots.retain(|s| !folded.contains(&s.index));
    for slot in slots.iter_mut().filter(|s| s.sounds) {
        let mut chain: Vec<ir::Script> = (players.iter())
            .filter(|(c, _)| hears(slot.channel, *c))
            .flat_map(|(_, scripts)| scripts.iter().cloned())
            .collect();
        // Presets save values by script place: each keeps its own.
        for (n, s) in chain.iter_mut().enumerate() {
            s.slot = n;
        }
        let ahead = chain.len();
        for s in &mut slot.instrument.scripts {
            s.slot += ahead;
        }
        chain.append(&mut slot.instrument.scripts);
        slot.instrument.scripts = chain;
    }
}

/// The output a slot's container names: the low half of its private
/// record's last u32 (its high half is the output's channel count).
fn output(private: &[u8]) -> u16 {
    let n = private.len();
    private
        .get(n.saturating_sub(4)..n.saturating_sub(2))
        .and_then(|b| b.try_into().ok())
        .map_or(0, u16::from_le_bytes)
}

/// The multi script slots of the multi at `path` that hold a script, in
/// slot order: KSP that sees the rack's MIDI before its instruments do
/// (`on midi_in`).
///
/// # Errors
/// When the file is not a multi this can read.
pub fn scripts(path: &Path) -> Result<Vec<ir::Script>, Error> {
    let (chunks, _, _) = container::opened(path)?;
    let bank = chunks
        .find_first(ni_file::kontakt::objects::KONTAKT_BANK_ID)
        .ok_or_else(|| Error::new(path, "multi", "no bank"))?;
    let bank = Bank::try_from(bank).map_err(|e| Error::new(path, "bank", e))?;
    scripts_from_bank(path, &bank)
}

fn scripts_from_bank(path: &Path, bank: &Bank) -> Result<Vec<ir::Script>, Error> {
    let mut out = Vec::new();
    for (slot, chunk) in (bank.0.children.iter().filter(|c| c.id == SCRIPT)).enumerate() {
        let script = ni_file::kontakt::objects::BParScript::try_from(chunk)
            .and_then(|s| s.params())
            .map_err(|e| Error::new(path, "multi script", e))?;
        match script.text {
            Some(text) if !script.bypass && !text.trim().is_empty() => out.push(ir::Script {
                name: script.description.unwrap_or_default(),
                slot,
                dialect: ir::Dialect::Ksp,
                source: text,
                saved: script.persistent,
                data: Vec::new(),
                links: Vec::new(),
                container: None,
            }),
            _ => {}
        }
    }
    Ok(out)
}

/// Whether `path` names a multi.
#[must_use]
pub fn is_multi(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("nkm"))
}

/// A program list's first program, as the chunk an `.nki` stores it in: a
/// count, then per program a 16-bit tag and its structured object.
fn first_program(list: &[u8]) -> Option<Chunk> {
    let start = 4;
    let mut reader = Cursor::new(list.get(start..)?);
    let count = i16::from_le_bytes(list.get(..2)?.try_into().ok()?);
    (count > 0).then_some(())?;
    StructuredObject::read(&mut reader).ok()?;
    let end = start + usize::try_from(reader.position()).ok()?;
    Some(Chunk {
        id: PROGRAM,
        data: list.get(start..end)?.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(name: &str, slot: usize) -> ir::Script {
        ir::Script {
            name: name.into(),
            slot,
            dialect: ir::Dialect::Ksp,
            source: String::new(),
            saved: Vec::new(),
            data: Vec::new(),
            links: Vec::new(),
            container: None,
        }
    }

    /// Slot `index` on `channel` with `scripts`; it sounds when `zones`.
    fn slot(index: u16, channel: u16, zones: bool, scripts: &[(&str, usize)]) -> Slot {
        let mut group = ir::Group::DEFAULT;
        if zones {
            group
                .zones
                .push(kontra_format::builder::ZoneBuilder::new(0, 1).build());
        }
        Slot {
            index,
            instrument: ir::Instrument {
                groups: vec![group],
                scripts: scripts.iter().map(|&(n, s)| script(n, s)).collect(),
                ..ir::Instrument::DEFAULT
            },
            channel,
            output: 0,
            sounds: zones,
        }
    }

    fn chain(s: &Slot) -> Vec<(&str, usize)> {
        (s.instrument.scripts.iter())
            .map(|s| (s.name.as_str(), s.slot))
            .collect()
    }

    /// The player's scripts run first in the synth's chain, each at its own
    /// place, and the player is no slot of its own.
    #[test]
    fn a_player_runs_in_front_of_the_slot_on_its_channel() {
        let mut slots = vec![
            slot(0, 1, false, &[("arp", 0), ("chords", 2)]),
            slot(1, 1, true, &[("synth", 0)]),
        ];
        fold_players(&mut slots);
        assert_eq!(slots.len(), 1);
        assert_eq!(chain(&slots[0]), [("arp", 0), ("chords", 1), ("synth", 2)]);
    }

    /// Notes reach only slots that hear the player's channel: one on
    /// another channel keeps its chain and an omni slot hears every player.
    #[test]
    fn a_player_reaches_only_the_slots_on_its_channel() {
        let mut slots = vec![
            slot(0, 1, false, &[("arp", 0)]),
            slot(1, 3, false, &[("bass", 0)]),
            slot(2, 1, true, &[("synth", 0)]),
            slot(3, 2, true, &[("pad", 0)]),
            slot(4, 0, true, &[]),
        ];
        fold_players(&mut slots);
        let chains: Vec<_> = slots.iter().map(|s| (s.index, chain(s))).collect();
        assert_eq!(
            chains,
            [
                (2, vec![("arp", 0), ("synth", 1)]),
                (3, vec![("pad", 0)]),
                (4, vec![("arp", 0), ("bass", 1)]),
            ]
        );
    }

    /// Two players that hear each other never feed themselves or loop: a
    /// chain only feeds the scripts after it, and a player joins a sounding
    /// slot's chain once. A multi that sounds nothing is left alone.
    #[test]
    fn players_never_feed_themselves() {
        let mut slots = vec![
            slot(0, 0, false, &[("a", 0)]),
            slot(1, 0, false, &[("b", 0)]),
            slot(2, 0, true, &[]),
        ];
        fold_players(&mut slots);
        assert_eq!(slots.len(), 1);
        assert_eq!(chain(&slots[0]), [("a", 0), ("b", 1)]);
        let mut silent = vec![
            slot(0, 0, false, &[("a", 0)]),
            slot(1, 0, false, &[("b", 0)]),
        ];
        fold_players(&mut silent);
        assert_eq!(
            silent.iter().map(chain).collect::<Vec<_>>(),
            [vec![("a", 0)], vec![("b", 0)]]
        );
    }

    /// A real multi from the libraries on this machine.
    #[test]
    #[ignore = "reads a Kontakt multi on this machine"]
    fn a_multis_slots_translate_as_instruments() {
        let Some(path) = std::env::var_os("KONTRA_MULTI") else {
            return;
        };
        let installs = std::env::var_os("KONTAKT_STOCK_EXECUTABLE").map(PathBuf::from);
        let (slots, failed) = slots(Path::new(&path), None, installs.as_slice()).unwrap();
        for s in &slots {
            let found = (s.instrument.samples.iter())
                .filter(|x| x.file.exists())
                .count();
            println!(
                "slot {}: {} groups, {} scripts, {found}/{} samples found, channel {} output {}",
                s.index + 1,
                s.instrument.groups.len(),
                s.instrument.scripts.len(),
                s.instrument.samples.len(),
                s.channel,
                s.output
            );
            assert_eq!(
                found,
                s.instrument.samples.len(),
                "slot {} lost samples",
                s.index + 1
            );
        }
        println!("failed: {failed:?}");
        assert!(!slots.is_empty());
    }

    /// Zero bytes are silence only where zero is: 16-bit PCM yes, unsigned
    /// 8-bit PCM (silence 0x80) no, so an 8-bit sample of zeros sounds.
    #[test]
    fn zero_bytes_are_silence_only_in_a_signed_encoding() {
        let dir = std::env::temp_dir().join(format!("kontra-silent-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = |bits: u16| {
            let mut b = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
            b.extend(16u32.to_le_bytes());
            b.extend(1u16.to_le_bytes()); // PCM
            b.extend(1u16.to_le_bytes()); // mono
            b.extend(48_000u32.to_le_bytes());
            b.extend((48_000u32 * u32::from(bits / 8)).to_le_bytes());
            b.extend((bits / 8).to_le_bytes());
            b.extend(bits.to_le_bytes());
            b.extend(b"data");
            b.extend(64u32.to_le_bytes());
            b.extend([0u8; 64]);
            let file = dir.join(format!("{bits}.wav"));
            std::fs::write(&file, b).unwrap();
            ir::Sample {
                name: String::new(),
                file,
                offset: 0,
                len: None,
                channels: 1,
                frames: 0,
                rate: 48_000,
                keystream: None,
                raw: None,
            }
        };
        let (sixteen, eight) = (silent_wav(&wav(16)), silent_wav(&wav(8)));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(sixteen);
        assert!(!eight);
    }
}
