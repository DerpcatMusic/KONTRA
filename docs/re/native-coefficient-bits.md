# Preserve reference coefficient bits during lint cleanup

Clippy correctly flags rounded mathematical constants in ordinary formulas. Native DSP models also contain deliberately rounded reference coefficients, such as `6.283`, whose replacement with a higher-precision tau would change arithmetic. Those values now use explicit `f32::from_bits` or `f64::from_bits` expressions. A standard-library constant is used only where its bits equal the previous literal in the original inferred precision.

The release cleanup changes 20 coefficient expressions (one reused twice) and folds the constant reset expression `1 - (1 - 0)^2` to positive zero. A standalone Rust proof compares every old literal parsed by Rust against its replacement and checks both reset precisions. All comparisons pass; no lint is disabled and no arithmetic grouping, parameter law, DSP admission or native-parity warning is changed. Existing native/PCM tests remain the behavioral checks.

The private proof, exact original source hashes and hosted failure log are retained under `PluginArtifacts/kontra-release-0.3.482-20261015`. This is a build-quality correction, not a newly counted audio defect or evidence of additional native parity.
