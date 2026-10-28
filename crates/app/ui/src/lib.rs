//! The editor's views; design in docs/architecture-v3/07-APP.md §5.
//!
//! The Original view draws a library's [`kontra_ui_model::Interface`] as
//! its source drew it: [`view::page`] lays a page out from the tree, and
//! [`Pictures`] decodes the tree's pictures off the UI thread, once per
//! file content, so the first frame draws what is ready and waits on
//! nothing.

pub mod layout;
pub mod pictures;
pub mod view;

pub use pictures::Pictures;

/// The face the views draw text with until a source names its own.
pub const NOTO_SANS: &[u8] = include_bytes!("../../../../assets/NotoSans.ttf");

/// A [`mui::Ui`] that draws text with [`NOTO_SANS`].
#[must_use]
pub fn ui() -> mui::Ui {
    let ui = mui::Ui::default();
    match mui::prelude::Font::new(NOTO_SANS) {
        Ok(font) => ui.font(font),
        Err(_) => ui,
    }
}
