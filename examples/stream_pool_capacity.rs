//! Numeric-only sizing census; no sample/script/UI data leaves RAM.
use std::path::Path;
fn main() {
    let path = std::env::args_os().nth(1).expect("instrument path");
    let source = sampler_kontakt::read(Path::new(&path)).unwrap();
    let i = source.instrument;
    println!(
        "{}",
        serde_json::json!({
            "voice_limit": i.voice_limit.map(|v| v.voices),
            "voice_group_limits": i.voice_limits.iter().map(|v| v.voices).collect::<Vec<_>>(),
            "zones": i.zones.len(), "assets": i.assets.len(), "groups": i.groups.len(),
        })
    );
}
