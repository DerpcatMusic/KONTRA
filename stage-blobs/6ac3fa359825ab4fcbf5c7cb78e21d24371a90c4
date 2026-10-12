//! MUI editors inside moose plugins.
//!
//! [`MuiEditor`] is moose's `Editor`: a child window under the host's, a
//! wgpu surface, and a `Ui` frame per native event. [`Bridge`] binds widget
//! ids to moose parameters, so a drag is the host's begin/perform/end and
//! host automation is the value the next tree reads. [`window`] is the half
//! that knows no plugin framework: the `mui-baseview` crate, re-exported.
//!
//! The moose port of MUI's `mui-truce`; the MUI crates themselves (`mui`,
//! `mui-baseview`) come from <https://github.com/Matari-Audio/MUI>.
#![deny(unsafe_code)]
pub mod bridge;
mod editor;
mod platform;

pub use bridge::{Bridge, widget_id};
pub use editor::MuiEditor;
/// The MUI toolkit itself, so a plugin can `use moose_mui::mui::prelude::*`
/// without naming the git dependency.
pub use mui;
pub use mui_baseview as window;
pub use platform::{HostScale, ParentWindow};
