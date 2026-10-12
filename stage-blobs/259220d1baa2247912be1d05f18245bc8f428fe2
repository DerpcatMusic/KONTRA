//! Text fields: one line or many, a selection, a blinking caret, and the
//! edits the focused keys imply.
use std::ops::Range;

use mui_input::Key;
use mui_scene::prelude::*;

use super::grapheme;
use crate::Ui;
use crate::widgets::Response;

/// What Enter does in a [`text_edit`] field, and so whether it is one line
/// or many.
///
/// ```
/// use mui::prelude::*;
/// assert_eq!(TextOpts::default().newline, Newline::None);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Newline {
    /// One line: Enter submits and nothing makes a newline.
    #[default]
    None,
    /// Many lines: Enter (or Shift+Enter) is a newline, ctrl/cmd+Enter
    /// submits. A description box.
    Enter,
    /// Many lines: Shift+Enter is a newline, Enter submits. A chat box.
    ShiftEnter,
}

/// How a [`text_edit`] field behaves. The default is [`text_input`]'s:
/// one line that keeps the focus when Enter submits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextOpts {
    pub newline: Newline,
    /// Rows a multi-line field is tall; past them it scrolls, by the wheel
    /// and to follow the caret. A later `.h(..)` wins: the field measures
    /// the height it was given.
    pub rows: usize,
    /// Let go of the keyboard focus when the field submits, as a rename box
    /// does.
    pub blur_on_submit: bool,
}
impl Default for TextOpts {
    fn default() -> Self {
        Self {
            newline: Newline::None,
            rows: 4,
            blur_on_submit: false,
        }
    }
}

/// What a [`text_edit`] field did last frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextEdit {
    /// The value is not what it was.
    pub changed: bool,
    /// The submit key arrived: Enter on one line, and on many whichever of
    /// Enter and ctrl/cmd+Enter [`Newline`] left over.
    pub submitted: bool,
}

/// The text starts this far in from the field's edges.
const PAD: f64 = 8.0;
const PAD_Y: f64 = 6.0;

/// A caret's x from [`Ui::carets`]; `0` for a byte that is not a boundary.
fn caret_at(carets: &[(usize, f64)], byte: usize) -> f64 {
    carets
        .binary_search_by_key(&byte, |&(b, _)| b)
        .map_or(0.0, |i| carets[i].1)
}

fn byte(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map_or(s.len(), |(b, _)| b)
}
fn chars(s: &str, byte: usize) -> usize {
    s[..byte].chars().count()
}

/// The selection as a string, and the two helpers that edit it. Indices are
/// characters; `byte` turns them into slice offsets.
fn selected(value: &str, a: usize, c: usize) -> String {
    value[byte(value, a.min(c))..byte(value, a.max(c))].to_owned()
}
/// Remove the selection: the caret afterwards, and whether there was one.
fn take(value: &mut String, a: usize, c: usize) -> (usize, bool) {
    let (lo, hi) = (a.min(c), a.max(c));
    if lo == hi {
        return (c, false);
    }
    let (x, y) = (byte(value, lo), byte(value, hi));
    value.replace_range(x..y, "");
    (lo, true)
}
fn insert(value: &mut String, caret: &mut usize, c: char) {
    value.insert(byte(value, *caret), c);
    *caret += 1;
}

/// The row a caret at byte `b` sits on: the last line starting at or
/// before it, so a caret at a soft break starts the next line.
fn row_of(lines: &[Range<usize>], b: usize) -> usize {
    lines.iter().rposition(|l| l.start <= b).unwrap_or(0)
}

/// Where a multi-line field's text is laid out: its lines, as byte ranges,
/// against last frame's inner width.
struct Rows {
    size: f64,
    width: f64,
}
impl Rows {
    fn lines(&self, ui: &Ui, s: &str) -> Vec<Range<usize>> {
        ui.lines(s, self.size, self.width)
    }
    /// The caret `rows` lines above (negative) or below `caret`, at the
    /// same x; past either end it lands on that end.
    fn vertical(&self, ui: &Ui, value: &str, caret: usize, rows: isize) -> usize {
        let lines = self.lines(ui, value);
        let b = byte(value, caret);
        let row = row_of(&lines, b);
        let l = &lines[row];
        let x = caret_at(&ui.carets(&value[l.clone()], self.size), b - l.start);
        match usize::try_from(row as isize + rows) {
            Err(_) => 0,
            Ok(t) if t >= lines.len() => value.chars().count(),
            Ok(t) => {
                let l = &lines[t];
                chars(value, l.start) + ui.hit(&value[l.clone()], self.size, x)
            }
        }
    }
    /// The start or the end of the caret's own line. A soft-wrapped line
    /// ends before the spaces it broke at, or End would land on the next.
    fn home_end(&self, ui: &Ui, value: &str, caret: usize, end: bool) -> usize {
        let lines = self.lines(ui, value);
        let row = row_of(&lines, byte(value, caret));
        let l = &lines[row];
        let at = if !end {
            l.start
        } else if lines.get(row + 1).is_some_and(|n| n.start == l.end) {
            l.start + value[l.clone()].trim_end().len()
        } else {
            l.end
        };
        chars(value, at)
    }
}

/// Apply this frame's typed text and focused keys to `value`. Returns
/// whether the submit key arrived.
fn edit_keys(
    ui: &mut Ui,
    id: &str,
    value: &mut String,
    (anchor, caret): (&mut usize, &mut usize),
    newline: Newline,
    (rows, geometry): (Option<(&Rows, usize)>, Option<&mui_scene::TextGeometry>),
    (goal, upstream): (&mut Option<f64>, &mut bool),
) -> bool {
    let multi = rows.is_some();
    let mut submitted = false;
    let committed = ui.text(id).to_owned();
    let has_committed_text = !committed.is_empty();
    let native = ui.take_native_text(id);
    let native_bytes: usize = native
        .iter()
        .filter_map(|e| match e {
            mui_input::Ime::Commit(s) => Some(s.len()),
            _ => None,
        })
        .sum();
    let typed = committed.len().saturating_sub(native_bytes);
    for c in committed[..typed].chars().filter(|c| !c.is_control()) {
        (*caret, _) = take(value, *anchor, *caret);
        insert(value, caret, c);
        *anchor = *caret;
    }
    let mut committed_native = false;
    for operation in native {
        match operation {
            mui_input::Ime::Selection(range) => {
                let (mut start, mut end) = (range.start, range.end);
                // Before the first commit, native offsets describe the marked
                // display string; later offsets describe the edited buffer.
                if !committed_native && let Some(g) = geometry {
                    if range.end > g.text.len()
                        || !g.text.is_char_boundary(range.start)
                        || !g.text.is_char_boundary(range.end)
                    {
                        continue;
                    }
                    start = g.display_to_source(range.start);
                    end = g.display_to_source(range.end);
                    if range.start < g.state.marked.end
                        && range.end > g.state.marked.start
                        && let Some(replacement) = &g.state.replacement
                    {
                        start = start.min(replacement.start);
                        end = end.max(replacement.end);
                    }
                }
                if start > end
                    || end > value.len()
                    || !value.is_char_boundary(start)
                    || !value.is_char_boundary(end)
                {
                    continue;
                }
                *anchor = grapheme::floor(value, chars(value, start));
                *caret = grapheme::floor(value, chars(value, end));
            }
            mui_input::Ime::Commit(s) => {
                (*caret, _) = take(value, *anchor, *caret);
                for c in s
                    .chars()
                    .filter(|c| !c.is_control() || (multi && *c == '\n'))
                {
                    insert(value, caret, c);
                }
                *anchor = *caret;
                committed_native = true;
            }
            _ => {}
        }
    }
    for k in ui.keys(id).to_vec() {
        let cmd = k.mods.ctrl || k.mods.cmd;
        // Where the caret goes, for the keys that only move it.
        let mut moved: Option<usize> = None;
        match k.key {
            Key::Char(c) if cmd => match c.to_ascii_lowercase() {
                'a' => (*anchor, *caret) = (0, value.chars().count()),
                // An empty selection copies nothing: handing the host
                // "" would wipe whatever is already on the clipboard.
                'c' if anchor != caret => {
                    ui.set_clipboard(selected(value, *anchor, *caret));
                }
                'x' if anchor != caret => {
                    ui.set_clipboard(selected(value, *anchor, *caret));
                    (*caret, _) = take(value, *anchor, *caret);
                    *anchor = *caret;
                }
                'v' => {
                    if let Some(s) = ui.pasted().map(str::to_owned) {
                        (*caret, _) = take(value, *anchor, *caret);
                        let s = s.replace("\r\n", "\n").replace('\r', "\n");
                        for c in s
                            .chars()
                            .filter(|&c| !c.is_control() || (multi && c == '\n'))
                        {
                            insert(value, caret, c);
                        }
                        *anchor = *caret;
                    }
                }
                _ => {}
            },
            Key::Char(c) if !c.is_control() && !has_committed_text => {
                (*caret, _) = take(value, *anchor, *caret);
                insert(value, caret, c);
                *anchor = *caret;
            }
            Key::Enter => {
                let breaks = match newline {
                    Newline::None => false,
                    Newline::Enter => !cmd,
                    Newline::ShiftEnter => k.mods.shift,
                };
                if breaks {
                    (*caret, _) = take(value, *anchor, *caret);
                    insert(value, caret, '\n');
                    *anchor = *caret;
                } else {
                    submitted = true;
                }
            }
            Key::Backspace => {
                let (at, had) = take(value, *anchor, *caret);
                *caret = at;
                if !had && *caret > 0 {
                    let start = grapheme::previous(value, *caret);
                    value.replace_range(byte(value, start)..byte(value, *caret), "");
                    *caret = start;
                }
                *anchor = *caret;
            }
            Key::Delete => {
                let (at, had) = take(value, *anchor, *caret);
                *caret = at;
                if !had && *caret < value.chars().count() {
                    let end = grapheme::next(value, *caret);
                    value.replace_range(byte(value, *caret)..byte(value, end), "");
                }
                *anchor = *caret;
            }
            Key::Left | Key::Right if !k.mods.shift && anchor != caret => {
                let right = k.key == Key::Right;
                moved = Some(geometry.map_or_else(
                    || {
                        if right {
                            (*anchor).max(*caret)
                        } else {
                            (*anchor).min(*caret)
                        }
                    },
                    |g| {
                        chars(
                            value,
                            g.selection_edge(byte(value, *anchor), byte(value, *caret), right),
                        )
                    },
                ));
            }
            Key::Left | Key::Right => {
                moved = Some(geometry.map_or_else(
                    || {
                        if k.key == Key::Right {
                            grapheme::next(value, *caret)
                        } else {
                            grapheme::previous(value, *caret)
                        }
                    },
                    |g| {
                        chars(
                            value,
                            g.visual_move(byte(value, *caret), k.key == Key::Right),
                        )
                    },
                ));
            }
            Key::Home | Key::End => {
                let end = k.key == Key::End;
                *upstream = end && !cmd;
                moved = Some(match rows {
                    Some((r, _)) if !cmd => geometry.map_or_else(
                        || r.home_end(ui, value, *caret, end),
                        |g| {
                            let line = &g.lines[g.row(g.source_to_display(byte(value, *caret)))];
                            let stop = if end {
                                line.carets.stops.last()
                            } else {
                                line.carets.stops.first()
                            };
                            chars(
                                value,
                                g.display_to_source(line.range.start + stop.map_or(0, |p| p.0)),
                            )
                        },
                    ),
                    _ if end => value.chars().count(),
                    _ => 0,
                });
            }
            Key::Up | Key::Down | Key::PageUp | Key::PageDown => {
                if let Some((r, page)) = rows {
                    let step = match k.key {
                        Key::Up => -1,
                        Key::Down => 1,
                        Key::PageUp => -(page as isize),
                        _ => page as isize,
                    };
                    moved = Some(match geometry {
                        Some(g) => {
                            let (b, x, affinity) =
                                g.vertical_move(byte(value, *caret), step, *goal);
                            *goal = Some(x);
                            *upstream = affinity;
                            chars(value, b)
                        }
                        None => r.vertical(ui, value, *caret, step),
                    });
                }
            }
            _ => {}
        }
        if !matches!(k.key, Key::Up | Key::Down | Key::PageUp | Key::PageDown) {
            *goal = None;
            if !matches!(k.key, Key::Home | Key::End) {
                *upstream = false;
            }
        }
        if let Some(to) = moved {
            *caret = to;
            if !k.mods.shift {
                *anchor = *caret;
            }
        }
    }
    submitted
}

/// A single-line field: the text, a selection, a blinking caret, and the
/// edits the focused keys imply. Click to focus and set the caret, drag to
/// select, double click for a word, shift+arrows to extend. ctrl/cmd+A, C, X
/// and V select all, copy, cut and paste -- a copy reaches the host's
/// [`Clipboard`](crate::Clipboard), or `Frame::clipboard` for a host
/// without one, and a paste reads it back. Returns the field and whether
/// the value changed. [`text_edit`] is the same field with more to say:
/// many lines, and whether Enter submitted.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut name = String::from("Init");
/// let field = text_input(&mut ui, "name", &mut name);
/// assert!(!field.changed, "nothing is focused, so nothing was typed");
/// let field = field.el.w(140);
/// ```
pub fn text_input(ui: &mut Ui, id: impl Into<Id>, value: &mut String) -> Response {
    let r = text_edit(ui, id, value, TextOpts::default());
    Response {
        el: r.el,
        changed: r.changed.changed,
    }
}

/// A text field, one line or many as [`TextOpts::newline`] says. Many lines
/// wrap to the field's width, Up and Down move between them at the caret's
/// x, Home and End go to the ends of a line (with ctrl/cmd, of the text),
/// and the lines scroll inside the field -- by the wheel, and to keep the
/// caret in view. Selected text is re-inked on the selection's fill.
/// Returns the field and what it did last frame.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut notes = String::from("first\nsecond");
/// let opts = TextOpts { newline: Newline::Enter, rows: 6, ..TextOpts::default() };
/// let field = text_edit(&mut ui, "notes", &mut notes, opts);
/// assert!(!field.changed.changed && !field.changed.submitted, "nothing is focused");
/// ```
pub fn text_edit(
    ui: &mut Ui,
    id: impl Into<Id>,
    value: &mut String,
    opts: TextOpts,
) -> Response<TextEdit> {
    let id: Id = id.into();
    let id = id.as_str();
    let multi = opts.newline != Newline::None;
    let size = ui.theme().text;
    let lh = ui.line_height(size);
    let focused = ui.focused(id);
    let n = value.chars().count();
    let (anchor, caret) = ui.sel(id);
    let (mut anchor, mut caret) = (
        grapheme::floor(value, anchor.min(n)),
        grapheme::floor(value, caret.min(n)),
    );
    // Input is hit against the exact text the user saw, including preedit,
    // scroll and bidi order. Painting is resolved later against current bounds.
    let geometry = ui
        .scene()
        .and_then(|s| s.surface(id))
        .and_then(|s| s.text_geometry.clone())
        .filter(|g| &*g.state.value == value.as_str());
    let room = ui
        .scene()
        .and_then(|s| s.surface(id))
        .map(|s| (s.frame.size.width - 2. * PAD).max(0.01));
    let rows = multi.then(|| Rows {
        size,
        width: room.unwrap_or(1e9),
    });
    let view = ui
        .scene()
        .and_then(|s| s.surface(id))
        .map_or(opts.rows.max(1) as f64 * lh, |s| {
            (s.frame.size.height - 2. * PAD_Y).max(lh)
        });
    let scroll = geometry
        .as_ref()
        .map_or_else(|| ui.text_scroll(id), |g| g.scroll);
    let r = ui.get(id);
    let (mut goal, mut upstream) = ui.text_navigation(id);
    // A marked composition owns its cursor and pending source replacement.
    // Native replacement selections arrive as ordered Ime::Selection events;
    // raw pointer events must not destroy that replacement before a commit.
    if (r.pressed || r.dragged)
        && ui.preedit().is_none()
        && let Some(p) = ui.local(id)
    {
        let (at, affinity) = geometry
            .as_ref()
            .map_or((0, false), |g| g.hit_with_affinity(p));
        caret = grapheme::floor(value, chars(value, at.min(value.len())));
        goal = None;
        upstream = affinity;
        if r.pressed {
            anchor = caret;
        }
    }
    if r.double_clicked && ui.preedit().is_none() {
        (anchor, caret) = grapheme::word(value, caret);
    }

    // Only a frame with input can edit, so only that frame pays for the copy
    // the change is judged against.
    let typing = focused && !(ui.text(id).is_empty() && ui.keys(id).is_empty());
    let before = typing.then(|| value.clone());
    let page = ((view / lh).floor() as usize).max(1);
    let geometry = geometry.filter(|g| &*g.state.value == value.as_str());
    let submitted = focused
        && edit_keys(
            ui,
            id,
            value,
            (&mut anchor, &mut caret),
            opts.newline,
            (rows.as_ref().map(|r| (r, page)), geometry.as_deref()),
            (&mut goal, &mut upstream),
        );
    ui.set_sel(id, anchor, caret);
    ui.set_text_navigation(id, goal, upstream);
    if submitted && opts.blur_on_submit {
        ui.blur();
    }
    let edit = TextEdit {
        changed: before.is_some_and(|b| b != *value),
        submitted,
    };

    let source_selection = byte(value, anchor.min(caret))..byte(value, anchor.max(caret));
    let pre = focused
        .then(|| ui.preedit())
        .flatten()
        .map(|(t, c)| (t.to_owned(), c));
    let mut shown = value.clone();
    let (replacement, marked, at, sel) = match pre {
        Some((t, c)) => {
            let base = source_selection.start;
            shown.replace_range(source_selection.clone(), &t);
            // Platform ranges are untrusted byte offsets. Snap them backwards
            // to a valid grapheme boundary before measuring or slicing.
            let snap = |b: usize| {
                use unicode_segmentation::UnicodeSegmentation;
                t.grapheme_indices(true)
                    .map(|(i, _)| i)
                    .chain([t.len()])
                    .take_while(|i| *i <= b.min(t.len()))
                    .last()
                    .unwrap_or(0)
            };
            let (a, b) = c.map_or((t.len(), t.len()), |(a, b)| (snap(a), snap(b)));
            (
                Some(source_selection.clone()),
                base..base + t.len(),
                base + b,
                base + a.min(b)..base + a.max(b),
            )
        }
        None => (None, 0..0, byte(value, caret), source_selection.clone()),
    };
    let mut el = text(shown)
        .h(if multi {
            opts.rows.max(1) as f64 * lh + 2. * PAD_Y
        } else {
            lh + 2. * PAD_Y
        })
        .clip()
        .pad((PAD, PAD_Y))
        .radius(6.)
        .fill(Role::Field)
        .on(State::Focus, |s| s.stroke(Role::Primary))
        .cursor(Cursor::Text)
        .focusable()
        .when(multi, mui_scene::Styled::captures_wheel)
        .a11y(A11y::TextInput {
            value: value.as_str().into(),
            selection: (anchor, caret),
            carets: Vec::new(),
        })
        .id(id);
    let follow_caret = typing
        || r.pressed
        || r.dragged
        || geometry
            .as_ref()
            .is_none_or(|g| g.state.caret != at || g.state.marked != marked);
    el.payload_mut().extras_mut().editable_text = Some(mui_scene::EditableText {
        value: value.as_str().into(),
        selection: sel,
        caret: at,
        caret_upstream: upstream,
        marked,
        replacement,
        multiline: multi,
        caret_visible: focused
            && ui.blink()
            && ui.preedit().is_none_or(|(_, cursor)| cursor.is_some()),
        scroll: if multi { scroll + r.wheel.y } else { 0. },
        follow_caret,
        insets: [PAD, PAD_Y],
        previous_viewport: geometry.as_ref().map(|g| g.viewport),
    });
    if multi {
        ui.set_text_scroll(id, scroll);
    }
    if focused {
        ui.set_ime_caret(id, mui_geometry::Point::new(PAD, PAD_Y), lh);
    }
    Response { el, changed: edit }
}
