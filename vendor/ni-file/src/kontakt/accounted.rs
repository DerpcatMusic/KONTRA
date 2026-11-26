//! Byte-accounted readers for the records and objects the native reader tables leave
//! unread: a bank's and a program's blocks, voice groups, start criteria, a zone's
//! sample markers, a slot's program list, output routing, multi MIDI tables, the
//! sample file-name table and the session state.
//!
//! Every layout here is derived from the bytes of real Kontakt files: version fields,
//! length prefixes, repeated structures and values cross-checked against what KONTRA's
//! other decoders read from the same files. A reader accounts for every byte of its
//! input or fails. Each value it reads is a [`Field`] at depth 0, so a test can check
//! that the fields tile the input and that every value re-encodes to its bytes.
//!
//! Where every observed file holds zeros, the width of a field is not observable; such
//! spans read as single bytes.

use super::native_schema::{decode_chunk, Decoded, Field, Opaque, Undecoded};

/// A cursor that records each value it reads.
struct Tape<'a> {
    data: &'a [u8],
    pos: usize,
    fields: Vec<Field>,
    opaque: Vec<Opaque>,
}

type R<T> = Result<T, Undecoded>;

impl<'a> Tape<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            fields: Vec::new(),
            opaque: Vec::new(),
        }
    }

    fn take(&mut self, kind: char, len: usize, value: Option<u64>) -> R<&'a [u8]> {
        let bytes = self
            .pos
            .checked_add(len)
            .and_then(|end| self.data.get(self.pos..end))
            .ok_or(Undecoded::Truncated)?;
        self.fields.push(Field {
            kind,
            offset: self.pos,
            len,
            value,
            slot: None,
            depth: 0,
        });
        self.pos += len;
        Ok(bytes)
    }

    /// An unsigned little-endian integer of `n` bytes (kind B, H, I or Q).
    fn uint(&mut self, n: usize) -> R<u64> {
        let kind = match n {
            1 => 'B',
            2 => 'H',
            4 => 'I',
            _ => 'Q',
        };
        let at = self.pos;
        let b = self
            .data
            .get(at..at.saturating_add(n))
            .ok_or(Undecoded::Truncated)?;
        let v = b.iter().rev().fold(0u64, |v, &b| v << 8 | u64::from(b));
        self.take(kind, n, Some(v))?;
        Ok(v)
    }

    fn u8(&mut self) -> R<u8> {
        self.uint(1).map(|v| v as u8)
    }
    fn u16(&mut self) -> R<u16> {
        self.uint(2).map(|v| v as u16)
    }
    fn u32(&mut self) -> R<u32> {
        self.uint(4).map(|v| v as u32)
    }
    fn u64(&mut self) -> R<u64> {
        self.uint(8)
    }
    fn f32(&mut self) -> R<f32> {
        let v = u32::from_le_bytes(self.peek(4)?.try_into().unwrap_or_default());
        self.take('F', 4, Some(u64::from(v)))?;
        Ok(f32::from_bits(v))
    }

    fn peek(&self, n: usize) -> R<&'a [u8]> {
        self.data
            .get(self.pos..self.pos.saturating_add(n))
            .ok_or(Undecoded::Truncated)
    }

    /// `n` single bytes.
    fn bytes(&mut self, n: usize) -> R<()> {
        (0..n).try_for_each(|_| self.u8().map(drop))
    }

    /// A u32 count of UTF-16 units, then the units (kind W, value the count).
    fn wide(&mut self) -> R<()> {
        let n = u32::from_le_bytes(self.peek(4)?.try_into().unwrap_or_default());
        let len = (n as usize).checked_mul(2).and_then(|b| b.checked_add(4));
        self.take('W', len.ok_or(Undecoded::Truncated)?, Some(u64::from(n)))?;
        Ok(())
    }

    /// A u32 byte count, then the bytes (kind S, value the count).
    fn string(&mut self) -> R<()> {
        let n = u32::from_le_bytes(self.peek(4)?.try_into().unwrap_or_default());
        self.take('S', (n as usize).saturating_add(4), Some(u64::from(n)))?;
        Ok(())
    }

    /// A count that the remaining bytes can hold at `min` bytes per element.
    fn count(&mut self, min: usize) -> R<usize> {
        let n = self.u32()? as usize;
        if n.saturating_mul(min) > self.data.len() - self.pos {
            return Err(Undecoded::Truncated);
        }
        Ok(n)
    }

    /// An inline object header: flag 0 and the given version.
    fn inline(&mut self, version: u16) -> R<()> {
        if self.u8()? != 0 || self.u16()? != version {
            return Err(Undecoded::Rejected);
        }
        Ok(())
    }

    /// A structured object (flag 1, version, private, public and children areas) with no
    /// chunk header, decoded as a child chunk of SerType `id` is (kind O, value the id).
    fn object(&mut self, id: u16) -> R<()> {
        let mut len = 3;
        if self.peek(1)? != [1] {
            return Err(Undecoded::Rejected);
        }
        for _ in 0..3 {
            let n = self
                .data
                .get(self.pos + len..self.pos + len + 4)
                .ok_or(Undecoded::Truncated)?;
            len += 4 + u32::from_le_bytes(n.try_into().unwrap_or_default()) as usize;
        }
        let body = self.take('O', len, Some(u64::from(id)))?;
        self.opaque.extend(decode_chunk(id, body));
        Ok(())
    }

    fn done(self, class: &'static str) -> R<Decoded> {
        if self.pos != self.data.len() {
            return Err(Undecoded::Trailing(self.data.len() - self.pos));
        }
        Ok(Decoded {
            class,
            fields: self.fields,
            consumed: self.pos,
            opaque: self.opaque,
        })
    }
}

/// Whether a reader here reads chunk `id` from its body on (a record, not an object).
pub(crate) fn has_record(id: u16) -> bool {
    matches!(id, 0x32 | 0x36 | 0x38 | 0x3e | 0x48 | 0x49 | 0x4b)
}

/// Whether a reader here reads the blocks of objects of SerType `id`.
pub(crate) fn has_block(id: u16) -> bool {
    matches!(id, 0x03 | 0x28 | 0x35 | 0x0f02)
}

/// Whether a reader here reads stored `version` of SerType `id`.
pub(crate) fn reads_block(id: u16, version: u16) -> bool {
    match id {
        0x03 => matches!(version, 0x73 | 0x76 | 0x77),
        0x28 => program_suffix_len(version).is_some(),
        0x35 | 0x0f02 => version == 0x10,
        _ => false,
    }
}

/// A chunk body read from its first byte on, or None when no reader here reads `id`.
pub(crate) fn record(id: u16, data: &[u8]) -> Option<R<Decoded>> {
    has_record(id).then(|| {
        let mut t = Tape::new(data);
        let class = match id {
            0x32 => voice_groups(&mut t).map(|()| "VoiceGroups"),
            0x36 => program_list(&mut t).map(|()| "ProgramList"),
            0x38 => start_criteria(&mut t).map(|()| "StartCriteriaList"),
            0x3e => output_configuration(&mut t).map(|()| "BOutputConfiguration"),
            0x48 => return multi_slots(data),
            0x49 => return midi_port_maps(data),
            _ => file_name_table(&mut t).map(|()| "FNTableImpl"),
        }?;
        t.done(class)
    })
}

/// One block (private or public) of an object of SerType `id` at stored `version`, or
/// None when no reader here reads it.
pub(crate) fn block(id: u16, version: u16, private: bool, data: &[u8]) -> Option<R<Decoded>> {
    reads_block(id, version).then(|| {
        let mut t = Tape::new(data);
        let class = match (id, private) {
            (0x03, true) => bank_private(&mut t, version).map(|()| "Bank"),
            (0x03, false) => bank_public(&mut t).map(|()| "Bank"),
            (0x28, true) => program_private(&mut t, version).map(|()| "Program"),
            (0x28, false) => program_public(&mut t, version).map(|()| "Program"),
            (0x35, false) => sample_markers(&mut t).map(|()| "ZoneSampleMarkers"),
            (0x0f02, false) => session_state(&mut t).map(|()| "SessionState"),
            _ => Err(Undecoded::Rejected),
        }?;
        t.done(class)
    })
}

/// Bank private block: three bytes; from 0x76 eight more and an identifier string.
fn bank_private(t: &mut Tape, version: u16) -> R<()> {
    t.bytes(3)?;
    if version >= 0x76 {
        t.bytes(8)?;
        t.string()?;
    }
    Ok(())
}

/// Bank public block: master volume, master tune, tempo and the multi's name.
fn bank_public(t: &mut Tape) -> R<()> {
    t.f32()?;
    t.f32()?;
    t.u32()?;
    t.wide()
}

/// How many bytes of the program private block's suffix each stored version holds.
fn program_suffix_len(version: u16) -> Option<usize> {
    match version {
        0xa8 | 0xab => Some(26),
        0xae | 0xb1 | 0xb3 => Some(40),
        0xb5 => Some(52),
        _ => None,
    }
}

/// Reads `layout` in order: 'I' a 4-byte field, 'H' a 2-byte one, 'B' a single byte.
fn fixed(t: &mut Tape, layout: &str) -> R<()> {
    layout.chars().try_for_each(|c| {
        t.uint(match c {
            'I' => 4,
            'H' => 2,
            _ => 1,
        })
        .map(drop)
    })
}

/// Program private block: a fixed prefix, a list of named records (a u32 size and, when
/// it is not zero, a u32 count and the records), then a fixed suffix that grows with the
/// version. A 4-byte field is one that holds a value above its
/// low byte, -1 or a count in some file; the rest read as single bytes.
fn program_private(t: &mut Tape, version: u16) -> R<()> {
    let suffix = program_suffix_len(version).ok_or(Undecoded::NoReader)?;
    fixed(t, "IBBIBII")?; // 0..19
    fixed(t, &"B".repeat(25))?; // 19..44, flags
    fixed(t, "IIIBBH")?; // 44..60
    fixed(t, &"B".repeat(9))?; // 60..69
    let n = match t.u32()? {
        0 => 0,
        _ => t.count(26)?,
    };
    // Each record (flag 0, version 0x70 or 0x71): a u32, a byte, a u16 index, a u32, an
    // i32, two f32 and a name.
    for _ in 0..n {
        if t.u8()? != 0 || !matches!(t.u16()?, 0x70 | 0x71) {
            return Err(Undecoded::Rejected);
        }
        t.u32()?;
        t.u8()?;
        t.u16()?;
        t.u32()?;
        t.u32()?;
        t.f32()?;
        t.f32()?;
        t.string()?;
    }
    let layout = concat!(
        "IBBBBBBBBIIIBB", // 26 bytes (0xa8, 0xab)
        "IIBBBBBB",       // 14 more (0xae, 0xb1, 0xb3)
        "III",            // 12 more (0xb5)
    );
    let mut left = suffix;
    for c in layout.chars() {
        if left == 0 {
            break;
        }
        let n = if c == 'I' { 4 } else { 1 };
        t.uint(n)?;
        left = left.checked_sub(n).ok_or(Undecoded::Rejected)?;
    }
    Ok(())
}

/// Program public block: name, sizes, tuning and ranges, three credit strings, three
/// categories, then fields later versions append.
fn program_public(t: &mut Tape, version: u16) -> R<()> {
    t.wide()?; // name
    t.u64()?; // total sample bytes (f64 bits)
    t.u8()?; // transpose
    t.f32()?; // volume
    t.f32()?; // pan
    t.f32()?; // tune
    t.bytes(4)?; // velocity and key range
    t.u16()?; // default key switch
    t.u32()?; // preload size
    t.u32()?; // library id
    t.u32()?; // fingerprint
    t.u32()?; // loading flags
    t.u8()?; // group solo
    t.u32()?; // icon
    for _ in 0..3 {
        t.wide()?; // credits, author, url
    }
    for _ in 0..3 {
        t.u16()?; // categories
    }
    if version >= 0x91 {
        t.u32()?; // resource container file reference
    }
    if version == 0xa6 {
        t.wide()?;
        t.wide()?;
    }
    if version >= 0xa6 {
        t.wide()?;
    }
    if version >= 0xa8 {
        t.wide()?;
    }
    if version >= 0xaf {
        t.u32()?;
    }
    if version >= 0xb0 {
        sound_data(t)?;
    }
    if version >= 0xb1 {
        t.u8()?;
        sound_data(t)?;
    }
    if version >= 0xb2 {
        t.u32()?;
    }
    if version >= 0xb3 {
        let n = t.count(1)?;
        t.bytes(n)?;
    }
    if version >= 0xb4 {
        t.u32()?;
    }
    if version >= 0xa6 {
        t.u32()?; // file reference
    }
    if version >= 0xa2 {
        t.u32()?; // file reference
    }
    Ok(())
}

fn expect(t: &mut Tape, want: u32) -> R<()> {
    if t.u32()? != want {
        return Err(Undecoded::Rejected);
    }
    Ok(())
}

/// An inline sound-data object (flag 0, version 1): a presence byte, then metadata,
/// grouped items and an optional 16-byte value.
fn sound_data(t: &mut Tape) -> R<()> {
    t.inline(1)?;
    if t.u8()? == 0 {
        return Ok(());
    }
    expect(t, 2)?;
    t.u32()?;
    t.u32()?;
    for _ in 0..5 {
        t.wide()?;
    }
    for _ in 0..7 {
        t.u32()?;
    }
    for _ in 0..t.count(4)? {
        for _ in 0..t.count(4)? {
            t.wide()?;
        }
    }
    for _ in 0..t.count(4)? {
        t.wide()?;
    }
    for _ in 0..2 {
        for _ in 0..t.count(8)? {
            t.wide()?;
            t.wide()?;
        }
    }
    expect(t, 1)?;
    for _ in 0..t.count(12)? {
        expect(t, 1)?;
        let items = t.count(24)?;
        t.wide()?;
        for _ in 0..items {
            expect(t, 1)?;
            t.wide()?;
            t.f32()?;
            t.f32()?;
            t.u32()?;
            t.u32()?;
        }
    }
    if t.u8()? != 0 {
        t.u32()?;
        t.u16()?;
        t.u16()?;
        t.bytes(8)?;
    }
    Ok(())
}

/// One voice limit (flag 0, version 0x60): name, kill mode, prefer-released, voices,
/// fade time and exclusion group.
fn voice_limit(t: &mut Tape) -> R<()> {
    t.inline(0x60)?;
    t.wide()?;
    t.u16()?;
    t.u8()?;
    t.u32()?;
    t.u32()?;
    t.u32()?;
    Ok(())
}

/// 0x32: the instrument's voice limit, a 128-bit mask, one limit per set bit.
fn voice_groups(t: &mut Tape) -> R<()> {
    voice_limit(t)?;
    let mask = t.peek(16)?;
    t.bytes(16)?;
    for i in 0..128 {
        if mask[i / 8] & (1 << (i % 8)) != 0 {
            voice_limit(t)?;
        }
    }
    Ok(())
}

/// 0x36: a u16 count, then per program its u16 index and the program object.
fn program_list(t: &mut Tape) -> R<()> {
    for _ in 0..t.u16()? {
        t.u16()?;
        t.object(0x28)?;
    }
    Ok(())
}

/// 0x38: an 8-bit mask, one criterion (flag 0, version 0x70) per set bit: mode, next,
/// key range, controller and its range, cycle class, slice zone and slice, a flag.
fn start_criteria(t: &mut Tape) -> R<()> {
    let mask = t.u8()?;
    for _ in 0..mask.count_ones() {
        t.inline(0x70)?;
        t.u32()?;
        t.u32()?;
        for _ in 0..5 {
            t.u16()?;
        }
        for _ in 0..3 {
            t.u32()?;
        }
        t.u8()?;
    }
    Ok(())
}

/// 0x3E (flag 0, version 0): a counted channel map, a 128-bit mask of output strips, and
/// per strip (flag 0, version 0x70) its name, channels, routing, level and counted
/// insert effects.
fn output_configuration(t: &mut Tape) -> R<()> {
    t.inline(0)?;
    for _ in 0..t.count(4)? {
        t.u32()?;
    }
    let mask = t.peek(16)?;
    t.bytes(16)?;
    for i in 0..128 {
        if mask[i / 8] & (1 << (i % 8)) != 0 {
            t.inline(0x70)?;
            t.wide()?;
            t.u16()?;
            t.u16()?;
            t.f32()?;
            for _ in 0..t.count(19)? {
                t.object(0x25)?;
            }
        }
    }
    Ok(())
}

/// 0x48: 64 slots, each a MIDI channel byte, a flag byte, a u16 (and from later banks an
/// i32), then a counted list of at most four u32. The record does not say which slot
/// size it holds, so the one that reads every byte is taken.
fn multi_slots(data: &[u8]) -> R<Decoded> {
    let read = |wide: bool| {
        let mut t = Tape::new(data);
        for _ in 0..64 {
            t.u8()?;
            t.u8()?;
            t.u16()?;
            if wide {
                t.u32()?;
            }
            let n = t.count(4)?;
            if n > 4 {
                return Err(Undecoded::Rejected);
            }
            for _ in 0..n {
                t.u32()?;
            }
        }
        t.done("MultiSlots")
    };
    read(data.len() == 64 * 28).or_else(|_| read(data.len() != 64 * 28))
}

/// 0x49: one counted 64-entry channel map, or four counted 16-entry maps.
fn midi_port_maps(data: &[u8]) -> R<Decoded> {
    let read = |maps: usize, max: usize| {
        let mut t = Tape::new(data);
        for _ in 0..maps {
            let n = t.count(4)?;
            if n > max {
                return Err(Undecoded::Rejected);
            }
            for _ in 0..n {
                t.u32()?;
            }
        }
        t.done("MidiPortMaps")
    };
    read(1, 64).or_else(|_| read(4, 16))
}

/// A file name: a segment count, then per segment its kind and, for text segments, a
/// UTF-16 string.
fn file_name(t: &mut Tape) -> R<()> {
    for _ in 0..t.count(1)? {
        match t.u8()? {
            1 | 2 | 4 | 5 | 8 | 9 => t.wide()?,
            3 | 6 | 11 => {}
            _ => return Err(Undecoded::Rejected),
        }
    }
    Ok(())
}

fn file_names(t: &mut Tape) -> R<usize> {
    let n = t.count(4)?;
    for _ in 0..n {
        file_name(t)?;
    }
    Ok(n)
}

/// 0x4B, the sample file-name table, led by a u16 version.
/// - 2: special files, sample files, one u64 then one u32 per sample, other files, one
///   u16 per other file, and in some tables further counted name lists.
/// - 3: a count, then per file two u32, two file names, a u64 and two u32.
fn file_name_table(t: &mut Tape) -> R<()> {
    match t.u16()? {
        2 => {
            file_names(t)?;
            let samples = file_names(t)?;
            for _ in 0..samples {
                t.u64()?;
            }
            for _ in 0..samples {
                t.u32()?;
            }
            for _ in 0..file_names(t)? {
                t.u16()?;
            }
            // Some tables end with further counted name lists (empty in every file seen).
            while t.pos < t.data.len() {
                file_names(t)?;
            }
        }
        3 => {
            for _ in 0..t.count(32)? {
                t.u32()?;
                t.u32()?;
                file_name(t)?;
                file_name(t)?;
                t.u64()?;
                t.u32()?;
                t.u32()?;
            }
        }
        _ => return Err(Undecoded::Rejected),
    }
    Ok(())
}

/// 0x35 (version 0x10), a zone's sample markers: a byte, an i32, a marker count and,
/// when there are markers, the sample length, then per marker its frame as u32 and f32
/// and a u32 flag word.
fn sample_markers(t: &mut Tape) -> R<()> {
    t.u8()?;
    t.u32()?;
    let n = t.count(12)?;
    if n > 0 {
        t.u32()?;
        for _ in 0..n {
            t.u32()?;
            t.f32()?;
            t.u32()?;
        }
    }
    Ok(())
}

/// 0x0F02 (version 0x10): child chunks. 0x0F01 (flag 0, version 0x10) holds a u64
/// length and a MessagePack document of that length; 0x0F00 (flag 0, version 0x10) an
/// f32, three u32 and twelve bytes.
fn session_state(t: &mut Tape) -> R<()> {
    while t.pos < t.data.len() {
        let id = t.u16()?;
        let len = t.u32()? as usize;
        let end = t.pos.saturating_add(len);
        t.inline(0x10)?;
        match id {
            0x0f01 => {
                let n = usize::try_from(t.u64()?).map_err(|_| Undecoded::Truncated)?;
                let doc = t.peek(n)?;
                if msgpack_len(doc) != Some(n) {
                    return Err(Undecoded::Rejected);
                }
                t.take('R', n, None)?;
            }
            0x0f00 => {
                t.f32()?;
                t.u32()?;
                t.u32()?;
                t.u32()?;
                t.bytes(12)?;
            }
            _ => return Err(Undecoded::Rejected),
        }
        if t.pos != end {
            return Err(Undecoded::Rejected);
        }
    }
    Ok(())
}

/// The byte length of the one MessagePack value at the start of `d`.
fn msgpack_len(d: &[u8]) -> Option<usize> {
    let (mut at, mut pending) = (0usize, 1usize);
    let be = |at: usize, n: usize| -> Option<usize> {
        let b = d.get(at..at + n)?;
        Some(b.iter().fold(0usize, |v, &b| v << 8 | usize::from(b)))
    };
    while pending > 0 {
        pending -= 1;
        let tag = *d.get(at)?;
        at += 1;
        match tag {
            0x00..=0x7f | 0xc0 | 0xc2 | 0xc3 | 0xe0..=0xff => {}
            0x80..=0x8f => pending += 2 * usize::from(tag & 0x0f),
            0x90..=0x9f => pending += usize::from(tag & 0x0f),
            0xa0..=0xbf => at += usize::from(tag & 0x1f),
            0xc4..=0xc6 | 0xd9..=0xdb => {
                let w = if tag <= 0xc6 {
                    1 << (tag - 0xc4)
                } else {
                    1 << (tag - 0xd9)
                };
                at += w + be(at, w)?;
            }
            0xc7..=0xc9 => {
                let w = 1 << (tag - 0xc7);
                at += w + 1 + be(at, w)?;
            }
            0xca => at += 4,
            0xcb => at += 8,
            0xcc..=0xcf => at += 1 << (tag - 0xcc),
            0xd0..=0xd3 => at += 1 << (tag - 0xd0),
            0xd4..=0xd8 => at += 1 + (1 << (tag - 0xd4)),
            0xdc | 0xdd => {
                let w = if tag == 0xdc { 2 } else { 4 };
                pending += be(at, w)?;
                at += w;
            }
            0xde | 0xdf => {
                let w = if tag == 0xde { 2 } else { 4 };
                pending += 2 * be(at, w)?;
                at += w;
            }
            _ => return None,
        }
    }
    (at <= d.len()).then_some(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msgpack_lengths() {
        // {"a": [1, "bc", nil], "d": 2.0f32}
        let doc = [
            0x82, 0xa1, b'a', 0x93, 0x01, 0xa2, b'b', b'c', 0xc0, 0xa1, b'd', 0xca, 0, 0, 0, 0x40,
        ];
        assert_eq!(msgpack_len(&doc), Some(doc.len()));
        assert_eq!(msgpack_len(&doc[..doc.len() - 1]), None);
    }
}
