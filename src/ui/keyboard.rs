//! A playable keyboard in Kontakt's manner: keys that play samples are plain,
//! keys that don't are dimmed, and keys a script colored (keyswitches, usually)
//! wear that color on their whole face.

use super::{Cx, theme::*};
use crate::ksp::{KeyState, Value};
use crate::plugin::SamplerParams;
use moose::mui::mui::prelude::*;
use std::ops::RangeInclusive;
use std::sync::Arc;
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
    let computer = &cx.state.computer;
    let qwerty = cx.selection.qwerty.then(|| {
        let from = computer.octave_c();
        let text = format!(
            "QWERTY {} – {} · velocity {}",
            note_name(from),
            note_name(from + 17),
            computer.velocity.load(Ordering::Relaxed)
        );
        caption(text).text_size(SMALL).fill(Role::Dim).shrink(0)
    });
    let bar = row(
        [
            section("Keyboard"),
            caption(shown_range).text_size(SMALL).reserve("C#-2 – C#-2"),
            caption(plays)
                .text_size(SMALL)
                .fill(Role::Dim)
                .lines(1)
                .flex(1)
                .min_w(0),
        ]
        .into_iter()
        .chain(qwerty)
        .chain([cluster(vec![down_el, up_el, toggle_el])])
        .collect::<Vec<_>>(),
    )
    .gap(INSET)
    .align(Align::Center)
    .pad(edges(TIGHT, TIGHT, TIGHT, INSET))
    .shrink(0);
    if !open {
        return col![bar].gap(0).shrink(0).fill(Role::Surface);
    }

    let slot = cx.state.selected;
    let first_note = (cx.state.octave * 12) as u8;
    let shown = first_note..first_note + (OCTAVES * 12) as u8;
    play(ui, cx, shown);
    let keys = cx.part_view().keys.clone();
    let mut octaves = Vec::new();
    for octave in cx.state.octave..cx.state.octave + OCTAVES {
        let mut make = |n: i16, black: bool| {
            let note = (octave * 12 + n) as u8;
            if ui.get(format!("key-{note}")).clicked_with(Button::Secondary) {
                super::menu::open(ui, cx, super::menu::Target::Key(note));
            }
            let lit = cx.p.shared.played[note as usize]
                .load(Ordering::Relaxed)
                .max(cx.p.shared.heard[note as usize].load(Ordering::Relaxed));
            key(ui, cx.p, note, black, looks[note as usize], lit, keys.get(&note))
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
    let wheels = wheels(ui, cx.p, slot, &mut cx.state.modulation);
    col![
        bar,
        row![
            wheels,
            col![strip, row(octaves).gap(1).h(CONTROL * 3.).clip()]
                .gap(1)
                .flex(1)
                .min_w(0)
        ]
        .gap(SPACE)
        .align(Align::End)
        .pad(edges(0., INSET, SPACE, INSET))
    ]
    .gap(0)
    .shrink(0)
    .fill(Role::Surface)
}

/// The pitch and mod wheels, left of the keys. Dragging bends or modulates
/// the selected part the way its keys play it; the pitch wheel springs back
/// to the middle when let go, the mod wheel stays. Both follow incoming MIDI.
fn wheels(ui: &mut Ui, p: &SamplerParams, slot: usize, held: &mut Option<f64>) -> El {
    let bend = f64::from(p.shared.bend.load(Ordering::Relaxed).min(16383));
    let mut at = (bend - 8192.) / 8192.;
    let before = at;
    let pitch = wheel(ui, "wheel-pitch", "Pitch bend", &mut at, -1.0..=1.0, 0.);
    if ui.get("wheel-pitch").released {
        at = 0.;
    }
    if at != before {
        p.shared.bend(slot, (8192. + at * 8192.).round().clamp(0., 16383.) as u16);
    }
    // A drag keeps its unrounded value between frames, so moves finer than
    // a step (a Shift drag) add up; MIDI moving the wheel meanwhile wins.
    let sent = f64::from(p.shared.modulation.load(Ordering::Relaxed).min(127));
    let mut depth = held.filter(|v| v.round() == sent).unwrap_or(sent);
    let modulation = wheel(ui, "wheel-mod", "Modulation (CC1)", &mut depth, 0.0..=127.0, 0.);
    *held = ui.get("wheel-mod").held.then_some(depth);
    if depth.round() != sent {
        p.shared.modulate(slot, depth.round() as u8);
    }
    row![pitch, modulation].gap(TIGHT).h(CONTROL * 3.).shrink(0)
}

/// A narrow vertical wheel over `range`, filled from `origin`: drag it,
/// scroll it, step it with the arrows; double-click returns it to `origin`.
fn wheel(ui: &mut Ui, id: &str, name: &str, value: &mut f64, range: RangeInclusive<f64>, origin: f64) -> El {
    let (lo, hi) = (*range.start(), *range.end());
    let r = ui.get(id);
    // The thumb follows the pointer: the wheel's own height is its travel.
    let travel = ui.scene().and_then(|s| s.surface(id)).map_or(CONTROL * 3., |s| s.frame.size.height);
    ui.drag(id, value, range.clone(), travel, true);
    if let Some(wheel) = ui.wheel(id) {
        *value = (*value - wheel.y.signum() * (hi - lo) / 50.).clamp(lo, hi);
    }
    stepped(ui, id, value, &range);
    if r.double_clicked {
        *value = origin;
    }
    let unit = |v: f64| ((v - lo) / (hi - lo)).clamp(0., 1.);
    let (at, from) = (unit(*value), unit(origin));
    let lift = ui.state(id).hover.max(if r.held { 1. } else { 0. }) as f32;
    let focused = ui.focus_visible(id);
    canvas(move |s| {
        let thumb = TIGHT;
        let y = |u: f64| (s.height - thumb) * (1. - u);
        let mid = (s.width / 2.).round();
        let mut draw = vec![
            Draw::fill(rect(0., 0., s.width, s.height), Role::Ink.alpha(0.08 + 0.04 * lift)),
            Draw::fill(rect(mid - 1., 0., 2., s.height), Role::Ink.alpha(0.14)),
        ];
        let (a, b) = if at > from { (y(at), y(from)) } else { (y(from), y(at)) };
        if b - a > 0.5 {
            draw.push(Draw::fill(rect(mid - 1., a + thumb / 2., 2., b - a), Role::Ink.alpha(0.6)));
        }
        if origin > lo {
            draw.push(Draw::fill(rect(0., (y(from) + thumb / 2.).round(), s.width, 1.), Role::Ink.alpha(0.3)));
        }
        draw.push(Draw::fill(rect(0., y(at).round(), s.width, thumb), Role::Ink.alpha(0.75 + 0.2 * lift)));
        if focused {
            draw.push(Draw::stroke(rect(0.5, 0.5, s.width - 1., s.height - 1.), Role::Primary.alpha(0.9), 1.));
        }
        draw
    })
    .w(SPACE * 2.)
    .h(Len::Pct(100.))
    .shrink(0)
    .cursor(Cursor::ResizeV)
    .focusable()
    .a11y(A11y::Slider { value: *value, min: lo, max: hi })
    .named(name.to_owned())
    .tip(format!("{name}: drag up or down"))
    .id(id.to_owned())
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
                        (false, Look::Mapped) => Fill::from(Color::oklch(0.66, 0.1, MAPPED_HUE)),
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

/// The green of a key that plays.
const MAPPED_HUE: f32 = 150.;

/// What a key does for the selected part.
#[derive(Clone, Copy, PartialEq)]
enum Look {
    Unmapped,
    Mapped,
    Colored(Color),
}

fn looks(cx: &mut Cx) -> [Look; 128] {
    let mut looks = [Look::Unmapped; 128];
    if cx.part().is_none() {
        return looks;
    }
    let v = &cx.view.parts[cx.state.selected];
    if let Some(i) = &v.instrument {
        // Tens of thousands of zones: walked once per instrument, not per frame.
        let (seen, mapped) = &mut cx.state.mapped;
        if !seen.upgrade().is_some_and(|s| Arc::ptr_eq(&s, i)) {
            *mapped = [false; 128];
            for z in i.zones.iter().filter(|z| z.available) {
                let (low, high) = (z.low_key.min(127) as usize, z.high_key.min(127) as usize);
                if low <= high {
                    mapped[low..=high].fill(true);
                }
            }
            *seen = Arc::downgrade(i);
        }
        for (look, &m) in looks.iter_mut().zip(mapped.iter()) {
            if m {
                *look = Look::Mapped;
            }
        }
    }
    for (&note, state) in v.keys.iter() {
        if let Some(look) = state.color.as_ref().and_then(key_color) {
            looks[note.min(127) as usize] = look;
        }
    }
    // A remapped keyswitch's color moves to the key that plays it.
    if let Some(p) = cx.part().filter(|p| p.articulate.source == p.path) {
        let a = &p.articulate;
        for (from, to) in a.articulations.iter().filter_map(|r| Some((r.key? as usize & 127, r.remap? as usize & 127))) {
            let look = looks[from];
            if !a.keep_original {
                looks[from] = if cx.state.mapped.1[from] { Look::Mapped } else { Look::Unmapped };
            }
            looks[to] = look;
        }
    }
    looks
}

/// Mouse playing: a press starts the key under the pointer, dragging across
/// the keys moves the note along (a glissando) and letting go stops it.
/// Lower on a key plays louder, as on a real one.
fn play(ui: &Ui, cx: &mut Cx, shown: std::ops::Range<u8>) {
    let shared = &cx.p.shared;
    let slot = cx.state.selected;
    for note in shown.clone() {
        let r = ui.get(format!("key-{note}"));
        if r.pressed && r.button == Some(Button::Primary) {
            if let Some((_, sounding)) = cx.state.gliss.take() {
                shared.release_key(sounding);
            }
            let velocity = key_under(ui, note..note + 1).map_or(100, |(_, v)| v);
            shared.press_key(slot, note, velocity);
            cx.state.gliss = Some((note, note));
        }
    }
    let Some((origin, sounding)) = cx.state.gliss else {
        return;
    };
    if !ui.get(format!("key-{origin}")).held {
        shared.release_key(sounding);
        cx.state.gliss = None;
        return;
    }
    if let Some((note, velocity)) = key_under(ui, shown).filter(|(n, _)| *n != sounding) {
        shared.release_key(sounding);
        shared.press_key(slot, note, velocity);
        cx.state.gliss = Some((origin, note));
    }
}

/// The key under the pointer among `notes`, and the velocity its height
/// plays: black keys first, as they lie over the white ones.
fn key_under(ui: &Ui, notes: std::ops::Range<u8>) -> Option<(u8, u8)> {
    let scene = ui.scene()?;
    let black = |n: &u8| matches!(n % 12, 1 | 3 | 6 | 8 | 10);
    let (blacks, whites): (Vec<u8>, Vec<u8>) = notes.partition(black);
    blacks.into_iter().chain(whites).find_map(|note| {
        let id = format!("key-{note}");
        let at = ui.local(id.as_str())?;
        let size = scene.surface(&id)?.frame.size;
        let inside = (0. ..size.width).contains(&at.x) && (0. ..size.height).contains(&at.y);
        let down = (at.y / size.height).clamp(0., 1.);
        inside.then(|| (note, (24. + 103. * down).round() as u8))
    })
}

fn key(
    ui: &mut Ui,
    p: &SamplerParams,
    note: u8,
    black: bool,
    look: Look,
    lit: u8,
    script: Option<&KeyState>,
) -> El {
    let id = format!("key-{note}");
    if ui.get(id.as_str()).key_activated {
        p.shared.audition(Some(note));
    }
    let held = lit > 0;
    let name = note_name(note);
    let face = match (look, black) {
        (Look::Colored(c), false) => c,
        (Look::Colored(c), true) => Color::oklch(c.lightness() * 0.62, c.chroma(), c.hue()),
        // A key that plays is tinted green: faintly on white, deeper on black.
        (Look::Mapped, false) => Color::oklch(0.93, 0.035, MAPPED_HUE),
        (Look::Mapped, true) => Color::oklch(0.33, 0.075, MAPPED_HUE),
        (Look::Unmapped, false) => Color::oklch(0.56, 0., 0.),
        (Look::Unmapped, true) => Color::oklch(0.24, 0., 0.),
    };
    // A sounding key takes the accent whatever its color, brighter the
    // harder it is played, ringed in a deeper amber so it reads even on
    // an orange key.
    let face = if held {
        let a = accent();
        let v = f32::from(lit) / 127.;
        Color::oklch(a.lightness() * if black { 0.85 } else { 1. } + 0.06 * v, a.chroma() + 0.03, a.hue())
    } else {
        face
    };
    let mut parts = vec![spacer()];
    if note.is_multiple_of(12) {
        parts.push(
            caption(name.clone())
                .text_size(SMALL - 2.)
                .fill(Color::oklch(0.38, 0., 0.))
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
        .when(held, |e| e.stroke(Color::oklch(0.45, 0.12, accent().hue())).stroke_width(2))
        // Lit at once, fading out when let go.
        .animate_with(if held { Spring::instant() } else { Spring::new(0.35, 1.) })
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
/// mapping's look (NONE, DEFAULT), INACTIVE dims it, WHITE, BLACK and
/// anything unnamed show a key that plays, a color colors it. Red is a
/// keyswitch's crisp red.
fn key_color(value: &Value) -> Option<Look> {
    let name = match value {
        Value::Text(name) => name
            .trim_start_matches('$')
            .trim_start_matches("KEY_COLOR_"),
        _ => return Some(Look::Mapped),
    };
    let hue = match name {
        "RED" => return Some(Look::Colored(keyswitch())),
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
        "" | "NONE" | "DEFAULT" => return None,
        "INACTIVE" => return Some(Look::Unmapped),
        // WHITE, BLACK and unnamed values.
        _ => return Some(Look::Mapped),
    };
    Some(Look::Colored(Color::oklch(0.68, 0.16, hue)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_key_colors() {
        let color = |name: &str| key_color(&Value::Text(name.into()));
        let Some(Look::Colored(red)) = color("$KEY_COLOR_RED") else {
            panic!("red colors a key")
        };
        assert!((red.hue() - 25.).abs() < 1. && red.chroma() > 0.18, "a crisp red");
        assert!(color("$KEY_COLOR_NONE").is_none(), "NONE falls back to the mapping");
        assert!(color("$KEY_COLOR_DEFAULT").is_none(), "DEFAULT falls back to the mapping");
        assert!(color("$KEY_COLOR_BLACK") == Some(Look::Mapped), "BLACK shows a key that plays");
        assert!(color("$KEY_COLOR_INACTIVE") == Some(Look::Unmapped), "INACTIVE is dim");
        const { assert!(MAX_OCTAVE * 12 + OCTAVES * 12 <= 128) };
    }
}
