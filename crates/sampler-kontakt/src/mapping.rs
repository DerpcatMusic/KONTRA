//! Source scalars retain their serialized units and signed values. These views
//! do not admit a group/zone for audio or discard its unmodeled DSP and metadata.
use crate::{Bytes, Chunk, Error, ErrorKind, Limits, Reader, Record, Structured};

#[derive(Clone, Copy, Debug)]
pub struct Group<'a> {
    pub object: Structured<'a>,
    /// Exact UTF-16LE bytes; not decoded with lossy replacement.
    pub name: Bytes<'a>,
    pub gain: f32,
    pub pan: f32,
    /// Serialized linear frequency ratio, not semitones.
    pub tune: f32,
    pub key_tracking: bool,
    pub reverse: bool,
    pub release_trigger: bool,
    pub release_monophonic: bool,
    pub release_counter: i32,
    pub midi_channel: i16,
    pub voice_group: i32,
    pub amp_split: i32,
    pub muted: bool,
    pub soloed: bool,
    pub interpolation: i32,
    pub extension: Bytes<'a>,
}

impl<'a> Group<'a> {
    pub fn parse(record: Record<'a>) -> Result<Self, Error> {
        let object = record.object;
        if record.group.is_some() {
            return Err(object.raw().error(ErrorKind::UnsupportedLayout));
        }
        if object.version != 0x95 {
            return Err(object
                .raw()
                .error(ErrorKind::UnsupportedVersion(u32::from(object.version))));
        }
        let mut r = Reader(object.public);
        let at = r.0;
        let units = r.u32()? as usize;
        let bytes = units
            .checked_mul(2)
            .ok_or_else(|| at.error(ErrorKind::Limit))?;
        let name = r.take(bytes)?;
        Ok(Self {
            object,
            name,
            gain: r.f32()?,
            pan: r.f32()?,
            tune: r.f32()?,
            key_tracking: r.boolean()?,
            reverse: r.boolean()?,
            release_trigger: r.boolean()?,
            release_monophonic: r.boolean()?,
            release_counter: r.i32()?,
            midi_channel: r.i16()?,
            voice_group: r.i32()?,
            amp_split: r.i32()?,
            muted: r.boolean()?,
            soloed: r.boolean()?,
            interpolation: r.i32()?,
            extension: r.0,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Zone<'a> {
    pub object: Structured<'a>,
    pub group: u32,
    pub start: i32,
    /// Raw signed source field. Converting to an asset view needs its profile.
    pub end: i32,
    pub start_modulation: i32,
    pub velocity: [i16; 2],
    pub keys: [i16; 2],
    /// Low/high velocity, then low/high key.
    pub fades: [i16; 4],
    pub root_key: i16,
    pub gain: f32,
    pub pan: f32,
    pub tune: f32,
    pub filename_prefix: Option<Bytes<'a>>,
    pub filename_id: i32,
    /// Sample metadata and additional state remain in their original encoding.
    pub metadata: Bytes<'a>,
}

impl<'a> Zone<'a> {
    pub fn parse(record: Record<'a>) -> Result<Self, Error> {
        let object = record.object;
        let group = record
            .group
            .ok_or_else(|| object.raw().error(ErrorKind::UnsupportedLayout))?;
        if !matches!(object.version, 0x95 | 0x98 | 0x9a) {
            return Err(object
                .raw()
                .error(ErrorKind::UnsupportedVersion(u32::from(object.version))));
        }
        let mut r = Reader(object.public);
        Ok(Self {
            object,
            group,
            start: r.i32()?,
            end: r.i32()?,
            start_modulation: r.i32()?,
            velocity: [r.i16()?, r.i16()?],
            keys: [r.i16()?, r.i16()?],
            fades: [r.i16()?, r.i16()?, r.i16()?, r.i16()?],
            root_key: r.i16()?,
            gain: r.f32()?,
            pan: r.f32()?,
            tune: r.f32()?,
            filename_prefix: if object.version == 0x9a {
                Some(r.take(6)?)
            } else {
                None
            },
            filename_id: r.i32()?,
            metadata: r.0,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LoopSlot<'a> {
    pub slot: u8,
    pub raw: Bytes<'a>,
    pub object: Option<Structured<'a>>,
    pub mode: i32,
    pub start: i32,
    pub length: i32,
    pub count: i32,
    pub alternating: bool,
    pub tune: f32,
    pub crossfade: i32,
    pub extension: Bytes<'a>,
}

/// Eight original loop slots, including holes, disabled entries and unsupported
/// modes. No first-loop fallback or dense reindexing is performed here.
#[derive(Clone, Copy, Debug)]
pub struct Loops<'a>([Option<LoopSlot<'a>>; 8]);

impl<'a> Loops<'a> {
    pub fn parse(chunk: Chunk<'a>, limits: Limits) -> Result<Self, Error> {
        chunk.expect_id(0x39)?;
        if chunk.body.data.len() > limits.bytes {
            return Err(chunk.body.error(ErrorKind::Limit));
        }
        let mut r = Reader(chunk.body);
        let mask = r.u8()?;
        if mask.count_ones() as usize > limits.records {
            return Err(chunk.body.error(ErrorKind::Limit));
        }
        let mut slots = [None; 8];
        for (slot, entry) in slots.iter_mut().enumerate() {
            if mask & (1 << slot) == 0 {
                continue;
            }
            let start = r.0;
            let structured = r.boolean()?;
            let version = r.u16()?;
            if version != 0x60 {
                return Err(start.error(ErrorKind::UnsupportedVersion(u32::from(version))));
            }
            let (object, public) = if structured {
                r = Reader(start);
                let object = r.structured(false)?;
                (Some(object), object.public)
            } else {
                (None, r.take(25)?)
            };
            let mut fields = Reader(public);
            *entry = Some(LoopSlot {
                slot: slot as u8,
                raw: Bytes {
                    data: &start.data[..r.0.offset - start.offset],
                    offset: start.offset,
                },
                object,
                mode: fields.i32()?,
                start: fields.i32()?,
                length: fields.i32()?,
                count: fields.i32()?,
                alternating: fields.boolean()?,
                tune: fields.f32()?,
                crossfade: fields.i32()?,
                extension: fields.0,
            });
        }
        r.finish()?;
        Ok(Self(slots))
    }
    pub fn slots(&self) -> &[Option<LoopSlot<'a>>; 8] {
        &self.0
    }
}
