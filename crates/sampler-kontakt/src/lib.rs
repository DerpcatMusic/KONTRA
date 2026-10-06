//! Borrowed Kontakt source records, independent of playback and the old importer.
//!
//! The input is an already expanded Kontakt chunk stream, NOT an NKI/NKM file.
//! Unknown chunks, versions, private fields and duplicate IDs remain intact.
//! Decoding a source record does not admit its semantics for native playback.
#![forbid(unsafe_code)]

#[cfg(feature = "library-access")]
mod access;
mod container;
mod effects;
pub mod keyswitch;
mod library;
mod load;
mod mapping;
pub mod nis;
mod nks;
mod resource_container;
mod resources;
mod samples;
mod script;
mod stream;
#[cfg(feature = "library-access")]
pub use access::library_key;
pub use container::{Multi, read_chunks, read_multi};
pub use library::{Kontakt, read, read_program};
pub use load::{
    Loaded, Options, Progress, finish, load, load_cancelable, load_read, load_read_streamed,
    load_streamed, prepare,
};
pub use mapping::{Group, LoopSlot, Loops, Zone};
pub use nks::Nks42;
pub use resource_container::ResourceContainer;
pub use resources::Resources;
pub use samples::{Decoded, Samples, Source, decode};
pub use script::{Script, Strings};
pub use stream::{SampleReader, StreamPolicy, StreamReport, Streamed, Streamer};

/// Without the `library-access` feature, encrypted content is refused.
#[cfg(not(feature = "library-access"))]
pub fn library_key(
    _: &std::path::Path,
) -> Result<std::sync::Arc<dyn ni_file::nis::LibraryKey>, String> {
    Err("encrypted library content needs the library-access feature".into())
}

/// Why a real instrument could not be loaded, with the file it concerns.
#[derive(Debug)]
pub enum LoadError {
    Io {
        path: std::path::PathBuf,
        error: std::io::Error,
    },
    /// A vendored decoder rejected a record.
    Decode {
        path: std::path::PathBuf,
        what: &'static str,
        error: ni_file::Error,
    },
    /// Encrypted content without usable local access data.
    Access {
        path: std::path::PathBuf,
        reason: String,
    },
    Invalid {
        path: std::path::PathBuf,
        reason: String,
    },
    /// The translated instrument could not be lowered to a playable plan.
    Lower(sampler_core::lower::LowerError),
    /// The caller canceled the load.
    Canceled,
}

impl LoadError {
    pub(crate) fn io(path: &std::path::Path, error: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            error,
        }
    }
    pub(crate) fn decode(
        path: &std::path::Path,
        what: &'static str,
        error: ni_file::Error,
    ) -> Self {
        Self::Decode {
            path: path.into(),
            what,
            error,
        }
    }
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Decode { path, what, error } => write!(f, "{}: {what}: {error}", path.display()),
            Self::Access { path, reason } => {
                write!(f, "{}: encrypted, no access: {reason}", path.display())
            }
            Self::Invalid { path, reason } => write!(f, "{}: {reason}", path.display()),
            Self::Lower(error) => write!(f, "lowering: {error}"),
            Self::Canceled => f.write_str("load canceled"),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { error, .. } => Some(error),
            Self::Decode { error, .. } => Some(error),
            Self::Lower(error) => Some(error),
            _ => None,
        }
    }
}

/// Per-input limits. Nested chunk lists are validated only when explicitly opened;
/// this API never recursively expands an unknown object's contents.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub bytes: usize,
    pub records: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Limit,
    Truncated,
    TrailingData,
    InvalidBoolean,
    UnsupportedLayout,
    UnsupportedVersion(u32),
    IncorrectId { expected: u16, actual: u16 },
    InvalidMagic,
    InvalidCompression,
    LengthMismatch,
    Allocation,
    AccessRequired,
}

/// Byte position in the source buffer being decoded, never a nested relative offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    pub offset: usize,
    pub kind: ErrorKind,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Kontakt source at byte {}: {:?}", self.offset, self.kind)
    }
}
impl std::error::Error for Error {}

/// A byte slice retaining its location in the source document. All source views
/// borrow the caller's one immutable buffer; no self-references or audio ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bytes<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Bytes<'a> {
    pub fn data(self) -> &'a [u8] {
        self.data
    }
    pub fn offset(self) -> usize {
        self.offset
    }
    fn error(self, kind: ErrorKind) -> Error {
        Error {
            offset: self.offset,
            kind,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Reader<'a>(Bytes<'a>);

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<Bytes<'a>, Error> {
        let (data, rest) = self
            .0
            .data
            .split_at_checked(count)
            .ok_or_else(|| self.0.error(ErrorKind::Truncated))?;
        let result = Bytes {
            data,
            offset: self.0.offset,
        };
        self.0 = Bytes {
            data: rest,
            offset: self.0.offset + count,
        };
        Ok(result)
    }
    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?.data[0])
    }
    fn u16(&mut self) -> Result<u16, Error> {
        let b = self.take(2)?.data;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?.data;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Result<u64, Error> {
        let b = self.take(8)?.data;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
    fn i16(&mut self) -> Result<i16, Error> {
        Ok(self.u16()? as i16)
    }
    fn i32(&mut self) -> Result<i32, Error> {
        Ok(self.u32()? as i32)
    }
    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn record64(&mut self, minimum: usize) -> Result<Bytes<'a>, Error> {
        let start = self.0;
        let length = usize::try_from(self.u64()?).map_err(|_| start.error(ErrorKind::Limit))?;
        if length < minimum {
            return Err(start.error(ErrorKind::LengthMismatch));
        }
        self.take(length - 8)?;
        Ok(Bytes {
            data: &start.data[..length],
            offset: start.offset,
        })
    }
    fn boolean(&mut self) -> Result<bool, Error> {
        let at = self.0;
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(at.error(ErrorKind::InvalidBoolean)),
        }
    }
    fn sized(&mut self) -> Result<Bytes<'a>, Error> {
        let length = self.u32()? as usize;
        self.take(length)
    }
    fn optional(&mut self) -> Result<Option<Bytes<'a>>, Error> {
        let length = self.u32()?;
        if length == u32::MAX {
            Ok(None)
        } else {
            self.take(length as usize).map(Some)
        }
    }
    fn finish(self) -> Result<(), Error> {
        if self.0.data.is_empty() {
            Ok(())
        } else {
            Err(self.0.error(ErrorKind::TrailingData))
        }
    }
    fn chunk(&mut self) -> Result<Chunk<'a>, Error> {
        let start = self.0;
        let id = self.u16()?;
        let body = self.sized()?;
        let length = self.0.offset - start.offset;
        Ok(Chunk {
            id,
            body,
            raw: Bytes {
                data: &start.data[..length],
                offset: start.offset,
            },
        })
    }
    fn structured(&mut self, bounded: bool) -> Result<Structured<'a>, Error> {
        let start = self.0;
        if !self.boolean()? {
            // A chunk supplies the end of an unstructured body. An element inside
            // an array does not; treating the rest of the list as one body loses peers.
            if !bounded {
                return Err(start.error(ErrorKind::UnsupportedLayout));
            }
            let version = self.u16()?;
            let public = self.take(self.0.data.len())?;
            return Ok(Structured {
                is_structured: false,
                version,
                public,
                private: self.0,
                children: self.0,
                raw: start,
            });
        }
        let version = self.u16()?;
        let private = self.sized()?;
        let public = self.sized()?;
        let children = self.sized()?;
        let length = self.0.offset - start.offset;
        Ok(Structured {
            is_structured: true,
            version,
            private,
            public,
            children,
            raw: Bytes {
                data: &start.data[..length],
                offset: start.offset,
            },
        })
    }
}

/// Validated framing; iteration is allocation-free and preserves original order.
#[derive(Clone, Copy, Debug)]
pub struct Chunks<'a>(Bytes<'a>);

impl<'a> Chunks<'a> {
    pub fn parse(payload: &'a [u8], limits: Limits) -> Result<Self, Error> {
        Self::at(
            Bytes {
                data: payload,
                offset: 0,
            },
            limits,
        )
    }
    fn at(bytes: Bytes<'a>, limits: Limits) -> Result<Self, Error> {
        if bytes.data.len() > limits.bytes {
            return Err(bytes.error(ErrorKind::Limit));
        }
        let mut reader = Reader(bytes);
        let mut count = 0;
        while !reader.0.data.is_empty() {
            if count == limits.records {
                return Err(reader.0.error(ErrorKind::Limit));
            }
            reader.chunk()?;
            count += 1;
        }
        Ok(Self(bytes))
    }
    pub fn raw(self) -> Bytes<'a> {
        self.0
    }
    pub fn iter(self) -> impl Iterator<Item = Chunk<'a>> {
        let mut reader = Reader(self.0);
        // Construction validates every record against this same immutable slice.
        std::iter::from_fn(move || reader.chunk().ok())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Chunk<'a> {
    pub id: u16,
    pub body: Bytes<'a>,
    raw: Bytes<'a>,
}

impl<'a> Chunk<'a> {
    pub fn raw(self) -> Bytes<'a> {
        self.raw
    }
    pub fn structured(self) -> Result<Structured<'a>, Error> {
        let mut reader = Reader(self.body);
        let object = reader.structured(true)?;
        reader.finish()?;
        Ok(object)
    }
    fn expect_id(self, expected: u16) -> Result<(), Error> {
        if self.id == expected {
            Ok(())
        } else {
            Err(self.raw.error(ErrorKind::IncorrectId {
                expected,
                actual: self.id,
            }))
        }
    }
    /// Group list (0x33) or zone list (0x34), retaining each zone's source group ID.
    pub fn records(self, limits: Limits) -> Result<Records<'a>, Error> {
        if !matches!(self.id, 0x33 | 0x34) {
            return Err(self.raw.error(ErrorKind::UnsupportedLayout));
        }
        if self.body.data.len() > limits.bytes {
            return Err(self.body.error(ErrorKind::Limit));
        }
        let mut reader = Reader(self.body);
        let count = reader.u32()? as usize;
        let zones = self.id == 0x34;
        if count > limits.records {
            return Err(self.body.error(ErrorKind::Limit));
        }
        let records = Records {
            bytes: reader.0,
            count,
            zones,
        };
        for _ in 0..count {
            if zones {
                reader.u32()?;
            }
            reader.structured(false)?;
        }
        reader.finish()?;
        Ok(records)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Structured<'a> {
    pub is_structured: bool,
    pub version: u16,
    pub private: Bytes<'a>,
    pub public: Bytes<'a>,
    children: Bytes<'a>,
    raw: Bytes<'a>,
}

impl<'a> Structured<'a> {
    pub fn raw(self) -> Bytes<'a> {
        self.raw
    }
    pub fn children(self, limits: Limits) -> Result<Chunks<'a>, Error> {
        Chunks::at(self.children, limits)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Record<'a> {
    /// None for a group record, Some(original ID) for a zone record.
    pub group: Option<u32>,
    pub object: Structured<'a>,
}

#[derive(Clone, Copy, Debug)]
pub struct Records<'a> {
    bytes: Bytes<'a>,
    count: usize,
    zones: bool,
}

impl<'a> Records<'a> {
    pub fn len(self) -> usize {
        self.count
    }
    pub fn is_empty(self) -> bool {
        self.count == 0
    }
    pub fn iter(self) -> impl Iterator<Item = Record<'a>> {
        let mut reader = Reader(self.bytes);
        (0..self.count).map(move |_| {
            // The complete list was validated before this view was constructed.
            let group = self
                .zones
                .then(|| reader.u32().expect("validated group ID"));
            Record {
                group,
                object: reader
                    .structured(false)
                    .expect("validated structured record"),
            }
        })
    }
}
