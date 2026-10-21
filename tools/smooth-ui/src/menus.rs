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
    fn the_size_steps_are_the_reference_widths_and_grow() {
        assert_eq!(SIZE_NAMES.len(), SIZE_WIDTHS.len());
        assert!(SIZE_WIDTHS.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(SIZE_WIDTHS[SIZE_DEFAULT], 791.0);
        assert_eq!(Prefs::default().size, SIZE_DEFAULT);
    }
}
