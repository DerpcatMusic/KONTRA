//! A KSP script's UI tree, built after `on init` from what the script set.
#![cfg(test)]

use std::sync::Arc;

use kontra_engine::{Engine, Plan};
use kontra_host::ui::{Face, Live, interface};
use kontra_stream::{Store, StoreConfig};
use kontra_ui_model as ui;

fn tree(source: &str) -> ui::Interface {
    tree_with(source, &[])
}

fn tree_with(source: &str, data: &[(String, Vec<u8>)]) -> ui::Interface {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let script = kontra_ksp::compile(source, &[]).unwrap();
    let mut plan = Plan::new(vec![], 16);
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let engine = Engine::new(Arc::new(plan), store.client(), 48_000);
    interface(&Face::of(&script, data), 0, &engine, &|_| None).unwrap()
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
            set_control_par_str(get_ui_id($title), $CONTROL_PAR_PICTURE, \"plate\")
            set_control_par(get_ui_id($title), $CONTROL_PAR_PICTURE_STATE, 2)
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
    // A knob's value picks its frame; PICTURE_STATE is a label's.
    assert_eq!(knob.images[0].frame, None);
    assert_eq!(
        ui.assets[knob.images[0].asset.0].path,
        "Resources/pictures/knob.png"
    );

    let label = widget(&ui, "$title");
    assert_eq!(label.images[0].frame, Some(2));
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

#[test]
fn a_performance_view_gives_its_controls_type_place_and_parent() {
    let view = br#"{"value":{"performanceView":{"size":{"width":700,"height":300},
        "controls":[{"index":0,"value":{"common":{"id":"main","show":false},"controls":[
          {"index":6,"value":{"common":{"id":"mode","position":{"x":7,"y":8}},
            "entries":[{"string":"A","value":0},{"string":"B","value":1}]}}]}}]}}}"#;
    let ui = tree_with(
        "on init
            load_performance_view(\"v\")
            move_control_px($MAIN_Mode, 9, 8)
        end on",
        &[("v.nckp".into(), view.to_vec())],
    );
    assert!(ui.performance);
    assert_eq!(ui.pages[0].size.width, 700.);
    let mode = widget(&ui, "$main_mode");
    assert!(matches!(&mode.kind, ui::Kind::Menu { items } if items.len() == 2));
    assert_eq!(
        (mode.rect.x, mode.rect.y),
        (9., 8.),
        "init overrides the view"
    );
    let main = ui.widgets.iter().position(|w| w.name == "$main");
    assert_eq!(mode.parent.map(|p| p.0), main);
    assert_eq!(
        ui.widgets
            .iter()
            .filter(|w| w.name.eq_ignore_ascii_case("$main_mode"))
            .count(),
        1,
        "the script's spelling is the view's control, not a second one"
    );
    let at = ui
        .widgets
        .iter()
        .position(|w| w.name == "$main_mode")
        .unwrap();
    assert!(
        !ui.visible(ui::WidgetRef(at)),
        "a hidden view panel hides its children"
    );
}

/// A performance-requesting slot whose page cannot be built must not end the
/// search for the page to show.
///
/// Slot 0 asks for the performance view and the model rejects its tree (its
/// label's bitmap font is the same picture as the wallpaper, so one asset is
/// asked for as an image and as a font); slot 1 asks for nothing; slot 2 asks
/// and builds. `View::of` must open slot 2. The `find(..).and_then(..)` this
/// replaced stopped at slot 0's failed construction and returned `None`, so the
/// editor opened nothing while a valid performance view sat two slots on.
#[test]
fn a_performing_slot_that_cannot_build_does_not_end_the_search() {
    let broken = "on init
            make_perfview
            declare ui_label $a (1, 1)
            set_control_par(get_ui_id($a), $CONTROL_PAR_FONT_TYPE, get_font_id(\"bg\"))
            set_control_par_str($INST_WALLPAPER_ID, $CONTROL_PAR_PICTURE, \"bg\")
        end on";
    let admin = "on init
            declare ui_knob $x (0, 100, 1)
        end on";
    let asks = "on init
            make_perfview
            declare ui_knob $y (0, 100, 1)
        end on";
    let scripts: Vec<_> = [broken, admin, asks]
        .iter()
        .map(|source| kontra_ksp::compile(source, &[]).unwrap())
        .collect();
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![], 16);
    plan.scripts = scripts
        .iter()
        .map(|s| kontra_host::script::plan(s, &[], &[]))
        .collect();
    let engine = Engine::new(Arc::new(plan), store.client(), 48_000);
    let faces: Vec<Option<Face>> = scripts.iter().map(|s| Some(Face::of(s, &[]))).collect();

    // The fixture's point: slot 0 is preferred *and* cannot build, so the
    // selection has to walk past it rather than stop.
    assert!(
        interface(faces[0].as_ref().unwrap(), 0, &engine, &|_| None).is_err(),
        "slot 0 must be the slot whose tree is rejected"
    );

    let view = kontra_host::view::View::of(&faces, &[None, None, None], &engine, None)
        .expect("a performance view two slots on");
    assert_eq!(
        view.slot, 2,
        "the search ended at a slot that could not build, or opened an admin page"
    );
}

#[test]
fn an_idle_ui_rebuilds_nothing_and_a_moved_knob_rebuilds_what_it_touched() {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let script = kontra_ksp::compile(
        "on init
            declare ui_knob $k (0, 100, 1)
            declare ui_label $readout (1, 1)
            declare ui_switch $other
            declare ui_table %steps[8] (2, 2, 100)
            message(get_ui_id($k))
        end on
        on ui_control ($k)
            set_text($readout, $k)
        end on",
        &[],
    )
    .unwrap();
    let mut plan = Plan::new(vec![], 16);
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut engine = Engine::new(Arc::new(plan), store.client(), 48_000);
    let face = Face::of(&script, &[]);
    let mut live = Live::new(&face, 0, &engine, &|_| None).unwrap();
    let k: i32 = engine.last_message().parse().unwrap();

    // A second of 512-frame blocks with nothing moving.
    let mut out = vec![[0f32; 2]; 512];
    let mut rebuilt = 0;
    for _ in 0..94 {
        engine.process(&mut out, &[]);
        rebuilt += live.refresh(&face, 0, &engine, &|_| None).widgets.len();
    }
    assert_eq!(rebuilt, 0, "idle: no widget rebuilds");

    // The knob moves every block: it and the label its callback writes.
    let names = |ws: &[ui::WidgetRef], ui: &ui::Interface| {
        ws.iter()
            .map(|w| ui.widgets[w.0].name.clone())
            .collect::<Vec<_>>()
    };
    for v in 1..=60 {
        engine.set_control(0, k, v);
        engine.process(&mut out, &[]);
        let changed = live.refresh(&face, 0, &engine, &|_| None);
        assert!(!changed.page);
        assert_eq!(names(&changed.widgets, &live.tree), ["$k", "$readout"]);
    }
    assert_eq!(widget(&live.tree, "$readout").text, "60");
    assert_eq!(widget(&live.tree, "$k").value, Some(ui::Value::Integer(60)));
    assert!(live.refresh(&face, 0, &engine, &|_| None).is_empty());
}

#[test]
fn a_page_the_script_moves_after_init_refreshes_without_rebuilding_widgets() {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let script = kontra_ksp::compile(
        "on init
            make_perfview
            set_ui_height_px(300)
            declare ui_switch $tab
            message(get_ui_id($tab))
        end on
        on ui_control ($tab)
            set_skin_offset($tab * 300)
        end on",
        &[],
    )
    .unwrap();
    let mut plan = Plan::new(vec![], 16);
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut engine = Engine::new(Arc::new(plan), store.client(), 48_000);
    let face = Face::of(&script, &[]);
    let mut live = Live::new(&face, 0, &engine, &|_| None).unwrap();
    let tab: i32 = engine.last_message().parse().unwrap();
    let mut out = vec![[0f32; 2]; 512];
    engine.process(&mut out, &[]);
    assert!(live.refresh(&face, 0, &engine, &|_| None).is_empty());

    engine.set_control(0, tab, 1);
    engine.process(&mut out, &[]);
    let changed = live.refresh(&face, 0, &engine, &|_| None);
    assert!(changed.page, "the skin offset moved the page");
    assert_eq!(live.tree.pages[0].background.offset_y, 300);
    assert_eq!(names_of(&changed.widgets, &live.tree), ["$tab"]);
    assert!(live.refresh(&face, 0, &engine, &|_| None).is_empty());
}

fn names_of(ws: &[ui::WidgetRef], ui: &ui::Interface) -> Vec<String> {
    ws.iter().map(|w| ui.widgets[w.0].name.clone()).collect()
}

/// A font from `get_font_id` is the bitmap font of its picture; a factory
/// font stays stock.
#[test]
fn a_picture_font_id_is_a_bitmap_font() {
    let ui = tree(
        "on init
            declare ui_label $a (1, 1)
            set_control_par(get_ui_id($a), $CONTROL_PAR_FONT_TYPE, get_font_id(\"digits\"))
            declare ui_label $b (1, 1)
            set_control_par(get_ui_id($b), $CONTROL_PAR_FONT_TYPE, 4)
        end on",
    );
    let font = |name| {
        let style = widget(&ui, name).style.unwrap();
        ui.styles[style.0].font.clone()
    };
    let ui::Font::Bitmap(asset) = font("$a") else {
        panic!("{:?}", font("$a"));
    };
    let asset = &ui.assets[asset.0];
    assert_eq!(asset.path, "Resources/pictures/digits.png");
    assert_eq!(asset.kind, ui::AssetKind::BitmapFont);
    assert_eq!(font("$b"), ui::Font::Stock(4));
}

/// A label with a font but no alignment sets its words from the left;
/// a button centres them (KSP manual; Pacific's "Mic Mixer" in Kontakt).
#[test]
fn a_label_aligns_left_unless_told() {
    let ui = tree(
        "on init
            declare ui_label $l (1, 1)
            set_control_par(get_ui_id($l), $CONTROL_PAR_FONT_TYPE, 12)
            declare ui_label $c (1, 1)
            set_control_par(get_ui_id($c), $CONTROL_PAR_FONT_TYPE, 12)
            set_control_par(get_ui_id($c), $CONTROL_PAR_TEXT_ALIGNMENT, 1)
            declare ui_button $b
            set_control_par(get_ui_id($b), $CONTROL_PAR_FONT_TYPE, 12)
        end on",
    );
    let align = |name| ui.styles[widget(&ui, name).style.unwrap().0].align;
    assert_eq!(align("$l"), ui::Align::Left);
    assert_eq!(align("$c"), ui::Align::Center);
    assert_eq!(align("$b"), ui::Align::Center);
}

/// `attach_level_meter` puts the channel it shows into the tree; an
/// unattached meter shows none (ANALOG, Conflux, Morphology, Shreddage).
#[test]
fn an_attached_level_meter_shows_its_channel() {
    let ui = tree(
        "on init
            make_perfview
            declare ui_level_meter $left
            declare ui_level_meter $right
            declare ui_level_meter $idle
            attach_level_meter(get_ui_id($left), -1, -1, 0, -1)
            attach_level_meter(get_ui_id($right), -1, -1, 1, -1)
            declare ui_level_meter $bus
            declare ui_level_meter $group
            attach_level_meter(get_ui_id($bus), -1, -1, 0, 2)
            attach_level_meter(get_ui_id($group), 0, -1, 1, 2)
        end on",
    );
    let meter = |name| match widget(&ui, name).kind {
        ui::Kind::LevelMeter { channel, bus, .. } => (channel, bus),
        _ => panic!("{name} is no level meter"),
    };
    assert_eq!(
        ["$left", "$right", "$idle", "$bus", "$group"].map(meter),
        [
            (Some(0), None),
            (Some(1), None),
            (None, None),
            (Some(0), Some(2)),
            // A group's FX chain is no instrument bus: the output, for now.
            (Some(1), None),
        ]
    );
}

/// A text edit shows the text its variable holds, as `on init` left it and
/// as a later callback writes it.
#[test]
fn a_text_edit_shows_what_the_script_wrote() {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let script = kontra_ksp::compile(
        "on init
            make_perfview
            declare ui_text_edit @name
            @name := \"Cello\"
            declare ui_switch $s
            message(get_ui_id($s))
        end on
        on ui_control ($s)
            @name := \"Viola\"
        end on",
        &[],
    )
    .unwrap();
    let mut plan = Plan::new(vec![], 16);
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut engine = Engine::new(Arc::new(plan), store.client(), 48_000);
    let face = Face::of(&script, &[]);
    let mut live = Live::new(&face, 0, &engine, &|_| None).unwrap();
    assert_eq!(widget(&live.tree, "@name").text, "Cello");
    let s: i32 = engine.last_message().parse().unwrap();
    engine.set_control(0, s, 1);
    engine.process(&mut vec![[0f32; 2]; 512], &[]);
    live.refresh(&face, 0, &engine, &|_| None);
    assert_eq!(widget(&live.tree, "@name").text, "Viola");
}

/// A switch fires on mouse down, a button on mouse up (KSP manual, `ui_switch`
/// and `ui_button` remarks).
#[test]
fn a_switch_fires_on_press_and_a_button_on_release() {
    let ui = tree(
        "on init
            declare ui_switch $s
            declare ui_button $b
        end on",
    );
    assert!(widget(&ui, "$s").activate_on_press);
    assert!(!widget(&ui, "$b").activate_on_press);
}

/// `set_key_color` reaches the keyboard in Kontakt 8's measured colours
/// (CHORUS, Dolce, Areia, Afflatus colour their keyswitch ranges).
#[test]
fn script_key_colours_reach_the_keybed() {
    let ui = tree(
        "on init
            make_perfview
            set_key_color(36, $KEY_COLOR_RED)
            set_key_color(37, $KEY_COLOR_RED)
            set_key_color(38, $KEY_COLOR_NONE)
            set_key_name(36, \"Legato\")
        end on",
    );
    assert_eq!(
        ui.keybed,
        [(36, ui::Rgba::rgb(0xDA918F)), (37, ui::Rgba::rgb(0x782F2D))]
    );
    assert_eq!(ui.key_names, [(36, "Legato".to_owned())]);
    assert_eq!(
        ui.key_pressed, None,
        "Kontakt shows the keys played by default"
    );
    let ui = tree(
        "on init
            make_perfview
            set_key_pressed_support(1)
            set_key_pressed(40, 1)
        end on",
    );
    assert_eq!(ui.key_pressed, Some(vec![40]));
}

/// What the user libraries' scripts show beyond controls: a waveform
/// display's zone and play cursor (Areia Pads), a text line added to a
/// label (ANALOG STRINGS), a custom id read back (Conflux) and the status
/// `message` (Areia, Dolce, CHORUS).
#[test]
fn waveform_text_lines_custom_ids_and_messages_reach_the_tree() {
    let ui = tree(
        "on init
            make_perfview
            declare ui_waveform $wave (5, 4)
            attach_zone($wave, 7, $UI_WAVEFORM_USE_TABLE)
            set_ui_wf_property($wave, $UI_WF_PROP_PLAY_CURSOR, 0, 250000)
            declare ui_label $list (1, 4)
            set_text($list, \"1 Cutoff\")
            set_control_par_str(get_ui_id($list), $CONTROL_PAR_TEXTLINE, \"2 Drive\")
            declare ui_menu $fx
            set_control_par(get_ui_id($fx), $CONTROL_PAR_CUSTOM_ID, 12)
            declare ui_label $echo (1, 1)
            set_text($echo, get_control_par(get_ui_id($fx), $CONTROL_PAR_CUSTOM_ID))
            message(\"Loading done\")
        end on",
    );
    let wave = widget(&ui, "$wave").waveform.as_ref().unwrap();
    assert_eq!((wave.zone, wave.cursor_us), (7, 250_000));
    assert_ne!(wave.flags, 0);
    assert_eq!(widget(&ui, "$list").text, "1 Cutoff\n2 Drive");
    assert_eq!(widget(&ui, "$echo").text, "12");
    assert_eq!(ui.message, "Loading done");
}
