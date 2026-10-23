//! Sample formats compiled into this build. The root plugin asks this registry
//! which format claims a file; a format that is not compiled in is absent, and
//! no shared path names a format.

use sampler_ir::format::FormatModule;
use std::path::Path;

/// Every format compiled into this build.
pub fn enabled() -> Vec<&'static dyn FormatModule> {
    let mut formats: Vec<&'static dyn FormatModule> =
        vec![&sampler_kontakt::FORMAT, &sampler_uvi::FORMAT];
    #[cfg(feature = "format-ample")]
    formats.push(&sampler_ample::FORMAT);
    #[cfg(feature = "format-ikmpak")]
    formats.push(&sampler_ikmpak::FORMAT);
    formats
}

/// The enabled format that claims `path` by extension, if any.
pub fn claim(path: &Path) -> Option<&'static dyn FormatModule> {
    enabled().into_iter().find(|format| format.claims(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_formats_claim_one_extension() {
        let formats = enabled();
        for (i, a) in formats.iter().enumerate() {
            for b in &formats[i + 1..] {
                for ext in a.extensions() {
                    assert!(
                        !b.extensions().iter().any(|x| x.eq_ignore_ascii_case(ext)),
                        "{} and {} both claim .{ext}",
                        a.id(),
                        b.id()
                    );
                }
            }
        }
    }

    #[test]
    fn claims_by_extension_ignoring_case() {
        assert_eq!(
            claim(Path::new("Piano.NKI")).map(|f| f.id()),
            Some("kontakt")
        );
        assert_eq!(claim(Path::new("bank.uvip")).map(|f| f.id()), Some("uvi"));
        assert!(claim(Path::new("loop.wav")).is_none());
    }
}
