//! Browser entry point for the same inspection implementation as corpus-health.
//! The worker provides only selected local files through a read-only filesystem.
#[path = "inspect.rs"]
mod inspect;

use std::{path::Path, sync::Arc};

fn bank(path: &Path) -> Result<Arc<sampler_uvi::Bank>, Arc<sampler_uvi::AccessError>> {
    sampler_uvi::Bank::open(path).map(Arc::new).map_err(Arc::new)
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut report = match inspect::report(&args) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    report["browser_engine"] = serde_json::json!({
        "revision": option_env!("KONTRA_WEB_REVISION").unwrap_or("development"),
        "target": "wasm32-unknown-emscripten",
        "scope": "Local read-only inspection; native audio playback and UI pixel equivalence are not tested"
    });
    let failed = report["status"] == "failed";
    println!("{}", serde_json::to_string(&report).unwrap());
    std::process::exit(if failed { 1 } else { 0 });
}
