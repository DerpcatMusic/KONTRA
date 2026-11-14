//! What the control thread asks of the audio thread ([`Command`]).

use kontra_engine::{Engine, Event, Prepared, ir};
use kontra_ui_model::UiModifiers;

use super::{Hold, MacroTarget, Mix, requests};
use crate::persist::Capture;

/// What the control thread asks of the audio thread.
pub enum Command {
    /// Put `engine` in `part`, retiring the one there.
    Install {
        part: usize,
        engine: Box<Engine>,
        mix: Mix,
        /// Which load it is: effect units built for another never go in.
        generation: u64,
    },
    /// Swap in `part`'s plan with its scripts' zone edits, if the part is
    /// still the load that made them; the box comes back with the old plan.
    Zones {
        part: usize,
        generation: u64,
        swap: Box<requests::ZoneSwap>,
    },
    /// A purged group's samples are in: it plays from the next note, and
    /// the script's async completes (`took`: it loaded).
    Unpurged {
        part: usize,
        generation: u64,
        unpurge: kontra_engine::Unpurge,
        took: bool,
    },
    /// Put the effect unit a script asked for into `part`'s slot, if the
    /// part is still the load that asked; the box comes back holding what
    /// it replaced, so audio frees nothing.
    Effect {
        part: usize,
        generation: u64,
        unit: Box<Option<Prepared>>,
    },
    Remove(usize),
    /// What host macro `n` moves at its frame ([`RackAudio::move_macro`]).
    ///
    /// [`RackAudio::move_macro`]: super::RackAudio::move_macro
    Macro {
        n: usize,
        target: Option<MacroTarget>,
    },
    Mix(usize, Mix),
    /// Linear master gain.
    Master(f32),
    /// Tempo in quarter notes per minute.
    Tempo(f32),
    /// End every note on every part at once.
    Panic,
    /// Play `Event` on every part at the next block (the editor's keyboard
    /// and wheels).
    Play(Event),
    /// How long each note into `part` waits: auto-align.
    Hold(usize, Hold),
    /// Read a part's persistent script cells and send them back.
    Capture(Box<Capture>),
    /// Lend `part`'s engine to the control thread to recall saved values
    /// into, if the part is still that load: the box comes back holding it,
    /// and the part is silent until it is installed again.
    Recall(Box<Recalling>),
    /// Answer a part's file request ([`files`]), if the part is still
    /// that load; the box comes back.
    File {
        part: usize,
        generation: u64,
        reply: Box<kontra_engine::files::FileReply>,
    },
    /// Copy what a part's interface reads of its script and send it back.
    Look(Box<crate::view::LookFrame>),
    /// The user moved control `id` of script `slot` to `value`, holding
    /// `keys` at the click.
    Control {
        part: usize,
        slot: u8,
        id: i32,
        /// The cell of a table set, 0 for any other control.
        cell: u32,
        value: i64,
        keys: UiModifiers,
    },
    /// The user typed `text` into text edit `id` of `part`'s script `slot`,
    /// whose variable's text lives in `cell`.
    ControlText {
        part: usize,
        slot: u8,
        id: i32,
        cell: u32,
        text: Box<kontra_engine::Text>,
    },
    /// The user moved widget `n` of script `slot`'s own page to `value`.
    Widget {
        part: usize,
        slot: u8,
        n: u32,
        value: f64,
        cell: Option<u32>,
    },
    /// The user edited a group's sound (the Sound tab): its envelope, and
    /// the insert it changed, if any. The box comes back empty, so audio
    /// frees nothing.
    Sound {
        part: usize,
        group: usize,
        envelope: kontra_engine::Ahdsr,
        insert: Box<Option<(usize, ir::Effect)>>,
    },
}

/// A recall of saved values (by script slot) into a part's running
/// scripts: the audio side puts the part's engine in it.
pub struct Recalling {
    pub part: usize,
    pub generation: u64,
    pub saved: Box<[kontra_engine::Saved]>,
    pub engine: Option<Box<Engine>>,
}
