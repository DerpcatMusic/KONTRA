// Groups allow you to apply settings like effects, volume, panning, etc. to multiple samples at once rather than having to adjust each one individually.

use std::io::{Cursor, Write};

use crate::{
    Error,
    kontakt::{StructuredObject, objects::start_criteria_list::StartCriteriaList},
    read_bytes::ReadBytesExt,
};

/// Type:           Chunk
/// SerType:        0x04
/// Kontakt 7:      BGroup?
/// KontaktIO:      K4PL\_Group
#[derive(Debug)]
pub struct Group(pub StructuredObject);

/// Bounded source serialization identity, not decoded playback parameters.
/// The preceding flag does not indicate absence: valid records follow zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceIdentity {
    pub flag: u8,
    pub structured: bool,
    pub version: u16,
    pub mode: u32,
}

/// Native unstructured v0x106, mode-9 source record. The fixed common prefix
/// and final nested state stay opaque. Enum fields retain their serialized
/// IDs, rather than assuming they equal KSP constants or menu positions.
/// Field order is verified against the native source reader and writer.
#[derive(Debug, Clone, PartialEq)]
pub struct WavetableSource {
    pub common: [u8; 30],
    pub position: f32,
    pub form1: f32,
    pub phase: f32,
    pub phase_random: f32,
    pub form_type: u32,
    pub quality: u32,
    pub inharmonic_enabled: bool,
    pub inharmonic: f32,
    pub form2: f32,
    pub form2_type: u32,
    pub mod_wave: u32,
    pub mod_type: u32,
    pub mod_amount: f32,
    pub mod_tune: f32,
    pub unknown_tail: [u8; 16],
}

impl WavetableSource {
    pub fn read(mut reader: impl ReadBytesExt) -> Result<Self, Error> {
        let mut common = [0; 30];
        reader.read_exact(&mut common)?;
        let result = Self {
            common,
            position: reader.read_f32_le()?,
            form1: reader.read_f32_le()?,
            phase: reader.read_f32_le()?,
            phase_random: reader.read_f32_le()?,
            form_type: reader.read_u32_le()?,
            quality: reader.read_u32_le()?,
            inharmonic_enabled: match reader.read_u8()? {
                0 => false, 1 => true,
                _ => return Err(Error::Static("Invalid wavetable inharmonic flag")),
            },
            inharmonic: reader.read_f32_le()?,
            form2: reader.read_f32_le()?,
            form2_type: reader.read_u32_le()?,
            mod_wave: reader.read_u32_le()?,
            mod_type: reader.read_u32_le()?,
            mod_amount: reader.read_f32_le()?,
            mod_tune: reader.read_f32_le()?,
            unknown_tail: { let mut tail = [0; 16]; reader.read_exact(&mut tail)?; tail },
        };
        result.validate()?;
        Ok(result)
    }

    fn validate(&self) -> Result<(), Error> {
        if self.common[..7] != [0, 6, 1, 9, 0, 0, 0]
            || !(1..=34).contains(&self.form_type)
            || !(1..=34).contains(&self.form2_type)
            || !(1..=4).contains(&self.quality)
            || self.mod_wave > 9 || self.mod_type > 12
        {
            return Err(Error::Static("Unsupported or malformed v0x106 wavetable source record"));
        }
        Ok(())
    }

    /// Write the same bounded record, including all unmodeled common/nested
    /// bytes. This edits source metadata; it does not implement native DSP.
    pub fn write(&self, mut writer: impl Write) -> Result<(), Error> {
        self.validate()?;
        writer.write_all(&self.common)?;
        for value in [self.position, self.form1, self.phase, self.phase_random] {
            writer.write_all(&value.to_le_bytes())?;
        }
        for value in [self.form_type, self.quality] { writer.write_all(&value.to_le_bytes())?; }
        writer.write_all(&[u8::from(self.inharmonic_enabled)])?;
        for value in [self.inharmonic, self.form2] { writer.write_all(&value.to_le_bytes())?; }
        for value in [self.form2_type, self.mod_wave, self.mod_type] { writer.write_all(&value.to_le_bytes())?; }
        for value in [self.mod_amount, self.mod_tune] { writer.write_all(&value.to_le_bytes())?; }
        writer.write_all(&self.unknown_tail)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupParams {
    pub name: String,
    /// Linear amplitude ratio (0.5 is -6 dB).
    pub volume: f32,
    pub pan: f32,
    /// Linear pitch ratio (0.5 is one octave down), not semitones.
    pub tune: f32,
    /// Repitch samples to midi note triggered.
    pub key_tracking: bool,
    /// Play samples in reverse.
    pub reverse: bool,
    pub release_trigger: bool,
    pub release_trigger_note_monophonic: bool,
    pub rls_trig_counter: i32,
    pub midi_channel: i16,
    pub voice_group_index: i32,
    pub fx_idx_amp_split_point: i32,
    pub muted: bool,
    pub soloed: bool,
    pub interp_quality: i32,
    // Children: InternalModArray16 0x3B, ExternalModArray32 0x3C,
    // BParGroupDynamics 0x4A (256 zero bytes in every local preset).
    pub start_criteria: StartCriteriaList,
    pub unknown_tail: Vec<u8>,
}

/// Group private data: 136 records of `(u32 8, u32 flags, u32 0)`, then 24
/// bytes of unknown state, then the group insert effect rack. Fixed in all
/// 224,868 local groups (Kontakt 5 to 7).
const PRIVATE_RECORDS: usize = 136;
const PRIVATE_TRAILER: usize = 24;

impl Group {
    /// The group insert effect rack (8 slots), stored in the private data.
    pub fn insert_fx(&self) -> Result<super::BParamArrayBParFX8, Error> {
        super::BParamArrayBParFX8::read(self.private_rack_reader()?, 8)
    }

    fn private_rack_reader(&self) -> Result<Cursor<&[u8]>, Error> {
        let data = &self.0.private_data;
        let records = PRIVATE_RECORDS * 12;
        if data.len() < records + PRIVATE_TRAILER
            || data[..records].chunks(12).any(|r| r[..4] != [8, 0, 0, 0])
        {
            return Err(Error::Static("Unrecognized group private data"));
        }
        Ok(Cursor::new(&data[records + PRIVATE_TRAILER..]))
    }

    /// The instrument bus (0-based) the group is routed to, or `None` for the
    /// default route. Read from the fixed tail of the private data: every one of
    /// 195 sampled local instruments stores 0..=15 or 255 there, Una Corda
    /// routes its tone groups to bus 0 and its noise groups to bus 1 (its script
    /// toggles those buses' racks), Areia its five mic positions to 0..=4.
    pub fn bus_route(&self) -> Option<u8> {
        let data = &self.0.private_data;
        let at = data.len().checked_sub(20)?;
        data.get(at).copied().filter(|&bus| bus < 16)
    }

    fn source_reader(&self) -> Result<(Cursor<&[u8]>, u8), Error> {
        let mut reader = self.private_rack_reader()?;
        super::BParamArrayBParFX8::read(&mut reader, 8)?;
        let flag = reader.read_u8()?;
        if flag > 1 { return Err(Error::Static("Invalid group source flag")); }
        Ok((reader, flag))
    }

    /// Full versioned source parameters, followed by retained private state.
    pub fn source_params(&self) -> Result<(super::BParSrcMode, &[u8]), Error> {
        let (mut reader, _) = self.source_reader()?;
        let params = super::BParSrcMode::read(&mut reader)?;
        let tail = &reader.get_ref()[reader.position() as usize..];
        Ok((params, tail))
    }

    /// Read only the seven-byte identity whose location is verified in legacy
    /// v0x102/0x103/0x104 groups and Kontakt 8 v0x106 groups. Their remaining private bytes
    /// remain opaque; this does not establish their source-record length or
    /// implement wavetable/time-stretch playback. Snapshot state stays strict.
    pub fn source_identity(&self) -> Result<SourceIdentity, Error> {
        let (mut reader, flag) = self.source_reader()?;
        if reader.read_u8()? != 0 {
            return Err(Error::Static("Unsupported structured group source identity"));
        }
        let version = reader.read_u16_le()?;
        if !matches!(version, 0x100..=0x106) {
            return Err(Error::Generic(format!("Unsupported group source identity version 0x{version:x}")));
        }
        Ok(SourceIdentity { flag, structured: false, version, mode: reader.read_u32_le()? })
    }

    /// Opaque v0x102 source state after the private insert rack. Exposing its
    /// serialization header/mode permits snapshot compatibility checks without
    /// interpreting the remaining source parameters.
    pub fn source_state(&self) -> Result<[u8; 32], Error> {
        let (mut reader, _) = self.source_reader()?;
        let mut state = [0; 32];
        std::io::Read::read_exact(&mut reader, &mut state)?;
        if state[..3] != [0, 2, 1] {
            return Err(Error::Static("Unsupported group source state version"));
        }
        Ok(state)
    }

    pub fn wavetable_source(&self) -> Result<Option<WavetableSource>, Error> {
        if self.source_identity()?.mode != 9 { return Ok(None); }
        WavetableSource::read(self.source_reader()?.0).map(Some)
    }

    /// Replace only the existing mode-9 record; retain the source flag,
    /// private rack, and the group-private trailer after the source.
    pub fn set_wavetable_source(&mut self, source: &WavetableSource) -> Result<(), Error> {
        let (mut reader, _) = self.source_reader()?;
        let start = self.0.private_data.len() - reader.get_ref().len() + reader.position() as usize;
        WavetableSource::read(&mut reader)?;
        let mut bytes = [0; 99];
        source.write(Cursor::new(bytes.as_mut_slice()))?;
        self.0.private_data[start..start + bytes.len()].copy_from_slice(&bytes);
        Ok(())
    }

    pub fn params(&self) -> Result<GroupParams, Error> {
        let mut reader = Cursor::new(&self.0.public_data);

        Ok(GroupParams {
            name: reader.read_widestring_utf16()?,
            volume: reader.read_f32_le()?,
            pan: reader.read_f32_le()?,
            tune: reader.read_f32_le()?,
            key_tracking: reader.read_bool()?,
            reverse: reader.read_bool()?,
            release_trigger: reader.read_bool()?,
            release_trigger_note_monophonic: reader.read_bool()?,
            rls_trig_counter: reader.read_i32_le()?,
            midi_channel: reader.read_i16_le()?,
            voice_group_index: reader.read_i32_le()?,
            fx_idx_amp_split_point: reader.read_i32_le()?,
            muted: reader.read_bool()?,
            soloed: reader.read_bool()?,
            interp_quality: reader.read_i32_le()?,
            start_criteria: self
                .0
                .find_first(0x38)
                .ok_or(Error::Static("Group has no start criteria list"))?
                .try_into()?,
            unknown_tail: reader.read_all()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wavetable_source_roundtrip_edits_preserve_opaque_state_and_bounds() {
        let mut common = [0xA5; 30];
        common[..7].copy_from_slice(&[0, 6, 1, 9, 0, 0, 0]);
        let source = WavetableSource { common, position: 0.25, form1: 0.5,
            phase: 0.125, phase_random: 0.75, form_type: 17, quality: 3,
            inharmonic_enabled: false, inharmonic: 0.375, form2: 0.625,
            form2_type: 1, mod_wave: 6, mod_type: 0, mod_amount: -12.0,
            mod_tune: f32::from_bits(0x7fc01234), unknown_tail: [0xDE; 16] };
        let mut bytes = Vec::new();
        source.write(&mut bytes).unwrap();
        assert_eq!(bytes.len(), 99);
        let parsed = WavetableSource::read(Cursor::new(&bytes)).unwrap();
        let mut roundtrip = Vec::new();
        parsed.write(&mut roundtrip).unwrap();
        assert_eq!(roundtrip, bytes);
        for end in 0..99 { assert!(WavetableSource::read(Cursor::new(&bytes[..end])).is_err()); }
        for (offset, value) in [(1, 5), (54, 2), (46, 0), (50, 5), (67, 10), (71, 13)] {
            let mut invalid = bytes.clone(); invalid[offset] = value;
            assert!(WavetableSource::read(Cursor::new(invalid)).is_err());
        }
        let mut private = Vec::new();
        for _ in 0..136 { private.extend(8u32.to_le_bytes()); private.extend([0; 8]); }
        private.extend([0; 24]);
        private.extend([0, 0x13, 0]); private.extend(8u32.to_le_bytes()); private.extend([0; 8]);
        private.push(1);
        let start = private.len();
        private.extend(&bytes); private.extend([0xFE; 22]);
        let mut group = Group(StructuredObject { version: 1, private_data: private,
            public_data: Vec::new(), children: Vec::new() });
        let before = group.0.private_data.clone();
        let mut edited = group.wavetable_source().unwrap().unwrap();
        edited.position = 0.75;
        group.set_wavetable_source(&edited).unwrap();
        assert_eq!(group.wavetable_source().unwrap().unwrap().position, 0.75);
        assert_eq!(&group.0.private_data[..start + 30], &before[..start + 30]);
        assert_eq!(&group.0.private_data[start + 34..], &before[start + 34..]);
        edited.quality = 5;
        let before = group.0.private_data.clone();
        assert!(group.set_wavetable_source(&edited).is_err());
        assert_eq!(group.0.private_data, before);
    }

    #[test]
    fn source_identity_reads_known_headers_without_interpreting_opaque_state() {
        let mut private = Vec::new();
        for _ in 0..136 { private.extend(8u32.to_le_bytes()); private.extend([0; 8]); }
        private.extend([0; 24]);
        private.extend([0, 0x13, 0]);
        private.extend(8u32.to_le_bytes());
        private.extend([0; 8]);
        private.push(0); // Native source-presence flag.
        let source = private.len();
        private.extend([0; 7]);
        private.extend([0xde, 0xad]); // Unknown remaining source fields.
        let mut group = Group(StructuredObject { version: 1, private_data: private,
            public_data: Vec::new(), children: Vec::new() });
        for version in 0x100u16..=0x106 {
            for mode in [0u32, 3, 9] {
                group.0.private_data[source + 1..source + 3].copy_from_slice(&version.to_le_bytes());
                group.0.private_data[source + 3..source + 7].copy_from_slice(&mode.to_le_bytes());
                let original = group.0.private_data.clone();
                let identity = group.source_identity().unwrap();
                assert_eq!((identity.version, identity.mode), (version, mode));
                assert!(!identity.structured);
                assert_eq!(group.0.private_data, original);
            }
        }
        group.0.private_data.truncate(source + 6);
        assert!(group.source_identity().is_err());
        group.0.private_data.resize(source + 7, 0);
        group.0.private_data[source + 1..source + 3].copy_from_slice(&0x107u16.to_le_bytes());
        assert!(group.source_identity().is_err());
        group.0.private_data[source + 1..source + 3].copy_from_slice(&0x104u16.to_le_bytes());
        group.0.private_data[source] = 1;
        assert!(group.source_identity().is_err());
    }
}
