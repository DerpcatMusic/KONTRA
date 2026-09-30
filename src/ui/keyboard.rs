//! A playable keyboard in Kontakt's manner: keys that play samples are plain,
//! keys that don't are dimmed, and keys a script colored (keyswitches, usually)
//! wear that color on their whole face.

use super::{Cx, theme::*};
use crate::ksp::{KeyState, Value};
use crate::plugin::SamplerParams;
use moose::mui::mui::prelude::*;
use std::sync::atomic::Ordering;

const OCTAVES: i16 = 7;
/// Highest first octave that keeps the last key at or below MIDI 127.
const MAX_OCTAVE: i16 = (128 / 12) - OCTAVES;

/// The keyboard dock: a title bar with the shown range, octave stepping and
/// the collapse switch, then a range strip over the keys.
pub fn dock(ui: &mut Ui, cx: &mut Cx) -> El {
    let looks = looks(cx);
    // Center the keys on a newly shown instrument's range.
    let shown = cx.part().map(|p| (p.path.clone(), p.program));
    let used = |l: &Look| *l != Look::Unmapped;
    let span = looks
        .iter()
        .position(used)
        .map(|low| (low, looks.iter().rposition(used).unwrap_or(low)));
    if shown != cx.state.keyboard_for && cx.part_view().instrument.is_some() {
        if let Some((low, high)) = span {
            let octaves = (high / 12 - low / 12) as i16 + 1;
            let first = low as i16 / 12 - ((OCTAVES - octaves) / 2).max(0);
            cx.state.octave = first.clamp(0, MAX_OCTAVE);
        }
        cx.state.keyboard_for = shown;
    }

    let (down, down_el) = icon_button(ui, "octave-down", Icon::Left, "Octave down", false);
    let (up, up_el) = icon_button(ui, "octave-up", Icon::Right, "Octave up", false);
    if down || up {
        cx.p.shared.release_keyboard();
    }
    if down {
        cx.state.octave = (cx.state.octave - 1).max(0);
    }
    if up {
        cx.state.octave = (cx.state.octave + 1).min(MAX_OCTAVE);
    }
    let open = cx.state.keyboard;
    let (toggle, toggle_el) = icon_button(
        ui,
        "keyboard-toggle",
        if open { Icon::Down } else { Icon::Up },
        if open {
            "Hide the keyboard"
        } else {
            "Show the keyboard"
        },
        false,
    );
    if toggle {
        cx.state.keyboard = !open;
    }
    let first = cx.state.octave * 12;
    let shown_range = format!(
        "{} – {}",
        note_name(first as u8),
        note_name((first + OCTAVES * 12 - 1) as u8)
    );
    let plays = match span {
        Some((low, high)) if cx.part().is_some() => {
            format!("Plays {} – {}", note_name(low as u8), note_name(high as u8))
        }
        _ => String::new(),
    };
    let bar = row![
        section("Keyboard"),
        caption(shown_range).text_size(11).reserve("C#-2 – C#-2"),
        caption(plays)
            .text_size(11)
            .fill(Role::Dim)
            .lines(1)
            .flex(1)
            .min_w(0),
        down_el,
        up_el,
        toggle_el
    ]
    .gap(GAP + HALF)
    .align(Align::Center)
    .pad(edges(0., GAP, 0., GAP + HALF))
    .h(BAR)
    .shrink(0);
    if !open {
        return col![bar].gap(0).shrink(0).fill(Role::Surface);
    }

    let slot = cx.state.selected;
    let keys = cx.part_view().keys.clone();
    let mut octaves = Vec::new();
    for octave in cx.state.octave..cx.state.octave + OCTAVES {
        let mut make = |n: i16, black: bool| {
            let note = (octave * 12 + n) as u8;
            if ui.get(format!("key-{note}")).clicked_with(Button::Secondary) {
                super::menu::open(ui, cx, super::menu::Target::Key(note));
            }
            key(
                ui,
                cx.p,
                slot,
                note,
                black,
                looks[note as usize],
                keys.get(&note),
            )
        };
        let whites =
            row([0, 2, 4, 5, 7, 9, 11].map(|n| make(n, false).flex(1).min_w(0).h(Len::Pct(100.))))
                .gap(1);
        let mut blacks = Vec::new();
        for (gap, n) in BLACKS {
            blacks.push(spacer().flex(gap));
            blacks.push(make(n, true).flex(1.2).min_w(0).h(Len::Pct(100.)));
        }
        blacks.push(spacer().flex(1.4));
        octaves.push(
            stack([
                whites.w(Len::Pct(100.)).h(Len::Pct(100.)),
                row(blacks)
                    .gap(0)
                    .h(Len::Pct(60.))
                    .anchor(Align::Start, Align::Start)
                    .w(Len::Pct(100.)),
            ])
            .flex(1)
            .min_w(0)
            .h(Len::Pct(100.)),
        );
    }
    let strip = range_strip(looks, cx.state.octave);
    col![
        bar,
        col![strip, row(octaves).gap(1).h(76).clip()]
            .gap(2)
            .pad(edges(0., GAP + HALF, GAP, GAP + HALF))
    ]
    .gap(0)
    .shrink(0)
    .fill(Role::Surface)
}

/// The spacer before each black key and the key, in an octave 14 units wide.
const BLACKS: [(f64, i16); 5] = [(1.4, 1), (0.8, 3), (2.8, 6), (0.8, 8), (0.8, 10)];

/// Where note `n` of an octave sits, as a fraction of the octave's width.
fn span_in_octave(n: i16) -> (f64, f64) {
    const WHITES: [i16; 7] = [0, 2, 4, 5, 7, 9, 11];
    if let Some(i) = WHITES.iter().position(|w| *w == n) {
        return (i as f64 / 7., 1. / 7.);
    }
    let mut at = 0.;
    for (gap, key) in BLACKS {
        at += gap;
        if key == n {
            return (at / 14., 1.2 / 14.);
        }
        at += 1.2;
    }
    (0., 0.)
}

/// A thin bar over the keys: where the instrument plays, and its colored keys.
fn range_strip(looks: [Look; 128], octave: i16) -> El {
    canvas(move |s| {
        let octave_w = (s.width - f64::from(OCTAVES - 1)) / f64::from(OCTAVES);
        let mut draw = vec![Draw::fill(
            rect(0., 0., s.width, s.height),
            Role::Ink.alpha(0.06),
        )];
        for pass in [false, true] {
            for o in 0..OCTAVES {
                for n in 0..12 {
                    let note = ((octave + o) * 12 + n) as usize;
                    let Some(look) = looks.get(note) else {
                        continue;
                    };
                    let fill = match (pass, look) {
                        (false, Look::Mapped) => Role::Ink.alpha(0.45),
                        (true, Look::Colored(c)) => Fill::from(*c),
                        _ => continue,
                    };
                    let (x, w) = span_in_octave(n);
                    let left = f64::from(o) * (octave_w + 1.) + x * octave_w;
                    draw.push(Draw::fill(
                        rect(left.floor(), 0., (w * octave_w).ceil() + 1., s.height),
                        fill,
                    ));
                }
            }
        }
        draw
    })
    .w(Len::Pct(100.))
    .h(3)
    .shrink(0)
    .named("Key range")
}

/// What a key does for the selected part.
#[derive(Clone, Copy, PartialEq)]
enum Look {
    Unmapped,
    Mapped,
    Colored(Color),
}

fn looks(cx: &Cx) -> [Look; 128] {
    let mut looks = [Look::Unmapped; 128];
    let v = cx.part_view();
    if cx.part().is_none() {
        return looks;
    }
    if let Some(i) = &v.instrument {
        for z in i.zones.iter().filter(|z| z.available) {
            for n in z.low_key.min(127)..=z.high_key.min(127) {
                looks[n as usize] = Look::Mapped;
            }
        }
    }
    for (&note, state) in v.keys.iter() {
        if let Some(color) = state.color.as_ref().and_then(key_color) {
            looks[note.min(127) as usize] = color.map_or(Look::Mapped, Look::Colored);
        }
    }
    looks
}

fn key(
    ui: &mut Ui,
    p: &SamplerParams,
    slot: usize,
    note: u8,
    black: bool,
    look: Look,
    script: Option<&KeyState>,
) -> El {
    let id = format!("key-{note}");
    let r = ui.get(id.as_str());
    if r.pressed && r.button == Some(Button::Primary) {
        p.shared.press_key(slot, note);
    }
    if r.released {
        p.shared.release_key(note);
    }
    if r.key_activated {
        p.shared.audition(Some(note));
    }
    let held = p.shared.key_owners[note as usize].load(Ordering::Relaxed) < 128;
    let name = note_name(note);
    let face = match (held, look, black) {
        (true, ..) => accent(),
        (false, Look::Colored(c), false) => c,
        (false, Look::Colored(c), true) => Color::oklch(c.lightness() * 0.62, c.chroma(), c.hue()),
        (false, Look::Mapped, false) => Color::oklch(0.92, 0., 0.),
        (false, Look::Mapped, true) => Color::oklch(0.17, 0., 0.),
        (false, Look::Unmapped, false) => Color::oklch(0.56, 0., 0.),
        (false, Look::Unmapped, true) => Color::oklch(0.24, 0., 0.),
    };
    let mut parts = vec![spacer()];
    if note.is_multiple_of(12) {
        parts.push(
            caption(name.clone())
                .text_size(9)
                .fill(Color::oklcha(0., 0., 0., 0.55))
                .justify(Justify::Center)
                .shrink(0),
        );
    }
    let label = match script.map(|k| k.name.as_str()).filter(|n| !n.is_empty()) {
        Some(what) => format!("{name} · {what}"),
        None => name.clone(),
    };
    col(parts)
        .pad((2, 3))
        .fill(face)
        .on(State::Hover, move |s| {
            if held {
                s
            } else {
                let lift = if black { 0.12 } else { -0.06 };
                s.fill(Color::oklch((face.lightness() + lift).clamp(0., 1.), face.chroma(), face.hue()))
            }
        })
        .align(Align::Center)
        .focusable()
        .a11y(A11y::Button)
        .named(format!("Play {label}"))
        .tip(label)
        .id(id)
}

/// What a script's `$KEY_COLOR_*` does to a key: `None` leaves the zone
/// mapping's look, `Some(None)` shows a plain key (Kontakt's DEFAULT, WHITE,
/// BLACK, INACTIVE and anything unnamed), `Some(color)` colors it.
fn key_color(value: &Value) -> Option<Option<Color>> {
    let name = match value {
        Value::Text(name) => name
            .trim_start_matches('$')
            .trim_start_matches("KEY_COLOR_"),
        _ => return Some(None),
    };
    let hue = match name {
        "RED" => 25.,
        "ORANGE" => 50.,
        "LIGHT_ORANGE" => 65.,
        "WARM_YELLOW" => 80.,
        "YELLOW" => 100.,
        "LIME" => 125.,
        "GREEN" => 145.,
        "MINT" => 165.,
        "CYAN" => 195.,
        "TURQUOISE" => 210.,
        "BLUE" => 255.,
        "PLUM" => 290.,
        "VIOLET" => 300.,
        "PURPLE" => 315.,
        "MAGENTA" => 340.,
        "FUCHSIA" => 355.,
        "" | "NONE" => return None,
        // DEFAULT, WHITE, BLACK, INACTIVE and unnamed values.
        _ => return Some(None),
    };
    Some(Some(Color::oklch(0.68, 0.13, hue)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_key_colors() {
        let color = |name: &str| key_color(&Value::Text(name.into()));
        assert!((color("$KEY_COLOR_RED").unwrap().unwrap().hue() - 25.).abs() < 1.);
        assert_eq!(
            color("$KEY_COLOR_NONE"),
            None,
            "NONE falls back to the mapping"
        );
        assert_eq!(
            color("$KEY_COLOR_BLACK"),
            Some(None),
            "BLACK shows a plain key"
        );
        assert_eq!(color("$KEY_COLOR_DEFAULT"), Some(None), "DEFAULT is a plain key");
        const { assert!(MAX_OCTAVE * 12 + OCTAVES * 12 <= 128) };
    }
}
