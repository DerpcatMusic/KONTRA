//! A part's script state for the project: the persistent variables of its
//! KSP scripts (the controls' values among them), read from the live engine
//! and written in Kontakt's saved form (`"<name> <value…>"`), so a reload
//! restores them the way a library's own saved values are restored: after
//! `on init`, before `on persistence_changed`.
//!
//! The engine lives on the audio thread, so the rack sends a [`Capture`]
//! there; the audio side fills its values in place and sends it back. Text
//! variables come back the same way, as fixed-size [`Text`].
//!
//! A Lua script's persistent widgets are saved the same way, in UVI's form
//! (`"<name>=<value>"`); its load restores them before `onLoad`.

use std::collections::HashMap;

use kontra_engine::{Engine, Text};

use crate::ui::lua::Page as _;
use kontra_ksp::{Cells, FIRST_UI_ID, Persistence, Script};

/// One script slot's persistent number variables, and the instrument script
/// it was compiled from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vars {
    pub script: usize,
    pub vars: Vec<Var>,
    /// A Lua script's persistent widgets: name and number.
    pub widgets: Vec<(Box<str>, u32)>,
    /// A Lua script: what its `onSave` returns is saved too.
    pub state: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Var {
    pub name: Box<str>,
    pub real: bool,
    /// A text variable (`@`, `!`): its cells are text cells.
    pub text: bool,
    pub base: u32,
    pub len: u32,
    /// Its UI id when it is a control's value.
    pub control: Option<i32>,
    /// Saved with the instrument only (`make_instr_persistent`): a
    /// snapshot neither saves nor changes it.
    pub instrument: bool,
}

impl Vars {
    /// The persistent variables of `compiled`, instrument script `script`.
    #[must_use]
    pub fn of(script: usize, compiled: &Script) -> Self {
        let control = |n: usize| {
            let n = u32::try_from(n).ok()?;
            compiled.controls.iter().find(|c| c.variable == n)?;
            Some(FIRST_UI_ID + i32::try_from(n).ok()?)
        };
        let vars = (compiled.variables.iter().enumerate())
            .filter(|(_, v)| v.persistence != Persistence::None)
            .filter_map(|(n, v)| {
                let (real, text, base, len) = match v.cells {
                    Cells::Int { base, len } => (false, false, base, len),
                    Cells::Real { base, len } => (true, false, base, len),
                    Cells::Text { base, len } => (false, true, base, len),
                    _ => return None,
                };
                Some(Var {
                    name: v.name.clone(),
                    real,
                    text,
                    base,
                    len,
                    control: control(n),
                    instrument: v.persistence == Persistence::Instrument,
                })
            })
            .collect();
        Self {
            script,
            vars,
            widgets: Vec::new(),
            state: false,
        }
    }

    /// The persistent widgets with a value of the page a Lua script built,
    /// instrument script `script`.
    #[must_use]
    pub fn lua(script: usize, page: &kontra_lua::Ui) -> Self {
        let valued = [
            "Knob",
            "Slider",
            "NumBox",
            "Menu",
            "OnOffButton",
            "MultiStateButton",
        ];
        let widgets = (0u32..)
            .zip(&page.widgets)
            .filter(|(_, w)| w.persistent && !w.name.is_empty() && valued.contains(&&*w.kind))
            .map(|(n, w)| (w.name.as_str().into(), n))
            .collect();
        Self {
            script,
            vars: Vec::new(),
            widgets,
            state: true,
        }
    }

    /// What `saved` (in Kontakt's form) writes into these variables' cells,
    /// for a recall into the running engine. Entries naming no persistent
    /// variable write nothing.
    #[must_use]
    pub fn recall(&self, saved: &[String]) -> kontra_engine::Saved {
        let restore = kontra_ksp::restore(saved, |key| {
            let var = (self.vars.iter())
                .find(|v| v.name.trim_start_matches(SIGILS).eq_ignore_ascii_case(key))?;
            let (base, len) = (var.base, var.len);
            Some(match (var.text, var.real) {
                (true, _) => Cells::Text { base, len },
                (false, true) => Cells::Real { base, len },
                (false, false) => Cells::Int { base, len },
            })
        });
        kontra_engine::Saved {
            ints: restore.ints,
            texts: (restore.texts.iter())
                .map(|(cell, text)| (*cell, Text::from_str_cut(text).0))
                .collect(),
        }
    }
}

/// The type marks a KSP variable name starts with.
const SIGILS: [char; 6] = ['$', '%', '~', '?', '@', '!'];

/// Cells for the audio side to read out of one part's engine.
///
/// The read is bounded: only the script slots that ran or were written since
/// the last capture are read, and the audio side reads at most a budget of
/// cells a block ([`Capture::read`]), going on in the next block. A save is
/// as old as the read took (a million cells: a few blocks).
pub struct Capture {
    pub(crate) part: usize,
    pub(crate) generation: u64,
    /// Persistent number variables, back to back in `values`.
    spans: Box<[Span]>,
    values: Box<[i64]>,
    /// Control value cells, read again in a save's form: (slot, cell, where
    /// in `values`).
    controls: Box<[(u8, usize, usize)]>,
    /// Persistent text variables, back to back in `texts`.
    text_spans: Box<[Span]>,
    texts: Box<[Text]>,
    /// Lua widgets by (slot, number), and their values.
    widgets: Box<[(u8, u32)]>,
    widget_values: Box<[Option<f64>]>,
    /// By Lua slot, room for its `onSave` state and how much it took.
    states: Box<[State]>,
    /// By script slot, its change count ([`kontra_engine::Vm::runs`]) when
    /// last read, and whether this capture reads it.
    seen: Box<[Option<u32>]>,
    fresh: Box<[bool]>,
    /// How far the read is.
    at: Cursor,
}

/// One variable's cells: its slot, first cell and length, where they go in
/// the capture, and whether they were there to read.
#[derive(Clone, Copy, Debug)]
struct Span {
    slot: u8,
    base: usize,
    len: usize,
    to: usize,
    read: bool,
    /// A control's value: read again in its saved form.
    control: bool,
}

/// How far a [`Capture::read`] is: not begun, the next number or text
/// span and the cells of it already read, the next control, or done.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Cursor {
    #[default]
    Start,
    Numbers(usize, usize),
    Texts(usize, usize),
    Controls(usize),
    Done,
}

/// A Lua slot, room for its saved state, and the length it took.
type State = (u8, Box<[u8]>, Option<usize>);

/// What one text cell costs a read's budget, in number cells.
const TEXT_WEIGHT: usize = size_of::<Text>() / size_of::<i64>();
/// What reading one control cell in its saved form costs, in number cells
/// (a search of the controls and of its menu).
const CONTROL_WEIGHT: usize = 16;

impl Capture {
    /// Every persistent cell of a part whose script slots are `slots`.
    #[must_use]
    pub fn new(part: usize, generation: u64, slots: &[Vars]) -> Box<Self> {
        let slotted = || {
            (slots.iter().enumerate()).filter_map(|(slot, v)| Some((u8::try_from(slot).ok()?, v)))
        };
        let spans = |text: bool| -> Box<[Span]> {
            let mut to = 0;
            slotted()
                .flat_map(|(slot, v)| v.vars.iter().map(move |var| (slot, var)))
                .filter(|(_, var)| var.text == text)
                .filter_map(|(slot, var)| {
                    let (base, len) = (
                        usize::try_from(var.base).ok()?,
                        usize::try_from(var.len).ok()?,
                    );
                    let control = var.control.is_some();
                    let span = Span {
                        slot,
                        base,
                        len,
                        to,
                        read: false,
                        control,
                    };
                    to += len;
                    Some(span)
                })
                .collect()
        };
        let (spans, text_spans) = (spans(false), spans(true));
        let controls = (spans.iter().filter(|s| s.control))
            .flat_map(|s| (0..s.len).map(move |n| (s.slot, s.base + n, s.to + n)))
            .collect();
        let widgets: Box<[(u8, u32)]> = slotted()
            .flat_map(|(slot, v)| v.widgets.iter().map(move |&(_, n)| (slot, n)))
            .collect();
        let states = slotted()
            .filter(|(_, v)| v.state)
            .map(|(slot, _)| (slot, vec![0; STATE].into(), None))
            .collect();
        Box::new(Self {
            part,
            generation,
            values: vec![0; spans.iter().map(|s| s.len).sum()].into(),
            texts: vec![Text::EMPTY; text_spans.iter().map(|s| s.len).sum()].into(),
            spans,
            controls,
            text_spans,
            widget_values: vec![None; widgets.len()].into(),
            widgets,
            states,
            seen: vec![None; slots.len()].into(),
            fresh: vec![false; slots.len()].into(),
            at: Cursor::Start,
        })
    }

    /// Reads on from where it stands, at most `budget` cells' worth (taken
    /// from it), from `engine`; true once done. Runs on the audio thread,
    /// in place. A slot whose machine did not run and was not written since
    /// the last capture is not read: its values are that capture's
    /// ([`Capture::keep`]), and an idle part costs nothing.
    pub(crate) fn read(&mut self, engine: &mut Engine, budget: &mut usize) -> bool {
        if self.at == Cursor::Start {
            self.begin(engine);
            self.at = Cursor::Numbers(0, 0);
        }
        let engine = &*engine;
        if let Cursor::Numbers(span, done) = self.at {
            let cells = |slot, range| engine.vm(slot)?.cells(range);
            let mut at = (span, done);
            if !copy(
                &mut self.spans,
                &mut self.values,
                &self.fresh,
                &mut at,
                budget,
                1,
                cells,
            ) {
                self.at = Cursor::Numbers(at.0, at.1);
                return false;
            }
            self.at = Cursor::Texts(0, 0);
        }
        if let Cursor::Texts(span, done) = self.at {
            let cells = |slot, range| engine.vm(slot)?.texts(range);
            let mut at = (span, done);
            let (spans, texts) = (&mut self.text_spans, &mut self.texts);
            if !copy(
                spans,
                texts,
                &self.fresh,
                &mut at,
                budget,
                TEXT_WEIGHT,
                cells,
            ) {
                self.at = Cursor::Texts(at.0, at.1);
                return false;
            }
            self.at = Cursor::Controls(0);
        }
        if let Cursor::Controls(mut next) = self.at {
            let fresh = |slot: u8| self.fresh.get(usize::from(slot)).copied().unwrap_or(false);
            while let Some(&(slot, cell, to)) = self.controls.get(next) {
                // Passing over an unread slot's control costs a cell too, so
                // no block walks the whole list on an empty budget.
                let cost = if fresh(slot) { CONTROL_WEIGHT } else { 1 };
                let Some(left) = budget.checked_sub(cost) else {
                    self.at = Cursor::Controls(next);
                    return false;
                };
                *budget = left;
                if fresh(slot)
                    && let (Some(saved), Some(value)) =
                        (engine.script_saved_in(slot, cell), self.values.get_mut(to))
                {
                    *value = saved;
                }
                next += 1;
            }
            self.at = Cursor::Done;
        }
        true
    }

    /// Decides which slots this capture reads, and takes their Lua widgets
    /// and saved states (a few per part).
    fn begin(&mut self, engine: &mut Engine) {
        for (slot, (seen, fresh)) in (0u8..).zip(self.seen.iter_mut().zip(self.fresh.iter_mut())) {
            let now = engine.vm(slot).map(kontra_engine::Vm::runs);
            // A slot with no machine reads as missing every time.
            *fresh = now.is_none() || *seen != now;
            *seen = now;
        }
        let fresh = |slot: u8| self.fresh.get(usize::from(slot)).copied().unwrap_or(false);
        for (slot, room, len) in self.states.iter_mut().filter(|s| fresh(s.0)) {
            *len = engine.save_state(*slot, room);
        }
        let widgets = self.widget_values.iter_mut().zip(self.widgets.iter());
        for (value, &(slot, n)) in widgets.filter(|(_, w)| fresh(w.0)) {
            *value = engine.widget(slot, n).map(|w| w.value);
        }
    }

    /// Asks only for what changed since `last`, this part's capture before,
    /// and starts the read over (a capture is reused).
    pub fn after(&mut self, last: Option<&Self>) {
        match last.filter(|l| l.seen.len() == self.seen.len()) {
            Some(last) => self.seen.copy_from_slice(&last.seen),
            None => self.seen.fill(None),
        }
        self.fresh.fill(false);
        self.at = Cursor::Start;
    }

    /// Takes `last`'s values for the slots this capture did not read: they
    /// did not change since.
    fn keep(&mut self, last: Option<&mut Self>) {
        let Some(last) = last.filter(|l| l.generation == self.generation) else {
            return;
        };
        let fresh = |slot: u8| self.fresh.get(usize::from(slot)).copied().unwrap_or(false);
        let spans = self.spans.iter_mut().zip(last.spans.iter());
        for (mine, theirs) in spans.filter(|(s, _)| !fresh(s.slot)) {
            mine.read = theirs.read;
            let range = mine.to..mine.to + mine.len;
            if let (Some(to), Some(from)) =
                (self.values.get_mut(range.clone()), last.values.get(range))
            {
                to.copy_from_slice(from);
            }
        }
        let spans = self.text_spans.iter_mut().zip(last.text_spans.iter());
        for (mine, theirs) in spans.filter(|(s, _)| !fresh(s.slot)) {
            mine.read = theirs.read;
            let range = mine.to..mine.to + mine.len;
            if let (Some(to), Some(from)) =
                (self.texts.get_mut(range.clone()), last.texts.get(range))
            {
                to.copy_from_slice(from);
            }
        }
        let widgets =
            (self.widget_values.iter_mut().zip(self.widgets.iter())).zip(last.widget_values.iter());
        for ((mine, _), theirs) in widgets.filter(|((_, w), _)| !fresh(w.0)) {
            *mine = *theirs;
        }
        let states = self.states.iter_mut().zip(last.states.iter_mut());
        for (mine, theirs) in states.filter(|(s, _)| !fresh(s.0)) {
            std::mem::swap(&mut mine.1, &mut theirs.1);
            mine.2 = theirs.2;
        }
    }
}

/// Takes capture `c` back from the audio side. Read to the end and having
/// read any slot, it becomes `latest`, with what it did not read again kept
/// from the one before; the other goes to `spare`, its room for the next.
/// One that read nothing leaves `latest` standing: nothing is copied. One
/// the audio side gave up on (its part was replaced) is not a capture.
pub(crate) fn landed(
    mut c: Box<Capture>,
    latest: &mut Option<Box<Capture>>,
    spare: &mut Option<Box<Capture>>,
) {
    let done = c.at == Cursor::Done;
    if done && (c.fresh.contains(&true) || latest.is_none()) {
        c.keep(latest.as_deref_mut());
        *spare = latest.replace(c);
    } else {
        *spare = Some(c);
    }
}

/// Copies the cells of `spans` in the `fresh` slots from `cells` into
/// `values`, from `at` (span, cells of it done), each cell costing `weight`
/// of `budget`; true once every span is copied, else `at` is where to go on.
fn copy<'e, T: Copy + 'e>(
    spans: &mut [Span],
    values: &mut [T],
    fresh: &[bool],
    at: &mut (usize, usize),
    budget: &mut usize,
    weight: usize,
    cells: impl Fn(u8, std::ops::Range<usize>) -> Option<&'e [T]>,
) -> bool {
    while let Some(span) = spans.get_mut(at.0) {
        let left = span.len.saturating_sub(at.1);
        if left == 0 || !fresh.get(usize::from(span.slot)).copied().unwrap_or(false) {
            *at = (at.0 + 1, 0);
            continue;
        }
        let n = left.min(*budget / weight);
        if n == 0 {
            return false;
        }
        *budget -= n * weight;
        if at.1 == 0 {
            span.read = true;
        }
        let from = span.base + at.1;
        let to = span.to + at.1;
        match (cells(span.slot, from..from + n), values.get_mut(to..to + n)) {
            (Some(src), Some(dst)) => dst.copy_from_slice(src),
            _ => span.read = false,
        }
        at.1 += n;
    }
    true
}

/// By instrument script, the saved values to load it with: `loaded` (what it
/// loaded with) with each persistent number variable replaced by its value
/// in `capture`. Scripts with nothing captured keep `loaded`. Linear in the
/// cells and entries: a persistent array may hold a million values.
#[must_use]
pub fn saved(loaded: &[Vec<String>], slots: &[Vars], capture: &Capture) -> Vec<Vec<String>> {
    // By (slot, first cell, text): the captured values of each variable.
    let numbers = (capture.spans.iter()).map(|s| ((s.slot, s.base, false), *s));
    let texts = (capture.text_spans.iter()).map(|s| ((s.slot, s.base, true), *s));
    let spans: HashMap<(u8, usize, bool), Span> = numbers.chain(texts).collect();
    let mut out = loaded.to_vec();
    lua_saved(&mut out, slots, capture);
    for (slot, vars) in slots.iter().enumerate() {
        let (Ok(slot), Some(entries)) = (u8::try_from(slot), out.get_mut(vars.script)) else {
            continue;
        };
        let mut by_name: HashMap<String, usize> = (entries.iter().enumerate())
            .filter_map(|(n, e)| Some((bare(e.split(' ').next()?), n)))
            .collect();
        for var in &vars.vars {
            let span = usize::try_from(var.base)
                .ok()
                .and_then(|base| spans.get(&(slot, base, var.text)))
                .filter(|s| s.read && usize::try_from(var.len).is_ok_and(|len| len == s.len));
            let Some(span) = span else {
                continue;
            };
            let range = span.to..span.to + span.len;
            let entry = if var.text {
                let Some(values) = capture.texts.get(range) else {
                    continue;
                };
                // A text array saves one line per value, each ended by a
                // newline, as Kontakt writes it (oracle: 80 values, 80
                // newlines); a single text saves bare.
                let values: Vec<&str> = values.iter().map(Text::as_str).collect();
                let end = if values.len() > 1 { "\n" } else { "" };
                format!("{} {}{end}", var.name, values.join("\n"))
            } else {
                let Some(values) = capture.values.get(range) else {
                    continue;
                };
                numbers_entry(&var.name, values, var.real)
            };
            match by_name
                .get(&bare(&var.name))
                .and_then(|&n| entries.get_mut(n))
            {
                Some(old) => *old = entry,
                None => {
                    by_name.insert(bare(&var.name), entries.len());
                    entries.push(entry);
                }
            }
        }
    }
    out
}

/// `name` and its cells, space-separated, as Kontakt saves them: written
/// into one string, a million values with no allocation each.
fn numbers_entry(name: &str, cells: &[i64], real: bool) -> String {
    use std::fmt::Write as _;
    let mut entry = String::with_capacity(name.len() + cells.len() * 4);
    entry.push_str(name);
    for &cell in cells {
        // Writing into a String cannot fail.
        let _ = if real {
            write!(entry, " {}", f64::from_bits(cell.cast_unsigned()))
        } else {
            // Integer cells hold Kontakt's 32-bit values: the low half.
            write!(entry, " {}", cell as i32)
        };
    }
    entry
}

/// Room for a Lua script's saved state (Augmented Orchestra's: 336 bytes).
const STATE: usize = 16 << 10;

/// The Lua widgets of `capture` into `out`, as `name=value` lines, and the
/// script's `onSave` state as one more.
fn lua_saved(out: &mut [Vec<String>], slots: &[Vars], capture: &Capture) {
    let read: HashMap<(u8, u32), Option<f64>> = (capture.widgets.iter().copied())
        .zip(capture.widget_values.iter().copied())
        .collect();
    for (slot, vars) in slots.iter().enumerate() {
        let (Ok(slot), Some(entries)) = (u8::try_from(slot), out.get_mut(vars.script)) else {
            continue;
        };
        let mut by_name: HashMap<String, usize> = (entries.iter().enumerate())
            .filter_map(|(n, e)| Some((e.split_once('=')?.0.to_owned(), n)))
            .collect();
        let state = (capture.states.iter())
            .find(|s| s.0 == slot)
            .and_then(|(_, room, len)| std::str::from_utf8(room.get(..(*len)?)?).ok());
        let state = state.map(|s| {
            (
                kontra_lua::STATE.into(),
                format!("{}={s}", kontra_lua::STATE),
            )
        });
        let values = (vars.widgets.iter()).filter_map(|(name, n)| {
            let value = read.get(&(slot, *n)).copied().flatten()?;
            Some((name.to_string(), format!("{name}={value}")))
        });
        for (name, entry) in values.chain(state) {
            match by_name.get(&name).and_then(|&at| entries.get_mut(at)) {
                Some(e) => *e = entry,
                None => {
                    by_name.insert(name, entries.len());
                    entries.push(entry);
                }
            }
        }
    }
}

/// A variable's name without its type sigil, as saved names are matched.
fn bare(name: &str) -> String {
    name.trim_start_matches(['$', '%', '~', '?', '@', '!'])
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lua_pages_persistent_widgets_save_as_name_equals_value() {
        let page = kontra_lua::compile(
            r#"Knob{"Air", 64, 0, 127} Knob{"Temp", 1, persistent=false}
               OnOffButton{"Hold", false} Label{"Says"}"#,
            &[],
            &[],
        )
        .unwrap()
        .ui;
        let vars = Vars::lua(0, &page);
        assert_eq!(vars.widgets, [("Air".into(), 0), ("Hold".into(), 2)]);
        let mut capture = Capture::new(0, 0, std::slice::from_ref(&vars));
        capture.widget_values = [Some(100.), Some(1.)].into();
        let state = br#"{["ccVel"]=100,}"#;
        let (_, room, len) = &mut capture.states[0];
        room[..state.len()].copy_from_slice(state);
        *len = Some(state.len());
        let loaded = vec![vec!["Air=64".to_owned(), "Other=3".to_owned()]];
        let out = saved(&loaded, std::slice::from_ref(&vars), &capture);
        let state = r#"__state={["ccVel"]=100,}"#;
        assert_eq!(out, [vec!["Air=100", "Other=3", "Hold=1", state]]);
    }

    #[test]
    fn captured_values_replace_the_loaded_ones_in_kontakts_form() {
        let script = kontra_ksp::compile(
            "on init
                declare ui_knob $vol(0, 100, 1)
                declare ~tone
                declare %steps[3]
                declare $scratch
                declare @name
                make_persistent($vol)
                make_persistent(~tone)
                make_persistent(%steps)
                make_persistent(@name)
            end on",
            &[],
        )
        .unwrap();
        let vars = Vars::of(0, &script);
        let names: Vec<_> = vars.vars.iter().map(|v| &*v.name).collect();
        assert_eq!(names, ["$vol", "~tone", "%steps", "@name"]);
        assert!(vars.vars[0].control.is_some() && vars.vars[1].control.is_none());
        let mut capture = Capture::new(0, 0, std::slice::from_ref(&vars));
        let cell = |name: &str| vars.vars.iter().find(|v| &*v.name == name).unwrap().base;
        capture.spans.iter_mut().for_each(|s| s.read = true);
        capture.text_spans.iter_mut().for_each(|s| s.read = true);
        let set = |c: &mut Capture, cell: u32, v: i64| {
            let span = c
                .spans
                .iter()
                .find(|s| (s.base..s.base + s.len).contains(&(cell as usize)));
            let at = span.map(|s| s.to + cell as usize - s.base).unwrap();
            c.values[at] = v;
        };
        set(&mut capture, cell("$vol"), 37);
        set(&mut capture, cell("~tone"), 0.25f64.to_bits() as i64);
        for n in 0..3 {
            set(&mut capture, cell("%steps") + n, i64::from(n) - 1);
        }
        let at = capture
            .text_spans
            .iter()
            .find(|s| s.base == cell("@name") as usize);
        capture.texts[at.unwrap().to] = Text::from_str_cut("solo cello").0;
        let loaded = vec![vec!["$VOL 10".to_owned(), "@name strings".to_owned()]];
        let out = saved(&loaded, std::slice::from_ref(&vars), &capture);
        assert_eq!(
            out,
            [vec![
                "$vol 37",
                "@name solo cello",
                "~tone 0.25",
                "%steps -1 0 1"
            ]]
        );
        // What it writes restores to the same cells.
        let r = script.restore(&out[0]);
        assert!(r.ints.contains(&(cell("$vol"), 37)) && r.skipped.is_empty());
        assert_eq!(r.texts, [(cell("@name"), "solo cello".to_owned())]);
    }
}
