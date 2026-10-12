//! BParSrcMode wire records. Offsets include the flag/version prefix.
//! Field order: DSP_FORMAT_SPECIFICATION.md, Kontakt object framing, and
//! native source reader/writer 0x140d03aa0 / 0x140d12910. Unassigned controls
//! are named by byte offset; they are not guessed display/DSP parameters.
use crate::{Error, read_bytes::ReadBytesExt};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SourceValue {
    Float(f32),
    Integer(u32),
    Flag(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceField {
    pub offset: u16,
    pub name: &'static str,
    pub value: SourceValue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BParSrcMode {
    pub version: u16,
    /// Serialized enum, not the native internal enum or KSP constant.
    pub mode: u32,
    pub fields: Vec<SourceField>,
    pub bytes: u16,
}

impl BParSrcMode {
    /// Read exactly one source record, leaving the group's private trailer.
    pub fn read(mut r: impl ReadBytesExt) -> Result<Self, Error> {
        let start = r.stream_position()?;
        if r.read_u8()? != 0 {
            return Err(Error::Static("Unsupported structured source mode"));
        }
        let version = r.read_u16_le()?;
        if !(0x100..=0x106).contains(&version) {
            return Err(Error::Static("Unsupported source mode version"));
        }
        let mode = r.read_u32_le()?;
        if mode > 9 {
            return Err(Error::Static("Unsupported source mode ID"));
        }
        let mut out = Self {
            version,
            mode,
            fields: Vec::new(),
            bytes: 0,
        };
        // The common timing record grew from one float to three floats and a
        // flag in v0x102. Preserve disabled/sentinel values rather than clamp.
        out.field(&mut r, start, "common_float_7", 0)?;
        out.field(&mut r, start, "common_flag_11", 2)?;
        out.field(&mut r, start, "common_enum_12", 1)?;
        out.field(&mut r, start, "common_flag_16", 2)?;
        out.field(&mut r, start, "timing_value", 0)?;
        if version >= 0x102 {
            out.field(&mut r, start, "timing_unit", 0)?;
            out.field(&mut r, start, "timing_free", 0)?;
            out.field(&mut r, start, "timing_flag", 2)?;
        }
        match mode {
            1 | 2 => {
                out.field(&mut r, start, "machine_float_1", 0)?;
                out.field(&mut r, start, "machine_float_2", 0)?;
                out.field(&mut r, start, "machine_flag", 2)?;
            }
            4 => {
                out.field(&mut r, start, "slice_float_1", 0)?;
                out.field(&mut r, start, "slice_float_2", 0)?;
                out.field(&mut r, start, "slice_flag_1", 2)?;
                if version >= 0x106 {
                    out.field(&mut r, start, "slice_flag_2", 2)?;
                }
            }
            5 => {
                out.field(&mut r, start, "dfd_flag", 2)?;
                out.field(&mut r, start, "dfd_integer_1", 1)?;
                out.field(&mut r, start, "dfd_integer_2", 1)?;
            }
            8 => {
                out.field(&mut r, start, "pro_flag_1", 2)?;
                if version == 0x100 {
                    out.field(&mut r, start, "legacy_pro_integer", 1)?;
                } else {
                    out.field(&mut r, start, "pro_flag_2", 2)?;
                }
                out.field(&mut r, start, "pro_float_1", 0)?;
                out.field(&mut r, start, "pro_float_2", 0)?;
            }
            9 => {
                // Wavetable extensions first appeared in v0x103; v0x100..102
                // mode 9 carries only the common record.
                if version >= 0x103 {
                    for name in ["position", "form1", "phase", "phase_random"] {
                        out.field(&mut r, start, name, 0)?;
                    }
                    out.field(&mut r, start, "form_type", 1)?;
                    out.field(&mut r, start, "quality", 1)?;
                    if version >= 0x104 {
                        out.field(&mut r, start, "inharmonic_enabled", 2)?;
                        out.field(&mut r, start, "inharmonic", 0)?;
                    }
                    if version >= 0x105 {
                        out.field(&mut r, start, "form2", 0)?;
                        for name in ["form2_type", "mod_wave", "mod_type"] {
                            out.field(&mut r, start, name, 1)?;
                        }
                        for name in ["mod_amount", "mod_tune"] {
                            out.field(&mut r, start, name, 0)?;
                        }
                        // The existing v0x106 reader establishes a 16-byte
                        // nested tail. Its semantic/type layout is unresolved.
                        for name in [
                            "nested_word_83",
                            "nested_word_87",
                            "nested_word_91",
                            "nested_word_95",
                        ] {
                            out.field(&mut r, start, name, 1)?;
                        }
                    }
                }
            }
            _ => {}
        }
        out.bytes = u16::try_from(r.stream_position()? - start)
            .map_err(|_| Error::Static("Source record length overflow"))?;
        Ok(out)
    }

    fn field(
        &mut self,
        r: &mut impl ReadBytesExt,
        start: u64,
        name: &'static str,
        kind: u8,
    ) -> Result<(), Error> {
        let offset = (r.stream_position()? - start) as u16;
        let value = match kind {
            0 => SourceValue::Float(r.read_f32_le()?),
            1 => SourceValue::Integer(r.read_u32_le()?),
            _ => SourceValue::Flag(match r.read_u8()? {
                0 => false,
                1 => true,
                _ => return Err(Error::Static("Invalid source flag")),
            }),
        };
        self.fields.push(SourceField {
            offset,
            name,
            value,
        });
        Ok(())
    }
}
