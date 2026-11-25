//! Hover and popups snap: the frames that change pixels after a hover or a
//! click (MUI's scheduler, the CPU presenter's damage) end within two ticks,
//! where tweens and slides kept the library grid and the menu redrawing for
//! half a second (34 frames).

use std::time::{Duration, Instant};

use kontra_ui::app::{self, App, Library};
use mui::Clipboard;
use mui::host::{Driver, Shared, View};
use mui::input::{Button, Mods};
use mui::prelude::{El, Input, Point, Ui};
use mui::vello::kurbo::Affine;
use mui::vello::software::Renderer;

struct NoClipboard;
impl Clipboard for NoClipboard {
    fn get(&mut self) -> Option<String> {
        None
    }
    fn set(&mut self, _: &str) {}
}

struct Editor(App);
impl View for Editor {
    fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
        app::view(ui, &mut self.0).el
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
}

struct Window {
    driver: Driver,
    shared: Shared<Editor>,
    cpu: Renderer,
    now: Instant,
}

#[cfg(test)]
impl Window {
    fn new() -> Self {
        let libraries = (0..12)
            .map(|n| Library {
                name: format!("Library {n}"),
                presets: vec!["A".into()],
                ..Library::default()
            })
            .collect();
        let mut w = Self {
            driver: Driver::new((1180, 900), 1.0, Box::new(NoClipboard)),
            shared: Shared {
                ui: kontra_ui::ui(),
                view: Editor(App {
                    libraries,
                    ..App::default()
                }),
            },
            cpu: Renderer::new((1180, 900)).unwrap(),
            now: Instant::now(),
        };
        w.driver
            .pointer_moved(Point::new(700., 450.), Mods::default());
        w.ticks(120);
        w
    }

    /// 60 Hz ticks; the last one (1-based) whose frame changed pixels, or 0.
    fn ticks(&mut self, n: usize) -> usize {
        (1..=n).fold(0, |last, i| {
            self.now += Duration::from_micros(16_667);
            if !self.driver.advance(&mut self.shared, self.now) {
                return last;
            }
            let scene = self.shared.ui.scene().unwrap();
            let damage = self.cpu.render_damage(scene, Affine::IDENTITY).unwrap();
            if damage.is_empty() { last } else { i }
        })
    }

    fn center(&self, id: &str) -> Point {
        let s = self.shared.ui.scene().and_then(|s| s.surface(id));
        let s = s.unwrap_or_else(|| panic!("no {id} on screen"));
        Point::new(
            s.frame.x + s.frame.size.width / 2.,
            s.frame.y + s.frame.size.height / 2.,
        )
    }

    fn click(&mut self, id: &str) -> usize {
        self.driver.pointer_moved(self.center(id), Mods::default());
        self.ticks(1);
        self.driver.button(Button::Primary, true, Mods::default());
        self.ticks(1);
        self.driver.button(Button::Primary, false, Mods::default());
        self.ticks(120)
    }

    fn shown(&self, id: &str) -> bool {
        self.shared.ui.scene().and_then(|s| s.surface(id)).is_some()
    }
}

#[test]
fn hovering_a_library_tile_settles_at_once() {
    let mut w = Window::new();
    let tile = w.center("library-0");
    w.driver.pointer_moved(tile, Mods::default());
    assert!(w.ticks(120) <= 2, "a tile hover keeps drawing");
    w.driver
        .pointer_moved(Point::new(700., 450.), Mods::default());
    assert!(w.ticks(120) <= 2, "a tile unhover keeps drawing");
}

#[test]
fn the_menu_opens_and_closes_on_the_next_frame() {
    let mut w = Window::new();
    assert!(
        w.click("app-menu") <= 2,
        "the menu keeps moving as it opens"
    );
    assert!(w.shown("app-menu-popup"));
    assert!(
        w.click("app-menu") <= 2,
        "the menu keeps moving as it closes"
    );
    assert!(!w.shown("app-menu-popup"));
}
