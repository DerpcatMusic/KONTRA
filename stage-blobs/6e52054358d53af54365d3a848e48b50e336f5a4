//! Borrowed NIS item framing. Layer chains are validated iteratively; children
//! are opened on demand. Unknown properties and descriptor words are preserved.
use crate::{Bytes, Error, ErrorKind, Limits, Reader};
use std::borrow::Cow;

fn version(r: &mut Reader<'_>) -> Result<(), Error> {
    let at = r.0;
    let version = r.u32()?;
    if version != 1 {
        return Err(at.error(ErrorKind::UnsupportedVersion(version)));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub struct Item<'a> {
    raw: Bytes<'a>,
    layers: Bytes<'a>,
    children: Bytes<'a>,
    child_count: usize,
    limits: Limits,
    pub flags: u32,
    pub reserved: u32,
    pub uuid: Bytes<'a>,
    pub trailing: Bytes<'a>,
}

impl<'a> Item<'a> {
    pub fn parse(bytes: &'a [u8], limits: Limits) -> Result<Self, Error> {
        Self::at(
            Bytes {
                data: bytes,
                offset: 0,
            },
            limits,
        )
    }
    fn at(raw: Bytes<'a>, limits: Limits) -> Result<Self, Error> {
        if raw.data.len() > limits.bytes {
            return Err(raw.error(ErrorKind::Limit));
        }
        let mut outer = Reader(raw);
        let body = outer.record64(40)?;
        outer.finish()?;
        let mut r = Reader(body);
        r.take(8)?;
        version(&mut r)?;
        let magic = r.take(4)?;
        if magic.data != b"hsin" {
            return Err(magic.error(ErrorKind::InvalidMagic));
        }
        let flags = r.u32()?;
        let reserved = r.u32()?;
        let uuid = r.take(16)?;
        let layers = r.record64(20)?;
        let mut next = Some(layers);
        let mut count = 0;
        while let Some(raw) = next {
            if count == limits.records {
                return Err(raw.error(ErrorKind::Limit));
            }
            next = Layer::at(raw)?.inner;
            count += 1;
        }
        version(&mut r)?;
        let count_at = r.0;
        let child_count = r.u32()? as usize;
        if child_count > limits.records {
            return Err(count_at.error(ErrorKind::Limit));
        }
        let children = r.0;
        for _ in 0..child_count {
            r.take(12)?;
            r.record64(40)?;
        }
        Ok(Self {
            raw,
            layers,
            children,
            child_count,
            limits,
            flags,
            reserved,
            uuid,
            trailing: r.0,
        })
    }
    pub fn raw(self) -> Bytes<'a> {
        self.raw
    }
    pub fn layers(self) -> impl Iterator<Item = Layer<'a>> {
        let mut next = Some(self.layers);
        std::iter::from_fn(move || {
            let layer = Layer::at(next?).expect("validated NIS layer chain");
            next = layer.inner;
            Some(layer)
        })
    }
    pub fn child_count(self) -> usize {
        self.child_count
    }
    /// Decode an EncryptionItem only when its source marker explicitly says clear.
    /// Compressed storage is owned by the caller; uncompressed storage stays borrowed.
    /// No key discovery, decryption or production playback admission occurs here.
    pub fn unencrypted_subtree(self, max_bytes: usize) -> Result<Cow<'a, [u8]>, Error> {
        let mut layers = self.layers();
        let encryption = layers.next().expect("validated nonempty layer chain");
        encryption.expect(*b"NISD", 0x74)?;
        let mut r = Reader(encryption.properties);
        version(&mut r)?;
        let marker = r.0;
        if r.boolean()? {
            return Err(marker.error(ErrorKind::AccessRequired));
        }
        r.finish()?;
        let subtree = layers
            .next()
            .ok_or_else(|| self.raw.error(ErrorKind::UnsupportedLayout))?;
        subtree.expect(*b"NISD", 0x73)?;
        let mut r = Reader(subtree.properties);
        version(&mut r)?;
        if r.boolean()? {
            let expected = r.u32()? as usize;
            let compressed = r.sized()?;
            r.finish()?;
            Ok(Cow::Owned(crate::nks::expand(
                compressed, expected, max_bytes,
            )?))
        } else {
            let bytes = r.record64(40)?;
            r.finish()?;
            if bytes.data.len() > max_bytes {
                return Err(bytes.error(ErrorKind::Limit));
            }
            Ok(Cow::Borrowed(bytes.data))
        }
    }
    /// Validates each child's own framing when opened; no recursive tree build.
    /// Consumers must handle errors, and impose a traversal depth/total work budget.
    pub fn children(self) -> impl Iterator<Item = Result<Child<'a>, Error>> {
        let mut r = Reader(self.children);
        (0..self.child_count).map(move |_| {
            let descriptor = r.take(12).expect("validated NIS child descriptor");
            let raw = r.record64(40).expect("validated NIS child extent");
            Ok(Child {
                descriptor,
                item: Self::at(raw, self.limits)?,
            })
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Child<'a> {
    /// Original sibling index, domain and ID, not assumed equal to child contents.
    pub descriptor: Bytes<'a>,
    pub item: Item<'a>,
}

#[derive(Clone, Copy, Debug)]
pub struct Layer<'a> {
    raw: Bytes<'a>,
    inner: Option<Bytes<'a>>,
    /// Logical FOURCC order (the wire stores these four bytes reversed).
    pub domain: [u8; 4],
    pub id: u32,
    pub properties: Bytes<'a>,
}

impl<'a> Layer<'a> {
    fn at(raw: Bytes<'a>) -> Result<Self, Error> {
        let mut r = Reader(raw);
        r.u64()?; // The enclosing reader already validated this exact extent.
        let b = r.take(4)?.data;
        let domain = [b[3], b[2], b[1], b[0]];
        let id = r.u32()?;
        version(&mut r)?;
        let inner = if domain == *b"NISD" && id == 1 {
            None
        } else {
            Some(r.record64(20)?)
        };
        Ok(Self {
            raw,
            inner,
            domain,
            id,
            properties: r.0,
        })
    }
    pub fn raw(self) -> Bytes<'a> {
        self.raw
    }
    fn expect(self, domain: [u8; 4], id: u32) -> Result<(), Error> {
        if self.domain == domain && self.id == id {
            Ok(())
        } else {
            Err(self.raw.error(ErrorKind::UnsupportedLayout))
        }
    }
    /// Raw preset chunk bytes, preserving the surrounding authentication/checksum
    /// metadata in `properties`. This does not authenticate the source document.
    pub fn preset_chunks(self) -> Result<Bytes<'a>, Error> {
        self.expect(*b"NISD", 0x6d)?;
        let mut r = Reader(self.properties);
        version(&mut r)?;
        r.u32()?; // Authentication checksum remains in the original properties.
        version(&mut r)?;
        let at = r.0;
        let length = usize::try_from(r.u64()?).map_err(|_| at.error(ErrorKind::Limit))?;
        let chunks = r.take(length)?;
        r.finish()?;
        Ok(chunks)
    }
}
