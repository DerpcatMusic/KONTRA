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

use kontra_host::rack::{Event, Rack, RackAudio};
use kontra_stream::{Store, StoreConfig};
use kontra_ui_model::{Kind, MenuItem, UiModifiers, Value};

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

/// The preset menu and the name field on the plate.
fn discover(rack: &Rack) -> (usize, usize) {
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
}

/// What the menu and the name field show, and an entry a few rows from the
/// one the menu shows: a pick that only re-selects the current row cannot
/// pass (12 rows on, and not a header).
fn away_from(rack: &Rack, menu: usize, edit: usize) -> (Option<Value>, String, MenuItem) {
    let part = rack.parts()[0].as_ref().expect("part 0");
    let tree = part.view.as_ref().expect("an interface").tree();
    let Kind::Menu { items } = &tree.widgets[menu].kind else {
        panic!("not a menu");
    };
    let was = tree.widgets[menu].value.clone();
    let was_text = tree.widgets[edit].text.clone();
    let shown: Vec<&MenuItem> = (items.iter())
        .filter(|i| i.visible && !i.text.trim_start().starts_with('-'))
        .collect();
    let current = shown
        .iter()
        .position(|i| Some(Value::Integer(i.value)) == was);
    let at = current.map_or(6, |n| (n + 12) % shown.len());
    let picked = shown[at].clone();
    assert_ne!(
        Some(Value::Integer(picked.value)),
        was,
        "the pick is a different entry"
    );
    (was, was_text, picked)
}

/// What the menu and the name field show.
fn shows(rack: &Rack, menu: usize, edit: usize) -> (Option<Value>, String) {
    let part = rack.parts()[0].as_ref().expect("part 0");
    let tree = part.view.as_ref().expect("an interface").tree();
    (
        tree.widgets[menu].value.clone(),
        tree.widgets[edit].text.clone(),
    )
}

/// A pick applied where the host renders nothing: no audio process call at
/// all, only the control tick the plugin runs.
///
/// That is the state a stopped host leaves the plugin in, and the one the
/// report came from: `RackAudio` carries the queued commands out inside
/// `process`, which nothing calls, so before the fix the pick sat in the
/// ring while the editor showed the old value. The look frame the tree is
/// built from is read out of the engine on the audio side, so a tree that
/// shows the pick is the engine's own value: the control side ran it.
#[test]
fn a_menu_pick_applies_with_no_block_ever_rendered() {
    let Some(path) = instrument() else { return };
    let store = Store::new(StoreConfig::DEFAULT).expect("a store");
    let (mut rack, audio) = Rack::new(RATE, store, BLOCK);
    rack.load(0, &path).expect("the instrument loads");

    let (menu, edit) = discover(&rack);
    let (was, was_text, picked) = away_from(&rack, menu, edit);
    rack.control(
        0,
        menu,
        None,
        f64::from(picked.value),
        UiModifiers::default(),
    );
    // One tick, the way `Tick` runs it: no block, before or after.
    rack.tick();

    assert!(
        rack.stats().is_none(),
        "no block was rendered, so the audio side published no stats"
    );
    assert_eq!(audio.missed(), 0, "no block was rendered to miss");
    let (value, text) = shows(&rack, menu, edit);
    assert_eq!(
        value,
        Some(Value::Integer(picked.value)),
        "the script kept {picked:?} (it was {was:?})"
    );
    assert_eq!(
        text.trim(),
        picked.text.trim(),
        "the name field did not follow (it was {was_text:?})"
    );
}

/// A full command ring: the edit waits its turn and is carried out, rather
/// than being dropped into the void or stuck behind the ring for ever.
#[test]
fn a_full_command_ring_keeps_the_edit_and_carries_it_out() {
    let Some(path) = instrument() else { return };
    let store = Store::new(StoreConfig::DEFAULT).expect("a store");
    let (mut rack, audio) = Rack::new(RATE, store, BLOCK);
    rack.load(0, &path).expect("the instrument loads");

    // `Rack::new`'s command ring holds 256: fill it past the brim, so the
    // pick has to be kept and sent again.
    for _ in 0..300 {
        rack.play(Event::midi1(0x90, 60, 100));
    }
    let (menu, edit) = discover(&rack);
    let (was, was_text, picked) = away_from(&rack, menu, edit);
    rack.control(
        0,
        menu,
        None,
        f64::from(picked.value),
        UiModifiers::default(),
    );
    // One tick, the way the plugin runs it with no host rendering: it drains
    // the ring, then must send the kept edit again in its next pass rather
    // than leave it pending for a tick that may never come.
    rack.tick();

    let (value, text) = shows(&rack, menu, edit);
    assert_eq!(
        value,
        Some(Value::Integer(picked.value)),
        "the edit was lost behind a full ring ({picked:?}, was {was:?})"
    );
    assert_eq!(text.trim(), picked.text.trim(), "was {was_text:?}");
    assert!(rack.stats().is_none(), "still no block was rendered");
    assert_eq!(audio.missed(), 0);
}

#[test]
fn a_menu_picked_through_rack_control_reaches_the_script_and_the_name_field() {
    let Some(path) = instrument() else { return };
    let store = Store::new(StoreConfig::DEFAULT).expect("a store");
    let (mut rack, mut audio) = Rack::new(RATE, store, BLOCK);
    rack.load(0, &path).expect("the instrument loads");
    settle(&mut rack, &mut audio);

    let (menu, edit) = discover(&rack);

    let (was, was_text, picked) = away_from(&rack, menu, edit);
    rack.control(
        0,
        menu,
        None,
        f64::from(picked.value),
        UiModifiers::default(),
    );
    settle(&mut rack, &mut audio);

    // The host renders, so the audio side carries the commands out and the
    // control tick leaves it alone: this is the other direction.
    assert!(rack.stats().is_some(), "a block was rendered");
    assert_eq!(audio.missed(), 0, "no block was taken from the audio side");
    let part = rack.parts()[0].as_ref().expect("part 0");
    let tree = part.view.as_ref().expect("an interface").tree();
    assert_eq!(
        tree.widgets[menu].value,
        Some(Value::Integer(picked.value)),
        "the script kept {picked:?} (it was {was:?})"
    );
    let name = picked.text.trim();
    assert!(
        tree.widgets[edit].text.trim() == name,
        "the name field reads {:?}, not {name:?} (it was {was_text:?})",
        tree.widgets[edit].text
    );
}
