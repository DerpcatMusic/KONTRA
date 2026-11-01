//! A compiled script as the engine runs it: its program, where its
//! callbacks start, and where its controls' values live.

use kontra_vm::{Program, Text};

use crate::{EnginePar, EnginePars};

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
    /// Absolute library folder, when known.
    pub library_folder: Option<Text>,
    /// The installed sampler's factory data folder, when one was found.
    pub factory_folder: Option<Text>,
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
    /// Reads the array files the script names by path while it initialises.
    pub files: Option<std::sync::Arc<dyn crate::files::Files>>,
    /// The script's arrays: first cell, whether they are text cells, and
    /// name. An array file holds the name of the array it fills.
    pub arrays: Vec<(u32, bool, Box<str>)>,
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
    /// A prepared folder by script path kind.
    #[must_use]
    pub fn folder(&self, kind: i32) -> Option<Text> {
        match kind {
            1 => self.library_folder,
            2 => self.source_folder,
            // No installation: scripts read an empty path, as the manual allows.
            3 => Some(self.factory_folder.unwrap_or(Text::EMPTY)),
            _ => None,
        }
    }

    /// Copies a prepared numeric array into its cells.
    pub fn load_number_data(&self, base: u32, len: usize, cells: &mut [i64]) {
        if let Some((_, values)) = self.data.iter().find(|(b, _)| *b == base) {
            let cells = cells.get_mut(base as usize..).unwrap_or_default();
            for (cell, value) in cells.iter_mut().zip(values).take(len) {
                *cell = *value;
            }
        }
    }

    /// What `par` lives on, so the engine can tell a target the
    /// instrument lacks.
    #[must_use]
    pub fn holder(&self, par: EnginePar) -> Holder {
        use kontra_ir::KnobTarget as K;
        match par {
            EnginePar::Knob(n) => match self.knobs.get(usize::from(n)).map(|k| k.0) {
                Some(K::Effect(_) | K::Slot(_) | K::Impulse(_)) => Holder::Slot,
                Some(K::LfoRate | K::LfoSteps | K::LfoWidth | K::LfoWeight(_)) => Holder::Lfo,
                _ => Holder::Group,
            },
            EnginePar::EffectBypass
            | EnginePar::EffectOutput
            | EnginePar::EffectDry
            | EnginePar::SendLevel(_)
            | EnginePar::Reverb(_)
            | EnginePar::ReverbNetwork
            | EnginePar::GainerGain
            | EnginePar::StereoWidth
            | EnginePar::StereoPan => Holder::Slot,
            EnginePar::EffectType | EnginePar::EffectSubtype => Holder::SlotKind,
            EnginePar::LfoFade => Holder::Lfo,
            EnginePar::RouteDepth | EnginePar::RouteIntensity => Holder::Route,
            EnginePar::ModBypass => Holder::Mod,
            EnginePar::Volume | EnginePar::Pan | EnginePar::Tune | EnginePar::OutputChannel => {
                Holder::Level
            }
            _ => Holder::Group,
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

/// What an engine parameter lives on ([`ScriptPlan::holder`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    /// The group or the instrument itself.
    Group,
    /// The module in an effect slot.
    Slot,
    /// The slot: its type and subtype answer even when it is empty.
    SlotKind,
    /// One of the group's LFOs.
    Lfo,
    /// One of the group's modulation routes.
    Route,
    /// One of the group's modulators.
    Mod,
    /// A level or output, of a group, the instrument or a bus.
    Level,
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

pub use kontra_script_ui::{Controls, LookPars};

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

/// What recalling saved values (a preset of the loaded instrument) does to
/// its scripts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recall {
    /// `init` runs again on a fresh state, then the values land, then the
    /// persistence-changed callback runs.
    Restart,
    /// The scripts keep their state: the values land, then the
    /// persistence-changed callback runs.
    Keep,
}

impl Recall {
    /// The law a script's snapshot type picks: even types run `init` again,
    /// odd ones keep the state (KSP manual, `set_snapshot_type`).
    #[must_use]
    pub fn of_type(kind: i64) -> Self {
        if kind & 1 == 0 {
            Self::Restart
        } else {
            Self::Keep
        }
    }
}

/// One script slot's saved values, as cell writes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Saved {
    pub ints: Vec<(u32, i64)>,
    pub texts: Vec<(u32, Text)>,
}
