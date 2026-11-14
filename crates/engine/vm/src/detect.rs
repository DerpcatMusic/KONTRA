//! The `$NI_DETECT_*` tags `detect_*` writes, in the numbering Kontakt 8.13.1
//! uses (measured in the native oracle, `tools/kontakt-oracle/ksp_constants.py`;
//! `crates/script/ksp/data/constants.tsv` holds the same numbers and a test
//! keeps the two equal). Lists run in the manual's order from their first tag.

/// A tag no command writes but means "detection failed".
pub const KEY_INVALID: i32 = -1;
pub const SAMPLE_INVALID: i32 = -1;
pub const SAMPLE_INSTRUMENT: i32 = 0;
pub const SAMPLE_DRUM: i32 = 1;
pub const DRUM_INVALID: i32 = -1;
pub const INSTRUMENT_INVALID: i32 = -1;

/// What the real-valued commands give when detection fails
/// (`~NI_DETECT_*_INVALID`; unprobed: tools/ksp-conformance cases).
pub const INVALID_PITCH: f64 = -1.0;
pub const INVALID_LOUDNESS: f64 = -1.0;
pub const INVALID_PEAK: f64 = -1.0;
pub const INVALID_RMS: f64 = -1.0;
pub const INVALID_TEMPO: f64 = -1.0;

/// First tag of each list; `index` 0.. counts from the manual's first entry.
const KEY_FIRST: i32 = 0;
const DRUM_FIRST: i32 = 0;
const INSTRUMENT_FIRST: i32 = 0;

/// Tag of key `index`: 0..12 C..B major, 12..24 C..B minor.
#[must_use]
pub fn key(index: u8) -> i32 {
    KEY_FIRST + i32::from(index)
}

/// Tag of drum type `index` (kick first).
#[must_use]
pub fn drum(index: u8) -> i32 {
    DRUM_FIRST + i32::from(index)
}

/// Tag of instrument type `index` (bass first).
#[must_use]
pub fn instrument(index: u8) -> i32 {
    INSTRUMENT_FIRST + i32::from(index)
}
