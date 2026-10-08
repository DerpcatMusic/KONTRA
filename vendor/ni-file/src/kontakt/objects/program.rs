use std::io::{Cursor, Read};

use crate::{
    Error,
    kontakt::{chunk::Chunk, error::KontaktError, structured_object::StructuredObject},
    read_bytes::ReadBytesExt,
};

use super::zone_list::ZoneList;

const CHUNK_ID: u16 = 0x28;

/// SerType:        0x28
/// Public versions: {0x80, 0x82, 0x90, 0x91, 0x92} and 0xa0..=0xb5.
/// Kontakt 7:      BProgram
/// KontaktIO:      K4PL_Program
#[derive(Debug)]
pub struct Program(pub StructuredObject);

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ProgramPublicParams {
    pub name: String,
    pub num_bytes_samples_total: f64,
    pub transpose: i8,
    pub volume: f32,
    pub pan: f32,
    pub tune: f32,
    pub low_velocity: u8,
    pub high_velocity: u8,
    pub low_key: u8,
    pub high_key: u8,
    pub default_key_switch: i16,
    pub dfd_channel_preload_size: i32,
    pub library_id: i32,
    pub fingerprint: u32,
    pub loading_flags: u32,
    pub group_solo: bool,
    pub cat_icon_idx: i32,
    pub instrument_credits: String,
    pub instrument_author: String,
    pub instrument_url: String,
    pub instrument_cat1: i16,
    pub instrument_cat2: i16,
    pub instrument_cat3: i16,
    pub resource_container_filename: Option<i32>,
    pub wallpaper_filename: Option<i32>,
    /// Opaque public suffix, populated only by the bounded `Program::params` view.
    pub unknown_tail: Vec<u8>,
}

impl ProgramPublicParams {
    pub fn read<R: ReadBytesExt>(mut reader: R, _version: u16) -> Result<Self, Error> {
        Ok(Self {
            name: reader.read_widestring_utf16()?,
            num_bytes_samples_total: reader.read_f64_le()?,
            transpose: reader.read_i8()?,
            volume: reader.read_f32_le()?,
            pan: reader.read_f32_le()?,
            tune: reader.read_f32_le()?,
            low_velocity: reader.read_u8()?,
            high_velocity: reader.read_u8()?,
            low_key: reader.read_u8()?,
            high_key: reader.read_u8()?,
            default_key_switch: reader.read_i16_le()?,
            dfd_channel_preload_size: reader.read_i32_le()?,
            library_id: reader.read_i32_le()?,
            fingerprint: reader.read_u32_le()?,
            loading_flags: reader.read_u32_le()?,
            group_solo: reader.read_bool()?,
            cat_icon_idx: reader.read_i32_le()?,
            instrument_credits: reader.read_widestring_utf16()?,
            instrument_author: reader.read_widestring_utf16()?,
            instrument_url: reader.read_widestring_utf16()?,
            instrument_cat1: reader.read_i16_le()?,
            instrument_cat2: reader.read_i16_le()?,
            instrument_cat3: reader.read_i16_le()?,
            resource_container_filename: { None },
            wallpaper_filename: { None },
            unknown_tail: Vec::new(),
        })
    }
}

/// Complete positional public record. Optional fields are absent from that
/// version's wire layout, rather than normalized empty strings or references.
/// F1/F2 and W strings deliberately retain neutral names; no wallpaper mapping
/// is inferred. The prefix's legacy filename fields remain `None`.
#[derive(Debug)]
pub struct ProgramPublicRecord {
    pub version: u16,
    pub prefix: ProgramPublicParams,
    /// Exact P byte, alongside the legacy prefix's `== 1` boolean interpretation.
    pub group_solo_byte: u8,
    pub resource_container_filename_ref: Option<i32>, // F0
    pub discarded_strings: Option<[String; 2]>,       // a6 D0/D1
    pub tail_string_0: Option<String>,                // W0
    pub tail_string_1: Option<String>,                // W1, unconditional from a8
    pub word_0: Option<u32>,                          // U0
    pub sound_data_0: Option<ProgramSoundData>,       // S0
    pub byte_0: Option<u8>,                           // Q0
    pub sound_data_1: Option<ProgramSoundData>,       // S1
    pub word_1: Option<u32>,                          // U1
    pub bytes_0: Option<Vec<u8>>,                     // B0, not UTF-8
    pub word_2: Option<u32>,                          // U2
    pub filename_ref_1: Option<i32>,                  // F1
    pub terminal_filename_ref: Option<i32>,           // F2, from a2
}

/// Inline flag-0/version-1 SER::BNISoundData. Preserve the nonzero presence byte.
#[derive(Debug)]
pub struct ProgramSoundData {
    pub presence: u8,
    pub body: Option<ProgramSoundDataBody>,
}

#[derive(Debug)]
pub struct ProgramSoundDataBody {
    pub metadata: ProgramSoundMetadata,
    /// Groups1, with group/item versions exactly 1.
    pub groups: Vec<ProgramSoundGroup>,
    pub value_presence: u8,
    /// Exact 16 wire bytes (u32/u16/u16/eight bytes); semantics are unproved.
    pub value: Option<[u8; 16]>,
}

/// Metadata2, retaining every ordered collection, duplicate and empty group.
#[derive(Debug)]
pub struct ProgramSoundMetadata {
    pub leading_words: [u32; 2],
    pub strings: [String; 5],
    pub words: [u32; 7],
    pub string_groups: Vec<Vec<String>>,
    pub string_list: Vec<String>,
    pub pair_list_0: Vec<(String, String)>,
    pub pair_list_1: Vec<(String, String)>,
}

#[derive(Debug)]
pub struct ProgramSoundGroup {
    pub string: String,
    pub items: Vec<ProgramSoundItem>,
}

#[derive(Debug)]
pub struct ProgramSoundItem {
    pub string: String,
    pub float_0: f32,
    pub float_1: f32,
    pub word_0: u32,
    pub word_1: u32,
}

impl ProgramPublicRecord {
    /// The slice bounds all lengths/counts before allocation or iteration.
    /// Uses the existing strict UTF-16 helper; invalid UTF-16 is an error.
    pub fn read(public_data: &[u8], version: u16) -> Result<Self, Error> {
        if !matches!(version, 0x80 | 0x82 | 0x90..=0x92 | 0xa0..=0xb5) {
            return Err(Error::Generic(format!(
                "Unsupported Program public version 0x{version:04x}"
            )));
        }
        // Preflight P's lengths before the legacy reader multiplies UTF-16
        // counts by two (including on 32-bit targets). It remains unchanged.
        let mut check = Cursor::new(public_data);
        let count = public_count(&mut check, 2, "prefix name")?;
        check.set_position(check.position() + count as u64 * 2);
        let mut fixed = [0; 48];
        check.read_exact(&mut fixed)?;
        let group_solo_byte = fixed[43];
        for _ in 0..3 {
            let count = public_count(&mut check, 2, "prefix string")?;
            check.set_position(check.position() + count as u64 * 2);
        }
        check.read_exact(&mut [0; 6])?;

        let mut reader = Cursor::new(public_data);
        let mut record = Self {
            version,
            prefix: ProgramPublicParams::read(&mut reader, version)?,
            group_solo_byte,
            resource_container_filename_ref: None,
            discarded_strings: None,
            tail_string_0: None,
            tail_string_1: None,
            word_0: None,
            sound_data_0: None,
            byte_0: None,
            sound_data_1: None,
            word_1: None,
            bytes_0: None,
            word_2: None,
            filename_ref_1: None,
            terminal_filename_ref: None,
        };
        if version >= 0x91 {
            record.resource_container_filename_ref = Some(reader.read_i32_le()?);
        }
        if version == 0xa6 {
            record.discarded_strings =
                Some([public_string(&mut reader)?, public_string(&mut reader)?]);
        }
        if version >= 0xa6 {
            record.tail_string_0 = Some(public_string(&mut reader)?);
        }
        if version >= 0xa8 {
            record.tail_string_1 = Some(public_string(&mut reader)?);
        }
        if version >= 0xaf {
            record.word_0 = Some(reader.read_u32_le()?);
        }
        if version >= 0xb0 {
            record.sound_data_0 = Some(ProgramSoundData::read(&mut reader)?);
        }
        if version >= 0xb1 {
            record.byte_0 = Some(reader.read_u8()?);
            record.sound_data_1 = Some(ProgramSoundData::read(&mut reader)?);
        }
        if version >= 0xb2 {
            record.word_1 = Some(reader.read_u32_le()?);
        }
        if version >= 0xb3 {
            let count = public_count(&mut reader, 1, "B0 bytes")?;
            record.bytes_0 = Some(reader.read_bytes(count)?);
        }
        if version >= 0xb4 {
            record.word_2 = Some(reader.read_u32_le()?);
        }
        if version >= 0xa6 {
            record.filename_ref_1 = Some(reader.read_i32_le()?);
        }
        if version >= 0xa2 {
            record.terminal_filename_ref = Some(reader.read_i32_le()?);
        }
        let remaining = public_data.len() as u64 - reader.position();
        if remaining != 0 {
            return Err(Error::Generic(format!(
                "Program public version 0x{version:04x}: {remaining} trailing bytes"
            )));
        }
        Ok(record)
    }
}

// Concrete public-section bounds, not an alternative serialization framework.
fn public_count(reader: &mut Cursor<&[u8]>, minimum: u64, field: &str) -> Result<usize, Error> {
    let count = reader.read_u32_le()?;
    let remaining = reader.get_ref().len() as u64 - reader.position();
    if u64::from(count) > remaining / minimum {
        return Err(Error::Generic(format!(
            "Program {field} at offset {}: count {count}, minimum {minimum} bytes each, remaining {remaining}",
            reader.position() - 4
        )));
    }
    Ok(count as usize)
}

fn public_string(reader: &mut Cursor<&[u8]>) -> Result<String, Error> {
    public_count(reader, 2, "UTF-16 string")?;
    reader.set_position(reader.position() - 4);
    Ok(reader.read_widestring_utf16()?)
}

fn public_version(reader: &mut Cursor<&[u8]>, expected: u32) -> Result<(), Error> {
    let got = reader.read_u32_le()?;
    if got != expected {
        return Err(Error::VersionMismatch { expected, got });
    }
    Ok(())
}

impl ProgramSoundData {
    fn read(reader: &mut Cursor<&[u8]>) -> Result<Self, Error> {
        let flag = reader.read_u8()?;
        let version = reader.read_u16_le()?;
        if flag != 0 || version != 1 {
            return Err(Error::Generic(format!(
                "Unsupported inline BNISoundData flag {flag}/version {version}"
            )));
        }
        let presence = reader.read_u8()?;
        let body = if presence == 0 {
            None
        } else {
            let metadata = ProgramSoundMetadata::read(reader)?;
            public_version(reader, 1)?; // Groups1
            let count = public_count(reader, 12, "sound groups")?;
            let mut groups = Vec::new();
            for _ in 0..count {
                public_version(reader, 1)?;
                let count = public_count(reader, 24, "sound group items")?;
                let string = public_string(reader)?;
                let mut items = Vec::new();
                for _ in 0..count {
                    public_version(reader, 1)?;
                    items.push(ProgramSoundItem {
                        string: public_string(reader)?,
                        float_0: reader.read_f32_le()?,
                        float_1: reader.read_f32_le()?,
                        word_0: reader.read_u32_le()?,
                        word_1: reader.read_u32_le()?,
                    });
                }
                groups.push(ProgramSoundGroup { string, items });
            }
            let value_presence = reader.read_u8()?;
            let value = if value_presence == 0 {
                None
            } else {
                let mut bytes = [0; 16];
                reader.read_exact(&mut bytes)?;
                Some(bytes)
            };
            Some(ProgramSoundDataBody {
                metadata,
                groups,
                value_presence,
                value,
            })
        };
        Ok(Self { presence, body })
    }
}

impl ProgramSoundMetadata {
    fn read(reader: &mut Cursor<&[u8]>) -> Result<Self, Error> {
        public_version(reader, 2)?;
        let leading_words = [reader.read_u32_le()?, reader.read_u32_le()?];
        let strings = [
            public_string(reader)?,
            public_string(reader)?,
            public_string(reader)?,
            public_string(reader)?,
            public_string(reader)?,
        ];
        let mut words = [0; 7];
        for word in &mut words {
            *word = reader.read_u32_le()?;
        }
        let count = public_count(reader, 4, "metadata string groups")?;
        let mut string_groups = Vec::new();
        for _ in 0..count {
            let count = public_count(reader, 4, "metadata group strings")?;
            let mut strings = Vec::new();
            for _ in 0..count {
                strings.push(public_string(reader)?);
            }
            string_groups.push(strings);
        }
        let count = public_count(reader, 4, "metadata string list")?;
        let mut string_list = Vec::new();
        for _ in 0..count {
            string_list.push(public_string(reader)?);
        }
        let mut pair_lists = [Vec::new(), Vec::new()];
        for pairs in &mut pair_lists {
            let count = public_count(reader, 8, "metadata pairs")?;
            for _ in 0..count {
                pairs.push((public_string(reader)?, public_string(reader)?));
            }
        }
        let [pair_list_0, pair_list_1] = pair_lists;
        Ok(Self {
            leading_words,
            strings,
            words,
            string_groups,
            string_list,
            pair_list_0,
            pair_list_1,
        })
    }
}

impl Program {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        Ok(Self(StructuredObject::read(&mut reader)?))
    }

    pub fn version(&self) -> u16 {
        self.0.version
    }

    /// Common prefix plus the bounded opaque suffix. Use `public_record` for
    /// the verified versioned suffix layout; no filename aliases are inferred.
    pub fn params(&self) -> Result<ProgramPublicParams, Error> {
        let mut reader = Cursor::new(&self.0.public_data);
        let mut params = ProgramPublicParams::read(&mut reader, self.0.version)?;
        params.unknown_tail = reader.read_all()?;
        Ok(params)
    }

    /// Decode the entire supported public section without modifying its raw bytes.
    /// Unknown versions, malformed bodies and trailing bytes are errors; `self.0`
    /// still owns the complete raw public/private sections and children.
    pub fn public_record(&self) -> Result<ProgramPublicRecord, Error> {
        ProgramPublicRecord::read(&self.0.public_data, self.version())
    }

    pub fn zone_list(&self) -> Option<Result<ZoneList, Error>> {
        self.0.find_first(0x34).map(ZoneList::try_from)
    }

    pub fn children(&self) -> &Vec<Chunk> {
        &self.0.children
    }

    // 0x32 VoiceGroups
    // 0x33 GroupList
}

impl std::convert::TryFrom<&Chunk> for Program {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        if chunk.id != CHUNK_ID {
            return Err(KontaktError::IncorrectID {
                expected: CHUNK_ID,
                got: chunk.id,
            }
            .into());
        }
        let reader = Cursor::new(&chunk.data);
        Self::read(reader)
    }
}

#[derive(Debug, Default)]
pub struct ProgramDataPrivateParams {}

impl ProgramDataPrivateParams {
    /// Private parameter fields, filenames and nested arrays are not fully mapped.
    /// This reader does not consume or validate the body. Use `Program.0.private_data`
    /// to retain it until a complete typed layout is available.
    pub fn read<R: ReadBytesExt>(_reader: R, version: u16) -> Result<Self, Error> {
        Err(Error::Generic(format!(
            "Program private parameters (version 0x{version:04x}) are unsupported; retain Program.0.private_data"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Authored positional records: no vendor presets or native writer dependency.
    #[derive(Default)]
    struct PublicFixture {
        bytes: Vec<u8>,
        versions: Vec<usize>,
        counts: Vec<usize>,
        inline_headers: Vec<usize>,
    }

    impl PublicFixture {
        fn word(&mut self, value: u32) -> usize {
            let at = self.bytes.len();
            self.bytes.extend(value.to_le_bytes());
            at
        }

        fn count(&mut self, value: u32) {
            let at = self.word(value);
            self.counts.push(at);
        }

        fn nested_version(&mut self, value: u32) {
            let at = self.word(value);
            self.versions.push(at);
        }

        fn wide(&mut self, value: &str) {
            let units: Vec<_> = value.encode_utf16().collect();
            self.count(units.len() as u32);
            for unit in units {
                self.bytes.extend(unit.to_le_bytes());
            }
        }

        fn sound(&mut self, presence: u8, rich: bool, metadata_groups: u32) {
            self.inline_headers.push(self.bytes.len());
            self.bytes.extend([0, 1, 0, presence]);
            if presence == 0 {
                return;
            }
            self.nested_version(2);
            self.word(0xdeadbeef);
            self.word(0x80000001);
            for string in ["m0", "m1", "m2", "m3", "m4"] {
                self.wide(if rich { string } else { "" });
            }
            for word in 0..7 {
                self.word(0x80000000 | word);
            }
            self.count(metadata_groups);
            for group in 0..metadata_groups {
                self.count(if rich && group == 0 { 4 } else { 0 });
                if rich && group == 0 {
                    for string in ["first", "duplicate", "duplicate", "fourth 🦀"] {
                        self.wide(string);
                    }
                }
            }
            self.count(if rich { 2 } else { 0 });
            if rich {
                self.wide("list");
                self.wide("list");
            }
            for (count, first, second) in [(2, "key", "value"), (1, "", "second")] {
                self.count(if rich { count } else { 0 });
                if rich {
                    for _ in 0..count {
                        self.wide(first);
                        self.wide(second);
                    }
                }
            }
            self.nested_version(1); // Groups1
            self.count(if rich { 2 } else { 0 });
            if rich {
                for (name, items) in [("group", 2), ("empty", 0)] {
                    self.nested_version(1);
                    self.count(items);
                    self.wide(name);
                    for _ in 0..items {
                        self.nested_version(1);
                        self.wide("item 🦀");
                        self.word(0x7fc01234); // NaN payload, no arithmetic/normalization.
                        self.word(0x80000000); // Negative zero.
                        self.word(0xfedcba98);
                        self.word(0x76543210);
                    }
                }
            }
            let value_presence = if !rich {
                0
            } else if presence == 2 {
                255
            } else {
                2
            };
            self.bytes.push(value_presence);
            if value_presence != 0 {
                self.bytes.extend(0u8..16);
            }
        }
    }

    fn public_fixture(version: u16, rich: bool) -> PublicFixture {
        let mut f = PublicFixture::default();
        f.wide(if rich { "Program 🦀" } else { "" });
        f.bytes.extend(0x7ff8000000001234u64.to_le_bytes());
        f.bytes.push(0x80); // Transpose -128.
        for bits in [0x7fc05678, 0x80000000, 0x7f800000] {
            f.word(bits);
        }
        f.bytes.extend([1, 127, 2, 126]);
        f.bytes.extend((-2i16).to_le_bytes());
        for word in [0xffffffff, 0x80000000, 0xdeadbeef, 0xfedcba98] {
            f.word(word);
        }
        f.bytes.push(255); // Legacy bool remains false; complete view preserves this byte.
        f.word(0x80000001);
        for string in ["credits", "author", "url"] {
            f.wide(if rich { string } else { "" });
        }
        for value in [-3i16, 4, -5] {
            f.bytes.extend(value.to_le_bytes());
        }
        if version >= 0x91 {
            f.word((-7i32) as u32);
        }
        if version == 0xa6 {
            f.wide("discarded 0");
            f.wide("discarded 1 🦀");
        }
        if version >= 0xa6 {
            f.wide(if rich { "W0" } else { "" });
        }
        if version >= 0xa8 {
            f.wide(if rich { "W1 🦀" } else { "" });
        }
        if version >= 0xaf {
            f.word(0xdeadbeef);
        }
        if version >= 0xb0 {
            f.sound(if rich { 2 } else { 0 }, rich, if rich { 2 } else { 0 });
        }
        if version >= 0xb1 {
            f.bytes.push(255);
            f.sound(if rich { 255 } else { 0 }, rich, if rich { 1 } else { 0 });
        }
        if version >= 0xb2 {
            f.word(0x80000001);
        }
        if version >= 0xb3 {
            f.count(if rich { 3 } else { 0 });
            if rich {
                f.bytes.extend([0xff, 0, 0x80]);
            }
        }
        if version >= 0xb4 {
            f.word(0xfedcba98);
        }
        if version >= 0xa6 {
            f.word(17);
        }
        if version >= 0xa2 {
            f.word((-123i32) as u32);
        }
        f
    }

    fn raw_program(version: u16, public_data: Vec<u8>) -> Program {
        Program(StructuredObject {
            version,
            public_data,
            private_data: vec![0xff, 0x80],
            children: Vec::new(),
        })
    }

    #[test]
    fn reader_regression_public_versions_and_tail_order() {
        let versions: Vec<_> = [0x80, 0x82, 0x90, 0x91, 0x92]
            .into_iter()
            .chain(0xa0..=0xb5)
            .collect();
        for version in versions {
            for rich in [false, true] {
                let f = public_fixture(version, rich);
                let program = raw_program(version, f.bytes.clone());
                let record = program.public_record().unwrap();
                assert_eq!(record.version, version);
                assert_eq!(record.group_solo_byte, 255);
                assert!(!record.prefix.group_solo);
                assert_eq!(record.prefix.name, if rich { "Program 🦀" } else { "" });
                assert_eq!(
                    record.prefix.num_bytes_samples_total.to_bits(),
                    0x7ff8000000001234
                );
                assert_eq!(record.prefix.volume.to_bits(), 0x7fc05678);
                assert_eq!(record.prefix.pan.to_bits(), 0x80000000);
                assert_eq!(record.prefix.tune.to_bits(), 0x7f800000);
                assert_eq!(record.prefix.transpose, -128);
                assert_eq!(record.prefix.dfd_channel_preload_size, -1);
                assert_eq!(record.prefix.library_id, i32::MIN);
                assert_eq!(record.prefix.fingerprint, 0xdeadbeef);
                assert_eq!(record.prefix.loading_flags, 0xfedcba98);
                assert_eq!(record.prefix.cat_icon_idx, -2147483647);
                assert_eq!(
                    (
                        record.prefix.instrument_cat1,
                        record.prefix.instrument_cat2,
                        record.prefix.instrument_cat3
                    ),
                    (-3, 4, -5)
                );
                assert_eq!(
                    record.prefix.instrument_credits,
                    if rich { "credits" } else { "" }
                );
                assert_eq!(
                    record.prefix.instrument_author,
                    if rich { "author" } else { "" }
                );
                assert_eq!(record.prefix.instrument_url, if rich { "url" } else { "" });
                assert_eq!(record.prefix.resource_container_filename, None);
                assert_eq!(record.prefix.wallpaper_filename, None);
                assert_eq!(
                    record.resource_container_filename_ref,
                    (version >= 0x91).then_some(-7)
                );
                assert_eq!(record.filename_ref_1, (version >= 0xa6).then_some(17));
                assert_eq!(
                    record.terminal_filename_ref,
                    (version >= 0xa2).then_some(-123)
                );
                assert_eq!(
                    record
                        .discarded_strings
                        .as_ref()
                        .map(|s| [s[0].as_str(), s[1].as_str()]),
                    (version == 0xa6).then_some(["discarded 0", "discarded 1 🦀"])
                );
                assert_eq!(
                    record.tail_string_0.as_deref(),
                    (version >= 0xa6).then_some(if rich { "W0" } else { "" })
                );
                assert_eq!(
                    record.tail_string_1.as_deref(),
                    (version >= 0xa8).then_some(if rich { "W1 🦀" } else { "" })
                );
                assert_eq!(record.word_0, (version >= 0xaf).then_some(0xdeadbeef));
                assert_eq!(record.byte_0, (version >= 0xb1).then_some(255));
                assert_eq!(record.word_1, (version >= 0xb2).then_some(0x80000001));
                assert_eq!(record.word_2, (version >= 0xb4).then_some(0xfedcba98));
                assert_eq!(
                    record.bytes_0.as_deref(),
                    (version >= 0xb3).then_some(if rich { &[0xff, 0, 0x80][..] } else { &[][..] })
                );
                for (sound, present, presence, metadata_groups, value_presence) in [
                    (&record.sound_data_0, version >= 0xb0, 2, 2, 255),
                    (&record.sound_data_1, version >= 0xb1, 255, 1, 2),
                ] {
                    assert_eq!(sound.is_some(), present);
                    if let Some(sound) = sound {
                        assert_eq!(sound.presence, if rich { presence } else { 0 });
                        assert_eq!(sound.body.is_some(), rich);
                        if let Some(body) = &sound.body {
                            assert_eq!(body.metadata.leading_words, [0xdeadbeef, 0x80000001]);
                            assert_eq!(body.metadata.strings, ["m0", "m1", "m2", "m3", "m4"]);
                            assert_eq!(
                                body.metadata.words,
                                [
                                    0x80000000, 0x80000001, 0x80000002, 0x80000003, 0x80000004,
                                    0x80000005, 0x80000006
                                ]
                            );
                            assert_eq!(body.metadata.string_groups.len(), metadata_groups);
                            assert_eq!(
                                body.metadata.string_groups[0],
                                ["first", "duplicate", "duplicate", "fourth 🦀"]
                            );
                            if metadata_groups == 2 {
                                assert!(body.metadata.string_groups[1].is_empty());
                            }
                            assert_eq!(body.metadata.string_list, ["list", "list"]);
                            assert_eq!(
                                body.metadata.pair_list_0,
                                vec![(String::from("key"), String::from("value")); 2]
                            );
                            assert_eq!(
                                body.metadata.pair_list_1,
                                vec![(String::new(), String::from("second"))]
                            );
                            assert_eq!(body.groups.len(), 2);
                            assert_eq!(body.groups[0].string, "group");
                            assert_eq!(body.groups[0].items.len(), 2);
                            assert_eq!(body.groups[1].string, "empty");
                            assert!(body.groups[1].items.is_empty());
                            for item in &body.groups[0].items {
                                assert_eq!(item.string, "item 🦀");
                                assert_eq!(item.float_0.to_bits(), 0x7fc01234);
                                assert_eq!(item.float_1.to_bits(), 0x80000000);
                                assert_eq!((item.word_0, item.word_1), (0xfedcba98, 0x76543210));
                            }
                            assert_eq!(body.value_presence, value_presence);
                            assert_eq!(body.value, Some(std::array::from_fn(|i| i as u8)));
                        }
                    }
                }
                assert_eq!(program.0.public_data, f.bytes);
                assert_eq!(program.0.private_data, [0xff, 0x80]);
                assert_eq!(program.params().unwrap().name, record.prefix.name);
                let mut extra = f.bytes;
                extra.push(0);
                assert!(
                    raw_program(version, extra).public_record().is_err(),
                    "trailing byte 0x{version:x}"
                );
            }
        }
        // Every F0/F1/F2 signed index, including distinct negative sentinels and 0.
        let fixture = public_fixture(0xa2, false);
        for index in [i32::MIN, -123, -1, 0, 17, i32::MAX] {
            let mut bytes = fixture.bytes.clone();
            bytes[70..74].copy_from_slice(&index.to_le_bytes());
            bytes[74..78].copy_from_slice(&index.to_le_bytes());
            let r = ProgramPublicRecord::read(&bytes, 0xa2).unwrap();
            assert_eq!(
                (r.resource_container_filename_ref, r.terminal_filename_ref),
                (Some(index), Some(index))
            );
            let mut bytes = public_fixture(0xa7, false).bytes;
            bytes[78..82].copy_from_slice(&index.to_le_bytes());
            assert_eq!(
                ProgramPublicRecord::read(&bytes, 0xa7)
                    .unwrap()
                    .filename_ref_1,
                Some(index)
            );
        }
    }

    #[test]
    fn reader_regression_public_minimal_present_sound_body() {
        let mut f = public_fixture(0xb0, false);
        let mut sound = PublicFixture::default();
        sound.sound(1, false, 0);
        assert_eq!(sound.bytes.len(), 89); // Header + Metadata2(76) + Groups1(8) + value flag.
        let at = f.inline_headers[0];
        f.bytes.splice(at..at + 4, sound.bytes);
        let r = ProgramPublicRecord::read(&f.bytes, 0xb0).unwrap();
        let sound = r.sound_data_0.unwrap();
        assert_eq!(sound.presence, 1);
        let body = sound.body.unwrap();
        assert!(body.metadata.string_groups.is_empty());
        assert!(body.metadata.string_list.is_empty());
        assert!(body.metadata.pair_list_0.is_empty());
        assert!(body.metadata.pair_list_1.is_empty());
        assert!(body.groups.is_empty());
        assert_eq!((body.value_presence, body.value), (0, None));
    }

    #[test]
    fn reader_regression_public_rejects_truncation_versions_counts_and_preserves_raw() {
        let fixture = public_fixture(0xb5, true);
        for end in 0..fixture.bytes.len() {
            let program = raw_program(0xb5, fixture.bytes[..end].to_vec());
            assert!(program.public_record().is_err(), "truncation {end}");
            assert_eq!(program.0.public_data, fixture.bytes[..end]);
        }
        for version in 0..=0xffff {
            if matches!(version, 0x80 | 0x82 | 0x90..=0x92 | 0xa0..=0xb5) {
                continue;
            }
            let error = ProgramPublicRecord::read(&[], version).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("Unsupported Program public version")
            );
        }
        let mut bad_records = Vec::new();
        for at in &fixture.versions {
            for value in [0u32, 3, u32::MAX] {
                let mut bad = fixture.bytes.clone();
                bad[*at..*at + 4].copy_from_slice(&value.to_le_bytes());
                bad_records.push(bad);
            }
        }
        for at in &fixture.counts {
            let mut bad = fixture.bytes.clone();
            bad[*at..*at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            bad_records.push(bad);
        }
        for at in &fixture.inline_headers {
            for (flag, version) in [(1, 1u16), (2, 1), (255, 1), (0, 0), (0, 2), (0, 0xffff)] {
                let mut bad = fixture.bytes.clone();
                bad[*at] = flag;
                bad[*at + 1..*at + 3].copy_from_slice(&version.to_le_bytes());
                bad_records.push(bad);
            }
        }
        // Invalid UTF-16 in P and W1: fail typed decoding without replacing units.
        for at in [4, fixture.counts[5] + 4] {
            let mut bad = fixture.bytes.clone();
            bad[at..at + 2].copy_from_slice(&0xd800u16.to_le_bytes());
            bad_records.push(bad);
        }
        for bytes in bad_records {
            let mut body = vec![1, 0xb5, 0];
            body.extend(2u32.to_le_bytes());
            body.extend([0xff, 0x80]);
            body.extend((bytes.len() as u32).to_le_bytes());
            body.extend(&bytes);
            body.extend(0u32.to_le_bytes());
            let original = Chunk {
                id: 0x28,
                data: body,
            };
            let program = Program::try_from(&original).unwrap();
            assert!(program.public_record().is_err());
            assert_eq!(program.0.public_data, bytes);
            assert_eq!(program.0.private_data, [0xff, 0x80]);
            let mut raw = Vec::new();
            original.write(&mut raw).unwrap();
            let reread = Chunk::read(Cursor::new(&raw)).unwrap();
            let mut rewritten = Vec::new();
            reread.write(&mut rewritten).unwrap();
            assert_eq!(rewritten, raw);
        }
        // The old frontend API intentionally still accepts unknown versions and suffixes.
        let mut program = raw_program(0x81, fixture.bytes);
        assert!(program.public_record().is_err());
        assert_eq!(program.params().unwrap().name, "Program 🦀");
        program.0.public_data.push(0xff);
        assert!(program.params().is_ok());
    }

    #[test]
    fn reader_regression_private_params_remain_opaque() {
        let mut filename = vec![0; 80];
        filename[61..65].copy_from_slice(&1i32.to_le_bytes());
        for version in [0x80, 0x81, 0xaf, 0xffff] {
            // Truncation, unsupported inner versions, old empty successes and filename panics all fail.
            for data in [
                vec![],
                vec![0],
                vec![0; 4],
                2u32.to_le_bytes().to_vec(),
                vec![0; 80],
                filename.clone(),
            ] {
                let mut reader = Cursor::new(&data);
                let error = ProgramDataPrivateParams::read(&mut reader, version).unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains(&format!("version 0x{version:04x}"))
                );
                assert!(error.to_string().contains("unsupported"));
                assert_eq!(
                    reader.position(),
                    0,
                    "opaque private data must not be partially consumed"
                );
            }
        }

        // Program framing is still supported, including opaque private data and children.
        let private = vec![0xff; 80];
        let voice_groups = Chunk {
            id: 0x32,
            data: vec![0, 0x60, 0, 0xff],
        };
        let mut children = Vec::new();
        voice_groups.write(&mut children).unwrap();
        let mut body = vec![1, 0x80, 0];
        body.extend((private.len() as u32).to_le_bytes());
        body.extend(&private);
        body.extend(0u32.to_le_bytes()); // public data
        body.extend((children.len() as u32).to_le_bytes());
        body.extend(&children);
        let program = Program::read(Cursor::new(body)).unwrap();
        assert_eq!(program.version(), 0x80);
        assert!(
            ProgramDataPrivateParams::read(Cursor::new(&program.0.private_data), program.version())
                .is_err()
        );
        assert_eq!(program.0.private_data, private);
        let raw = &program.children()[0];
        assert!(raw.into_object().is_err());
        let mut preserved = Vec::new();
        raw.write(&mut preserved).unwrap();
        assert_eq!(preserved, children);
    }
}
