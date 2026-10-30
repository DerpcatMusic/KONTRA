//! The editor's views; design in docs/architecture-v3/07-APP.md §5.
//!
//! [`app::view`] draws the editor from an [`app::App`] the host fills, out
//! of the components in [`kit`]. The Original view draws a library's
//! [`kontra_ui_model::Interface`] as its source drew it: [`view::page`]
//! lays a page out from the tree, and [`Pictures`] decodes the tree's
//! pictures off the UI thread, once per file content, so the first frame
//! draws what is ready and waits on nothing.

pub mod app;
pub mod artwork;
pub mod icon;
pub mod input;
pub mod keyboard;
pub mod kit;
pub mod layout;
pub use kontra_ui_pictures as pictures;
pub mod theme;
pub mod view;

pub use pictures::Pictures;
pub use theme::{NOTO_SANS, ui};
