//! Wiring between front ends, formats and the engine (07-APP.md). Each piece
//! here only adapts one crate's output to another's input.

#[cfg(feature = "align")]
pub mod align;
pub mod browser;
pub mod instrument;
pub use kontra_face::perf_view;
pub mod persist;
pub mod presets;
pub mod rack;
pub mod relink;
pub mod script;
pub mod sound;
pub mod state;
pub use kontra_face as ui;
/// The picture a UVI bank shows in the browser.
pub use kontra_format_uvi::cover as bank_cover;
pub mod view;

mod error {
    #![allow(clippy::disallowed_types)]

    /// Structured error returned by host instrument loading and rack operations.
    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error(transparent)]
        Format(#[from] kontra_format::FormatError),
        #[error(transparent)]
        Kontakt(#[from] kontra_format_kontakt::Error),
        #[error(transparent)]
        Uvi(#[from] kontra_format_uvi::Error),
        #[error("part {part} is past the rack's {max}")]
        PartOutOfRange { part: usize, max: usize },
        #[error("every part is in use")]
        NoFreePart,
        #[error("multi load failed: {0}")]
        Multi(String),
        #[error("a Kontakt snapshot saves over the snapshot its part loaded")]
        NotSnapshotPart,
    }
}

pub use error::Error;
