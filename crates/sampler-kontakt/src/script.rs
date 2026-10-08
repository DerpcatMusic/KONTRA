use crate::{Bytes, Chunk, Error, ErrorKind, Limits, Reader, Structured};

/// Source bytes and saved state are intentionally not coerced into Unicode or
/// initialized VM values. Linked resources, encoding and persistence are separate
/// semantic decisions. In particular, absent and empty text are distinct.
#[derive(Clone, Copy, Debug)]
pub struct Script<'a> {
    pub object: Structured<'a>,
    pub text: Option<Bytes<'a>>,
    pub source_editor_open: bool,
    pub touched_but_not_applied: bool,
    pub bypass: bool,
    pub password_hash: Bytes<'a>,
    pub description: Option<Bytes<'a>>,
    pub textfile_name: Option<Bytes<'a>>,
    /// None means an old record with no table; Some(empty) is an explicit table.
    pub persistent: Option<Strings<'a>>,
    /// Extensions are preserved and must be admitted by the execution profile.
    pub extension: Bytes<'a>,
}

impl<'a> Script<'a> {
    pub fn parse(chunk: Chunk<'a>, limits: Limits) -> Result<Self, Error> {
        chunk.expect_id(0x06)?;
        if chunk.body.data.len() > limits.bytes {
            return Err(chunk.body.error(ErrorKind::Limit));
        }
        let object = chunk.structured()?;
        if !matches!(object.version, 0x50 | 0x60) {
            return Err(object
                .raw
                .error(ErrorKind::UnsupportedVersion(u32::from(object.version))));
        }
        let mut r = Reader(object.public);
        let text = r.optional()?;
        let source_editor_open = r.boolean()?;
        let touched_but_not_applied = r.boolean()?;
        let bypass = r.boolean()?;
        let password_hash = r.sized()?;
        let description = r.optional()?;
        let textfile_name = r.optional()?;
        let persistent = if r.0.data.is_empty() {
            None
        } else {
            let at = r.0;
            let count = r.u32()? as usize;
            if count > limits.records {
                return Err(at.error(ErrorKind::Limit));
            }
            let bytes = r.0;
            for _ in 0..count {
                r.sized()?;
            }
            Some(Strings { bytes, count })
        };
        Ok(Self {
            object,
            text,
            source_editor_open,
            touched_but_not_applied,
            bypass,
            password_hash,
            description,
            textfile_name,
            persistent,
            extension: r.0,
        })
    }
}

/// A validated length-prefixed string table. No lossy UTF-8 replacement and no
/// dropped malformed table: the caller receives exact bytes or a source error.
#[derive(Clone, Copy, Debug)]
pub struct Strings<'a> {
    bytes: Bytes<'a>,
    count: usize,
}

impl<'a> Strings<'a> {
    pub fn len(self) -> usize {
        self.count
    }
    pub fn is_empty(self) -> bool {
        self.count == 0
    }
    pub fn iter(self) -> impl Iterator<Item = Bytes<'a>> {
        let mut r = Reader(self.bytes);
        (0..self.count).map(move |_| r.sized().expect("validated persistent string"))
    }
}
