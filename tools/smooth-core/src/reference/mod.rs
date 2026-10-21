//! Literal translations of soothe3 functions, one module per area of the binary.
//!
//! Each module's doc comments cite the addresses it translates. These are reference
//! code for differential checks against the emulated binary, not wired into `dsp.rs`.

pub mod output;
pub mod sidechain;
