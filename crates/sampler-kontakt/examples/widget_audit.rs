//! Metadata-only audit. Never prints or writes source, assets or saved strings.
use sampler_ui_ir::{Binding, Kind};
use std::{collections::BTreeSet, path::Path};

const WIDGETS: &[&str] = &[
    "ui_knob",
    "ui_slider",
    "ui_button",
    "ui_switch",
    "ui_menu",
    "ui_table",
    "ui_xy",
    "ui_waveform",
    "ui_wavetable",
    "ui_file_selector",
    "ui_level_meter",
    "ui_value_edit",
    "ui_label",
    "ui_text_edit",
    "ui_panel",
    "ui_mouse_area",
];

// Source usage is not an initialized-control census. Ignore strings/comments.
fn identifiers(source: &str) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' || c == '"' {
            let end = if c == '{' { '}' } else { '"' };
            while let Some(c) = chars.next() {
                if c == '\\' && end == '"' {
                    chars.next();
                } else if c == end {
                    break;
                }
            }
        } else if c.is_ascii_alphabetic() || c == '$' || c == '_' {
            let mut word = String::from(c);
            while chars
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
            {
                word.push(chars.next().unwrap());
            }
            result.insert(word);
        }
    }
    result
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let path = Path::new(&args[2]);
    if args[1] == "usage" {
        let mut used = BTreeSet::new();
        let multi = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nkm"));
        let programs = if multi {
            sampler_kontakt::read_multi(path)
                .expect("multi metadata")
                .programs
                .len()
        } else {
            1
        };
        let mut scripts = 0;
        for program in 0..programs {
            let read = if multi {
                sampler_kontakt::read_program(path, program)
            } else {
                sampler_kontakt::read(path)
            }
            .expect("metadata read");
            scripts += read.instrument.behaviors.len();
            for b in &read.instrument.behaviors {
                used.extend(identifiers(&b.source));
            }
        }
        println!("scripts\t{scripts}\tprograms\t{programs}");
        for token in used {
            if WIDGETS.contains(&token.as_str())
                || token.starts_with("$CONTROL_PAR_")
                || matches!(
                    token.as_str(),
                    "ui_control"
                        | "ui_controls"
                        | "ui_update"
                        | "set_knob_defval"
                        | "set_table_steps_shown"
                        | "attach_zone"
                        | "attach_level_meter"
                        | "fs_get_filename"
                        | "fs_navigate"
                        | "load_komplete_ui"
                        | "load_performance_view"
                        | "make_perfview"
                        | "move_control_px"
                        | "move_control"
                )
            {
                println!("use\t{token}");
            }
        }
        return;
    }
    let read = sampler_kontakt::read(path).expect("metadata read");
    let options = sampler_kontakt::Options {
        keys: 0..=0,
        library: Some(path.into()),
        ..Default::default()
    };
    let loaded = sampler_kontakt::load_read(read, &options, |_| {}, || false)
        .expect("witness initialization");
    println!(
        "faces\t{}\tscripts\t{}\tplan_controls\t{}",
        loaded.interfaces.len(),
        loaded.scripts.len(),
        loaded.plan.controls().len()
    );
    for (f, face) in loaded.interfaces.iter().enumerate() {
        println!(
            "face\t{f}\t{:?}\twidgets\t{}\tunsupported\t{}",
            face.source,
            face.widgets.len(),
            face.unsupported.len()
        );
        let mut kinds = std::collections::BTreeMap::new();
        for (n, w) in face.widgets.iter().enumerate() {
            let kind = match &w.kind {
                Kind::Knob { .. } => "knob",
                Kind::Slider { .. } => "slider",
                Kind::Button { .. } => "button",
                Kind::Switch => "switch",
                Kind::Menu { .. } => "menu",
                Kind::Table { .. } => "table",
                Kind::Xy { .. } => "xy",
                Kind::Waveform => "waveform",
                Kind::Wavetable { .. } => "wavetable",
                Kind::FileSelector { .. } => "file_selector",
                Kind::LevelMeter { .. } => "level_meter",
                Kind::ValueEdit { .. } => "value_edit",
                Kind::Label => "label",
                Kind::TextEdit => "text_edit",
                Kind::Panel => "panel",
                Kind::MouseArea => "mouse_area",
                Kind::Image => "image",
            };
            *kinds.entry(kind).or_insert(0) += 1;
            if face.visible(sampler_ui_ir::WidgetRef(n))
                && matches!(w.kind, Kind::Knob { .. } | Kind::Slider { .. })
            {
                let bound = matches!(w.binding, Binding::Control(id) if loaded.plan.controls().iter().any(|c| c.id.0 == id.0));
                println!(
                    "continuous\t{f}\t{n}\t{kind}\t{:?}\tdrag\t{:?}\tbound\t{bound}\trect\t{:?}\tauto\t{}",
                    w.kind,
                    w.drag,
                    face.page_rect(sampler_ui_ir::WidgetRef(n)),
                    w.auto_size
                );
            }
        }
        for (kind, count) in kinds {
            println!("kind\t{f}\t{kind}\t{count}");
        }
    }
    let controls: Vec<_> = loaded
        .interfaces
        .iter()
        .flat_map(|face| {
            face.widgets.iter().enumerate().filter_map(|(n, w)| {
                if !face.visible(sampler_ui_ir::WidgetRef(n))
                    || !matches!(w.kind, Kind::Knob { .. } | Kind::Slider { .. })
                {
                    return None;
                }
                let Binding::Control(id) = w.binding else {
                    return None;
                };
                let def = loaded.plan.controls().iter().find(|c| c.id.0 == id.0)?;
                let value = match def.domain {
                    sampler_core::ControlDomain::Integer { min, max } => {
                        sampler_core::ControlValue::Integer(min / 2 + max / 2)
                    }
                    sampler_core::ControlDomain::Real { min, max } => {
                        sampler_core::ControlValue::Real(min / 2. + max / 2.)
                    }
                    sampler_core::ControlDomain::Toggle => sampler_core::ControlValue::Toggle(true),
                };
                Some((def.id, value))
            })
        })
        .collect();
    let limits = sampler_core::Limits::for_plan(&loaded.plan, 16, 64);
    let mut runtime = sampler_core::Runtime::new(loaded.plan, limits).expect("runtime");
    let plan = runtime.active_plan();
    let context = sampler_core::ControlContext {
        performance: runtime.performance(0).unwrap(),
        origin: sampler_core::ChannelAddress {
            protocol: sampler_core::Protocol::Midi1,
            port: 0,
            group: 0,
            channel: 0,
        },
        channels: 1,
    };
    let mut admitted = 0;
    let mut readback = 0;
    let mut callback_readback = 0;
    let mut frames = [[0.; 2]; 64];
    for (id, value) in &controls {
        admitted += usize::from(
            runtime
                .invoke_control(
                    context,
                    plan,
                    None,
                    sampler_core::ControlWrite {
                        id: *id,
                        value: *value,
                    },
                )
                .is_ok(),
        );
        readback += usize::from(runtime.control_value(plan, *id) == Ok(*value));
        runtime.render(&mut frames).expect("control callback block");
        callback_readback += usize::from(runtime.control_value(plan, *id) == Ok(*value));
    }
    println!(
        "native_edits\t{}\tadmitted\t{admitted}\treadback\t{readback}\tafter_callback_block\t{callback_readback}",
        controls.len()
    );
}

#[test]
fn identifiers_keep_widget_and_parameter_names() {
    let names = identifiers(
        "declare ui_slider $x(0,100)\nset_control_par(1,$CONTROL_PAR_MOUSE_BEHAVIOUR,100)",
    );
    assert!(names.contains("ui_slider"));
    assert!(names.contains("$CONTROL_PAR_MOUSE_BEHAVIOUR"));
    assert!(!names.contains("slider"));
    assert!(!identifiers("{ ui_table } \"ui_menu\" ui_xy").contains("ui_table"));
    assert_eq!(
        identifiers("{ ui_table } \"ui_menu\" ui_xy"),
        BTreeSet::from(["ui_xy".to_owned()])
    );
}
