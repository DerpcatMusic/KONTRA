//! Parameter laws: curves from a normalized knob position to a parameter value.
//!
//! A [`Law`] is evaluated on the control thread when a value arrives, never inside the
//! sample loop. Law files are TOML (see `README.md`), embedded with [`laws!`].

pub mod common;
pub mod kontakt;
pub mod uvi;

mod catalog;
mod file;
mod formula;
mod law;
mod network;
mod real;
mod trace;

pub use catalog::{Named, catalog};
pub use file::{LawError, LawSource, LawSpec, parse};
pub use formula::FormulaId;
pub use law::{Law, ModuleLaw};
pub use network::{CombNetwork, HadamardNetwork, LINES as NETWORK_LINES, ReverbNetwork};
pub use trace::{Application, LawId, collect, record};

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
                id: $crate::LawId(concat!(module_path!(), "::", stringify!($name))),
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
