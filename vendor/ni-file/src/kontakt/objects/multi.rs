//! A multi's bank-level configuration: output routing (0x3E), per-slot MIDI
//! settings (0x48) and the four MIDI port channel maps (0x49).
//!
//! Kontakt 8.13.1 evidence: `BOutputConfiguration` reader `FUN_140cff730`
//! (SerType 0x3E, version 0); the BBank reader `FUN_140d06f50` reads the
//! 0x48/0x49 contents inline up to bank version 0x72 (two bytes per slot,
//! 64 counted `u32` lists, four counted 16-entry maps) and from 0x73 on
//! leaves them to these chunks.

use std::io::Cursor;

use crate::{
    kontakt::{Chunk, KontaktError, StructuredObject},
    read_bytes::ReadBytesExt,
    Error,
};

const SLOTS: usize = 64;
const PARTITIONS: usize = 128;
const BPARFX: u16 = 0x25;

fn expect(chunk: &Chunk, id: u16) -> Result<(), Error> {
    if chunk.id != id {
        return Err(KontaktError::IncorrectID {
            expected: id,
            got: chunk.id,
        }
        .into());
    }
    Ok(())
}

/// A `u32` count followed by that many `u32`, at most `max`.
fn counted(r: &mut Cursor<&[u8]>, max: usize) -> Result<Vec<u32>, Error> {
    let n = r.read_u32_le()? as usize;
    if n > max {
        return Err(Error::Generic(format!("Counted list of {n} exceeds {max}")));
    }
    (0..n).map(|_| Ok(r.read_u32_le()?)).collect()
}

fn ensure_consumed(r: &Cursor<&[u8]>) -> Result<(), Error> {
    if r.position() as usize != r.get_ref().len() {
        return Err(Error::Static("Trailing multi configuration bytes"));
    }
    Ok(())
}

/// One output channel strip (`BOutputConfiguration::OutputPartition`,
/// reader `FUN_140d04e60`, version 0x70).
#[derive(Debug)]
pub struct OutputPartition {
    /// Position among the 128 partitions.
    pub index: usize,
    /// The strip's name, such as `st. 1` or `aux 1`.
    pub name: String,
    /// 2 on every stereo strip seen.
    pub channels: u16,
    /// −1 on the main strip, the aux number (0-based) on aux strips.
    pub routing: i16,
    /// 1.0 on every strip seen (unity).
    pub level: f32,
    /// The strip's insert effects as `BParFX` (0x25) chunks.
    pub fx: Vec<Chunk>,
}

/// SerType 0x3E, `BOutputConfiguration`.
#[derive(Debug)]
pub struct OutputConfiguration {
    /// Up to 128 entries mapping output channels.
    pub channel_map: Vec<u32>,
    pub partitions: Vec<OutputPartition>,
}

impl TryFrom<&Chunk> for OutputConfiguration {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Error> {
        expect(chunk, 0x3E)?;
        let mut r = Cursor::new(chunk.data.as_slice());
        if r.read_u8()? != 0 || r.read_u16_le()? != 0 {
            return Err(Error::Static("Unsupported output configuration version"));
        }
        let channel_map = counted(&mut r, PARTITIONS)?;
        let mut present = [0u8; PARTITIONS / 8];
        std::io::Read::read_exact(&mut r, &mut present)?;
        let mut partitions = Vec::new();
        for index in (0..PARTITIONS).filter(|i| present[i >> 3] & (1 << (i & 7)) != 0) {
            if r.read_u8()? != 0 {
                return Err(Error::Static("Structured output partition"));
            }
            if r.read_u16_le()? != 0x70 {
                return Err(Error::Static("Unsupported output partition version"));
            }
            let name = r.read_widestring_utf16()?;
            let channels = r.read_u16_le()?;
            let routing = r.read_u16_le()? as i16;
            let level = r.read_f32_le()?;
            let count = r.read_u32_le()?;
            if count > 8 {
                return Err(Error::Static("Too many output partition effects"));
            }
            let mut fx = Vec::new();
            for _ in 0..count {
                let at = r.position() as usize;
                StructuredObject::read(&mut r)?;
                fx.push(Chunk {
                    id: BPARFX,
                    data: chunk.data[at..r.position() as usize].to_vec(),
                });
            }
            partitions.push(OutputPartition {
                index,
                name,
                channels,
                routing,
                level,
                fx,
            });
        }
        ensure_consumed(&r)?;
        Ok(Self {
            channel_map,
            partitions,
        })
    }
}

/// One multi slot's MIDI settings.
#[derive(Debug)]
pub struct SlotMidi {
    /// 0 is omni; 1..=64 are ports A–D × channels 1–16. Kontakt's default
    /// for slot n is n + 1.
    pub midi_channel: u8,
    /// Second per-slot byte (BBank `+0x221`).
    pub flag: u8,
    /// Bytes between the two per-slot bytes and the list: 2 in the 24-byte
    /// record, 6 in the 28-byte one (an `i32` that is −1 on unused slots).
    pub extra: Vec<u8>,
    /// Counted list of up to four values.
    pub values: Vec<u32>,
}

/// Chunk 0x48: the 64 slots' MIDI settings.
#[derive(Debug)]
pub struct MultiSlots(pub Vec<SlotMidi>);

impl TryFrom<&Chunk> for MultiSlots {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Error> {
        expect(chunk, 0x48)?;
        let extra = match chunk.data.len() {
            n if n == SLOTS * 24 => 2,
            n if n == SLOTS * 28 => 6,
            n => {
                return Err(Error::Generic(format!(
                    "Unexpected multi slot table of {n} bytes"
                )))
            }
        };
        let mut r = Cursor::new(chunk.data.as_slice());
        let mut slots = Vec::with_capacity(SLOTS);
        for _ in 0..SLOTS {
            let midi_channel = r.read_u8()?;
            let flag = r.read_u8()?;
            let extra = r.read_bytes(extra)?;
            let values = counted(&mut r, 4)?;
            slots.push(SlotMidi {
                midi_channel,
                flag,
                extra,
                values,
            });
        }
        ensure_consumed(&r)?;
        Ok(Self(slots))
    }
}

/// Chunk 0x49: the four MIDI ports' 16-channel maps.
#[derive(Debug)]
pub struct MidiPortMaps(pub [Vec<u32>; 4]);

impl TryFrom<&Chunk> for MidiPortMaps {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Error> {
        expect(chunk, 0x49)?;
        let mut r = Cursor::new(chunk.data.as_slice());
        let maps = [
            counted(&mut r, 16)?,
            counted(&mut r, 16)?,
            counted(&mut r, 16)?,
            counted(&mut r, 16)?,
        ];
        ensure_consumed(&r)?;
        Ok(Self(maps))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counted_bytes(values: &[u32]) -> Vec<u8> {
        let mut out = (values.len() as u32).to_le_bytes().to_vec();
        values.iter().for_each(|v| out.extend(v.to_le_bytes()));
        out
    }

    #[test]
    fn reads_identity_port_maps() {
        let identity: Vec<u32> = (0..16).collect();
        let data = (0..4).flat_map(|_| counted_bytes(&identity)).collect();
        let maps = MidiPortMaps::try_from(&Chunk { id: 0x49, data }).unwrap();
        assert!(maps.0.iter().all(|m| *m == identity));
    }

    #[test]
    fn reads_both_slot_record_sizes() {
        for extra in [2usize, 6] {
            let mut data = Vec::new();
            for slot in 0..SLOTS as u8 {
                data.extend([slot + 1, 0]);
                data.extend(vec![0xff; extra]);
                data.extend(counted_bytes(&[0, 0, 0, 0]));
            }
            let slots = MultiSlots::try_from(&Chunk { id: 0x48, data }).unwrap();
            assert_eq!(slots.0[63].midi_channel, 64);
            assert_eq!(slots.0[0].extra.len(), extra);
        }
    }

    #[test]
    fn reads_an_output_strip_with_an_empty_insert_slot() {
        let mut data = vec![0, 0, 0];
        data.extend(counted_bytes(&[0, 1]));
        let mut present = [0u8; 16];
        present[0] = 0b10;
        data.extend(present);
        data.extend([0, 0x70, 0]);
        data.extend(3u32.to_le_bytes());
        "aux".encode_utf16().for_each(|c| data.extend(c.to_le_bytes()));
        data.extend(2u16.to_le_bytes());
        data.extend(0u16.to_le_bytes());
        data.extend(1f32.to_le_bytes());
        data.extend(1u32.to_le_bytes());
        // An empty structured BParFX v0x50: four private bytes, no public data or children.
        data.extend([1, 0x50, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let o = OutputConfiguration::try_from(&Chunk { id: 0x3E, data }).unwrap();
        assert_eq!(o.channel_map, [0, 1]);
        let strip = &o.partitions[0];
        assert_eq!((strip.index, strip.name.as_str(), strip.channels), (1, "aux", 2));
        assert_eq!(strip.fx.len(), 1);
    }
}
