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

use std::io::Cursor;
use std::path::Path;

use kontra_ir as ir;
use ni_file::kontakt::objects::Bank;
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
}

/// The slots of the multi at `path`, in slot order; with `only`, that slot
/// alone (the others are not translated).
///
/// # Errors
/// When the file is not a multi this can read; a slot whose instrument
/// cannot be translated is left out of the list with its reason.
pub fn slots(path: &Path, only: Option<u16>) -> Result<(Vec<Slot>, Vec<String>), Error> {
    let (chunks, _, encrypted) = container::opened(path)?;
    let decode = |what| move |e: ni_file::Error| Error::new(path, what, e);
    let bank = chunks
        .find_first(ni_file::kontakt::objects::KONTAKT_BANK_ID)
        .ok_or_else(|| Error::new(path, "multi", "no bank"))?;
    let bank = Bank::try_from(bank).map_err(decode("bank"))?;
    let list = bank.slot_list().map_err(decode("slot list"))?;
    let table = (chunks.find_first(FILE_TABLE)).or_else(|| chunks.find_first(OLD_FILE_TABLE));
    let slot_table = bank.0.find_first(SLOT_TABLE);
    let stride = slot_table.map_or(0, |t| t.data.len() / 64);
    let channel = |index: u16| -> u16 {
        let at = usize::from(index) * stride;
        let bytes = slot_table.and_then(|t| t.data.get(at..at + 2));
        bytes
            .and_then(|b| b.try_into().ok())
            .map_or(0, u16::from_le_bytes)
    };
    let mut indices: Vec<u16> = (list.slots.keys().copied())
        .filter(|i| only.is_none_or(|o| o == *i))
        .collect();
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
        let table = table.map(|t| Chunk {
            id: t.id,
            data: t.data.clone(),
        });
        let stream = KontaktChunks([Some(program), table].into_iter().flatten().collect());
        match translate::instrument(path, &stream, None, &[], encrypted, None) {
            Ok(instrument) => slots.push(Slot {
                index,
                instrument,
                channel: channel(index),
                output: output(&container.0.private_data),
            }),
            Err(e) => failed.push(format!("slot {}: {e}", index + 1)),
        }
    }
    Ok((slots, failed))
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

    /// A real multi from the libraries on this machine.
    #[test]
    #[ignore = "reads a Kontakt multi on this machine"]
    fn a_multis_slots_translate_as_instruments() {
        let Some(path) = std::env::var_os("KONTRA_MULTI") else {
            return;
        };
        let (slots, failed) = slots(Path::new(&path), None).unwrap();
        for s in &slots {
            let found = (s.instrument.samples.iter()).filter(|x| x.file.exists()).count();
            println!(
                "slot {}: {} groups, {} scripts, {found}/{} samples found, channel {} output {}",
                s.index + 1,
                s.instrument.groups.len(),
                s.instrument.scripts.len(),
                s.instrument.samples.len(),
                s.channel,
                s.output
            );
            assert_eq!(found, s.instrument.samples.len(), "slot {} lost samples", s.index + 1);
        }
        println!("failed: {failed:?}");
        assert!(!slots.is_empty());
    }
}
