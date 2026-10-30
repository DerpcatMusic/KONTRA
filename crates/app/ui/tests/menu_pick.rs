//! A script's menu picked from its own page, headless, all the way through:
//! the click writes the *entry's* value (not its row), that value reaches the
//! engine's `on ui_control`, and the next frame reads back both the menu and
//! the name field the script sets from it.
//!
//! A value that changes only in the UI model looks exactly like a pick that
//! does nothing: the tree is redrawn from the script every frame, so a value
//! the engine never took is overwritten with the old entry before the user
//! sees it. This test therefore ends at the engine's own read-back.

use std::sync::Arc;

use kontra_engine::{Engine, Plan};
use kontra_host::ui::{Face, Live};
use kontra_stream::{Store, StoreConfig};
use kontra_ui::input::{self, Gesture, Set};
use kontra_ui::{Pictures, view};
use kontra_ui_model::{Interface, Kind, PageRef, WidgetRef};
use mui::prelude::{Buttons, Input, Mods, Point, PointerInput, Size, Ui};

/// A menu whose entries carry values of their own (10, 20, …) rather than
/// their row, and a name field the script writes from the value, as Electric
/// Mint's `$vig33` and `@u5jzi` do.
const SOURCE: &str = "on init
    declare ui_menu $preset
    add_menu_item($preset, \"- - - FACTORY PRESETS - - -\", 0)
    add_menu_item($preset, \"One\", 10)
    add_menu_item($preset, \"Two\", 20)
    add_menu_item($preset, \"Three\", 30)
    add_menu_item($preset, \"Four\", 40)
    add_menu_item($preset, \"Five\", 50)
    add_menu_item($preset, \"Six\", 60)
    add_menu_item($preset, \"Seven\", 70)
    declare ui_text_edit @name
    set_ui_height(6)
    move_control($preset, 1, 1)
    move_control(@name, 1, 3)
    $preset := 10
    set_text(@name, \"One\")
end on
on ui_control($preset)
    if $preset = 10
        set_text(@name, \"One\")
    end if
    if $preset = 40
        set_text(@name, \"Four\")
    end if
    if $preset = 60
        set_text(@name, \"Six\")
    end if
end on";

/// One page drawn and driven the way the editor drives it.
struct Page {
    ui: Ui,
    tree: Interface,
    pictures: Pictures,
    gesture: Gesture,
    size: (f64, f64),
}

impl Page {
    fn new(tree: Interface) -> Self {
        let (w, h) = tree
            .pages
            .first()
            .map_or((400., 300.), |p| (p.size.width, p.size.height));
        Self {
            ui: kontra_ui::ui(),
            tree,
            pictures: Pictures::new(Box::new(|_| None)),
            gesture: Gesture::default(),
            size: (w, h),
        }
    }

    fn frame(&mut self, at: Input) -> Vec<input::Edit> {
        let (edits, popup) = input::read(
            &mut self.ui,
            0,
            &self.tree,
            PageRef(0),
            1.,
            &mut self.gesture,
        );
        let (page, _) = view::page(
            &mut self.ui,
            0,
            &self.tree,
            PageRef(0),
            &mut self.pictures,
            (1., &[]),
        );
        let el = match popup {
            Some(popup) => mui::prelude::stack([page, popup]),
            None => page,
        };
        let (w, h) = self.size;
        assert!(
            self.ui
                .frame(el, Some(Size::new(w, h)), at, 1. / 60.)
                .is_ok()
        );
        edits
    }

    /// Frames until the popup and its pictures settle.
    fn settle(&mut self) {
        for _ in 0..30 {
            self.frame(Input::default());
        }
    }

    fn at(&self, id: &str) -> Point {
        let f = self
            .ui
            .scene()
            .and_then(|s| s.surface(id))
            .map_or((0., 0., 0., 0.), |s| {
                (
                    s.frame.x,
                    s.frame.y,
                    s.frame.size.width,
                    s.frame.size.height,
                )
            });
        Point::new(f.0 + f.2 / 2., f.1 + f.3 / 2.)
    }

    /// Down on `at`, then up, then a quiet frame; every edit they carried.
    fn click(&mut self, at: Point) -> Vec<input::Edit> {
        let pointer = |down| Input {
            pointer: PointerInput {
                pos: Some(at),
                buttons: if down {
                    Buttons::PRIMARY
                } else {
                    Buttons::default()
                },
                mods: Mods::default(),
            },
            ..Default::default()
        };
        let mut out = self.frame(pointer(true));
        out.extend(self.frame(pointer(true)));
        out.extend(self.frame(pointer(false)));
        out.extend(self.frame(Input::default()));
        out
    }

    fn click_id(&mut self, id: &str) -> Vec<input::Edit> {
        let at = self.at(id);
        self.click(at)
    }
}

#[test]
fn picking_a_menu_entry_reaches_the_script_and_the_name_field() {
    let store = Store::new(StoreConfig::DEFAULT).expect("a store");
    let script = kontra_ksp::compile(SOURCE, &[]).expect("the script compiles");
    let mut plan = Plan::new(Vec::new(), 16);
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut engine = Engine::new(Arc::new(plan), store.client(), 48_000);
    let face = Face::of(&script, &[]);
    let meta = |_: &str| None;
    let mut live = Live::new(&face, 0, &engine, &meta).expect("a page");
    let all = 0..live.tree.widgets.len();
    kontra_ui::layout::resolve(&mut live.tree, all);

    let menu = live
        .tree
        .widgets
        .iter()
        .position(|w| matches!(w.kind, Kind::Menu { .. }))
        .expect("the script's menu");
    let edit = live
        .tree
        .widgets
        .iter()
        .position(|w| matches!(w.kind, Kind::TextEdit))
        .expect("the script's name field");
    let id = live.tree.widgets[menu].source_id.expect("a control id");
    assert_eq!(
        live.tree.widgets[menu].value,
        Some(kontra_ui_model::Value::Integer(10)),
        "the menu starts on the entry the script set"
    );

    let mut page = Page::new(live.tree.clone());
    page.settle();
    let anchor = input::surface(0, menu);
    // The plate opens the list; then the sixth row, four entries from where
    // the menu stands — not the row after it, and not its own row either.
    page.click_id(&anchor);
    assert_eq!(page.gesture.menu, Some(menu), "the menu opened");
    let edits = page.click_id(&format!("{anchor}-6"));

    assert_eq!(
        edits.iter().map(|e| (&e.widget, &e.to)).collect::<Vec<_>>(),
        [(&WidgetRef(menu), &Set::Value(60.))],
        "the click names the sixth entry's value, 60, not its row"
    );

    // What the editor and the host do with it.
    input::apply(&mut page.tree, &edits);
    engine.set_click(0, id, [false; 3], 0);
    engine.set_control(0, id, 60);
    live.tree = page.tree;
    let changed = live.refresh(&face, 0, &engine, &meta);
    kontra_ui::layout::resolve(&mut live.tree, changed.widgets.iter().map(|w| w.0));

    assert_eq!(
        live.tree.widgets[menu].value,
        Some(kontra_ui_model::Value::Integer(60)),
        "the engine kept the pick"
    );
    assert_eq!(
        live.tree.widgets[edit].text, "Six",
        "and the script named it in the field"
    );
    // A block later, still: a value only the UI model took would be gone.
    let mut block = vec![[0f32; 2]; 512];
    engine.process(&mut block, &[]);
    let changed = live.refresh(&face, 0, &engine, &meta);
    kontra_ui::layout::resolve(&mut live.tree, changed.widgets.iter().map(|w| w.0));
    assert_eq!(
        (
            live.tree.widgets[menu].value.clone(),
            live.tree.widgets[edit].text.clone()
        ),
        (Some(kontra_ui_model::Value::Integer(60)), "Six".to_owned()),
        "a block later the menu and the field still show the pick"
    );
}
