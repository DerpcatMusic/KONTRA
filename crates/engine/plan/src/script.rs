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
    /// Where the UI-look builtins store what they set.
    pub look: LookPars,
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
