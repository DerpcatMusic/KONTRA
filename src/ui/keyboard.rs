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

pub fn strip(ui: &mut Ui, cx: &mut Cx) -> El {
    let looks = looks(cx);
    // Center the keys on a newly shown instrument's range.
    let shown = cx.part().map(|p| (p.path.clone(), p.program));
    if shown != cx.state.keyboard_for && cx.part_view().instrument.is_some() {
        let used = |l: &Look| *l != Look::Unmapped;
        if let Some(low) = looks.iter().position(used) {
            let high = looks.iter().rposition(used).unwrap_or(low);
            let span = (high / 12 - low / 12) as i16 + 1;
            let first = low as i16 / 12 - ((OCTAVES - span) / 2).max(0);
            cx.state.octave = first.clamp(0, MAX_OCTAVE);
        }
        cx.state.keyboard_for = shown;
    }

    let (down, down_el) = action(ui, "octave-down", "‹", false);
    let (up, up_el) = action(ui, "octave-up", "›", false);
    if down || up {
        cx.p.shared.release_keyboard();
    }
    if down {
        cx.state.octave = (cx.state.octave - 1).max(0);
    }
    if up {
        cx.state.octave = (cx.state.octave + 1).min(MAX_OCTAVE);
    }
    let first = cx.state.octave * 12;
    let range = format!(
        "{} – {}",
        note_name(first as u8),
        note_name((first + OCTAVES * 12 - 1) as u8)
    );
    let slot = cx.state.selected;
    let keys = cx.part_view().keys.clone();

    let mut octaves = Vec::new();
    for octave in cx.state.octave..cx.state.octave + OCTAVES {
        let mut make = |n: i16, black: bool| {
            let note = (octave * 12 + n) as u8;
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
        for (gap, n) in [(1.4, 1), (0.8, 3), (2.8, 6), (0.8, 8), (0.8, 10)] {
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
    row![
        col![
            caption(range).fill(Role::Dim).reserve("C#-2 – C#-2"),
            row![down_el, up_el].gap(HALF)
        ]
        .gap(HALF)
        .align(Align::Center)
        .shrink(0),
        row(octaves).gap(1).flex(1).min_w(0).h(84).clip()
    ]
    .gap(WIDE)
    .align(Align::Center)
    .pad((WIDE, GAP + HALF))
    .shrink(0)
    .fill(Role::Surface)
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
    if r.pressed {
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
        (false, Look::Mapped, false) => Color::oklch(0.94, 0., 0.),
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
