use crate::{read_bytes::ReadBytesExt, Error};

use super::{
    native_schema,
    objects::{
        snapshot_metadata_names, BParFX, BParFXRotator, BParFXSendLevels, BParScript,
        BParamArrayBParFX8, Bank, EffectParameters, EnvelopeAhdsr, EnvelopeDbd, EnvelopeFlex,
        EnvelopeFollower, ExternalMod, ExternalModArray32, FNTableImpl, FXDelay,
        FileNameListPreK51, Glide, GroupDynamics, GroupList, InsertBus, InternalMod,
        InternalModArray16, Lfo, LoopArray, MidiPortMaps, MultiSlots, OutputConfiguration,
        PrivateRawObject, Program, ProgramContainer, ProgramList, QuickBrowseData, SaveSettings,
        SlotList, Snapshot, StartCriteria, StartCriteriaList, VoiceGroup, VoiceGroups, ZoneList,
    },
    structured_object::StructuredObject,
};

#[derive(Debug)]
pub struct Chunk {
    pub id: u16,
    pub data: Vec<u8>,
}

impl Chunk {
    /// Write the raw chunk without interpreting or discarding unknown bytes.
    pub fn write<W: std::io::Write>(&self, mut writer: W) -> Result<(), Error> {
        let length =
            u32::try_from(self.data.len()).map_err(|_| Error::Static("Kontakt chunk too large"))?;
        writer.write_all(&self.id.to_le_bytes())?;
        writer.write_all(&length.to_le_bytes())?;
        writer.write_all(&self.data)?;
        Ok(())
    }

    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        let at = reader.stream_position()?;
        let id = reader
            .read_u16_le()
            .map_err(|e| Error::context(format!("Kontakt chunk ID at offset {at}"), e))?;
        let length = reader.read_u32_le().map_err(|e| {
            Error::context(
                format!("Kontakt chunk 0x{id:04x} length at offset {}", at + 2),
                e,
            )
        })? as usize;
        let data = reader.read_bytes(length).map_err(|e| {
            Error::context(
                format!(
                    "Kontakt chunk 0x{id:04x} body at offset {}, declared length {length}",
                    at + 6
                ),
                e,
            )
        })?;
        Ok(Self { id, data })
    }

    pub fn into_object(&self) -> Result<KontaktObject, Error> {
        KontaktObject::try_from(self)
    }
}

#[derive(Debug)]
pub enum KontaktObject {
    BParModBase,
    BAutomationObject,
    OutputPartition,
    Bank(Bank),
    BGroup,
    BLoop,
    BParScript(BParScript),
    /// Wrapper whose only child is the concrete modulator.
    BParEnv(StructuredObject),
    BParLFO(Lfo),
    BParArp,
    BParEnvF(EnvelopeFollower),
    BParGlide(Glide),
    BParExternalMod(ExternalMod),
    BParInternalMod(InternalMod),
    BParSrcMode,
    BParStartCriteria(StartCriteria),
    BParFXDelay(FXDelay),
    BParFXChorus,
    BParFXFlanger,
    BParFXGainer,
    BParFXPhaser,
    BParFXReverb,
    BParFXIRC,
    BParFXSendLevel(BParFXSendLevels),
    BParFXFilter,
    BParFXCompressor,
    BParFXInverter,
    BParFXDYX,
    BParFXLimiter,
    BParFXSurroundPanner,
    BParFXShaper,
    BParFXDistortion,
    BParFXStereoSpread,
    BParFXLofi,
    BParFXSkreamer,
    BParFXRotator(BParFXRotator),
    BParFXTwang,
    BParFXCabinet,
    BParFX(BParFX),
    BDyxMorphGroup,
    BDyxMorphMap,
    BProgram,
    BProgramContainer(ProgramContainer),
    BSample,
    VoiceGroup(VoiceGroup),
    BZone,
    BZoneLevelEnv,
    BZoneArraySer,
    BGroupCompleteSer,
    PresetImpl,
    VoiceGroups(VoiceGroups),
    GroupList(GroupList),
    ZoneList(ZoneList),
    PrivateRawObject(PrivateRawObject),
    ProgramList(ProgramList),
    SlotList(SlotList),
    StartCriteriaList(StartCriteriaList),
    LoopArray(LoopArray),
    BParameterArraySerBParFX8(BParamArrayBParFX8),
    BParameterArraySerBParInternalMod16(InternalModArray16),
    BParameterArraySerBParExternalMod32(ExternalModArray32),
    BOutputConfiguration(OutputConfiguration),
    FileNameListPreK51(FileNameListPreK51),
    FNTablePreK51,
    BParEnvAhdsr(EnvelopeAhdsr),
    BParEnvFm7(EnvelopeFlex),
    BParEnvDbd(EnvelopeDbd),
    BParFXTape,
    BParFXTrans,
    BParFXSSLGEQ,
    BInsertBus(InsertBus),
    BParFXSSLGBusComp,
    SaveSettings(SaveSettings),
    BParGroupDynamics(GroupDynamics),
    FNTableImpl(FNTableImpl),
    FileNameList,
    BParFXFBComp,
    BParFXJump,
    QuickBrowseData(QuickBrowseData),
    BSnapshot(Snapshot),
    BGroupSnapshot,
    /// Stored instrument name and content name.
    BSnapshotMetaData(String, String),
    BParFXVan51,
    BParFXACBox,
    BParFXHotSolo,
    BParFXBassInvader,
    BParFXCat,
    BParFXDStortion,
    BParFXPlateReverb,
    BParFXCryWah,
    BParFXReplikaDelay,
    BParFXPhasis,
    BParFXFlair,
    BParFXChoral,
    BParFXCoreCell,
    BParFXHilbertLimiter,
    BParFXGaloisReverb,
    BParFXSupercharger,
    BParFXBassPro,
    BParFXPsycheDelay,
    BParFXRingModulator,
    Unsupported(u16),
    /// Any other `BParFX*` module, decoded by Kontakt's own parameter schema.
    BParFXModule(EffectParameters),
    /// 0x0F02: Kontakt's UI session (browser search, QuickBrowse, view scale,
    /// window size): MessagePack plus a binary record. Not decoded: no sound
    /// depends on it.
    SessionState,
    /// 0x48: a multi's per-slot MIDI settings.
    MultiSlots(MultiSlots),
    /// 0x49: a multi's four MIDI port channel maps.
    MidiPortMaps(MidiPortMaps),

    Program(Program),
    StructuredObject(StructuredObject),
}

/// Parameters of a `BParFX*` module, or None when it is not one or the schema
/// has no layout for its version. The version is peeked first so other chunks
/// are not parsed twice.
fn effect_module(chunk: &Chunk) -> Result<Option<EffectParameters>, Error> {
    let Some(&[0, lo, hi]) = chunk
        .data
        .get(..3)
        .and_then(|b| <&[u8; 3]>::try_from(b).ok())
    else {
        return Ok(None);
    };
    let fx = native_schema::class_of(chunk.id, u16::from_le_bytes([lo, hi]))
        .is_some_and(|c| c.starts_with("BParFX") && c != "BParFX");
    if !fx {
        return Ok(None);
    }
    let object = StructuredObject::try_from(chunk)?;
    EffectParameters::read(chunk.id, object.version, &object.public_data)
}

impl TryFrom<&Chunk> for KontaktObject {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<KontaktObject, Error> {
        // Delay, send levels, rotator and the BParFX wrapper keep their own decoders.
        if !matches!(chunk.id, 0x10 | 0x17 | 0x22 | 0x25) {
            if let Some(fx) = effect_module(chunk)? {
                return Ok(KontaktObject::BParFXModule(fx));
            }
        }
        Ok(match chunk.id {
            // 0x54 BParFXBassInvader?,
            0x00 => KontaktObject::BParModBase,
            0x01 => KontaktObject::BAutomationObject,
            0x02 => KontaktObject::OutputPartition,
            0x03 => KontaktObject::Bank(chunk.try_into()?),
            0x04 => KontaktObject::BGroup,
            0x05 => KontaktObject::BLoop,
            0x06 => KontaktObject::BParScript(chunk.try_into()?),
            0x07 => KontaktObject::BParEnv(chunk.try_into()?),
            0x08 => KontaktObject::BParLFO(chunk.try_into()?),
            0x09 => KontaktObject::BParArp,
            0x0a => KontaktObject::BParEnvF(chunk.try_into()?),
            0x0b => KontaktObject::BParGlide(chunk.try_into()?),
            0x0c => KontaktObject::BParExternalMod(chunk.try_into()?),
            0x0d => KontaktObject::BParInternalMod(chunk.try_into()?),
            0x0e => KontaktObject::BParSrcMode,
            0x0f => KontaktObject::BParStartCriteria(chunk.try_into()?),
            0x10 => KontaktObject::BParFXDelay(chunk.try_into()?),
            0x11 => KontaktObject::BParFXChorus,
            0x12 => KontaktObject::BParFXFlanger,
            0x13 => KontaktObject::BParFXGainer,
            0x14 => KontaktObject::BParFXPhaser,
            0x15 => KontaktObject::BParFXReverb,
            0x16 => KontaktObject::BParFXIRC,
            0x17 => KontaktObject::BParFXSendLevel(chunk.try_into()?),
            0x18 => KontaktObject::BParFXFilter,
            0x19 => KontaktObject::BParFXCompressor,
            0x1a => KontaktObject::BParFXInverter,
            0x1b => KontaktObject::BParFXDYX,
            0x1c => KontaktObject::BParFXLimiter,
            0x1d => KontaktObject::BParFXSurroundPanner,
            0x1e => KontaktObject::BParFXDistortion,
            0x1f => KontaktObject::BParFXStereoSpread,
            0x20 => KontaktObject::BParFXLofi,
            0x21 => KontaktObject::BParFXSkreamer,
            0x22 => KontaktObject::BParFXRotator(chunk.try_into()?),
            0x23 => KontaktObject::BParFXTwang,
            0x24 => KontaktObject::BParFXCabinet,
            0x25 => KontaktObject::BParFX(chunk.try_into()?),
            0x26 => KontaktObject::BDyxMorphGroup,
            0x27 => KontaktObject::BDyxMorphMap,
            0x28 => KontaktObject::Program(chunk.try_into()?),
            0x29 => KontaktObject::BProgramContainer(chunk.try_into()?),
            0x2a => KontaktObject::BSample,
            0x2b => KontaktObject::VoiceGroup(chunk.try_into()?),
            0x2c => KontaktObject::BZone,
            0x2d => KontaktObject::BZoneLevelEnv,
            0x2e => KontaktObject::BZoneArraySer,
            0x2f => KontaktObject::BGroupCompleteSer,
            0x30 => KontaktObject::PresetImpl,
            0x32 => KontaktObject::VoiceGroups(chunk.try_into()?),
            0x33 => KontaktObject::GroupList(chunk.try_into()?),
            0x34 => KontaktObject::ZoneList(chunk.try_into()?),
            0x35 => KontaktObject::PrivateRawObject(chunk.try_into()?),
            0x36 => KontaktObject::ProgramList(chunk.try_into()?),
            0x37 => KontaktObject::SlotList(chunk.try_into()?),
            0x38 => KontaktObject::StartCriteriaList(chunk.try_into()?),
            0x39 => KontaktObject::LoopArray(chunk.try_into()?),
            0x3a => KontaktObject::BParameterArraySerBParFX8(chunk.try_into()?),
            0x3b => KontaktObject::BParameterArraySerBParInternalMod16(chunk.try_into()?),
            0x3c => KontaktObject::BParameterArraySerBParExternalMod32(chunk.try_into()?),
            0x3d => KontaktObject::FileNameListPreK51(chunk.try_into()?),
            0x3e => KontaktObject::BOutputConfiguration(chunk.try_into()?),
            0x3f => KontaktObject::BParEnvAhdsr(chunk.try_into()?),
            0x40 => KontaktObject::BParEnvFm7(chunk.try_into()?),
            0x41 => KontaktObject::BParEnvDbd(chunk.try_into()?),
            0x42 => KontaktObject::BParFXTape,
            0x43 => KontaktObject::BParFXTrans,
            0x44 => KontaktObject::BParFXSSLGEQ,
            0x45 => KontaktObject::BInsertBus(chunk.try_into()?),
            0x46 => KontaktObject::BParFXSSLGBusComp,
            0x47 => KontaktObject::SaveSettings(chunk.try_into()?),
            0x48 => KontaktObject::MultiSlots(chunk.try_into()?),
            0x49 => KontaktObject::MidiPortMaps(chunk.try_into()?),
            0x4a => KontaktObject::BParGroupDynamics(chunk.try_into()?),
            0x4b => KontaktObject::FNTableImpl(chunk.try_into()?),
            0x4c => KontaktObject::BParFXFBComp,
            0x4d => KontaktObject::BParFXJump,
            0x4e => KontaktObject::QuickBrowseData(chunk.try_into()?),
            0x4f => KontaktObject::BSnapshot(chunk.try_into()?),
            0x50 => KontaktObject::BGroupSnapshot,
            0x51 => {
                let (name, content) = snapshot_metadata_names(chunk)?;
                KontaktObject::BSnapshotMetaData(name, content)
            }
            0x52 => KontaktObject::BParFXVan51,
            0x53 => KontaktObject::BParFXACBox,
            0x54 => KontaktObject::BParFXHotSolo,
            0x55 => KontaktObject::BParFXCat,
            0x56 => KontaktObject::BParFXDStortion,
            0x57 => KontaktObject::BParFXPlateReverb,
            0x58 => KontaktObject::BParFXCryWah,
            0x59 => KontaktObject::BParFXGaloisReverb,
            0x5a => KontaktObject::BParFXReplikaDelay,
            0x5b => KontaktObject::BParFXPhasis,
            0x5c => KontaktObject::BParFXFlair,
            0x5d => KontaktObject::BParFXChoral,
            0x5e => KontaktObject::BParFXCoreCell,
            0x5f => KontaktObject::BParFXHilbertLimiter,
            0x60 => KontaktObject::BParFXSupercharger,
            0x61 => KontaktObject::BParFXBassPro,
            0x63 => KontaktObject::BParFXPsycheDelay,
            0x64 => KontaktObject::BParFXRingModulator,
            0x0f02 => KontaktObject::SessionState,
            _ => KontaktObject::Unsupported(chunk.id),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    #[test]
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
    fn test_structured_object() -> Result<(), Error> {
        let file =
            File::open("tests/data/Objects/Kontakt/0x28-Program/ProgramV80/ProgramV80-000.kon")?;
        let data = Chunk::read(file)?;
        let _chunk: KontaktObject = (&data).try_into()?;

        Ok(())
    }

    #[test]
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
    fn test_fntableimpl() -> Result<(), Error> {
        let file = File::open("tests/data/Objects/Kontakt/0x4B-FNTableImpl/FNTableImpl-000.kon")?;
        let data = Chunk::read(file)?;
        let _chunk: KontaktObject = (&data).try_into()?;

        Ok(())
    }
}
