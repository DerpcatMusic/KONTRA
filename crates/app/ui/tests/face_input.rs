//! A library's own controls played with the pointer, headless: a knob drags
//! and resets, the wheel turns it, a switch flips, a menu floats its items
//! and a pick sets the value, a value edit drags its translator's sweep and
//! reports its release, a table sets the cells a drag crosses, a text edit
//! takes typed text.

use kontra_ui::input::{self, Edit, Gesture, Set};
use kontra_ui::{Pictures, view};
use kontra_ui_model::{
    self as model, Interface, Kind, MenuItem, Page, PageRef, Range, Rect, Size, Widget, WidgetRef,
};
use mui::prelude::{
    Buttons, Input, Key, KeyPress, Mods, Point, PointerInput, Size as ViewSize, Ui, Vec2,
};

struct Face {
    ui: Ui,
    tree: Interface,
    pictures: Pictures,
    gesture: Gesture,
    /// Whether the last frame owed another.
    animating: bool,
}

impl Face {
    fn new() -> Self {
        let mut tree = Interface::default();
        tree.pages.push(Page {
            size: Size {
                width: 400.,
                height: 300.,
            },
            ..Page::default()
        });
        let range = Range {
            min: 0.,
            max: 100.,
            default: 25.,
            step: Some(1.),
        };
        let mut knob = Widget::new(
            "$knob",
            PageRef(0),
            Rect::new(10, 10, 85, 52),
            Kind::Knob {
                range,
                display: model::Display::default(),
            },
        );
        knob.value = Some(model::Value::Integer(0));
        let switch = Widget::new(
            "$switch",
            PageRef(0),
            Rect::new(120, 10, 85, 18),
            Kind::Switch,
        );
        let item = |text: &str, value| MenuItem {
            text: text.into(),
            value,
            visible: true,
        };
        let menu = Widget::new(
            "$menu",
            PageRef(0),
            Rect::new(220, 10, 85, 18),
            Kind::Menu {
                items: vec![item("Hall", 0), item("Room", 1), item("Plate", 2)],
            },
        );
        let mut edit = Widget::new(
            "$edit",
            PageRef(0),
            Rect::new(10, 100, 60, 18),
            Kind::ValueEdit {
                range,
                display: model::Display::default(),
                arrows: false,
            },
        );
        edit.value = Some(model::Value::Integer(0));
        edit.drag = Some(model::Drag {
            axis: model::Orientation::Vertical,
            sweep: model::Sweep::PerUnit(4.),
        });
        edit.reports_release = true;
        let mut table = Widget::new(
            "%table",
            PageRef(0),
            Rect::new(10, 150, 80, 40),
            Kind::Table {
                columns: 4,
                range,
                bipolar: false,
                cells: vec![0.; 4],
                steps_shown: None,
            },
        );
        table.value = Some(model::Value::Integers(vec![0; 4]));
        let text = Widget::new(
            "@name",
            PageRef(0),
            Rect::new(120, 100, 80, 18),
            Kind::TextEdit,
        );
        tree.widgets.extend([knob, switch, menu, edit, table, text]);
        Self {
            ui: kontra_ui::ui(),
            tree,
            pictures: Pictures::new(Box::new(|_| None)),
            gesture: Gesture::default(),
            animating: false,
        }
    }

    /// One frame with `input`; the edits it carried.
    fn frame(&mut self, input: Input) -> Vec<Edit> {
        let (edits, popup) = input::read(
            &mut self.ui,
            0,
            &self.tree,
            PageRef(0),
            1.,
            &mut self.gesture,
        );
        input::apply(&mut self.tree, &edits);
        let (page, _) = view::page(
            &mut self.ui,
            0,
            &self.tree,
            PageRef(0),
            &mut self.pictures,
            (1., &[]),
        );
        let el = match popup {
            Some(menu) => mui::prelude::stack([page, menu]),
            None => page,
        };
        let frame = self
            .ui
            .frame(el, Some(ViewSize::new(400., 300.)), input, 1. / 60.);
        self.animating = frame.is_ok_and(|f| f.animating);
        edits
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

    fn pointer(&mut self, pos: Point, down: bool, mods: Mods) -> Vec<Edit> {
        let buttons = if down {
            Buttons::PRIMARY
        } else {
            Buttons::default()
        };
        self.frame(Input {
            pointer: PointerInput {
                pos: Some(pos),
                buttons,
                mods,
            },
            ..Default::default()
        })
    }

    /// Down and up on `id`, then a quiet frame: every edit they carried.
    fn click(&mut self, id: &str, mods: Mods) -> Vec<Edit> {
        let p = self.at(id);
        let mut out = self.pointer(p, true, mods);
        out.extend(self.pointer(p, false, mods));
        out.extend(self.frame(Input::default()));
        out
    }

    fn value(&self, n: usize) -> f64 {
        self.tree.widgets.get(n).map_or(f64::NAN, input::value)
    }
}

#[test]
fn library_controls_answer_the_pointer_as_kontakt_plays_them() {
    let mut f = Face::new();
    f.frame(Input::default());
    let knob = input::surface(0, 0);
    // A knob with no drag behaviour sweeps its range in 200 points: up 100 is half.
    let start = f.at(&knob);
    f.pointer(start, true, Mods::default());
    for dy in [20., 60., 100.] {
        f.pointer(Point::new(start.x, start.y - dy), true, Mods::default());
    }
    f.pointer(Point::new(start.x, start.y - 100.), false, Mods::default());
    assert_eq!(f.value(0), 50.);
    // Ctrl with a click: the default.
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    let reset = f.click(&knob, ctrl);
    assert_eq!(f.value(0), 25.);
    assert!(reset.contains(&Edit {
        widget: WidgetRef(0),
        to: Set::Value(25.),
        keys: kontra_ui_model::UiModifiers {
            ctrl: true,
            ..Default::default()
        },
    }));
    // The wheel down turns it down.
    f.frame(Input {
        pointer: PointerInput {
            pos: Some(f.at(&knob)),
            ..Default::default()
        },
        wheel: Vec2::new(0., 20.),
        ..Default::default()
    });
    assert!(f.animating, "a wheel turn owes the frame that reads it");
    f.frame(Input::default());
    assert_eq!(f.value(0), 15.);
    // A switch flips on a click.
    f.click(&input::surface(0, 1), Mods::default());
    assert_eq!(f.value(1), 1.);
    // A menu floats its items; a pick sets the value and closes it.
    f.click(&input::surface(0, 2), Mods::default());
    assert_eq!(f.gesture.menu, Some(2));
    f.click(&format!("{}-2", input::surface(0, 2)), Mods::default());
    assert_eq!(f.value(2), 2.);
    assert_eq!(f.gesture.menu, None);
    // A value edit sweeps 4 points a unit: up 20 is 5.
    let edit = input::surface(0, 3);
    let start = f.at(&edit);
    f.pointer(start, true, Mods::default());
    f.pointer(Point::new(start.x, start.y - 20.), true, Mods::default());
    f.pointer(Point::new(start.x, start.y - 20.), false, Mods::default());
    let released = f.frame(Input::default());
    assert_eq!(f.value(3), 5.);
    assert_eq!(released.len(), 1, "a moved drag reports its release too");
    // A click that moves nothing still reports its release.
    let still = f.click(&edit, Mods::default());
    assert_eq!(
        still.iter().map(|e| &e.to).collect::<Vec<_>>(),
        [&Set::Value(5.)]
    );
    // That was a double click: what is typed replaces the number, Enter sets it.
    let key = |key| Input {
        keys: vec![KeyPress {
            key,
            mods: Mods::default(),
        }],
        ..Default::default()
    };
    f.frame(Input {
        text: "42".into(),
        ..Default::default()
    });
    f.frame(key(Key::Enter));
    f.frame(Input::default());
    assert_eq!(f.value(3), 42.);
    // A drag across the table at mid height sets every cell it crosses.
    let table = input::surface(0, 4);
    let mid = f.at(&table);
    let mut crossed = f.pointer(Point::new(mid.x - 35., mid.y), true, Mods::default());
    crossed.extend(f.pointer(Point::new(mid.x + 35., mid.y), true, Mods::default()));
    crossed.extend(f.pointer(Point::new(mid.x + 35., mid.y), false, Mods::default()));
    crossed.extend(f.frame(Input::default()));
    assert_eq!(
        crossed.iter().map(|e| e.to.clone()).collect::<Vec<_>>(),
        [0, 1, 2, 3].map(|k| Set::Cell(k, 50.))
    );
    assert_eq!(
        f.tree.widgets[4].value,
        Some(model::Value::Integers(vec![50; 4]))
    );
    // A click on a text edit floats a field; Enter sends what was typed.
    f.click(&input::surface(0, 5), Mods::default());
    f.frame(Input {
        text: "Hi".into(),
        ..Default::default()
    });
    let mut typed = f.frame(key(Key::Enter));
    typed.extend(f.frame(Input::default()));
    assert_eq!(typed.len(), 1);
    assert_eq!(typed[0].to, Set::Text("Hi".into()));
    assert_eq!(f.tree.widgets[5].text, "Hi");
    // Then it rests: no frame while nothing moves.
    for _ in 0..30 {
        f.frame(Input::default());
    }
    assert!(!f.animating, "an untouched face idles");
}

#[test]
fn a_value_edit_arrow_steps_one_shown_unit_a_press() {
    let mut f = Face::new();
    if let Some(w) = f.tree.widgets.get_mut(3) {
        w.kind = Kind::ValueEdit {
            range: Range {
                min: 0.,
                max: 100.,
                default: 0.,
                step: Some(1.),
            },
            display: model::Display {
                ratio: 10.,
                ..model::Display::default()
            },
            arrows: true,
        };
        w.value = Some(model::Value::Integer(50));
    }
    f.frame(Input::default());
    let mid = f.at(&input::surface(0, 3));
    // The field is 60x18: its arrows sit in the right 14 points.
    let (up, down) = (
        Point::new(mid.x + 26., mid.y - 4.),
        Point::new(mid.x + 26., mid.y + 4.),
    );
    let mut press = |p| {
        let mut out = f.pointer(p, true, Mods::default());
        out.extend(f.pointer(p, false, Mods::default()));
        out.extend(f.frame(Input::default()));
        out
    };
    let stepped = press(up);
    assert_eq!(
        stepped.iter().map(|e| &e.to).collect::<Vec<_>>(),
        [&Set::Value(60.)],
        "one callback a step, none on release"
    );
    press(down);
    press(down);
    assert_eq!(f.value(3), 40.);
}
