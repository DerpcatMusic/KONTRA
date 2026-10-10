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
            (distance <= 24. && l.x < r.x + r.width && r.x < l.x + l.width)
                .then_some((distance, w))
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

pub fn view(
    ui: &mut Ui,
    namespace: &str,
    face: &ir::Interface,
    page: ir::PageRef,
    _assets: &ir_view::Assets,
    scale: f64,
    values: &mut ir_view::Values,
    input: &mut ir_view::InputState,
) -> El {
    let controls: Vec<_> = face
        .draw_order(page)
        .into_iter()
        .filter(|&n| {
            face.visible(n)
                && !matches!(
                    face.widgets[n.0].kind,
                    ir::Kind::Panel | ir::Kind::Image | ir::Kind::Label | ir::Kind::MouseArea
                )
        })
        .collect();
    let empty = ir_view::Assets::default();
    let mut bands = Vec::new();
    for band in clusters(face, controls, true, SECTION_GAP) {
        let mut sections = Vec::new();
        for group in clusters(face, band, false, SECTION_GAP) {
            let section_index = group[0].0;
            let top = group
                .iter()
                .map(|&n| face.page_rect(n).y)
                .min_by(f64::total_cmp)
                .unwrap_or(0.);
            let left = group
                .iter()
                .map(|&n| face.page_rect(n).x)
                .min_by(f64::total_cmp)
                .unwrap_or(0.);
            let right = group
                .iter()
                .map(|&n| {
                    let r = face.page_rect(n);
                    r.x + r.width
                })
                .max_by(f64::total_cmp)
                .unwrap_or(0.);
            let title = face
                .widgets
                .iter()
                .enumerate()
                .filter(|(n, w)| matches!(w.kind, ir::Kind::Label) && face.visible(WidgetRef(*n)))
                .filter_map(|(n, w)| {
                    let r = face.page_rect(WidgetRef(n));
                    let distance = top - r.y - r.height;
                    ((0. ..=24.).contains(&distance) && r.x < right && r.x + r.width > left)
                        .then_some((distance, clean(&w.text)))
                })
                .min_by(|(a, _), (b, _)| a.total_cmp(b))
                .map(|(_, text)| text)
                .filter(|s| !s.is_empty());
            let mut strips = Vec::new();
            for mut strip in clusters(face, group, false, COLUMN_GAP) {
                strip.sort_by(|&a, &b| face.page_rect(a).y.total_cmp(&face.page_rect(b).y));
                let mut cells = Vec::new();
                for n in strip {
                    let (width, height) = match face.widgets[n.0].kind {
                        ir::Kind::Knob { .. } => (88., 80.),
                        ir::Kind::Slider {
                            orientation: ir::Orientation::Vertical,
                            ..
                        } => (64., 140.),
                        ir::Kind::Slider { .. } => (152., 32.),
                        ir::Kind::Table { .. } | ir::Kind::Waveform | ir::Kind::Xy { .. } => {
                            (160., 96.)
                        }
                        _ => (152., CONTROL),
                    };
                    let (width, height) = (width * scale, height * scale);
                    let label = name(face, n);
                    cells.push(
                        col![
                            caption(label.clone()).lines(1).min_w(0).fill(secondary()).tip(label).id(format!("{namespace}-caption-{}", n.0)),
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
                        .shrink(0),
                    );
                }
                strips.push(col(cells).gap(SPACE).align(Align::Center).shrink(0));
            }
            let body = row(strips).wrap().gap(INSET).align(Align::Start).w(Len::Pct(100.)).min_w(0).shrink(0);
            sections.push(
                match title {
                    Some(t) => col![section(&t).tip(t).id(format!("{namespace}-section-{section_index}")), rule(), body].gap(SPACE),
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
    let root=format!("{namespace}-ir-view");
    let size=ui.scene().and_then(|scene|scene.surface(&root)).map(|surface|surface.frame.size)
        .unwrap_or(Size::new(f64::from(face.pages[page.0].size.width)*scale,f64::from(face.pages[page.0].size.height)*scale));
    let body=col(bands).gap(INSET).pad(INSET).w(Len::Pct(100.)).shrink(0).named("KONTRA performance controls");
    let mut layers=vec![body];
    if let Some(popup)=ir_view::menu_popup(ui,namespace,face,scale,values,input,size.width,size.height) {layers.push(popup);}
    stack(layers).w(Len::Pct(100.)).min_w(0).shrink(0).id(root)
}
