# survey-uvi-1 validation receipt

Base: integration build `3962616c`. All Cargo commands used the prescribed
`kontakto-heavy` wrapper with `CARGO_PROFILE_DEV_DEBUG=0` and
`CARGO_PROFILE_TEST_DEBUG=0`; no release build or full test suite was run.

## Focused verification

| Check | Result |
| --- | --- |
| `step_envelope_reads_saved_indexed_levels_and_native_defaults` | Failed before getter implementation; passed afterward. |
| `uvi_missing_module_and_approximation_name_the_source_path` | Failed before diagnostic formatting change; passed afterward. |
| `image_geometry_uses_intrinsic_size_only_on_omitted_axes` | Failed before image sizing change; passed afterward. |
| `image_resource_headers_are_cached_across_snapshots` | Passed; failing-first direction not checked. |
| `image_headers_reject_truncation_empty_and_excessive_sizes` | Passed; failing-first direction not checked. |
| `cargo test -p sampler-uvi --test image_geometry --test step_parameters` | Both focused integration tests passed together. |
| `cargo check -p sampler-uvi --no-default-features` | Initially failed because pure XML `promote` was bank-feature gated; passed after removing those gates. |
| `survey_ao_callback_attribution` (ignored, explicitly selected) | Passed once; debug core render p50 737 us, p99 3731 us. See the attribution note for measurement limits. |
| `cargo test --workspace --no-run` | Passed, exit 0; 43.77 s reported compilation time. No workspace tests executed. |

The workspace compilation gate preceded the final feature-off guard repair.
That repair leaves the default-feature helper body unchanged, and the final
feature-off check passed. Later edits were comments, test formatting and this
receipt; the workspace gate was not repeated under the one-run rule.

The callback measurement is not a before/after host comparison. No corrected
VWinds level, native UVI recording, frozen-v1 audio A/B, or new AO screenshot
was measured. DSP behavior was not changed without a verified law. Indexed
StepEnvelope reads and image geometry use focused fixtures; binary and vendor
evidence are recorded in the adjacent investigation notes.

Own scratch logs and build target are removed at handoff. No library files,
integration worktree files, frozen binaries, or survey outputs were modified.
