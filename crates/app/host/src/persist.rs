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
pub struct Capture {
    pub(crate) part: usize,
    pub(crate) generation: u64,
    cells: Box<[(u8, u32)]>,
    values: Box<[Option<i64>]>,
    text_cells: Box<[(u8, u32)]>,
    texts: Box<[Option<Text>]>,
    /// Lua widgets by (slot, number), and their values.
    widgets: Box<[(u8, u32)]>,
    widget_values: Box<[Option<f64>]>,
    /// By Lua slot, room for its `onSave` state and how much it took.
    states: Box<[State]>,
    /// The engine's callback count when the states were last taken, and
    /// whether this capture took them.
    seen: Option<u32>,
    fresh: bool,
}

/// A Lua slot, room for its saved state, and the length it took.
type State = (u8, Box<[u8]>, Option<usize>);

impl Capture {
    /// Every persistent cell of a part whose script slots are `slots`.
    #[must_use]
    pub fn new(part: usize, generation: u64, slots: &[Vars]) -> Box<Self> {
        let cells = |text: bool| -> Box<[(u8, u32)]> {
            (slots.iter().enumerate())
                .filter_map(|(slot, v)| Some((u8::try_from(slot).ok()?, v)))
                .flat_map(|(slot, v)| {
                    (v.vars.iter())
                        .filter(move |var| var.text == text)
                        .flat_map(move |var| (var.base..var.base + var.len).map(move |c| (slot, c)))
                })
                .collect()
        };
        let (cells, text_cells) = (cells(false), cells(true));
        let widgets: Box<[(u8, u32)]> = (slots.iter().enumerate())
            .filter_map(|(slot, v)| Some((u8::try_from(slot).ok()?, v)))
            .flat_map(|(slot, v)| v.widgets.iter().map(move |&(_, n)| (slot, n)))
            .collect();
        let states = (slots.iter().enumerate())
            .filter(|(_, v)| v.state)
            .filter_map(|(slot, _)| Some((u8::try_from(slot).ok()?, vec![0; STATE].into(), None)))
            .collect();
        Box::new(Self {
            part,
            generation,
            states,
            seen: None,
            fresh: false,
            values: vec![None; cells.len()].into_boxed_slice(),
            texts: vec![None; text_cells.len()].into_boxed_slice(),
            widget_values: vec![None; widgets.len()].into_boxed_slice(),
            cells,
            text_cells,
            widgets,
        })
    }

    /// Reads the cells from `engine`; runs on the audio thread, in place.
    /// `onSave` runs only when a callback ran since the states were last
    /// taken: an idle part costs nothing (a host's save cannot wait for
    /// the audio thread, so the state is kept ready).
    pub(crate) fn read(&mut self, engine: &mut Engine) {
        let runs = |slot| engine.vm(slot).map_or(0, kontra_engine::Vm::runs);
        let now = (self.states.iter()).fold(0u32, |n, s| n.wrapping_add(runs(s.0)));
        self.fresh = self.seen != Some(now);
        if self.fresh {
            for (slot, room, len) in &mut *self.states {
                *len = engine.save_state(*slot, room);
            }
        }
        self.seen = Some(now);
        for (value, &(slot, cell)) in self.values.iter_mut().zip(self.cells.iter()) {
            *value = engine.script_saved_in(slot, cell as usize);
        }
        for (value, &(slot, cell)) in self.texts.iter_mut().zip(self.text_cells.iter()) {
            *value = engine.script_text_in(slot, cell as usize);
        }
        for (value, &(slot, n)) in self.widget_values.iter_mut().zip(self.widgets.iter()) {
            *value = engine.widget(slot, n).map(|w| w.value);
        }
    }

    /// Asks only for what changed since `last`, this part's capture before.
    pub fn after(&mut self, last: Option<&Self>) {
        self.seen = last.and_then(|l| l.seen);
    }

    /// Keeps `last`'s states when this capture did not take them.
    pub fn keep(&mut self, last: Option<Box<Self>>) {
        if let Some(mut last) = last.filter(|_| !self.fresh) {
            std::mem::swap(&mut self.states, &mut last.states);
        }
    }

    /// Where each (slot, cell) was read, for lookups in time linear in the
    /// cells captured.
    fn index(&self) -> HashMap<(u8, u32), Option<i64>> {
        self.cells
            .iter()
            .copied()
            .zip(self.values.iter().copied())
            .collect()
    }

    fn text_index(&self) -> HashMap<(u8, u32), Option<&str>> {
        (self.text_cells.iter().copied())
            .zip(self.texts.iter().map(|t| t.as_ref().map(Text::as_str)))
            .collect()
    }
}

/// By instrument script, the saved values to load it with: `loaded` (what it
/// loaded with) with each persistent number variable replaced by its value
/// in `capture`. Scripts with nothing captured keep `loaded`. Linear in the
/// cells and entries: a persistent array may hold a million values.
#[must_use]
pub fn saved(loaded: &[Vec<String>], slots: &[Vars], capture: &Capture) -> Vec<Vec<String>> {
    let read = capture.index();
    let read_text = capture.text_index();
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
            let values: Option<Vec<String>> = (var.base..var.base + var.len)
                .map(|c| {
                    if var.text {
                        read_text
                            .get(&(slot, c))
                            .copied()
                            .flatten()
                            .map(str::to_owned)
                    } else {
                        read.get(&(slot, c))
                            .copied()
                            .flatten()
                            .map(|v| text(v, var.real))
                    }
                })
                .collect();
            let Some(values) = values else {
                continue;
            };
            // A text array saves one line per value.
            let joiner = if var.text { "\n" } else { " " };
            let entry = format!("{} {}", var.name, values.join(joiner));
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

/// A cell's value as Kontakt saves it.
fn text(cell: i64, real: bool) -> String {
    if real {
        f64::from_bits(cell as u64).to_string()
    } else {
        // Integer cells hold Kontakt's 32-bit values.
        (cell as i32).to_string()
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
        let set = |c: &mut Capture, cell: u32, v: i64| {
            let at = c.cells.iter().position(|&x| x == (0, cell)).unwrap();
            c.values[at] = Some(v);
        };
        set(&mut capture, cell("$vol"), 37);
        set(&mut capture, cell("~tone"), 0.25f64.to_bits() as i64);
        for n in 0..3 {
            set(&mut capture, cell("%steps") + n, i64::from(n) - 1);
        }
        let at = capture
            .text_cells
            .iter()
            .position(|&x| x == (0, cell("@name")))
            .unwrap();
        capture.texts[at] = Some(Text::from_str_cut("solo cello").0);
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
