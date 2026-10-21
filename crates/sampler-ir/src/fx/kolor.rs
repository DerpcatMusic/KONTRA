//! Kontakt Kolor (`BParFXKolor`, module 0x79): mode and control tables as data.
//!
//! No DSP is defined here; the topology is not recovered yet. Every constant
//! carries its evidence level so a consumer can tell what is established:
//!
//! - [`Evidence::Documented`]: named in the KSP reference constants.
//! - [`Evidence::Measured`]: read from the Kontakt 8 binary (its reader and
//!   name registrations, decompiled at `source/kontakt8`).
//! - [`Evidence::Unconfirmed`]: not yet tied to behaviour or to a stored field.

/// How well a Kolor constant is established.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    Documented,
    Measured,
    Unconfirmed,
}

/// A Kolor mode (`$NI_KOLOR_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    SolidConsole,
    AnalogAura,
    HarmonicCtrl,
    FullOn,
    SuperAmp,
    SmoothFuzz,
    Tape,
    Crunch,
    Saturaver,
    Diode,
}

/// Every mode with its selector (the reader's enum value, `Measured`) and its
/// KSP constant name (`Documented`). The reader rejects selectors above 9.
pub const MODES: [(Mode, u8, &str); 10] = [
    (Mode::SolidConsole, 0, "NI_KOLOR_SOLID_CONSOLE"),
    (Mode::AnalogAura, 1, "NI_KOLOR_ANALOG_AURA"),
    (Mode::HarmonicCtrl, 2, "NI_KOLOR_HARMONIC_CTRL"),
    (Mode::FullOn, 3, "NI_KOLOR_FULL_ON"),
    (Mode::SuperAmp, 4, "NI_KOLOR_SUPER_AMP"),
    (Mode::SmoothFuzz, 5, "NI_KOLOR_SMOOTH_FUZZ"),
    (Mode::Tape, 6, "NI_KOLOR_TAPE"),
    (Mode::Crunch, 7, "NI_KOLOR_CRUNCH"),
    (Mode::Saturaver, 8, "NI_KOLOR_SATURAVER"),
    (Mode::Diode, 9, "NI_KOLOR_DIODE"),
];

/// A Kolor knob: its KSP name, the engine-parameter id the binary registers for it
/// (`Measured`, `None` when only the KSP constant is known), and whether its stored
/// field has been tied to the name (`Unconfirmed` for every knob so far).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Control {
    pub name: &'static str,
    pub id: Option<u16>,
    pub evidence: Evidence,
}

/// The 13 `$ENGINE_PAR_KOLOR_*` knobs. The binary registers 11 floats under ids
/// 0xe4..=0xee in this order (drive, boost level, bass saver, lpf, hpf, bass,
/// mid, mid freq, mid q, treble, mix); its reader stores them in the same order
/// after the mode selector. Boost and Mode have no registration in that block.
pub const CONTROLS: [Control; 13] = [
    Control { name: "ENGINE_PAR_KOLOR_DRIVE", id: Some(0xe4), evidence: Evidence::Unconfirmed },
    Control { name: "ENGINE_PAR_KOLOR_BOOST_LEVEL", id: Some(0xe5), evidence: Evidence::Unconfirmed },
    Control { name: "ENGINE_PAR_KOLOR_BASS_SAVER", id: Some(0xe6), evidence: Evidence::Unconfirmed },
    Control { name: "ENGINE_PAR_KOLOR_LPF", id: Some(0xe7), evidence: Evidence::Unconfirmed },
    Control { name: "ENGINE_PAR_KOLOR_HPF", id: Some(0xe8), evidence: Evidence::Unconfirmed },
    Control { name: "ENGINE_PAR_KOLOR_BASS", id: Some(0xe9), evidence: Evidence::Unconfirmed },
    Control { name: "ENGINE_PAR_KOLOR_MID", id: Some(0xea), evidence: Evidence::Unconfirmed },
    Control { name: "ENGINE_PAR_KOLOR_MID_FREQ", id: Some(0xeb), evidence: Evidence::Unconfirmed },
    Control { name: "ENGINE_PAR_KOLOR_MID_Q", id: Some(0xec), evidence: Evidence::Unconfirmed },
    Control { name: "ENGINE_PAR_KOLOR_TREBLE", id: Some(0xed), evidence: Evidence::Unconfirmed },
    Control { name: "ENGINE_PAR_KOLOR_MIX", id: Some(0xee), evidence: Evidence::Unconfirmed },
    Control { name: "ENGINE_PAR_KOLOR_BOOST", id: None, evidence: Evidence::Documented },
    Control { name: "ENGINE_PAR_KOLOR_MODE", id: None, evidence: Evidence::Documented },
];

/// The saved record as the binary's reader reads it: the mode selector, then 11
/// floats, then one byte. The order is `Measured`; the float-to-knob tie is not.
pub const RECORD_FLOATS: usize = 11;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_complete_and_match_the_reader() {
        let mut names: Vec<&str> = CONTROLS.iter().map(|c| c.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 13);
        // Selectors are the reader's 0..=9, each once.
        let mut selectors: Vec<u8> = MODES.iter().map(|(_, s, _)| *s).collect();
        selectors.sort_unstable();
        assert_eq!(selectors, (0..10).collect::<Vec<u8>>());
        // The 11 registered floats are contiguous ids, matching the reader's float count.
        let ids: Vec<u16> = CONTROLS.iter().filter_map(|c| c.id).collect();
        assert_eq!(ids, (0xe4..=0xee).collect::<Vec<u16>>());
        assert_eq!(ids.len(), RECORD_FLOATS);
    }
}
