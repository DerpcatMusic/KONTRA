use serde::{Deserialize, Serialize};

/// Effect type, keyed by the effect object's serialization ID (the child of a
/// `BParFX` slot). Names follow Kontakt's `BParFX*` class names; see
/// `audits/EFFECTS.md` for how each was identified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Kind {
    Delay,
    Chorus,
    Flanger,
    Gainer,
    Phaser,
    LegacyReverb,
    Convolution,
    SendLevels,
    Filter,
    Compressor,
    Inverter,
    Dyx,
    Limiter,
    SurroundPanner,
    Distortion,
    StereoModeller,
    LoFi,
    Skreamer,
    Rotator,
    Twang,
    Cabinet,
    TapeSaturator,
    TransientMaster,
    SolidGeq,
    SolidBusComp,
    FeedbackCompressor,
    Jump,
    Van51,
    AcBox,
    HotSolo,
    Cat,
    DStortion,
    PlateReverb,
    CryWah,
    Reverb,
    ReplikaDelay,
    Phasis,
    Flair,
    Choral,
    CoreCell,
    HilbertLimiter,
    Supercharger,
    BassPro,
    PsycheDelay,
    RingModulator,
    Unknown(u16),
}

pub(super) const TABLE: &[(u16, Kind, &str)] = &[
    (0x10, Kind::Delay, "Delay (legacy)"),
    (0x11, Kind::Chorus, "Chorus (legacy)"),
    (0x12, Kind::Flanger, "Flanger (legacy)"),
    (0x13, Kind::Gainer, "Gainer"),
    (0x14, Kind::Phaser, "Phaser (legacy)"),
    (0x15, Kind::LegacyReverb, "Reverb (legacy)"),
    (0x16, Kind::Convolution, "Convolution"),
    (0x17, Kind::SendLevels, "Send Levels"),
    (0x18, Kind::Filter, "Filter"),
    (0x19, Kind::Compressor, "Compressor"),
    (0x1a, Kind::Inverter, "Inverter"),
    (0x1b, Kind::Dyx, "DYX"),
    (0x1c, Kind::Limiter, "Limiter"),
    (0x1d, Kind::SurroundPanner, "Surround Panner"),
    (0x1e, Kind::Distortion, "Distortion"),
    (0x1f, Kind::StereoModeller, "Stereo Modeller"),
    (0x20, Kind::LoFi, "Lo-Fi"),
    (0x21, Kind::Skreamer, "Skreamer"),
    (0x22, Kind::Rotator, "Rotator"),
    (0x23, Kind::Twang, "Twang"),
    (0x24, Kind::Cabinet, "Cabinet"),
    (0x42, Kind::TapeSaturator, "Tape Saturator"),
    (0x43, Kind::TransientMaster, "Transient Master"),
    (0x44, Kind::SolidGeq, "Solid G-EQ"),
    (0x46, Kind::SolidBusComp, "Solid Bus Comp"),
    (0x4c, Kind::FeedbackCompressor, "Feedback Compressor"),
    (0x4d, Kind::Jump, "Jump"),
    (0x52, Kind::Van51, "Van51"),
    (0x53, Kind::AcBox, "AC Box"),
    (0x54, Kind::HotSolo, "Hot Solo"),
    (0x55, Kind::Cat, "Cat"),
    (0x56, Kind::DStortion, "DStortion"),
    (0x57, Kind::PlateReverb, "Plate Reverb"),
    (0x58, Kind::CryWah, "Cry Wah"),
    (0x59, Kind::Reverb, "Reverb"),
    (0x5a, Kind::ReplikaDelay, "Replika"),
    (0x5b, Kind::Phasis, "Phasis"),
    (0x5c, Kind::Flair, "Flair"),
    (0x5d, Kind::Choral, "Choral"),
    (0x5e, Kind::CoreCell, "Core Cell"),
    (0x5f, Kind::HilbertLimiter, "Hilbert Limiter"),
    (0x60, Kind::Supercharger, "Supercharger"),
    (0x61, Kind::BassPro, "Bass Pro"),
    (0x63, Kind::PsycheDelay, "Psyche Delay"),
    (0x64, Kind::RingModulator, "Ring Modulator"),
];

/// KSP `$EFFECT_TYPE_*` names by serialization ID, which is the value the
/// constants take here (Kontakt's own values are opaque to scripts).
const KSP_NAMES: &[(&str, u16)] = &[
    ("$EFFECT_TYPE_DELAY", 0x10),
    ("$EFFECT_TYPE_CHORUS", 0x11),
    ("$EFFECT_TYPE_FLANGER", 0x12),
    ("$EFFECT_TYPE_GAINER", 0x13),
    ("$EFFECT_TYPE_PHASER", 0x14),
    ("$EFFECT_TYPE_REVERB", 0x15),
    ("$EFFECT_TYPE_IRC", 0x16),
    ("$EFFECT_TYPE_SEND_LEVELS", 0x17),
    ("$EFFECT_TYPE_FILTER", 0x18),
    ("$EFFECT_TYPE_COMPRESSOR", 0x19),
    ("$EFFECT_TYPE_INVERTER", 0x1a),
    ("$EFFECT_TYPE_LIMITER", 0x1c),
    ("$EFFECT_TYPE_SURROUND_PANNER", 0x1d),
    ("$EFFECT_TYPE_DISTORTION", 0x1e),
    ("$EFFECT_TYPE_STEREO", 0x1f),
    ("$EFFECT_TYPE_LOFI", 0x20),
    ("$EFFECT_TYPE_SKREAMER", 0x21),
    ("$EFFECT_TYPE_ROTATOR", 0x22),
    ("$EFFECT_TYPE_TWANG", 0x23),
    ("$EFFECT_TYPE_CABINET", 0x24),
    ("$EFFECT_TYPE_TAPE_SAT", 0x42),
    ("$EFFECT_TYPE_TRANSIENT_MASTER", 0x43),
    ("$EFFECT_TYPE_SOLID_GEQ", 0x44),
    ("$EFFECT_TYPE_SOLID_BUS_COMP", 0x46),
    ("$EFFECT_TYPE_FB_COMP", 0x4c),
    ("$EFFECT_TYPE_JUMP", 0x4d),
    ("$EFFECT_TYPE_VAN51", 0x52),
    ("$EFFECT_TYPE_ACBOX", 0x53),
    ("$EFFECT_TYPE_HOT_SOLO", 0x54),
    ("$EFFECT_TYPE_CAT", 0x55),
    ("$EFFECT_TYPE_DSTORTION", 0x56),
    ("$EFFECT_TYPE_PLATE_REVERB", 0x57),
    ("$EFFECT_TYPE_CRY_WAH", 0x58),
    ("$EFFECT_TYPE_REVERB2", 0x59),
    ("$EFFECT_TYPE_REPLIKA", 0x5a),
    ("$EFFECT_TYPE_PHASIS", 0x5b),
    ("$EFFECT_TYPE_FLAIR", 0x5c),
    ("$EFFECT_TYPE_CHORAL", 0x5d),
    ("$EFFECT_TYPE_BASS_PRO", 0x61),
    ("$EFFECT_TYPE_PSYCHEDELAY", 0x63),
    ("$EFFECT_TYPE_RINGMOD", 0x64),
];

/// Value of a KSP `$EFFECT_TYPE_*` constant; `$EFFECT_TYPE_NONE` (an empty slot) is 0.
pub fn ksp_effect_type(name: &str) -> Option<i32> {
    if name == "$EFFECT_TYPE_NONE" {
        return Some(0);
    }
    KSP_NAMES.iter().find(|(n, _)| *n == name).map(|&(_, id)| i32::from(id))
}

impl Kind {
    /// Serialization ID, also this kind's `$EFFECT_TYPE_*` value.
    pub fn ser_id(self) -> u16 {
        match self {
            Kind::Unknown(id) => id,
            _ => TABLE.iter().find(|(_, k, _)| *k == self).map_or(0, |(id, ..)| *id),
        }
    }

    pub fn from_ser_id(id: u16) -> Self {
        TABLE
            .iter()
            .find(|(ser, ..)| *ser == id)
            .map_or(Kind::Unknown(id), |(_, kind, _)| *kind)
    }

    /// Whether a rack slot of this kind plays (see `processor::Dsp`).
    pub fn has_dsp(self) -> bool {
        matches!(self, Kind::Gainer | Kind::StereoModeller | Kind::Reverb | Kind::Convolution | Kind::SendLevels)
    }

    /// Kontakt's display name, for warnings and reports.
    pub fn name(self) -> String {
        match TABLE.iter().find(|(_, kind, _)| *kind == self) {
            Some((.., name)) => (*name).to_owned(),
            None => match self {
                Kind::Unknown(id) => format!("unknown effect 0x{id:02x}"),
                _ => unreachable!("every named kind is in TABLE"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for (id, kind, _) in TABLE {
            assert_eq!(Kind::from_ser_id(*id), *kind);
        }
        assert_eq!(Kind::from_ser_id(0x99), Kind::Unknown(0x99));
        assert_eq!(Kind::Unknown(0x99).name(), "unknown effect 0x99");
    }
}
