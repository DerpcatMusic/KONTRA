//! A compiled script as the engine runs it: its program, where its
//! callbacks start, and where its controls' values live.

use kontra_vm::{Program, Text};

use crate::EnginePars;

/// A compiled script and where its callbacks start.
#[derive(Clone, Debug)]
pub struct ScriptPlan {
    pub program: Program,
    pub callbacks: Callbacks,
    /// Callbacks that may run or wait at once.
    pub coroutines: usize,
    /// Library data arrays `load_array(%x, 2)` reads: (first cell, values).
    pub data: Vec<(u32, Vec<i64>)>,
    /// Text array data prepared from library resources.
    pub text_data: Vec<(u32, Vec<Text>)>,
    /// Absolute folder of the instrument file, when known.
    pub source_folder: Option<Text>,
    /// Saved values written after `on init`, before `on persistence_changed`.
    pub restore: Vec<(u32, i64)>,
    pub restore_texts: Vec<(u32, Text)>,
    pub controls: Controls,
    pub engine_pars: EnginePars,
    /// The translator's law for each reverb control, in `ir::ReverbPar::ALL` order.
    pub reverb_laws: [kontra_ir::Law; 8],
    /// The networks [`EnginePar::ReverbNetwork`] picks from, in order.
    pub reverb_networks: Vec<kontra_ir::Network>,
    /// What each [`EnginePar::Knob`] sets, through the translator's law.
    pub knobs: Vec<(kontra_ir::KnobTarget, kontra_ir::Law)>,
    /// Code in another language, run by its own machine instead of `program`.
    pub foreign: Option<std::sync::Arc<dyn kontra_vm::Foreign>>,
    /// The release of a note this script played runs its own release
    /// callback, as a key-up would (KSP: `note_off` triggers `on release`);
    /// otherwise only the slots after it see the release.
    pub hears_own_releases: bool,
    /// A key-up this script holds back still ends the notes it played for
    /// the key's length (KSP `play_note(.., -1)`); otherwise they play on
    /// until the script ends them (UVI `postEvent`).
    pub held_release_ends_children: bool,
    /// MIDI controllers that move this script's controls.
    pub links: Vec<CcLink>,
}

impl ScriptPlan {
    /// Copies a prepared numeric array into its cells.
    pub fn load_number_data(&self, base: u32, len: usize, cells: &mut [i64]) {
        if let Some((_, values)) = self.data.iter().find(|(b, _)| *b == base) {
            let cells = cells.get_mut(base as usize..).unwrap_or_default();
            for (cell, value) in cells.iter_mut().zip(values).take(len) {
                *cell = *value;
            }
        }
    }

    /// Copies a prepared text array into its cells. Returns false when the resource was not found.
    pub fn load_text_data(&self, base: u32, len: usize, cells: &mut [Text]) -> bool {
        let Some((_, values)) = self.text_data.iter().find(|(b, _)| *b == base) else {
            return false;
        };
        let cells = cells.get_mut(base as usize..).unwrap_or_default();
        for (cell, value) in cells.iter_mut().zip(values).take(len) {
            *cell = *value;
        }
        true
    }
}

/// A MIDI controller tied to control `id` (a UI id): its value `v` sets
/// the control to `lo + (from + (to − from)·v/127)·(hi − lo)`, rounded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CcLink {
    pub cc: u8,
    pub id: i32,
    pub lo: i64,
    pub hi: i64,
    pub from: f32,
    pub to: f32,
}

impl CcLink {
    /// The control's value at controller value `v`.
    #[must_use]
    pub fn value(&self, v: u8) -> i64 {
        let t = f64::from(self.from) + f64::from(self.to - self.from) * f64::from(v) / 127.0;
        self.lo + (t * (self.hi - self.lo) as f64).round() as i64
    }
}

/// The script's UI controls as the engine sees them: ids, and where values live.
#[derive(Clone, Debug, Default)]
pub struct Controls {
    /// The UI id of variable 0.
    pub first: i32,
    /// The parameter id that means a control's value.
    pub value_par: i32,
    /// The parameter ids of a menu's entry count and selected entry.
    pub num_items_par: i32,
    pub selected_item_par: i32,
    /// By UI id from `first` (every variable has one), the value cell of
    /// each control whose value is a number.
    pub cells: Vec<Option<u32>>,
    /// By UI id, the steps of each control whose value is an int array (a
    /// table's): `set_control_par_arr` writes them from its cell on.
    pub steps: Vec<u32>,
    /// (value cell, control) for every control with a value cell, sorted by
    /// cell: a save finds each cell's control in log time instead of
    /// scanning every control per cell (Areia: seconds on the audio thread).
    pub by_cell: Vec<(u32, u32)>,
    /// Where the UI-look builtins store what they set.
    pub look: LookPars,
    /// The parameter ids that read whether Shift, Alt and Ctrl were held at
    /// the control's last click; `i32::MIN` when the script never names one.
    pub key_pars: [i32; 3],
}

impl Controls {
    /// The control whose value lives in `cell`.
    #[must_use]
    pub fn of_cell(&self, cell: u32) -> Option<usize> {
        let at = self.by_cell.binary_search_by_key(&cell, |&(c, _)| c).ok()?;
        self.by_cell.get(at).map(|&(_, n)| n as usize)
    }
}

pub use kontra_script_ui::LookPars;

/// Entry points; a missing callback lets events through unchanged.
#[derive(Clone, Debug, Default)]
pub struct Callbacks {
    pub init: Option<u32>,
    pub note: Option<u32>,
    pub release: Option<u32>,
    pub controller: Option<u32>,
    pub persistence_changed: Option<u32>,
    pub listener: Option<u32>,
    pub pgs_changed: Option<u32>,
    /// `on ui_controls`, run first when any control moves.
    pub ui_controls: Option<u32>,
    pub async_complete: Option<u32>,
    /// Every control by UI id, with its own `on ui_control` if it has one.
    pub ui_control: Vec<(i32, Option<u32>)>,
    /// A front end's own widget moved ([`kontra_vm::Machine::stage_widget`]).
    pub widget: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::Controls;

    #[test]
    fn a_cell_finds_the_first_control_on_it() {
        let c = Controls {
            by_cell: vec![(3, 0), (7, 2), (9, 1)],
            ..Controls::default()
        };
        assert_eq!(
            [3, 7, 9, 4].map(|cell| c.of_cell(cell)),
            [Some(0), Some(2), Some(1), None]
        );
    }
}
