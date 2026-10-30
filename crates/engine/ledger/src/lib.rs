//! The engine's bookkeeping of notes and scheduled work: every live note
//! and what it carries ([`ledger`]), the time-ordered queue of input,
//! script resumes and releases ([`queue`]), and the group masks scripts
//! set per note ([`group_masks`]).

pub mod group_masks;
pub mod ledger;
pub mod queue;

pub use ledger::NoteRef;
