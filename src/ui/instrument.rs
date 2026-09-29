//! The selected instrument: its header, its script's performance view, its
//! key/velocity mapping, details, and plain-spoken notices.

use super::{Cx, Tab, rack::pan_text, theme::*};
use crate::import::{self, Instrument};
use crate::ksp::{Control, Interface, Value};
use moose::mui::mui::geometry::Path as DrawPath;
use moose::mui::mui::prelude::*;
use moose::mui::mui::scene::Fit;
use std::f64::consts::PI;
use std::path::Path;
use std::sync::Arc;

/// The instrument loaded for the selected part, once it matches the part.
fn current<'a>(cx: &'a Cx) -> Option<&'a Arc<Instrument>> {
    let part = cx.part()?;
    let v = cx.part_view();
    v.instrument
        .as_ref()
        .filter(|i| i.path == Path::new(&part.path) && v.program == part.program)
}

/// The empty rack's view.
pub fn welcome(cx: &Cx) -> El {
    let mut lines = vec![
        title("Pick an instrument").text_weight(Weight::SEMIBOLD),
        body("Choose a library on the left and click an instrument, or drag it onto the rack. Multis load the whole rack.")
            .fill(Role::Dim)
            .lines(3)
            .max_size(Size::new(420., 200.)),
    ];
    if !cx.view.multi_status.is_empty() {
        lines.push(
            caption(cx.view.multi_status.clone())
                .fill(Role::Dim)
                .lines(2),
        );
    }
    col![spacer(), col(lines).gap(GAP).align(Align::Center), spacer()]
        .align(Align::Center)
        .pad(WIDE * 2.)
        .flex(1)
        .min_h(0)
}

/// Name, library, size, preset stepping, part level and pan, audition.
pub fn header(ui: &mut Ui, cx: &mut Cx) -> El {
    let Some(part) = cx.part().cloned() else {
        return block(Len::Pct(100.), 0).shrink(0);
    };
    let slot = cx.state.selected;
    let library = cx.library_of(Path::new(&part.path));
    let multi = import::is_multi(Path::new(&part.path));
    let siblings: Vec<_> = cx
        .view
        .files
        .iter()
        .filter(|path| import::is_multi(path) == multi && cx.library_of(path) == library)
        .collect();
    let index = siblings
        .iter()
        .position(|path| path.to_string_lossy() == part.path);
    let (previous, prev_el) = action(ui, "preset-prev", "‹", false);
    let (next, next_el) = action(ui, "preset-next", "›", false);
    let target = index.and_then(|n| match (previous, next) {
        (true, _) => n.checked_sub(1),
        (_, true) => Some(n + 1).filter(|n| *n < siblings.len()),
        _ => None,
    });
    if let Some(target) = target {
        let path = siblings[target].to_string_lossy().into_owned();
        cx.replace(slot, path);
    }
    let prev_el = prev_el
        .named("Previous preset")
        .when(!index.is_some_and(|n| n > 0), |e| e.disabled());
    let next_el = next_el
        .named("Next preset")
        .when(!index.is_some_and(|n| n + 1 < siblings.len()), |e| {
            e.disabled()
        });

    let v = cx.part_view();
    let instrument = current(cx).cloned();
    let name = instrument
        .as_ref()
        .map_or_else(|| super::header::stem(&part.path), |i| i.name.clone());
    let mut facts = vec![library_label(&library)];
    if let Some(i) = &instrument {
        facts.push(format!("{} groups", i.groups.len()));
        facts.push(format!("{} zones", i.zones.len()));
    }
    if v.loading {
        facts.push("loading samples…".into());
    } else if v.bytes > 0 {
        facts.push(megabytes(v.bytes));
    }
    facts.retain(|f| !f.is_empty());

    let part = &mut cx.selection.parts[slot];
    let mut gain = f64::from(part.gain);
    let gain_el = number(
        ui,
        "performance-gain",
        "Level",
        &mut gain,
        -60.0..=6.0,
        format!("{:.1} dB", part.gain),
    );
    part.gain = gain as f32;
    let mut pan = f64::from(part.pan);
    let pan_el = number(
        ui,
        "performance-pan",
        "Pan",
        &mut pan,
        -1.0..=1.0,
        pan_text(part.pan),
    );
    part.pan = pan as f32;
    let (audition, play_el) = action(ui, "performance-play", "Audition", false);
    if audition {
        cx.p.shared.audition(None);
    }

    row![
        row![prev_el, next_el].gap(0).shrink(0),
        col![
            title(name).text_weight(Weight::SEMIBOLD).lines(1),
            caption(facts.join("  ·  ")).fill(Role::Dim).lines(1)
        ]
        .gap(2)
        .align(Align::Start)
        .flex(1)
        .min_w(0),
        gain_el,
        pan_el,
        play_el.named("Audition the selected part"),
    ]
    .gap(WIDE)
    .align(Align::Center)
    .pad((WIDE, GAP + HALF))
    .shrink(0)
}

/// Load failures, missing samples and rack notices, in words a player can act on.
pub fn notices(cx: &Cx) -> Vec<El> {
    let mut out = Vec::new();
    if !cx.state.notice.is_empty() {
        out.push(banner(Role::Warning, cx.state.notice.clone()));
    }
    let v = cx.part_view();
    if cx.part().is_some() {
        if let Some(reason) = v.status.strip_prefix("Load failed: ") {
            let still = if v.active.is_empty() {
                String::new()
            } else {
                format!(" Still playing: {}.", v.active)
            };
            out.push(banner(
                Role::Danger,
                format!("This instrument could not be loaded: {reason}.{still}"),
            ));
        }
        if let Some(i) = current(cx) {
            if let Some(w) = i.warnings.iter().find(|w| w.contains("never downloaded")) {
                out.push(banner(Role::Warning, sentence(w)));
            } else if v.status.contains("zones skipped") || !i.missing_samples.is_empty() {
                let silent = i.zones.iter().filter(|z| !z.available).count();
                out.push(banner(
                    Role::Warning,
                    format!(
                        "Some samples are missing{}, so parts of this instrument stay silent. Repair the library in Native Access or check its folder.",
                        if silent > 0 { format!(" ({silent} zones)") } else { String::new() }
                    ),
                ));
            }
        }
    }
    if cx.state.tab == Tab::Rack && !cx.view.multi_status.is_empty() {
        out.push(
            caption(cx.view.multi_status.clone())
                .fill(Role::Dim)
                .lines(2)
                .pad((WIDE, 0)),
        );
    }
    if out.is_empty() {
        return out;
    }
    vec![col(out).gap(GAP).pad((WIDE, GAP + HALF)).shrink(0)]
}

/// Capitalize and end with a period.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    let mut out: String = chars
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    out.push_str(chars.as_str());
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

/// The instrument's own performance view, scaled to the space it has.
pub fn perform(ui: &mut Ui, cx: &mut Cx) -> El {
    let v = cx.part_view();
    let loaded = current(cx).is_some();
    let stage_size = ui
        .scene()
        .and_then(|s| s.surface("instrument-stage"))
        .map(|s| (s.frame.size.width, s.frame.size.height));
    let content = match (&v.interface, &v.wallpaper) {
        (Some(interface), wallpaper) if loaded => {
            performance_view(interface, wallpaper.as_ref(), stage_size)
        }
        (None, Some(image)) if loaded => block(Len::Pct(100.), Len::Pct(100.))
            .fill(Fill::Image(image.clone(), Fit::Contain))
            .named("Instrument wallpaper")
            .id("instrument-wallpaper"),
        _ => {
            let text = if !loaded {
                "Loading instrument…"
            } else if current(cx).is_some_and(|i| !i.scripts.is_empty()) {
                "This instrument's scripts run, but its performance view can't be shown yet."
            } else {
                "This instrument has no performance view. Play it from the keyboard below."
            };
            body(text).fill(Role::Dim).lines(3)
        }
    };
    col![
        spacer(),
        row![spacer(), content, spacer()].align(Align::Center),
        spacer()
    ]
    .align(Align::Center)
    .pad(WIDE)
    .flex(1)
    .min_h(0)
    .min_w(0)
    .clip()
    .id("instrument-stage")
}

/// Kontakt's performance view height includes a 68 px header its wallpaper leaves out.
const WALLPAPER_OFFSET: f64 = 68.;

/// Authored KSP coordinates, scaled uniformly to fit `stage` (last frame's size).
fn performance_view(
    interface: &Interface,
    image: Option<&Arc<Image>>,
    stage: Option<(f64, f64)>,
) -> El {
    let (iw, ih) = (
        f64::from(interface.width.max(1)),
        f64::from(interface.height.max(1)),
    );
    let scale = stage
        .map_or(1., |(w, h)| {
            ((w - 2. * WIDE) / iw).min((h - 2. * WIDE) / ih)
        })
        .clamp(0.4, 2.);
    let (width, height) = (iw * scale, ih * scale);
    let controls: Vec<Widget> = interface
        .controls
        .iter()
        .filter_map(|c| Widget::of(c, interface))
        .collect();

    let image = image.cloned();
    let shapes = controls.clone();
    let mut layers = vec![
        canvas(move |_| {
            let mut draw = Vec::new();
            if let Some(image) = &image {
                draw.push(Draw::image(
                    0.,
                    -WALLPAPER_OFFSET * scale,
                    f64::from(image.width) * scale,
                    f64::from(image.height) * scale,
                    image.clone(),
                ));
            }
            for w in &shapes {
                w.draw(scale, &mut draw);
            }
            draw
        })
        .w(width)
        .h(height)
        .at(0, 0)
        .id("instrument-wallpaper"),
    ];
    for (n, w) in controls.iter().enumerate() {
        if let Some(label) = w.label(scale) {
            layers.push(label.id(format!("ksp-control-{n}")));
        }
    }
    stack(layers)
        .w(width)
        .h(height)
        .fill(Role::Field)
        .radius(8)
        .clip()
        .shrink(0)
        .named(format!("{} performance view", interface.title))
        .id("ksp-preview")
}

/// One visible script control, in authored pixels.
#[derive(Clone)]
struct Widget {
    kind: Kind,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    text: String,
    /// 0..1 for knobs and sliders; on/off for buttons.
    value: f64,
    background: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Knob,
    Slider,
    Button,
    Menu,
    Label,
    Value,
    Other,
}

impl Widget {
    fn of(c: &Control, interface: &Interface) -> Option<Self> {
        let int = |name: &str| match c.properties.get(&format!("$CONTROL_PAR_{name}")) {
            Some(Value::Int(n)) => Some(*n),
            _ => None,
        };
        let text = |name: &str| match c.properties.get(&format!("$CONTROL_PAR_{name}")) {
            Some(Value::Text(s)) => s.clone(),
            _ => String::new(),
        };
        let hide = int("HIDE").unwrap_or(0);
        if hide & 1 != 0 {
            return None;
        }
        let kind = match c.kind.as_str() {
            "ui_knob" => Kind::Knob,
            "ui_slider" => Kind::Slider,
            "ui_button" | "ui_switch" => Kind::Button,
            "ui_menu" => Kind::Menu,
            "ui_label" => Kind::Label,
            "ui_value_edit" => Kind::Value,
            "ui_panel" | "ui_mouse_area" => return None,
            _ => Kind::Other,
        };
        let (x, y) = (int("POS_X").unwrap_or(0), int("POS_Y").unwrap_or(0));
        let default_h = if kind == Kind::Knob { 52 } else { 18 };
        let (w, h) = (
            int("WIDTH").unwrap_or(85),
            int("HEIGHT").unwrap_or(default_h),
        );
        if x < 0 || y < 0 || x >= interface.width || y >= interface.height || w <= 0 || h <= 0 {
            return None;
        }
        let raw = match c.properties.get("$CONTROL_PAR_VALUE") {
            Some(Value::Int(n)) => f64::from(*n),
            Some(Value::Real(r)) => *r,
            _ => 0.,
        };
        let (min, max) = (
            int("MIN_VALUE").unwrap_or(0),
            int("MAX_VALUE").unwrap_or(1_000_000),
        );
        let value = if max > min {
            ((raw - f64::from(min)) / f64::from(max - min)).clamp(0., 1.)
        } else {
            0.
        };
        let mut label = text("TEXT");
        if kind == Kind::Menu {
            label = c
                .menu
                .iter()
                .find(|(_, v)| f64::from(*v) == raw)
                .map(|(t, _)| t.clone())
                .unwrap_or_default();
        } else if kind == Kind::Value {
            label = format!("{raw}");
        } else if label.is_empty() && kind == Kind::Knob {
            label = c
                .variable
                .trim_start_matches(['$', '~', '?', '%', '@', '!'])
                .replace('_', " ");
        }
        if label.is_empty() && matches!(kind, Kind::Button | Kind::Label) {
            return None;
        }
        if hide & 8 != 0 {
            label.clear();
        }
        let w = if kind == Kind::Knob { w.max(40) } else { w };
        Some(Self {
            kind,
            x: f64::from(x),
            y: f64::from(y),
            w: f64::from(w.min(interface.width - x)),
            h: f64::from(h.min(interface.height - y)),
            text: label,
            value: if kind == Kind::Button {
                raw.min(1.)
            } else {
                value
            },
            background: hide & 2 == 0,
        })
    }

    /// Shapes: panels, knob arcs, slider tracks.
    fn draw(&self, scale: f64, out: &mut Vec<Draw>) {
        let (x, y, w, h) = (
            self.x * scale,
            self.y * scale,
            self.w * scale,
            self.h * scale,
        );
        let panel = Color::oklcha(0.16, 0.005, 260., 0.82);
        match self.kind {
            Kind::Knob => {
                let label = if self.text.is_empty() {
                    0.
                } else {
                    14. * scale
                };
                let d = (h - label).min(w).max(8.);
                let (cx, cy, r) = (x + w / 2., y + d / 2., d / 2. - 2. * scale);
                out.push(Draw::fill(circle(cx, cy, r), panel));
                let (from, sweep) = (0.75 * PI, 1.5 * PI);
                let stroke = (2.5 * scale).max(1.5);
                out.push(Draw::stroke(
                    arc(cx, cy, r - stroke, from, sweep),
                    Color::oklcha(1., 0., 0., 0.14),
                    stroke,
                ));
                if self.value > 0.001 {
                    out.push(Draw::stroke(
                        arc(cx, cy, r - stroke, from, sweep * self.value),
                        accent(),
                        stroke,
                    ));
                }
                let a = from + sweep * self.value;
                let (inner, outer) = (r * 0.25, r - stroke * 2.);
                out.push(Draw::stroke(
                    DrawPath::polyline(
                        [
                            Point::new(cx + a.cos() * inner, cy + a.sin() * inner),
                            Point::new(cx + a.cos() * outer, cy + a.sin() * outer),
                        ],
                        false,
                    ),
                    Color::oklch(0.95, 0., 0.),
                    stroke * 0.8,
                ));
            }
            Kind::Slider => {
                let t = (3. * scale).max(2.);
                let mid = y + h / 2.;
                out.push(Draw::fill(rect(x, mid - t / 2., w, t), panel));
                out.push(Draw::fill(
                    rect(x, mid - t / 2., w * self.value, t),
                    accent(),
                ));
                out.push(Draw::fill(
                    circle(x + w * self.value, mid, t * 1.8),
                    Color::oklch(0.95, 0., 0.),
                ));
            }
            Kind::Button => {
                let fill = if self.value >= 1. { accent() } else { panel };
                if self.background || self.value >= 1. {
                    out.push(Draw::fill(rounded(x, y, w, h, 4. * scale), fill));
                }
            }
            Kind::Menu | Kind::Value => {
                if self.background {
                    out.push(Draw::fill(rounded(x, y, w, h, 4. * scale), panel));
                }
                if self.kind == Kind::Menu && w > h * 2. {
                    // A small caret at the right edge.
                    let (cx, cy, r) = (x + w - h / 2., y + h / 2., (h * 0.16).max(2.));
                    let caret = [
                        (cx - r, cy - r / 2.),
                        (cx + r, cy - r / 2.),
                        (cx, cy + r / 2.),
                    ];
                    out.push(Draw::fill(
                        DrawPath::polyline(caret.map(|(x, y)| Point::new(x, y)), true),
                        Color::oklch(0.8, 0., 0.),
                    ));
                }
            }
            Kind::Label => {}
            Kind::Other => {
                out.push(Draw::stroke(
                    rounded(x, y, w, h, 4. * scale),
                    Color::oklcha(1., 0., 0., 0.12),
                    1.,
                ));
            }
        }
    }

    /// Text on top of the shapes.
    fn label(&self, scale: f64) -> Option<El> {
        if self.text.is_empty() {
            return None;
        }
        let size = (11. * scale).clamp(9., 16.);
        let (x, y, w, h) = (
            self.x * scale,
            self.y * scale,
            self.w * scale,
            self.h * scale,
        );
        let ink = match self.kind {
            Kind::Button if self.value >= 1. => Color::oklch(0.18, 0., 0.),
            _ => Color::oklch(0.93, 0., 0.),
        };
        let text = body(match self.kind {
            _ => self.text.clone(),
        })
        .text_size(size)
        .fill(ink)
        .lines(1);
        let (el, y, h) = match self.kind {
            Kind::Knob => {
                let line = 14. * scale;
                (
                    text.pad((HALF * scale, 0.))
                        .fill(Color::oklcha(0.16, 0.005, 260., 0.7))
                        .radius(3),
                    y + h - line,
                    line,
                )
            }
            Kind::Label => (
                text.pad((HALF * scale, 0.))
                    .fill(Color::oklcha(0.16, 0.005, 260., 0.7))
                    .radius(3),
                y,
                h,
            ),
            Kind::Button | Kind::Value => (text.justify(Justify::Center), y, h),
            _ => (text.pad((HALF * scale, 0.)), y, h),
        };
        Some(
            row![el]
                .justify(
                    if matches!(self.kind, Kind::Knob | Kind::Button | Kind::Value) {
                        Justify::Center
                    } else {
                        Justify::Start
                    },
                )
                .align(Align::Center)
                .w(w)
                .h(h)
                .at(x, y)
                .clip()
                .named(format!("Preview only: {}", self.text)),
        )
    }
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> DrawPath {
    DrawPath::polyline(
        [(x, y), (x + w, y), (x + w, y + h), (x, y + h)].map(|(x, y)| Point::new(x, y)),
        true,
    )
}

fn rounded(x: f64, y: f64, w: f64, h: f64, r: f64) -> DrawPath {
    let r = r.min(w / 2.).min(h / 2.);
    let corner = |cx: f64, cy: f64, start: f64| {
        (0..=6).map(move |n| {
            let a = start + f64::from(n) * PI / 12.;
            Point::new(cx + a.cos() * r, cy + a.sin() * r)
        })
    };
    DrawPath::polyline(
        corner(x + w - r, y + r, -PI / 2.)
            .chain(corner(x + w - r, y + h - r, 0.))
            .chain(corner(x + r, y + h - r, PI / 2.))
            .chain(corner(x + r, y + r, PI)),
        true,
    )
}

fn circle(cx: f64, cy: f64, r: f64) -> DrawPath {
    arc(cx, cy, r, 0., 2. * PI)
}

fn arc(cx: f64, cy: f64, r: f64, from: f64, sweep: f64) -> DrawPath {
    let steps = ((sweep.abs() / (2. * PI)) * 48.).ceil().max(2.) as usize;
    let closed = sweep >= 2. * PI - 1e-9;
    DrawPath::polyline(
        (0..=steps).map(|n| {
            let a = from + sweep * n as f64 / steps as f64;
            Point::new(cx + a.cos() * r, cy + a.sin() * r)
        }),
        closed,
    )
}

/// Groups on the left; the selected group's zones on a key × velocity grid.
pub fn mapping(ui: &mut Ui, cx: &mut Cx) -> El {
    let instrument = current(cx).cloned();
    let slot = cx.state.selected;
    let group = cx.part().map_or(0, |p| p.group);
    let mut groups = Vec::new();
    if let Some(i) = &instrument {
        for (n, g) in i.groups.iter().enumerate() {
            let label = if g.name.is_empty() {
                format!("Group {}", n + 1)
            } else {
                g.name.clone()
            };
            let (hit, el) = action(ui, format!("group-{n}"), &label, group == n as u32);
            if hit {
                cx.selection.parts[slot].group = n as u32;
            }
            groups.push(el.lines(1).min_w(0).w(Len::Pct(100.)).shrink(0));
        }
    }
    let zones: Vec<_> = instrument
        .iter()
        .flat_map(|i| i.zones.iter())
        .filter(|z| z.group == group as usize)
        .map(|z| {
            (
                z.low_key,
                z.high_key,
                z.low_velocity,
                z.high_velocity,
                z.available,
            )
        })
        .collect();
    let grid = canvas(move |s| {
        let mut draw = Vec::new();
        for n in (0..128).step_by(12) {
            draw.push(Draw::fill(
                rect(f64::from(n) / 128. * s.width, 0., 1., s.height),
                Role::Ink.alpha(0.08),
            ));
        }
        for v in [32., 64., 96.] {
            draw.push(Draw::fill(
                rect(0., s.height * (1. - v / 128.), s.width, 1.),
                Role::Ink.alpha(0.06),
            ));
        }
        for &(lo, hi, lv, hv, available) in &zones {
            let x = f64::from(lo) / 128. * s.width;
            let y = f64::from(127 - hv) / 128. * s.height;
            let w = f64::from(hi.saturating_sub(lo) + 1) / 128. * s.width;
            let h = (f64::from(hv.saturating_sub(lv) + 1) / 128. * s.height).max(1.);
            let fill = if available {
                Role::Primary.alpha(0.35)
            } else {
                Role::Danger.alpha(0.25)
            };
            draw.push(Draw::fill(
                rect(x, y, (w - 1.).max(1.), (h - 1.).max(1.)),
                fill,
            ));
        }
        draw
    })
    .flex(1)
    .min_h(0)
    .fill(Role::Field)
    .radius(8)
    .clip()
    .named("Selected group key and velocity mapping");
    row![
        col(groups)
            .gap(2)
            .w(220)
            .shrink(0)
            .min_h(0)
            .scroll()
            .id("groups-scroll"),
        col![
            grid,
            row![
                caption(note_name(0)),
                spacer(),
                caption("Key × velocity"),
                spacer(),
                caption(note_name(127))
            ]
            .shrink(0)
        ]
        .gap(GAP)
        .flex(1)
        .min_w(0)
        .min_h(0)
    ]
    .gap(WIDE)
    .pad(WIDE)
    .flex(1)
    .min_h(0)
}

/// What was loaded and what could not be.
pub fn info(cx: &Cx) -> El {
    let v = cx.part_view();
    let mut rows = Vec::new();
    if let Some(i) = current(cx) {
        rows.push(section("Instrument"));
        rows.push(
            body(format!(
                "{} groups · {} zones · {} missing sample references",
                i.groups.len(),
                i.zones.len(),
                i.missing_samples.len()
            ))
            .lines(3),
        );
        rows.push(
            caption(i.path.display().to_string())
                .fill(Role::Dim)
                .lines(4),
        );
        if !v.status.is_empty() {
            rows.push(caption(v.status.clone()).fill(Role::Dim).lines(3));
        }
        if !i.warnings.is_empty() {
            rows.push(section("Import notes").pad(edges(GAP, 0., 0., 0.)));
            for w in &i.warnings {
                rows.push(
                    body(w.as_str())
                        .text_size(12)
                        .fill(Role::Dim)
                        .lines(6)
                        .shrink(0),
                );
            }
        }
    }
    rows.push(section("Scripts"));
    rows.push(
        body("Scripts play the groups they choose; long samples stream from disk. The performance view is a preview: its controls don't respond yet.")
            .fill(Role::Dim)
            .text_size(12)
            .lines(4),
    );
    for line in v.interface_status.lines().filter(|l| !l.is_empty()) {
        rows.push(caption(line.to_owned()).fill(Role::Dim).lines(3).shrink(0));
    }
    if !v.wallpaper_status.is_empty() {
        rows.push(caption(v.wallpaper_status.clone()).fill(Role::Dim).lines(3));
    }
    col(rows)
        .gap(GAP)
        .pad(WIDE)
        .flex(1)
        .min_h(0)
        .scroll()
        .id("details-scroll")
}
