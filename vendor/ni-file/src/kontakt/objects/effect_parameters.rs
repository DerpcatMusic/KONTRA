//! Byte-exact public FX fields. Units are serialized units, not KSP knob values.
//! Layouts, versions and validation come from Kontakt 8.13.1's own readers (`native_schema`);
//! the names below are hand-assigned where evidence exists. See
//! docs/architecture-v2/KONTAKT_FX_MOD_FORMAT.md for evidence and limits.
use crate::{
    kontakt::native_schema::{self, Undecoded},
    Error,
};

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
            ("mode", 'i'),
            ("threshold_db", 'f'),
            ("ratio", 'f'),
            ("attack_ms", 'f'),
            ("release_ms", 'f'),
            ("link", 'b'),
        ],
        0x1a => &[("flag_0", 'b'), ("flag_1", 'b')],
        0x1c => &[("in_gain_db", 'f'), ("release_ms", 'f')],
        0x1d => &[("param_0", 'f'), ("mode", 'i')],
        0x1e => &[("mode", 'i'), ("drive", 'f'), ("damping", 'f')],
        0x1f => &[("spread", 'f'), ("pan", 'f'), ("pseudo_stereo", 'b')],
        0x20 => &[
            ("bits", 'f'),
            ("frequency", 'f'),
            ("flag_2", 'b'),
            ("noise_level", 'f'),
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
            ("mode", 'i'),
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
            ("room_type", 'i'), // u32 enum 0/1: Kontakt 8.13.1 FUN_140d019f0
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

const FIELD_NAMES: [&str; 64] = [
    "param_0", "param_1", "param_2", "param_3", "param_4", "param_5", "param_6", "param_7",
    "param_8", "param_9", "param_10", "param_11", "param_12", "param_13", "param_14", "param_15",
    "param_16", "param_17", "param_18", "param_19", "param_20", "param_21", "param_22", "param_23",
    "param_24", "param_25", "param_26", "param_27", "param_28", "param_29", "param_30", "param_31",
    "param_32", "param_33", "param_34", "param_35", "param_36", "param_37", "param_38", "param_39",
    "param_40", "param_41", "param_42", "param_43", "param_44", "param_45", "param_46", "param_47",
    "param_48", "param_49", "param_50", "param_51", "param_52", "param_53", "param_54", "param_55",
    "param_56", "param_57", "param_58", "param_59", "param_60", "param_61", "param_62", "param_63",
];

impl EffectParameters {
    /// Decode an effect's stored parameters exactly as Kontakt 8.13.1's own reader does
    /// (`native_schema`): the stored version selects the layout, validated values are checked,
    /// and every byte must be accounted for. Unknown IDs and versions return None.
    ///
    /// Field order is the native read order. Names come from the hand table when its kinds
    /// agree position by position (older versions read a prefix of newer ones); otherwise
    /// they stay generic. Floats/bytes retain exact bits; consumers validate physical ranges.
    pub fn read(module: u16, version: u16, public: &[u8]) -> Result<Option<Self>, Error> {
        let Some(class) = native_schema::class_of(module, version) else {
            return Ok(None);
        };
        if !class.starts_with("BParFX") || class == "BParFX" {
            return Ok(None);
        }
        let decoded = native_schema::decode(module, version, public).map_err(|e| match e {
            Undecoded::Truncated if matches!(module, 0x16 | 0x17) => {
                Error::Static("Truncated FX parameter list")
            }
            Undecoded::Truncated => Error::Static("Truncated FX parameter"),
            Undecoded::Trailing(_) => Error::Static("Trailing FX parameter bytes"),
            other => Error::Generic(format!("{class} 0x{version:X}: {other:?}")),
        })?;
        let reads: Vec<&native_schema::Field> = decoded.fields.iter().filter(|f| f.depth == 0).collect();
        let kind = |f: &native_schema::Field| match f.kind {
            'F' => 'f',
            'I' => 'i',
            'B' => 'b',
            other => other,
        };
        // The hand names describe the Shaper only, not the SurroundPanner sharing SerType 0x1D.
        let named = effect_layout(module)
            .filter(|_| module != 0x1d || version == 0x80)
            .filter(|names| {
                names.len() >= reads.len()
                    && reads.iter().zip(names.iter()).all(|(f, &(_, c))| kind(f) == c)
            });
        let mut fields = Vec::with_capacity(reads.len());
        for (index, f) in reads.iter().enumerate() {
            let name = named.map_or_else(
                || FIELD_NAMES.get(index).copied().unwrap_or("param_n"),
                |n| n[index].0,
            );
            let raw = f.value.unwrap_or(0);
            fields.push(EffectField {
                name,
                value: match f.kind {
                    'F' => EffectValue::Float(f32::from_bits(raw as u32)),
                    'B' => EffectValue::Byte(raw as u8),
                    'H' => EffectValue::Integer(raw as u16 as i32),
                    _ => EffectValue::Integer(raw as u32 as i32),
                },
            });
        }
        Ok(Some(Self { module, version, fields }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bytes for a layout of float/int/byte kinds with recognizable values.
    fn bytes_for(kinds: impl Iterator<Item = char>) -> (Vec<u8>, Vec<EffectValue>) {
        let (mut bytes, mut expected) = (Vec::new(), Vec::new());
        for (i, kind) in kinds.enumerate() {
            match kind {
                'b' => {
                    bytes.push(0);
                    expected.push(EffectValue::Byte(0));
                }
                'i' => {
                    bytes.extend(0i32.to_le_bytes());
                    expected.push(EffectValue::Integer(0));
                }
                _ => {
                    bytes.extend((i as f32 + 0.25).to_le_bytes());
                    expected.push(EffectValue::Float(i as f32 + 0.25));
                }
            }
        }
        (bytes, expected)
    }

    #[test]
    fn hand_names_describe_the_native_layouts_and_native_framing_is_exact() {
        let mut checked = 0;
        for info in native_schema::readers() {
            if !info.class.starts_with("BParFX") || info.class == "BParFX" || info.status != "typed" {
                continue;
            }
            let Some(names) = effect_layout(info.sertype).filter(|_| info.sertype != 0x1d || info.current == 0x80) else {
                continue;
            };
            let kinds: Vec<char> = names.iter().map(|&(_, c)| c).collect();
            // The hand table names one stored version's layout (the newest it was written for):
            // find the version whose native layout is exactly that kind sequence.
            let Some(version) = info.versions.iter().flat_map(|&(lo, hi)| lo..=hi).find(|&v| {
                native_schema::synthesize(info.sertype, v)
                    .ok()
                    .and_then(|b| native_schema::decode(info.sertype, v, &b).ok())
                    .is_some_and(|d| {
                        d.fields.iter().filter(|f| f.depth == 0).map(|f| match f.kind {
                            'F' => 'f',
                            'I' => 'i',
                            'B' => 'b',
                            o => o,
                        }).eq(kinds.iter().copied())
                    })
            }) else {
                continue;
            };
            let (bytes, expected) = bytes_for(kinds.iter().copied());
            let parsed = EffectParameters::read(info.sertype, version, &bytes)
                .unwrap_or_else(|e| panic!("{} 0x{version:X}: {e}", info.class))
                .unwrap();
            assert_eq!(
                parsed.fields.iter().map(|f| f.value.clone()).collect::<Vec<_>>(),
                expected,
                "{}",
                info.class
            );
            for (f, &(name, _)) in parsed.fields.iter().zip(names.iter()) {
                assert_eq!(f.name, name, "{}", info.class);
            }
            for end in 0..bytes.len() {
                assert!(
                    EffectParameters::read(info.sertype, version, &bytes[..end]).is_err(),
                    "{} accepted a {end}-byte prefix",
                    info.class
                );
            }
            let mut longer = bytes.clone();
            longer.push(0);
            assert!(EffectParameters::read(info.sertype, version, &longer).is_err());
            checked += 1;
        }
        assert!(checked >= 8, "only {checked} hand layouts compared");
    }

    #[test]
    fn every_native_effect_layout_round_trips_with_exact_boundaries() {
        let mut checked = 0;
        for info in native_schema::readers() {
            if !info.class.starts_with("BParFX") || info.class == "BParFX" || info.status != "typed" {
                continue;
            }
            for &(lo, hi) in info.versions {
                for version in lo..=hi {
                    let Ok(bytes) = native_schema::synthesize(info.sertype, version) else { continue };
                    let p = EffectParameters::read(info.sertype, version, &bytes)
                        .unwrap_or_else(|e| panic!("{} 0x{version:X}: {e}", info.class))
                        .unwrap();
                    assert!(!p.fields.is_empty(), "{}", info.class);
                    for end in 0..bytes.len() {
                        assert!(
                            EffectParameters::read(info.sertype, version, &bytes[..end]).is_err(),
                            "{} 0x{version:X} accepted a {end}-byte prefix",
                            info.class
                        );
                    }
                    let mut longer = bytes.clone();
                    longer.push(0);
                    assert!(EffectParameters::read(info.sertype, version, &longer).is_err());
                    checked += 1;
                }
            }
        }
        assert!(checked > 80, "only {checked} effect layouts");
    }

    #[test]
    fn unknown_kinds_and_versions_are_not_decoded() {
        assert_eq!(EffectParameters::read(0xfe, 0x10, &[0; 8]).unwrap(), None);
        assert_eq!(EffectParameters::read(0x64, 0x10, &[]).unwrap(), None); // version Kontakt never wrote
        assert_eq!(EffectParameters::read(0x10, 0x99, &[0; 40]).unwrap(), None);
    }

    #[test]
    fn effect_storage_preserves_kind_version_float_bits_and_unassigned_bytes() {
        // Own storage fixture only: this does not establish Tape's physical laws.
        let words = [0x8000_0000u32, 0x7fc1_2345, 0x7f80_0000];
        let mut bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        bytes.push(0xa5);
        let original = bytes.clone();
        let parsed = EffectParameters::read(0x42, 0x10, &bytes).unwrap().unwrap();
        assert_eq!((parsed.module, parsed.version), (0x42, 0x10));
        for (field, expected) in parsed.fields[..3].iter().zip(words) {
            let EffectValue::Float(value) = &field.value else {
                panic!("float storage changed type")
            };
            assert_eq!(value.to_bits(), expected);
        }
        assert_eq!(parsed.fields[3].value, EffectValue::Byte(0xa5));
        assert_eq!(bytes, original);
        assert!(EffectParameters::read(0xfe, 0x10, &bytes).unwrap().is_none());
        assert_eq!(bytes, original, "an unknown kind must not mutate or alias the payload");
    }

    /// Ladder filter (kind 33) at v0x92: u32 type, a flag byte, the type again, then floats.
    /// Convolution (0x16): a u32 block size and two counted curves are plain native reads.
    #[test]
    fn ladder_and_convolution_do_not_reinterpret_integer_or_flag_bytes() {
        let mut bytes = 33i32.to_le_bytes().to_vec();
        bytes.push(0xa5);
        bytes.extend(33i32.to_le_bytes());
        for value in [0.25f32, 0.75, 0.5] {
            bytes.extend(value.to_le_bytes());
        }
        let p = EffectParameters::read(0x18, 0x92, &bytes).unwrap().unwrap();
        assert_eq!(p.fields[0].value, EffectValue::Integer(33));
        assert_eq!(p.fields[1].value, EffectValue::Byte(0xa5));
        assert_eq!(p.fields[2].value, EffectValue::Integer(33));
        assert_eq!(p.fields[4].value, EffectValue::Float(0.75));
        let mut ir = 1f32.to_le_bytes().to_vec();
        ir.extend(1024i32.to_le_bytes());
        ir.extend([0; 32]);
        ir.extend([0, 1, 0, 1, 0]);
        ir.extend([0; 8]);
        ir.extend((-1i32).to_le_bytes());
        let p = EffectParameters::read(0x16, 0x70, &ir).unwrap().unwrap();
        assert_eq!(p.fields[1].value, EffectValue::Integer(1024));
        assert_eq!(p.fields.last().unwrap().value, EffectValue::Integer(-1));
    }
}
