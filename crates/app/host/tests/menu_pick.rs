//! A script menu picked from the editor's side of the rack, at the real
//! instrument: `Rack::control` — what the plugin's editor calls for a click —
//! must reach the script, and the tree the editor draws must read the pick
//! back, both the control's value and the name field the script writes.
//!
//! Electric Mint's Sound Preset menu is the case that was reported as
//! "picking an entry does nothing": 158 entries, 79 shown at a time (one
//! style's group), the current one "Spacious EDM" at value 53.
#![cfg(test)]

use std::path::PathBuf;

use kontra_host::rack::{Rack, RackAudio};
use kontra_stream::{Store, StoreConfig};
use kontra_ui_model::{Kind, MenuItem, UiModifiers};

const RATE: u32 = 48_000;
const BLOCK: usize = 512;

fn instrument() -> Option<PathBuf> {
    const LIBRARY: &str = "Armory/VSTs/Kontakt1/Native Instruments Session Guitarist Electric Mint 1.1.0/Session Guitarist - Electric Mint Library/Instruments";
    let path = std::env::var_os("KONTRA_EMINT").map_or_else(
        || {
            std::env::var_os("HOME")
                .map_or_else(|| PathBuf::from("/home"), PathBuf::from)
                .join(LIBRARY)
                .join("Electric Mint.nki")
        },
        PathBuf::from,
    );
    if !path.exists() {
        eprintln!("skipped: no {}", path.display());
        return None;
    }
    Some(path)
}

/// Blocks and ticks, as the plugin's task runs them: the audio side applies
/// the commands, then the control side takes the look that comes back.
fn settle(rack: &mut Rack, audio: &mut RackAudio) {
    for _ in 0..6 {
        audio.process(BLOCK, &[]);
        rack.tick();
    }
}

#[test]
fn a_menu_picked_through_rack_control_reaches_the_script_and_the_name_field() {
    let Some(path) = instrument() else { return };
    let store = Store::new(StoreConfig::DEFAULT).expect("a store");
    let (mut rack, mut audio) = Rack::new(RATE, store, BLOCK);
    rack.load(0, &path).expect("the instrument loads");
    settle(&mut rack, &mut audio);

    let (menu, edit) = {
        let part = rack.parts()[0].as_ref().expect("part 0");
        let tree = part.view.as_ref().expect("an interface").tree();
        let menu = (tree.widgets.iter().enumerate())
            .filter(|(_, w)| matches!(&w.kind, Kind::Menu { items } if items.len() > 20))
            .max_by_key(|(_, w)| match &w.kind {
                Kind::Menu { items } => items.len(),
                _ => 0,
            })
            .map(|(n, _)| n)
            .expect("the preset menu");
        // The field on the plate, above the page's middle.
        let edit = (tree.widgets.iter().enumerate())
            .find(|(_, w)| matches!(w.kind, Kind::TextEdit) && w.rect.y < 60.)
            .map(|(n, _)| n)
            .expect("the name field");
        (menu, edit)
    };

    let (was, was_text, items) = {
        let part = rack.parts()[0].as_ref().expect("part 0");
        let tree = part.view.as_ref().expect("an interface").tree();
        let Kind::Menu { items } = &tree.widgets[menu].kind else {
            panic!("not a menu");
        };
        (
            tree.widgets[menu].value.clone(),
            tree.widgets[edit].text.clone(),
            items.clone(),
        )
    };
    // An entry a few rows from the one the menu shows, so a pick that only
    // re-selects the current row cannot pass: 12 rows on, and not a header.
    let shown: Vec<&MenuItem> = (items.iter())
        .filter(|i| i.visible && !i.text.trim_start().starts_with('-'))
        .collect();
    let current = shown
        .iter()
        .position(|i| Some(kontra_ui_model::Value::Integer(i.value)) == was);
    let at = current.map_or(6, |n| (n + 12) % shown.len());
    let picked = shown[at];
    assert_ne!(
        Some(kontra_ui_model::Value::Integer(picked.value)),
        was,
        "the pick is a different entry"
    );

    rack.control(
        0,
        menu,
        None,
        f64::from(picked.value),
        UiModifiers::default(),
    );
    settle(&mut rack, &mut audio);

    let part = rack.parts()[0].as_ref().expect("part 0");
    let tree = part.view.as_ref().expect("an interface").tree();
    assert_eq!(
        tree.widgets[menu].value,
        Some(kontra_ui_model::Value::Integer(picked.value)),
        "the script kept {picked:?} (it was {was:?})"
    );
    let name = picked.text.trim();
    assert!(
        tree.widgets[edit].text.trim() == name,
        "the name field reads {:?}, not {name:?} (it was {was_text:?})",
        tree.widgets[edit].text
    );
}
