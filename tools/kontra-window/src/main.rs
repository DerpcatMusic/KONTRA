//! The plugin's editor in a window of its own, through the same windowing
//! code a host opens it with. `kontra-window state <instrument> <file>`
//! writes a state holding that instrument; `kontra-window --state <file>`
//! opens the editor on it. `tools/kontra-window/drive.sh` drives it under
//! Xvfb with real pointer events.

use std::path::PathBuf;

use kontra_host::state::{Saved, SavedPart, VERSION};
use moose::core::export::PluginExport;
use moose::core::state::snapshot_plugin;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    if args.next().is_some_and(|a| a == "state") {
        let (Some(path), Some(out)) = (args.next(), args.next()) else {
            return Err("usage: kontra-window state <instrument> <file>".into());
        };
        let plugin = kontra_plugin::Plugin::create();
        let saved = Saved {
            version: VERSION,
            parts: vec![SavedPart {
                part: 0,
                path: PathBuf::from(path),
                channel: 0,
                output: 0,
                gain: 1.,
                pan: 0.,
                solo: false,
                mute: false,
                scripts: Vec::new(),
                delays: None,
                slot_types: Vec::new(),
                slot_responses: Vec::new(),
            }],
        };
        let params = plugin.params_arc();
        if let Ok(mut s) = params.saved.write() {
            *s = saved.write();
        }
        std::fs::write(out, snapshot_plugin(&plugin))?;
        return Ok(());
    }
    moose_standalone::run::<kontra_plugin::Plugin>();
    Ok(())
}
