//! Sample store: preloads, streaming and the I/O thread (05-STREAM.md).
//!
//! One mechanism serves both: a sample is cut into [`PAGE_FRAMES`]-frame pages.
//! The preload law pins pages at load; every other page streams through one
//! shared cache. The audio thread reads lock-free and never waits: a page not in
//! memory plays as silence and is counted, so a voice keeps its timeline (rule 4).

mod cache;
mod file;
mod sniff;
mod fmt;
mod sample;
mod source;
mod store;

pub use file::open;
pub use fmt::{Encoding, SampleFmt};
pub use sample::{PAGE_FRAMES, Read, Sample, SampleId};
pub use source::{MemSource, Source};
pub use store::{BlobKey, LoadRequest, Store, StoreConfig, StreamClient, StreamStats};
