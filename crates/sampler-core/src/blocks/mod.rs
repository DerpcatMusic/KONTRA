//! Universal modulation and voice building blocks, written once. Format crates
//! only configure them; nothing here knows a file format.
pub mod analog_adsr;
pub mod arp;
pub mod envelope;
pub mod follower;
pub mod glide;
pub mod lfo;
pub mod pattern_arp;
pub mod step_table;

// Original-instruction proof foundation; no production source/graph admission.
#[cfg(test)]
mod uvi_lfo;
