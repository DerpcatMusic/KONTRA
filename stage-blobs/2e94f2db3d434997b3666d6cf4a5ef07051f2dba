//! A colour picker: a saturation-value square, a hue strip, an optional
//! alpha strip and a hex field, all over one [`Color`].
use mui_material::prelude::*;

use crate::Ui;
use crate::widgets::{Response, stepped, text_input};

/// How a [`color_picker`] behaves. The default is an opaque colour.
///
/// ```
/// use mui::prelude::*;
/// assert!(!ColorOpts::default().alpha);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColorOpts {
    /// Show the opacity strip and take `#rrggbbaa` in the hex field.
    pub alpha: bool,
}

/// Straight sRGB and alpha, each `0..1`.
type Rgba = [f32; 4];

/// A picker: drag in the square for saturation and value, along the strips
/// for hue and (with [`ColorOpts::alpha`]) opacity, or type a hex colour -- `#rrggbb`,
/// or `#rrggbbaa` with alpha. The strips step with the arrow keys when
/// focused. Returns the picker and whether the colour changed.
///
/// The hue is remembered while the colour is a grey or black, so dragging
/// saturation back up returns to the hue it left.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut tint = Color::srgb(0.2, 0.5, 0.9);
/// let picker = color_picker(&mut ui, "tint", &mut tint, ColorOpts { alpha: true });
/// assert!(!picker.changed, "nothing dragged or typed");
/// ```
pub fn color_picker(
    ui: &mut Ui,
    id: impl Into<Id>,
    value: &mut Color,
    opts: ColorOpts,
) -> Response {
    let (id, alpha): (Id, bool) = (id.into(), opts.alpha);
    let [sv, hue, opacity, hex] = ["sv", "hue", "alpha", "hex"].map(|k| format!("{id}/{k}"));
    let rgba = srgba(*value);
    // The hue and saturation a grey no longer carries: last frame's, if the
    // colour is still the one that frame made.
    let mut hsv = match ui.stash::<([f32; 3], Rgba)>(&sv) {
        Some((hsv, was)) if near(*was, rgba) => *hsv,
        _ => to_hsv(rgba),
    };
    let mut a = rgba[3];
    let before = (hsv, a);
    if let Some((x, y)) = grab(ui, &sv) {
        (hsv[1], hsv[2]) = (x, 1.0 - y);
    }
    if let Some((x, _)) = grab(ui, &hue) {
        hsv[0] = x * 360.0;
    }
    if let Some((x, _)) = grab(ui, &opacity) {
        a = x;
    }
    let mut h = f64::from(hsv[0]);
    if stepped(ui, &hue, &mut h, &(0.0..=360.0)) {
        hsv[0] = h as f32;
    }
    let mut al = f64::from(a);
    if stepped(ui, &opacity, &mut al, &(0.0..=1.0)) {
        a = al as f32;
    }
    let mut changed = (hsv, a) != before;
    if changed {
        let [r, g, b] = to_rgb(hsv);
        *value = Color::srgba(r, g, b, a);
    }
    // Typing: the field holds its own text while focused, so a half-typed
    // `#3a` is not overwritten by the colour it does not yet make.
    let mut text = match ui.stash::<String>(&hex) {
        Some(t) if ui.focused(&hex) => t.clone(),
        _ => to_hex(srgba(*value), alpha),
    };
    let Response {
        el: field,
        changed: typed,
    } = text_input(ui, hex.as_str(), &mut text);
    if typed && let Some(([r, g, b], ta)) = parse_hex(&text) {
        let ta = if alpha { ta.unwrap_or(a) } else { a };
        *value = Color::srgba(r, g, b, ta);
        hsv = to_hsv([r, g, b, ta]);
        changed = true;
    }
    let focused = ui.focused(&hex);
    ui.set_stash(&hex, focused.then_some(text));
    ui.set_stash(&sv, Some((hsv, srgba(*value))));

    let w = ui.theme().control * 48.0;
    let (sh, bar) = (w * 0.7, ui.theme().control * 4.0);
    let [r, g, b] = to_rgb([hsv[0], 1.0, 1.0]);
    let pure = Color::srgb(r, g, b);
    let white = Color::srgb(1.0, 1.0, 1.0);
    let black = Color::srgb(0.0, 0.0, 0.0);
    let ring = |d: f64| {
        block(d, d)
            .pill()
            .stroke(white)
            .stroke_width(2.0)
            .shell(1.0, black.with_alpha(0.5))
    };
    let square = stack([
        block(w, sh)
            .radius(4.0)
            .fill(Gradient::linear(90.0, [(0.0, white), (1.0, pure)])),
        block(w, sh).radius(4.0).fill(Gradient::linear(
            180.0,
            [(0.0, black.with_alpha(0.0)), (1.0, black)],
        )),
        ring(12.0).at(
            f64::from(hsv[1]) * w - 6.0,
            (1.0 - f64::from(hsv[2])) * sh - 6.0,
        ),
    ])
    .size(w, sh)
    .cursor(Cursor::Crosshair)
    .id(sv);
    let strip = |key: String, label: &str, t: f32, v: f64, max: f64, fill: Gradient| {
        stack([
            block(w, bar).pill().fill(fill),
            ring(bar + 4.0)
                .anchor(Align::Start, Align::Center)
                .offset(f64::from(t) * (w - bar - 4.0), 0.0),
        ])
        .size(w, bar + 4.0)
        .a11y(A11y::Slider {
            value: v,
            min: 0.0,
            max,
        })
        .named(label)
        .focusable()
        .id(key)
    };
    let rainbow = Gradient::linear(
        90.0,
        (0..=6).map(|k| {
            let [r, g, b] = to_rgb([k as f32 * 60.0, 1.0, 1.0]);
            (k as f32 / 6.0, Color::srgb(r, g, b))
        }),
    );
    let mut parts = vec![
        square,
        strip(
            hue,
            "Hue",
            hsv[0] / 360.0,
            f64::from(hsv[0]),
            360.0,
            rainbow,
        ),
    ];
    if alpha {
        let solid = *value;
        let ramp = Gradient::linear(
            90.0,
            [(0.0, solid.with_alpha(0.0)), (1.0, solid.with_alpha(1.0))],
        );
        parts.push(strip(opacity, "Alpha", a, f64::from(a), 1.0, ramp));
    }
    parts.push(
        row([
            block(bar * 2.0, bar * 2.0)
                .radius(4.0)
                .fill(*value)
                .stroke(Role::Ink.alpha(0.3))
                .stroke_width(1.0),
            field.grow(1.0),
        ])
        .gap(S)
        .align(Align::Center),
    );
    Response {
        el: col(parts).gap(S).w(w).id(id),
        changed,
    }
}

/// Where the press holding `id` is, as shares of its box, clamped to it.
fn grab(ui: &Ui, id: &str) -> Option<(f32, f32)> {
    let r = ui.get(id);
    if !(r.pressed || r.held) {
        return None;
    }
    let p = ui.local(id)?;
    let s = ui.scene()?.surface(id)?.frame.size;
    let share = |v: f64, n: f64| {
        if n > 0.0 {
            (v / n).clamp(0.0, 1.0) as f32
        } else {
            0.0
        }
    };
    Some((share(p.x, s.width), share(p.y, s.height)))
}

fn srgba(c: Color) -> Rgba {
    c.to_srgb().components
}

/// Within one 8-bit step: a host that stores the colour as bytes hands back
/// a rounded copy, which must still count as the colour this picker made.
fn near(a: Rgba, b: Rgba) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1.0 / 255.0)
}

fn to_hsv([r, g, b, _]: Rgba) -> [f32; 3] {
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    [h, if max > 0.0 { d / max } else { 0.0 }, max]
}

fn to_rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    let f = |n: f32| {
        let k = (n + h / 60.0).rem_euclid(6.0);
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [f(5.0), f(3.0), f(1.0)]
}

fn to_hex([r, g, b, a]: Rgba, alpha: bool) -> String {
    let byte = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut s = format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b));
    if alpha {
        s += &format!("{:02x}", byte(a));
    }
    s
}

/// `#rrggbb` or `#rrggbbaa`, the `#` optional; the alpha when it was given.
fn parse_hex(s: &str) -> Option<([f32; 3], Option<f32>)> {
    let s = s.trim().trim_start_matches('#');
    if !s.is_ascii() || !(s.len() == 6 || s.len() == 8) {
        return None;
    }
    let byte = |i: usize| {
        u8::from_str_radix(&s[i..i + 2], 16)
            .ok()
            .map(|v| f32::from(v) / 255.0)
    };
    let a = if s.len() == 8 { Some(byte(6)?) } else { None };
    Some(([byte(0)?, byte(2)?, byte(4)?], a))
}

#[cfg(test)]
mod tests {
    use super::near;

    #[test]
    fn a_byte_rounded_copy_is_still_the_colour_the_picker_made() {
        let made = [0.2013, 0.2102, 0.1981, 1.0];
        let stored = made.map(|x: f32| (x * 255.0).round() / 255.0);
        assert!(near(made, stored), "hue memory survives 8-bit storage");
        assert!(
            !near(made, [0.25, 0.21, 0.2, 1.0]),
            "a real change is not near"
        );
    }
}
