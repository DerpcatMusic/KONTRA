//! The vectorized performance view's plan: what KONTRA draws for each
//! control of a library's view, where its words go and at what size. The
//! controls keep their places and their order; what their pictures showed
//! becomes flat faces, and every word fits the room it is given or is not
//! drawn. The view draws the plan, and the audit checks it.

use super::cover::advance;
use super::panel;
use super::perf_view::{FONT, Kind, LINE, Shown, break_lines, caption_of, drags_vertically, frame, keep_spaces, knob_like, prop, value};
use crate::artwork::Picture;
use crate::ksp::{Control, Interface, Value};
use moose::mui::mui::scene::Image;
use std::collections::HashMap;
use std::sync::{Arc, Weak};

/// Immutable artwork classifications owned by one editor. Weak entries retain
/// no pixels and expire when a library's pictures leave the rack.
#[derive(Default)]
pub struct Assets {
    images: HashMap<usize, (Weak<Image>, f32)>,
    pictures: HashMap<usize, (Weak<Picture>, bool)>,
    #[cfg(test)]
    sampled: usize,
}

impl Assets {
    fn prune(&mut self) {
        self.images.retain(|_, (source, _)| source.strong_count() > 0);
        self.pictures.retain(|_, (source, _)| source.strong_count() > 0);
    }

    fn opacity(&mut self, image: &Arc<Image>) -> f32 {
        let key = Arc::as_ptr(image) as usize;
        if let Some((_, value)) = self.images.get(&key) { return *value; }
        let value = opacity(image);
        #[cfg(test)]
        { self.sampled += 1; }
        self.images.insert(key, (Arc::downgrade(image), value));
        value
    }

    fn clear(&mut self, picture: &Arc<Picture>) -> bool {
        let key = Arc::as_ptr(picture) as usize;
        if let Some((_, value)) = self.pictures.get(&key) { return *value; }
        let value = picture.frames.iter().step_by(picture.frames.len().div_ceil(8).max(1))
            .all(|frame| self.opacity(frame) < 0.02);
        self.pictures.insert(key, (Arc::downgrade(picture), value));
        value
    }
}

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
    /// A slider laid over a waveform: a line at its position.
    Marker,
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
    /// An authored animated face was replaced: cover its entire footprint.
    pub skin: bool,
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

/// A broad pictured field edited across its short axis is a value display,
/// rather than a thumb moving along a track. Keep that authored display and
/// its interaction; narrow faders and round knobs still use native faces.
pub(super) fn native_control(s: &Shown, c: &Control) -> bool {
    match s.kind {
        Kind::Knob => true,
        Kind::Slider => {
            // Exclude compact value strips as well as long, skinny tracks.
            let broad = s.picture.is_some() && !knob_like(prop(c, "$CONTROL_PAR_PICTURE"), s.w, s.h)
                && s.w.min(s.h) >= 3. * FONT && s.w.max(s.h) < 3. * s.w.min(s.h);
            let vertical = drags_vertically(s.kind, s.w, s.h, prop(c, "$CONTROL_PAR_PICTURE"),
                int(c, "$CONTROL_PAR_MOUSE_BEHAVIOUR").unwrap_or(0));
            !(broad && vertical != (s.h > s.w))
        }
        _ => false,
    }
}

/// The plan for every control of `drawn` (in drawing order, each with the
/// frame of its picture the original view shows).
pub fn plan(interface: &Interface, _pictures: &HashMap<String, Arc<Picture>>, drawn: &[(Shown, Option<Arc<Image>>)], assets: &mut Assets, current_value: impl Fn(usize) -> Option<f64>) -> Vec<Plan> {
    assets.prune();
    let (vw, vh) = (f64::from(interface.width), f64::from(interface.height));
    let view = vw * vh;
    let names = names(interface, drawn);
    let clear: Vec<bool> = drawn.iter().map(|(s, _)| s.picture.as_ref().is_some_and(|p| assets.clear(p))).collect();
    // Structural skin pairing: an otherwise empty animated label can draw
    // the face of a transparent control occupying the same rectangle. Hide
    // only a uniquely matched control. Coincident animated layers form one
    // skin only when their frame count and phase agree; other artwork stays.
    let controls: Vec<usize> = drawn.iter().enumerate()
        .filter(|(n, (s, _))| native_control(s, &interface.controls[s.control]) && clear[*n] && !over_wave(drawn, s))
        .map(|(n, _)| n).collect();
    let mut pairs = Vec::new();
    let mut counts = vec![0usize; drawn.len()];
    for (n, (label, _)) in drawn.iter().enumerate() {
        let c = &interface.controls[label.control];
        if label.kind != Kind::Label || !prop(c, "$CONTROL_PAR_TEXT").trim().is_empty()
            || !label.picture.as_ref().is_some_and(|p| p.frames.len() > 1 && p.stretch == [false; 2])
        { continue; }
        for &m in &controls {
            let control = &drawn[m].0;
            if inside(label, control) >= 0.9 * (label.w * label.h).max(control.w * control.h)
            {
                pairs.push((n, m));
                counts[n] += 1;
                counts[m] += 1;
            }
        }
    }
    let mut paired = vec![false; drawn.len()];
    for &(label, control) in &pairs {
        let face = &drawn[label].0;
        let c = &interface.controls[face.control];
        let target = &interface.controls[drawn[control].0.control];
        let at = frame(value(target), f64::from(int(target, "$CONTROL_PAR_MIN_VALUE").unwrap_or(0)),
            f64::from(int(target, "$CONTROL_PAR_MAX_VALUE").unwrap_or(1_000_000)), face.picture.as_ref().unwrap().frames.len());
        let phase = int(c, "$CONTROL_PAR_PICTURE_STATE").and_then(|v| usize::try_from(v).ok());
        // Integer KSP arithmetic may truncate where native sprites round.
        paired[label] = phase.is_some_and(|p| p == at || p.checked_add(1) == Some(at))
            && counts[label] == 1 && pairs.iter().filter(|(_, target)| *target == control).all(|&(other, _)| {
            let sibling = &drawn[other].0;
            counts[other] == 1
                && inside(face, sibling) >= 0.9 * (face.w * face.h).max(sibling.w * sibling.h)
                && face.picture.as_ref().map(|p| p.frames.len()) == sibling.picture.as_ref().map(|p| p.frames.len())
                && int(c, "$CONTROL_PAR_PICTURE_STATE") == int(&interface.controls[sibling.control], "$CONTROL_PAR_PICTURE_STATE")
        });
    }
    // Names encoded in pictures can still come from authoritative menu text.
    // This ID is compiled from the authored picture expression, not guessed
    // from a variable or picture name. More than one source is ambiguous.
    let mut skin_names: HashMap<usize, Option<(i32, String)>> = HashMap::new();
    for &(label, control) in &pairs {
        if !paired[label] { continue; }
        let c = &interface.controls[drawn[label].0.control];
        let Some(id) = int(c, "picture menu") else { continue; };
        let Some(menu) = interface.controls.iter().find(|c| c.id == id && c.kind == "ui_menu") else { continue; };
        let Some((text, _)) = menu.menu.iter().find(|(_, v)| f64::from(*v) == value(menu)) else { continue; };
        skin_names.entry(control).and_modify(|source| {
            if source.as_ref().is_none_or(|(old, _)| *old != id) { *source = None; }
        }).or_insert(Some((id, text.clone())));
    }
    let mut faces: Vec<Face> = Vec::with_capacity(drawn.len());
    for (n, (s, frame)) in drawn.iter().enumerate() {
        let c = &interface.controls[s.control];
        let picture = prop(c, "$CONTROL_PAR_PICTURE");
        let solid = (s.kind == Kind::Label).then(|| frame.as_ref().map(|f| assets.opacity(f))).flatten();
        // Clear in every state: a place to click, nothing to see.
        let clear = clear[n];
        let said = matches!(s.kind, Kind::Switch | Kind::Button)
            && !keep_spaces(prop(c, "$CONTROL_PAR_TEXT")).trim().is_empty();
        let named = |w: &[&str]| [picture, c.variable.as_str()].iter().any(|n| panel::raw_words(n).iter().any(|x| w.contains(&x.as_str())));
        let face = match s.kind {
            Kind::Label if paired[n] => Face::Clear,
            Kind::Switch | Kind::Button | Kind::Label if named(&["cover", "mask"]) => Face::Cover,
            Kind::Label if solid.is_some_and(|o| o >= 0.6) && s.w * s.h < 0.8 * view => {
                let on = faces[..n].iter().zip(drawn).any(|(f, (o, _))| matches!(f, Face::Panel(_) | Face::Cover) && inside(o, s) >= 0.9 * s.w * s.h);
                Face::Panel(on)
            }
            Kind::Switch | Kind::Button if !said && clear => Face::Clear,
            // A clear slider shows through to the pictures under it that draw it
            // (ANALOG STRINGS' strings); over nothing, it shows nothing.
            Kind::Slider | Kind::Knob if clear && !drawn.iter().any(|(o, f)| o.kind == Kind::Label && f.is_some() && inside(o, s) > 0.) => Face::Clear,
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
        // Retained labels contribute text occlusion, but the renderer consumes
        // no Vectorized words for the other Original controls.
        if !matches!(s.kind, Kind::Knob | Kind::Slider | Kind::Label) {
            out.push(Plan { face, words: Vec::new(), skin: false });
            continue;
        }
        let mut words = Vec::new();
        let now = current_value(s.control).unwrap_or_else(|| value(c));
        let (said, align, top) = caption_of(c, s.kind, now);
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
            (_, Face::Clear | Face::Cover | Face::Marker) => {}
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
                    let shown_value = if label.is_empty() { format!("{}", now.round()) } else { keep_spaces(label) };
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
        if let Some(Some((_, name))) = skin_names.get(&n) {
            push(&mut words, name, (0., s.w), 2., FONT * 1.4, 1);
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
        if s.kind == Kind::Label { words.clear(); }
        out.push(Plan { face, words, skin: pairs.iter().any(|&(label, control)| control == n && paired[label]) });
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

/// Names for native round sliders, except where an authored label names them.
fn names(interface: &Interface, drawn: &[(Shown, Option<Arc<Image>>)]) -> HashMap<usize, String> {
    // Only round sliders consume inferred names. Native knobs use authored
    // text; menus, switches and labels are still drawn by the Original view.
    let knobs: Vec<_> = drawn.iter().map(|(s, _)| s)
        .filter(|s| s.kind == Kind::Slider && knob_like(prop(&interface.controls[s.control], "$CONTROL_PAR_PICTURE"), s.w, s.h))
        .collect();
    if knobs.is_empty() { return HashMap::new(); }
    let said: std::collections::HashSet<String> = interface.controls.iter()
        .filter(|c| c.kind == "ui_label")
        .map(|c| keep_spaces(prop(c, "$CONTROL_PAR_TEXT")).trim().to_lowercase())
        .collect();
    let prefixes = panel::prefixes(interface);
    knobs.into_iter().filter_map(|s| {
        // Keep the authored label, including one just above or below its knob.
        if drawn.iter().any(|(o, _)| o.kind == Kind::Label
            && !prop(&interface.controls[o.control], "$CONTROL_PAR_TEXT").trim().is_empty()
            && o.x < s.x + s.w + 8. && o.x + o.w > s.x - 8.
            && o.y < s.y + s.h + FONT * 2. && o.y + o.h > s.y - FONT * 2.)
        { return None; }
        let c = &interface.controls[s.control];
        let picture = prop(c, "$CONTROL_PAR_PICTURE");
        let mentions_pan = |t: &str| t.to_lowercase().split(['_', ' ', '$']).any(|w| w == "pan");
        let number = |name, default| match c.properties.get(name) {
            Some(Value::Int(n)) => f64::from(*n), Some(Value::Real(r)) => *r, _ => default,
        };
        let (lo, hi) = (number("$CONTROL_PAR_MIN_VALUE", 0.), number("$CONTROL_PAR_MAX_VALUE", 1_000_000.));
        let bipolar = picture.to_lowercase().contains("bip") || mentions_pan(prop(c, "$CONTROL_PAR_TEXT"))
            || mentions_pan(&c.variable) || (lo < 0. && lo == -hi);
        let name = panel::guess_name(c, picture, &prefixes, bipolar, true);
        (!said.contains(&name.trim().to_lowercase())).then_some((s.control, name))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictured_value_fields_keep_callback_frames_while_faders_and_knobs_stay_native() {
        // Areia's revealed graph is a 180x83, 128-frame slider edited
        // vertically. Its articulation scrollbar is a 14x220 vertical track.
        let script = r#"on init
make_perfview
set_ui_height_px(400)
declare ui_switch $advanced
declare ui_slider $graph(0,127)
move_control_px($graph,10,100)
set_control_par(get_ui_id($graph),$CONTROL_PAR_HIDE,16)
set_control_par(get_ui_id($graph),$CONTROL_PAR_MOUSE_BEHAVIOUR,-1000)
set_control_par_str(get_ui_id($graph),$CONTROL_PAR_PICTURE,"linear")
declare ui_slider $scroll(0,1000000)
move_control_px($scroll,210,100)
set_control_par(get_ui_id($scroll),$CONTROL_PAR_MOUSE_BEHAVIOUR,-2350)
set_control_par_str(get_ui_id($scroll),$CONTROL_PAR_PICTURE,"track")
declare ui_slider $knob(0,127)
move_control_px($knob,250,100)
set_control_par_str(get_ui_id($knob),$CONTROL_PAR_PICTURE,"round")
declare ui_slider $thin(0,127)
move_control_px($thin,10,250)
set_control_par(get_ui_id($thin),$CONTROL_PAR_MOUSE_BEHAVIOUR,-1000)
set_control_par_str(get_ui_id($thin),$CONTROL_PAR_PICTURE,"thin")
declare ui_slider $compact(0,127)
move_control_px($compact,250,250)
set_control_par(get_ui_id($compact),$CONTROL_PAR_MOUSE_BEHAVIOUR,-755)
set_control_par_str(get_ui_id($compact),$CONTROL_PAR_PICTURE,"compact")
declare ui_menu $shape
add_menu_item($shape,"Linear",0)
add_menu_item($shape,"Shelf",1)
declare ui_label $status(1,1)
set_text($status,"")
end on
on ui_control($advanced)
set_control_par(get_ui_id($graph),$CONTROL_PAR_HIDE,0)
end on
on ui_control($shape)
set_control_par_str(get_ui_id($graph),$CONTROL_PAR_PICTURE,"shelf")
$graph := 0
end on
on ui_control($graph)
set_text($status,"Edited")
end on"#;
        let mut engine = crate::ksp::LogEngine::new(Vec::new(), 48000.);
        let (mut runtime, errors) = crate::ksp::Runtime::with_scripts(&[script], &mut engine, 8, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        let image = |w, h, red| Arc::new(Image::rgba(w, h, [red,0,0,255].repeat(w as usize * h as usize)).unwrap());
        let changed = image(180,83,200);
        let shelf = image(180,83,100);
        let mut frames = vec![image(180,83,20);128];
        frames[96] = changed.clone();
        let pictures: HashMap<_, _> = [
            ("linear".into(), Arc::new(Picture { frames, stretch:[false;2], atlas:None })),
            ("shelf".into(), Arc::new(Picture { frames:vec![shelf.clone();128], stretch:[false;2], atlas:None })),
            ("track".into(), Arc::new(Picture { frames:vec![image(14,220,80);189], stretch:[false;2], atlas:None })),
            ("round".into(), Arc::new(Picture { frames:vec![image(60,60,80);128], stretch:[false;2], atlas:None })),
            ("thin".into(), Arc::new(Picture { frames:vec![image(180,18,80);128], stretch:[false;2], atlas:None })),
            ("compact".into(), Arc::new(Picture { frames:vec![image(75,28,80);128], stretch:[false;2], atlas:None })),
        ].into();
        let shown = |u: &Interface, n| super::super::perf_view::layout(u, &pictures).into_iter().find(|s|s.control==n).unwrap();
        assert!(!super::super::perf_view::layout(&runtime.interface(0), &pictures).iter().any(|s|s.control==1));
        runtime.ui_control(&mut engine,0,0,1);
        let u = runtime.interface(0);
        assert!(!native_control(&shown(&u,1), &u.controls[1]), "the revealed broad graph keeps its picture");
        for n in [2,3,4,5] {
            assert!(native_control(&shown(&u,n), &u.controls[n]), "scrollbar, round knob, skinny and compact cross-axis faders {n} stay native");
        }
        runtime.ui_control(&mut engine,0,1,96);
        let u = runtime.interface(0);
        assert_eq!(prop(&u.controls[7], "$CONTROL_PAR_TEXT"), "Edited", "the authored graph callback still runs");
        assert!(Arc::ptr_eq(&super::super::perf_view::frame_of(&shown(&u,1), &u.controls[1]).unwrap(), &changed));
        runtime.ui_control(&mut engine,0,6,1);
        let u = runtime.interface(0);
        assert!(!native_control(&shown(&u,1), &u.controls[1]));
        assert!(Arc::ptr_eq(&super::super::perf_view::frame_of(&shown(&u,1), &u.controls[1]).unwrap(), &shelf), "the menu callback switches the retained graph artwork");
    }

    #[test]
    fn native_slider_names_update_and_original_controls_keep_their_text() {
        let mut u = crate::ksp::initialize("on init\nmake_perfview\nset_ui_height_px(140)\ndeclare ui_switch $tab\nset_text($tab, \"Workbench\")\nmove_control_px($tab, 200, 20)\ndeclare ui_slider $tone(0,100)\nset_text($tone, \"Cutoff\")\nmove_control_px($tone, 30, 30)\nset_control_par(get_ui_id($tone), $CONTROL_PAR_WIDTH, 50)\nset_control_par(get_ui_id($tone), $CONTROL_PAR_HEIGHT, 50)\ndeclare ui_label $label(1,1)\nset_text($label, \"Authored caption\")\nmove_control_px($label, 200, 100)\nend on", 0, 8).unwrap();
        let pictures = HashMap::new();
        let read = |u: &Interface| {
            let drawn = super::super::perf_view::layout(u, &pictures).into_iter().map(|s| (s,None)).collect::<Vec<_>>();
            plan(u, &pictures, &drawn, &mut Assets::default(), |_| None)
        };
        let p = read(&u);
        assert!(p[0].words.is_empty() && p[2].words.is_empty(), "Original switch and label text is not replanned");
        assert_eq!(prop(&u.controls[0], "$CONTROL_PAR_TEXT"), "Workbench");
        assert!(p[1].words.iter().any(|w| w.text == "Cutoff"));
        u.controls[1].properties.insert("$CONTROL_PAR_TEXT".into(), Value::Text("Resonance".into()));
        u.controls[1].properties.insert("$CONTROL_PAR_VALUE".into(), Value::Int(77));
        assert!(read(&u)[1].words.iter().any(|w| w.text == "Resonance"), "live names do not go stale");
        u.controls[2].properties.insert("$CONTROL_PAR_POS_X".into(), Value::Int(30));
        u.controls[2].properties.insert("$CONTROL_PAR_POS_Y".into(), Value::Int(81));
        assert!(read(&u)[1].words.is_empty(), "a nearby authored Original label still suppresses a duplicate name");
        assert_eq!(prop(&u.controls[2], "$CONTROL_PAR_TEXT"), "Authored caption");
        let mut gain = u.controls[1].clone();
        gain.kind = "ui_knob".into();
        gain.variable = "$gain".into();
        gain.properties.insert("$CONTROL_PAR_POS_X".into(), Value::Int(400));
        gain.properties.insert("$CONTROL_PAR_TEXT".into(), Value::Text("Gain".into()));
        gain.properties.insert("$CONTROL_PAR_VALUE".into(), Value::Int(0));
        u.controls.push(gain);
        let drawn = super::super::perf_view::layout(&u, &pictures).into_iter().map(|s| (s,None)).collect::<Vec<_>>();
        let projected = plan(&u, &pictures, &drawn, &mut Assets::default(), |n| (n == 3).then_some(77.));
        assert!(projected[3].words.iter().any(|w| w.text == "77"), "native value words show the pending value");
        assert_eq!(value(&u.controls[3]), 0., "callback metadata is not modified for drawing");
    }

    #[test]
    fn cached_artwork_classification_preserves_live_plans_and_releases_pixels() {
        let mut u = crate::ksp::initialize("on init\nmake_perfview\nset_ui_height_px(100)\ndeclare ui_switch $s\nset_text($s, \"\")\nset_control_par_str(get_ui_id($s), $CONTROL_PAR_PICTURE, \"clear\")\nend on", 0, 8).unwrap();
        let image = Arc::new(Image::rgba(24, 24, vec![0; 24 * 24 * 4]).unwrap());
        let picture = Arc::new(Picture {frames: vec![image.clone(); 9], stretch: [false; 2], atlas: None});
        let mut pictures: HashMap<_, _> = [("clear".to_owned(), picture)].into();
        let mut assets = Assets::default();
        for value in [0, 1, 0] {
            u.controls[0].properties.insert("$CONTROL_PAR_VALUE".into(), Value::Int(value));
            let drawn: Vec<_> = super::super::perf_view::layout(&u, &pictures).into_iter()
                .map(|s| { let image = super::super::perf_view::frame_of(&s, &u.controls[s.control]); (s,image) }).collect();
            let cached = plan(&u, &pictures, &drawn, &mut assets, |_| None);
            let fresh = plan(&u, &pictures, &drawn, &mut Assets::default(), |_| None);
            assert!(cached.iter().zip(&fresh).all(|(a,b)| a.face == b.face && a.words == b.words), "live plans retain their faces and text");
            assert_eq!(assets.sampled, 1, "immutable pixels are sampled once across live value changes");
        }
        let solid = Arc::new(Image::rgba(24, 24, vec![255; 24 * 24 * 4]).unwrap());
        pictures.insert("clear".into(), Arc::new(Picture {frames: vec![solid.clone(); 9], stretch: [false; 2], atlas: None}));
        let drawn: Vec<_> = super::super::perf_view::layout(&u, &pictures).into_iter()
            .map(|s| { let image = super::super::perf_view::frame_of(&s, &u.controls[s.control]); (s,image) }).collect();
        let cached = plan(&u, &pictures, &drawn, &mut assets, |_| None);
        let fresh = plan(&u, &pictures, &drawn, &mut Assets::default(), |_| None);
        assert!(cached.iter().zip(&fresh).all(|(a,b)| a.face == b.face && a.words == b.words), "replaced artwork receives its new classification");
        assert_eq!(assets.sampled, 2);
        assert!(!matches!(cached[0].face, Face::Clear), "opaque replacement is visible");
        drop(drawn);
        drop(pictures);
        assert_eq!(Arc::strong_count(&image), 1, "the cache retains no image pixels");
        assert_eq!(Arc::strong_count(&solid), 1);
        drop(image);
        drop(solid);
        assets.prune();
        assert!(assets.images.is_empty() && assets.pictures.is_empty(), "retired library metadata is reclaimed");
    }

    #[test]
    fn animated_skin_pairs_replace_one_face_and_preserve_other_labels() {
        // Real Main-page geometry: two coincident 180-frame, 732x71
        // animated skin layers behind one transparent 734x73 slider. The
        // second declares 800x67 but its nonstretch picture controls its size.
        let mut u = crate::ksp::initialize("on init\nmake_perfview\nset_ui_height_px(300)\ndeclare ui_label $face(1,1)\nset_text($face,\"\")\nmove_control_px($face,0,87)\nset_control_par_str(get_ui_id($face),$CONTROL_PAR_PICTURE,\"face\")\ndeclare ui_slider $macro(0,1000000)\nmove_control_px($macro,0,86)\nset_control_par_str(get_ui_id($macro),$CONTROL_PAR_PICTURE,\"transparent\")\nend on", 0, 8).unwrap();
        u.width = 800;
        u.controls[1].properties.insert("$CONTROL_PAR_VALUE".into(), Value::Int(568554));
        u.controls[0].properties.insert("$CONTROL_PAR_PICTURE_STATE".into(), Value::Int(101));
        let mut name = u.controls[0].clone();
        name.id += 2;
        name.properties.insert("$CONTROL_PAR_POS_Y".into(), Value::Int(86));
        name.properties.insert("$CONTROL_PAR_WIDTH".into(), Value::Int(800));
        name.properties.insert("$CONTROL_PAR_HEIGHT".into(), Value::Int(67));
        u.controls.push(name);
        let image = |w, h, alpha| Arc::new(Image::rgba(w, h, vec![alpha; w as usize * h as usize * 4]).unwrap());
        let pictures: HashMap<_, _> = [
            ("face".into(), Arc::new(Picture { frames: vec![image(732,71,255);180], stretch: [false;2], atlas: None })),
            ("transparent".into(), Arc::new(Picture { frames: vec![image(734,73,0)], stretch: [false;2], atlas: None })),
        ].into();
        let plans = |u: &Interface| {
            let drawn: Vec<_> = super::super::perf_view::layout(u, &pictures).into_iter()
                .map(|s| {let image = super::super::perf_view::frame_of(&s, &u.controls[s.control]); (s,image)}).collect();
            plan(u, &pictures, &drawn, &mut Assets::default(), |_| None)
        };
        let p = plans(&u);
        assert_eq!(p[0].face, Face::Clear, "paired old thumb disappears");
        assert_eq!(p[1].face, Face::Normal, "the native fader remains");
        assert_eq!(p[2].face, Face::Clear, "the synchronized name-skin layer also disappears");
        // A label ambiguous between two controls must retain its artwork.
        let mut duplicate_control = u.controls[1].clone();
        duplicate_control.id += 3;
        u.controls.push(duplicate_control);
        assert_ne!(plans(&u)[0].face, Face::Clear);
        assert_ne!(plans(&u)[2].face, Face::Clear);
        u.controls.pop();
        u.controls[0].properties.insert("$CONTROL_PAR_TEXT".into(), Value::Text("Macro".into()));
        assert_ne!(plans(&u)[0].face, Face::Clear, "regular text labels remain");
        u.controls[0].properties.insert("$CONTROL_PAR_TEXT".into(), Value::Text(String::new()));
        u.controls[2].properties.insert("$CONTROL_PAR_PICTURE_STATE".into(), Value::Int(100));
        let p = plans(&u);
        assert_ne!(p[0].face, Face::Clear, "unrelated animation phases remain");
        assert_ne!(p[2].face, Face::Clear);
        u.controls[2].properties.insert("$CONTROL_PAR_PICTURE_STATE".into(), Value::Int(101));
        let mut waveform = u.controls[1].clone();
        waveform.id += 2;
        waveform.kind = "ui_waveform".into();
        waveform.properties.remove("$CONTROL_PAR_PICTURE");
        waveform.properties.insert("$CONTROL_PAR_WIDTH".into(), Value::Int(734));
        waveform.properties.insert("$CONTROL_PAR_HEIGHT".into(), Value::Int(73));
        u.controls.push(waveform);
        let p = plans(&u);
        assert_eq!(p[1].face, Face::Marker, "waveform position controls remain markers");
        assert_ne!(p[0].face, Face::Clear, "waveform artwork is never paired away");
    }

    #[test]
    fn menu_selected_picture_skins_use_the_authored_menu_caption() {
        let script = "on init\nmake_perfview\nset_ui_height_px(200)\ndeclare ui_menu $selector\nadd_menu_item($selector,\"Rhythm\",13)\nadd_menu_item($selector,\"Filter\",4)\n$selector := 13\nset_control_par(get_ui_id($selector),$CONTROL_PAR_HIDE,16)\ndeclare ui_label $name(1,1)\nset_text($name,\"\")\nset_control_par_str(get_ui_id($name),$CONTROL_PAR_PICTURE,\"face_\" & $selector)\nset_control_par(get_ui_id($name),$CONTROL_PAR_PICTURE_STATE,101)\ndeclare ui_slider $amount(0,1000000)\n$amount := 568554\nset_control_par_str(get_ui_id($amount),$CONTROL_PAR_PICTURE,\"transparent\")\nend on";
        let mut u = crate::ksp::initialize(script, 0, 8).unwrap();
        u.width = 800;
        assert_eq!(u.controls[1].properties["picture menu"], Value::Int(u.controls[0].id));
        let pictures: HashMap<_, _> = [
            ("face_13".into(), Arc::new(Picture { frames: vec![Arc::new(Image::rgba(732,71,vec![255;732*71*4]).unwrap());180], stretch: [false;2], atlas: None })),
            ("transparent".into(), Arc::new(Picture { frames: vec![Arc::new(Image::rgba(734,73,vec![0;734*73*4]).unwrap())], stretch: [false;2], atlas: None })),
        ].into();
        for (value, caption) in [(13,"Rhythm"),(4,"Filter")] {
            u.controls[0].properties.insert("$CONTROL_PAR_VALUE".into(), Value::Int(value));
            let drawn: Vec<_> = super::super::perf_view::layout(&u, &pictures).into_iter()
                .map(|s| {let image = super::super::perf_view::frame_of(&s, &u.controls[s.control]); (s,image)}).collect();
            let p = plan(&u, &pictures, &drawn, &mut Assets::default(), |_| None);
            assert_eq!(p[0].face, Face::Clear);
            assert!(p[1].skin, "native surface covers the replaced skin");
            assert!(p[1].words.iter().any(|w| w.text == caption), "selected menu text survives bitmap removal");
        }
        let unknown = script.replace("end on", "set_control_par_str(get_ui_id($name),$CONTROL_PAR_PICTURE,\"another\")\nend on");
        let u = crate::ksp::initialize(&unknown, 0, 8).unwrap();
        assert!(!u.controls[1].properties.contains_key("picture menu"), "an ambiguous direct assignment has no inferred source");
    }

    #[test]
    fn pictured_waveform_layers_do_not_become_slider_skins() {
        // Actual source display: 180-frame, horizontally cropped 258x63
        // pictures at source index32, overlaid by a 260x58 loop-start slider0.
        let mut u = crate::ksp::initialize("on init\nmake_perfview\nset_ui_height_px(200)\ndeclare ui_label $dark(1,1)\nset_text($dark,\"\")\nset_control_par_str(get_ui_id($dark),$CONTROL_PAR_PICTURE,\"wave\")\nset_control_par(get_ui_id($dark),$CONTROL_PAR_WIDTH,258)\nset_control_par(get_ui_id($dark),$CONTROL_PAR_PICTURE_STATE,32)\ndeclare ui_label $bright(1,1)\nset_text($bright,\"\")\nset_control_par_str(get_ui_id($bright),$CONTROL_PAR_PICTURE,\"wave\")\nset_control_par(get_ui_id($bright),$CONTROL_PAR_WIDTH,257)\nset_control_par(get_ui_id($bright),$CONTROL_PAR_PICTURE_STATE,32)\ndeclare ui_slider $start(0,1000000)\nset_control_par_str(get_ui_id($start),$CONTROL_PAR_PICTURE,\"transparent\")\nend on", 0, 8).unwrap();
        let pictures: HashMap<_, _> = [
            ("wave".into(), Arc::new(Picture { frames: vec![Arc::new(Image::rgba(258,63,vec![255;258*63*4]).unwrap());180], stretch: [true,false], atlas: None })),
            ("transparent".into(), Arc::new(Picture { frames: vec![Arc::new(Image::rgba(260,58,vec![0;260*58*4]).unwrap())], stretch: [false;2], atlas: None })),
        ].into();
        for source in [32, 0] {
            for label in &mut u.controls[..2] { label.properties.insert("$CONTROL_PAR_PICTURE_STATE".into(), Value::Int(source)); }
            let drawn: Vec<_> = super::super::perf_view::layout(&u, &pictures).into_iter()
                .map(|s| {let image = super::super::perf_view::frame_of(&s, &u.controls[s.control]); (s,image)}).collect();
            let p = plan(&u, &pictures, &drawn, &mut Assets::default(), |_| None);
            assert_ne!(p[0].face, Face::Clear);
            assert_ne!(p[1].face, Face::Clear);
            assert!(!p[2].skin, "source0 coinciding with slider0 is still a resizable display, not its face");
        }
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
