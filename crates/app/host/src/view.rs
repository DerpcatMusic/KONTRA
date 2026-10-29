//! A part's library interface on the control side: the tree the editor
//! shows, built once the part's `on init` ran, and how a widget the user
//! moves reaches the script that owns it.

use std::cell::RefCell;
use std::path::Path;

use kontra_engine::Engine;
use kontra_ui_model as model;

use crate::ui::{Face, Library, Live};

/// The interface of a part's first script with one.
pub struct View {
    pub slot: u8,
    pub live: Live,
}

impl View {
    /// The view of the first script slot with a face, read from `engine`
    /// as its `on init` left it; pictures come from the library at `path`.
    #[must_use]
    pub fn of(faces: &[Option<Face>], engine: &Engine, path: Option<&Path>) -> Option<Self> {
        let library = path.map(|p| RefCell::new(Library::open(p)));
        let meta = |p: &str| library.as_ref()?.borrow_mut().meta(p);
        faces.iter().enumerate().find_map(|(slot, face)| {
            let slot = u8::try_from(slot).ok()?;
            let live = Live::new(face.as_ref()?, slot, engine, &meta).ok()?;
            Some(Self { slot, live })
        })
    }

    /// The script control behind `widget`, as (slot, UI id), and `value`
    /// as the script's integer; None for a widget no control owns.
    #[must_use]
    pub fn control(&self, widget: usize, value: f64) -> Option<(u8, i32, i64)> {
        let w = self.live.tree.widgets.get(widget)?;
        let id = w.source_id?;
        let value = match w.value {
            Some(model::Value::Real(_)) => value.to_bits() as i64,
            _ => value.round() as i64,
        };
        Some((self.slot, id, value))
    }
}
