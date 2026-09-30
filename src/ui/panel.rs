//! A script's performance view, rebuilt from our own controls.
//!
//! A Kontakt script places bitmap controls at pixel positions. Here each
//! visible control is read for what it is (knob, slider, switch, menu, value,
//! label) and given a name: its own text, the label beside it, or the words
//! in its picture or variable name. Then the geometry is read back as
//! structure: clusters far apart are sections, a label over a cluster titles
//! it, controls stacked under each other form columns (a mixer's channel
//! strips), controls side by side form rows. The result is laid out with our
//! own parts; pictures, wallpapers and positions are left behind.

use super::menu::{self, Target};
use super::{Cx, theme::*};
use crate::artwork::Picture;
use crate::ksp::{Control, Interface, Value};
use moose::mui::mui::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

/// Authored pixels between clusters that make them separate sections.
const SECTION_GAP: f64 = 16.;
/// Authored pixels between controls that still make them one column.
const COLUMN_GAP: f64 = 4.;
/// How far below or above a control its label may sit, in authored pixels.
const LABEL_REACH: f64 = 24.;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Face {
    Knob,
    Fader,
    VFader,
    Toggle,
    Menu,
    Value,
    Text,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn right(&self) -> f64 {
        self.x + self.w
    }
    fn bottom(&self) -> f64 {
        self.y + self.h
    }
    fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2., self.y + self.h / 2.)
    }
    fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
}

/// One control as we show it.
#[derive(Clone, Debug)]
pub struct Item {
    /// Index into `Interface::controls`.
    control: usize,
    face: Face,
    /// Filled from and marked at its middle: pan, balance, detune.
    bipolar: bool,
    name: String,
    /// The script's own value text (`$CONTROL_PAR_LABEL`).
    label: String,
    raw: f64,
    min: f64,
    max: f64,
    reset: f64,
    at: Rect,
}

/// A titled cluster of controls: columns of rows, left to right, top down.
#[derive(Clone, Debug)]
pub struct Section {
    /// Which band, top down: sections of one band share a line.
    band: usize,
    title: Option<String>,
    columns: Vec<Vec<Vec<Item>>>,
}

/// Text without glyphs from a library's private icon font, which ours can't
/// draw, and without the tabs and spaces scripts pad labels with.
pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(*c as u32, 0xE000..=0xF8FF))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Picture and variable name words that say what a control looks like, not
/// what it does.
const NOISE: &[&str] = &[
    "but", "button", "btn", "switch", "sw", "slider", "knob", "label", "lbl", "pic", "dark",
    "light", "big", "small", "toggle", "onoff", "bg", "img", "ver", "hor", "vert", "horiz",
    "k4", "bip", "clear", "empty", "of", "transparent", "select", "screen", "item", "byp",
    "on", "off", "fdr", "knb", "sli", "swi", "mnu", "gi", "gui", "tab", "dropdown", "arrow",
];

/// `sliUCVol` as `sli UC Vol`: camel-case humps are words.
fn humps(ident: &str) -> String {
    let c: Vec<char> = ident.chars().collect();
    let mut out = String::with_capacity(ident.len() + 4);
    for (i, &ch) in c.iter().enumerate() {
        let upper = ch.is_ascii_uppercase();
        let after_lower = i > 0 && c[i - 1].is_ascii_lowercase();
        let acronym_end = i > 0
            && c[i - 1].is_ascii_uppercase()
            && c.get(i + 1).is_some_and(char::is_ascii_lowercase);
        if upper && (after_lower || acronym_end) {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

/// Readable words from an identifier: `pyramid_but_classic_mix_1_of_2`
/// becomes "Classic Mix" when `prefix` is "pyramid". None when nothing is left.
fn words(ident: &str, prefix: &str) -> Option<String> {
    // A letter and digits (`t1`, `k4`) number a page or a skin, not a function.
    let numbering = |w: &str| {
        let mut c = w.chars();
        c.next().is_some_and(|f| f.is_alphabetic()) && c.all(|d| d.is_ascii_digit()) && w.len() > 1
    };
    let parts: Vec<String> = humps(ident.trim_start_matches(['$', '~', '?', '%', '@', '!']))
        .split(['_', '-', ' ', '.'])
        .map(str::to_lowercase)
        .filter(|w| !w.is_empty() && w != prefix)
        .filter(|w| !NOISE.contains(&w.as_str()) && !w.chars().all(|c| c.is_ascii_digit()))
        .filter(|w| !numbering(w))
        .collect();
    if parts.is_empty() {
        return None;
    }
    let title = |w: &String| {
        let mut c = w.chars();
        c.next()
            .map(|f| f.to_uppercase().chain(c).collect::<String>())
            .unwrap_or_default()
    };
    Some(parts.iter().map(title).collect::<Vec<_>>().join(" "))
}

/// A variable name a person wrote, not an obfuscator's `$q5nsy` or
/// `$zptkf`: words joined by `_`, or one word without digits that has
/// vowels enough to say.
fn readable(variable: &str) -> bool {
    let word = variable.trim_start_matches(['$', '~', '?', '%', '@', '!']);
    let vowels = word.chars().filter(|c| "aeiouyAEIOUY".contains(*c)).count();
    word.contains('_')
        || !word.chars().any(|c| c.is_ascii_digit()) && vowels * 10 >= word.len() * 3
}

/// The first word most picture names share: the library's own prefix.
fn picture_prefix(interface: &Interface) -> String {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut total = 0;
    for c in &interface.controls {
        if let Some(Value::Text(p)) = c.properties.get("$CONTROL_PAR_PICTURE")
            && let Some(first) = p.split(['_', '-']).next().filter(|f| !f.is_empty())
        {
            *counts.entry(first.to_lowercase()).or_default() += 1;
            total += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .filter(|(_, n)| *n * 2 > total && *n > 2)
        .map(|(p, _)| p)
        .unwrap_or_default()
}

/// A control read for what it is, before it has a name.
fn read(
    control: usize,
    c: &Control,
    interface: &Interface,
    pictures: &HashMap<String, Arc<Picture>>,
) -> Option<(Item, String)> {
    let prop = |name: &str| c.properties.get(&format!("$CONTROL_PAR_{name}"));
    let int = |name: &str| match prop(name) {
        Some(Value::Int(n)) => Some(f64::from(*n)),
        Some(Value::Real(r)) => Some(*r),
        _ => None,
    };
    let text = |name: &str| match prop(name) {
        Some(Value::Text(s)) => s.as_str(),
        _ => "",
    };
    // $HIDE_WHOLE_CONTROL; the other bits hide parts of a picture.
    if int("HIDE").is_some_and(|h| h as i32 & 16 != 0) {
        return None;
    }
    let picture_name = text("PICTURE");
    // A logo that opens a credits page is branding, not a control.
    if [picture_name, c.variable.as_str()]
        .iter()
        .any(|n| n.to_lowercase().contains("logo"))
    {
        return None;
    }
    let picture = pictures.get(picture_name);
    let knob = ["knob", "dial", "rotary"]
        .iter()
        .any(|k| picture_name.to_lowercase().contains(k));
    // Kontakt sizes a control to a picture that cannot stretch.
    let (w, h) = match picture {
        Some(p) if !p.resizable && !p.frames.is_empty() => {
            (f64::from(p.frames[0].width), f64::from(p.frames[0].height))
        }
        _ => (
            int("WIDTH").unwrap_or(85.),
            int("HEIGHT").unwrap_or(if c.kind == "ui_knob" { 52. } else { 18. }),
        ),
    };
    let at = Rect {
        x: int("POS_X").unwrap_or(0.),
        y: int("POS_Y").unwrap_or(0.),
        w,
        h,
    };
    let (iw, ih) = (f64::from(interface.width), f64::from(interface.height));
    if at.x < 0. || at.y < 0. || at.x >= iw || at.y >= ih || w <= 0. || h <= 0. {
        return None;
    }
    let square = (0.6..=1.6).contains(&(w / h)) && w.min(h) >= 24.;
    let face = match c.kind.as_str() {
        "ui_knob" => Face::Knob,
        "ui_slider" if knob || square => Face::Knob,
        "ui_slider" if w >= h => Face::Fader,
        "ui_slider" => Face::VFader,
        "ui_switch" | "ui_button" => Face::Toggle,
        "ui_menu" => Face::Menu,
        "ui_value_edit" => Face::Value,
        "ui_label" => Face::Text,
        _ => return None,
    };
    let raw = match prop("VALUE") {
        Some(Value::Int(n)) => f64::from(*n),
        Some(Value::Real(r)) => *r,
        _ => 0.,
    };
    let (min, max) = match face {
        Face::Toggle => (0., 1.),
        Face::Menu => (0., 0.),
        _ => (int("MIN_VALUE").unwrap_or(0.), int("MAX_VALUE").unwrap_or(1_000_000.)),
    };
    let own = clean(text("TEXT"));
    let mentions_pan = |s: &str| s.to_lowercase().split(['_', ' ', '$']).any(|w| w == "pan");
    let bipolar = matches!(face, Face::Knob | Face::Fader | Face::VFader)
        && (picture_name.to_lowercase().contains("bip")
            || mentions_pan(&own)
            || mentions_pan(&c.variable));
    let reset = int("DEFAULT_VALUE").unwrap_or(if bipolar { (min + max) / 2. } else { min });
    let item = Item {
        control,
        face,
        bipolar,
        name: own,
        label: clean(text("LABEL")),
        raw,
        min,
        max,
        reset,
        at,
    };
    Some((item, picture_name.to_owned()))
}

/// Visible controls, named, with the labels that named them taken out.
fn items(interface: &Interface, pictures: &HashMap<String, Arc<Picture>>) -> Vec<Item> {
    let prefix = picture_prefix(interface);
    let mut read: Vec<(Item, String)> = interface
        .controls
        .iter()
        .enumerate()
        .filter_map(|(n, c)| read(n, c, interface, pictures))
        // A label that is only a number is a readout; ours show the value.
        .filter(|(i, _)| {
            i.face != Face::Text || i.name.chars().any(char::is_alphabetic)
        })
        .collect();
    // Continuous controls take the label nearest them first, then switches.
    let mut order: Vec<usize> = (0..read.len())
        .filter(|&n| read[n].0.face != Face::Text && read[n].0.name.is_empty())
        .collect();
    order.sort_by_key(|&n| read[n].0.face == Face::Toggle);
    let mut taken = vec![false; read.len()];
    for n in order {
        let c = read[n].0.at;
        let (cx, cy) = c.center();
        let best = read
            .iter()
            .enumerate()
            .filter(|(m, (l, _))| l.face == Face::Text && !taken[*m])
            .filter(|(_, (l, _))| {
                let (lx, _) = l.at.center();
                let beside = lx >= c.x - SPACE && lx <= c.right() + SPACE;
                let below = l.at.y >= c.bottom() - c.h / 4. && l.at.y <= c.bottom() + LABEL_REACH;
                let above = l.at.bottom() <= c.y + c.h / 4. && l.at.bottom() >= c.y - LABEL_REACH;
                l.at.overlaps(&c) || beside && (below || above)
            })
            .min_by(|(_, (a, _)), (_, (b, _))| {
                let d = |r: &Rect| {
                    let (x, y) = r.center();
                    (x - cx).hypot(y - cy)
                };
                d(&a.at).total_cmp(&d(&b.at))
            })
            .map(|(m, _)| m);
        if let Some(m) = best {
            taken[m] = true;
            read[n].0.name = read[m].0.name.clone();
        }
    }
    for (n, (item, picture)) in read.iter_mut().enumerate() {
        if taken[n] || !item.name.is_empty() {
            continue;
        }
        let variable = &interface.controls[item.control].variable;
        // A picture that says no more than a couple of letters ("Uc") yields
        // to a readable variable name.
        let from_picture = words(picture, &prefix);
        let from_variable = readable(variable).then(|| words(variable, "")).flatten();
        item.name = match from_picture {
            Some(p) if p.len() <= 3 && from_variable.is_some() => from_variable,
            Some(p) => Some(p),
            None => from_variable,
        }
            .or_else(|| item.bipolar.then(|| "Pan".to_owned()))
            .unwrap_or_default();
    }
    let kept: Vec<Item> = read
        .into_iter()
        .enumerate()
        .filter(|(n, (i, _))| !taken[*n] && (!i.name.is_empty() || i.face == Face::Menu))
        .map(|(_, (i, _))| i)
        .collect();
    // A one-letter switch reads only beside its siblings (a strip's M and S);
    // alone, like Vista's corner "B", it is a mark, not a control.
    kept.iter()
        .filter(|i| {
            i.face != Face::Toggle
                || i.name.chars().filter(|c| c.is_alphanumeric()).count() > 1
                || kept.iter().any(|o| {
                    !std::ptr::eq(*i, o) && o.face == Face::Toggle && touching(&i.at, &o.at)
                })
        })
        .cloned()
        .collect()
}

/// Side by side on one line, at most a spacing apart: one segmented control.
fn touching(a: &Rect, b: &Rect) -> bool {
    let (left, right) = if a.x <= b.x { (a, b) } else { (b, a) };
    right.x - left.right() <= 2. && (a.y - b.y).abs() < a.h.min(b.h) / 2.
}

/// Groups of `items` whose horizontal extents touch within `gap`, left to right.
fn columns(mut items: Vec<Item>, gap: f64) -> Vec<Vec<Item>> {
    items.sort_by(|a, b| a.at.x.total_cmp(&b.at.x));
    let mut out: Vec<(f64, Vec<Item>)> = Vec::new();
    for item in items {
        match out.last_mut() {
            Some((right, group)) if item.at.x <= *right + gap => {
                *right = right.max(item.at.right());
                group.push(item);
            }
            _ => out.push((item.at.right(), vec![item])),
        }
    }
    out.into_iter().map(|(_, g)| g).collect()
}

/// Groups of `items` whose vertical extents touch within `gap`, top down:
/// bands split only where a gap runs the panel's whole width.
fn bands(mut items: Vec<Item>, gap: f64) -> Vec<Vec<Item>> {
    items.sort_by(|a, b| a.at.y.total_cmp(&b.at.y));
    let mut out: Vec<(f64, Vec<Item>)> = Vec::new();
    for item in items {
        match out.last_mut() {
            Some((bottom, group)) if item.at.y <= *bottom + gap => {
                *bottom = bottom.max(item.at.bottom());
                group.push(item);
            }
            _ => out.push((item.at.bottom(), vec![item])),
        }
    }
    out.into_iter().map(|(_, g)| g).collect()
}

/// Which controls share a row: a row of switches, of knobs, of faders or of
/// fields, never a mix, so each row has one height and one baseline.
fn kind(face: Face) -> u8 {
    match face {
        Face::Toggle => 0,
        Face::Knob | Face::VFader => 1,
        Face::Fader => 2,
        Face::Menu | Face::Value | Face::Text => 3,
    }
}

/// Rows of a column: items whose vertical extents mostly overlap, top down,
/// each left to right, split by [`kind`].
fn rows(mut items: Vec<Item>) -> Vec<Vec<Item>> {
    items.sort_by(|a, b| a.at.y.total_cmp(&b.at.y));
    let mut out: Vec<(f64, f64, Vec<Item>)> = Vec::new();
    for item in items {
        let (top, bottom) = (item.at.y, item.at.bottom());
        match out.last_mut() {
            Some((t, b, row)) if top.max(*t) < bottom.min(*b) - item.at.h.min(*b - *t) / 2. => {
                *b = b.max(bottom);
                row.push(item);
            }
            _ => out.push((top, bottom, vec![item])),
        }
    }
    let mut split = Vec::new();
    for (_, _, mut row) in out {
        row.sort_by(|a, b| a.at.x.total_cmp(&b.at.x));
        for k in 0..4 {
            let part: Vec<Item> = row.iter().filter(|i| kind(i.face) == k).cloned().collect();
            if !part.is_empty() {
                split.push(part);
            }
        }
    }
    split
}

/// The interface's controls, grouped as they were drawn.
pub fn sections(interface: &Interface, pictures: &HashMap<String, Arc<Picture>>) -> Vec<Section> {
    bands(items(interface, pictures), SECTION_GAP)
        .into_iter()
        .enumerate()
        .flat_map(|(band, items)| columns(items, SECTION_GAP).into_iter().map(move |g| (band, g)))
        .map(|(band, mut group)| {
            let top = group
                .iter()
                .filter(|i| i.face != Face::Text)
                .map(|i| i.at.y)
                .fold(f64::INFINITY, f64::min);
            let title = group
                .iter()
                .enumerate()
                .filter(|(_, i)| i.face == Face::Text && i.at.bottom() <= top + COLUMN_GAP)
                .min_by(|(_, a), (_, b)| a.at.y.total_cmp(&b.at.y))
                .map(|(n, _)| n);
            let title = title.map(|n| group.remove(n).name);
            Section {
                band,
                title,
                columns: columns(group, COLUMN_GAP).into_iter().map(rows).collect(),
            }
        })
        .filter(|s| !s.columns.is_empty())
        .collect()
}

/// `part`'s script controls, rebuilt: bands top down, each a wrapping line
/// of sections side by side.
pub fn view(ui: &mut Ui, cx: &mut Cx, part: usize, sections: &[Section]) -> El {
    let mut bands: Vec<Vec<El>> = Vec::new();
    // Every section opens with a title rule when any has a title, so their
    // first rows share a line.
    let titled = sections.iter().any(|s| s.title.is_some());
    for section in sections {
        let mut columns = Vec::new();
        for column in &section.columns {
            let mut lines = Vec::new();
            for line in column {
                let switches = line.len() > 1
                    && line.iter().all(|i| i.face == Face::Toggle)
                    && line.windows(2).all(|w| touching(&w[0].at, &w[1].at));
                let knobs = kind(line[0].face) == 1;
                // A lone field, fader, menu or switch spans its column, so
                // labels, values and edges line up down the column.
                let span = line.len() == 1 && kind(line[0].face) != 1;
                let els: Vec<El> = line
                    .iter()
                    .map(|i| control(ui, cx, part, i))
                    .map(|el| if span { el.flex(1).min_w(0) } else { el })
                    .collect();
                lines.push(if switches {
                    segmented(els.into_iter().map(|e| e.flex(1)).collect())
                } else {
                    // Knob cells are centred stacks; their row is centred too.
                    row(els)
                        .gap(if knobs { INSET } else { SPACE })
                        .align(Align::End)
                        .justify(if knobs { Justify::Center } else { Justify::Start })
                        .shrink(0)
                });
            }
            columns.push(col(lines).gap(SPACE).align(Align::Stretch).shrink(0));
        }
        let body = row(columns).gap(INSET).align(Align::Start).shrink(0);
        while bands.len() <= section.band {
            bands.push(Vec::new());
        }
        bands[section.band].push(match (&section.title, titled) {
            (Some(t), _) => col![section_title(t), body],
            (None, true) => col![section_title(""), body],
            (None, false) => col![body],
        }
        .gap(SPACE)
        .align(Align::Stretch)
        .shrink(0));
    }
    col(bands
        .into_iter()
        .filter(|b| !b.is_empty())
        .map(|b| row(b).wrap().gap(INSET * 2.).line_gap(INSET).align(Align::Start).w(Len::Pct(100.)))
        .collect::<Vec<_>>())
        .gap(INSET)
        .align(Align::Start)
        .pad(INSET)
        .w(Len::Pct(100.))
        .named("Performance controls")
}

fn section_title(title: &str) -> El {
    col![section(title), rule()]
        .gap(TIGHT)
        .align(Align::Start)
        .w(Len::Pct(100.))
}

/// What a knob or fader reads: the script's own text, as Kontakt shows it
/// (Vista's "0.0" is decibels, not a raw value), else pan or a percentage.
fn readout(item: &Item, value: f64) -> String {
    let unit = if item.max > item.min {
        ((value - item.min) / (item.max - item.min)).clamp(0., 1.)
    } else {
        0.
    };
    if !item.label.is_empty() {
        item.label.clone()
    } else if item.bipolar {
        pan_text(unit * 2. - 1.)
    } else {
        format!("{:.0} %", unit * 100.)
    }
}

/// One control, wired to the script: every edit runs its `on ui_control`.
fn control(ui: &mut Ui, cx: &mut Cx, part: usize, item: &Item) -> El {
    let id = format!("ksp-{part}-{}", item.control);
    let name = item.name.as_str();
    let range = item.min..=item.max.max(item.min + 1.);
    match item.face {
        Face::Knob | Face::Fader | Face::VFader => {
            // Drag travel finer than one step accumulates while held.
            let key = (part, item.control);
            let mut value = match cx.state.held {
                Some((p, c, v)) if (p, c) == key => v,
                _ => item.raw,
            };
            let kind = if item.bipolar {
                Fader::bipolar(&range, item.reset, "R 100")
            } else {
                Fader::over(&range, item.reset, "100 %")
            };
            let (_, el) = match item.face {
                Face::Knob => dial(ui, &id, name, &mut value, range, kind),
                Face::VFader => fader(ui, &id, name, &mut value, range, kind.vertical(), |v| {
                    readout(item, v)
                }),
                _ => fader(ui, &id, name, &mut value, range, kind, |v| readout(item, v)),
            };
            if ui.get(id.as_str()).held {
                cx.state.held = Some((part, item.control, value));
            } else if cx.state.held.is_some_and(|(p, c, _)| (p, c) == key) {
                cx.state.held = None;
            }
            let rounded = value.round();
            if rounded != item.raw {
                cx.p.shared.edit_control(part, item.control, rounded as i32);
            }
            let title = caption(name.to_owned())
                .text_size(SMALL)
                .fill(Role::Dim)
                .lines(1);
            match item.face {
                Face::Knob => col![
                    el,
                    title.max_size(Size::new(KNOB * 3., CONTROL)),
                    caption(readout(item, value)).text_size(SMALL).lines(1)
                ]
                .gap(TIGHT)
                .align(Align::Center)
                .min_w(KNOB)
                .shrink(0),
                Face::VFader => col![el, title].gap(TIGHT).align(Align::Center).shrink(0),
                // Names sit above, left; values at the right end.
                _ => col![title, el.align_self(Align::Stretch)]
                    .gap(TIGHT)
                    .align(Align::Start)
                    .min_w(CONTROL * 5.)
                    .shrink(0),
            }
        }
        Face::Toggle => {
            let on = item.raw >= 1.;
            let (hit, el) = latch(ui, id.as_str(), name, name, on);
            if hit {
                cx.p.shared.edit_control(part, item.control, i32::from(!on));
            }
            el
        }
        Face::Menu => {
            let current = cx
                .view
                .parts
                .get(part)
                .and_then(|v| v.interface.as_ref())
                .and_then(|i| i.controls.get(item.control))
                .and_then(|c| c.menu.iter().find(|(_, v)| f64::from(*v) == item.raw))
                .map(|(t, _)| clean(t))
                .unwrap_or_default();
            let (hit, el) = dropdown(ui, id.as_str(), &current, if name.is_empty() { "Menu" } else { name });
            if hit {
                let target = Target::Script {
                    part,
                    control: item.control,
                };
                menu::open_under(ui, cx, target, &id);
            }
            if name.is_empty() {
                el
            } else {
                col![
                    caption(name.to_owned()).text_size(SMALL).fill(Role::Dim).lines(1),
                    el
                ]
                .gap(TIGHT)
                .shrink(0)
            }
        }
        Face::Value => {
            let mut value = item.raw;
            let el = number(ui, id.as_str(), name, &mut value, range, format!("{}", item.raw));
            if value.round() != item.raw {
                cx.p.shared.edit_control(part, item.control, value.round() as i32);
            }
            el
        }
        Face::Text => caption(name.to_owned()).fill(Role::Dim).lines(1).min_w(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn control(kind: &str, var: &str, props: &[(&str, Value)]) -> Control {
        Control {
            variable: var.into(),
            kind: kind.into(),
            properties: props
                .iter()
                .map(|(k, v)| (format!("$CONTROL_PAR_{k}"), v.clone()))
                .collect::<BTreeMap<_, _>>(),
            menu: Vec::new(),
        }
    }

    fn at(x: i32, y: i32, w: i32, h: i32) -> Vec<(&'static str, Value)> {
        vec![
            ("POS_X", Value::Int(x)),
            ("POS_Y", Value::Int(y)),
            ("WIDTH", Value::Int(w)),
            ("HEIGHT", Value::Int(h)),
        ]
    }

    fn with(mut p: Vec<(&'static str, Value)>, more: &[(&'static str, Value)]) -> Vec<(&'static str, Value)> {
        p.extend(more.iter().cloned());
        p
    }

    #[test]
    fn a_mic_mixer_becomes_titled_channel_strips() {
        let text = |t: &str| ("TEXT", Value::Text(t.into()));
        let mut controls = vec![control("ui_label", "$a1", &with(at(3, 5, 90, 18), &[text("Mic Mixer")]))];
        for x in [8, 108] {
            controls.push(control("ui_switch", "$b1", &with(at(x, 30, 45, 18), &[text("CL")])));
            controls.push(control("ui_switch", "$c1", &with(at(x + 45, 30, 22, 18), &[text("M")])));
            controls.push(control("ui_knob", "$d1", &with(at(x + 2, 55, 32, 40), &[text("Volume")])));
            controls.push(control(
                "ui_slider",
                "$e1",
                &with(at(x + 2, 100, 85, 18), &[("PICTURE", Value::Text("K4_SLIDER_BIP_1".into()))]),
            ));
        }
        // A numeric readout label and a hidden control drop out.
        controls.push(control("ui_label", "$f1", &with(at(20, 140, 30, 18), &[text("63 %")])));
        controls.push(control("ui_knob", "$g1", &with(at(300, 30, 32, 40), &[text("Hidden"), ("HIDE", Value::Int(16))])));
        // A knob named by the label under it, in a section of its own.
        controls.push(control("ui_slider", "$h1", &at(400, 30, 60, 60)));
        controls.push(control("ui_label", "$i1", &with(at(398, 95, 70, 18), &[text("Dynamics")])));
        let interface = Interface {
            width: 632,
            height: 180,
            controls,
            ..Default::default()
        };
        let s = sections(&interface, &HashMap::new());
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].title.as_deref(), Some("Mic Mixer"));
        assert_eq!(s[0].columns.len(), 2, "one strip per channel");
        let strip = &s[0].columns[1];
        let names: Vec<Vec<&str>> = strip
            .iter()
            .map(|r| r.iter().map(|i| i.name.as_str()).collect())
            .collect();
        assert_eq!(names, [vec!["CL", "M"], vec!["Volume"], vec!["Pan"]]);
        assert!(strip[2][0].bipolar && strip[2][0].face == Face::Fader);
        assert_eq!(s[1].title, None);
        assert_eq!(s[1].columns[0][0][0].name, "Dynamics");
        assert_eq!(s[1].columns[0][0][0].face, Face::Knob);
    }

    #[test]
    fn a_lone_letter_goes_and_a_row_holds_one_kind() {
        let text = |t: &str| ("TEXT", Value::Text(t.into()));
        let controls = vec![
            // Vista's stray "B": one letter, no neighbour to explain it.
            control("ui_switch", "$b", &with(at(300, 10, 20, 18), &[text("B")])),
            // A button row with a slider floating in it (Solo Violin's Vel).
            control("ui_switch", "$legato", &with(at(10, 10, 60, 18), &[text("Legato")])),
            control("ui_switch", "$port", &with(at(72, 10, 60, 18), &[text("Port")])),
            control("ui_slider", "$vel", &with(at(134, 10, 80, 18), &[text("Vel"), ("PICTURE", Value::Text("K4_SLIDER_1".into()))])),
        ];
        let interface = Interface { width: 400, height: 260, controls, ..Default::default() };
        let s = sections(&interface, &HashMap::new());
        let names: Vec<Vec<&str>> = s
            .iter()
            .flat_map(|s| s.columns.iter().flatten())
            .map(|r| r.iter().map(|i| i.name.as_str()).collect())
            .collect();
        assert_eq!(names, [vec!["Legato", "Port"], vec!["Vel"]]);
        // A knob far below the row is a band of its own, not a column under it.
        let mut interface = interface;
        interface.controls.push(control("ui_knob", "$tone", &with(at(20, 200, 32, 40), &[text("Tone")])));
        let s = sections(&interface, &HashMap::new());
        assert_eq!(s.iter().map(|s| s.band).collect::<Vec<_>>(), [0, 1]);
    }

    #[test]
    fn names_come_from_pictures_and_variables() {
        assert_eq!(words("pyramid_but_classic_mix_1_of_2", "pyramid").as_deref(), Some("Classic Mix"));
        assert_eq!(words("$slider_controller_reverb_mix", "").as_deref(), Some("Controller Reverb Mix"));
        assert_eq!(words("K4_SLIDER_BIP_1", ""), None);
        assert_eq!(words("$T1_sliUCVol", "").as_deref(), Some("Uc Vol"));
        assert_eq!(words("gi_uc_knb_127", ""), Some("Uc".into()));
        assert!(!readable("$q5nsy") && !readable("$zptkf") && !readable("$ruqnf"));
        assert!(readable("$vibrato") && readable("$mic_volume"));
        assert_eq!(clean("\t\t\t   Legato Rebowed"), "Legato Rebowed");
    }
}
