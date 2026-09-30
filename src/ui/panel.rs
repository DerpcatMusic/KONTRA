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
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Weak};

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
    /// A vertical list of exclusive choices: an articulation list.
    List,
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
    /// The unit the script gave the knob (`set_knob_unit`).
    unit: Option<Unit>,
    /// A switch that turns on or off whatever sits beside it.
    enable: bool,
    raw: f64,
    min: f64,
    max: f64,
    reset: f64,
    at: Rect,
    /// Hidden by its script: kept only as a scrolled-away row of a list.
    hidden: bool,
    /// A [`Face::List`]'s rows, top down.
    list: Vec<Entry>,
}

/// One row of a list: a choice, its own on/off, the keyswitch that picks it.
#[derive(Clone, Debug)]
struct Entry {
    /// The switch that picks it; none in a list of layers.
    control: Option<usize>,
    name: String,
    on: bool,
    enable: Option<(usize, bool)>,
    key: Option<String>,
    /// Fields and their letters on the row: a layer's velocity range.
    extras: Vec<Item>,
}

/// A titled cluster of controls: columns of rows, left to right, top down.
#[derive(Clone, Debug)]
pub struct Section {
    /// Which band, top down: sections of one band share a line.
    band: usize,
    title: Option<String>,
    /// Lone buttons beside it ("Info"), shown at the end of its title.
    actions: Vec<Item>,
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
    "vertical", "horizontal", "bip", "clear", "empty", "of", "transparent", "select",
    "screen", "item", "byp", "on", "off", "fdr", "knb", "sli", "swi", "mnu", "gi", "gui", "tab",
    "dropdown", "arrow", "bypass", "enable", "ui",
];

/// Words that make a switch an on/off for something beside it.
const ENABLE: &[&str] = &["onoff", "byp", "bypass", "enable", "on", "off"];

/// Words scripts shorten for want of pixels, spelled out. A trailing period
/// marks an abbreviation too ("Rel. Off.").
const ABBREVIATIONS: &[(&str, &str)] = &[
    ("amt", "Amount"),
    ("art", "Articulation"),
    ("artic", "Articulation"),
    ("att", "Attack"),
    ("atk", "Attack"),
    ("ctrl", "Control"),
    ("dec", "Decay"),
    ("def", "Default"),
    ("dyn", "Dynamics"),
    ("env", "Envelope"),
    ("expr", "Expression"),
    ("freq", "Frequency"),
    ("infl", "Influence"),
    ("leg", "Legato"),
    ("lvl", "Level"),
    ("off", "Offset"),
    ("pizz", "Pizzicato"),
    ("rel", "Release"),
    ("res", "Resonance"),
    ("rev", "Reverb"),
    ("rnd", "Random"),
    ("rvrb", "Reverb"),
    ("sens", "Sensitivity"),
    ("spd", "Speed"),
    ("stac", "Staccato"),
    ("sus", "Sustain"),
    ("thresh", "Threshold"),
    ("trem", "Tremolo"),
    ("tun", "Tune"),
    ("vel", "Velocity"),
    ("vol", "Volume"),
];

/// `name` with its abbreviations spelled out: "Rel Vol" is "Release Volume".
/// "Off" only counts with its period; alone it is a state.
fn spell_out(name: &str) -> String {
    name.split(' ')
        .map(|word| {
            let dotted = word.len() > 2 && word.ends_with('.');
            let bare = word.trim_end_matches('.').to_lowercase();
            ABBREVIATIONS
                .iter()
                .find(|(short, _)| *short == bare && (dotted || bare != "off"))
                .map_or(word, |(_, long)| long)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A note name as Kontakt writes it: "C-1", "D#3", "G8".
fn is_note(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| ('A'..='G').contains(&c)) && {
        let rest = chars.as_str().trim_start_matches(['#', 'b']);
        !rest.is_empty() && rest.trim_start_matches('-').chars().all(|c| c.is_ascii_digit())
    }
}

/// A label that shows a value, not a name: "0.0 dB", "20.0 ms", "63 %", "OFF".
fn is_readout(text: &str) -> bool {
    let t = text.trim();
    if matches!(t.to_lowercase().as_str(), "off" | "on" | "-inf" | "-inf db" | "def.") {
        return true;
    }
    match leading_number(t) {
        Some((_, rest)) => rest.is_empty() || unit_token(rest),
        None => false,
    }
}

/// A leading number and what follows it, trimmed: "25000.0" is (25000, "").
fn leading_number(text: &str) -> Option<(f64, &str)> {
    let t = text.trim();
    let end = t
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || (i == 0 && matches!(c, '-' | '+'))))
        .map_or(t.len(), |(i, _)| i);
    let value = t[..end].parse::<f64>().ok()?;
    Some((value, t[end..].trim()))
}

fn unit_token(text: &str) -> bool {
    matches!(
        text.to_lowercase().as_str(),
        "db" | "ms" | "s" | "hz" | "khz" | "%" | "st" | "ct" | "cents" | "x" | "bpm"
    )
}

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

/// The words of an identifier that may say what it does, lowercase: humps
/// and separators split, look-words and numbering dropped, trailing digits
/// cut (`onoff0`, `space2`).
fn raw_words(ident: &str) -> Vec<String> {
    // A letter and digits (`t1`, `k4`) number a page or a skin, not a function.
    let numbering = |w: &str| {
        let mut c = w.chars();
        c.next().is_some_and(|f| f.is_alphabetic()) && c.all(|d| d.is_ascii_digit()) && w.len() > 1
    };
    humps(ident.trim_start_matches(['$', '~', '?', '%', '@', '!']))
        .split(['_', '-', ' ', '.'])
        .map(str::to_lowercase)
        .filter(|w| !numbering(w))
        .map(|w| w.trim_end_matches(|c: char| c.is_ascii_digit()).to_owned())
        .filter(|w| !w.is_empty() && !NOISE.contains(&w.as_str()))
        .collect()
}

/// Readable words from an identifier: `pyramid_but_classic_mix_1_of_2`
/// becomes "Classic Mix" when "pyramid" is a library prefix, and `Mas_sliDyn`
/// "Dynamics" when "mas" is. None when nothing is left.
fn words(ident: &str, prefixes: &[String]) -> Option<String> {
    let mut parts = raw_words(ident);
    let lead = parts.iter().take_while(|w| prefixes.contains(w)).count();
    parts.drain(..lead);
    if parts.is_empty() {
        return None;
    }
    let title = |w: &String| {
        let mut c = w.chars();
        c.next()
            .map(|f| f.to_uppercase().chain(c).collect::<String>())
            .unwrap_or_default()
    };
    Some(spell_out(&parts.iter().map(title).collect::<Vec<_>>().join(" ")))
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

/// The library's own prefixes: the first word most picture names share
/// ("pyramid", "uc"), and the one most readable variable names share
/// ("mas"). They say whose control it is, never what it does.
fn prefixes(interface: &Interface) -> Vec<String> {
    let leading = |names: &mut dyn Iterator<Item = String>| {
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut total = 0;
        for first in names {
            *counts.entry(first).or_default() += 1;
            total += 1;
        }
        counts
            .into_iter()
            .max_by_key(|(_, n)| *n)
            .filter(|(_, n)| *n * 2 > total && *n > 2)
            .map(|(p, _)| p)
    };
    // A picture's first word that says anything: `gi_uc_knb` leads with "uc".
    let pictures = &mut interface.controls.iter().filter_map(|c| match c.properties.get("$CONTROL_PAR_PICTURE") {
        Some(Value::Text(p)) => raw_words(p).into_iter().next(),
        _ => None,
    });
    // A variable's very first word, as written: `$label_art_x` leads with a
    // look-word, so its "art" is never taken for a prefix.
    let variables = &mut interface
        .controls
        .iter()
        .map(|c| c.variable.as_str())
        .filter(|v| readable(v))
        .filter_map(|v| {
            let first = humps(v.trim_start_matches(['$', '~', '?', '%', '@', '!']));
            let first = first.split(['_', '-', ' ', '.']).next()?.to_lowercase();
            raw_words(&first).into_iter().next().filter(|w| *w == first)
        });
    leading(pictures).into_iter().chain(leading(variables)).collect()
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
    let hidden = int("HIDE").is_some_and(|h| h as i32 & 16 != 0);
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
    // A list scrolls its rows off the panel's foot and hides them there.
    if at.x < 0. || at.y < 0. || at.x >= iw || !hidden && at.y >= ih || w <= 0. || h <= 0. {
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
    // A range around zero (Una Corda's Color, -40 to 40) is bipolar too.
    let bipolar = matches!(face, Face::Knob | Face::Fader | Face::VFader)
        && (picture_name.to_lowercase().contains("bip")
            || mentions_pan(&own)
            || mentions_pan(&c.variable)
            || (min < 0. && min == -max));
    let reset = int("DEFAULT_VALUE").unwrap_or(if bipolar { (min + max) / 2. } else { min });
    let enable = face == Face::Toggle
        && own.is_empty()
        && [picture_name, c.variable.as_str()].iter().any(|n| {
            humps(n)
                .to_lowercase()
                .split(['_', '-', ' ', '.', '$'])
                .any(|w| ENABLE.contains(&w.trim_end_matches(|c: char| c.is_ascii_digit())))
        });
    let item = Item {
        control,
        face,
        bipolar,
        name: spell_out(&own),
        label: clean(text("LABEL")),
        unit: Unit::of_knob(text("UNIT")),
        enable,
        raw,
        min,
        max,
        reset,
        at,
        hidden,
        list: Vec::new(),
    };
    Some((item, picture_name.to_owned()))
}

/// Visible controls, named, with the labels that named them taken out.
fn items(interface: &Interface, pictures: &HashMap<String, Arc<Picture>>) -> Vec<Item> {
    let prefixes = prefixes(interface);
    let mut read: Vec<(Item, String)> = interface
        .controls
        .iter()
        .enumerate()
        .filter_map(|(n, c)| read(n, c, interface, pictures))
        // A label that shows a value ("0.0 dB", "63") is a readout; ours
        // show the value, and it names nothing.
        .filter(|(i, _)| {
            i.face != Face::Text || i.name.chars().any(char::is_alphabetic) && !is_readout(&i.name)
        })
        .collect();
    // Covers and masks shade a scrolled list's ends; they do nothing.
    read.retain(|(i, picture)| {
        let variable = interface.controls[i.control].variable.as_str();
        ![picture.as_str(), variable]
            .iter()
            .any(|n| raw_words(n).iter().any(|w| matches!(w.as_str(), "cover" | "mask")))
    });
    let lists = lists(&mut read);
    // A list shows whole, so its scroll bar goes, and a switch behind it
    // is its picture; hidden controls go too.
    read.retain(|(i, _)| {
        !i.hidden
            && !(matches!(i.face, Face::Fader | Face::VFader) && lists.iter().any(|l| scrolls(&l.at, &i.at)))
            && !(i.face == Face::Toggle && lists.iter().any(|l| behind(&l.at, &i.at)))
    });
    // A label spanning two or more controls side by side under it heads
    // them; it names none of them.
    let heads = |l: &Rect| {
        let under: Vec<Rect> = read
            .iter()
            .filter(|(c, _)| c.face != Face::Text)
            .map(|(c, _)| c.at)
            .filter(|c| c.x >= l.x - SPACE && c.right() <= l.right() + SPACE)
            .filter(|c| c.y >= l.bottom() - 2. && c.y <= l.bottom() + LABEL_REACH)
            .collect();
        under.iter().any(|a| under.iter().any(|b| a.right() <= b.x))
    };
    // Where a label may sit to name control `c`: on it, under or over it
    // within reach, or just right of it, level with it.
    let names = |c: &Rect, l: &Rect| {
        let (lx, ly) = l.center();
        let beside = lx >= c.x - SPACE && lx <= c.right() + SPACE;
        let below = l.y >= c.bottom() - c.h / 4. && l.y <= c.bottom() + LABEL_REACH;
        let above = l.bottom() <= c.y + c.h / 4. && l.bottom() >= c.y - LABEL_REACH;
        let right = l.x >= c.right() - c.w / 4. && l.x <= c.right() + SPACE && ly >= c.y && ly <= c.bottom();
        l.overlaps(c) || beside && (below || above) || right
    };
    // A note name beside an articulation is its keyswitch, not a name.
    let labels: Vec<usize> = (0..read.len())
        .filter(|&m| read[m].0.face == Face::Text && !heads(&read[m].0.at))
        .filter(|&m| !is_note(&read[m].0.name))
        .collect();
    // Continuous controls take labels first, then switches. Within each,
    // the closest pairs go first, so a label under one knob and over the
    // next goes to the one it is nearer, not to whichever came first.
    let mut taken = vec![false; read.len()];
    for switches in [false, true] {
        let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
        for (n, (c, _)) in read.iter().enumerate() {
            if c.face == Face::Text || !c.name.is_empty() || (c.face == Face::Toggle) != switches {
                continue;
            }
            for &m in labels.iter().filter(|&&m| names(&c.at, &read[m].0.at)) {
                pairs.push((apart(&c.at, &read[m].0.at), n, m));
            }
        }
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, n, m) in pairs {
            if taken[m] || !read[n].0.name.is_empty() {
                continue;
            }
            taken[m] = true;
            // Text drawn along a slider is its value ("Default" on an output
            // selector): it goes, and names nothing.
            let (c, l) = (read[n].0.at, read[m].0.at);
            let along = (l.right().min(c.right()) - l.x.max(c.x)).max(0.);
            let on = read[n].0.face == Face::Fader
                && along >= l.w * 0.75
                && l.y >= c.y - 2.
                && l.bottom() <= c.bottom() + 2.;
            if !on {
                read[n].0.name = read[m].0.name.clone();
            }
        }
    }
    for (n, (item, picture)) in read.iter_mut().enumerate() {
        if taken[n] || !item.name.is_empty() {
            continue;
        }
        let variable = &interface.controls[item.control].variable;
        // A picture that says no more than a couple of letters ("Uc") yields
        // to a readable variable name.
        let from_picture = words(picture, &prefixes);
        let from_variable = readable(variable).then(|| words(variable, &prefixes)).flatten();
        item.name = match from_picture {
            Some(p) if p.len() <= 3 && from_variable.is_some() => from_variable,
            Some(p) => Some(p),
            None => from_variable,
        }
            .or_else(|| item.bipolar.then(|| "Pan".to_owned()))
            .unwrap_or_default();
    }
    let mut kept: Vec<Item> = read
        .into_iter()
        .enumerate()
        .filter(|(n, (i, _))| !taken[*n] && (!i.name.is_empty() || i.face == Face::Menu))
        .map(|(_, (i, _))| i)
        .collect();
    // An on/off switch against another reads "On" beside it: Solo's
    // articulation rows are "Sustained · On", not "Sustained · Articulation".
    let beside: Vec<bool> = kept
        .iter()
        .map(|i| {
            i.enable
                && kept.iter().any(|o| {
                    !std::ptr::eq(i, o) && o.face == Face::Toggle && !o.enable && touching(&i.at, &o.at)
                })
        })
        .collect();
    for (item, beside) in kept.iter_mut().zip(beside) {
        if beside {
            item.name = "On".into();
        }
    }
    // A vertical fader stands among its kind, a mixer's strips; alone it
    // lies down, a compact fader and not a column of its own.
    let tall: Vec<Rect> = kept.iter().filter(|i| i.face == Face::VFader).map(|i| i.at).collect();
    for item in &mut kept {
        if item.face == Face::VFader
            && !tall.iter().any(|o| *o != item.at && (o.y - item.at.y).abs() <= 2. && o.h == item.at.h)
        {
            item.face = Face::Fader;
        }
    }
    // A one-letter switch reads only beside its siblings (a strip's M and S);
    // alone, like Vista's corner "B", it is a mark, not a control.
    let mut out: Vec<Item> = kept
        .iter()
        .filter(|i| {
            i.face != Face::Toggle
                || i.name.chars().filter(|c| c.is_alphanumeric()).count() > 1
                || kept.iter().any(|o| {
                    !std::ptr::eq(*i, o) && o.face == Face::Toggle && touching(&i.at, &o.at)
                })
        })
        .cloned()
        .collect();
    out.extend(lists);
    out
}

/// Stacks of `key` switches of one width down one edge, one gap apart, top
/// down, as indices into `read`; rows already `taken` left out. A row may
/// be taller than the rest: Solo's legato spans two.
fn stacks(read: &[(Item, String)], taken: &[bool], key: &dyn Fn(&Item) -> bool) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..read.len()).filter(|&n| !taken[n] && key(&read[n].0)).collect();
    // A hidden row under a shown one is folded into it, not scrolled away.
    let shown: Vec<Rect> = order.iter().map(|&n| &read[n].0).filter(|i| !i.hidden).map(|i| i.at).collect();
    order.retain(|&n| !read[n].0.hidden || !shown.iter().any(|s| s.overlaps(&read[n].0.at)));
    order.sort_by(|&a, &b| {
        let (a, b) = (&read[a].0.at, &read[b].0.at);
        (a.x.total_cmp(&b.x))
            .then(a.w.total_cmp(&b.w))
            .then(a.y.total_cmp(&b.y))
    });
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for n in order {
        let at = read[n].0.at;
        let joins = runs.last().is_some_and(|run| {
            let last = read[run[run.len() - 1]].0.at;
            let gap = |above: &Rect, below: &Rect| below.y - above.bottom();
            let first = if run.len() > 1 { gap(&read[run[run.len() - 2]].0.at, &last) } else { gap(&last, &at) };
            (last.x, last.w) == (at.x, at.w) && gap(&last, &at) == first && (0. ..=SPACE).contains(&first)
        });
        match runs.last_mut() {
            Some(run) if joins => run.push(n),
            _ => runs.push(vec![n]),
        }
    }
    runs.retain(|run| run.len() >= 3 && run.iter().filter(|&&n| !read[n].0.hidden).count() >= 2);
    runs
}

/// Vertical lists, taken out of `read`. First, choices: wide switches
/// stacked one gap apart, an articulation list, one set or several. Then rows
/// of on/off switches left over, each beside a name: a legato's layers.
/// Each row takes the label on it for its name, the on/off switch on it,
/// the note name beside it for its keyswitch, and the fields and letters on
/// it ("L 1 H 64"); the rows' pictures and backgrounds go. Rows the script
/// scrolled away and hid come back, so the list shows whole.
fn lists(read: &mut Vec<(Item, String)>) -> Vec<Item> {
    let mut taken = vec![false; read.len()];
    let mut out = Vec::new();
    let choice = |i: &Item| i.face == Face::Toggle && !i.enable && i.at.w >= i.at.h * 3.;
    let enable = |i: &Item| i.face == Face::Toggle && i.enable;
    for choices in [true, false] {
        let key: &dyn Fn(&Item) -> bool = if choices { &choice } else { &enable };
        for run in stacks(read, &taken, key) {
            let mut t = taken.clone();
            let (mut entries, mut at) = (Vec::new(), None::<Rect>);
            for &n in &run {
                let r = read[n].0.at;
                let hidden = read[n].0.hidden;
                let level = |o: &Rect| {
                    let (_, y) = o.center();
                    y >= r.y && y <= r.bottom()
                };
                // A hidden row's parts are hidden with it.
                let find = |t: &[bool], want: &dyn Fn(&Item) -> bool| {
                    (0..read.len()).find(|&m| m != n && !t[m] && read[m].0.hidden == hidden && want(&read[m].0))
                };
                let near = |o: &Rect, right: f64| o.x < right + LABEL_REACH && o.right() > r.x;
                // Its name: a label on the row, level with it or starting with it.
                let label = find(&t, &|i| {
                    i.face == Face::Text
                        && !is_note(&i.name)
                        && i.name.chars().filter(|c| c.is_alphanumeric()).count() > 1
                        && (level(&i.at) || (i.at.y - r.y).abs() <= TIGHT)
                        && near(&i.at, r.right())
                });
                let name = label.map_or_else(|| read[n].0.name.clone(), |m| read[m].0.name.clone());
                if name.is_empty() || !choices && label.is_none() {
                    continue;
                }
                t[n] = true;
                if let Some(m) = label {
                    t[m] = true;
                }
                let right = label.map_or(r.right(), |m| read[m].0.at.right().max(r.right()));
                // Rows of on/off switches may pick too, more than one at a
                // time: Solo's layered articulations.
                let (pick, switch) = if choices {
                    (Some(n), find(&t, &|i| enable(i) && level(&i.at) && near(&i.at, right)))
                } else {
                    (find(&t, &|i| choice(i) && level(&i.at) && near(&i.at, right)), Some(n))
                };
                for m in [pick, switch].into_iter().flatten() {
                    t[m] = true;
                }
                let key = find(&t, &|i| i.face == Face::Text && is_note(&i.name) && level(&i.at) && near(&i.at, right));
                if let Some(m) = key {
                    t[m] = true;
                }
                let mut found = Vec::new();
                while let Some(m) = find(&t, &|i| {
                    let letters = i.face == Face::Text && i.name.chars().filter(|c| c.is_alphanumeric()).count() <= 2;
                    (letters || matches!(i.face, Face::Value | Face::Menu)) && level(&i.at) && near(&i.at, right)
                }) {
                    t[m] = true;
                    found.push(m);
                }
                // Letters only name fields; alone they belong elsewhere.
                if found.iter().all(|&m| read[m].0.face == Face::Text) {
                    found.drain(..).for_each(|m| t[m] = false);
                }
                let mut extras: Vec<Item> = found.iter().map(|&m| read[m].0.clone()).collect();
                extras.sort_by(|a, b| a.at.x.total_cmp(&b.at.x));
                if !hidden {
                    let parts = [label, pick, switch, key].into_iter().flatten().map(|m| read[m].0.at);
                    for o in parts.chain(extras.iter().map(|e| e.at)).chain([r]) {
                        at = Some(match at {
                            Some(a) => {
                                let (x, y) = (a.x.min(o.x), a.y.min(o.y));
                                Rect { x, y, w: a.right().max(o.right()) - x, h: a.bottom().max(o.bottom()) - y }
                            }
                            None => o,
                        });
                    }
                }
                entries.push(Entry {
                    control: pick.map(|m| read[m].0.control),
                    name,
                    on: pick.is_some_and(|m| read[m].0.raw >= 1.),
                    enable: switch.map(|m| (read[m].0.control, read[m].0.raw >= 1.)),
                    key: key.map(|m| read[m].0.name.clone()),
                    extras,
                });
            }
            if entries.len() < 2 {
                continue;
            }
            taken = t;
            let first = read[run[0]].0.clone();
            out.push(Item {
                face: Face::List,
                name: if choices { "Choices" } else { "Layers" }.into(),
                at: at.unwrap_or(first.at),
                list: entries,
                ..first
            });
        }
    }
    let mut n = 0;
    read.retain(|_| {
        n += 1;
        !taken[n - 1]
    });
    out
}

pub use crate::articulate::Found;

/// The articulations in `sections`: the rows of the first list of choices
/// with keyswitches (else the longest list of choices), each keyswitch read
/// from the note beside it or else from the key the script named after it.
pub fn articulations(sections: &[Section], slot: usize, keys: &std::collections::BTreeMap<u8, crate::ksp::KeyState>) -> Vec<Found> {
    let lists: Vec<&Item> = sections
        .iter()
        .flat_map(|s| s.columns.iter().flatten().flatten())
        .filter(|i| i.face == Face::List && i.list.iter().all(|e| e.control.is_some()))
        .collect();
    let keyed = |i: &&&Item| i.list.iter().any(|e| e.key.is_some());
    let Some(list) = lists.iter().find(keyed).or_else(|| lists.iter().max_by_key(|i| i.list.len())) else {
        return keyswitches(keys)
            .into_iter()
            .map(|(key, name)| (name, Some(key), None))
            .collect();
    };
    list.list
        .iter()
        .map(|e| {
            let named = || keys.iter().find(|(_, k)| clean(&k.name) == e.name).map(|(&n, _)| n);
            let key = e.key.as_deref().and_then(crate::articulate::parse_note).or_else(named);
            (e.name.clone(), key, e.control.map(|c| (slot as u16, c as u16)))
        })
        .collect()
}

/// Keyswitches a script names on the keyboard, for a panel without a list
/// (Afflatus): the longest run of named, colored keys a note or two apart,
/// at least two long.
pub fn keyswitches(keys: &std::collections::BTreeMap<u8, crate::ksp::KeyState>) -> Vec<(u8, String)> {
    let colored = |k: &crate::ksp::KeyState| match &k.color {
        Some(Value::Text(c)) => !matches!(
            c.trim_start_matches('$').trim_start_matches("KEY_COLOR_"),
            "" | "NONE" | "DEFAULT" | "INACTIVE" | "WHITE" | "BLACK"
        ),
        _ => false,
    };
    let mut runs: Vec<Vec<(u8, String)>> = Vec::new();
    for (&note, k) in keys.iter().filter(|(_, k)| colored(k) && !clean(&k.name).is_empty()) {
        match runs.last_mut() {
            Some(run) if note - run[run.len() - 1].0 <= 2 => run.push((note, clean(&k.name))),
            _ => runs.push(vec![(note, clean(&k.name))]),
        }
    }
    runs.into_iter().filter(|r| r.len() >= 2).max_by_key(Vec::len).unwrap_or_default()
}

/// Whether switch `s` lies behind list `list`: a picture over half its rows.
fn behind(list: &Rect, s: &Rect) -> bool {
    let along = s.bottom().min(list.bottom()) - s.y.max(list.y);
    along >= list.h / 2. && s.x < list.right() && s.right() > list.x
}

/// Whether slider `bar` scrolls list `list`: upright along its right edge.
fn scrolls(list: &Rect, bar: &Rect) -> bool {
    let along = bar.bottom().min(list.bottom()) - bar.y.max(list.y);
    bar.h > bar.w && bar.x >= list.x && bar.x <= list.right() + SPACE && along >= bar.h / 2.
}

/// How far label `l` is from control `c`. Text centred on the control is
/// nearest, the more of it the control covers; otherwise the distance
/// between centres, a little further for a label over the control than
/// under it, where names usually sit.
fn apart(c: &Rect, l: &Rect) -> f64 {
    let (lx, ly) = l.center();
    if lx >= c.x && lx <= c.right() && ly >= c.y && ly <= c.bottom() {
        let w = l.right().min(c.right()) - l.x.max(c.x);
        let h = l.bottom().min(c.bottom()) - l.y.max(c.y);
        return -(w * h) / (l.w * l.h);
    }
    let (cx, cy) = c.center();
    let d = (cx - lx).hypot(cy - ly);
    if ly < c.y { d * 1.25 } else { d }
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
        Face::List => 4,
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
        for k in 0..5 {
            let part: Vec<Item> = row.iter().filter(|i| kind(i.face) == k).cloned().collect();
            if !part.is_empty() {
                split.push(part);
            }
        }
    }
    split
}

/// A part's sections as last read, and what they were read from: the
/// interface, and its shape (everything but values and readouts), so a new
/// interface a script publishes with only values changed reads nothing again.
#[derive(Default)]
pub struct Cache {
    interface: Weak<Interface>,
    pictures: usize,
    shape: u64,
    sections: Arc<Vec<Section>>,
    /// Every control the sections show, whose values to watch.
    used: Vec<usize>,
}

/// `interface`'s sections through `cache`: read again only when its shape
/// or pictures change. Values are read live as the controls are drawn.
pub fn cached(
    cache: &mut Cache,
    interface: &Arc<Interface>,
    pictures: &Arc<HashMap<String, Arc<Picture>>>,
) -> Arc<Vec<Section>> {
    let pics = Arc::as_ptr(pictures) as usize;
    let same = cache.interface.upgrade().is_some_and(|i| Arc::ptr_eq(&i, interface));
    if !same || pics != cache.pictures {
        let shape = shape(interface);
        if shape != cache.shape || pics != cache.pictures {
            let read = sections(interface, pictures);
            cache.used = read
                .iter()
                .flat_map(|s| s.columns.iter().flatten().flatten().chain(&s.actions))
                .flat_map(|i| {
                    let rows = i.list.iter().flat_map(|e| {
                        let extras = e.extras.iter().map(|x| x.control);
                        e.control.into_iter().chain(e.enable.map(|(c, _)| c)).chain(extras)
                    });
                    std::iter::once(i.control).chain(rows).collect::<Vec<_>>()
                })
                .collect();
            cache.sections = Arc::new(read);
            cache.shape = shape;
        }
        cache.interface = Arc::downgrade(interface);
        cache.pictures = pics;
    }
    cache.sections.clone()
}

/// The values and readouts of what `cache` shows, hashed: a performance
/// view is drawn again when they move.
pub fn values(cache: &Cache, interface: &Interface) -> u64 {
    let mut h = DefaultHasher::new();
    for c in cache.used.iter().filter_map(|&n| interface.controls.get(n)) {
        for key in ["$CONTROL_PAR_VALUE", "$CONTROL_PAR_LABEL"] {
            if let Some(v) = c.properties.get(key) {
                hash_value(v, &mut h);
            }
        }
    }
    h.finish()
}

/// What decides an interface's sections: every control's kind, name, menu
/// and properties but its value and readout.
fn shape(interface: &Interface) -> u64 {
    let mut h = DefaultHasher::new();
    (interface.width, interface.height, interface.controls.len()).hash(&mut h);
    for c in &interface.controls {
        (&c.variable, &c.kind, &c.menu).hash(&mut h);
        for (key, v) in &c.properties {
            if key != "$CONTROL_PAR_VALUE" && key != "$CONTROL_PAR_LABEL" {
                key.hash(&mut h);
                hash_value(v, &mut h);
            }
        }
    }
    h.finish()
}

fn hash_value(v: &Value, h: &mut DefaultHasher) {
    match v {
        Value::Int(n) => n.hash(h),
        Value::Real(r) => r.to_bits().hash(h),
        Value::Text(t) => t.hash(h),
        Value::Array(a) => a.iter().for_each(|v| hash_value(v, h)),
    }
}

/// Control `control`'s value in `part`'s interface as it is now.
fn live(cx: &Cx, part: usize, control: usize) -> Option<f64> {
    let c = cx.view.parts.get(part)?.interface.as_ref()?.controls.get(control)?;
    match c.properties.get("$CONTROL_PAR_VALUE")? {
        Value::Int(n) => Some(f64::from(*n)),
        Value::Real(r) => Some(*r),
        _ => None,
    }
}

/// `item` with its value and readout as they are now.
fn fresh(cx: &Cx, part: usize, item: &Item) -> Item {
    let mut item = item.clone();
    item.raw = live(cx, part, item.control).unwrap_or(item.raw);
    let label = cx.view.parts.get(part).and_then(|v| v.interface.as_ref()).and_then(|i| i.controls.get(item.control));
    if let Some(Value::Text(t)) = label.and_then(|c| c.properties.get("$CONTROL_PAR_LABEL")) {
        item.label = clean(t);
    }
    item
}

/// The interface's controls, grouped as they were drawn.
pub fn sections(interface: &Interface, pictures: &HashMap<String, Arc<Picture>>) -> Vec<Section> {
    let mut out = bands(items(interface, pictures), SECTION_GAP)
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
                actions: Vec::new(),
                columns: columns(group, COLUMN_GAP).into_iter().map(rows).collect(),
            }
        })
        .filter(|s| !s.columns.is_empty())
        .collect::<Vec<_>>();
    // A lone untitled button ("Info") leaves the layout for the far end of
    // its band, kept with the section before it, or else after it.
    let lone = |s: &Section| {
        s.title.is_none()
            && matches!(s.columns.as_slice(), [c] if matches!(c.as_slice(), [r] if matches!(r.as_slice(), [i] if i.face == Face::Toggle)))
    };
    for n in (0..out.len()).rev() {
        if !lone(&out[n]) {
            continue;
        }
        let band = out[n].band;
        if let Some(m) = (0..n).rev().chain(n + 1..out.len()).find(|&m| out[m].band == band && !lone(&out[m])) {
            let s = out.remove(n);
            out[if m > n { m - 1 } else { m }].actions.extend(s.columns.into_iter().flatten().flatten());
        }
    }
    out
}

/// `part`'s script controls, rebuilt: bands top down, each a wrapping line
/// of sections side by side.
pub fn view(ui: &mut Ui, cx: &mut Cx, part: usize, sections: &[Section]) -> El {
    sync(cx, part, sections);
    let mut bands: Vec<Vec<El>> = Vec::new();
    // Every section opens with a title rule when any has a title, so their
    // first rows share a line.
    let titled = sections.iter().any(|s| s.title.is_some());
    let mut actions: Vec<Vec<El>> = Vec::new();
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
        actions.resize_with(bands.len(), Vec::new);
        for item in &section.actions {
            let el = control(ui, cx, part, item).h(STRIP);
            actions[section.band].push(el);
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
    if let Some(el) = keyswitch_list(ui, cx, part) {
        bands.resize_with(bands.len().max(1), Vec::new);
        bands[0].push(el);
    }
    actions.resize_with(bands.len(), Vec::new);
    // A band's lone buttons sit at its far end.
    col(bands
        .into_iter()
        .zip(actions)
        .filter(|(b, _)| !b.is_empty())
        .map(|(b, a)| {
            let line = row(b).wrap().gap(INSET * 2.).line_gap(INSET).align(Align::Start).flex(1).min_w(0);
            let mut all = vec![line];
            all.extend(a);
            row(all).gap(TIGHT).align(Align::Start).w(Len::Pct(100.))
        })
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

/// What a continuous control's value is measured in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Unit {
    Decibels,
    Millis,
    Hertz,
    Percent,
    Semitones,
    Pan,
}

impl Unit {
    /// The unit a script gave its knob: `$KNOB_UNIT_DB` and so on.
    fn of_knob(unit: &str) -> Option<Self> {
        Some(match unit.trim_start_matches('$').strip_prefix("KNOB_UNIT_")? {
            "DB" => Self::Decibels,
            "MS" => Self::Millis,
            "HZ" => Self::Hertz,
            "PERCENT" => Self::Percent,
            "ST" => Self::Semitones,
            _ => return None,
        })
    }

    /// The unit a name implies, by its last word that implies one: a
    /// "Release Volume" is a level, not a time.
    fn of_name(name: &str) -> Option<Self> {
        name.split_whitespace().rev().find_map(|w| {
            Some(match w.trim_end_matches('.').to_lowercase().as_str() {
                "volume" | "gain" | "level" | "send" => Self::Decibels,
                "attack" | "hold" | "decay" | "release" | "time" | "delay" | "predelay" => Self::Millis,
                "cutoff" | "frequency" => Self::Hertz,
                "tune" | "transpose" | "pitch" | "detune" => Self::Semitones,
                "pan" | "balance" => Self::Pan,
                "width" | "mix" | "amount" | "dynamics" | "expression" | "depth" | "speed" => Self::Percent,
                _ => return None,
            })
        })
    }

    /// `value` in this unit: "0.0 dB", "120 ms", "1.2 kHz", "+3 st", "L 20".
    fn format(self, value: f64) -> String {
        match self {
            Self::Decibels if !value.is_finite() || value <= -120. => "-inf dB".into(),
            // Kontakt shows 0.0, never -0.0.
            Self::Decibels => format!("{:.1} dB", (value * 10.).round() / 10. + 0.),
            Self::Millis if value >= 1000. => format!("{:.2} s", value / 1000.),
            Self::Millis if value >= 100. => format!("{value:.0} ms"),
            Self::Millis => format!("{value:.1} ms"),
            Self::Hertz if value >= 1000. => format!("{:.1} kHz", value / 1000.),
            Self::Hertz => format!("{value:.0} Hz"),
            Self::Percent => format!("{value:.0} %"),
            Self::Semitones => tune_text(value),
            Self::Pan => pan_text(value),
        }
    }

    /// The widest readout, so a fader keeps its length as values change.
    fn widest(self) -> &'static str {
        match self {
            Self::Decibels => "-60.0 dB",
            Self::Millis => "000.0 ms",
            Self::Hertz => "00.0 kHz",
            Self::Percent => "100 %",
            Self::Semitones => "+00.00 st",
            Self::Pan => "R 100",
        }
    }

    /// The engine parameter a control in this unit drives when it spans
    /// the engine's own 0 to 1,000,000: Vista's mic Volume is the group
    /// volume, read by Kontakt's volume law.
    fn engine_par(self, name: &str) -> Option<i32> {
        use crate::engine::engine_par as id;
        let says = |w: &str| name.to_lowercase().split(' ').any(|n| n == w);
        Some(match self {
            Self::Decibels => id::VOLUME,
            Self::Hertz => id::CUTOFF,
            Self::Semitones => id::TUNE,
            Self::Pan => id::PAN,
            Self::Millis if says("attack") => id::ATTACK,
            Self::Millis if says("hold") => id::HOLD,
            Self::Millis if says("decay") => id::DECAY,
            Self::Millis if says("release") => id::RELEASE,
            Self::Percent if says("width") => id::STEREO,
            _ => return None,
        })
    }
}

/// The unit `item` reads in: the script's, else what its name says.
fn unit_of(item: &Item) -> Option<Unit> {
    item.unit.or_else(|| Unit::of_name(&item.name))
}

/// What a knob or fader reads, always with its unit. The script's own text
/// when it says something ("-inf dB", "OFF", "63 %"); a bare number from it
/// (Vista's "0.0") gets the unit its name or knob gives; failing that, a
/// control over the engine's range reads by the engine's law; else pan, a
/// signed percentage around a bipolar middle, or a percentage.
fn readout(item: &Item, value: f64) -> String {
    let unit = unit_of(item);
    let label = item.label.trim();
    match leading_number(label) {
        _ if label.is_empty() => {}
        Some((n, "")) => {
            if let Some(u) = unit.filter(|u| *u != Unit::Pan) {
                return u.format(n);
            }
        }
        // "0%" reads "0 %".
        Some((_, rest)) if unit_token(rest) => {
            let digits = &label[..label.len() - rest.len()];
            return format!("{} {rest}", digits.trim_end());
        }
        _ => return label.to_owned(),
    }
    if item.min == 0. && item.max == 1_000_000. {
        let law = unit.and_then(|u| Some((u, u.engine_par(&item.name)?)));
        if let Some((u, shown)) =
            law.and_then(|(u, id)| Some((u, crate::engine::engine_par_display(id, value.round() as i32)?)))
        {
            return match shown {
                crate::engine::Disp::Gain(_) => format!("{shown} dB"),
                crate::engine::Disp::Pan(_) => shown.to_string(),
                crate::engine::Disp::Num(v, _) => u.format(f64::from(v)),
            };
        }
    }
    let span = item.max - item.min;
    let at = if span > 0. { ((value - item.min) / span).clamp(0., 1.) } else { 0. };
    match unit {
        Some(Unit::Pan) => pan_text(at * 2. - 1.),
        _ if item.bipolar => match ((at * 2. - 1.) * 100.).round() {
            0. => "0 %".into(),
            n => format!("{n:+.0} %"),
        },
        _ => format!("{:.0} %", at * 100.),
    }
}

/// The widest `item` reads, for its fader's length.
fn widest(item: &Item) -> &'static str {
    match unit_of(item) {
        Some(u) => u.widest(),
        None if item.bipolar => "+100 %",
        None => "100 %",
    }
}

/// A value field's text: whole numbers, signed semitones for a transpose.
fn value_text(item: &Item) -> String {
    match unit_of(item) {
        Some(Unit::Semitones) => Unit::Semitones.format(item.raw),
        _ => format!("{}", item.raw),
    }
}

/// One control, wired to the script: every edit runs its `on ui_control`.
fn control(ui: &mut Ui, cx: &mut Cx, part: usize, item: &Item) -> El {
    let now;
    let item = if item.face == Face::List {
        item
    } else {
        now = fresh(cx, part, item);
        &now
    };
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
                Fader::bipolar(&range, item.reset, widest(item))
            } else {
                Fader::over(&range, item.reset, widest(item))
            };
            let pans = item.face != Face::Knob && unit_of(item) == Some(Unit::Pan);
            let (_, el) = match item.face {
                // Every pan slider is the header's and the mixer's wedge.
                _ if pans => {
                    let (lo, span) = (*range.start(), range.end() - range.start());
                    let mut pan = (value - lo) / span * 2. - 1.;
                    let el = pan_wedge(ui, &id, &mut pan);
                    value = lo + (pan + 1.) / 2. * span;
                    (false, el)
                }
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
                _ if pans => col![title, el].gap(TIGHT).align(Align::Center).shrink(0),
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
            let el = number(ui, id.as_str(), name, &mut value, range, value_text(item));
            if value.round() != item.raw {
                cx.p.shared.edit_control(part, item.control, value.round() as i32);
            }
            el
        }
        Face::Text => caption(name.to_owned()).fill(Role::Dim).lines(1).min_w(0),
        Face::List => {
            let routed = routed(cx, part, &item.list);
            let mut rows = Vec::new();
            if routed {
                rows.push(routing_bar(ui, cx, part));
            }
            rows.extend((item.list.iter().enumerate()).map(|(n, e)| entry(ui, cx, part, e, routed.then_some(n))));
            list(rows, name)
        }
    }
}

/// A list's rows in a column of hairline seams.
fn list(rows: Vec<El>, name: &str) -> El {
    col(rows)
            .gap(1)
            .align(Align::Stretch)
            .fill(hairline())
            .min_w(CONTROL * 10.)
            .max_size(Size::new(CONTROL * 16., CONTROL * 100.))
            .named(name.to_owned())
            .shrink(0)
}

/// Whether `list` is the articulation list `part` routes by.
fn routed(cx: &Cx, part: usize, list: &[Entry]) -> bool {
    cx.selection.parts.get(part).is_some_and(|p| {
        let a = &p.articulate;
        a.source == p.path
            && a.articulations.len() == list.len()
            && a.articulations.iter().zip(list).all(|(a, e)| a.name == e.name)
    })
}

/// Keep `part`'s articulation setup in step with what its panel shows.
fn sync(cx: &mut Cx, part: usize, sections: &[Section]) {
    let v = &cx.view.parts[part];
    let found = articulations(sections, v.script_slot, &v.keys);
    if let Some(p) = cx.selection.parts.get_mut(part).filter(|p| !p.path.is_empty()) {
        p.articulate.sync(&p.path.clone(), &found);
    }
}

/// What a performance view shows beyond its script's values: the part's
/// articulation setup and the keys its scripts name.
pub fn deps(cx: &Cx, slot: usize) -> u64 {
    let mut h = DefaultHasher::new();
    (Arc::as_ptr(&cx.view.parts[slot].keys) as usize).hash(&mut h);
    if let Some(p) = cx.selection.parts.get(slot) {
        format!("{:?}{:?}", p.articulate, p.mpe).hash(&mut h);
    }
    h.finish()
}

/// Above the articulation list: how its notes pick one.
fn routing_bar(ui: &mut Ui, cx: &mut Cx, part: usize) -> El {
    use crate::articulate::Mode;
    let a = &cx.selection.parts[part].articulate;
    let (now, fixed) = (a.mode, a.fixed_velocity);
    let mut modes = Vec::new();
    for (mode, label) in [(Mode::Keyswitch, "Keys"), (Mode::Channel, "Channel"), (Mode::Velocity, "Velocity")] {
        let (hit, el) = action(ui, format!("art-mode-{part}-{label}"), label, mode == now);
        if hit {
            cx.selection.parts[part].articulate.mode = mode;
        }
        // Text grounds on its own fill, not the seams' hairline.
        let el = if mode == now { el } else { el.fill(Role::Field) };
        modes.push(el.tip(match mode {
            Mode::Keyswitch => "Keyswitches pick the articulation",
            Mode::Channel => "Each articulation plays on its own MIDI channel",
            Mode::Velocity => "Each articulation plays in its own velocity range",
        }));
    }
    let mut cells = vec![segmented(modes), spacer()];
    if now == Mode::Velocity {
        let mut v = f64::from(fixed);
        let text = if fixed == 0 { "Scaled".to_owned() } else { format!("Vel {fixed}") };
        let name = "Velocity played: scaled from the note's place in its range, or fixed";
        cells.push(field(ui, format!("art-fixed-{part}"), name, &mut v, 0.0..=127.0, text, "Scaled"));
        cx.selection.parts[part].articulate.fixed_velocity = v.round() as u8;
    }
    row(cells).gap(SPACE).align(Align::Center).pad(edges(0., TIGHT, 0., 0.)).h(CONTROL).fill(Role::Field).named("Articulation routing")
}

/// A small number to drag or type, inline in a list row.
fn field(ui: &mut Ui, id: String, name: &str, value: &mut f64, range: std::ops::RangeInclusive<f64>, text: String, widest: &str) -> El {
    drag_value(ui, id, name, value, range)
        .size(Xs)
        .el
        .value_text(text)
        .el()
        .h(STRIP - 4.)
        .reserve(widest.to_owned())
        .shrink(0)
        .tip(name.to_owned())
}

/// A routed row's own settings: whether it takes part, and its channel or
/// velocity range.
fn routing_cells(ui: &mut Ui, cx: &mut Cx, part: usize, n: usize) -> Vec<El> {
    use crate::articulate::Mode;
    let a = &cx.selection.parts[part].articulate;
    let (mode, art) = (a.mode, a.articulations[n].clone());
    if mode == Mode::Keyswitch {
        return Vec::new();
    }
    let mut cells = Vec::new();
    let mut edited = art.clone();
    // Channel or lowest velocity 0 leaves the articulation out.
    if mode == Mode::Channel {
        let mut ch = if art.enabled { f64::from(art.channel) + 1. } else { 0. };
        let text = if art.enabled { format!("Ch {}", art.channel + 1) } else { "Off".into() };
        let name = format!("{}: MIDI channel", art.name);
        cells.push(field(ui, format!("art-ch-{part}-{n}"), &name, &mut ch, 0.0..=16.0, text, "Ch 16"));
        edited.enabled = ch >= 1.;
        edited.channel = (ch.round() as u8).clamp(1, 16) - 1;
    } else {
        // Where the range sits in 1..=127, and its ends.
        let width = CONTROL * 2.;
        let at = |v: u8| width * f64::from(v.saturating_sub(1)) / 126.;
        let span = (at(art.high) - at(art.low)).max(2.);
        cells.push(
            row![block(at(art.low), 3), block(span, 3).fill(Fill::from(accent()))]
                .w(width)
                .h(3)
                .fill(hairline())
                .opacity(if art.enabled { 1. } else { 0.3 })
                .shrink(0),
        );
        let (mut low, mut high) = (if art.enabled { f64::from(art.low) } else { 0. }, f64::from(art.high));
        let text = if art.enabled { art.low.to_string() } else { "Off".into() };
        let name = format!("{}: lowest velocity", art.name);
        cells.push(field(ui, format!("art-low-{part}-{n}"), &name, &mut low, 0.0..=127.0, text, "Off"));
        let name = format!("{}: highest velocity", art.name);
        cells.push(field(ui, format!("art-high-{part}-{n}"), &name, &mut high, 1.0..=127.0, art.high.to_string(), "127"));
        edited.enabled = low >= 1.;
        edited.low = (low.round() as u8).max(1);
        edited.high = (high.round() as u8).max(edited.low);
    }
    if edited != art {
        cx.selection.parts[part].articulate.articulations[n] = edited;
    }
    cells
}

/// Articulations a panel without a list names on the keyboard, as a list.
fn keyswitch_list(ui: &mut Ui, cx: &mut Cx, part: usize) -> Option<El> {
    let a = &cx.selection.parts.get(part)?.articulate;
    if a.articulations.is_empty() || a.articulations.iter().any(|a| a.control.is_some()) {
        return None;
    }
    let entries: Vec<Entry> = (a.articulations.iter())
        .map(|a| Entry {
            control: None,
            name: a.name.clone(),
            on: false,
            enable: None,
            key: a.key.map(note_name),
            extras: Vec::new(),
        })
        .collect();
    let mut rows = vec![routing_bar(ui, cx, part)];
    rows.extend(entries.iter().enumerate().map(|(n, e)| entry(ui, cx, part, e, Some(n))));
    Some(col![section_title("Articulations"), list(rows, "Articulations")].gap(SPACE).align(Align::Stretch).shrink(0))
}

/// One row of a list: set, it is raised with an accent edge. Its check box
/// turns the choice on or off; the keyswitch that picks it sits at the right.
fn entry(ui: &mut Ui, cx: &mut Cx, part: usize, e: &Entry, routed: Option<usize>) -> El {
    let set = |c: usize, was: bool| live(cx, part, c).map_or(was, |v| v >= 1.);
    let e = &Entry {
        on: e.control.map_or(e.on, |c| set(c, e.on)),
        enable: e.enable.map(|(c, on)| (c, set(c, on))),
        ..e.clone()
    };
    // As in Kontakt, a click flips the switch; the script keeps one set.
    let id = e.control.map(|c| format!("ksp-{part}-{c}"));
    if let (Some(c), Some(id)) = (e.control, &id)
        && ui.get(id.as_str()).activated()
    {
        cx.p.shared.edit_control(part, c, i32::from(!e.on));
    }
    let mut cells = vec![block(2, Len::Pct(100.)).fill(if e.on { Fill::from(accent()) } else { Role::Ink.alpha(0.) })];
    if let Some((control, on)) = e.enable {
        let (hit, el) = check(ui, format!("ksp-{part}-{control}"), &format!("{}: on", e.name), on);
        if hit {
            cx.p.shared.edit_control(part, control, i32::from(!on));
        }
        cells.push(el);
    }
    cells.push(
        body(e.name.clone())
            .text_size(TEXT)
            .fill(if e.on { Role::Ink } else { Role::Dim })
            .lines(1)
            .min_w(0)
            .flex(1),
    );
    if let Some(n) = routed {
        cells.extend(routing_cells(ui, cx, part, n));
    }
    let remap = routed.and_then(|n| cx.selection.parts[part].articulate.articulations[n].remap);
    if let Some(key) = &e.key {
        // Neutral text; the keyboard's keyswitch color rides on a swatch.
        // A remapped keyswitch shows the key that plays it, in ink.
        let badge = row![
            block(SMALL * 0.6, SMALL * 0.6).fill(Fill::from(keyswitch())),
            caption(remap.map_or_else(|| key.clone(), note_name))
                .text_size(SMALL)
                .fill(if remap.is_some() { Role::Ink } else { Role::Dim })
                .reserve("C#-1"),
        ]
        .gap(SPACE * 0.5)
        .align(Align::Center)
        .shrink(0);
        cells.push(match routed {
            Some(n) => {
                let id = format!("art-key-{part}-{n}");
                if ui.get(id.as_str()).activated() {
                    let target = Target::Keyswitch { part, row: n, learning: None };
                    menu::open_under(ui, cx, target, &id);
                }
                let tip = match remap {
                    Some(to) => format!("{}: keyswitch {key}, played from {}", e.name, note_name(to)),
                    None => format!("{}: keyswitch {key}. Click to remap", e.name),
                };
                interactive(
                    badge.pad((TIGHT, 0)).h(STRIP - 2.).focusable().a11y(A11y::Button).named(tip.clone()).tip(tip).id(id),
                    false,
                )
            }
            None => badge,
        });
    }
    for item in &e.extras {
        cells.push(control(ui, cx, part, item).h(STRIP - 2.));
    }
    let el = row(cells)
        .gap(SPACE)
        .align(Align::Center)
        .pad(edges(0., if e.extras.is_empty() { SPACE } else { 1. }, 0., 0.))
        .h(STRIP)
        .fill(if e.on { Role::Raised } else { Role::Field });
    match id {
        Some(id) => interactive(
            el.focusable()
                .a11y(A11y::Toggle { on: e.on })
                .named(e.name.clone())
                .tip(e.key.as_ref().map_or_else(|| e.name.clone(), |k| format!("{} · keyswitch {k}", e.name)))
                .id(id),
            e.on,
        ),
        None => el.named(e.name.clone()),
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

    /// Afflatus's options page: names right of their knobs, an envelope
    /// whose labels sit over its knobs and under the knob above, and a
    /// heading over three switches.
    #[test]
    fn labels_go_to_the_control_they_belong_to() {
        let text = |t: &str| ("TEXT", Value::Text(t.into()));
        let label = |x, y, w, t: &str| control("ui_label", "$l", &with(at(x, y, w, 17), &[text(t)]));
        let knob = |x, y, var: &str| control("ui_knob", var, &at(x, y, 32, 32));
        let mut controls = vec![label(658, 82, 202, "Sustain MonoLeg PolyLeg")];
        for (x, var) in [(659, "$sustain_on"), (729, "$mono_leg_on"), (799, "$poly_leg_on")] {
            controls.push(control("ui_switch", var, &at(x, 99, 60, 32)));
        }
        controls.push(knob(772, 189, "$Rel_Offset"));
        controls.push(label(806, 191, 58, "Release Offset"));
        for (x, name) in [(647, "Attack"), (703, "Decay"), (759, "Sustain"), (815, "Release")] {
            controls.push(label(x, 234, 56, name));
            controls.push(knob(x + 13, 250, &format!("$Advanced_CO_{name}")));
        }
        let interface = Interface {
            width: 900,
            height: 300,
            controls,
            ..Default::default()
        };
        let named: Vec<(String, String)> = items(&interface, &HashMap::new())
            .into_iter()
            .map(|i| (interface.controls[i.control].variable.clone(), i.name))
            .collect();
        let name = |var: &str| named.iter().find(|(v, _)| v == var).map(|(_, n)| n.as_str());
        assert_eq!(name("$Rel_Offset"), Some("Release Offset"));
        assert_eq!(name("$Advanced_CO_Sustain"), Some("Sustain"));
        assert_eq!(name("$Advanced_CO_Attack"), Some("Attack"));
        assert_ne!(name("$mono_leg_on"), Some("Sustain MonoLeg PolyLeg"), "the heading names no switch");
        assert_eq!(name("$l"), Some("Sustain MonoLeg PolyLeg"), "and stays a heading");
        assert!(is_note("C-1") && is_note("D#3") && !is_note("Decay") && !is_note("A"));
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
        assert_eq!(names, [vec!["Legato", "Port"], vec!["Velocity"]]);
        // A knob far below the row is a band of its own, not a column under it.
        let mut interface = interface;
        interface.controls.push(control("ui_knob", "$tone", &with(at(20, 200, 32, 40), &[text("Tone")])));
        let s = sections(&interface, &HashMap::new());
        assert_eq!(s.iter().map(|s| s.band).collect::<Vec<_>>(), [0, 1]);
    }

    #[test]
    fn names_come_from_pictures_and_variables() {
        let pyramid = ["pyramid".to_owned()];
        assert_eq!(words("pyramid_but_classic_mix_1_of_2", &pyramid).as_deref(), Some("Classic Mix"));
        assert_eq!(words("$slider_controller_reverb_mix", &[]).as_deref(), Some("Controller Reverb Mix"));
        assert_eq!(words("K4_SLIDER_BIP_1", &[]), None);
        assert_eq!(words("$T1_sliUCVol", &[]).as_deref(), Some("Uc Volume"));
        assert_eq!(words("gi_uc_knb_127", &[]), Some("Uc".into()));
        assert_eq!(words("gi_uc_btn_space2", &["uc".into()]).as_deref(), Some("Space"));
        assert_eq!(words("$switch_art_onoff0", &[]).as_deref(), Some("Articulation"));
        assert!(!readable("$q5nsy") && !readable("$zptkf") && !readable("$ruqnf"));
        assert!(readable("$vibrato") && readable("$mic_volume"));
        assert_eq!(clean("\t\t\t   Legato Rebowed"), "Legato Rebowed");
        assert_eq!(spell_out("Rel Vol"), "Release Volume");
        assert_eq!(spell_out("Atk Thresh"), "Attack Threshold");
        assert_eq!(spell_out("Rel. Off."), "Release Offset");
        assert_eq!(spell_out("Mute Off"), "Mute Off", "a bare Off is a state");
        assert!(is_readout("0.0 dB") && is_readout("20.0 ms") && is_readout("63") && is_readout("OFF"));
        assert!(!is_readout("Dynamics") && !is_readout("C-1 Legato"));
    }

    /// Una Corda's and Solo's script-internal names come out as words a
    /// player would use: library prefixes, numbering and on/off suffixes go,
    /// abbreviations are spelled out, an on/off switch beside its
    /// articulation reads "On".
    #[test]
    fn library_prefixes_and_abbreviations_leave_the_names() {
        let pic = |p: &str| ("PICTURE", Value::Text(p.into()));
        let una_corda = [
            ("ui_switch", "$Mas_swiTab1", "gi_uc_btn_tab_workbench", 10),
            ("ui_switch", "$Mas_swiTab2", "gi_uc_btn_tab_response", 120),
            ("ui_slider", "$Mas_sliColor", "gi_uc_knb_127", 240),
            ("ui_slider", "$Mas_sliDyn", "gi_uc_knb_127", 360),
            ("ui_switch", "$Mas_swiSpace", "gi_uc_btn_space2", 480),
        ];
        let controls = una_corda
            .iter()
            .map(|(kind, var, p, x)| control(kind, var, &with(at(*x, 10, 60, 60), &[pic(p)])))
            .collect();
        let interface = Interface { width: 632, height: 200, controls, ..Default::default() };
        let names: Vec<String> = items(&interface, &HashMap::new()).into_iter().map(|i| i.name).collect();
        assert_eq!(names, ["Workbench", "Response", "Color", "Dynamics", "Space"]);

        let controls = vec![
            control("ui_label", "$label_art_select2", &with(at(10, 10, 100, 18), &[("TEXT", Value::Text("\t\tSustained".into()))])),
            control("ui_switch", "$switch_art_select_transparent2", &with(at(10, 10, 100, 18), &[pic("empty")])),
            control("ui_switch", "$switch_art_onoff2", &with(at(110, 10, 18, 18), &[pic("pyramid_but_screen_item_byp")])),
            control("ui_knob", "$lfw", &with(at(10, 60, 40, 40), &[("TEXT", Value::Text("Rel Vol".into()))])),
        ];
        let interface = Interface { width: 632, height: 200, controls, ..Default::default() };
        let names: Vec<String> = items(&interface, &HashMap::new()).into_iter().map(|i| i.name).collect();
        assert_eq!(names, ["Sustained", "On", "Release Volume"]);
    }

    /// Every knob and fader reads with its unit, never a bare number.
    #[test]
    fn values_read_with_their_units() {
        let item = |name: &str, label: &str, min: f64, max: f64| Item {
            control: 0,
            face: Face::Knob,
            bipolar: min < 0. && min == -max,
            name: name.into(),
            label: label.into(),
            unit: None,
            enable: false,
            raw: 0.,
            min,
            max,
            reset: 0.,
            at: Rect::default(),
            hidden: false,
            list: Vec::new(),
        };
        // Vista's mic Volume: the script's "0.0" is decibels.
        assert_eq!(readout(&item("Volume", "0.0", 0., 1e6), 630_000.), "0.0 dB");
        // Without a label, by the engine's volume law, live as it moves.
        assert_eq!(readout(&item("Volume", "", 0., 1e6), 630_000.), "0.0 dB");
        assert_eq!(readout(&item("Volume", "", 0., 1e6), 0.), "-inf dB");
        assert_eq!(readout(&item("Pan", "", 0., 1e6), 250_000.), "L 50");
        assert_eq!(readout(&item("Attack", "", 0., 1e6), 0.), "0.0 ms");
        assert_eq!(readout(&item("Cutoff", "", 0., 1e6), 1e6), "21.7 kHz");
        assert_eq!(readout(&item("Tune", "", 0., 1e6), 1e6), "+36 st");
        // Pacific's Release knob says "25000.0" in milliseconds.
        let mut release = item("Release", "25000.0", 0., 1e6);
        release.unit = Unit::of_knob("$KNOB_UNIT_MS");
        assert_eq!(readout(&release, 1e6), "25.00 s");
        // What the script says in words stays; "0%" gets its space.
        assert_eq!(readout(&item("Space", "-inf dB", 0., 5e5), 0.), "-inf dB");
        assert_eq!(readout(&item("Amount", "OFF", 0., 6.), 0.), "OFF");
        assert_eq!(readout(&item("Dynamics", "0%", -200., 200.), 0.), "0 %");
        // Una Corda's Color, -40 to 40, a bare "0": signed around its middle.
        assert_eq!(readout(&item("Color", "0", -40., 40.), 0.), "0 %");
        assert_eq!(readout(&item("Color", "", -40., 40.), 20.), "+50 %");
        assert_eq!(readout(&item("Reverb", "", 0., 127.), 80.), "63 %");
        assert_eq!(Unit::Hertz.format(1234.), "1.2 kHz");
        assert_eq!(Unit::Millis.format(120.), "120 ms");
        assert_eq!(Unit::Semitones.format(3.), "+3 st");
    }

    /// Plays mixed articulations at once on real libraries through the
    /// articulation router in channel mode and reports, per pair, whether
    /// both sound: each articulation's groups are learned from its own
    /// keyswitch first (four times over, for round robins). `KONTAKTO_SHOT`
    /// names the instruments; run with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn articulations_play_together_on_libraries() {
        use crate::articulate::{Articulate, In, Mode, Route, Router, apply};
        use crate::engine::{Bank, Engine, MAX_BLOCK, MEMORY_LIMIT, load_scripts};
        use std::collections::BTreeSet;
        let files = crate::import::presets(std::path::Path::new(crate::import::LIBRARY_ROOT)).unwrap_or_default();
        let names = std::env::var("KONTAKTO_SHOT").unwrap_or_default();
        const RATE: f64 = 48000.;
        let render = |e: &mut Engine, seconds: f64| {
            let (mut l, mut r) = ([0f32; MAX_BLOCK], [0f32; MAX_BLOCK]);
            for _ in 0..(seconds * RATE / MAX_BLOCK as f64) as usize {
                e.render(&mut l, &mut r);
            }
        };
        // Groups by family: their names without numbers, so round robins
        // and dynamic layers of one articulation read as one.
        let sounding = |e: &Engine, i: &crate::import::Instrument| -> BTreeSet<String> {
            (e.voice_census().iter())
                .filter(|v| !v.released && v.gain * v.envelope > 1e-3)
                .map(|v| i.groups[v.group as usize].name.chars().filter(|c| !c.is_ascii_digit()).collect())
                .collect()
        };
        let quiet = |e: &mut Engine| {
            for c in 0..16 {
                e.cc(c, 123, 0);
            }
            render(e, 2.0);
        };
        for name in names.split(',') {
            let Some(path) = files.iter().find(|p| p.file_stem().is_some_and(|n| n == name)) else {
                println!("== {name}: not found");
                continue;
            };
            let i = crate::import::read(path).unwrap();
            let (script, _) = load_scripts(&i, i.script_state.clone(), RATE);
            let controllers = script.as_deref().map_or(Vec::new(), |rt| rt.init_controllers.clone());
            let bank = Bank::load_counting(&i, MEMORY_LIMIT, &controllers, &Default::default()).unwrap();
            let mut e = Engine::default();
            e.blocking_streams = true;
            e.set_bank(Some(Box::new(bank)));
            e.set_script(script);
            render(&mut e, 1.0);
            let view = crate::plugin::script_interface(e.script());
            let Some(interface) = view.interface else {
                println!("== {name}: no performance view");
                continue;
            };
            let pictures = crate::artwork::pictures(
                &i.path,
                interface.controls.iter().filter_map(|c| match c.properties.get("$CONTROL_PAR_PICTURE") {
                    Some(Value::Text(n)) => Some(n.as_str()),
                    _ => None,
                }),
            );
            // KONTAKTO_MODE=mpe: member channel bend and pressure, per note.
            if std::env::var("KONTAKTO_MODE").is_ok_and(|m| m == "mpe") {
                use crate::articulate::{Mpe, Zone, feed};
                let note = std::env::var("KONTAKTO_NOTE").ok().and_then(|n| n.parse().ok()).unwrap_or(64);
                let mut router = Router::default();
                let mpe = Mpe { zone: Zone::Lower, ..Mpe::default() };
                router.set_route(Route::new(&path.to_string_lossy(), &Articulate::default(), &mpe));
                // Crossings per second and level of 0.5 s of the note, 0.2 s in.
                let mut listen = |e: &mut Engine, router: &mut Router, before: &[In], after: &[In]| {
                    quiet(e);
                    for &ev in before {
                        feed(router, e, ev, 0);
                    }
                    feed(router, e, In::NoteOn(1, note, 100), 0);
                    for &ev in after {
                        feed(router, e, ev, 0);
                    }
                    render(e, 0.2);
                    let (mut l, mut r) = ([0f32; MAX_BLOCK], [0f32; MAX_BLOCK]);
                    let (mut crossings, mut energy, mut last, mut n) = (0, 0f64, 0f32, 0);
                    for _ in 0..(0.5 * RATE / MAX_BLOCK as f64) as usize {
                        e.render(&mut l, &mut r);
                        for &x in &l {
                            crossings += usize::from((x >= 0.) != (last >= 0.));
                            energy += f64::from(x * x);
                            last = x;
                            n += 1;
                        }
                    }
                    feed(router, e, In::NoteOff(1, note), 0);
                    (crossings as f64 / 0.5, 10. * (energy / n as f64).max(1e-20).log10())
                };
                let plain = listen(&mut e, &mut router, &[In::Bend(1, 8192)], &[]);
                let up = listen(&mut e, &mut router, &[In::Bend(1, 8192 + 2048)], &[]);
                let soft = listen(&mut e, &mut router, &[In::Bend(1, 8192)], &[In::Pressure(1, 0)]);
                let hard = listen(&mut e, &mut router, &[], &[In::Pressure(1, 127)]);
                let handles = e.script().is_some_and(|rt| rt.handles_poly_at());
                println!("== {name}: note {note}, scripts take poly_at: {handles}");
                println!("  plain {:.0} crossings/s {:.1} dB; bent +12 st {:.0}/s (x{:.2})", plain.0, plain.1, up.0, up.0 / plain.0);
                println!("  pressure 0: {:.1} dB, 127: {:.1} dB ({:+.1} dB)", soft.1, hard.1, hard.1 - soft.1);
                continue;
            }
            let mut found = articulations(&sections(&interface, &pictures), view.slot, &view.keys);
            // KONTAKTO_BY_CONTROL=1 switches by the list's controls, as a click does, not by key.
            if std::env::var("KONTAKTO_BY_CONTROL").is_ok() {
                found.iter_mut().filter(|f| f.2.is_some()).for_each(|f| f.1 = None);
            }
            println!("== {name}: {} articulations", found.len());
            // A note every articulation plays: the middle of the mapped keys above the keyswitches.
            let lowest = found.iter().filter_map(|f| f.1).max().map_or(0, |k| k + 1);
            let mut mapped: Vec<u8> = (i.zones.iter())
                .filter(|z| z.available && z.high_key >= lowest)
                .flat_map(|z| z.low_key.max(lowest)..=z.high_key)
                .collect();
            mapped.sort_unstable();
            let note = mapped.get(mapped.len() / 2).copied().unwrap_or(60).min(120);
            let note = std::env::var("KONTAKTO_NOTE").ok().and_then(|n| n.parse().ok()).unwrap_or(note);
            println!("  playing {note} and {}", note + 4);
            let path = path.to_string_lossy().into_owned();
            let mut a = Articulate::default();
            a.sync(&path, &found);
            let mut router = Router::default();
            let by_velocity = std::env::var("KONTAKTO_MODE").is_ok_and(|m| m == "velocity");
            let send = |e: &mut Engine, router: &mut Router, ev| router.input(ev, 0, &mut |o| apply(e, o));
            // Each articulation's groups, as its own keyswitch or click plays them.
            let mut groups: Vec<BTreeSet<String>> = Vec::new();
            for (n, f) in found.iter().enumerate() {
                let mut all = BTreeSet::new();
                for rep in 0..8 {
                    quiet(&mut e);
                    match (f.1, f.2) {
                        (Some(key), _) => {
                            e.note_on(0, key, 100);
                            e.note_off(0, key);
                        }
                        (None, Some((slot, control))) => e.ui_control(slot.into(), control.into(), 1),
                        _ => {}
                    }
                    render(&mut e, 0.05);
                    // Both notes the pairs play, round robins and all.
                    let played = note + if rep % 2 == 0 { 0 } else { 4 };
                    e.note_on(0, played, 100);
                    render(&mut e, 0.25);
                    all.extend(sounding(&e, &i));
                    e.note_off(0, played);
                }
                println!("  {n:2} {:32} key {:?} control {:?}: {} group families", f.0, f.1, f.2, all.len());
                groups.push(all);
            }
            let Some(first) = (0..found.len()).find(|&n| !groups[n].is_empty()) else {
                continue;
            };
            let (mut together, mut pairs) = (0, 0);
            for b in (0..found.len()).filter(|&b| b != first) {
                let only_a: BTreeSet<String> = groups[first].difference(&groups[b]).cloned().collect();
                let only_b: BTreeSet<String> = groups[b].difference(&groups[first]).cloned().collect();
                if only_a.is_empty() || only_b.is_empty() {
                    println!("  {} + {}: same groups, cannot tell apart", found[first].0, found[b].0);
                    continue;
                }
                for staggered in [false, true] {
                    quiet(&mut e);
                    // KONTAKTO_MODE=velocity splits by velocity (1..=63, 64..=127) on one channel.
                    let mut a = a.clone();
                    a.mode = if by_velocity { Mode::Velocity } else { Mode::Channel };
                    for (n, art) in a.articulations.iter_mut().enumerate() {
                        art.enabled = n == first || n == b;
                        art.channel = u8::from(n == b);
                        (art.low, art.high) = if n == b { (64, 127) } else { (1, 63) };
                    }
                    let (ch, soft) = if by_velocity { (0, 40) } else { (1, 100) };
                    router.set_route(Route::new(&path, &a, &Default::default()));
                    send(&mut e, &mut router, In::NoteOn(0, note, soft));
                    if staggered {
                        render(&mut e, 0.1);
                    }
                    send(&mut e, &mut router, In::NoteOn(ch, note + 4, 100));
                    render(&mut e, 0.25);
                    let now = sounding(&e, &i);
                    let (has_a, has_b) = (!now.is_disjoint(&only_a), !now.is_disjoint(&only_b));
                    pairs += 1;
                    together += usize::from(has_a && has_b);
                    println!(
                        "  {} + {} ({}): {}",
                        found[first].0,
                        found[b].0,
                        if staggered { "second 100 ms later" } else { "one chord" },
                        match (has_a, has_b) {
                            (true, true) => "both sound".to_owned(),
                            _ => format!("first {has_a}, second {has_b}: {now:?}"),
                        }
                    );
                    send(&mut e, &mut router, In::NoteOff(0, note));
                    send(&mut e, &mut router, In::NoteOff(ch, note + 4));
                }
            }
            println!("  together in {together} of {pairs}");
        }
    }
}
