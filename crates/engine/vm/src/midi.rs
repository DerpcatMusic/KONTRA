//! The numbers `on midi_in` and `on note_controller` scripts see, in Kontakt
//! 8.13.1's numbering (measured in the native oracle with
//! `tools/kontakt-oracle/ksp_constants.py`; `crates/script/ksp/data/constants.tsv`
//! holds the same values and a test keeps the two equal).

/// `$NI_CB_TYPE_NOTE_CONTROLLER` and `$NI_CB_TYPE_MIDI_IN`.
pub const CB_NOTE_CONTROLLER: u8 = 16;
pub const CB_MIDI_IN: u8 = 9;

/// `$MIDI_COMMAND_*`.
pub const NOTE_OFF: u8 = 0x80;
pub const NOTE_ON: u8 = 0x90;
pub const POLY_AT: u8 = 0xA0;
pub const CC: u8 = 0xB0;
pub const PROGRAM_CHANGE: u8 = 0xC0;
pub const MONO_AT: u8 = 0xD0;
pub const PITCH_BEND: u8 = 0xE0;

/// `$NI_VL_TMPRO_STANDARD` and `$NI_VL_TMPRO_HQ`, the voice types of
/// `get_voice_limit` / `set_voice_limit`, and the limits Time Machine Pro
/// starts with (unprobed: tools/ksp-conformance case manual_voice_limit_default).
pub const VL_STANDARD: i32 = 0;
pub const VL_HQ: i32 = 1;
pub const VL_DEFAULT: [i32; 2] = [32, 16];
