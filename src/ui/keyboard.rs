//! A playable keyboard in Kontakt's manner: keys that play samples are plain,
//! keys that don't are dimmed.
//! With a part selected it plays that
//! part; with none, every part MIDI channel 1 reaches, and a thin strip in
//! each part's color over the keys shows what each plays.

use super::{Cx, theme::*};
use crate::plugin::SamplerParams;
use moose::mui::mui::prelude::*;
use std::ops::RangeInclusive;
use std::sync::atomic::Ordering;

const OCTAVES: i16 = 7;
/// Highest first octave that keeps the last key at or below MIDI 127.
const MAX_OCTAVE: i16 = (128 / 12) - OCTAVES;

/// The keyboard dock: a title bar with the shown range, octave stepping and
/// the collapse switch, then a range strip over the keys.
pub fn dock(ui: &mut Ui, cx: &mut Cx) -> El {
    let parts = shown_parts(cx);
    let looks = looks(cx, &parts);
    // Center the keys on a newly shown instrument's range.
    let shown = parts.first().and_then(|&s| cx.selection.parts.get(s)).map(|p| (p.path.clone(), p.program));
    let used = |l: &Look| !matches!(l, Look::Unmapped);
    let span = looks
        .iter()
        .position(used)
        .map(|low| (low, looks.iter().rposition(used).unwrap_or(low)));
    if shown != cx.state.keyboard_for && shown.is_some() {
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
    // Esc lets go too, but a host may keep Esc for itself.
    let (all, all_el) = icon_button(ui, "keyboard-all", Icon::Close, "Show every part", false);
    if all {
        cx.state.selected_none();
    }
    let all_el = cx.state.chosen().is_some().then_some(all_el);
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
    let plays = match playable(&looks) {
        _ if cx.state.chosen().is_none() => "Every part · the keys play channel 1 or Omni · click a part to focus it".to_owned(),
        Some((low, high)) => format!("Plays {} – {}", note_name(low as u8), note_name(high as u8)),
        None => String::new(),
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
        caption(text).text_size(SMALL).fill(secondary()).shrink(0)
    });
    let bar = row(
        [
            section("Keyboard"),
            caption(shown_range).text_size(SMALL).reserve("C#-2 – C#-2"),
            caption(plays)
                .text_size(SMALL)
                .fill(secondary())
                .lines(1)
                .flex(1)
                .min_w(0),
        ]
        .into_iter()
        .chain(qwerty)
        .chain(all_el)
        .chain([cluster(vec![down_el, up_el, toggle_el])])
        .collect::<Vec<_>>(),
    )
    .gap(INSET)
    .align(Align::Center)
    .pad(edges(TIGHT, TIGHT, TIGHT, INSET))
    .shrink(0);
    let first_note = (cx.state.octave * 12) as u8;
    let shown = first_note..first_note + (OCTAVES * 12) as u8;
    // Hidden, the keys hold no pointer: a glissando still lets go.
    play(ui, cx, shown);
    if !open {
        return col![bar].gap(0).shrink(0).fill(Role::Surface);
    }

    let slot = cx.state.played();
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
            key(ui, cx.p, note, black, looks[note as usize], lit)
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
    let strip = match parts.len() {
        0 | 1 => range_strip(looks, cx.state.octave),
        _ => part_strips(cx, &parts),
    };
    let wheels = wheels(ui, cx.p, slot, &mut cx.state.modulation);
    col![
        bar,
        row![
            wheels,
            col![strip, row(octaves).gap(1).h(CONTROL * 3.).clip().id("keys")]
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
        {
            for o in 0..OCTAVES {
                for n in 0..12 {
                    let note = ((octave + o) * 12 + n) as usize;
                    let Some(look) = looks.get(note) else {
                        continue;
                    };
                    let Look::Mapped(hue) = look else { continue };
                    let fill = Fill::from(Color::oklch(0.66, 0.1, *hue));
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

/// Where `note` sits across a strip `width` wide showing `OCTAVES` from
/// `octave`: its left edge and width, as the keys under it lie.
fn key_x(note: usize, octave: i16, width: f64) -> (f64, f64) {
    let octave_w = (width - f64::from(OCTAVES - 1)) / f64::from(OCTAVES);
    let o = note as f64 / 12. - f64::from(octave);
    let (x, w) = span_in_octave((note % 12) as i16);
    (o.floor() * (octave_w + 1.) + x * octave_w, w * octave_w)
}

/// The parts the keys show: the selected one or, with none selected, every
/// loaded part on any port or channel, in rack order.
fn shown_parts(cx: &Cx) -> Vec<usize> {
    let loaded = |slot: usize| cx.selection.parts.get(slot).is_some_and(|p| !p.path.is_empty());
    match cx.state.chosen() {
        Some(slot) => loaded(slot).then_some(slot).into_iter().collect(),
        None => (cx.selection.order.iter().map(|&s| s as usize)).filter(|&slot| loaded(slot)).collect(),
    }
}

/// The keys part `slot`'s instrument maps, as its load report counted them.
fn mapped(cx: &Cx, slot: usize) -> [bool; 128] {
    let report = cx.view.parts.get(slot).and_then(|v| v.report.as_ref());
    std::array::from_fn(|k| report.is_some_and(|r| r.decoded.maps(k as u8)))
}

/// With several parts shown: a thin bar per part in its color from its
/// lowest playable key to its highest, parts whose ranges overlap on rows of
/// their own.
fn part_strips(cx: &mut Cx, shown: &[usize]) -> El {
    const ROW: f64 = 3.;
    let mut rows: Vec<Vec<(usize, usize, usize)>> = Vec::new();
    let mut tip = Vec::new();
    for &slot in shown {
        let looks = part_looks(cx, slot);
        let Some((low, high)) = playable(&looks) else { continue };
        tip.push(format!("{}: {} – {}", super::rack::name(cx, slot), note_name(low as u8), note_name(high as u8)));
        match rows.iter_mut().find(|r| r.iter().all(|&(_, l, h)| high < l || low > h)) {
            Some(r) => r.push((slot, low, high)),
            None => rows.push(vec![(slot, low, high)]),
        }
    }
    let (octave, count) = (cx.state.octave, rows.len().max(1));
    let (first, last) = ((octave * 12) as usize, ((octave + OCTAVES) * 12 - 1) as usize);
    canvas(move |s| {
        let mut draw = Vec::new();
        for (n, r) in rows.iter().enumerate() {
            let y = n as f64 * (ROW + 1.);
            for &(slot, low, high) in r.iter().filter(|(_, l, h)| *h >= first && *l <= last) {
                let (left, _) = key_x(low.max(first), octave, s.width);
                let (right, w) = key_x(high.min(last), octave, s.width);
                draw.push(Draw::fill(rect(left.floor(), y, (right + w).ceil() - left.floor(), ROW), part_color(slot)));
            }
        }
        draw
    })
    .w(Len::Pct(100.))
    .h(count as f64 * (ROW + 1.) - 1.)
    .shrink(0)
    .tip(tip.join("\n"))
    .named("What each part plays")
    .id("part-ranges")
}

/// What a key does for the parts shown.
#[derive(Clone, Copy, PartialEq)]
enum Look {
    Unmapped,
    /// Plays, tinted in its part's hue.
    Mapped(f32),
}

/// The lowest and highest keys that play notes.
fn playable(looks: &[Look; 128]) -> Option<(usize, usize)> {
    let plays = |l: &Look| matches!(l, Look::Mapped(_));
    let low = looks.iter().position(plays)?;
    Some((low, looks.iter().rposition(plays).unwrap_or(low)))
}

/// Part `slot`'s keys: those its zones map play, in its hue.
// TODO(v2): script key colors and articulation keyswitch marks.
fn part_looks(cx: &Cx, slot: usize) -> [Look; 128] {
    let hue = part_color(slot).hue();
    mapped(cx, slot).map(|m| if m { Look::Mapped(hue) } else { Look::Unmapped })
}

/// The shown parts' keys together.
fn looks(cx: &mut Cx, shown: &[usize]) -> [Look; 128] {
    let mut looks = [Look::Unmapped; 128];
    for &slot in shown {
        for (look, part) in looks.iter_mut().zip(part_looks(cx, slot)) {
            if *look == Look::Unmapped {
                *look = part;
            }
        }
    }
    looks
}

/// Mouse playing: a press starts the key under the pointer, dragging across
/// the keys moves the note along (a glissando) and letting go stops it.
/// Lower on a key plays louder, as on a real one.
///
/// Then whatever the keys hold that neither the glissando nor a held
/// computer key accounts for is let go, every frame: a note whose release
/// was lost (an editor dropped mid-drag, a window that never saw the
/// button come up) cannot outlive the next frame.
fn play(ui: &Ui, cx: &mut Cx, shown: std::ops::Range<u8>) {
    glide(ui, cx, shown);
    let shared = &cx.p.shared;
    let sounding = cx.state.gliss.map(|(_, n)| n);
    let typed = cx.state.computer.notes();
    for note in 0..128u8 {
        if shared.played[note as usize].load(Ordering::Relaxed) > 0
            && sounding != Some(note)
            && !typed.contains(&note)
        {
            shared.release_key(note);
        }
    }
}

fn glide(ui: &Ui, cx: &mut Cx, shown: std::ops::Range<u8>) {
    let shared = &cx.p.shared;
    let slot = cx.state.played();
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
) -> El {
    let id = format!("key-{note}");
    if ui.get(id.as_str()).key_activated {
        p.shared.audition(Some(note));
    }
    let held = lit > 0;
    // The pointer is on the key: a glissando's first key keeps the capture
    // (and so MUI's hover) after the pointer has moved on.
    let over = ui.get(id.as_str()).hovered;
    let name = note_name(note);
    let face = match (look, black) {
        // A key that plays is tinted in its part's hue: faintly on white, deeper on black.
        (Look::Mapped(hue), false) => Color::oklch(0.93, 0.035, hue),
        (Look::Mapped(hue), true) => Color::oklch(0.33, 0.075, hue),
        (Look::Unmapped, false) => Color::oklch(0.56, 0., 0.),
        (Look::Unmapped, true) => Color::oklch(0.24, 0., 0.),
    };
    // A sounding key lights like an LED under its top edge: neutral, strong
    // there and fading down the key, stronger the harder it is played. Dark
    // on a light key, light on a dark one, so it reads on any key's color.
    let v = f32::from(lit) / 127.;
    let light = Color::oklch(if face.lightness() > 0.6 { 0.3 } else { 0.95 }, 0., 0.);
    let strength = 0.55 + 0.45 * v;
    let led = block(Len::Pct(100.), Len::Pct(100.))
        .fill(Gradient::linear(
            180.,
            [
                (0., light.with_alpha(strength)),
                (0.12, light.with_alpha(0.8 * strength)),
                (0.55, light.with_alpha(0.3 * strength)),
                (1., light.with_alpha(0.)),
            ],
        ))
        .opacity(if held { 1. } else { 0. })
        // Lit at once, fading out when let go.
        .animate_with(if held { Spring::instant() } else { Spring::new(0.35, 1.) });
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
    let label = name.clone();
    let text = col(parts).pad((2, 3)).align(Align::Center).w(Len::Pct(100.)).h(Len::Pct(100.));
    stack![led, text]
        .fill(face)
        .on(State::Hover, move |s| {
            if held || !over {
                s
            } else {
                let lift = if black { 0.12 } else { -0.06 };
                s.fill(Color::oklch((face.lightness() + lift).clamp(0., 1.), face.chroma(), face.hue()))
            }
        })
        // Pressed is the LED's to show, on whichever key sounds.
        .on(State::Press, |s| s)
        .focusable()
        .a11y(A11y::Button)
        .named(format!("Play {label}"))
        .tip(label)
        .id(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keyboard_fits_midi() {
        const { assert!(MAX_OCTAVE * 12 + OCTAVES * 12 <= 128) };
    }
}
