use std::collections::HashMap;

use std::io::Cursor;
use time::OffsetDateTime;

use crate::{
    kontakt::{chunk::Chunk, objects::BFileName, KontaktError},
    read_bytes::ReadBytesExt,
    Error,
};

const CHUNK_ID: u16 = 0x4B;

/// Lossless, editable native v2 filename-table record. The existing FNTableImpl
/// remains the convenient joined-path/calendar-date view for importers.
/// Sample u32 records and trailing metadata have no inferred semantic meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FNTableRecord {
    pub special_files: Vec<super::BFileNameRecord>,
    pub samples: Vec<FNTableSampleRecord>,
    pub other_files: Vec<super::BFileNameRecord>,
    pub trailing_data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FNTableSampleRecord {
    pub filename: super::BFileNameRecord,
    pub timestamp: u64,
    pub unknown_record: u32,
}

fn read_filenames(reader: &mut Cursor<&[u8]>) -> Result<Vec<super::BFileNameRecord>, Error> {
    let count = reader.read_u32_le()? as usize;
    if count
        > reader
            .get_ref()
            .len()
            .saturating_sub(reader.position() as usize)
            / 4
    {
        return Err(Error::Static("Invalid filename table count"));
    }
    let mut files = Vec::new();
    files
        .try_reserve_exact(count)
        .map_err(|e| Error::Generic(e.to_string()))?;
    for _ in 0..count {
        files.push(super::BFileNameRecord::read(reader)?);
    }
    Ok(files)
}

impl TryFrom<&Chunk> for FNTableRecord {
    type Error = Error;
    fn try_from(chunk: &Chunk) -> Result<Self, Error> {
        if chunk.id != CHUNK_ID {
            return Err(KontaktError::IncorrectID {
                expected: CHUNK_ID,
                got: chunk.id,
            }
            .into());
        }
        let mut reader = Cursor::new(chunk.data.as_slice());
        if reader.read_u16_le()? != 2 {
            return Err(Error::Static("Unsupported filename table version"));
        }
        let special_files = read_filenames(&mut reader)?;
        let filenames = read_filenames(&mut reader)?;
        if filenames.len()
            > reader
                .get_ref()
                .len()
                .saturating_sub(reader.position() as usize)
                / 12
        {
            return Err(Error::Static("Truncated filename sample metadata"));
        }
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(filenames.len())
            .map_err(|e| Error::Generic(e.to_string()))?;
        for filename in filenames {
            samples.push(FNTableSampleRecord {
                filename,
                timestamp: reader.read_u64_le()?,
                unknown_record: 0,
            });
        }
        for sample in &mut samples {
            sample.unknown_record = reader.read_u32_le()?;
        }
        let other_files = read_filenames(&mut reader)?;
        let trailing_data = reader.read_bytes(chunk.data.len() - reader.position() as usize)?;
        Ok(Self {
            special_files,
            samples,
            other_files,
            trailing_data,
        })
    }
}

impl FNTableRecord {
    /// Encode the native table, preserving segment kinds, timestamp precision,
    /// uninterpreted sample records and all trailing metadata.
    pub fn to_chunk(&self) -> Result<Chunk, Error> {
        for count in [
            self.special_files.len(),
            self.samples.len(),
            self.other_files.len(),
        ] {
            u32::try_from(count).map_err(|_| Error::Static("Too many filename table records"))?;
        }
        let mut length = 14usize;
        for file in self
            .special_files
            .iter()
            .chain(self.samples.iter().map(|s| &s.filename))
            .chain(&self.other_files)
        {
            length = length
                .checked_add(file.encoded_len()?)
                .ok_or(Error::Static("Filename table too large"))?;
        }
        length = self
            .samples
            .len()
            .checked_mul(12)
            .and_then(|n| length.checked_add(n))
            .and_then(|n| n.checked_add(self.trailing_data.len()))
            .ok_or(Error::Static("Filename table too large"))?;
        u32::try_from(length).map_err(|_| Error::Static("Filename table too large"))?;
        let mut data = Vec::new();
        data.try_reserve_exact(length)
            .map_err(|e| Error::Generic(e.to_string()))?;
        data.extend(2u16.to_le_bytes());
        data.extend((self.special_files.len() as u32).to_le_bytes());
        for file in &self.special_files {
            file.append_to(&mut data);
        }
        data.extend((self.samples.len() as u32).to_le_bytes());
        for sample in &self.samples {
            sample.filename.append_to(&mut data);
        }
        for sample in &self.samples {
            data.extend(sample.timestamp.to_le_bytes());
        }
        for sample in &self.samples {
            data.extend(sample.unknown_record.to_le_bytes());
        }
        data.extend((self.other_files.len() as u32).to_le_bytes());
        for file in &self.other_files {
            file.append_to(&mut data);
        }
        data.extend(&self.trailing_data);
        Ok(Chunk { id: CHUNK_ID, data })
    }

    pub fn write(&self, writer: impl std::io::Write) -> Result<(), Error> {
        self.to_chunk()?.write(writer)
    }
}

/// A table representing external files of different kinds, used in Kontakt 5.1+.
/// Kontakt: FNTableImpl
/// LibKIO: BFileName
#[derive(Debug)]
pub struct FNTableImpl {
    /// List of resources and paths (nkr, search paths)
    pub special_filetable: HashMap<u32, String>,
    /// List of samples (wav, ncw)
    pub sample_filetable: HashMap<u32, String>,
    /// List of sample timestamps
    pub sample_timestamp_table: HashMap<u32, time::Date>,
    /// List of instruments (nki) and internal files (ir samples)
    pub other_filetable: HashMap<u32, String>,
}

impl std::convert::TryFrom<&Chunk> for FNTableImpl {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        if chunk.id != CHUNK_ID {
            return Err(KontaktError::IncorrectID {
                expected: CHUNK_ID,
                got: chunk.id,
            }
            .into());
        }
        let reader = std::io::Cursor::new(&chunk.data);
        Self::read(reader)
    }
}

impl FNTableImpl {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        let version = reader.read_u16_le()?;
        if version == 3 {
            // Kontakt 8 stores one flat namespace instead of three separate
            // tables. Keep the same native indices for every lookup category.
            let count = reader.read_u32_le()?;
            let position = reader.stream_position()?;
            let end = reader.seek(std::io::SeekFrom::End(0))?;
            reader.seek(std::io::SeekFrom::Start(position))?;
            if u64::from(count) > end.saturating_sub(position) / 32 {
                return Err(Error::Static("Invalid flat filename table count"));
            }
            let mut files = HashMap::new();
            for i in 0..count {
                // Native record header and metadata remain uninterpreted.
                reader.read_bytes(8)?;
                files.insert(i, BFileName::read(&mut reader)?.join("/"));
                reader.read_bytes(20)?;
            }
            return Ok(Self {
                special_filetable: files.clone(),
                sample_filetable: files.clone(),
                other_filetable: files,
                sample_timestamp_table: HashMap::new(),
            });
        }
        if version != 2 {
            return Err(Error::Generic(format!(
                "Unsupported filename table version {version}"
            )));
        }

        // special filetable
        let file_count = reader.read_u32_le()?;
        let mut special_filetable = HashMap::new();
        for i in 0..file_count {
            special_filetable.insert(i, BFileName::read(&mut reader)?.join("/"));
        }

        // sample filetable
        let file_count = reader.read_u32_le()?;
        let mut sample_filetable = HashMap::new();
        for i in 0..file_count {
            sample_filetable.insert(i, BFileName::read(&mut reader)?.join("/"));
        }

        // sample timestamps
        let mut sample_timestamp_table = HashMap::new();
        for i in 0..file_count {
            let unix_timestamp = reader.read_u64_le()? as i64;
            let datetime = OffsetDateTime::from_unix_timestamp(unix_timestamp).unwrap();
            let timestamp: time::Date = datetime.date();
            sample_timestamp_table.insert(i, timestamp);
        }

        // offsets?
        for _ in 0..file_count {
            let _a = reader.read_u32_le()?;
        }

        // other filetable
        let file_count = reader.read_u32_le()?;
        let mut other_filetable = HashMap::new();
        for i in 0..file_count {
            other_filetable.insert(i, BFileName::read(&mut reader)?.join("/"));
        }

        Ok(Self {
            special_filetable,
            sample_filetable,
            other_filetable,
            sample_timestamp_table,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    #[test]
    fn test_fntableimpl() -> Result<(), Error> {
        let mut file = File::open("tests/data/Objects/Kontakt/FNTableImpl/FNTableImpl-000")?;
        file.read_bytes(6)?; // skip chunk header
        FNTableImpl::read(file)?;

        let mut file = File::open("tests/data/Objects/Kontakt/FNTableImpl/FNTableImpl-001")?;
        file.read_bytes(6)?; // skip chunk header
        FNTableImpl::read(file)?;

        let mut file = File::open("tests/data/Objects/Kontakt/FNTableImpl/FNTableImpl-002")?;
        file.read_bytes(6)?; // skip chunk header
        FNTableImpl::read(file)?;

        let mut file = File::open("tests/data/Objects/Kontakt/FNTableImpl/FNTableImpl-003")?;
        file.read_bytes(6)?; // skip chunk header
        FNTableImpl::read(file)?;

        Ok(())
    }

    #[test]
    fn test_fntable_004() -> Result<(), Error> {
        let mut file = File::open("tests/data/Objects/Kontakt/FNTableImpl/FNTableImpl-004")?;
        file.read_bytes(6)?; // skip chunk header
        let _table = FNTableImpl::read(file)?;
        Ok(())
    }

    #[test]
    fn test_fntable_005() -> Result<(), Error> {
        let mut file = File::open("tests/data/Objects/Kontakt/FNTableImpl/FNTableImpl-005")?;
        file.read_bytes(6)?; // skip chunk header
        let _table = FNTableImpl::read(file)?;
        Ok(())
    }

    #[test]
    fn test_fntable_006() -> Result<(), Error> {
        let mut file = File::open("tests/data/Objects/Kontakt/FNTableImpl/FNTableImpl-006")?;
        file.read_bytes(6)?; // skip chunk header
        let _table = FNTableImpl::read(file)?;
        Ok(())
    }
}
