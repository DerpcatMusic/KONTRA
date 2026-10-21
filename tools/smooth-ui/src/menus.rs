//! The toolbar's two menus: the gear's settings and the help mark's pages.
//!
//! Both open under the toolbar at its right end and close on a press anywhere
//! else or Escape. The settings are the reference's own: they are the
//! interface's preferences, not parameters, so they live in [`Prefs`] and the
//! host keeps them between sessions.

use crate::skin::*;
use crate::theme;
use crate::tokens::{metric, radius, space, ty};
use mui::prelude::*;

/// Which menu is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Open {
    #[default]
    Closed,
    Settings,
    Help,
}

/// The interface size steps, XS to XL: the window's width in points, as the
/// reference's own settings read them. The height keeps the design's aspect.
pub const SIZE_NAMES: [&str; 5] = ["XS", "S", "M", "L", "XL"];
pub const SIZE_WIDTHS: [f64; 5] = [395.5, 508.5, 621.5, 791.0, 1017.0];
/// The step a new instance opens at: L, the nearest to the design's own width.
pub const SIZE_DEFAULT: usize = 3;

/// The interface's preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub tooltips: bool,
    pub frequency_near_cursor: bool,
    pub always_ultra: bool,
    pub low_latency_all: bool,
    pub listen_on_drag: bool,
    pub listen_delta: bool,
    /// The interface size step, an index into [`SIZE_WIDTHS`].
    pub size: usize,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            tooltips: true,
            frequency_near_cursor: false,
            always_ultra: false,
            low_latency_all: false,
            listen_on_drag: false,
            listen_delta: false,
            size: SIZE_DEFAULT,
        }
    }
}

/// The id the menu's panel carries, for the press-outside test.
const PANEL_ID: &str = "menu-panel";

/// The band menu's panel, for the press-outside test.
pub const BAND_MENU_ID: &str = "band-menu-panel";

/// The most a band menu can be tall, with its shape list open: the window
/// clamps it to stay on screen.
pub const BAND_MENU_H: f64 = 320.0;

/// A band's context menu: which band, and the window point it opened at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandMenu {
    pub index: usize,
    pub at: (f64, f64),
    /// Whether the shape list under "shape" is showing.
    pub shapes: bool,
}

/// What the band menu asked for. The view does delete and reset itself; the
/// rest are host edits the plug-in makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandAsk {
    Delete,
    Reset,
    Bypass,
    Solo,
    Shape(usize),
}

/// The band's state the menu words its rows by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BandState {
    pub bypassed: bool,
    pub soloed: bool,
    pub shape: usize,
}

/// Where a menu of `size` opens at `at` in a window of `(w, h)`, kept on screen.
pub fn clamped(at: (f64, f64), size: (f64, f64), window: (f64, f64)) -> (f64, f64) {
    (
        at.0.clamp(0.0, (window.0 - size.0).max(0.0)),
        at.1.clamp(0.0, (window.1 - size.1).max(0.0)),
    )
}

/// The band menu's panel, and the one ask the frame made (a press lands on
/// one row at most).
pub fn band_menu(ui: &mut Ui, menu: &mut BandMenu, state: BandState) -> (El, Option<BandAsk>) {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let mut ask = None;
    let mut rows = Vec::new();
    let mut row = |ui: &mut Ui, id: &str, name: &str| -> bool {
        rows.push(action_pill(name).tipped(id).focusable().cursor(Cursor::Hand));
        ui.get(id).activated()
    };
    if row(ui, "band-menu-delete", "Delete band") {
        ask = Some(BandAsk::Delete);
    }
    let bypass = if state.bypassed { "Enable band" } else { "Bypass band" };
    if row(ui, "band-menu-bypass", bypass) {
        ask = Some(BandAsk::Bypass);
    }
    if row(ui, "band-menu-reset", "Reset band") {
        ask = Some(BandAsk::Reset);
    }
    let solo = if state.soloed { "Unsolo band" } else { "Solo band" };
    if row(ui, "band-menu-solo", solo) {
        ask = Some(BandAsk::Solo);
    }
    // The shape is a list of its own: the row opens it, and a pick closes the menu.
    if row(ui, "band-menu-shape", if menu.shapes { "Shape ▾" } else { "Shape ▸" }) {
        menu.shapes = !menu.shapes;
    }
    if menu.shapes {
        for (index, name) in crate::graph::SHAPES.iter().enumerate() {
            let id = format!("band-menu-shape-{index}");
            let mark = if index == state.shape { "• " } else { "    " };
            if row(ui, &id, &format!("{mark}{name}")) {
                ask = Some(BandAsk::Shape(index));
            }
        }
    }
    let panel = col(rows)
        .gap(space::XS)
        .pad(space::L)
        .w(metric::MENU_W)
        .radius(radius::BOX)
        .fill(ca(t.ground, 0.96))
        .align(Align::Start)
        .shrink(0)
        .id(BAND_MENU_ID);
    (panel, ask)
}

/// What the toolbar's two buttons and a press elsewhere do to the menu.
pub fn steer(ui: &Ui, open: &mut Open) {
    if ui.get("gear").activated() {
        *open = if *open == Open::Settings { Open::Closed } else { Open::Settings };
    } else if ui.get("help").activated() {
        *open = if *open == Open::Help { Open::Closed } else { Open::Help };
    } else if *open != Open::Closed && ui.dismissed(&[PANEL_ID, "gear", "help"]) {
        *open = Open::Closed;
    }
}

/// A line of the system information a bug report wants.
pub fn system_info(name: &str, version: &str) -> String {
    format!(
        "{name} {version}\n{} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// The menu that is open, or nothing.
pub fn menu(ui: &mut Ui, open: Open, prefs: &mut Prefs) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let lines: Vec<El> = match open {
        Open::Closed => return block(space::NONE, space::NONE).shrink(0),
        Open::Settings => {
            let before = *prefs;
            let mut rows = Vec::new();
            for (id, name, value) in [
                ("pref-tooltips", "Show tooltips", &mut prefs.tooltips),
                ("pref-frequency", "Show frequency near cursor", &mut prefs.frequency_near_cursor),
                ("pref-ultra", "Always render on ultra", &mut prefs.always_ultra),
                ("pref-latency", "Low latency for all instances", &mut prefs.low_latency_all),
                ("pref-listen-drag", "Band listen: on node drag", &mut prefs.listen_on_drag),
                ("pref-listen-delta", "Band listen: use delta", &mut prefs.listen_delta),
            ] {
                rows.push(toggle_pill(ui, id, name, value));
            }
            // The interface size: five steps, the chosen one lit.
            rows.push(segmented_at(ui, "pref-size", &SIZE_NAMES, &mut prefs.size, metric::MENU_W - space::L * 2.0));
            // The two render policies exclude each other, as the tooltips say.
            if prefs.always_ultra && !before.always_ultra {
                prefs.low_latency_all = false;
            }
            if prefs.low_latency_all && !before.low_latency_all {
                prefs.always_ultra = false;
            }
            rows
        }
        Open::Help => {
            if ui.get("help-copy").clicked {
                ui.set_clipboard(system_info(WORDMARK, VERSION));
            }
            vec![
                action_pill("Copy system info").tipped("help-copy"),
                label(
                    "Built with MUI, vello, rustfft and the moose plug-in framework.",
                    ty::FINE,
                    c(t.ink_quiet),
                ),
            ]
        }
    };
    col(lines)
        .gap(space::S)
        .pad(space::L)
        .w(metric::MENU_W)
        .radius(radius::BOX)
        .fill(ca(t.ground, 0.96))
        .align(Align::Start)
        .shrink(0)
        .id(PANEL_ID)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_menu_opened_near_the_edge_stays_on_the_window() {
        let window = (800.0, 473.0);
        assert_eq!(clamped((100.0, 50.0), (230.0, 320.0), window), (100.0, 50.0));
        assert_eq!(clamped((790.0, 400.0), (230.0, 320.0), window), (570.0, 153.0));
        assert_eq!(clamped((-5.0, -5.0), (230.0, 320.0), window), (0.0, 0.0));
    }

    #[test]
    fn the_size_steps_are_the_reference_widths_and_grow() {
        assert_eq!(SIZE_NAMES.len(), SIZE_WIDTHS.len());
        assert!(SIZE_WIDTHS.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(SIZE_WIDTHS[SIZE_DEFAULT], 791.0);
        assert_eq!(Prefs::default().size, SIZE_DEFAULT);
    }
}
