#![allow(dead_code, reason = "each test binary uses only some of these")]
//! Headless editor drivers shared by the test binaries.

use kontra_ui::app::{self, Action, App};
use mui::prelude::{Buttons, Input, Point, PointerInput, Size, Ui};

pub fn frame(ui: &mut Ui, app: &mut App, input: Input) -> (Vec<Action>, bool) {
    let r = app::view(ui, app);
    let f = ui.frame(r.el, Some(Size::new(900., 600.)), input, 1. / 60.);
    (r.changed, f.is_ok_and(|f| f.animating))
}

/// Frames until nothing moves; the actions they carried (a click lands the
/// frame after its release).
pub fn settle(ui: &mut Ui, app: &mut App) -> Vec<Action> {
    let mut out = Vec::new();
    for _ in 0..240 {
        let (actions, moving) = frame(ui, app, Input::default());
        out.extend(actions);
        if !moving {
            break;
        }
    }
    out
}

pub fn click(ui: &mut Ui, app: &mut App, id: &str) -> Vec<Action> {
    clicks(ui, app, id, 1)
}

/// `n` clicks on `id` in quick succession (2: a double-click).
pub fn clicks(ui: &mut Ui, app: &mut App, id: &str, n: usize) -> Vec<Action> {
    // A missing target clicks nothing, and the asserts after it say so.
    let Some(s) = ui.scene().and_then(|s| s.surface(id)).map(|s| s.frame) else {
        return Vec::new();
    };
    let at = Point::new(s.x + s.size.width / 2., s.y + s.size.height / 2.);
    let mut out = Vec::new();
    let presses = [Buttons::PRIMARY, Buttons::default()].repeat(n);
    for buttons in presses {
        let pointer = PointerInput {
            pos: Some(at),
            buttons,
            ..Default::default()
        };
        out.extend(
            frame(
                ui,
                app,
                Input {
                    pointer,
                    ..Default::default()
                },
            )
            .0,
        );
    }
    out.extend(settle(ui, app));
    out
}

pub fn about_app() -> App {
    App {
        folders: vec!["/music/libraries".into()],
        build: app::Build {
            version: "0.6.24",
            revision: "abc123def",
            built: "2026-11-24 12:00 UTC",
            target: "x86_64-unknown-linux-gnu",
            formats: "CLAP · VST3",
            changelog: include_str!("../../../../../CHANGELOG.md"),
        },
        ..App::default()
    }
}
