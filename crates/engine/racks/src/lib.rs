//! The effect racks voices, groups and buses run: a bus or instrument rack
//! on summed audio, each voice's own inserts, and the split of a group's
//! slots between its voices and its sum.

pub mod chain;
pub mod group_racks;
pub mod rack;
pub mod voice_rack;
