//! A multi (`.nkm`): its bank's slots, each an instrument of its own.
//!
//! The bank (chunk 0x03) holds a slot list of up to 64 program containers;
//! each holds a program list whose programs are the slot's instrument, as an
//! `.nki` stores it. Each slot's program is cut out as its own chunk and
//! translated as an instrument, with the multi's file table beside it.

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

/// One slot of a multi: where it sits and the instrument it holds.
#[derive(Debug)]
pub struct Slot {
    /// Its place in the multi, 0 to 63.
    pub index: u16,
    pub instrument: ir::Instrument,
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
            Ok(instrument) => slots.push(Slot { index, instrument }),
            Err(e) => failed.push(format!("slot {}: {e}", index + 1)),
        }
    }
    Ok((slots, failed))
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
            println!(
                "slot {}: {} groups, {} scripts",
                s.index + 1,
                s.instrument.groups.len(),
                s.instrument.scripts.len()
            );
        }
        println!("failed: {failed:?}");
        assert!(!slots.is_empty());
    }
}
