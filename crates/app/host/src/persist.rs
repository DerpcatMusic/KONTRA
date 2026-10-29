//! A part's script state for the project: the persistent variables of its
//! KSP scripts (the controls' values among them), read from the live engine
//! and written in Kontakt's saved form (`"<name> <value…>"`), so a reload
//! restores them the way a library's own saved values are restored: after
//! `on init`, before `on persistence_changed`.
//!
//! The engine lives on the audio thread, so the rack sends a [`Capture`]
//! there; the audio side fills its values in place and sends it back. Text
//! variables come back the same way, as fixed-size [`Text`].

use std::collections::HashMap;

use kontra_engine::{Engine, Text};
use kontra_ksp::{Cells, FIRST_UI_ID, Persistence, Script};

/// One script slot's persistent number variables, and the instrument script
/// it was compiled from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vars {
    pub script: usize,
    pub vars: Vec<Var>,
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
        Self { script, vars }
    }
}

/// Cells for the audio side to read out of one part's engine.
pub struct Capture {
    pub(crate) part: usize,
    pub(crate) generation: u64,
    cells: Box<[(u8, u32)]>,
    values: Box<[Option<i64>]>,
    text_cells: Box<[(u8, u32)]>,
    texts: Box<[Option<Text>]>,
}

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
        Box::new(Self {
            part,
            generation,
            values: vec![None; cells.len()].into_boxed_slice(),
            texts: vec![None; text_cells.len()].into_boxed_slice(),
            cells,
            text_cells,
        })
    }

    /// Reads the cells from `engine`; runs on the audio thread, in place.
    pub(crate) fn read(&mut self, engine: &Engine) {
        for (value, &(slot, cell)) in self.values.iter_mut().zip(self.cells.iter()) {
            *value = engine.script_cell_in(slot, cell as usize);
        }
        for (value, &(slot, cell)) in self.texts.iter_mut().zip(self.text_cells.iter()) {
            *value = engine.script_text_in(slot, cell as usize);
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
