//! The plugin's editor in a window of its own, through the same windowing
//! code a host opens it with. `kontra-window state <instrument> <file>`
//! writes a state holding that instrument; `kontra-window --state <file>`
//! opens the editor on it; `kontra-window read <instrument> <name>` prints
//! one of its library's resources (`Resources/pictures/x.txt`), as the
//! editor reads it. `tools/kontra-window/drive.sh` drives it under
//! Xvfb with real pointer events.

use std::path::PathBuf;

use kontra_host::state::{Saved, SavedPart, VERSION};
use moose::core::export::PluginExport;
use moose::core::state::snapshot_plugin;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    if first.as_ref().is_some_and(|a| a == "read") {
        let (Some(path), Some(name)) = (args.next(), args.next()) else {
            return Err("usage: kontra-window read <instrument> <name>".into());
        };
        let bytes = kontra_host::ui::Library::open(std::path::Path::new(&path))
            .read(&name.to_string_lossy())
            .ok_or("no such resource")?;
        std::io::Write::write_all(&mut std::io::stdout(), &bytes)?;
        return Ok(());
    }
    if first.is_some_and(|a| a == "state") {
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
