use std::io::Cursor;

use crate::{
    kontakt::{structured_object::StructuredObject, Chunk, KontaktError},
    read_bytes::ReadBytesExt,
    Error,
};

pub const KONTAKT_BANK_ID: u16 = 0x03;

/// Type:           Chunk<StructuredObject>
/// SerType:        0x03
/// Known Versions: 0x60, 0x71, 0x72, 0x73
/// Kontakt 7:      BBank
/// KontaktIO:      K4PL\_Bank
///
/// Children:
///   SaveSettings 0x47
///   BParScript 0x6
///   BParScript 0x6
///   BParScript 0x6
///   BParScript 0x6
///   BParScript 0x6
///   BOutputConfiguration 0x3e
///   SlotList 0x37
///   Unsupported(72) 0x48
///   Unsupported(73) 0x49
///
///   ProgramContainer? 0x29
///   BMidiObject?
///   BMultiChannelProcessBuffer?
///
#[derive(Debug)]
pub struct Bank(pub StructuredObject);

#[derive(Debug)]
pub struct BankPublicParams {
    pub master_volume: f32,
    pub master_tune: f32,
    pub master_tempo: i32,
    pub name: String,
}

#[derive(Debug)]
pub struct BankSlotSettings {
    /// Native filter: 0 accepts any channel; otherwise compare channel - 1.
    pub channel: u16,
    pub flags: [u8; 2],
    pub auxiliary: Vec<f32>,
}

#[derive(Debug)]
pub struct BankOutputConfiguration {
    pub mapping: Vec<u32>,
    /// Occupied records in their original bitmap order.
    pub records: Vec<BankOutputRecord>,
}

#[derive(Debug)]
pub struct BankOutputRecord {
    pub index: u8,
    pub name: String,
    pub channels: u16,
    /// -1 disables auxiliary routing; 0..3 select a slot auxiliary level.
    pub auxiliary: i16,
    pub volume: f32,
    pub fx: Vec<StructuredObject>,
}

impl Bank {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        let so = StructuredObject::read(&mut reader)?;

        Ok(Self(so))
    }

    pub fn params(&self) -> Result<BankPublicParams, Error> {
        let mut reader = Cursor::new(&self.0.public_data);

        Ok(BankPublicParams {
            master_volume: reader.read_f32_le()?,
            master_tune: reader.read_f32_le()?,
            master_tempo: reader.read_i32_le()?,
            name: reader.read_widestring_utf16()?,
        })
    }

    pub fn slot_list(&self) -> Result<super::SlotList, Error> {
        (&self.0.children[7]).try_into()
    }

    /// Decode the verified v0x73/v0x74 slot metadata without applying it.
    pub fn slot_settings(&self) -> Result<Vec<BankSlotSettings>, Error> {
        if !matches!(self.0.version, 0x73 | 0x74) {
            return Err(Error::Static("Unsupported bank slot-settings version"));
        }
        let chunk = self.0.find_first(0x48).ok_or(KontaktError::MissingChunk(0x48))?;
        let mut reader = Cursor::new(&chunk.data);
        let mut slots = Vec::with_capacity(64);
        for _ in 0..64 {
            let channel = reader.read_u16_le()?;
            let flags = [reader.read_u8()?, reader.read_u8()?];
            let count = reader.read_u32_le()?;
            if count > 4 {
                return Err(Error::Static("Bank slot has more than four auxiliary levels"));
            }
            let auxiliary = (0..count).map(|_| reader.read_f32_le()).collect::<Result<_, _>>()?;
            slots.push(BankSlotSettings { channel, flags, auxiliary });
        }
        if reader.position() != chunk.data.len() as u64 {
            return Err(Error::Static("Unsupported bank slot-settings trailing data"));
        }
        Ok(slots)
    }

    pub fn output_configuration(&self) -> Result<BankOutputConfiguration, Error> {
        let chunk = self.0.find_first(0x3e).ok_or(KontaktError::MissingChunk(0x3e))?;
        if chunk.data.first() != Some(&0) {
            return Err(Error::Static("Unsupported structured bank output configuration"));
        }
        let object = StructuredObject::try_from(chunk)?;
        if object.version != 0 || !object.private_data.is_empty() || !object.children.is_empty() {
            return Err(Error::Static("Unsupported bank output-configuration format"));
        }
        let mut reader = Cursor::new(&object.public_data);
        let count = reader.read_u32_le()? as usize;
        if count > object.public_data.len().saturating_sub(20) / 4 {
            return Err(Error::Static("Truncated bank output mapping"));
        }
        let mapping = (0..count).map(|_| reader.read_u32_le()).collect::<Result<_, _>>()?;
        let occupied = reader.read_bytes(16)?;
        let mut records = Vec::new();
        for index in 0..128 {
            if occupied[index / 8] & (1 << (index % 8)) == 0 { continue; }
            if reader.read_u8()? != 0 || reader.read_u16_le()? != 0x70 {
                return Err(Error::Static("Unsupported bank output-record format"));
            }
            let name = reader.read_widestring_utf16()?;
            let channels = reader.read_u16_le()?;
            let auxiliary = reader.read_i16_le()?;
            let volume = reader.read_f32_le()?;
            let count = reader.read_u32_le()?;
            if count > 4 {
                return Err(Error::Static("Bank output has more than four effects"));
            }
            let mut fx = Vec::with_capacity(count as usize);
            for _ in 0..count {
                if object.public_data.get(reader.position() as usize) != Some(&1) {
                    return Err(Error::Static("Unsupported unstructured bank output effect"));
                }
                fx.push(StructuredObject::read(&mut reader)?);
            }
            records.push(BankOutputRecord { index: index as u8, name, channels, auxiliary, volume, fx });
        }
        if reader.position() != object.public_data.len() as u64 {
            return Err(Error::Static("Unsupported bank output-configuration trailing data"));
        }
        Ok(BankOutputConfiguration { mapping, records })
    }
}

impl std::convert::TryFrom<&Chunk> for Bank {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        // match KontaktObject::try_from(chunk)? {
        //     KontaktObject::BBank(bank) => Ok(bank),
        //     _ => Err(KontaktError::IncorrectID {
        //         expected: 0x03,
        //         got: chunk.id,
        //     }.into())
        // }
        if chunk.id != KONTAKT_BANK_ID {
            return Err(KontaktError::IncorrectID {
                expected: KONTAKT_BANK_ID,
                got: chunk.id,
            }
            .into());
        }
        let reader = Cursor::new(&chunk.data);
        Self::read(reader)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use std::fs::File;

    #[test]
    fn test_bank() -> Result<(), Error> {
        let chunk = Chunk::read(File::open(
            "tests/data/Objects/Kontakt/Bank/BankV73-000.kon",
        )?)?;
        let bank = Bank::try_from(&chunk)?;
        dbg!(bank.0.version);
        dbg!(bank.params()?);
        for chunk in bank.0.children {
            println!("{:?} 0x{:x}", chunk.into_object()?, chunk.id);

            // let filename = format!("{:?}-{:x}.chunk", chunk.into_type()?, chunk.id);
            // std::fs::write(filename, chunk.data)?;
        }
        Ok(())
    }
}
