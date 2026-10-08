//! Metadata-only audit. Never prints or writes source, assets or saved strings.
use sampler_ui_ir::{Binding, Kind};
use std::{collections::BTreeSet, path::Path};

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
    assert!(
        args.len() == 3 && args[1] == "witness",
        "usage: widget_audit witness PATH"
    );
    let path = Path::new(&args[2]);
    let read = sampler_kontakt::read(path).expect("metadata read");
    let options = sampler_kontakt::Options {
        keys: 0..=0,
        library: Some(path.into()),
        ..Default::default()
    };
    let mut loaded = sampler_kontakt::load_read(read, &options, |_| {}, || false)
        .expect("witness initialization");
    println!(
        "faces\t{}\tscripts\t{}\tplan_controls\t{}",
        loaded.interfaces.len(),
        loaded.scripts.len(),
        loaded.plan.controls().len()
    );
    for (f, face) in loaded.interfaces.iter().enumerate() {
        let slot = match face.source {
            sampler_ui_ir::Source::Ksp { slot } => slot,
            _ => continue,
        };
        let script = loaded.scripts.iter().find(|s| s.slot() == slot).unwrap();
        let model = &script.model().interface;
        let source = &loaded
            .instrument
            .behaviors
            .iter()
            .find(|b| b.slot == Some(slot))
            .unwrap()
            .source;
        let descriptor = sampler_ksp::nckp::view_name(source).and_then(|name| {
            loaded
                .resources
                .as_mut()?
                .read(&format!("Resources/performance_view/{name}.nckp"))
        });
        let view = descriptor
            .as_ref()
            .and_then(|bytes| sampler_ksp::nckp::parse(bytes).ok());
        if let Some((view, skipped)) = &view {
            println!(
                "performance_view\t{f}\tcontrols\t{}\tskipped\t{}",
                view.controls.len(),
                skipped.len()
            );
            println!(
                "source_knob_declaration_token\t{f}\t{}",
                identifiers(source).contains("ui_knob")
            );
        }
        if let Some(bytes) = descriptor {
            use std::io::Write;
            let names: Vec<_> = model
                .widgets
                .iter()
                .filter(|w| w.properties.is_empty())
                .map(|w| w.name.as_str())
                .collect();
            let mut child = std::process::Command::new("python3")
                .args(["tools/widget-audit.py", "--nckp-stdin"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("raw resource counter");
            {
                let mut input = child.stdin.take().unwrap();
                writeln!(input, "{}", names.len()).unwrap();
                for name in names {
                    writeln!(input, "{name}").unwrap();
                }
                input.write_all(&bytes).unwrap();
            }
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success(), "raw resource counter failed");
            println!(
                "raw_performance_view\t{f}\t{}",
                String::from_utf8(output.stdout).unwrap().trim()
            );
        }
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
                if face.page_rect(sampler_ui_ir::WidgetRef(n)).x == 0
                    && face.page_rect(sampler_ui_ir::WidgetRef(n)).y == 0
                {
                    let raw = &model.widgets[n];
                    let assumed = loaded.instrument.unsupported.iter().any(|u| {
                        u.value.starts_with(&format!(
                            "{} is not in the performance view description; assumed",
                            w.name
                        ))
                    });
                    let described = view.as_ref().is_some_and(|(v, _)| {
                        v.controls
                            .iter()
                            .any(|c| c.name.eq_ignore_ascii_case(&w.name))
                    });
                    let suffix = view.as_ref().is_some_and(|(v, _)| {
                        v.controls
                            .iter()
                            .any(|c| c.name[1..].ends_with(&w.name[1..]))
                    });
                    println!(
                        "origin_control\t{f}\t{n}\tassumed\t{assumed}\tmodel_properties\t{}\tposition\t{:?}\thide\t{:?}\tdescribed\t{described}\tsuffix_match\t{suffix}",
                        raw.properties.len(),
                        raw.position(),
                        raw.int("$CONTROL_PAR_HIDE")
                    );
                }
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

#[test]
fn missing_performance_description_creates_visible_unsized_knob() {
    let script = sampler_ksp::compile(
        "on init\nload_performance_view(\"missing\")\n$ghost := 0\nend on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let model = script.model();
    assert_eq!(model.interface.widgets.len(), 1);
    assert!(model.interface.widgets[0].properties.is_empty());
    let face = script.ui(&|_| None).unwrap();
    assert!(face.visible(sampler_ui_ir::WidgetRef(0)));
    assert_eq!(face.widgets[0].rect, sampler_ui_ir::Rect::new(0, 0, 0, 0));
    assert!(face.widgets[0].auto_size);
}
