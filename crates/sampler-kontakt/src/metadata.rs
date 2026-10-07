//! Bounded metadata views. Unknown values retain their wire representation;
//! decoding them does not imply a playback setting or a host filesystem path.
use crate::{Bytes, Chunk, Error, ErrorKind, Limits, Reader, Structured};

fn wide<'a>(r: &mut Reader<'a>) -> Result<Bytes<'a>, Error> {
    let at = r.0;
    let units = r.u32()? as usize;
    r.take(
        units
            .checked_mul(2)
            .ok_or_else(|| at.error(ErrorKind::Limit))?,
    )
}

#[derive(Clone, Copy, Debug)]
pub struct FilenameSegment<'a> {
    pub kind: u8,
    pub text: Option<Bytes<'a>>,
}

fn segment<'a>(r: &mut Reader<'a>) -> Result<FilenameSegment<'a>, Error> {
    let at = r.0;
    let kind = r.u8()?;
    let text = match kind {
        1 | 2 | 4 | 5 | 8 | 9 => Some(wide(r)?),
        3 | 6 | 11 => None,
        _ => return Err(at.error(ErrorKind::UnsupportedLayout)),
    };
    Ok(FilenameSegment { kind, text })
}

#[derive(Clone, Copy, Debug)]
pub struct Filename<'a> {
    pub raw: Bytes<'a>,
    segments: Bytes<'a>,
    count: usize,
}

impl<'a> Filename<'a> {
    fn read(r: &mut Reader<'a>, limits: Limits) -> Result<Self, Error> {
        let start = r.0;
        let count = r.i32()?;
        if count < 0 || count as usize > limits.records {
            return Err(start.error(ErrorKind::Limit));
        }
        let segments = r.0;
        for _ in 0..count {
            segment(r)?;
        }
        Ok(Self {
            raw: Bytes {
                data: &start.data[..r.0.offset - start.offset],
                offset: start.offset,
            },
            segments,
            count: count as usize,
        })
    }
    pub fn segments(self) -> impl Iterator<Item = FilenameSegment<'a>> {
        let mut r = Reader(self.segments);
        (0..self.count).map(move |_| segment(&mut r).expect("validated filename segments"))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FilenameEntry<'a> {
    pub filename: Filename<'a>,
    /// v2 sample metadata, retained as a full raw timestamp.
    pub timestamp: Option<u64>,
    /// v2 sample record, often described as an offset; meaning unestablished.
    pub unknown_record: Option<u32>,
    /// v3's eight-byte prefix and twenty-byte suffix, without invented fields.
    pub prefix: Option<Bytes<'a>>,
    pub suffix: Option<Bytes<'a>>,
}

#[derive(Debug)]
pub struct FileTable<'a> {
    pub version: u16,
    pub special: Vec<Filename<'a>>,
    pub samples: Vec<FilenameEntry<'a>>,
    pub other: Vec<Filename<'a>>,
    /// v3 uses a flat namespace: these entries apply to every lookup category.
    pub flat: Vec<FilenameEntry<'a>>,
    pub extension: Bytes<'a>,
}

fn filenames<'a>(r: &mut Reader<'a>, limits: Limits) -> Result<Vec<Filename<'a>>, Error> {
    let count = r.u32()? as usize;
    if count > limits.records || count > r.0.data.len() / 4 {
        return Err(r.0.error(ErrorKind::Limit));
    }
    (0..count).map(|_| Filename::read(r, limits)).collect()
}

impl<'a> FileTable<'a> {
    pub fn parse(chunk: Chunk<'a>, limits: Limits) -> Result<Self, Error> {
        chunk.expect_id(0x4b)?;
        if chunk.body.data.len() > limits.bytes {
            return Err(chunk.body.error(ErrorKind::Limit));
        }
        let mut r = Reader(chunk.body);
        let version = r.u16()?;
        let mut result = Self {
            version,
            special: Vec::new(),
            samples: Vec::new(),
            other: Vec::new(),
            flat: Vec::new(),
            extension: r.0,
        };
        match version {
            2 => {
                result.special = filenames(&mut r, limits)?;
                let names = filenames(&mut r, limits)?;
                for filename in names {
                    result.samples.push(FilenameEntry {
                        filename,
                        timestamp: Some(r.u64()?),
                        unknown_record: None,
                        prefix: None,
                        suffix: None,
                    });
                }
                for sample in &mut result.samples {
                    sample.unknown_record = Some(r.u32()?);
                }
                result.other = filenames(&mut r, limits)?;
            }
            3 => {
                let count = r.u32()? as usize;
                if count > limits.records || count > r.0.data.len() / 32 {
                    return Err(r.0.error(ErrorKind::Limit));
                }
                for _ in 0..count {
                    let prefix = Some(r.take(8)?);
                    let filename = Filename::read(&mut r, limits)?;
                    result.flat.push(FilenameEntry {
                        filename,
                        timestamp: None,
                        unknown_record: None,
                        prefix,
                        suffix: Some(r.take(20)?),
                    });
                }
            }
            _ => {
                return Err(chunk
                    .body
                    .error(ErrorKind::UnsupportedVersion(u32::from(version))));
            }
        }
        result.extension = r.0;
        Ok(result)
    }
}

/// SaveSettings BFN references may be serialized as segments or a negative
/// string reference. Preserve both encodings; do not reinterpret either.
#[derive(Clone, Copy, Debug)]
pub enum SavedFilename<'a> {
    Segments(Filename<'a>),
    Reference { marker: i32, text: Bytes<'a> },
}

fn saved_filename<'a>(r: &mut Reader<'a>, limits: Limits) -> Result<SavedFilename<'a>, Error> {
    let start = r.0;
    let marker = r.i32()?;
    if marker < 0 {
        Ok(SavedFilename::Reference {
            marker,
            text: wide(r)?,
        })
    } else {
        *r = Reader(start);
        Ok(SavedFilename::Segments(Filename::read(r, limits)?))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SaveSettings<'a> {
    pub object: Structured<'a>,
    pub translated: SavedFilename<'a>,
    pub original: SavedFilename<'a>,
    pub unknown: i32,
    pub flags: [bool; 3],
    pub extension: Bytes<'a>,
}

impl<'a> SaveSettings<'a> {
    pub fn parse(chunk: Chunk<'a>, limits: Limits) -> Result<Self, Error> {
        chunk.expect_id(0x47)?;
        if chunk.body.data.len() > limits.bytes {
            return Err(chunk.body.error(ErrorKind::Limit));
        }
        let object = chunk.structured()?;
        if object.is_structured || object.version != 0x10 {
            return Err(object.raw.error(ErrorKind::UnsupportedLayout));
        }
        let mut r = Reader(object.public);
        Ok(Self {
            object,
            translated: saved_filename(&mut r, limits)?,
            original: saved_filename(&mut r, limits)?,
            unknown: r.i32()?,
            flags: [r.boolean()?, r.boolean()?, r.boolean()?],
            extension: r.0,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct QuickBrowse<'a> {
    pub object: Structured<'a>,
    pub unknown: i32,
    pub extension: Bytes<'a>,
}

impl<'a> QuickBrowse<'a> {
    pub fn parse(chunk: Chunk<'a>) -> Result<Self, Error> {
        chunk.expect_id(0x4e)?;
        let object = chunk.structured()?;
        if object.version != 1 {
            return Err(object
                .raw
                .error(ErrorKind::UnsupportedVersion(u32::from(object.version))));
        }
        let mut r = Reader(object.public);
        Ok(Self {
            object,
            unknown: r.i32()?,
            extension: r.0,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Bank<'a> {
    pub object: Structured<'a>,
    pub volume: f32,
    pub tune: f32,
    pub tempo: i32,
    pub name: Bytes<'a>,
    pub extension: Bytes<'a>,
}

impl<'a> Bank<'a> {
    pub fn parse(chunk: Chunk<'a>) -> Result<Self, Error> {
        chunk.expect_id(3)?;
        let object = chunk.structured()?;
        if !matches!(object.version, 0x60 | 0x71 | 0x72 | 0x73) {
            return Err(object
                .raw
                .error(ErrorKind::UnsupportedVersion(u32::from(object.version))));
        }
        let mut r = Reader(object.public);
        Ok(Self {
            object,
            volume: r.f32()?,
            tune: r.f32()?,
            tempo: r.i32()?,
            name: wide(&mut r)?,
            extension: r.0,
        })
    }
}

#[derive(Debug)]
pub struct ProgramList<'a>(pub Vec<(i16, Structured<'a>)>);

impl<'a> ProgramList<'a> {
    pub fn parse(chunk: Chunk<'a>, limits: Limits) -> Result<Self, Error> {
        chunk.expect_id(0x36)?;
        if chunk.body.data.len() > limits.bytes {
            return Err(chunk.body.error(ErrorKind::Limit));
        }
        let mut r = Reader(chunk.body);
        let count = r.i16()?;
        if count < 0 || count as usize > limits.records {
            return Err(chunk.body.error(ErrorKind::Limit));
        }
        let records = (0..count)
            .map(|_| Ok((r.i16()?, r.structured(false)?)))
            .collect::<Result<_, Error>>()?;
        r.finish()?;
        Ok(Self(records))
    }
}

#[derive(Debug)]
pub struct SlotList<'a>(pub Vec<(u8, Chunk<'a>)>);

impl<'a> SlotList<'a> {
    pub fn parse(chunk: Chunk<'a>, limits: Limits) -> Result<Self, Error> {
        chunk.expect_id(0x37)?;
        if chunk.body.data.len() > limits.bytes {
            return Err(chunk.body.error(ErrorKind::Limit));
        }
        let mut r = Reader(chunk.body);
        let mask = r.u64()?;
        if mask.count_ones() as usize > limits.records {
            return Err(chunk.body.error(ErrorKind::Limit));
        }
        let mut records = Vec::new();
        for slot in 0..64 {
            if mask & (1u64 << slot) != 0 {
                let child = r.chunk()?;
                child.expect_id(0x29)?;
                records.push((slot, child));
            }
        }
        r.finish()?;
        Ok(Self(records))
    }
}
