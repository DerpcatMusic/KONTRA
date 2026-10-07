//! Byte-exact public FX fields. Units are serialized units, not KSP knob values.
//! See docs/architecture-v2/KONTAKT_FX_MOD_FORMAT.md for evidence and limits.
use crate::Error;

#[derive(Clone, Debug, PartialEq)]
pub enum EffectValue {
    Float(f32),
    Integer(i32),
    Byte(u8),
    Floats(Vec<f32>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectField {
    pub name: &'static str,
    pub value: EffectValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectParameters {
    pub module: u16,
    pub version: u16,
    pub fields: Vec<EffectField>,
}

// Unassigned names are intentional. A KSP list is not a binary schema.
pub fn effect_layout(module: u16) -> Option<&'static [(&'static str, char)]> {
    Some(match module {
        0x10 => &[
            ("time_ms", 'f'),
            ("damping", 'f'),
            ("pan", 'f'),
            ("feedback", 'f'),
            ("time_unit", 'f'),
            ("time_free_ms", 'f'),
            ("sync_unknown", 'f'),
            ("sync_flag", 'b'),
        ],
        0x11 => &[
            ("depth", 'f'),
            ("speed", 'f'),
            ("phase", 'f'),
            ("speed_unit", 'f'),
            ("speed_free", 'f'),
            ("sync_unknown", 'f'),
            ("sync_flag", 'b'),
        ],
        0x12 => &[
            ("depth", 'f'),
            ("speed", 'f'),
            ("phase", 'f'),
            ("feedback", 'f'),
            ("color", 'f'),
            ("speed_unit", 'f'),
            ("speed_free", 'f'),
            ("sync_unknown", 'f'),
            ("sync_flag", 'b'),
        ],
        0x13 => &[("gain", 'f')],
        0x14 => &[
            ("depth", 'f'),
            ("param_1", 'f'),
            ("speed", 'f'),
            ("param_3", 'f'),
            ("speed_unit", 'f'),
            ("speed_free", 'f'),
            ("sync_unknown", 'f'),
            ("sync_flag", 'b'),
        ],
        0x19 => &[
            ("param_0", 'f'),
            ("threshold_db", 'f'),
            ("ratio", 'f'),
            ("attack_ms", 'f'),
            ("release_ms", 'f'),
            ("link", 'b'),
        ],
        0x1a => &[("flag_0", 'b'), ("flag_1", 'b')],
        0x1c => &[("in_gain_db", 'f'), ("release_ms", 'f')],
        0x1d => &[("param_0", 'f'), ("param_1", 'f')],
        0x1e => &[("param_0", 'f'), ("drive", 'f'), ("damping", 'f')],
        0x1f => &[("spread", 'f'), ("pan", 'f'), ("pseudo_stereo", 'b')],
        0x20 => &[
            ("bits", 'f'),
            ("frequency", 'f'),
            ("noise_level", 'f'),
            ("flag_3", 'b'),
            ("noise_color", 'f'),
        ],
        0x21 => &[
            ("tone", 'f'),
            ("drive", 'f'),
            ("bass", 'f'),
            ("bright", 'f'),
            ("mix", 'f'),
        ],
        0x22 => &[
            ("speed", 'f'),
            ("balance", 'f'),
            ("accel_hi", 'f'),
            ("accel_lo", 'f'),
            ("distance", 'f'),
            ("mix", 'f'),
        ],
        0x42 => &[
            ("gain", 'f'),
            ("warmth", 'f'),
            ("hf_rolloff", 'f'),
            ("quality", 'b'),
        ],
        0x43 => &[
            ("input", 'f'),
            ("attack", 'f'),
            ("sustain", 'f'),
            ("smooth", 'f'),
        ],
        0x44 => &[
            ("lf_gain", 'f'),
            ("lf_freq", 'f'),
            ("lf_bell", 'b'),
            ("lmf_gain", 'f'),
            ("lmf_freq", 'f'),
            ("lmf_q", 'f'),
            ("hmf_gain", 'f'),
            ("hmf_freq", 'f'),
            ("hmf_q", 'f'),
            ("hf_gain", 'f'),
            ("hf_freq", 'f'),
            ("hf_bell", 'b'),
        ],
        0x46 => &[
            ("threshold", 'f'),
            ("ratio", 'f'),
            ("attack", 'f'),
            ("release", 'f'),
            ("makeup", 'f'),
            ("mix", 'f'),
            ("link", 'b'),
            ("flag_7", 'b'),
            ("param_8", 'f'),
        ],
        0x4c => &[
            ("input", 'f'),
            ("ratio", 'f'),
            ("attack", 'f'),
            ("release", 'f'),
            ("makeup", 'f'),
            ("mix", 'f'),
            ("param_6", 'f'),
            ("hq_mode", 'b'),
            ("link", 'b'),
            ("flag_9", 'b'),
        ],
        0x59 => &[
            ("room_type", 'f'),
            ("time", 'f'),
            ("size", 'f'),
            ("damping", 'f'),
            ("modulation", 'f'),
            ("diffusion", 'f'),
            ("predelay", 'f'),
            ("high_cut", 'f'),
            ("low_shelf", 'f'),
            ("stereo", 'f'),
        ],
        _ => return None,
    })
}

struct Reader<'a>(&'a [u8]);
impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let (head, rest) = self
            .0
            .split_first_chunk::<N>()
            .ok_or(Error::Static("Truncated FX parameter"))?;
        self.0 = rest;
        Ok(*head)
    }
    fn float(&mut self) -> Result<f32, Error> {
        Ok(f32::from_le_bytes(self.take()?))
    }
    fn integer(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.take()?))
    }
    fn list(&mut self) -> Result<Vec<f32>, Error> {
        let count = u32::from_le_bytes(self.take()?) as usize;
        if count > self.0.len() / 4 {
            return Err(Error::Static("Truncated FX parameter list"));
        }
        (0..count).map(|_| self.float()).collect()
    }
}

impl EffectParameters {
    /// Unknown IDs return None; recognized layouts must consume the entire payload.
    /// Floats/bytes retain exact bits; consumers validate physical ranges separately.
    pub fn read(module: u16, version: u16, public: &[u8]) -> Result<Option<Self>, Error> {
        let mut r = Reader(public);
        let mut fields = Vec::new();
        let mut push = |name, value| fields.push(EffectField { name, value });
        if let Some(layout) = effect_layout(module) {
            for &(name, kind) in layout {
                push(
                    name,
                    if kind == 'b' {
                        EffectValue::Byte(r.take::<1>()?[0])
                    } else {
                        EffectValue::Float(r.float()?)
                    },
                );
            }
        } else {
            match module {
                0x17 => {
                    push("sends", EffectValue::Floats(r.list()?));
                    push("outputs", EffectValue::Floats(r.list()?));
                }
                0x16 => {
                    push("decimation", EffectValue::Float(r.float()?));
                    push("block_size", EffectValue::Integer(r.integer()?));
                    for name in [
                        "predelay_ms",
                        "early_length",
                        "early_low_cut_hz",
                        "early_high_cut_hz",
                        "late_length",
                        "late_low_cut_hz",
                        "late_high_cut_hz",
                        "xpoint",
                    ] {
                        push(name, EffectValue::Float(r.float()?));
                    }
                    for name in [
                        "reverse",
                        "auto_gain",
                        "preserve_length",
                        "bypass_latency_compensation",
                        "volume_envelope",
                    ] {
                        push(name, EffectValue::Byte(r.take::<1>()?[0]));
                    }
                    push("curve_x", EffectValue::Floats(r.list()?));
                    push("curve_db", EffectValue::Floats(r.list()?));
                    push("ir_index", EffectValue::Integer(r.integer()?));
                }
                0x18 => {
                    let kind = r.integer()?;
                    push("filter_type", EffectValue::Integer(kind));
                    if (30..=41).contains(&kind) && version == 0x92 {
                        push("native_flag", EffectValue::Byte(r.take::<1>()?[0]));
                    }
                    if r.integer()? != kind {
                        return Err(Error::Static("Mismatched filter types"));
                    }
                    if (22..=24).contains(&kind) {
                        for band in 0..(kind - 21) as usize {
                            for &name in &[
                                ["frequency_1_hz", "bandwidth_1_octaves", "gain_1_db"],
                                ["frequency_2_hz", "bandwidth_2_octaves", "gain_2_db"],
                                ["frequency_3_hz", "bandwidth_3_octaves", "gain_3_db"],
                            ][band]
                            {
                                push(name, EffectValue::Float(r.float()?));
                            }
                        }
                    } else {
                        if matches!(kind, 30..=41 | 70 | 71) {
                            push("leading_value", EffectValue::Float(r.float()?));
                        }
                        push("cutoff", EffectValue::Float(r.float()?));
                        push("resonance", EffectValue::Float(r.float()?));
                        if !r.0.is_empty() {
                            if r.0.len() % 4 != 0 {
                                return Err(Error::Static("Partial filter parameter"));
                            }
                            push(
                                "extra",
                                EffectValue::Floats(
                                    (0..r.0.len() / 4)
                                        .map(|_| r.float())
                                        .collect::<Result<_, _>>()?,
                                ),
                            );
                        }
                    }
                }
                _ => return Ok(None),
            }
        }
        if !r.0.is_empty() {
            return Err(Error::Static("Trailing FX parameter bytes"));
        }
        Ok(Some(Self {
            module,
            version,
            fields,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_layouts_preserve_packed_fields_and_require_exact_lengths() {
        for module in 0..=0x64 {
            let Some(layout) = effect_layout(module) else {
                continue;
            };
            let mut bytes = Vec::new();
            for (i, &(_, kind)) in layout.iter().enumerate() {
                if kind == 'b' {
                    bytes.push(0xa5);
                } else {
                    bytes.extend((i as f32 + 0.25).to_le_bytes());
                }
            }
            let parsed = EffectParameters::read(module, 0x50, &bytes)
                .unwrap()
                .unwrap();
            for (i, field) in parsed.fields.iter().enumerate() {
                assert_eq!(field.name, layout[i].0);
                assert_eq!(
                    field.value,
                    if layout[i].1 == 'b' {
                        EffectValue::Byte(0xa5)
                    } else {
                        EffectValue::Float(i as f32 + 0.25)
                    }
                );
            }
            for end in 0..bytes.len() {
                assert!(EffectParameters::read(module, 0x50, &bytes[..end]).is_err());
            }
            bytes.push(0);
            assert!(EffectParameters::read(module, 0x50, &bytes).is_err());
        }
    }
    #[test]
    fn ladder_and_convolution_do_not_reinterpret_integer_or_flag_bytes() {
        let mut bytes = 33i32.to_le_bytes().to_vec();
        bytes.push(0xa5);
        bytes.extend(33i32.to_le_bytes());
        for value in [0.25f32, 0.75, 0.5] {
            bytes.extend(value.to_le_bytes());
        }
        let p = EffectParameters::read(0x18, 0x92, &bytes).unwrap().unwrap();
        assert_eq!(p.fields[1].value, EffectValue::Byte(0xa5));
        assert_eq!(p.fields[3].value, EffectValue::Float(0.75));
        bytes[5] = 34;
        assert!(EffectParameters::read(0x18, 0x92, &bytes).is_err());
        let mut ir = 1f32.to_le_bytes().to_vec();
        ir.extend(1024i32.to_le_bytes());
        ir.extend([0; 32]);
        ir.extend([0, 1, 0, 1, 0]);
        ir.extend([0; 8]);
        ir.extend((-1i32).to_le_bytes());
        let p = EffectParameters::read(0x16, 0x10, &ir).unwrap().unwrap();
        assert_eq!(p.fields[1].value, EffectValue::Integer(1024));
        assert_eq!(p.fields.last().unwrap().value, EffectValue::Integer(-1));
        assert_eq!(EffectParameters::read(0x64, 0x10, &[]).unwrap(), None);
    }
}
