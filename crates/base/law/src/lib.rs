//! Parameter laws: curves from a normalized knob position to a parameter value.
//!
//! A [`Law`] is evaluated on the control thread when a value arrives, never inside the
//! sample loop. Law files are TOML (see `README.md`), embedded with [`laws!`].

mod file;
mod law;
mod network;
mod trace;

pub use file::{LawError, LawSource, LawSpec, parse};
pub use law::{FormulaId, Law, ModuleLaw};
pub use network::{CombNetwork, HadamardNetwork, LINES as NETWORK_LINES, ReverbNetwork};
pub use trace::{Application, LawId, collect, record};

/// A sine-shaped crossfade, with an adjustable power and a normalized input.
#[must_use]
pub fn sine_fade(x: f32, power: f32) -> f32 {
    (std::f32::consts::FRAC_PI_2 * x.clamp(0.0, 1.0))
        .sin()
        .powf(power)
}

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
