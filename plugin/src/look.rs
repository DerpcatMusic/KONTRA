//! The editor's view state as the project keeps it ([`SavedView`]): put
//! back when the editor opens, saved again whenever it changes.

use kontra_host::state::{SavedPartView, SavedView};
use kontra_ui::app::{App, Format, PartTab, Shelf, Tab, View};

const TABS: [Tab; 3] = [Tab::Rack, Tab::Mixer, Tab::Report];
const PART_TABS: [PartTab; 6] = [
    PartTab::Interface,
    PartTab::Articulations,
    PartTab::Mapping,
    PartTab::Sound,
    PartTab::Info,
    PartTab::Scripts,
];
const FORMATS: [Format; 2] = [Format::Kontakt, Format::Uvi];
const SHELVES: [Shelf; 3] = [Shelf::All, Shelf::Favorites, Shelf::Recent];

fn index<T: PartialEq>(all: &[T], x: &T) -> u8 {
    all.iter()
        .position(|a| a == x)
        .and_then(|n| u8::try_from(n).ok())
        .unwrap_or(0)
}

fn pick<T: Copy + Default>(all: &[T], n: u8) -> T {
    all.get(usize::from(n)).copied().unwrap_or_default()
}

/// What `app` shows, its parts in rack `slots`.
pub fn saved(app: &App, slots: &[usize]) -> SavedView {
    let v = &app.view;
    SavedView {
        tab: index(&TABS, &v.tab),
        browser_hidden: v.browser_hidden,
        keyboard_hidden: v.keyboard_hidden,
        keys_hidden: v.keys_hidden,
        mixer_wide: v.mixer_wide,
        octave: v.octave,
        scale: u8::try_from(v.scale).unwrap_or(0),
        face_scale: u8::try_from(v.face_scale).unwrap_or(0),
        format: index(&FORMATS, &v.format),
        shelf: index(&SHELVES, &v.shelf),
        descending: v.descending,
        parts: (app.parts.iter().zip(slots))
            .map(|(p, &part)| SavedPartView {
                part,
                open: p.open,
                tab: index(&PART_TABS, &p.tab),
            })
            .collect(),
    }
}

/// `app`'s view as `s` saved it; the parts are put back by [`parts`] as
/// they load.
pub fn apply(app: &mut App, s: &SavedView) {
    app.view = View {
        tab: pick(&TABS, s.tab),
        browser_hidden: s.browser_hidden,
        keyboard_hidden: s.keyboard_hidden,
        keys_hidden: s.keys_hidden,
        mixer_wide: s.mixer_wide,
        octave: s.octave,
        scale: usize::from(s.scale),
        face_scale: usize::from(s.face_scale),
        format: pick(&FORMATS, s.format),
        shelf: pick(&SHELVES, s.shelf),
        descending: s.descending,
        ..View::default()
    };
}

/// Puts back how each part in `waiting` sat, once it is in `app` (in rack
/// `slots`); what was put back leaves `waiting`.
pub fn parts(app: &mut App, slots: &[usize], waiting: &mut Vec<SavedPartView>) {
    waiting.retain(|w| {
        let Some(p) = (slots.iter().position(|&s| s == w.part)).and_then(|n| app.parts.get_mut(n))
        else {
            return true;
        };
        p.open = w.open;
        p.tab = pick(&PART_TABS, w.tab);
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_comes_back_as_it_was_left() {
        let mut app = App::default();
        app.view.tab = Tab::Mixer;
        app.view.browser_hidden = true;
        app.view.octave = -1;
        app.view.scale = 2;
        app.parts = vec![kontra_ui::app::Part {
            open: false,
            tab: PartTab::Mapping,
            ..Default::default()
        }];
        let saved = SavedView::read(&saved(&app, &[3]).write());
        let mut again = App::default();
        apply(&mut again, &saved);
        assert_eq!(
            (again.view.tab, again.view.browser_hidden),
            (Tab::Mixer, true)
        );
        assert_eq!((again.view.octave, again.view.scale), (-1, 2));
        let mut waiting = saved.parts.clone();
        again.parts = vec![kontra_ui::app::Part::default()];
        parts(&mut again, &[0], &mut waiting);
        assert_eq!(waiting.len(), 1, "slot 3 has not loaded yet");
        parts(&mut again, &[3], &mut waiting);
        assert!(waiting.is_empty());
        assert_eq!(
            (again.parts[0].open, again.parts[0].tab),
            (false, PartTab::Mapping)
        );
    }
}
