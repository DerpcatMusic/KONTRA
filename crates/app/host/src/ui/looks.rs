//! What a [`super::Live`] reads of a running script.

use kontra_engine::{ControlLook, Engine, Page};

/// What a [`super::Live`] reads of a running script: the engine itself, or a copy
/// of those reads made on the audio thread (`crate::view::LookFrame`).
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
}
