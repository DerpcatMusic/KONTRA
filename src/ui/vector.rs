//! The vectorized performance view's plan: what KONTRA draws for each
//! control of a library's view, where its words go and at what size. The
//! controls keep their places and their order; what their pictures showed
//! becomes flat faces, and every word fits the room it is given or is not
//! drawn. The view draws the plan, and the audit checks it.

use super::cover::advance;
use super::panel;
use super::perf_view::{FONT, Kind, LINE, Shown, break_lines, caption_of, keep_spaces, knob_like, prop, value};
use crate::artwork::Picture;
use crate::ksp::{Control, Interface, Value};
use moose::mui::mui::scene::Image;
use std::collections::HashMap;
use std::sync::Arc;

/// The smallest a word is set, against its size: Kontakt's own knob names
/// are about this small.
pub const SMALLEST: f64 = 0.65;

/// What stands in for a control's picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Face {
    /// KONTRA's own face for its kind.
    Normal,
    /// Nothing: its picture is clear, it only takes clicks.
    Clear,
    /// The view's own background: a picture that covers what scrolls under it.
    Cover,
    /// A flat panel, the picture behind other controls; `true` when it lies on another.
    Panel(bool),
    /// A small picture-only switch: a mark that shows whether it is on.
    Mark(Mark),
    /// A slider laid over a waveform: a line at its position.
    Marker,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mark {
    Dot,
    Left,
    Right,
}

/// Words drawn for a control, in authored points from its corner: they fit
/// `w` at `size`.
#[derive(Clone, Debug, PartialEq)]
pub struct Words {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub size: f64,
    /// 0 left, 1 centred, 2 right.
    pub align: i32,
}

impl Words {
    /// Where the ink lies across, from the control's corner.
    pub fn ink(&self) -> (f64, f64) {
        let wide = advance(&self.text, self.size).min(self.w);
        let x = match self.align {
            1 => self.x + (self.w - wide) / 2.,
            2 => self.x + self.w - wide,
            _ => self.x,
        };
        (x, wide)
    }
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub face: Face,
    pub words: Vec<Words>,
}

/// `text` set to fit `room` points: at `size`, or smaller down to
/// [`SMALLEST`] of it; else its last words that fit so; else nothing.
pub fn fitted(text: &str, room: f64, size: f64) -> Option<(String, f64)> {
    // As the script spaced it ("Sustain   MonoLeg   PolyLeg" over three
    // buttons) while it is whole.
    let mut words: Vec<&str> = text.split_whitespace().collect();
    let whole = words.len();
    while !words.is_empty() {
        let t = if words.len() == whole { text.trim().to_owned() } else { words.join(" ") };
        let wide = advance(&t, size);
        if wide <= room {
            return Some((t, size));
        }
        if wide * SMALLEST <= room {
            return Some((t, size * room / wide));
        }
        words.remove(0);
    }
    None
}

fn int(c: &Control, name: &str) -> Option<i32> {
    match c.properties.get(name)? {
        Value::Int(n) => Some(*n),
        Value::Real(r) => Some(*r as i32),
        _ => None,
    }
}

/// How much of `image` is solid, 0 to 1, from a grid of its pixels.
pub fn opacity(image: &Image) -> f32 {
    let (w, h) = (image.width as usize, image.height as usize);
    if w == 0 || h == 0 {
        return 0.;
    }
    let n = 16;
    let mut solid = 0;
    for j in 0..n {
        for i in 0..n {
            let (x, y) = ((2 * i + 1) * w / (2 * n), (2 * j + 1) * h / (2 * n));
            solid += usize::from(image.rgba.get((y * w + x) * 4 + 3).is_some_and(|a| *a >= 128));
        }
    }
    solid as f32 / (n * n) as f32
}

fn inside(a: &Shown, b: &Shown) -> f64 {
    let iw = ((a.x + a.w).min(b.x + b.w) - a.x.max(b.x)).max(0.);
    let ih = ((a.y + a.h).min(b.y + b.h) - a.y.max(b.y)).max(0.);
    iw * ih
}

/// The plan for every control of `drawn` (in drawing order, each with the
/// frame of its picture the original view shows).
pub fn plan(interface: &Interface, pictures: &HashMap<String, Arc<Picture>>, drawn: &[(Shown, Option<Arc<Image>>)]) -> Vec<Plan> {
    let (vw, vh) = (f64::from(interface.width), f64::from(interface.height));
    let view = vw * vh;
    let names = names(interface, pictures, drawn);
    let mut faces: Vec<Face> = Vec::with_capacity(drawn.len());
    for (n, (s, frame)) in drawn.iter().enumerate() {
        let c = &interface.controls[s.control];
        let picture = prop(c, "$CONTROL_PAR_PICTURE");
        let solid = frame.as_deref().map(opacity);
        // Clear in every state: a place to click, nothing to see.
        let clear = s.picture.as_ref().is_some_and(|p| p.frames.iter().step_by(p.frames.len().div_ceil(8).max(1)).all(|f| opacity(f) < 0.02));
        let said = !caption_of(c, s.kind, value(c)).0.trim().is_empty();
        let named = |w: &[&str]| [picture, c.variable.as_str()].iter().any(|n| panel::raw_words(n).iter().any(|x| w.contains(&x.as_str())));
        let lower = format!("{picture} {}", c.variable).to_lowercase();
        let face = match s.kind {
            Kind::Switch | Kind::Button | Kind::Label if named(&["cover", "mask"]) => Face::Cover,
            Kind::Label if solid.is_some_and(|o| o >= 0.6) && s.w * s.h < 0.8 * view => {
                let on = faces[..n].iter().zip(drawn).any(|(f, (o, _))| matches!(f, Face::Panel(_) | Face::Cover) && inside(o, s) >= 0.9 * s.w * s.h);
                Face::Panel(on)
            }
            Kind::Switch | Kind::Button if !said && clear => Face::Clear,
            // A clear slider shows through to the pictures under it that draw it
            // (ANALOG STRINGS' strings); over nothing, it shows nothing.
            Kind::Slider | Kind::Knob if clear && !drawn.iter().any(|(o, f)| o.kind == Kind::Label && f.is_some() && inside(o, s) > 0.) => Face::Clear,
            Kind::Switch | Kind::Button if !said && (lower.contains("prev") || lower.contains("left")) => Face::Mark(Mark::Left),
            Kind::Switch | Kind::Button if !said && (lower.contains("next") || lower.contains("right")) => Face::Mark(Mark::Right),
            // A small switch shows its state, as does a button that turns
            // something on; another button only acts: its name or an empty frame.
            Kind::Switch | Kind::Button
                if !said && s.w.min(s.h) <= 26. && s.w <= 2. * s.h
                    && !names.get(&s.control).is_some_and(|n| matches!(n.as_str(), "?" | "i") || n.starts_with(['+', '-']))
                    && names.get(&s.control).is_some_and(|n| matches!(n.as_str(), "On" | "Off" | "Power")) =>
            {
                Face::Mark(Mark::Dot)
            }
            Kind::Slider if over_wave(drawn, s) => Face::Marker,
            _ => Face::Normal,
        };
        faces.push(face);
    }
    // Last drawn first, so each control's words know the words drawn over
    // them: (x, y, w, h) in the view.
    let mut out: Vec<Plan> = Vec::with_capacity(drawn.len());
    let mut inked: Vec<(f64, f64, f64, f64)> = Vec::new();
    for (n, (s, _)) in drawn.iter().enumerate().rev() {
        let c = &interface.controls[s.control];
        let face = faces[n];
        let mut words = Vec::new();
        let (said, align, top) = caption_of(c, s.kind, value(c));
        // What lies on it, drawn after: its words go beside them.
        let on: Vec<(f64, f64)> = (drawn.iter().zip(&faces).skip(n + 1))
            .filter_map(|((o, _), f)| hides(o, &interface.controls[o.control], *f))
            .filter(|o| o.w * o.h < s.w * s.h && inside(o, s) > 0.)
            .map(|o| (o.x - s.x, o.x - s.x + o.w))
            .chain(inked.iter().filter(|i| i.1 < s.y + s.h && i.1 + i.3 > s.y).map(|i| (i.0 - s.x, i.0 - s.x + i.2)))
            .collect();
        // Where `text` goes: across the whole control, unless it would
        // run into what lies on it; then beside that.
        let spot = |text: &str, align: i32| {
            let hits = fitted(text, s.w - 4., FONT).is_some_and(|(text, size)| {
                let (x, wide) = Words { text, x: 2., y: 0., w: s.w - 4., h: 0., size, align }.ink();
                on.iter().any(|&(a, b)| x < b && x + wide > a)
            });
            if hits { free(s.w, &on) } else { (0., s.w) }
        };
        let push = |words: &mut Vec<Words>, text: &str, (x, w): (f64, f64), y: f64, h: f64, align: i32| {
            if let Some((text, size)) = fitted(text, w - 4., FONT) {
                words.push(Words { text, x: x + 2., y, w: w - 4., h, size, align });
            }
        };
        match (s.kind, face) {
            (_, Face::Clear | Face::Cover | Face::Mark(_) | Face::Marker) => {}
            (Kind::Label, _) => {
                // Padding the script set to clear an icon of its picture goes with the picture.
                let said = said.lines().map(str::trim).collect::<Vec<_>>().join("\n");
                let longest = said.lines().max_by(|a, b| advance(a, FONT).total_cmp(&advance(b, FONT))).unwrap_or("");
                let room = spot(longest, align);
                let lines = break_lines(&said, room.1 - 4., FONT, s.h >= 2. * LINE);
                let tall = LINE * lines.len() as f64;
                let y0 = top.map_or((s.h - tall) / 2., |y| y);
                // Text set below the label's foot is clipped away, as Kontakt hides it.
                if top.is_none_or(|y| y < s.h) {
                    for (k, line) in lines.iter().enumerate() {
                        push(&mut words, line, room, y0 + k as f64 * LINE, LINE, align);
                    }
                }
            }
            (Kind::Switch | Kind::Button, _) => {
                let name = if said.trim().is_empty() { names.get(&s.control).cloned().unwrap_or_default() } else { said };
                let tab = prop(c, "$CONTROL_PAR_HELP").split_once(':').is_some_and(|(title, _)| title.ends_with(" Tab"));
                // A small switch's name that will not fit whole is its
                // initials, as a mixer strip's S and M; more than two say
                // nothing, so its frame stands alone. A step (-12) shrinks.
                let step = name.trim().starts_with(['+', '-']);
                let name = if !step && fitted(&name, s.w - if tab { 0. } else { 4. }, FONT).is_none() {
                    let initials: String = name.split_whitespace().filter_map(|w| w.chars().next()).collect();
                    if initials.chars().count() <= 2 { initials } else { String::new() }
                } else {
                    name
                };
                let room = if tab { (-2., s.w + 4.) } else { spot(&name, align) };
                push(&mut words, &name, room, 0., s.h, align);
            }
            (Kind::Menu, _) => push(&mut words, &said, (0., if s.w > 30. { s.w - 14. } else { s.w }), 0., s.h, align),
            (Kind::Value | Kind::TextEdit, _) => push(&mut words, &said, (0., s.w), 0., s.h, align),
            (Kind::Knob, _) => {
                // Kontakt's own knob: its name over it, its value under it.
                let hide = int(c, "$CONTROL_PAR_HIDE").unwrap_or(0);
                let row = FONT * 1.4;
                let title = prop(c, "$CONTROL_PAR_TEXT");
                let title = if title.is_empty() { c.variable.trim_start_matches(['$', '~']) } else { title };
                if hide & 4 == 0 {
                    push(&mut words, &keep_spaces(title), (0., s.w + 2.), 0., row, 1);
                }
                if hide & 2 == 0 {
                    let label = prop(c, "$CONTROL_PAR_LABEL");
                    let shown_value = if label.is_empty() { format!("{}", value(c).round()) } else { keep_spaces(label) };
                    push(&mut words, &shown_value, (0., s.w + 2.), s.h - row, row, 1);
                }
            }
            (Kind::Slider, Face::Normal) if knob_like(prop(c, "$CONTROL_PAR_PICTURE"), s.w, s.h) => {
                // A knob the wallpaper named: the name under it, as wide as it.
                if let Some(name) = names.get(&s.control) {
                    push(&mut words, name, (0., s.w + 4.), s.h, FONT * 1.4, 1);
                }
            }
            _ => {}
        }
        // What a control drawn later covers, or what lies outside the
        // view, the original does not show either.
        words.retain(|w| {
            let (x, wide) = w.ink();
            let (x, y, tall) = (s.x + x, s.y + w.y + (w.h - w.size * 1.2).max(0.) / 2., w.size * 1.2);
            let meets = |o: &Shown| (x + wide).min(o.x + o.w) - x.max(o.x) > 0.5 && (y + tall).min(o.y + o.h) - y.max(o.y) > 0.5;
            let covered = (drawn.iter().zip(&faces).skip(n + 1)).filter_map(|((o, _), f)| hides(o, &interface.controls[o.control], *f)).any(|o| meets(&o))
                || inked.iter().any(|&(x, y, w, h)| meets(&Shown { x, y, w, h, ..s.clone() }));
            !covered && x >= 0. && y >= 0. && x + wide <= vw + 0.5 && y + tall <= vh + 0.5
        });
        inked.extend(words.iter().map(|w| {
            let (x, wide) = w.ink();
            (s.x + x, s.y + w.y + (w.h - w.size * 1.2).max(0.) / 2., wide, w.size * 1.2)
        }));
        out.push(Plan { face, words });
    }
    out.reverse();
    out
}

/// What of control `o`, drawn as `face`, hides what lies under it: its
/// rect, the middle of it for a round knob (its ring leaves the corners
/// and its foot open), none for a clear one or bare words.
pub fn hides(o: &Shown, c: &Control, face: Face) -> Option<Shown> {
    let round = o.kind == Kind::Knob || o.kind == Kind::Slider && knob_like(prop(c, "$CONTROL_PAR_PICTURE"), o.w, o.h);
    match face {
        // A marker is a line: what is under it shows.
        Face::Clear | Face::Marker => None,
        Face::Normal if matches!(o.kind, Kind::Label | Kind::Area | Kind::Other) => None,
        Face::Normal if round => {
            let k = o.w.min(o.h) * 0.15;
            Some(Shown { x: o.x + k, y: o.y + k, w: o.w - 2. * k, h: o.h - 2. * k, ..o.clone() })
        }
        _ => Some(o.clone()),
    }
}

/// Whether slider `s` lies mostly over a waveform: its position on the wave.
fn over_wave(drawn: &[(Shown, Option<Arc<Image>>)], s: &Shown) -> bool {
    drawn.iter().any(|(o, _)| o.kind == Kind::Waveform && inside(o, s) >= 0.5 * s.w * s.h)
}

/// The widest stretch of `0..w` none of `taken` covers, as (start, width).
fn free(w: f64, taken: &[(f64, f64)]) -> (f64, f64) {
    let mut cuts: Vec<(f64, f64)> = taken.iter().map(|&(a, b)| (a.max(0.), b.min(w))).filter(|(a, b)| b > a).collect();
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut best, mut at) = ((0., 0.), 0.);
    for (a, b) in cuts.into_iter().chain([(w, w)]) {
        if a - at > best.1 {
            best = (at, a - at);
        }
        at = f64::max(at, b);
    }
    best
}

/// Names for the controls whose pictures said them, as KONTRA's own view
/// reads them; none where one of the script's labels already says it
/// beside the control. A row of switches that share their first words
/// (Header Main, Header Edit, ...) says only what differs.
fn names(interface: &Interface, pictures: &HashMap<String, Arc<Picture>>, drawn: &[(Shown, Option<Arc<Image>>)]) -> HashMap<usize, String> {
    let said: std::collections::HashSet<String> = (interface.controls.iter())
        .filter(|c| c.kind == "ui_label")
        .map(|c| keep_spaces(prop(c, "$CONTROL_PAR_TEXT")).trim().to_lowercase())
        .collect();
    let prefixes = panel::prefixes(interface);
    let mut names: HashMap<usize, String> = panel::names(interface, pictures)
        .into_iter()
        .filter(|(_, n)| !said.contains(&n.trim().to_lowercase()))
        .collect();
    // A library's explicit tooltip heading beats a guessed picture prefix.
    for (s, _) in drawn.iter().filter(|(s, _)| matches!(s.kind, Kind::Switch | Kind::Button)) {
        if let Some((title, _)) = prop(&interface.controls[s.control], "$CONTROL_PAR_HELP").split_once(':') {
            if title.ends_with(" Tab") || title.ends_with(" On/Off") {
                names.insert(s.control, title.trim_end_matches(" Tab").trim_end_matches(" On/Off").to_owned());
            }
        }
    }
    let near = |s: &Shown, o: &Shown| {
        let t = &interface.controls[o.control];
        o.kind == Kind::Label && !prop(t, "$CONTROL_PAR_TEXT").trim().is_empty()
            && o.x < s.x + s.w + 8. && o.x + o.w > s.x - 8.
            && o.y < s.y + s.h + FONT * 2. && o.y + o.h > s.y - FONT * 2.
    };
    let toggles: Vec<&Shown> = drawn.iter().map(|(s, _)| s).filter(|s| matches!(s.kind, Kind::Switch | Kind::Button)).collect();
    for s in drawn.iter().map(|(s, _)| s) {
        if drawn.iter().any(|(o, _)| near(s, o)) && !matches!(s.kind, Kind::Switch | Kind::Button) {
            names.remove(&s.control);
        }
        // A wide switch named for what it turns on ("On") is a tab: its picture says what.
        if names.get(&s.control).is_some_and(|n| matches!(n.as_str(), "On" | "Power")) && s.w >= 3. * s.h {
            if let Some(w) = panel::words(prop(&interface.controls[s.control], "$CONTROL_PAR_PICTURE"), &prefixes) {
                names.insert(s.control, w);
            }
        }
    }
    // What a step button's picture says: help, or a step up or down ("tune_nag_12": -12).
    for s in &toggles {
        let picture = prop(&interface.controls[s.control], "$CONTROL_PAR_PICTURE").to_lowercase();
        let parts: Vec<&str> = picture.split(['_', '-', ' ', '.']).collect();
        let step = parts.windows(2).find_map(|p| {
            let sign = match p[0] {
                "neg" | "nag" | "minus" | "down" | "dec" => "-",
                "pos" | "plus" | "up" | "inc" => "+",
                _ => return None,
            };
            p[1].parse::<u32>().ok().map(|n| format!("{sign}{n}"))
        });
        if let Some(step) = step {
            names.insert(s.control, step);
        } else if parts.contains(&"help") {
            names.insert(s.control, "?".into());
        } else if parts.contains(&"info") {
            names.insert(s.control, "i".into());
        }
    }
    // Rows: switches of one height level with each other, touching.
    let mut rows: Vec<Vec<&Shown>> = Vec::new();
    let mut sorted = toggles.clone();
    sorted.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    for s in sorted {
        match rows.iter_mut().find(|r| r.last().is_some_and(|l| (l.y - s.y).abs() <= 2. && l.h == s.h && (s.x - (l.x + l.w)).abs() <= 16.)) {
            Some(r) => r.push(s),
            None => rows.push(vec![s]),
        }
    }
    for row in rows.iter().filter(|r| r.len() > 1) {
        let split: Vec<Vec<String>> = row.iter().map(|s| names.get(&s.control).map(|n| n.split(' ').map(str::to_owned).collect()).unwrap_or_default()).collect();
        let mut common = split.iter().map(Vec::len).min().unwrap_or(0).saturating_sub(1);
        while common > 0 && !split.iter().all(|w| w[..common] == split[0][..common]) {
            common -= 1;
        }
        if common > 0 {
            for (s, w) in row.iter().zip(&split) {
                names.insert(s.control, w[common..].join(" "));
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_headings_name_picture_only_tabs_and_switches() {
        let u = crate::ksp::initialize("on init\nmake_perfview\nset_ui_height_px(100)\ndeclare ui_switch $tab\nset_text($tab, \"\")\nmove_control_px($tab, 10, 20)\nset_control_par(get_ui_id($tab), $CONTROL_PAR_WIDTH, 40)\nset_control_par(get_ui_id($tab), $CONTROL_PAR_HEIGHT, 24)\nset_control_par_str(get_ui_id($tab), $CONTROL_PAR_HELP, \"Workbench Tab: Opens the page\")\ndeclare ui_switch $space\nset_text($space, \"\")\nmove_control_px($space, 100, 20)\nset_control_par_str(get_ui_id($space), $CONTROL_PAR_HELP, \"Space On/Off: Activates convolution\")\nend on", 0, 8).unwrap();
        let pictures = HashMap::new();
        let drawn = super::super::perf_view::layout(&u, &pictures).into_iter().map(|s| (s, None)).collect::<Vec<_>>();
        let plans = plan(&u, &pictures, &drawn);
        assert!(matches!(plans[0].face, Face::Normal));
        assert!(plans[0].words.iter().any(|w| w.text == "Workbench"));
        let names = names(&u, &pictures, &drawn);
        assert_eq!(names.get(&0).map(String::as_str), Some("Workbench"));
        assert_eq!(names.get(&1).map(String::as_str), Some("Space"));
    }

    #[test]
    fn words_fit_or_go() {
        let size = FONT;
        let wide = advance("Volume", size);
        assert_eq!(fitted("Volume", wide + 1., size), Some(("Volume".into(), size)));
        let (t, s) = fitted("Volume", wide * 0.8, size).unwrap();
        assert!(t == "Volume" && s < size && s >= size * SMALLEST);
        assert_eq!(fitted("Header Main", advance("Main", size) + 1., size), Some(("Main".into(), size)), "its last words");
        assert_eq!(fitted("Volume", 2., size), None);
        assert_eq!(fitted("  ", 100., size), None);
    }

    #[test]
    fn words_go_beside_what_lies_on_them() {
        assert_eq!(free(100., &[]), (0., 100.));
        assert_eq!(free(100., &[(0., 20.)]), (20., 80.));
        assert_eq!(free(100., &[(30., 40.), (35., 50.)]), (50., 50.));
        assert_eq!(free(100., &[(10., 20.), (60., 70.)]), (20., 40.));
    }
}
