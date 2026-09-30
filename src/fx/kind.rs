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

impl Kind {
    pub fn from_ser_id(id: u16) -> Self {
        TABLE
            .iter()
            .find(|(ser, ..)| *ser == id)
            .map_or(Kind::Unknown(id), |(_, kind, _)| *kind)
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
