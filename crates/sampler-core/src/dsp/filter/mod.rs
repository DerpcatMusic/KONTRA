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

// Audit ledger (core-filters.tsv) status of what lives here:
// - ar, ar_kernel: PROVED equation, tables, clock and scheduling; complete host equivalence
//   (019, 073, 085) is unproven, so AR is reported as isolated-kernel exact only.
// - formant: PROVED tables, laws, polynomials and parallel topology; live ramp/cadence (043)
//   and the control domain (077) are not, so live Formant II is not offered.
// - daft: PROVED parameter laws and scheduling; the audio kernel, resampler and saturation
//   (029..031, 087) are stand-ins.
// - ladder, ladder_kernel: PROVED selector, weights, compensation (all 12 rows); wave 4 adds the
//   cutoff law and table (045, 075), pole polynomial (076) and the kernel structure (052) from
//   FUN_140b00be2; gain law (046), correction constants, rate factor and clock (047) remain.
// - svf: generic TPT stand-in (065..072, 080, 090); only SV LP2's response is measured.
