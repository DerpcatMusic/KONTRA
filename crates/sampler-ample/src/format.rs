//! Registry entry for the Ample format (see `sampler_ir::format`).

/// Format identity and claimed extensions.
pub struct Module;

/// Registry entry the root plugin lists when this format is enabled.
pub static FORMAT: Module = Module;

impl sampler_ir::format::FormatModule for Module {
    fn id(&self) -> &'static str {
        "ample"
    }
    fn name(&self) -> &'static str {
        "Ample"
    }
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["ascf"]
    }
}
