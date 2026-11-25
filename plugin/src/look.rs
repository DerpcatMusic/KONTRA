//! The editor's view state as the project keeps it ([`SavedView`]): put
//! back when the editor opens, saved again whenever it changes.

use kontra_host::state::{SavedPartView, SavedView};
use kontra_ui::app::{
    App, Format, LibraryFilter, LibrarySort, LibraryView, PartTab, Presentation, Shelf, Tab, Tiles,
    View,
};

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
const TILES: [Tiles; 2] = [Tiles::Grid, Tiles::Cards];
const SORTS: [LibrarySort; 4] = [
    LibrarySort::NameAsc,
    LibrarySort::NameDesc,
    LibrarySort::MostPresets,
    LibrarySort::FavoritesFirst,
];
const FILTERS: [LibraryFilter; 4] = [
    LibraryFilter::All,
    LibraryFilter::Playable,
    LibraryFilter::HasSnapshots,
    LibraryFilter::Locked,
];

fn index<T: PartialEq>(all: &[T], x: &T) -> u8 {
    all.iter()
        .position(|a| a == x)
        .and_then(|n| u8::try_from(n).ok())
        .unwrap_or(0)
}

fn pick<T: Copy + Default>(all: &[T], n: u8) -> T {
    all.get(usize::from(n)).copied().unwrap_or_default()
}

/// A look as saved: KONTRA keeps the index it had beside the retired
/// Vector look (1), so older builds still read it.
fn look_index(look: Presentation) -> u8 {
    match look {
        Presentation::Original => 0,
        Presentation::Kontra => 2,
    }
}

/// A saved look: anything but Original, the retired Vector look too, is
/// KONTRA.
fn look_of(n: u8) -> Presentation {
    if n == 0 {
        Presentation::Original
    } else {
        Presentation::Kontra
    }
}

/// A saved interface scale in percent; 1 to 3 are the steps 125 to 200 %
/// of before.
pub fn scale_of(saved: u8) -> u32 {
    match saved {
        n @ 1..=3 => [125, 150, 200]
            .get(usize::from(n) - 1)
            .copied()
            .unwrap_or(100),
        n => u32::from(n),
    }
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
        descending: v.sort == LibrarySort::NameDesc,
        separated: v.library_view == LibraryView::Separated,
        tiles: index(&TILES, &v.tiles),
        sort: index(&SORTS, &v.sort),
        library_filter: index(&FILTERS, &v.library_filter),
        split: v.split as f32,
        browser_width: v.browser_width as f32,
        look: look_index(v.face_look),
        parts: (app.parts.iter().zip(slots))
            .map(|(p, &part)| SavedPartView {
                part,
                open: p.open,
                tab: index(&PART_TABS, &p.tab),
                look: p.look.map_or(0, |l| look_index(l) + 1),
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
        scale: scale_of(s.scale),
        face_scale: usize::from(s.face_scale),
        library_view: if s.separated {
            LibraryView::Separated
        } else {
            LibraryView::Unified
        },
        format: pick(&FORMATS, s.format),
        shelf: pick(&SHELVES, s.shelf),
        tiles: pick(&TILES, s.tiles),
        sort: if s.sort == 0 && s.descending {
            LibrarySort::NameDesc
        } else {
            pick(&SORTS, s.sort)
        },
        library_filter: pick(&FILTERS, s.library_filter),
        split: f64::from(s.split),
        browser_width: f64::from(s.browser_width),
        face_look: look_of(s.look),
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
        p.look = w.look.checked_sub(1).map(look_of);
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
        app.view.scale = 75;
        app.view.split = 0.3;
        app.view.browser_width = 420.;
        app.view.face_look = Presentation::Kontra;
        app.view.library_view = LibraryView::Separated;
        app.view.tiles = Tiles::Cards;
        app.view.sort = LibrarySort::MostPresets;
        app.view.library_filter = LibraryFilter::Playable;
        app.parts = vec![kontra_ui::app::Part {
            open: false,
            tab: PartTab::Mapping,
            look: Some(Presentation::Kontra),
            ..Default::default()
        }];
        let saved = SavedView::read(&saved(&app, &[3]).write());
        let mut again = App::default();
        apply(&mut again, &saved);
        assert_eq!(
            (again.view.tab, again.view.browser_hidden),
            (Tab::Mixer, true)
        );
        assert_eq!((again.view.octave, again.view.scale), (-1, 75));
        assert!((again.view.split - 0.3).abs() < 1e-6, "the browser's split");
        assert!(
            (again.view.browser_width - 420.).abs() < 1e-3,
            "the browser's width"
        );
        assert_eq!(again.view.face_look, Presentation::Kontra);
        assert_eq!(
            (
                again.view.library_view,
                again.view.tiles,
                again.view.sort,
                again.view.library_filter,
            ),
            (
                LibraryView::Separated,
                Tiles::Cards,
                LibrarySort::MostPresets,
                LibraryFilter::Playable,
            )
        );
        // Legacy project states with `descending: true` and no `sort` field restore Z–A.
        let legacy = SavedView::read(r#"{"descending":true}"#);
        let mut restored = App::default();
        apply(&mut restored, &legacy);
        assert_eq!(restored.view.sort, LibrarySort::NameDesc);
        assert_eq!(restored.view.library_view, LibraryView::Unified);
        let mut waiting = saved.parts.clone();
        again.parts = vec![kontra_ui::app::Part::default()];
        parts(&mut again, &[0], &mut waiting);
        assert_eq!(waiting.len(), 1, "slot 3 has not loaded yet");
        parts(&mut again, &[3], &mut waiting);
        assert!(waiting.is_empty());
        assert_eq!(
            (again.parts[0].open, again.parts[0].tab, again.parts[0].look),
            (false, PartTab::Mapping, Some(Presentation::Kontra))
        );
    }

    /// Sessions saved with three looks (Original 0, Vector 1, KONTRA 2;
    /// a part's own look 1 + that) come back with Vector as KONTRA.
    #[test]
    fn a_session_saved_with_the_vector_or_kontra_look_loads_as_kontra() {
        let old = SavedView::read(
            r#"{"look":1,"parts":[{"part":0,"open":true,"tab":0,"look":2},{"part":1,"open":true,"tab":0,"look":3},{"part":2,"open":true,"tab":0,"look":1},{"part":3,"open":true,"tab":0,"look":0}]}"#,
        );
        let mut app = App::default();
        apply(&mut app, &old);
        assert_eq!(app.view.face_look, Presentation::Kontra, "Vector");
        app.parts = (0..4).map(|_| kontra_ui::app::Part::default()).collect();
        let mut waiting = old.parts.clone();
        parts(&mut app, &[0, 1, 2, 3], &mut waiting);
        let looks: Vec<_> = app.parts.iter().map(|p| p.look).collect();
        let (o, k) = (Presentation::Original, Presentation::Kontra);
        assert_eq!(looks, [Some(k), Some(k), Some(o), None]);
        // KONTRA saves where it was, so an older build reads it as KONTRA.
        let again = saved(&app, &[0, 1, 2, 3]);
        assert_eq!(again.look, 2);
        let own: Vec<_> = again.parts.iter().map(|p| p.look).collect();
        assert_eq!(own, [3, 3, 1, 0]);
    }
}
