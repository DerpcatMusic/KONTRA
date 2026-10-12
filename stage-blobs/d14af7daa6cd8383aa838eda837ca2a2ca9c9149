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

/// Group/source parameters stored without a module slot byte. The six
/// wavetable names map to native target IDs 11..=16; both native target
/// readers and writers omit the module slot for these IDs.
const GROUP_TARGETS: [&str; 18] = [
    "volume",
    "pan",
    "pitch",
    "playPos",
    "loopStart",
    "loopLength",
    "warpFactor",
    "warpFactor2",
    "wavetablePosition",
    "wavetableInharmonic",
    "wavetableModAmount",
    "wavetableModFrequency",
    "formantShift",
    "overlap",
    "grainSize",
    "grainSpeed",
    "playDirection",
    "legacyAddIntensity",
];
const MAX_TARGETS: u32 = 16;
const MAX_NAME_BYTES: u32 = 4096;
const SHAPER_TABLE_LEN: usize = 128;

/// One parameter driven by a modulator.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// Bit field; 0x02 is negative depth, other bits retain unassigned meaning.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub unknown_flags: u8,
}

impl ModTarget {
    /// Native target flag bit 1 stores the depth sign, independently of Invert.
    /// Retained from the frozen format-reader checkpoint b7f6af0b.
    pub fn signed_intensity(&self) -> f32 {
        if self.unknown_flags & 2 != 0 {
            -self.intensity
        } else {
            self.intensity
        }
    }
}

/// Modulation shaper transfer curve.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModShaper {
    /// Whether the shaper is applied; disabled shapers keep their curve.
    pub enabled: bool,
    /// Stored curve data.
    pub curve: ShaperCurve,
}

/// Shaper curve representation, matching Kontakt's table and graphical editors.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ShaperCurve {
    /// 128 output values for evenly spaced inputs.
    Table(Vec<f32>),
    /// Breakpoints sorted by `x`.
    Breakpoints(Vec<Breakpoint>),
}

impl ShaperCurve {
    /// Output for input `x` (clamped to 0..=1) by linear interpolation.
    /// An empty curve is the identity.
    // ponytail: breakpoint segment curvature is ignored; apply it once its formula is known.
    pub fn evaluate(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::Table(table) => {
                let Some(last) = table.len().checked_sub(1) else {
                    return x;
                };
                let position = x * last as f32;
                let index = (position.floor() as usize).min(last);
                let next = (index + 1).min(last);
                let fraction = position - index as f32;
                table[index] + (table[next] - table[index]) * fraction
            }
            Self::Breakpoints(points) => match points.iter().position(|p| p.x >= x) {
                None => points.last().map_or(x, |p| p.y),
                Some(0) => points[0].y,
                Some(index) => {
                    let (a, b) = (points[index - 1], points[index]);
                    let span = b.x - a.x;
                    if span > 0.0 {
                        a.y + (b.y - a.y) * (x - a.x) / span
                    } else {
                        b.y
                    }
                }
            },
        }
    }
}

/// One node of a graphical shaper curve.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
/// Versions:       0x100, 0x101, 0x102, 0x103, 0x104
/// Kontakt 7:      BParExternalMod
#[derive(Debug)]
pub struct ExternalMod(pub StructuredObject);

/// Decoded external modulation assignment.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ExternalModParams {
    /// KSP modulator name (`find_mod`), e.g. `VEL_VOLUME`.
    pub name: String,
    pub source: ModSource,
    pub targets: Vec<ModTarget>,
    /// Numeric id stored after the source; meaning unknown.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub unknown_id: u32,
    /// Source bytes of unknown meaning: four with a source (e.g. `7f000000`
    /// for some CCs), two when unassigned.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub unknown_source_data: Vec<u8>,
    /// Additional private footer bytes: one in v0x103, two in v0x104.
    /// Their semantics are unknown; retain them without treating them as flags.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub unknown_tail: Vec<u8>,
}

impl ExternalMod {
    pub fn params(&self) -> Result<ExternalModParams, Error> {
        if !matches!(self.0.version, 0x100..=0x104) {
            return Err(Error::Generic(format!(
                "Unsupported BParExternalMod version 0x{:X}",
                self.0.version
            )));
        }

        let mut reader = Cursor::new(self.0.private_data.as_slice());
        let legacy = self.0.version == 0x100;
        let targets = read_targets(&mut reader, legacy)?;
        let name = read_assignment_name(&mut reader, legacy)?;
        let (source, unknown_len) = match reader.read_u32_le()? {
            1 => (read_source(&mut reader)?, 4),
            2 => (ModSource::Unassigned, 2),
            other => {
                return Err(Error::Generic(format!(
                    "Unknown external modulation category {other}"
                )));
            }
        };
        let unknown_source_data = reader.read_bytes(unknown_len)?;
        let unknown_id = reader.read_u32_le()?;
        // The native versioned reader appends one byte at 0x103, then a
        // second at 0x104. The common source/target layout is unchanged.
        let tail_len = self.0.version.saturating_sub(0x102) as usize;
        let unknown_tail = reader.read_bytes(tail_len)?;
        ensure_consumed(&reader)?;

        Ok(ExternalModParams {
            name,
            source,
            targets,
            unknown_id,
            unknown_source_data,
            unknown_tail,
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
            )));
        }
    })
}

/// Read a modulator's target list: all target headers first, then one
/// shaper per target.
pub(crate) fn read_targets(
    reader: &mut Cursor<&[u8]>,
    legacy: bool,
) -> Result<Vec<ModTarget>, Error> {
    let count = reader.read_u32_le()?;
    if !(1..=MAX_TARGETS).contains(&count) {
        return Err(Error::Generic(format!(
            "Invalid modulation target count {count}"
        )));
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
        let name = read_assignment_name(reader, legacy)?;
        let slot = if GROUP_TARGETS.contains(&param.as_str()) {
            None
        } else {
            Some(reader.read_u8()?)
        };
        let invert = read_flag(reader)
            .map_err(|error| Error::context(format!("Modulation target {param} invert"), error))?;
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
        target.shaper = read_shaper(reader).map_err(|error| {
            Error::context(format!("Modulation target {} shaper", target.param), error)
        })?;
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
            return Err(Error::Generic(format!(
                "Unknown modulation shaper kind {other}"
            )));
        }
    };
    Ok(Some(ModShaper { enabled, curve }))
}

/// Parameter arrays (0x3A/0x3B/0x3C) store a presence byte per slot followed
/// by the slot's chunk. Returns `(slot, chunk)` for every occupied slot.
pub fn read_param_slots(
    object: &StructuredObject,
    slots: usize,
) -> Result<Vec<(usize, Chunk)>, Error> {
    if !matches!(object.version, 0x10..=0x13) {
        return Err(Error::Generic(format!(
            "Unsupported parameter array version 0x{:X}",
            object.version
        )));
    }

    let parse = |slot_words: bool| -> Result<Vec<(usize, Chunk)>, Error> {
        let mut reader = Cursor::new(object.public_data.as_slice());
        if object.version == 0x13 && reader.read_u32_le()? as usize != slots {
            return Err(Error::Static(
                "Parameter array serialized slot count differs",
            ));
        }
        let mut items = Vec::new();
        for slot in 0..slots {
            let present = read_flag(&mut reader).map_err(|error| {
                Error::context(
                    format!(
                        "Parameter array 0x{:X} slot {slot} presence",
                        object.version
                    ),
                    error,
                )
            })?;
            if slot_words {
                reader.read_u32_le()?;
            }
            if present {
                items.push((slot, Chunk::read(&mut reader)?));
            }
        }
        if matches!(object.version, 0x11 | 0x13) {
            ensure_consumed(&reader)?;
        }
        Ok(items)
    };
    if object.version != 0x11 {
        return parse(false);
    }
    // Native v11 has two context-dependent layouts; bounded records must fit exactly one.
    match (parse(false), parse(true)) {
        (Ok(items), Err(_)) | (Err(_), Ok(items)) => Ok(items),
        (Ok(_), Ok(_)) => Err(Error::Static("Ambiguous parameter array v0x11 framing")),
        (Err(error), Err(_)) => Err(error),
    }
}

/// Length-prefixed 8-bit string.
pub(crate) fn read_name(reader: &mut Cursor<&[u8]>) -> Result<String, Error> {
    read_assignment_name(reader, false)
}

/// Only the legacy C-string reader uses -1 for an absent assignment/target name.
pub(crate) fn read_assignment_name(
    reader: &mut Cursor<&[u8]>,
    legacy: bool,
) -> Result<String, Error> {
    let len = reader.read_u32_le()?;
    if legacy && len == u32::MAX {
        return Ok(String::new());
    }
    if len > MAX_NAME_BYTES {
        return Err(Error::Generic(format!(
            "Modulation name too long ({len} bytes)"
        )));
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
        return Err(Error::Generic(format!(
            "{left} unexpected trailing modulation bytes"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_external_source_shapes_keep_signed_depth_shaper_smoothing_and_version_footer() {
        for version in 0x100u16..=0x104 {
            for source in 1u32..=12 {
                let mut data = 1u32.to_le_bytes().to_vec();
                data.extend(5u32.to_le_bytes());
                data.extend(b"pitch");
                data.extend(0.25f32.to_le_bytes());
                data.extend((-1i16).to_le_bytes());
                data.push(0x12);
                data.extend(250u16.to_le_bytes());
                data.extend(0u32.to_le_bytes());
                data.push(1);
                data.extend([1, 1]);
                for index in 0..128 {
                    data.extend((index as f32 / 127.0).to_le_bytes());
                }
                data.extend(0u32.to_le_bytes());
                data.extend(1u32.to_le_bytes());
                data.extend(source.to_le_bytes());
                if source == 4 {
                    data.push(74);
                }
                if source == 12 {
                    data.extend(9u32.to_le_bytes());
                }
                data.extend([0x7f, 0, 0, 0]);
                data.extend(17u32.to_le_bytes());
                data.extend(std::iter::repeat_n(
                    0xa5,
                    version.saturating_sub(0x102) as usize,
                ));
                let object = ExternalMod(StructuredObject {
                    version,
                    private_data: data,
                    public_data: Vec::new(),
                    children: Vec::new(),
                });
                let p = object.params().unwrap();
                assert_eq!(p.targets[0].signed_intensity(), -0.25);
                assert!(p.targets[0].invert);
                assert_eq!(p.targets[0].lag_ms, 250);
                let ShaperCurve::Table(table) = &p.targets[0].shaper.as_ref().unwrap().curve else {
                    panic!("table")
                };
                assert_eq!(table.len(), 128);
                assert_eq!((table[0], table[127]), (0.0, 1.0));
                assert_eq!(p.unknown_tail.len(), version.saturating_sub(0x102) as usize);
                let expected = match source {
                    1 => ModSource::PitchBend,
                    2 => ModSource::PolyAftertouch,
                    3 => ModSource::MonoAftertouch,
                    4 => ModSource::MidiCc(74),
                    5 => ModSource::KeyPosition,
                    6 => ModSource::Velocity,
                    7 => ModSource::ReleaseVelocity,
                    8 => ModSource::ReleaseTriggerCounter,
                    9 => ModSource::Constant,
                    10 => ModSource::RandomUnipolar,
                    11 => ModSource::RandomBipolar,
                    12 => ModSource::Script(9),
                    _ => unreachable!(),
                };
                assert_eq!(p.source, expected);
            }
        }
    }

    #[test]
    fn wavetable_group_targets_do_not_consume_module_slot_bytes() {
        for param in &GROUP_TARGETS[6..] {
            let mut private = 2u32.to_le_bytes().to_vec();
            for (target, slot, invert) in [(*param, None, true), ("filterCutoff", Some(7), false)] {
                private.extend((target.len() as u32).to_le_bytes());
                private.extend(target.as_bytes());
                private.extend(0.25f32.to_le_bytes());
                private.extend((-1i16).to_le_bytes());
                private.push(0x10);
                private.extend(15u16.to_le_bytes());
                private.extend(0u32.to_le_bytes()); // Unnamed target.
                if let Some(slot) = slot {
                    private.push(slot);
                }
                private.push(u8::from(invert));
            }
            let shaper_offset = private.len();
            private.extend([0, 0]); // Both shapers absent.
            private.extend(0u32.to_le_bytes()); // Unnamed assignment.
            private.extend(1u32.to_le_bytes());
            private.extend(6u32.to_le_bytes()); // Velocity.
            private.extend([0; 4]);
            private.extend(17u32.to_le_bytes());
            private.push(0x80);
            let mut body = vec![1, 3, 1];
            body.extend((private.len() as u32).to_le_bytes());
            body.extend(&private);
            body.extend([0; 8]);
            let original = Chunk {
                id: 0x0c,
                data: body,
            };
            let parsed = ExternalMod::try_from(&original).unwrap().params().unwrap();
            assert_eq!(
                (
                    parsed.targets[0].param.as_str(),
                    parsed.targets[0].slot,
                    parsed.targets[0].invert
                ),
                (*param, None, true)
            );
            assert_eq!(
                (
                    parsed.targets[1].param.as_str(),
                    parsed.targets[1].slot,
                    parsed.targets[1].invert
                ),
                ("filterCutoff", Some(7), false)
            );
            assert!(parsed.targets.iter().all(|target| target.shaper.is_none()));
            assert_eq!(parsed.unknown_id, 17);
            assert_eq!(parsed.unknown_tail, [0x80]);
            let mut before = Vec::new();
            original.write(&mut before).unwrap();
            let readback = Chunk::read(Cursor::new(&before)).unwrap();
            assert_eq!(
                ExternalMod::try_from(&readback).unwrap().params().unwrap(),
                parsed
            );
            let mut after = Vec::new();
            readback.write(&mut after).unwrap();
            assert_eq!(after, before);
            let mut bad = private.clone();
            bad[shaper_offset] = 15;
            let object = ExternalMod(StructuredObject {
                version: 0x103,
                private_data: bad,
                public_data: Vec::new(),
                children: Vec::new(),
            });
            assert!(
                object.params().is_err(),
                "unknown shaper kinds remain rejected"
            );
        }
    }

    #[test]
    fn modern_external_modulation_retains_opaque_footer_and_strict_bounds() {
        let mut private = Vec::new();
        private.extend(1u32.to_le_bytes());
        private.extend(6u32.to_le_bytes());
        private.extend(b"volume");
        private.extend(0.75f32.to_le_bytes());
        private.extend((-1i16).to_le_bytes());
        private.push(0x10);
        private.extend(0u16.to_le_bytes());
        private.extend(0u32.to_le_bytes());
        private.extend([0, 0]); // Name, invert, shaper.
        private.extend(10u32.to_le_bytes());
        private.extend(b"VEL_VOLUME");
        private.extend(1u32.to_le_bytes());
        private.extend(6u32.to_le_bytes()); // Velocity.
        private.extend([0x7f, 0, 0, 0]);
        private.extend(5u32.to_le_bytes());
        private.extend([0x80, 0xff]); // Unknown bytes need not be booleans.
        let mut modulator = ExternalMod(StructuredObject {
            version: 0x104,
            private_data: private.clone(),
            public_data: Vec::new(),
            children: Vec::new(),
        });
        let parsed = modulator.params().unwrap();
        assert_eq!(parsed.source, ModSource::Velocity);
        assert_eq!(parsed.name, "VEL_VOLUME");
        assert_eq!(parsed.targets[0].intensity, 0.75);
        assert_eq!(parsed.unknown_tail, [0x80, 0xff]);
        assert_eq!(modulator.0.private_data, private);
        // Retain the exact native version, framing and opaque private bytes.
        let mut body = vec![1, 4, 1];
        body.extend((private.len() as u32).to_le_bytes());
        body.extend(&private);
        body.extend([0; 8]); // Empty public and child data.
        let chunk = Chunk {
            id: 0x0c,
            data: body,
        };
        let mut original = Vec::new();
        chunk.write(&mut original).unwrap();
        let raw = Chunk::read(Cursor::new(&original)).unwrap();
        assert_eq!(
            ExternalMod::try_from(&raw).unwrap().params().unwrap(),
            parsed
        );
        let mut rewritten = Vec::new();
        raw.write(&mut rewritten).unwrap();
        assert_eq!(rewritten, original);
        for end in 0..private.len() {
            modulator.0.private_data = private[..end].to_vec();
            assert!(modulator.params().is_err(), "truncation {end}");
        }
        modulator.0.private_data = private.clone();
        modulator.0.private_data.push(0);
        assert!(modulator.params().is_err());
        modulator.0.private_data = private[..private.len() - 2].to_vec();
        modulator.0.version = 0x102;
        assert!(modulator.params().unwrap().unknown_tail.is_empty());
        modulator.0.version = 0x103;
        assert!(modulator.params().is_err(), "the v0x103 footer is required");
        modulator.0.private_data.push(0x80);
        let parsed = modulator.params().unwrap();
        assert_eq!(parsed.source, ModSource::Velocity);
        assert_eq!(parsed.targets[0].intensity, 0.75);
        assert_eq!(parsed.unknown_tail, [0x80]);
        let mut chunk = chunk;
        chunk.data[1..3].copy_from_slice(&0x103u16.to_le_bytes());
        chunk.data[3..7].copy_from_slice(&(modulator.0.private_data.len() as u32).to_le_bytes());
        chunk
            .data
            .splice(7..7 + private.len(), modulator.0.private_data.clone());
        let mut original = Vec::new();
        chunk.write(&mut original).unwrap();
        let raw = Chunk::read(Cursor::new(&original)).unwrap();
        assert_eq!(
            ExternalMod::try_from(&raw).unwrap().params().unwrap(),
            parsed
        );
        let mut rewritten = Vec::new();
        raw.write(&mut rewritten).unwrap();
        assert_eq!(rewritten, original);
        modulator.0.private_data.push(0xff);
        assert!(
            modulator.params().is_err(),
            "v0x103 does not accept the v0x104 footer"
        );
    }
}
