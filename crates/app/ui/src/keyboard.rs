//! The keybed along the editor's foot: play it with the pointer (a drag is
//! a glissando), see which keys each part plays and which are held.

use mui::prelude::*;

use crate::kit::edges;
use crate::theme::{SMALL, TEXT, TIGHT, accent};

/// The height of one part's band above the keys.
const BAND: f64 = 3.;

/// What the keybed shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keys {
    /// The first and last note shown; the first should be a C.
    pub range: (u8, u8),
    /// Notes sounding now.
    pub held: Vec<u8>,
    /// Each part's key span and colour: a light tint over the keys it plays.
    pub spans: Vec<(u8, u8, Color)>,
    /// The focused part's keyswitches, each in its articulation's colour.
    pub switches: Vec<(u8, Color)>,
}

const BLACK: [bool; 12] = [
    false, true, false, true, false, false, true, false, true, false, true, false,
];

fn black(note: u8) -> bool {
    BLACK.get(usize::from(note % 12)).copied().unwrap_or(false)
}

/// `note`'s name with middle C as C3, as samplers name it.
#[must_use]
pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let name = NAMES.get(usize::from(note % 12)).copied().unwrap_or("?");
    format!("{name}{}", i32::from(note / 12) - 2)
}

/// Where each key of `range` sits in a keybed `w` wide: (note, x, width).
fn layout(range: (u8, u8), w: f64) -> Vec<(u8, f64, f64)> {
    let whites = (range.0..=range.1).filter(|&n| !black(n)).count().max(1);
    let kw = w / whites as f64;
    let mut x = 0.;
    (range.0..=range.1)
        .map(|n| {
            if black(n) {
                (n, x - kw * 0.3, kw * 0.6)
            } else {
                x += kw;
                (n, x - kw, kw)
            }
        })
        .collect()
}

/// The note under `p` in a keybed of `size`: black keys win their upper
/// part.
fn hit(range: (u8, u8), size: Size, p: Point) -> Option<u8> {
    let keys = layout(range, size.width);
    let on = |&&(_, x, kw): &&(u8, f64, f64)| p.x >= x && p.x < x + kw;
    let upper = p.y < size.height * 0.62;
    keys.iter()
        .filter(|(n, ..)| black(*n))
        .find(|k| upper && on(k))
        .or_else(|| keys.iter().filter(|(n, ..)| !black(*n)).find(on))
        .map(|k| k.0)
}

/// The keybed `height` tall. `playing` is the note the pointer holds; the
/// response carries the note ons (true) and offs this frame.
pub fn keyboard(
    ui: &mut Ui,
    id: &str,
    keys: &Keys,
    playing: &mut Option<u8>,
    height: f64,
) -> Response<Vec<(u8, bool)>> {
    let r = ui.get(id);
    let size = ui.scene().and_then(|s| s.surface(id)).map(|s| s.frame.size);
    let under = ui
        .local(id)
        .zip(size)
        .and_then(|(p, s)| hit(keys.range, s, p));
    let mut events = Vec::new();
    let want = if r.held || r.pressed { under } else { None };
    if want != *playing {
        events.extend(playing.map(|n| (n, false)));
        events.extend(want.map(|n| (n, true)));
        *playing = want;
    }
    let octaves: Vec<El> = (keys.range.0..=keys.range.1)
        .step_by(12)
        .map(|n| {
            col![
                spacer(),
                caption(note_name(n))
                    .text_size(SMALL - 2.)
                    .fill(Color::oklch(0.35, 0., 0.))
            ]
            .align(Align::Start)
            .pad(edges(0., 0., TIGHT, TIGHT))
            .grow(1)
            .w(0.)
        })
        .collect();
    let keys = keys.clone();
    let face = canvas(move |s| {
        let rect = |x: f64, y: f64, w: f64, h: f64| {
            Path::polyline(
                [(x, y), (x + w, y), (x + w, y + h), (x, y + h)].map(|(x, y)| Point::new(x, y)),
                true,
            )
        };
        let all = layout(keys.range, s.width);
        let held = |n: u8| keys.held.contains(&n);
        // A keyswitch takes its articulation's colour; a key a part plays
        // is tinted lightly, the first part listed winning.
        let switch = |n: u8| keys.switches.iter().find(|s| s.0 == n).map(|s| s.1);
        let tint = |n: u8| {
            keys.spans
                .iter()
                .find(|&&(lo, hi, _)| (lo..=hi).contains(&n))
                .map(|s| s.2)
        };
        // Above the keys, stacked: the keyswitches, then each part's span.
        let switches = (!keys.switches.is_empty()).then_some((0u8, 0u8, accent()));
        let rows: Vec<(u8, u8, Color)> = switches
            .into_iter()
            .chain(keys.spans.iter().copied())
            .take(4)
            .collect();
        let band = rows.len() as f64 * BAND;
        let top = if band > 0. { band + 2. } else { 0. };
        let mut draw = Vec::new();
        if !keys.switches.is_empty() {
            for &(n, x, w) in &all {
                if let Some(c) = switch(n) {
                    draw.push(Draw::fill(rect(x, 0., w, BAND - 1.), Fill::from(c)));
                }
            }
        }
        let skip = usize::from(!keys.switches.is_empty());
        for (row, &(lo, hi, color)) in rows.iter().enumerate().skip(skip) {
            let x0 = all.iter().find(|k| k.0 >= lo).map(|k| k.1);
            let x1 = all.iter().rev().find(|k| k.0 <= hi).map(|k| k.1 + k.2);
            if let (Some(x0), Some(x1)) = (x0, x1) {
                draw.push(Draw::fill(
                    rect(x0, row as f64 * BAND, x1 - x0, BAND - 1.),
                    Fill::from(color),
                ));
            }
        }
        let h = s.height - top;
        for &(n, x, w) in all.iter().filter(|k| !black(k.0)) {
            let grey = Color::oklch(0.64, 0., 0.);
            let face = match (held(n), switch(n), tint(n)) {
                (true, ..) => accent(),
                (_, Some(c), _) => c.with_lightness(0.9).scale_chroma(0.5),
                (.., Some(c)) => c.with_lightness(0.68).scale_chroma(0.25),
                _ => grey,
            };
            draw.push(Draw::fill(rect(x + 0.5, top, w - 1., h), Fill::from(face)));
        }
        for &(n, x, w) in all.iter().filter(|k| black(k.0)) {
            let grey = Color::oklch(0.2, 0., 0.);
            let face = match (held(n), switch(n), tint(n)) {
                (true, ..) => Color::oklch(0.6, 0., 0.),
                (_, Some(c), _) => c.with_lightness(0.32).scale_chroma(0.7),
                (.., Some(c)) => c.with_lightness(0.22).scale_chroma(0.25),
                _ => grey,
            };
            draw.push(Draw::fill(rect(x, top, w, h * 0.62), Fill::from(face)));
        }
        draw
    })
    .w(Len::Pct(100.))
    .h(height);
    // It takes what its row leaves, beside the wheels, never more.
    let el = stack![face, row(octaves).w(Len::Pct(100.)).h(height)]
        .grow(1)
        .w(0.)
        .h(height)
        .min_w(TEXT * 20.)
        .cursor(Cursor::Hand)
        .tracks_pointer()
        .named("Keyboard")
        .id(id.to_owned());
    Response {
        el,
        changed: events,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_named_with_middle_c_as_c3_and_hit_where_drawn() {
        assert_eq!(note_name(60), "C3");
        assert_eq!(note_name(0), "C-2");
        assert_eq!(note_name(61), "C#3");
        let (range, size) = ((48, 59), Size::new(70., 50.));
        // Seven white keys 10 wide: the second's lower half is D, its seam's top C#.
        assert_eq!(hit(range, size, Point::new(15., 45.)), Some(50));
        assert_eq!(hit(range, size, Point::new(10., 10.)), Some(49));
        assert_eq!(hit(range, size, Point::new(69., 45.)), Some(59));
        assert_eq!(hit(range, size, Point::new(71., 45.)), None);
    }
}
