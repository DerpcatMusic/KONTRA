//! Generated sections/channel strips retain indices and bindings in the existing IR.
use super::{ir_view, theme::*};
use moose::mui::mui::prelude::*;
use sampler_ui_ir::{self as ir, WidgetRef};

const SECTION_GAP: f64 = 16.;
const COLUMN_GAP: f64 = 4.;

/// Split authored geometry into bands/columns. Store references, never copied control state.
fn clusters(
    face: &ir::Interface,
    mut widgets: Vec<WidgetRef>,
    vertical: bool,
    gap: f64,
) -> Vec<Vec<WidgetRef>> {
    let bounds = |n| {
        let r = face.page_rect(n);
        if vertical {
            (r.y, r.y + r.height)
        } else {
            (r.x, r.x + r.width)
        }
    };
    widgets.sort_by(|&a, &b| bounds(a).0.total_cmp(&bounds(b).0));
    let mut groups: Vec<Vec<WidgetRef>> = Vec::new();
    let mut end = f64::NEG_INFINITY;
    for n in widgets {
        let (start, bottom) = bounds(n);
        if groups.is_empty() || start - end > gap {
            groups.push(Vec::new());
            end = bottom;
        } else {
            end = end.max(bottom);
        }
        groups.last_mut().unwrap().push(n);
    }
    groups
}

fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(*c as u32, 0xE000..=0xF8FF))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn name(face: &ir::Interface, n: WidgetRef) -> String {
    let w = &face.widgets[n.0];
    let text = clean(&w.text);
    if !text.is_empty() {
        return text;
    }
    let r = face.page_rect(n);
    if let Some(label) = face
        .widgets
        .iter()
        .enumerate()
        .filter(|(i, w)| {
            matches!(w.kind, ir::Kind::Label)
                && face.visible(WidgetRef(*i))
                && !clean(&w.text).is_empty()
        })
        .filter_map(|(i, w)| {
            let l = face.page_rect(WidgetRef(i));
            let distance = (r.y - l.y - l.height).abs();
            (distance <= 24. && l.x < r.x + r.width && r.x < l.x + l.width).then_some((distance, w))
        })
        .min_by(|(a, _), (b, _)| a.total_cmp(b))
        .map(|(_, w)| clean(&w.text))
    {
        return label;
    }
    let raw = w
        .automation
        .name
        .as_deref()
        .unwrap_or(&w.name)
        .trim_start_matches(['$', '~', '?', '%', '@', '!']);
    let mut words = String::new();
    let mut lower = false;
    for c in raw.chars() {
        if c.is_uppercase() && lower {
            words.push(' ');
        }
        words.push(if matches!(c, '_' | '-' | '.') { ' ' } else { c });
        lower = c.is_lowercase();
    }
    let readable = words
        .split_whitespace()
        .filter(|w| {
            ![
                "knob", "slider", "button", "btn", "label", "panel", "switch", "ui",
            ]
            .contains(&w.to_lowercase().as_str())
        })
        .collect::<Vec<_>>()
        .join(" ");
    if readable.is_empty() {
        raw.to_owned()
    } else {
        readable
    }
}

// Only fields used by grouping, sizing and name/title inference. In particular,
// values, bindings, meter telemetry, resources and local input are not retained.
#[derive(PartialEq)]
struct Structure {
    page: ir::PageRef,
    parent: Option<WidgetRef>,
    rect: ir::Rect,
    viewport: Option<[i32; 2]>,
    z: i32,
    hidden: bool,
    class: u8,
    text: String,
    name: String,
    automation: Option<String>,
}

fn class(kind: &ir::Kind) -> u8 {
    match kind {
        ir::Kind::Panel | ir::Kind::Image | ir::Kind::MouseArea => 0,
        ir::Kind::Label => 1,
        ir::Kind::Knob { .. } => 2,
        ir::Kind::Slider {
            orientation: ir::Orientation::Vertical,
            ..
        } => 3,
        ir::Kind::Slider { .. } => 4,
        ir::Kind::Table { .. } | ir::Kind::Waveform | ir::Kind::Xy { .. } => 5,
        _ => 6,
    }
}

impl Structure {
    fn new(w: &ir::Widget) -> Self {
        Self {
            page: w.page,
            parent: w.parent,
            rect: w.rect,
            viewport: w.viewport,
            z: w.z,
            hidden: w.hidden,
            class: class(&w.kind),
            text: w.text.clone(),
            name: w.name.clone(),
            automation: w.automation.name.clone(),
        }
    }

    fn matches(&self, w: &ir::Widget) -> bool {
        self.page == w.page
            && self.parent == w.parent
            && self.rect == w.rect
            && self.viewport == w.viewport
            && self.z == w.z
            && self.hidden == w.hidden
            && self.class == class(&w.kind)
            && self.text == w.text
            && self.name == w.name
            && self.automation == w.automation.name
    }
}

struct Cell {
    widget: WidgetRef,
    label: String,
    width: f64,
    height: f64,
}

struct Section {
    index: usize,
    title: Option<String>,
    strips: Vec<Vec<Cell>>,
}

/// One current deterministic descriptor, not an El or a history of model copies.
pub(super) struct Layout {
    source: ir::Source,
    page: ir::PageRef,
    structure: Vec<Structure>,
    paint_order: Option<Vec<WidgetRef>>,
    bands: Vec<Vec<Section>>,
}

impl Layout {
    fn matches(&self, face: &ir::Interface, page: ir::PageRef) -> bool {
        self.source == face.source
            && self.page == page
            && self.paint_order == face.paint_order
            && self.structure.len() == face.widgets.len()
            && self
                .structure
                .iter()
                .zip(&face.widgets)
                .all(|(s, w)| s.matches(w))
    }

    fn new(face: &ir::Interface, page: ir::PageRef) -> Self {
        let controls = face
            .draw_order(page)
            .into_iter()
            .filter(|&n| face.visible(n) && class(&face.widgets[n.0].kind) >= 2)
            .collect();
        let mut bands = Vec::new();
        for band in clusters(face, controls, true, SECTION_GAP) {
            let mut sections = Vec::new();
            for group in clusters(face, band, false, SECTION_GAP) {
                let index = group[0].0;
                let top = group
                    .iter()
                    .map(|&n| face.page_rect(n).y)
                    .reduce(f64::min)
                    .unwrap_or(0.);
                let left = group
                    .iter()
                    .map(|&n| face.page_rect(n).x)
                    .reduce(f64::min)
                    .unwrap_or(0.);
                let right = group
                    .iter()
                    .map(|&n| {
                        let r = face.page_rect(n);
                        r.x + r.width
                    })
                    .reduce(f64::max)
                    .unwrap_or(0.);
                let title = face
                    .widgets
                    .iter()
                    .enumerate()
                    .filter(|(n, w)| {
                        matches!(w.kind, ir::Kind::Label) && face.visible(WidgetRef(*n))
                    })
                    .filter_map(|(n, w)| {
                        let r = face.page_rect(WidgetRef(n));
                        let distance = top - r.y - r.height;
                        ((0. ..=24.).contains(&distance) && r.x < right && r.x + r.width > left)
                            .then_some((distance, clean(&w.text)))
                    })
                    .min_by(|(a, _), (b, _)| a.total_cmp(b))
                    .map(|(_, text)| text)
                    .filter(|s| !s.is_empty());
                let strips = clusters(face, group, false, COLUMN_GAP)
                    .into_iter()
                    .map(|mut strip| {
                        strip.sort_by(|&a, &b| face.page_rect(a).y.total_cmp(&face.page_rect(b).y));
                        strip
                            .into_iter()
                            .map(|n| {
                                let knob = ir_view::knob_like(face, &face.widgets[n.0]);
                                let (width, height) = match if knob {
                                    2
                                } else {
                                    class(&face.widgets[n.0].kind)
                                } {
                                    2 => (88., 80.),
                                    3 => (64., 140.),
                                    4 => (152., 32.),
                                    5 => (160., 96.),
                                    _ => (152., CONTROL),
                                };
                                Cell {
                                    widget: n,
                                    label: name(face, n),
                                    width,
                                    height,
                                }
                            })
                            .collect()
                    })
                    .collect();
                sections.push(Section {
                    index,
                    title,
                    strips,
                });
            }
            bands.push(sections);
        }
        Self {
            source: face.source,
            page,
            paint_order: face.paint_order.clone(),
            structure: face.widgets.iter().map(Structure::new).collect(),
            bands,
        }
    }
}

pub fn view(
    ui: &mut Ui,
    namespace: &str,
    face: &ir::Interface,
    page: ir::PageRef,
    assets: &ir_view::Assets,
    scale: f64,
    values: &mut ir_view::Values,
    input: &mut ir_view::InputState,
) -> El {
    if input
        .generated
        .as_ref()
        .is_none_or(|layout| !layout.matches(face, page))
    {
        input.generated = Some(std::sync::Arc::new(Layout::new(face, page)));
    }
    // Clone only the descriptor Arc so widget_state can mutate input below.
    let layout = input.generated.as_ref().unwrap().clone();
    let empty = ir_view::Assets::default();
    let mut bands = Vec::new();
    // One frame past MUI's node cap blanks the editor: stop adding cells at the face's share.
    let mut nodes = 0;
    for band in &layout.bands {
        let mut sections = Vec::new();
        for section in band {
            let section_index = section.index;
            let mut strips = Vec::new();
            for strip in &section.strips {
                let mut cells = Vec::new();
                for cell in strip {
                    let n = cell.widget;
                    let (width, height) = (cell.width * scale, cell.height * scale);
                    let label = &cell.label;
                    let place = (width.to_bits(), height.to_bits(), scale.to_bits());
                    // The cell's pictures are not drawn here, but the face's loads still key it.
                    let built = ir_view::retained(
                        ui,
                        namespace,
                        face,
                        n,
                        assets,
                        ir_view::value_key(face, n, values),
                        (place, input.numeric_revision()),
                        |ui| {
                            col![
                                caption(label.clone())
                                    .lines(1)
                                    .min_w(0)
                                    .fill(secondary())
                                    .tip(label.clone())
                                    .id(format!("{namespace}-caption-{}", n.0)),
                                ir_view::widget_state(
                                    ui,
                                    namespace,
                                    face,
                                    n,
                                    &empty,
                                    ir::Presentation::Vector,
                                    scale,
                                    values,
                                    input,
                                    width,
                                    height
                                )
                            ]
                            .gap(TIGHT)
                            .align(Align::Stretch)
                            .w(width)
                            .shrink(0)
                        },
                    );
                    nodes += ir_view::node_count(&built);
                    if nodes > ir_view::FACE_NODES {
                        ir_view::warn_dropped(1);
                        break;
                    }
                    cells.push(built);
                }
                strips.push(col(cells).gap(SPACE).align(Align::Center).shrink(0));
            }
            let body = row(strips)
                .wrap()
                .gap(INSET)
                .align(Align::Start)
                .w(Len::Pct(100.))
                .min_w(0)
                .shrink(0);
            sections.push(
                match &section.title {
                    Some(t) => col![
                        super::theme::section(t)
                            .tip(t.clone())
                            .id(format!("{namespace}-section-{section_index}")),
                        rule(),
                        body
                    ]
                    .gap(SPACE),
                    None => col![body],
                }
                .pad(INSET)
                .fill(Role::Ink.alpha(0.035))
                .min_w(0),
            );
        }
        bands.push(
            row(sections)
                .wrap()
                .gap(INSET)
                .line_gap(INSET)
                .align(Align::Start)
                .w(Len::Pct(100.)),
        );
    }
    let root = format!("{namespace}-ir-view");
    let size = ui
        .scene()
        .and_then(|scene| scene.surface(&root))
        .map(|surface| surface.frame.size)
        .unwrap_or(Size::new(
            f64::from(face.pages[page.0].size.width) * scale,
            f64::from(face.pages[page.0].size.height) * scale,
        ));
    let body = col(bands)
        .gap(INSET)
        .pad(INSET)
        .w(Len::Pct(100.))
        .shrink(0)
        .named("KONTRA performance controls");
    let mut layers = vec![body];
    if let Some(popup) = ir_view::menu_popup(
        ui,
        namespace,
        face,
        scale,
        values,
        input,
        size.width,
        size.height,
    ) {
        layers.push(popup);
    }
    stack(layers).w(Len::Pct(100.)).min_w(0).shrink(0).id(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn fixture() -> ir::Interface {
        let page = ir::PageRef(0);
        let mut panel = ir::Widget::new(
            "panel",
            page,
            ir::Rect::new(0, 0, 300, 180),
            ir::Kind::Panel,
        );
        panel.viewport = Some([0, 0]);
        let mut label =
            ir::Widget::new("label", page, ir::Rect::new(0, 0, 80, 18), ir::Kind::Label);
        label.text = "Filter".into();
        let mut knob = ir::Widget::new(
            "$cutoffKnob",
            page,
            ir::Rect::new(0, 20, 80, 60),
            ir::Kind::Knob {
                range: ir::Range {
                    max: 100.,
                    default: 50.,
                    step: Some(1.),
                    ..Default::default()
                },
                display: Default::default(),
            },
        );
        knob.parent = Some(WidgetRef(0));
        knob.binding = ir::Binding::Control(ir::ControlId(1));
        ir::Interface {
            source: ir::Source::FalconLua,
            pages: vec![ir::Page {
                size: ir::Size {
                    width: 340.0,
                    height: 220.0,
                },
                ..Default::default()
            }],
            widgets: vec![panel, label, knob],
            ..Default::default()
        }
    }

    #[test]
    fn generated_shell_builds_reuse_layout_but_controls_and_scale_stay_live() {
        let face = fixture();
        let (mut ui, mut plain) = (super::super::theme::ui(), super::super::theme::ui());
        let (mut input, mut other) = (
            ir_view::InputState::default(),
            ir_view::InputState::default(),
        );
        let mut values = ir_view::Values::default();
        let mut other_values = ir_view::Values::default();
        let mut saved = None;
        let mut first_pixels = None;
        for frame in 0..5 {
            values.insert(ir::ControlId(1), 20. + frame as f64 * 10.);
            other_values.clone_from(&values);
            let scale = if frame < 3 { 1. } else { 1.25 };
            let root = view(
                &mut ui,
                "generated",
                &face,
                ir::PageRef(0),
                &Default::default(),
                scale,
                &mut values,
                &mut input,
            );
            let layout = input.generated.as_ref().unwrap();
            if let Some(ref old) = saved {
                assert!(
                    Arc::ptr_eq(old, layout),
                    "value/scale changes must not regroup controls"
                );
            } else {
                saved = Some(layout.clone());
                assert_eq!(layout.bands[0][0].title.as_deref(), Some("Filter"));
                assert_eq!(layout.bands[0][0].strips[0][0].label, "Filter");
            }
            // Force the ordinary clustering/name/layout path on the oracle.
            other.generated = None;
            let baseline = view(
                &mut plain,
                "generated",
                &face,
                ir::PageRef(0),
                &Default::default(),
                scale,
                &mut other_values,
                &mut other,
            );
            for (ui, root) in [(&mut ui, root), (&mut plain, baseline)] {
                ui.frame(
                    col![caption(format!("Shell {frame}")), root],
                    Some(Size::new(340., 220.)),
                    Input::default(),
                    1. / 60.,
                )
                .unwrap();
            }
            let pixels = super::super::tests::pixels(&ui, 340, 220);
            assert_eq!(pixels, super::super::tests::pixels(&plain, 340, 220));
            assert_eq!(
                ui.scene().unwrap().surface("generated-ir-2").unwrap().frame,
                plain
                    .scene()
                    .unwrap()
                    .surface("generated-ir-2")
                    .unwrap()
                    .frame
            );
            if frame == 0 {
                first_pixels = Some(pixels);
            } else if frame == 1 {
                assert_ne!(first_pixels.as_ref().unwrap(), &pixels);
            }
        }
        ui.focus("generated-ir-2");
        for event in [
            Input {
                keys: vec![KeyPress {
                    key: Key::Up,
                    mods: Mods::default(),
                }],
                ..Default::default()
            },
            Input::default(),
            Input::default(),
        ] {
            let root = view(
                &mut ui,
                "generated",
                &face,
                ir::PageRef(0),
                &Default::default(),
                1.25,
                &mut values,
                &mut input,
            );
            ui.frame(root, Some(Size::new(340., 220.)), event, 1. / 60.)
                .unwrap();
        }
        assert!(values[&ir::ControlId(1)] > 60.);
        assert!(!input.edits.is_empty());
        assert!(Arc::ptr_eq(
            saved.as_ref().unwrap(),
            input.generated.as_ref().unwrap()
        ));
    }

    #[test]
    fn generated_descriptor_checks_all_ancestor_order_name_and_size_inputs() {
        let face = fixture();
        let layout = Layout::new(&face, ir::PageRef(0));
        let changes: &[fn(&mut ir::Interface)] = &[
            |f| f.widgets[0].rect.x += 1.0,
            |f| f.widgets[0].viewport = Some([1, 0]),
            |f| f.widgets[0].hidden = true,
            |f| f.widgets[2].parent = None,
            |f| f.widgets[2].page = ir::PageRef(1),
            |f| f.widgets[2].z += 1,
            |f| f.paint_order = Some(vec![]),
            |f| f.widgets[2].rect.width += 1.0,
            |f| {
                f.widgets[2].kind = ir::Kind::Slider {
                    range: Default::default(),
                    orientation: ir::Orientation::Vertical,
                }
            },
            |f| f.widgets[1].text = "Resonance".into(),
            |f| f.widgets[2].text = "Cutoff".into(),
            |f| f.widgets[2].name = "$resonanceKnob".into(),
            |f| f.widgets[2].automation.name = Some("Frequency".into()),
            |f| {
                f.widgets.pop();
            },
            |f| f.source = ir::Source::Ksp { slot: 0 },
        ];
        for change in changes {
            let mut changed = face.clone();
            change(&mut changed);
            assert!(!layout.matches(&changed, ir::PageRef(0)));
        }
        assert!(!layout.matches(&face, ir::PageRef(1)));
        let mut dynamic = face.clone();
        dynamic.widgets[0].enabled = false;
        dynamic.widgets[0].opacity = 0.5;
        dynamic.widgets[2].value = Some(ir::Value::Real(75.));
        dynamic.widgets[2].binding = ir::Binding::Control(ir::ControlId(99));
        assert!(
            layout.matches(&dynamic, ir::PageRef(0)),
            "these inputs belong to eager widget_state, not topology"
        );
    }
}
