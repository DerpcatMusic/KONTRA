//! Universal `filter` DSP (wave 4): the group/bus filter kernels.
//!
//! One module per native topology; `sampler_ir::fx::filter::Config` picks the topology and
//! its laws. Kernels that are checked against native instruction vectors say so in their own
//! docs; anything else is a labelled stand-in.
pub(crate) mod ar;
pub(crate) mod ar_kernel;
pub(crate) mod daft;
pub(crate) mod formant;
pub(crate) mod ladder;
pub(crate) mod ladder_kernel;
pub(crate) mod svf;
