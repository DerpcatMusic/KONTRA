//! UVI-only resumable census through the production native UI renderer.
#[allow(dead_code)]
mod survey {
    include!("../crates/sampler-uvi/examples/ui_health.rs");
    pub fn collect() -> Result<(), Box<dyn std::error::Error>> {
        run(|face, bank, program| {
            kontakto::uvi_ui_health(face, &std::path::Path::new(bank).join(program))
        })
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::panic::set_hook(Box::new(|_| {}));
    survey::collect()
}
