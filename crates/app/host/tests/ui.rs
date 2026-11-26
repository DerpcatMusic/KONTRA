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

/// A label or a button with a font but no alignment sets its words from the
/// left (the reference 8.13, tools/ksp-ui fields page: unaligned buttons and
/// switches, lit or not); TEXT_ALIGNMENT 1 centres them.
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
    assert_eq!(align("$b"), ui::Align::Left);
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
    // Carried parameters have no look, so the tree does not report them as unsupported.
    assert_eq!(ui.unsupported, []);
}

/// A range from variables is the declared one too: a later MIN/MAX write
/// leaves it, read and drawn, and the default reads the minimum until set
/// (Kontakt 8.13, control_pars_12).
#[test]
fn a_declared_range_from_variables_ignores_later_writes() {
    let ui = tree(
        "on init
            declare $lo := 5
            declare $hi := 80
            declare ui_knob $k ($lo, $hi, 1)
            set_control_par(get_ui_id($k), $CONTROL_PAR_MIN_VALUE, 20)
            set_control_par(get_ui_id($k), $CONTROL_PAR_MAX_VALUE, 200)
            declare ui_label $echo (1, 1)
            set_text($echo, get_control_par(get_ui_id($k), $CONTROL_PAR_MIN_VALUE))
            add_text_line($echo, get_control_par(get_ui_id($k), $CONTROL_PAR_MAX_VALUE))
            add_text_line($echo, get_control_par(get_ui_id($k), $CONTROL_PAR_DEFAULT_VALUE))
        end on",
    );
    assert_eq!(widget(&ui, "$echo").text, "5\n80\n5");
    let ui::Kind::Knob { range, .. } = &widget(&ui, "$k").kind else {
        panic!("a knob");
    };
    assert_eq!((range.min, range.max, range.default), (5., 80., 5.));
}

/// `get_ui_wf_property` reads back what `set_ui_wf_property` stored.
#[test]
fn a_waveform_property_reads_back_as_set() {
    let ui = tree(
        "on init
            make_perfview
            declare ui_waveform $wave (5, 4)
            set_ui_wf_property($wave, $UI_WF_PROP_PLAY_CURSOR, 0, 250000)
            declare ui_label $echo (1, 1)
            set_text($echo, get_ui_wf_property($wave, $UI_WF_PROP_PLAY_CURSOR, 0))
        end on",
    );
    assert_eq!(widget(&ui, "$echo").text, "250000");
}

/// A file dropped on a mouse area reaches its `on ui_control` as a drop
/// with the files it takes (KSP manual, `ui_mouse_area`), and the face
/// knows what the area takes.
#[test]
fn a_file_dropped_on_a_mouse_area_reaches_its_callback() {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let script = kontra_ksp::compile(
        "on init
            make_perfview
            declare ui_mouse_area $area
            set_control_par(get_ui_id($area), $CONTROL_PAR_DND_ACCEPT_AUDIO, $NI_DND_ACCEPT_ONE)
            set_control_par(get_ui_id($area), $CONTROL_PAR_RECEIVE_DRAG_EVENTS, 1)
            declare ui_label $got (1, 1)
            message(get_ui_id($area))
        end on
        on ui_control ($area)
            if ($NI_MOUSE_EVENT_TYPE = $NI_MOUSE_EVENT_TYPE_DROP)
                set_text($got, num_elements(!NI_DND_ITEMS_AUDIO) & \" \" & !NI_DND_ITEMS_AUDIO[0])
            end if
            if ($NI_MOUSE_EVENT_TYPE = $NI_MOUSE_EVENT_TYPE_DRAG)
                set_text($got, \"over \" & $NI_MOUSE_OVER_CONTROL)
            end if
        end on",
        &[],
    )
    .unwrap();
    let mut plan = Plan::new(vec![], 16);
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut engine = Engine::new(Arc::new(plan), store.client(), 48_000);
    let face = Face::of(&script, &[]);
    let mut live = Live::new(&face, 0, &engine, &|_| None).unwrap();
    assert_eq!(
        widget(&live.tree, "$area").kind,
        ui::Kind::MouseArea {
            takes: [1, 0, 0],
            drag: true
        }
    );
    let area: i32 = engine.last_message().parse().unwrap();
    let mut run = |engine: &mut Engine, drop: bool, over: bool| {
        engine.set_click(0, area, [false; 3], 0);
        engine.drop_event(0, drop, over);
        engine.set_control(0, area, 0);
        engine.process(&mut vec![[0f32; 2]; 512], &[]);
        live.refresh(&face, 0, engine, &|_| None);
        widget(&live.tree, "$got").text.clone()
    };
    engine.drop_item(true, 0, kontra_engine::Text::from_str_cut("/s/kick.wav").0);
    assert_eq!(run(&mut engine, false, true), "over 1");
    assert_eq!(run(&mut engine, true, true), "1 /s/kick.wav");
}

/// A label's tab advances one space in the pixel font's family and about two
/// in font 13 (the reference 8.13, tools/ksp-ui tables page).
#[test]
fn ksp_label_tabs_advance_as_the_font_spaces_them() {
    let ui = tree(
        "on init
            make_perfview
            declare ui_label $lbl (1, 1)
            set_text($lbl, \"\t\t\t   Marcato Short\")
            declare ui_label $big (1, 1)
            set_control_par(get_ui_id($big), $CONTROL_PAR_FONT_TYPE, 13)
            set_text($big, \"\t\t\t   Marcato Short\")
        end on",
    );
    assert_eq!(widget(&ui, "$lbl").text, "      Marcato Short");
    assert_eq!(widget(&ui, "$big").text, "         Marcato Short");
}

/// A `Binding::Param` knob on a script-free interface updates engine audio
/// state directly via `Rack::control` -> `Command::SetParam` -> `Engine::set_param`
/// with zero script VM overhead.
#[test]
fn binding_param_knob_drives_engine_directly_without_script_vm() {
    use kontra_host::rack::{Event, Mix, Part, Rack};
    use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt};

    let mut store = Store::new(StoreConfig::DEFAULT).expect("create direct parameter test store");
    let fmt = SampleFmt {
        encoding: Encoding::F32,
        channels: 1,
    };
    let frames = 48_000;
    let key = BlobKey {
        file: "dc-param".into(),
        offset: 0,
        fmt,
    };
    let bytes: Vec<u8> = (0..frames).flat_map(|_| 0.5f32.to_le_bytes()).collect();
    let sample = store
        .load(
            &key,
            Arc::new(MemSource::new(fmt, bytes)),
            frames,
            std::slice::from_ref(&(0..frames)),
        )
        .expect("load direct parameter test DC sample");
    let region = kontra_engine::Region {
        mix: None,
        table: None,
        sample,
        start: 0,
        end: frames,
        loops: Default::default(),
        reverse: false,
    };
    let mut zone = kontra_engine::Zone::new(region, 60, 48_000);
    zone.keys = 0..=127;
    let mut plan = Plan::new(vec![zone], 16);
    for g in &mut plan.groups {
        g.mods.clear();
    }
    assert!(plan.scripts.is_empty(), "test must have zero scripts");

    let engine = Engine::new(Arc::new(plan), store.client(), 48_000);
    let (mut rack, mut audio) = Rack::new(48_000, store, 512);
    let part = Part {
        path: None,
        source: None,
        recall: kontra_engine::Recall::Restart,
        name: "Direct".into(),
        diagnostics: vec![],
        faces: vec![],
        mix: Mix::DEFAULT,
        solo: false,
        mute: false,
        script: Default::default(),
        plan: None,
        delays: None,
        view: None,
        sounds: Default::default(),
        slot_types: Vec::new(),
        slot_responses: Vec::new(),
        modified: false,
        named: Default::default(),
    };
    rack.install(0, Box::new(engine), part);

    let mut knob = ui::Widget::new(
        "Volume",
        ui::PageRef(0),
        ui::Rect::new(10, 10, 40, 40),
        ui::Kind::Knob {
            range: ui::Range {
                min: 0.0,
                max: 1.0,
                default: 1.0,
                step: None,
            },
            display: ui::Display::default(),
        },
    );
    knob.binding = ui::Binding::Param(ui::ParamBinding::with_map(
        ui::ir::Scope::Instrument,
        ui::ir::ParamAddr::Volume,
        ui::ParamMap::Linear { min: 0.0, max: 1.0 },
    ));
    knob.value = Some(ui::Value::Real(1.0));
    let interface = ui::Interface {
        pages: vec![ui::Page {
            name: "Main".into(),
            size: ui::Size {
                width: 200.0,
                height: 100.0,
            },
            ..ui::Page::default()
        }],
        widgets: vec![knob],
        ..ui::Interface::default()
    };
    rack.set_interface(0, interface.clone());
    let revision = |rack: &Rack| {
        rack.parts()[0]
            .as_ref()
            .unwrap()
            .view
            .as_ref()
            .unwrap()
            .revision
    };
    let first = revision(&rack);
    assert_ne!(first, 0, "a part without a view publishes revision zero");
    rack.set_interface(0, interface.clone());
    let replaced = revision(&rack);
    assert_ne!(first, replaced);

    let render_peak = |audio: &mut kontra_host::rack::RackAudio| -> f32 {
        audio.process(256, &[]);
        audio
            .output(0, 256)
            .iter()
            .map(|[l, r]| l.abs().max(r.abs()))
            .fold(0.0f32, f32::max)
    };

    // Start a note at unity gain (1.0).
    rack.play(Event::NoteOn {
        note: kontra_host::rack::HostNote {
            port: 0,
            channel: 0,
            key: 60,
            id: -1,
            clap: false,
        },
        velocity: 1.0,
        tune: 0.0,
    });
    let _ = render_peak(&mut audio);
    let full_peak = render_peak(&mut audio);
    assert!(
        full_peak > 0.1,
        "expected audible DC signal, got {full_peak}"
    );

    // Move the Binding::Param knob to 0.25 -> Linear(0..1) = 0.25 linear gain.
    // Let the engine's gain smoother settle over several blocks (~100 ms).
    rack.control(0, 0, None, 0.25, ui::UiModifiers::default());
    let mut attenuated_peak = 0.0;
    for _ in 0..24 {
        attenuated_peak = render_peak(&mut audio);
    }
    let ratio = attenuated_peak / full_peak;
    assert!(
        (ratio - 0.25).abs() < 0.03,
        "expected ~0.25 linear gain ratio, got {ratio:.3} (full={full_peak}, atten={attenuated_peak})"
    );

    // Confirm the view updated its own value and bumped revision.
    let view = rack
        .parts()
        .first()
        .and_then(Option::as_ref)
        .and_then(|p| p.view.as_ref());
    assert_eq!(
        view.and_then(|v| v.tree().widgets.first())
            .and_then(|w| w.value.as_ref()),
        Some(&ui::Value::Real(0.25))
    );
    assert_eq!(view.map(|v| v.revision), Some(replaced + 1));

    let mut table = interface.clone();
    table.widgets[0].kind = ui::Kind::Table {
        columns: 2,
        range: ui::Range {
            min: 0.,
            max: 1.,
            ..ui::Range::default()
        },
        bipolar: false,
        cells: vec![0.1, 0.2],
        steps_shown: None,
    };
    let cells = ui::Value::Reals(vec![0.1, 0.2]);
    table.widgets[0].value = Some(cells.clone());
    rack.set_interface(0, table);
    let table_revision = revision(&rack);
    rack.control(0, 0, Some(1), 0.9, ui::UiModifiers::default());
    for _ in 0..24 {
        attenuated_peak = render_peak(&mut audio);
    }
    let view = rack.parts()[0].as_ref().unwrap().view.as_ref().unwrap();
    assert_eq!(view.tree().widgets[0].value, Some(cells));
    assert_eq!(view.revision, table_revision);
    assert!((attenuated_peak / full_peak - 0.25).abs() < 0.03);

    let mut integer = interface;
    integer.widgets[0].value = Some(ui::Value::Integer(1));
    rack.set_interface(0, integer);
    rack.control(0, 0, None, 0.4, ui::UiModifiers::default());
    for _ in 0..24 {
        attenuated_peak = render_peak(&mut audio);
    }
    let view = rack.parts()[0].as_ref().unwrap().view.as_ref().unwrap();
    assert_eq!(view.tree().widgets[0].value, Some(ui::Value::Integer(0)));
    assert!(
        attenuated_peak < full_peak * 0.03,
        "engine should receive zero gain"
    );
}

/// A picture whose frame or position the script sets from one knob or
/// slider's value shows that control's value (Analog Strings' cylinders,
/// `179*$macros__intensity0/1000000`); one set from anything else does not.
#[test]
fn a_picture_the_script_turns_with_a_control_shows_that_control() {
    let ui = tree(
        "on init
            declare ui_slider $amount (0, 1000000)
            declare ui_switch $on
            declare ui_label $cylinder (1, 1)
            declare ui_label $slide (1, 1)
            declare ui_label $lamp (1, 1)
            declare ui_label $fixed (1, 1)
            declare ui_label $both (1, 1)
            declare %frames[2]
            declare $plain
        end on
        function draw
            set_control_par(get_ui_id($cylinder), $CONTROL_PAR_PICTURE_STATE, 179*$amount/1000000)
            set_control_par(get_ui_id($slide), $CONTROL_PAR_POS_X, 10 + $amount / 1000)
            set_control_par(get_ui_id($lamp), $CONTROL_PAR_PICTURE_STATE, $on)
            set_control_par(get_ui_id($fixed), $CONTROL_PAR_PICTURE_STATE, %frames[$plain])
            set_control_par(get_ui_id($both), $CONTROL_PAR_PICTURE_STATE, $amount + $on)
        end function
        on ui_control($amount)
            call draw
        end on",
    );
    let amount = ui
        .widgets
        .iter()
        .position(|w| w.name == "$amount")
        .map(ui::WidgetRef);
    assert_eq!(widget(&ui, "$cylinder").shows, amount);
    assert_eq!(widget(&ui, "$slide").shows, amount);
    assert_eq!(
        widget(&ui, "$lamp").shows,
        None,
        "a switch's state is not a value it turns"
    );
    assert_eq!(widget(&ui, "$fixed").shows, None, "an array, not a control");
    assert_eq!(widget(&ui, "$both").shows, None, "two controls");
    assert_eq!(widget(&ui, "$amount").shows, None);
}

/// A picture the script picks by a menu's value spells that menu's item
/// (Analog Strings' `"macro_slider_label_" & $macros__name0`).
#[test]
fn a_picture_the_script_picks_by_a_menu_spells_its_item() {
    let ui = tree(
        "on init
            declare ui_slider $amount (0, 1000000)
            declare ui_menu $name
            add_menu_item($name, \"Rhythm\", 0)
            declare ui_label $word (1, 1)
            declare ui_label $other (1, 1)
            set_control_par_str(get_ui_id($word), $CONTROL_PAR_PICTURE, \"label_\" & $name)
            set_control_par(get_ui_id($word), $CONTROL_PAR_PICTURE_STATE, $amount / 5556)
            set_control_par_str(get_ui_id($other), $CONTROL_PAR_PICTURE, \"label_\" & $amount)
        end on",
    );
    let at = |name| {
        ui.widgets
            .iter()
            .position(|w| w.name == name)
            .map(ui::WidgetRef)
    };
    let word = widget(&ui, "$word");
    assert_eq!((word.shows, word.spells), (at("$amount"), at("$name")));
    assert_eq!(
        widget(&ui, "$other").spells,
        None,
        "picked by a slider, not a menu"
    );
}

/// Each control's drag sweeps its range as Kontakt 8.13's did on the
/// `tools/ksp-ui` behaviour page (100 %, so page pixels are points): one
/// drag per control from mid range, the value it ended on.
#[test]
fn a_drag_moves_each_control_as_far_as_kontakt_did() {
    let ui = tree(
        "on init
            declare ui_knob $k (0, 1000, 1)
            declare ui_knob $k100 (0, 100, 1)
            declare ui_slider $s (0, 1000)
            declare ui_slider $s100 (0, 100)
            declare ui_slider $mb1000 (0, 1000)
            set_control_par(get_ui_id($mb1000), $CONTROL_PAR_MOUSE_BEHAVIOUR, 1000)
            declare ui_slider $mb500 (0, 1000)
            set_control_par(get_ui_id($mb500), $CONTROL_PAR_MOUSE_BEHAVIOUR, 500)
            declare ui_value_edit $v (0, 1000, 1)
            move_control_px($k, 10, 20)
            move_control_px($k100, 120, 20)
            move_control_px($s, 10, 110)
            move_control_px($s100, 120, 110)
            move_control_px($mb1000, 230, 110)
            move_control_px($mb500, 340, 110)
            move_control_px($v, 10, 200)
            { The renderer's stock slider size (crates/app/ui layout), set here
              because the tree leaves it to the renderer. }
            set_control_par(get_ui_id($mb1000), $CONTROL_PAR_WIDTH, 85)
            set_control_par(get_ui_id($mb1000), $CONTROL_PAR_HEIGHT, 18)
            set_control_par(get_ui_id($mb500), $CONTROL_PAR_WIDTH, 85)
            set_control_par(get_ui_id($mb500), $CONTROL_PAR_HEIGHT, 18)
        end on",
    );
    // (control, start, pixels along its axis, where Kontakt left it)
    let cases = [
        ("$k", 500., 10., 543.),
        ("$k", 500., 50., 714.),
        ("$k", 500., 100., 929.),
        ("$k", 500., -100., 71.),
        ("$k100", 50., 100., 93.),
        ("$s", 500., 10., 587.),
        ("$s", 500., 20., 674.),
        ("$s", 500., 40., 849.),
        ("$s100", 50., 20., 67.),
        ("$mb1000", 500., 20., 616.),
        ("$mb500", 500., 20., 558.),
        ("$v", 500., 10., 540.),
        ("$v", 500., 40., 660.),
    ];
    for (name, start, pixels, kontakt) in cases {
        let w = widget(&ui, name);
        let (ui::Kind::Knob { range, .. }
        | ui::Kind::Slider { range, .. }
        | ui::Kind::ValueEdit { range, .. }) = &w.kind
        else {
            panic!("{name} is not continuous");
        };
        let drag = w.drag.unwrap();
        let length = match drag.axis {
            ui::Orientation::Vertical => w.rect.height,
            ui::Orientation::Horizontal => w.rect.width,
        };
        let sweep = match drag.sweep {
            ui::Sweep::Points(p) => p,
            ui::Sweep::Lengths(k) => k * length,
        };
        let ours = (start + (range.max - range.min) * pixels / sweep).clamp(range.min, range.max);
        assert!(
            (ours - kontakt).abs() <= 2.,
            "{name} dragged {pixels}: {ours:.1}, Kontakt {kontakt}"
        );
    }
}

/// Who hears a click where two controls overlap, as Kontakt 8.13 answered
/// on the `tools/ksp-ui` layers page (`probes/layers.measured`): the later
/// declaration on top, a front layer over an earlier one, a back layer
/// under, a button over a later knob (type before declaration), a label
/// passing the click to the switch under it, a hidden control or a hidden
/// panel's child passing it on, one without its background still taking
/// it, a back-layer panel's child under a default button, and a button over
/// a mouse area.
#[test]
fn a_click_on_overlapping_controls_reaches_the_one_kontakt_picked() {
    let mut src = String::from("on init\n");
    let pairs = [
        ("ui_button", "da", "ui_button", "db", ""),
        (
            "ui_button",
            "fa",
            "ui_button",
            "fb",
            "set_control_par(get_ui_id($fa), $CONTROL_PAR_Z_LAYER, 1)",
        ),
        (
            "ui_button",
            "ga",
            "ui_button",
            "gb",
            "set_control_par(get_ui_id($gb), $CONTROL_PAR_Z_LAYER, -1)",
        ),
        ("ui_button", "ta", "ui_knob", "tb", ""),
        (
            "ui_switch",
            "la",
            "ui_label",
            "lb",
            "set_control_par(get_ui_id($lb), $CONTROL_PAR_Z_LAYER, 1)",
        ),
        (
            "ui_button",
            "ha",
            "ui_button",
            "hb",
            "set_control_par(get_ui_id($hb), $CONTROL_PAR_HIDE, $HIDE_WHOLE_CONTROL)",
        ),
        (
            "ui_button",
            "pa",
            "ui_button",
            "pb",
            "set_control_par(get_ui_id($pb), $CONTROL_PAR_HIDE, $HIDE_PART_BG)",
        ),
        ("ui_mouse_area", "ma", "ui_button", "mb", ""),
    ];
    for (c, (ka, a, kb, b, extra)) in pairs.iter().enumerate() {
        let x = 10 + 110 * c;
        let args = |k: &str| match k {
            "ui_knob" => " (0, 100, 1)",
            "ui_label" => " (1, 1)",
            _ => "",
        };
        src += &format!(
            "declare {ka} ${a}{}\ndeclare {kb} ${b}{}\n\
             move_control_px(${a}, {x}, 20)\nmove_control_px(${b}, {}, 30)\n",
            args(ka),
            args(kb),
            x + 20
        );
        for v in [a, b] {
            src += &format!(
                "set_control_par(get_ui_id(${v}), $CONTROL_PAR_WIDTH, 80)\n\
                 set_control_par(get_ui_id(${v}), $CONTROL_PAR_HEIGHT, 30)\n"
            );
        }
        src += extra;
        src += "\n";
    }
    src += "declare ui_panel $hp
        move_control_px($hp, 10, 120)
        set_control_par(get_ui_id($hp), $CONTROL_PAR_HIDE, $HIDE_WHOLE_CONTROL)
        declare ui_button $hpin
        set_control_par(get_ui_id($hpin), $CONTROL_PAR_PARENT_PANEL, get_ui_id($hp))
        declare ui_button $pzo
        move_control_px($pzo, 140, 130)
        declare ui_panel $pz
        move_control_px($pz, 120, 120)
        set_control_par(get_ui_id($pz), $CONTROL_PAR_Z_LAYER, -1)
        declare ui_button $pzin
        set_control_par(get_ui_id($pzin), $CONTROL_PAR_PARENT_PANEL, get_ui_id($pz))\n";
    for v in ["hpin", "pzo", "pzin"] {
        src += &format!(
            "set_control_par(get_ui_id(${v}), $CONTROL_PAR_WIDTH, 80)\n\
             set_control_par(get_ui_id(${v}), $CONTROL_PAR_HEIGHT, 30)\n"
        );
    }
    // Row 3, declared as the page does: a control's own layer in a panel,
    // and nested panels (each button 80 by 30).
    let layer =
        |v: &str, z: i32| format!("set_control_par(get_ui_id(${v}), $CONTROL_PAR_Z_LAYER, {z})\n");
    let into = |v: &str, p: &str| {
        format!("set_control_par(get_ui_id(${v}), $CONTROL_PAR_PARENT_PANEL, get_ui_id(${p}))\n")
    };
    let button = |v: &str, x: i32, y: i32| {
        format!(
            "declare ui_button ${v}\nmove_control_px(${v}, {x}, {y})\n\
             set_control_par(get_ui_id(${v}), $CONTROL_PAR_WIDTH, 80)\n\
             set_control_par(get_ui_id(${v}), $CONTROL_PAR_HEIGHT, 30)\n"
        )
    };
    let panel = |v: &str, x: i32, y: i32| {
        format!("declare ui_panel ${v}\nmove_control_px(${v}, {x}, {y})\n")
    };
    src += &[
        button("cout", 20, 230),
        panel("ca", 0, 220),
        button("cin", 0, 0) + &into("cin", "ca") + &layer("cin", -1),
        button("cbo", 130, 230),
        panel("cb", 110, 220) + &layer("cb", -1),
        button("cbin", 0, 0) + &into("cbin", "cb") + &layer("cbin", 1),
        panel("no", 220, 220),
        panel("ni", 0, 0) + &into("ni", "no"),
        button("nin", 0, 0) + &into("nin", "ni"),
        button("nout", 20, 10) + &into("nout", "no"),
        button("nbo", 350, 230),
        panel("nbout", 330, 220) + &layer("nbout", -1),
        panel("nbmid", 0, 0) + &into("nbmid", "nbout"),
        button("nbin", 0, 0) + &into("nbin", "nbmid"),
    ]
    .concat();
    src += "end on";
    let ui = tree(&src);
    // The topmost shown control under the point that takes a click (labels,
    // panels and mouse areas pass it on, as the renderer's input does).
    let hears = |x: f64, y: f64| {
        ui.draw_order(ui::PageRef(0))
            .into_iter()
            .rev()
            .find(|&n| {
                let w = &ui.widgets[n.0];
                let r = ui.page_rect(n);
                ui.visible(n)
                    && !matches!(
                        w.kind,
                        ui::Kind::Label | ui::Kind::Panel | ui::Kind::MouseArea { .. }
                    )
                    && (r.x..r.x + r.width).contains(&x)
                    && (r.y..r.y + r.height).contains(&y)
            })
            .map(|n| ui.widgets[n.0].name.clone())
    };
    let kontakt = ["$db", "$fa", "$ga", "$ta", "$la", "$ha", "$pb", "$mb"];
    for (c, want) in kontakt.iter().enumerate() {
        let x = 10. + 110. * c as f64 + 50.;
        assert_eq!(hears(x, 45.).as_deref(), Some(*want), "case {c}");
    }
    assert_eq!(hears(50., 135.), None, "a hidden panel's button");
    assert_eq!(
        hears(170., 145.).as_deref(),
        Some("$pzo"),
        "a back-layer panel's button"
    );
    let row3 = [
        (50., "$cout", "a back-layer button in a plain panel"),
        (160., "$cbo", "a front-layer button in a back-layer panel"),
        (
            270.,
            "$nout",
            "a button in a nested panel against a later one",
        ),
        (380., "$nbo", "a button two panels under a back layer"),
    ];
    for (x, want, case) in row3 {
        assert_eq!(hears(x, 245.).as_deref(), Some(want), "{case}");
    }
}
