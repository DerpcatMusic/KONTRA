# Legacy EQ: init-only group gain admission — SOURCE ONLY

Baseline: `13c044d6f9f80576507521a2dc6521e9aad7d9c5` (CURRENT13), not the historical `366515…` handoff source.

## One missing route and bounded fix

An `on init`-only `$ENGINE_PAR_GAIN1..3` write to a group EQ was ignored by `effects::apply_writes`. `Initialized::writes_effect_slots` excludes `Init`, so without EQ modulation or another runtime writer the group stayed on the static path. The existing live `eq_controls` default override did not cover that path.

Three group-only cases now change the existing serialized dB field before translation. The private `Slot::apply_eq_gain` first requires the entire record to decode as `Params::Eq`, then rejects an absent band. Only four bytes change: two subtype i32s (8 bytes), physical-band triples (12 bytes each), gain at triple offset 8. Hz, octaves, other bands, subtype, slot levels and bypass remain unchanged. Exact group/slot/generic filtering is preserved. Instrument/send/main/bus requests, other module owners, unknown symbols, malformed records and non-finite stored fields remain outside the route.

The law is the EXISTING OWN law: `-18 + 36 * clamp(value,0,1000000)/1000000`. The manual establishes ±18 dB, not this precise normalized law or native shape. No new algorithm, parameter catalog promotion, runtime alias or public API is introduced.

## Current-source callers and custody

Unmodified CURRENT13 source:

- `library.rs:432` → `source_modulators` → `source_eq_values` (`929`, `934–967`) captures saved group EQ metadata BEFORE `load::initialize_scripts` (`465–466`). Working `Slot` edits do not change that separate saved-state capture.
- `sampler-ksp/src/lib.rs:981–1030`, `Initialized::writes_effect_slots`: runtime callbacks/functions only; `Init` excluded at `1027`.
- `library.rs:1282` calls `effects::apply_writes(&mut slots,&self.engine,index,-1)` BEFORE `voice_chain` (`1289`), using physical group and slot IDs.
- `effects.rs:377–397` decodes module `0x18`, types `22–24`, as exact finite Hz/octaves/dB triples. `chain_with:874–899` → `eq_band:1642–1673` translates the edited dB to an IR Peak; the flat static omission policy stays unchanged.
- `sampler-core/src/lower.rs:1953–1955` lowers static IR Peak to the existing peak biquad. Existing live group controls (`library.rs:1652–1748`) still lower separately to `PeakingEq` (`sampler-core/src/lower.rs:1697–1720`), whose DSP explicitly says it is not native EQ (`dsp/eq.rs:1–188`). Core files are unmodified.

Line references above are BASELINE references; the patch adds 21 production lines and two tests in `effects.rs`.

## Authored tests (NOT_RUN/NOT_COMPILED)

In `sampler-kontakt::effects::tests`:

- `init_only_group_eq_gain_writes_reach_static_bands`: real KSP initialization with no runtime writer; last init write; all three serialized layouts; preserved Hz/BW bytes; resulting static IR gain/frequency and retained `UnknownLaw` diagnostics.
- `init_eq_gain_writes_reject_wrong_owners_and_clamp_own_law`: exact group/slot/generic; instrument scope rejection; absent bands; non-exact names; out-of-range values; wrong module; short/long/repeated-ID-mismatch/non-finite records.

Existing `live_eq_retains_flat_bands_and_their_physical_indices`, library EQ routing/saved-metadata tests, and core `registered_peak_gain_keeps_a_flat_band_and_drives_real_voice_audio` / `registered_peak_frequency_and_width_use_normalized_knob_domains` were only inspected as source. They provide NO execution credit here.

## Permitted checks

`python audits/legacy-eq-init-gain/check.py`: 79 independent byte-model cases plus literal Rust caller/guard anchors. The CURRENT13 file fails the new source anchors (exit 1); patched source passes (exit 0). This is NOT Rust test execution, a Rust interpreter, audio validation or native comparison. `receipt.json` pins the checked effects SHA-256.

Installed `/home/derpcat/.rustup/toolchains/1.99.0-x86_64-unknown-linux-gnu/bin/rustfmt --edition 2024 --config skip_children=true --emit stdout crates/sampler-kontakt/src/effects.rs`: syntax/format parser only, exit 0. Only authored blocks were formatted; unrelated source was not reformatted. `git diff --check`: exit 0.

No cargo, rustc, clippy, builds, test executables, native apps/readers, protected banks, host/perf jobs, installs, publication, target/wrapper changes, nested delegates or schedules.

## Archived primary authority (hashes recomputed, files not modified)

Under coordinator `handoff/official-docs/`:

| File / section | Document SHA-256 | Excerpt SHA-256 |
|---|---|---|
| `kontakt-filters.html` / Equalizers | `733ec41ca14fb7040588308b8e3fd94d264a5113c4e53b10532fca4cf0e70aa5` | `bed829073e1ac45daef2fc2728067bfb8cc706f15c26eb97f9ae38584d436bbb` |
| `ksp-engine.html` / Filters and EQs | `4a15e3b10a9da8993c90ddc7681fee9fd62a7eafc5f3685f47b0c5ab37d1d16d` | `b2044f97a88e5ecf12559482be156a25d4fe01876d536ae9125dbb5002107684` |
| `ksp-engine-commands.html` / set_engine_par() | `df2fe7192f32df7c4a738c713663bafdfec3630c2821fd9afe1887c7d5ed006d` | `a2a5a3d2e35ba5d30e6a4b97d711a534ac4680b6032d64d1ac201c9bd78b3012` |
| Same document / get_engine_par() | same | `7a06aef94e70fc8b8949924d7a2111ace7f1e317b7227c3c4769f5d7c780b157` |

These are vendor requirements, NOT native execution. Historical/project claims are not used to establish native curves or sound.

## Limits

All authored Rust: **NOT_RUN/NOT_COMPILED**. Native: **UNKNOWN**. CPU vs v1 AND Kontakt: **UNACHIEVED**. RAM vs v1 AND Kontakt: **UNACHIEVED**.

Import/control-thread work only; decoding allocates at most three band triples per write. No audio-thread changes and no measured resource claim. Frequency/BW init-only admission, instrument/send/bus numbered-EQ bindings, full runtime/native custody and native shape verification remain unimplemented/unproven. A frozen raw source/snapshot is not rewritten. ROOT, existing ledgers and all existing manifests (including `official-docs/sections.json`) receive no worker edits. No automatic merge.
