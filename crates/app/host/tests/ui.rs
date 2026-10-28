//! A KSP script's UI tree, built after `on init` from what the script set.
#![cfg(test)]

use std::sync::Arc;

use kontra_engine::{Engine, Plan};
use kontra_host::ui::{Face, interface};
use kontra_stream::{Store, StoreConfig};
use kontra_ui_model as ui;

fn tree(source: &str) -> ui::Interface {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let script = kontra_ksp::compile(source, &[]).unwrap();
    let mut plan = Plan::new(vec![], 16);
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let engine = Engine::new(Arc::new(plan), store.client(), 48_000);
    interface(&Face::of(&script), 0, &engine, &|_| None).unwrap()
}

fn widget<'a>(ui: &'a ui::Interface, name: &str) -> &'a ui::Widget {
    ui.widgets.iter().find(|w| w.name == name).unwrap()
}

#[test]
fn the_tree_carries_what_init_set() {
    let ui = tree(
        "on init
            make_perfview
            set_ui_height_px(420)
            set_ui_width_px(800)
            set_script_title(\"Strings\")
            set_control_par_str($INST_WALLPAPER_ID, $CONTROL_PAR_PICTURE, \"bg\")
            declare ui_panel $panel
            move_control_px($panel, 10, 20)
            declare ui_knob $cutoff (0, 1000, 10)
            set_knob_unit($cutoff, $KNOB_UNIT_HZ)
            set_control_help($cutoff, \"Filter cutoff\")
            set_control_par(get_ui_id($cutoff), $CONTROL_PAR_PARENT_PANEL, get_ui_id($panel))
            move_control_px($cutoff, 30, 40)
            set_control_par(get_ui_id($cutoff), $CONTROL_PAR_WIDTH, 50)
            set_control_par(get_ui_id($cutoff), $CONTROL_PAR_HEIGHT, 60)
            set_control_par_str(get_ui_id($cutoff), $CONTROL_PAR_PICTURE, \"knob\")
            set_control_par(get_ui_id($cutoff), $CONTROL_PAR_PICTURE_STATE, 3)
            $cutoff := 700
            declare ui_label $title (1, 1)
            set_text($title, \"Hello\")
            add_text_line($title, \"World\")
            hide_part($title, $HIDE_PART_BG)
            declare ui_switch $gone
            set_control_par(get_ui_id($gone), $CONTROL_PAR_HIDE, $HIDE_WHOLE_CONTROL)
            declare ui_menu $mode
            add_menu_item($mode, \"Legato\", 0)
            add_menu_item($mode, \"Staccato\", 1)
            declare ui_table %curve[4] (2, 2, 100)
            %curve[2] := 55
        end on",
    );
    assert!(ui.performance);
    let page = &ui.pages[0];
    assert_eq!((page.size.width, page.size.height), (800., 420.));
    assert_eq!(page.name, "Strings");
    let bg = page.background.image.unwrap();
    assert_eq!(ui.assets[bg.0].path, "Resources/pictures/bg.png");

    let knob = widget(&ui, "$cutoff");
    assert!(
        matches!(&knob.kind, ui::Kind::Knob { display, .. } if display.unit == "Hz" && display.ratio == 10.)
    );
    assert_eq!(knob.value, Some(ui::Value::Integer(700)));
    assert_eq!(knob.tooltip, "Filter cutoff");
    assert_eq!(knob.text, "cutoff");
    assert_eq!(
        (knob.rect.x, knob.rect.y, knob.rect.width, knob.rect.height),
        (30., 40., 50., 60.)
    );
    let panel = ui.widgets.iter().position(|w| w.name == "$panel");
    assert_eq!(knob.parent.map(|p| p.0), panel);
    assert_eq!(
        ui.page_rect(ui::WidgetRef(1)).x,
        40.,
        "child rects are panel-relative"
    );
    assert_eq!(knob.images[0].frame, Some(3));
    assert_eq!(
        ui.assets[knob.images[0].asset.0].path,
        "Resources/pictures/knob.png"
    );

    let label = widget(&ui, "$title");
    assert_eq!(label.text, "Hello\nWorld");
    assert!(label.hide.background && !label.hide.title);
    assert!(widget(&ui, "$gone").hidden);
    assert!(
        matches!(&widget(&ui, "$mode").kind, ui::Kind::Menu { items } if items.len() == 2 && items[1].text == "Staccato")
    );
    assert!(
        matches!(&widget(&ui, "%curve").kind, ui::Kind::Table { cells, columns: 4, .. } if cells[2] == 55.)
    );
    assert!(ui.unsupported.is_empty(), "{:?}", ui.unsupported);
}
