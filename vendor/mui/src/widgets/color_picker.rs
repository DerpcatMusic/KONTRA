//! One OKLCH color, several views: a square, a wheel or a triangle field
//! with a hue ring, or four sliders, and a CSS Color 4 text field. Layout
//! belongs to MUI; only the color geometry uses coordinates.
use std::sync::Arc;

use color::{AlphaColor, Hsl, Oklch, Srgb};
use mui_input::Button;
use mui_material::prelude::*;
use mui_scene::SpacingToken;

use crate::Ui;
use crate::widgets::{Response, Variant, button, presets, slider, text_input};

const CHROMA: f32 = 0.4;
const PIXELS: u32 = 256;

/// The field an [`oklch_picker`] shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PickerShape {
    #[default]
    Square,
    Wheel,
    Triangle,
    Sliders,
}

/// The notation of an [`oklch_picker`]'s text field and sliders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorFormat {
    #[default]
    Oklch,
    Rgb,
    Hsl,
    Hex,
}

/// An [`oklch_picker`]'s color and view; keep it between frames. The
/// shape and format tabs write `shape` and `format`.
pub struct OklchPicker {
    pub color: Color,
    pub shape: PickerShape,
    pub format: ColorFormat,
    draft: String,
    image: Option<((PickerShape, i16), Arc<Image>)>,
    ring: Option<bool>,
}
impl Default for OklchPicker {
    fn default() -> Self {
        Self::new(Color::oklch(0.67, 0.18, 265.0))
    }
}
impl OklchPicker {
    /// A wheel in OKLCH notation over `color`.
    pub fn new(color: Color) -> Self {
        Self {
            color,
            shape: PickerShape::Wheel,
            format: ColorFormat::Oklch,
            draft: String::new(),
            image: None,
            ring: None,
        }
    }
}

fn formatted(c: Color, format: ColorFormat) -> String {
    let [r, g, b, a] = c.to_srgb().components;
    let alpha = if a >= 0.999 {
        String::new()
    } else {
        format!(" / {a:.2}")
    };
    match format {
        ColorFormat::Oklch => format!(
            "oklch({:.0}% {:.3} {:.0}{alpha})",
            c.lightness() * 100.0,
            c.chroma(),
            c.hue()
        ),
        ColorFormat::Rgb => format!(
            "rgb({} {} {}{alpha})",
            (r * 255.0).round(),
            (g * 255.0).round(),
            (b * 255.0).round()
        ),
        ColorFormat::Hsl => {
            let [h, s, l, _] = c.to_srgb().convert::<Hsl>().components;
            format!("hsl({h:.0} {s:.0}% {l:.0}%{alpha})")
        }
        ColorFormat::Hex => {
            let (r, g, b, a) = (
                (r * 255.0).round() as u8,
                (g * 255.0).round() as u8,
                (b * 255.0).round() as u8,
                (a * 255.0).round() as u8,
            );
            if a == 255 {
                format!("#{r:02X}{g:02X}{b:02X}")
            } else {
                format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
            }
        }
    }
}

fn parse(s: &str) -> Option<Color> {
    let [l, c, h, a] = color::parse_color(s)
        .ok()?
        .to_alpha_color::<Oklch>()
        .components;
    (l.is_finite() && c.is_finite() && h.is_finite() && a.is_finite())
        .then(|| Color::oklcha(l.clamp(0.0, 1.0), c.max(0.0), h, a.clamp(0.0, 1.0)))
}

fn tabs<T: Copy + PartialEq>(ui: &mut Ui, id: &str, selected: &mut T, choices: &[(T, &str)]) -> El {
    row(choices.iter().map(|&(item, name)| {
        let tab = button(ui, format!("{id}/{name}"), name);
        if tab.changed {
            *selected = item;
        }
        let on = *selected == item;
        tab.size(SpacingToken::S)
            .role(if on { Role::Raised } else { Role::Ink })
            .variant(if on { Variant::Solid } else { Variant::Ghost })
            .into_el()
    }))
    .gap(2.0)
    .pad(2.0)
    .pill()
    .fill(Role::Field)
}

fn ring(x: f32, y: f32) -> bool {
    (x - 0.5).hypot(y - 0.5) >= 0.405
}
fn weights(x: f32, y: f32) -> [f32; 3] {
    let top = (0.76 - y) / 0.55;
    let right = (x - 0.2 - 0.3 * top) / 0.6;
    [top, 1.0 - top - right, right]
}
fn sample(shape: PickerShape, x: f32, y: f32, hue: f32) -> Option<Color> {
    if shape == PickerShape::Square {
        return Some(Color::oklch(1.0 - y, x * CHROMA, hue));
    }
    if ring(x, y) && (x - 0.5).hypot(y - 0.5) <= 0.5 {
        return Some(Color::oklch(
            0.7,
            0.22,
            (x - 0.5).atan2(0.5 - y).to_degrees().rem_euclid(360.0),
        ));
    }
    if shape == PickerShape::Wheel && (0.22..=0.78).contains(&x) && (0.22..=0.78).contains(&y) {
        return Some(Color::oklch(
            1.0 - (y - 0.22) / 0.56,
            (x - 0.22) / 0.56 * CHROMA,
            hue,
        ));
    }
    if shape == PickerShape::Triangle {
        let w = weights(x, y);
        if w.iter().all(|v| *v >= 0.0) {
            return Some(Color::oklch(w[0] + 0.65 * w[2], CHROMA * w[2], hue));
        }
    }
    None
}
fn bitmap(shape: PickerShape, hue: f32) -> Arc<Image> {
    let mut rgba = Vec::with_capacity((PIXELS * PIXELS * 4) as usize);
    for y in 0..PIXELS {
        for x in 0..PIXELS {
            if let Some(c) = sample(
                shape,
                (x as f32 + 0.5) / PIXELS as f32,
                (y as f32 + 0.5) / PIXELS as f32,
                hue,
            ) {
                let rgb = c.to_srgb().components;
                rgba.extend(rgb[..3].iter().map(|v| (v * 255.0).round() as u8));
                rgba.push(255);
            } else {
                rgba.extend([0; 4]);
            }
        }
    }
    Arc::new(Image::rgba(PIXELS, PIXELS, rgba).expect("fixed RGBA dimensions"))
}
fn marker(x: f64, y: f64) -> Path {
    Path::polyline(
        (0..24).map(|i| {
            let a = i as f64 * std::f64::consts::TAU / 24.0;
            Point::new(x + a.cos() * 6.0, y + a.sin() * 6.0)
        }),
        true,
    )
}
fn field(ui: &mut Ui, id: &Id, state: &mut OklchPicker) -> El {
    let target = format!("{id}/field");
    let response = ui.get(&target);
    let point = ui
        .local(&target)
        .zip(ui.scene().and_then(|s| s.surface(&target)))
        .map(|(p, s)| {
            (
                (p.x / s.frame.size.width.max(1.0)) as f32,
                (p.y / s.frame.size.height.max(1.0)) as f32,
            )
        });
    if response.pressed {
        state.ring = point.and_then(|(x, y)| {
            sample(state.shape, x, y, state.color.hue())
                .map(|_| state.shape != PickerShape::Square && ring(x, y))
        });
    }
    if response.held && response.button == Some(Button::Primary) {
        if let (Some(on_ring), Some((x, y))) = (state.ring, point) {
            let (x, y) = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
            let (mut l, mut c, mut h) = (
                state.color.lightness(),
                state.color.chroma(),
                state.color.hue(),
            );
            if on_ring {
                h = (x - 0.5).atan2(0.5 - y).to_degrees().rem_euclid(360.0);
            } else {
                (l, c) = match state.shape {
                    PickerShape::Square => (1.0 - y, x * CHROMA),
                    PickerShape::Wheel => (
                        1.0 - ((y - 0.22) / 0.56).clamp(0.0, 1.0),
                        ((x - 0.22) / 0.56).clamp(0.0, 1.0) * CHROMA,
                    ),
                    PickerShape::Triangle => {
                        let mut w = weights(x, y).map(|v| v.max(0.0));
                        let sum: f32 = w.iter().sum();
                        for v in &mut w {
                            *v /= sum.max(f32::EPSILON);
                        }
                        (w[0] + 0.65 * w[2], CHROMA * w[2])
                    }
                    PickerShape::Sliders => unreachable!(),
                };
            }
            state.color = Color::oklcha(l, c, h, state.color.alpha());
        }
    } else {
        state.ring = None;
    }
    let key = (state.shape, state.color.hue().round() as i16);
    if state.image.as_ref().is_none_or(|(old, _)| *old != key) {
        state.image = Some((key, bitmap(state.shape, key.1 as f32)));
    }
    let image = state.image.as_ref().unwrap().1.clone();
    let (l, c, h) = (
        state.color.lightness() as f64,
        (state.color.chroma() / CHROMA).clamp(0.0, 1.0) as f64,
        state.color.hue() as f64,
    );
    let mut points = match state.shape {
        PickerShape::Square => vec![(c, 1.0 - l)],
        PickerShape::Wheel => vec![(0.22 + 0.56 * c, 0.22 + 0.56 * (1.0 - l))],
        PickerShape::Triangle => {
            let t = (l - 0.65 * c).clamp(0.0, 1.0 - c);
            vec![(0.2 + 0.3 * t + 0.6 * c, 0.76 - 0.55 * t)]
        }
        PickerShape::Sliders => unreachable!(),
    };
    if state.shape != PickerShape::Square {
        let a = h.to_radians();
        points.push((0.5 + 0.453 * a.sin(), 0.5 - 0.453 * a.cos()));
    }
    canvas(move |size| {
        let rect = Path::polyline(
            [
                Point::new(0.0, 0.0),
                Point::new(size.width, 0.0),
                Point::new(size.width, size.height),
                Point::new(0.0, size.height),
            ],
            true,
        );
        let mut draws = vec![Draw::fill(rect, Fill::Image(image.clone(), Fit::Fill)).tag("field")];
        for (x, y) in &points {
            let path = marker(x * size.width, y * size.height);
            draws.push(Draw::stroke(
                path.clone(),
                Color::oklch(0.05, 0.0, 0.0),
                3.0,
            ));
            draws.push(Draw::stroke(path, Color::oklch(1.0, 0.0, 0.0), 1.5));
        }
        draws
    })
    .size(224.0, 224.0)
    .aspect(1.0)
    .shrink(1.0)
    .cursor(Cursor::Crosshair)
    .named("Color field")
    .id(target)
}

/// An intrinsic panel over one OKLCH [`Color`]: shape tabs, the field, format
/// tabs, a text field that takes any CSS Color 4 syntax, and sliders (all
/// four channels for [`PickerShape::Sliders`], else opacity). Returns the
/// panel and whether the color changed. The HSV [`color_picker`] is the
/// compact one.
///
/// [`color_picker`]: crate::widgets::color_picker
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut picker = OklchPicker::new(Color::oklch(0.7, 0.15, 30.0));
/// let panel = oklch_picker(&mut ui, "tint", &mut picker);
/// assert!(!panel.changed, "nothing dragged or typed");
/// ```
pub fn oklch_picker(ui: &mut Ui, id: impl Into<Id>, state: &mut OklchPicker) -> Response {
    let id: Id = id.into();
    let before = state.color;
    let shape_tabs = tabs(
        ui,
        &format!("{id}/shape"),
        &mut state.shape,
        &[
            (PickerShape::Square, "Square"),
            (PickerShape::Wheel, "Wheel"),
            (PickerShape::Triangle, "Tri"),
            (PickerShape::Sliders, "Rows"),
        ],
    );
    let format_tabs = tabs(
        ui,
        &format!("{id}/format"),
        &mut state.format,
        &[
            (ColorFormat::Oklch, "OKLCH"),
            (ColorFormat::Rgb, "RGB"),
            (ColorFormat::Hsl, "HSL"),
            (ColorFormat::Hex, "HEX"),
        ],
    );
    let visual = if state.shape == PickerShape::Sliders {
        Vec::new()
    } else {
        vec![field(ui, &id, state)]
    };
    let input_id = format!("{id}/value");
    if !ui.focused(&input_id) {
        state.draft = formatted(state.color, state.format);
    }
    let input = text_input(ui, input_id.as_str(), &mut state.draft);
    if input.changed
        && let Some(c) = parse(&state.draft)
    {
        state.color = c;
    }

    let mut values = match state.format {
        ColorFormat::Oklch | ColorFormat::Hex => [
            state.color.lightness() as f64 * 100.0,
            state.color.chroma() as f64,
            state.color.hue() as f64,
            state.color.alpha() as f64 * 100.0,
        ],
        ColorFormat::Rgb => {
            let [r, g, b, a] = state.color.to_srgb().components;
            [
                r as f64 * 255.0,
                g as f64 * 255.0,
                b as f64 * 255.0,
                a as f64 * 100.0,
            ]
        }
        ColorFormat::Hsl => {
            let [h, s, l, a] = state.color.to_srgb().convert::<Hsl>().components;
            [h as f64, s as f64, l as f64, a as f64 * 100.0]
        }
    };
    let names = match state.format {
        ColorFormat::Oklch | ColorFormat::Hex => ["Lightness", "Chroma", "Hue", "Opacity"],
        ColorFormat::Rgb => ["Red", "Green", "Blue", "Opacity"],
        ColorFormat::Hsl => ["Hue", "Saturation", "Lightness", "Opacity"],
    };
    let limits = match state.format {
        ColorFormat::Oklch | ColorFormat::Hex => [100.0, CHROMA as f64, 360.0, 100.0],
        ColorFormat::Rgb => [255.0, 255.0, 255.0, 100.0],
        ColorFormat::Hsl => [360.0, 100.0, 100.0, 100.0],
    };
    let mut changed = false;
    let channels = if state.shape == PickerShape::Sliders {
        0..4
    } else {
        3..4
    };
    let rows: Vec<_> = channels
        .map(|i| {
            let control = slider(
                ui,
                format!("{id}/{}", names[i]),
                names[i],
                &mut values[i],
                0.0..=limits[i],
            );
            changed |= control.changed;
            let readout = match names[i] {
                "Chroma" => format!("{:.3}", values[i]),
                "Hue" => format!("{:.0}°", values[i]),
                "Opacity" | "Lightness" | "Saturation" => format!("{:.0}%", values[i]),
                _ => format!("{:.0}", values[i]),
            };
            control.value_text(readout).into_el()
        })
        .collect();
    if changed {
        let [a, b, c, opacity] = values.map(|v| v as f32);
        state.color = if state.shape == PickerShape::Sliders {
            match state.format {
                ColorFormat::Oklch | ColorFormat::Hex => {
                    Color::oklcha(a / 100.0, b, c, opacity / 100.0)
                }
                ColorFormat::Rgb => Color::srgba(a / 255.0, b / 255.0, c / 255.0, opacity / 100.0),
                ColorFormat::Hsl => {
                    let [r, g, b, _] = AlphaColor::<Hsl>::new([a, b, c, opacity / 100.0])
                        .convert::<Srgb>()
                        .components;
                    Color::srgba(r, g, b, opacity / 100.0)
                }
            }
        } else {
            state.color.with_alpha(opacity / 100.0)
        };
        if !ui.focused(&input_id) {
            state.draft = formatted(state.color, state.format);
        }
    }
    let swatch = block(28.0, 28.0).radius(8.0).fill(state.color);
    let header = row([
        text("Color").text_size(18.0),
        spacer(),
        swatch,
        text(formatted(state.color, ColorFormat::Hex))
            .text_size(12.0)
            .fill(Role::Dim),
    ])
    .gap(S)
    .align(Align::Center);
    let mut sections = vec![header, shape_tabs];
    sections.extend(visual);
    sections.push(col([format_tabs, input.into_el(), col(rows).gap(S)]).gap(S));
    let panel = col(sections)
        .gap(L)
        .pad(L)
        .preset(presets::panel())
        .shadow(Shadow::soft(16.0))
        .id(id);
    Response {
        el: panel,
        changed: state.color != before,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_parse_and_every_shape_resolves() {
        for s in [
            "#3366cc80",
            "rgb(51 102 204 / 0.5)",
            "hsl(220 60% 50%)",
            "oklch(60% 0.1 240)",
        ] {
            assert!(parse(s).is_some(), "{s}");
        }
        let mut ui = Ui::new(Theme::DEFAULT);
        let mut picker = OklchPicker::default();
        for shape in [
            PickerShape::Square,
            PickerShape::Wheel,
            PickerShape::Triangle,
            PickerShape::Sliders,
        ] {
            picker.shape = shape;
            let tree = oklch_picker(&mut ui, "picker", &mut picker).el;
            ui.frame(
                tree,
                Some(Size::new(320.0, 800.0)),
                mui_input::Input::default(),
                0.0,
            )
            .unwrap();
        }
    }
}
