//! The colour look is one per process (`kontra_ui::theme::set_look`: the
//! plugin loads the saved colours once for every editor). A test that
//! changes it must not share a process with tests that compare paint, so it
//! lives in its own test binary: `cargo test` runs test binaries one after
//! another but a binary's tests in parallel threads.

mod common;
use common::{about_app, click, settle};
use kontra_ui::app::{self, Action};

/// A preset picked in Settings applies on the next frame and asks the host
/// to save its tokens; Reset brings back the neutral default.
#[test]
fn a_theme_preset_applies_at_once_and_is_saved() {
    use kontra_ui::theme::{self, Look};
    let mut ui = kontra_ui::ui();
    let mut app = about_app();
    app.view.sheet = Some(app::Sheet::Settings);
    settle(&mut ui, &mut app);
    click(&mut ui, &mut app, "theme-preset");
    settle(&mut ui, &mut app);
    let mut actions = click(&mut ui, &mut app, "theme-preset/list/1");
    actions.extend(settle(&mut ui, &mut app));
    let midnight = Look::PRESETS[1].1;
    assert_eq!(theme::look(), midnight);
    assert_eq!(ui.theme().palette, midnight.theme().palette);
    assert!(
        actions.contains(&Action::Theme(midnight.tokens())),
        "{actions:?}"
    );
    click(&mut ui, &mut app, "theme-reset");
    settle(&mut ui, &mut app);
    assert_eq!(theme::look(), Look::GRAPHITE);
    assert!(ui.theme().palette.neutral.chroma.abs() < 1e-6);
}
