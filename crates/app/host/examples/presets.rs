//! Preset survey: for each `<preset> <other preset>` pair, what the header
//! picker, the instrument's own preset controls and a project reload do.
//! `cargo run --release -p kontra-host --example presets -- <a> <b> [<a> <b>...]`
//!
//! Prints one line per pair: whether the header names what plays, whether
//! picking `b` recalled in place or loaded, how many saved values changed,
//! each own preset control moved and how many values it changed, and
//! whether a save and reload brings back the same values and name, and
//! how a fresh load of `b` differs from the pick. `SHOW_DIFF=1` prints the
//! values that differ.

use std::path::{Path, PathBuf};
use std::time::Instant;

use kontra_host::rack::{Rack, RackAudio, Store, StoreConfig};
use kontra_host::view::Interfaces;
use kontra_ui_model::{Kind, UiModifiers, Value};

const RATE: u32 = 48_000;
const BLOCK: usize = 512;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    for pair in args.chunks(2) {
        let [a, b] = pair else {
            return Err("pairs of presets".into());
        };
        match survey(a, b) {
            Ok(line) => println!("{line}"),
            Err(e) => println!("{}: FAIL {e}", a.display()),
        }
    }
    Ok(())
}

fn new_rack() -> Result<(Rack, RackAudio), Box<dyn std::error::Error>> {
    let (mut rack, audio) = Rack::new(RATE, Store::new(StoreConfig::DEFAULT)?, BLOCK);
    rack.show_interfaces(Interfaces::Shown);
    Ok((rack, audio))
}

/// About `seconds` of blocks, ticking the control side between them.
fn run(rack: &mut Rack, audio: &mut RackAudio, seconds: f64) {
    let blocks = (seconds * f64::from(RATE) / BLOCK as f64) as usize;
    for _ in 0..blocks.max(2) {
        rack.tick();
        audio.process(BLOCK, &[]);
    }
    rack.tick();
}

fn values(rack: &Rack) -> Vec<String> {
    rack.saved_scripts(0).concat()
}

fn changed(a: &[String], b: &[String]) -> usize {
    let differ = a.iter().zip(b).filter(|(x, y)| x != y).count();
    differ + a.len().abs_diff(b.len())
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn survey(a: &Path, b: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let (mut rack, mut audio) = new_rack()?;
    rack.set_saves(std::env::temp_dir().join("kontra-presets-survey"));
    rack.load(0, a)?;
    run(&mut rack, &mut audio, 1.0);
    let part = |r: &Rack| {
        r.parts()
            .first()
            .and_then(Option::as_ref)
            .map(|p| (p.name.clone(), p.script.generation(), p.plan.clone()))
    };
    let (name, _, plan) = part(&rack).ok_or("no part")?;
    let header = if name == stem(a) {
        "names it"
    } else {
        "MISSES"
    };
    let before = values(&rack);
    // The instrument's own preset controls: menus and buttons named so.
    let own = own_controls(&mut rack, &mut audio, &before);
    // The header picks b.
    let t = Instant::now();
    rack.load(0, b)?;
    let took = t.elapsed();
    run(&mut rack, &mut audio, 1.0);
    let (name_b, _, plan_b) = part(&rack).ok_or("no part after pick")?;
    let in_place = match (&plan, &plan_b) {
        (Some(x), Some(y)) if std::sync::Arc::ptr_eq(x, y) => "recalled in place",
        _ => "loaded",
    };
    let after = values(&rack);
    let header_b = if name_b == stem(b) {
        "names it"
    } else {
        "MISSES"
    };
    // Save, reload in a new rack.
    let saved = rack.save();
    let (mut again, mut audio2) = new_rack()?;
    let failed = again.restore(&saved);
    run(&mut again, &mut audio2, 1.0);
    let back = values(&again);
    if std::env::var_os("SHOW_DIFF").is_some() {
        for (x, y) in after.iter().zip(&back).filter(|(x, y)| x != y) {
            eprintln!("DIFF before reload: {x:.200}\n     after reload:  {y:.200}");
        }
    }
    let (mut fresh, mut audio3) = new_rack()?;
    fresh.load(0, b)?;
    run(&mut fresh, &mut audio3, 1.0);
    let clean = values(&fresh);
    if std::env::var_os("SHOW_DIFF").is_some() {
        for (x, y) in clean.iter().zip(&after).filter(|(x, y)| x != y) {
            eprintln!("FRESH: {x:.200}\n PICKED: {y:.200}");
        }
    }
    let fresh_vs = changed(&clean, &after);
    let fresh_vs_back = changed(&clean, &back);
    let name_back = part(&again).map(|p| p.0).unwrap_or_default();
    Ok(format!(
        "{} | header {header} ({name:?}) | pick {} → {in_place} in {} ms, header {header_b} ({name_b:?}), {} values changed | own: {own} | reload: {} values differ, name {:?}{} | fresh load of b vs picked {fresh_vs}, vs reloaded {fresh_vs_back}",
        stem(a),
        stem(b),
        took.as_millis(),
        changed(&before, &after),
        changed(&after, &back),
        name_back,
        if failed.is_empty() {
            String::new()
        } else {
            format!(" FAILED {failed:?}")
        },
    ))
}

/// Moves each preset menu one item on (and presses preset buttons), and
/// says how many saved values each move changed.
fn own_controls(rack: &mut Rack, audio: &mut RackAudio, before: &[String]) -> String {
    let found: Vec<(usize, String, f64)> = {
        let Some(view) = rack
            .parts()
            .first()
            .and_then(Option::as_ref)
            .and_then(|p| p.view.as_ref())
        else {
            return "no interface".into();
        };
        let tree = view.tree();
        (tree.widgets.iter().enumerate())
            .filter(|(_, w)| {
                let n = w.name.to_ascii_lowercase();
                n.contains("preset") || n.contains("snapshot")
            })
            .filter_map(|(n, w)| {
                let now = match w.value {
                    Some(Value::Integer(i)) => f64::from(i),
                    Some(Value::Real(r)) => r,
                    _ => w.initial_value,
                };
                let to = match &w.kind {
                    Kind::Menu { items } if items.len() > 1 => {
                        ((now.max(0.0) as usize + 2) % items.len()) as f64
                    }
                    Kind::Button { .. } | Kind::Switch => f64::from(u8::from(now < 0.5)),
                    _ => return None,
                };
                Some((n, w.name.clone(), to))
            })
            .take(12)
            .collect()
    };
    if found.is_empty() {
        return "none named preset".into();
    }
    let mut out = Vec::new();
    let mut last = before.to_vec();
    for (widget, name, to) in found {
        rack.control(0, widget, None, to, UiModifiers::default());
        run(rack, audio, 0.5);
        let now = values(rack);
        out.push(format!("{name}={}", changed(&last, &now)));
        last = now;
    }
    out.join(", ")
}
