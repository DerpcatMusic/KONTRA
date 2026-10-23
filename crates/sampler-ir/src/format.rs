//! Format registry contract: how a sample-format reader identifies itself.
//!
//! A format crate implements [`FormatModule`] and exposes one `static`. The
//! root plugin lists the enabled ones and asks them which files they claim;
//! it never names a format in a shared path. Loading stays on each format's
//! own API until its reader moves behind the shared IR.

use std::path::Path;

/// One sample format: identity plus the file extensions it owns.
pub trait FormatModule: Sync {
    /// Stable id for cargo features, settings and logs (`"kontakt"`).
    fn id(&self) -> &'static str;
    /// Display name.
    fn name(&self) -> &'static str;
    /// Version of the reader (its crate version).
    fn version(&self) -> &'static str;
    /// Extensions this format claims, without the dot. Case is ignored.
    fn extensions(&self) -> &'static [&'static str];
    /// True when the path has one of this format's extensions. No I/O.
    fn claims(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| self.extensions().iter().any(|x| x.eq_ignore_ascii_case(e)))
    }
}
