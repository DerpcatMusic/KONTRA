//! Corpus UI checks against the same frontend, assets and CPU renderer as the editor.
use super::{ir_view, pictures, theme};
use moose::mui::mui::{prelude::*, vello};
use sampler_ui_ir as ir;
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};

pub fn survey(
    instrument: &mut sampler_ir::Instrument,
    options: &sampler_kontakt::Options,
    shots: Option<&Path>,
) -> Value {
    let (scripts, faces, mut resources) = sampler_kontakt::compile_ui(instrument, options);
    let mut source =
        pictures::Source::of(options.library.as_deref().expect("survey instrument path"));
    let resource_locations = resources
        .as_ref()
        .map(|r| r.locations())
        .unwrap_or_default();
    let mut views = Vec::new();
    for face in &faces {
        let slot = match face.source {
            ir::Source::Ksp { slot } => slot,
            _ => 0,
        };
        let script = scripts.iter().find(|s| s.slot() == slot);
        let behavior = instrument
            .behaviors
            .iter()
            .enumerate()
            .find(|(i, b)| b.slot.unwrap_or(*i as u8) == slot)
            .map(|(_, b)| b);
        let frontend = if behavior.is_some_and(|b| b.source.contains("load_performance_view")) {
            "creator_tools"
        } else if !face.assets.is_empty() {
            "bitmap_ksp"
        } else {
            "stock_ksp"
        };
        let bound: BTreeSet<_> = script
            .into_iter()
            .flat_map(|s| s.controls())
            .map(|c| ir::ControlId(c.definition.id.0))
            .collect();
        let mut values: ir_view::Values = script
            .into_iter()
            .flat_map(|s| s.controls())
            .map(|c| {
                let v = match c.definition.default {
                    sampler_core::ControlValue::Integer(n) => n as f64,
                    sampler_core::ControlValue::Real(r) => r,
                    sampler_core::ControlValue::Toggle(b) => f64::from(b),
                };
                (ir::ControlId(c.definition.id.0), v)
            })
            .collect();
        let initial_values = values.clone();
        let face = ir_view::resolved(face);
        let mut missing = Vec::new();
        let mut assets = ir_view::Assets::default();
        assets.sync(&face, ir::Presentation::Bitmap, |a| {
            let loaded = source.load(a);
            if loaded.is_none() {
                missing.push(a.path.clone());
            }
            loaded
        });
        let mut layout = Vec::new();
        let mut unbound = Vec::new();
        let mut unsupported_widgets = BTreeSet::new();
        let mut font_resources = Vec::new();
        for font in script.into_iter().flat_map(|s| &s.model().interface.fonts) {
            let path = sampler_ksp::ui::picture_path(font);
            let image = resources
                .as_mut()
                .and_then(|r| r.read(&path))
                .and_then(|bytes| crate::artwork::decode(&bytes));
            if image.is_none() {
                missing.push(path.clone());
            }
            let metrics = image.as_ref().map(|i| json!({"width":i.width,"height":i.height,"red_markers":i.rgba[..i.width as usize*4].chunks_exact(4).filter(|c| c[..3] == [255,0,0]).count()}));
            font_resources.push(json!({"path":path,"found":image.is_some(),"metrics":metrics}));
            unsupported_widgets.insert("bitmap font rendering");
        }
        for (n, w) in face.widgets.iter().enumerate() {
            if !face.visible(ir::WidgetRef(n)) {
                continue;
            }
            let interactive = matches!(
                w.kind,
                ir::Kind::Knob { .. }
                    | ir::Kind::Slider { .. }
                    | ir::Kind::Button { .. }
                    | ir::Kind::Switch
                    | ir::Kind::Menu { .. }
                    | ir::Kind::ValueEdit { .. }
                    | ir::Kind::Table { .. }
                    | ir::Kind::Xy { .. }
                    | ir::Kind::TextEdit
                    | ir::Kind::FileSelector { .. }
                    | ir::Kind::MouseArea
            );
            if interactive && !matches!(w.binding, ir::Binding::Control(c) if bound.contains(&c)) {
                unbound.push(w.name.clone());
            }
            match w.kind {
                ir::Kind::Table { .. } => {
                    unsupported_widgets.insert("table interaction");
                }
                ir::Kind::Xy { .. } => {
                    unsupported_widgets.insert("xy interaction");
                }
                ir::Kind::TextEdit => {
                    unsupported_widgets.insert("text edit interaction");
                }
                ir::Kind::FileSelector { .. } => {
                    unsupported_widgets.insert("file selector");
                }
                ir::Kind::Waveform => {
                    unsupported_widgets.insert("waveform");
                }
                ir::Kind::Wavetable { .. } => {
                    unsupported_widgets.insert("wavetable");
                }
                ir::Kind::MouseArea => {
                    unsupported_widgets.insert("mouse area");
                }
                ir::Kind::LevelMeter { .. } => {
                    unsupported_widgets.insert("level meter binding");
                }
                _ => {}
            }
            let r = face.page_rect(ir::WidgetRef(n));
            if r.x < 0.
                || r.y < 0.
                || r.x + r.width > face.pages[w.page.0].size.width
            {
                layout.push(json!({"widget": w.name, "rect": [r.x, r.y, r.width, r.height], "error": "outside page"}));
            }
            if (r.width == 0. || r.height == 0.) && !matches!(w.kind, ir::Kind::Panel) {
                layout.push(json!({"widget": w.name, "error": "zero size"}));
            }
        }
        let mut renders = Vec::new();
        for mode in [ir::Presentation::Bitmap, ir::Presentation::Vector] {
            assets.sync(&face, mode, |a| source.load(a));
            for p in 0..face.pages.len() {
                let w = face.pages[p].size.width.ceil().clamp(1., 4096.) as u16;
                let h = ir_view::height(&face, ir::PageRef(p)).ceil().clamp(1., 4096.) as u16;
                let name = format!("slot-{slot}-page-{p}-{mode:?}.png");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || -> Result<(), String> {
                        let mut ui = theme::ui();
                        for _ in 0..2 {
                            let view = ir_view::view(
                                &mut ui,
                                &face,
                                ir::PageRef(p),
                                &assets,
                                mode,
                                1.,
                                &mut values,
                            );
                            ui.frame(
                                view,
                                Some(Size::new(f64::from(w), f64::from(h))),
                                Input::default(),
                                1. / 60.,
                            )
                            .map_err(|e| format!("{e:?}"))?;
                        }
                        let mut ctx = vello::vello_cpu::RenderContext::new(w, h);
                        let mut resources = vello::vello_cpu::Resources::default();
                        vello::paint(
                            &mut vello::Cpu {
                                ctx: &mut ctx,
                                resources: &mut resources,
                                cache: &mut vello::Cache::default(),
                            },
                            ui.scene().ok_or("no scene")?,
                            vello::kurbo::Affine::IDENTITY,
                        )
                        .map_err(|e| format!("{e:?}"))?;
                        ctx.flush();
                        let mut pix = vello::vello_cpu::Pixmap::new(w, h);
                        ctx.render(&mut pix, &mut resources);
                        if let Some(dir) = shots {
                            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                            let rgba: Vec<_> = pix
                                .take_unpremultiplied()
                                .iter()
                                .flat_map(|p| [p.r, p.g, p.b, p.a])
                                .collect();
                            moose::core::screenshot::save_png(
                                &dir.join(&name),
                                &rgba,
                                u32::from(w),
                                u32::from(h),
                            );
                        }
                        Ok(())
                    },
                ));
                renders.push(json!({"presentation": format!("{mode:?}"), "page": p, "ok": matches!(result, Ok(Ok(()))), "error": match result { Ok(Ok(())) => None, Ok(Err(e)) => Some(e), Err(_) => Some("renderer panic".into()) }, "shot": shots.map(|d| d.join(&name).display().to_string())}));
            }
        }
        let unsupported: Vec<_> = face.unsupported.iter().map(|u| json!({"feature":u.feature,"widget":u.widget.map(|w| face.widgets[w.0].name.clone())})).collect();
        let passive: Vec<_> = values
            .iter()
            .filter(|(id, v)| initial_values.get(id).is_some_and(|old| old != *v))
            .map(|(id, _)| {
                face.widgets
                    .iter()
                    .find(|w| w.binding == ir::Binding::Control(*id))
                    .map(|w| w.name.clone())
                    .unwrap_or_default()
            })
            .collect();
        let rendered = renders.iter().all(|r| r["ok"] == true);
        views.push(json!({"slot":slot,"frontend":frontend,"loaded":true,"widgets":face.widgets.len(),"visible_widgets":face.widgets.iter().enumerate().filter(|(n,_)| face.visible(ir::WidgetRef(*n))).count(),"missing_resources":missing,"font_resources":font_resources,"layout_errors":layout,"passive_value_changes":passive,"unbound_controls":unbound,"unsupported_widgets":unsupported_widgets,"unsupported_properties":unsupported,"renders":renders,"usable":rendered && passive.is_empty() && unbound.is_empty() && unsupported_widgets.is_empty(),"complete":rendered && passive.is_empty() && missing.is_empty() && layout.is_empty() && unbound.is_empty() && unsupported_widgets.is_empty() && unsupported.is_empty()}));
    }
    let errors: Vec<_> = instrument
        .unsupported
        .iter()
        .filter(|u| {
            matches!(
                u.feature.as_str(),
                "script" | "script interface" | "performance view" | "performance view control"
            )
        })
        .map(|u| json!({"script":u.location,"feature":u.feature,"error":u.value}))
        .collect();
    let authored = instrument.behaviors.iter().any(|b| {
        b.source.contains("make_perfview")
            || b.source.contains("load_performance_view")
            || b.source.contains("load_komplete_ui")
            || b.source.contains("ui_")
    });
    let expected: Vec<_> = instrument.behaviors.iter().enumerate().filter(|(_,b)| b.source.contains("make_perfview") || b.source.contains("load_performance_view") || b.source.contains("load_komplete_ui") || b.source.contains("ui_")).map(|(i,b)| {
        let slot = b.slot.unwrap_or(i as u8);
        let frontend = if b.source.contains("load_komplete_ui") { "komplete_ui" } else if b.source.contains("load_performance_view") { "creator_tools" } else if b.source.contains("CONTROL_PAR_PICTURE") || b.source.contains("set_skin_offset") { "bitmap_ksp" } else { "stock_ksp" };
        json!({"slot":slot,"frontend":frontend,"loaded":frontend != "komplete_ui" && views.iter().any(|v| v["slot"] == slot)})
    }).collect();
    json!({"resource_locations":resource_locations,"expected_frontends":expected,"authored":authored,"loaded":!faces.is_empty(),"usable":errors.is_empty() && expected.iter().all(|e| e["loaded"] == true) && (!authored || views.iter().any(|v| v["widgets"].as_u64().unwrap_or(0)>0)) && views.iter().all(|v| v["usable"] == true),"views":views,"errors":errors,"recall":"source persistent state applied before UI; host recall not probed"})
}

#[cfg(test)]
mod tests {
    #[test]
    fn komplete_ui_load_is_an_authored_frontend_even_without_stock_declarations() {
        let mut instrument = sampler_ir::Instrument {
            behaviors: vec![sampler_ir::Behavior {
                name: "Komplete".into(),
                language: sampler_ir::Language::Ksp,
                source: "on init\nload_komplete_ui(\"Main\")\nend on".into(),
                slot: Some(0),
                state: vec![],
                requires: vec![],
            }],
            ..Default::default()
        };
        let report = super::survey(
            &mut instrument,
            &sampler_kontakt::Options {
                library: Some(std::env::temp_dir().join("komplete-ui-survey.nki")),
                ..Default::default()
            },
            None,
        );
        assert_eq!(report["authored"], true);
        assert_eq!(report["expected_frontends"][0]["frontend"], "komplete_ui");
        assert_eq!(report["expected_frontends"][0]["loaded"], false);
        assert_eq!(report["usable"], false);
    }
}
