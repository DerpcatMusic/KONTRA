//! Parameter laws: curves from a normalized knob position to a parameter value.
//!
//! A [`Law`] is evaluated on the control thread when a value arrives, never inside the
//! sample loop. Law files are TOML (see `README.md`), embedded with [`laws!`].

mod file;
mod law;

pub use file::{LawError, LawSource, LawSpec, parse};
pub use law::{FormulaId, Law, ModuleLaw};

/// Declares embedded law files as `pub const` [`LawSource`]s and generates a test that
/// parses each one.
///
/// Paths are relative to the file that invokes the macro. Invoke it once per module: the
/// generated test is named `every_law_parses`.
///
/// ```text
/// kontra_law::laws!(
///     SOLID_BUS_COMP => "laws/vendor/solid_bus_comp.toml",
///     FILTER_CUTOFF => "laws/vendor/filter_cutoff.toml",
/// );
/// ```
#[macro_export]
macro_rules! laws {
    ($($name:ident => $path:literal),* $(,)?) => {
        $(
            #[doc = concat!("Law file `", $path, "`.")]
            pub const $name: $crate::LawSource = $crate::LawSource {
                path: $path,
                text: include_str!($path),
            };
        )*

        #[cfg(test)]
        #[test]
        fn every_law_parses() {
            $(
                if let Err(err) = $name.parse() {
                    panic!("{}: {err}", $name.path);
                }
            )*
        }
    };
}
