//! What a [`super::Live`] reads of a running script.

use kontra_engine::{ControlLook, Engine, Page};

/// What a [`super::Live`] reads of a running script: the engine itself, or a copy
/// of those reads made on the audio thread (the host's `LookFrame`).
pub trait Looks {
    fn control_look(&self, slot: u8, id: i32) -> Option<ControlLook<'_>>;
    fn script_cell_in(&self, slot: u8, n: usize) -> Option<i64>;
    /// Text cell `n` of script `slot` (an `@` control's variable).
    fn script_text_in(&self, slot: u8, n: usize) -> Option<&str>;
    fn page(&self, slot: u8) -> Option<&Page>;
    /// The bitmap font behind font id `id` (`get_font_id`), by picture name.
    fn font(&self, slot: u8, id: i32) -> Option<&str>;
    /// The keyboard's keys as the scripts left them, by note.
    fn keys(&self) -> &[kontra_engine::Key];
    /// Whether the keyboard shows only the keys scripts press.
    fn pressed_support(&self) -> bool;
    /// The last `message` a script printed.
    fn message(&self) -> &str;
}

impl Looks for Engine {
    fn control_look(&self, slot: u8, id: i32) -> Option<ControlLook<'_>> {
        Engine::control_look(self, slot, id)
    }

    fn script_cell_in(&self, slot: u8, n: usize) -> Option<i64> {
        Engine::script_cell_in(self, slot, n)
    }

    fn script_text_in(&self, slot: u8, n: usize) -> Option<&str> {
        Some(self.vm(slot)?.text(n)?.as_str())
    }

    fn page(&self, slot: u8) -> Option<&Page> {
        Engine::page(self, slot)
    }

    fn font(&self, slot: u8, id: i32) -> Option<&str> {
        Engine::font(self, slot, id)
    }

    fn keys(&self) -> &[kontra_engine::Key] {
        Engine::keys(self)
    }

    fn pressed_support(&self) -> bool {
        Engine::key_pressed_support(self)
    }

    fn message(&self) -> &str {
        Engine::last_message(self)
    }
}

/// The parameters the UI-look builtins set, after the script's own symbols:
/// a script that names one shares its id; `move_control`'s grid and the
/// table's shown steps have no constant, so always get their own.
pub const LOOK: [&str; 19] = [
    "$CONTROL_PAR_TEXT",
    "$CONTROL_PAR_LABEL",
    "$CONTROL_PAR_HELP",
    "$CONTROL_PAR_UNIT",
    "$CONTROL_PAR_DEFAULT_VALUE",
    "$CONTROL_PAR_POS_X",
    "$CONTROL_PAR_POS_Y",
    "$CONTROL_PAR_HIDE",
    "grid_x",
    "grid_y",
    "table_steps_shown",
    "level_meter",
    "level_meter_group",
    "level_meter_generic",
    "wf_flags",
    "wf_cursor",
    "$CONTROL_PAR_WT_ZONE",
    "$CONTROL_PAR_PICTURE",
    "$CONTROL_PAR_PICTURE_STATE",
];
