//! Modulation assignments shared by internal (0x0D) and external (0x0C) modulators.
//!
//! Layouts were decoded from local Kontakt presets; offsets, value
//! distributions and confidence are recorded in `audits/MODULATION.md`.

use std::io::Cursor;

use crate::{
    kontakt::{Chunk, KontaktError, StructuredObject},
    read_bytes::ReadBytesExt,
    Error,
};

/// Target parameters stored without a module slot byte. Every other
/// parameter (`eqGain1`, `filterCutoff`, `ahdsr_attack`, ...) carries one.
const GROUP_TARGETS: [&str; 3] = ["volume", "pitch", "playPos"];
const MAX_TARGETS: u32 = 16;
const MAX_NAME_BYTES: u32 = 4096;
const SHAPER_TABLE_LEN: usize = 128;

/// One parameter driven by a modulator.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ModTarget {
    /// Engine parameter id, e.g. `volume`, `pitch`, `playPos`, `eqGain1`.
    pub param: String,
    /// Modulation depth; every local preset stores 0..=1.
    pub intensity: f32,
    /// Lag (smoothing) time; the Kontakt manual gives this knob in milliseconds.
    pub lag_ms: u16,
    /// KSP target name (`find_target`); `<none>` or empty when unnamed.
    pub name: String,
    /// Owning module slot for module parameters; `None` for group parameters.
    pub slot: Option<u8>,
    /// Invert button.
    pub invert: bool,
    /// Modulation shaper curve, when one was ever edited.
    pub shaper: Option<ModShaper>,
    /// Always -1 in local presets.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub unknown_i16: i16,
    /// Bit field; 0x10 always set, 0x02/0x04 of unknown meaning.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub unknown_flags: u8,
}

/// Modulation shaper transfer curve.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ModShaper {
    /// Whether the shaper is applied; disabled shapers keep their curve.
    pub enabled: bool,
    /// Stored curve data.
    pub curve: ShaperCurve,
}

/// Shaper curve representation, matching Kontakt's table and graphical editors.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum ShaperCurve {
    /// 128 output values for evenly spaced inputs.
    Table(Vec<f32>),
    /// Breakpoints sorted by `x`.
    Breakpoints(Vec<Breakpoint>),
}

/// One node of a graphical shaper curve.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Breakpoint {
    /// Input position, 0..=1.
    pub x: f32,
    /// Output value, 0..=1.
    pub y: f32,
    /// Curvature of the segment starting here, -1..=1 (0 is linear).
    pub curve: f32,
}

/// External modulation source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum ModSource {
    PitchBend,
    PolyAftertouch,
    MonoAftertouch,
    /// MIDI continuous controller number.
    MidiCc(u8),
    KeyPosition,
    Velocity,
    ReleaseVelocity,
    ReleaseTriggerCounter,
    Constant,
    RandomUnipolar,
    RandomBipolar,
    /// Value sent by the script in this script slot.
    Script(u32),
    /// Assignment without a source.
    Unassigned,
}

/// # ExternalMod
///
/// Assignment of an external source (MIDI, velocity, script...) to a parameter.
///
/// Type:           Chunk<StructuredObject>, fields in private data
/// SerType:        0x0C
/// Versions:       0x100, 0x101, 0x102
/// Kontakt 7:      BParExternalMod
#[derive(Debug)]
pub struct ExternalMod(pub StructuredObject);

/// Decoded external modulation assignment.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ExternalModParams {
    /// KSP modulator name (`find_mod`), e.g. `VEL_VOLUME`.
    pub name: String,
    pub source: ModSource,
    pub targets: Vec<ModTarget>,
    /// Numeric id stored after the source; meaning unknown.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub unknown_id: u32,
    /// Four source bytes of unknown meaning (e.g. `7f000000` for some CCs).
    #[cfg_attr(feature = "serde", serde(skip))]
    pub unknown_source_data: [u8; 4],
}

impl ExternalMod {
    pub fn params(&self) -> Result<ExternalModParams, Error> {
        if !(0x100..=0x102).contains(&self.0.version) {
            return Err(Error::Generic(format!(
                "Unsupported BParExternalMod version 0x{:X}",
                self.0.version
            )));
        }

        let mut reader = Cursor::new(self.0.private_data.as_slice());
        let targets = read_targets(&mut reader)?;
        let name = read_name(&mut reader)?;
        let source = match reader.read_u32_le()? {
            1 => read_source(&mut reader)?,
            2 => ModSource::Unassigned,
            other => {
                return Err(Error::Generic(format!(
                    "Unknown external modulation category {other}"
                )))
            }
        };
        let mut unknown_source_data = [0; 4];
        std::io::Read::read_exact(&mut reader, &mut unknown_source_data)?;
        let unknown_id = reader.read_u32_le()?;
        ensure_consumed(&reader)?;

        Ok(ExternalModParams {
            name,
            source,
            targets,
            unknown_id,
            unknown_source_data,
        })
    }
}

impl TryFrom<&Chunk> for ExternalMod {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        const CHUNK_ID: u16 = 0x0C;
        if chunk.id != CHUNK_ID {
            return Err(KontaktError::IncorrectID {
                expected: CHUNK_ID,
                got: chunk.id,
            }
            .into());
        }
        Ok(Self(chunk.try_into()?))
    }
}

/// Source codes follow the source order in the Kontakt manual, starting at 1.
/// Codes 2, 3, 7, 10 and 11 were not present locally; they are assumed to
/// carry no extra payload, which the exact-length check would expose.
fn read_source(reader: &mut Cursor<&[u8]>) -> Result<ModSource, Error> {
    Ok(match reader.read_u32_le()? {
        1 => ModSource::PitchBend,
        2 => ModSource::PolyAftertouch,
        3 => ModSource::MonoAftertouch,
        4 => ModSource::MidiCc(reader.read_u8()?),
        5 => ModSource::KeyPosition,
        6 => ModSource::Velocity,
        7 => ModSource::ReleaseVelocity,
        8 => ModSource::ReleaseTriggerCounter,
        9 => ModSource::Constant,
        10 => ModSource::RandomUnipolar,
        11 => ModSource::RandomBipolar,
        12 => ModSource::Script(reader.read_u32_le()?),
        other => {
            return Err(Error::Generic(format!(
                "Unknown external modulation source {other}"
            )))
        }
    })
}

/// Read a modulator's target list: all target headers first, then one
/// shaper per target.
pub(crate) fn read_targets(reader: &mut Cursor<&[u8]>) -> Result<Vec<ModTarget>, Error> {
    let count = reader.read_u32_le()?;
    if !(1..=MAX_TARGETS).contains(&count) {
        return Err(Error::Generic(format!("Invalid modulation target count {count}")));
    }

    let mut targets = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let param = read_name(reader)?;
        let intensity = reader.read_f32_le()?;
        if !intensity.is_finite() {
            return Err(Error::Static("Non-finite modulation intensity"));
        }
        let unknown_i16 = reader.read_i16_le()?;
        let unknown_flags = reader.read_u8()?;
        let lag_ms = reader.read_u16_le()?;
        let name = read_name(reader)?;
        let slot = if GROUP_TARGETS.contains(&param.as_str()) {
            None
        } else {
            Some(reader.read_u8()?)
        };
        let invert = read_flag(reader)?;
        targets.push(ModTarget {
            param,
            intensity,
            lag_ms,
            name,
            slot,
            invert,
            shaper: None,
            unknown_i16,
            unknown_flags,
        });
    }
    for target in &mut targets {
        target.shaper = read_shaper(reader)?;
    }
    Ok(targets)
}

fn read_shaper(reader: &mut Cursor<&[u8]>) -> Result<Option<ModShaper>, Error> {
    let kind = reader.read_u8()?;
    if kind == 0 {
        return Ok(None);
    }

    let enabled = read_flag(reader)?;
    let curve = match kind {
        1 => {
            let table = (0..SHAPER_TABLE_LEN)
                .map(|_| reader.read_f32_le())
                .collect::<Result<Vec<_>, _>>()?;
            ShaperCurve::Table(table)
        }
        2 => {
            let count = reader.read_u8()?;
            let points = (0..count)
                .map(|_| {
                    Ok(Breakpoint {
                        x: reader.read_f32_le()?,
                        y: reader.read_f32_le()?,
                        curve: reader.read_f32_le()?,
                    })
                })
                .collect::<Result<Vec<_>, std::io::Error>>()?;
            ShaperCurve::Breakpoints(points)
        }
        other => {
            return Err(Error::Generic(format!("Unknown modulation shaper kind {other}")))
        }
    };
    Ok(Some(ModShaper { enabled, curve }))
}

/// Parameter arrays (0x3A/0x3B/0x3C) store a presence byte per slot followed
/// by the slot's chunk. Returns `(slot, chunk)` for every occupied slot.
pub fn read_param_slots(object: &StructuredObject, slots: usize) -> Result<Vec<(usize, Chunk)>, Error> {
    if !matches!(object.version, 0x10 | 0x12) {
        return Err(Error::Generic(format!(
            "Unsupported parameter array version 0x{:X}",
            object.version
        )));
    }

    let mut reader = Cursor::new(object.public_data.as_slice());
    let mut items = Vec::new();
    for slot in 0..slots {
        if read_flag(&mut reader)? {
            items.push((slot, Chunk::read(&mut reader)?));
        }
    }
    Ok(items)
}

/// Length-prefixed 8-bit string.
pub(crate) fn read_name(reader: &mut Cursor<&[u8]>) -> Result<String, Error> {
    let len = reader.read_u32_le()?;
    if len > MAX_NAME_BYTES {
        return Err(Error::Generic(format!("Modulation name too long ({len} bytes)")));
    }
    let bytes = reader.read_bytes(len as usize)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_flag(reader: &mut Cursor<&[u8]>) -> Result<bool, Error> {
    match reader.read_u8()? {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(Error::Generic(format!("Invalid boolean byte {other}"))),
    }
}

pub(crate) fn ensure_consumed(reader: &Cursor<&[u8]>) -> Result<(), Error> {
    let left = (reader.get_ref().len() as u64).saturating_sub(reader.position());
    if left != 0 {
        return Err(Error::Generic(format!("{left} unexpected trailing modulation bytes")));
    }
    Ok(())
}
