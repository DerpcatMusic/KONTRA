# 09 Build: compile time as a design constraint

Baseline (00-BASELINE.md): a one-constant edit in `sampler-core` costs
112.6 s of release rebuild and 15.9 s for core tests. Three reasons:
release has no incremental compilation, `sampler-core` is a 134k-line unit,
and the internal graph is a serial chain ending in a root crate that
always compiles the GPU UI.

## 1. Levers, in order of payoff

1. **Split the graph** (01-LAYOUT.md). A DSP edit rebuilds one family plus
   registry, voice, engine, host and plugin. The family crates are 1.5–4k
   lines and compile in parallel. Expect the core-edit chain to drop from
   ~5 serial heavy units to ~2 small and ~2 thin ones.
2. **Keep the UI out of engine work.** Engine, DSP, script and format
   tests depend on no UI crate, so `ash`, `naga`, `wgpu-core`, `x11rb` and
   `zbus` (~35 s cold) never build for them.
3. **A `fast` profile** for local listening and probe runs, alongside
   `release`:
   ```toml
   [profile.fast]                # local "release-like" iteration
   inherits = "release"
   lto = false
   codegen-units = 256
   incremental = true
   strip = "none"
   debug = "line-tables-only"
   ```
   `release` (thin LTO) and `maxperf` stay for shipping and benchmarks. The
   probe contracts (10-TESTING.md) run on `fast`. CPU numbers for the gate
   come from `release`.
4. **Use the existing `.cargo/fast-linux.toml` (clang + mold) by default
   locally.** Linking the plugin is a large share of the 16.7 s root rebuild.
5. **No workspace-hack crate (decided in P0.4, #787).** Building
   `-p sampler-core` then the root lib recompiled 15 small dependencies
   (serde, syn, rustfft, ...) for a second feature set. Both feature sets
   then stay cached in the target dir and in sccache, so it is a one-time
   cost per target dir. A hakari crate would need regenerating on every
   dependency change, so it costs more than it saves. Revisit only if
   `xtask timings` shows feature-set rebuilds on every edit.
6. **Fewer generics across crate boundaries.** Engine-facing APIs take
   concrete types (`AnyModule`, `&mut Block`). Monomorphization happens
   inside the crate that owns the type, not in every downstream crate.
7. **Cut macro cost.** Proc macros stay in one small crate with
   `syn` features trimmed. `modules!` expands once, in `kontra-dsp`.
8. **Lints that make the real-time rules mechanical.** New `kontra-*`
   crates inherit `[workspace.lints]`: clippy denies `unwrap_used`,
   `expect_used`, `panic`, `todo`, `unimplemented`, `unreachable` and
   `indexing_slicing` outside tests, and `unsafe_code` is denied (a SIMD or
   FFI module allows it locally, one `// SAFETY:` comment per block). A
   panic on the audio thread is a dropout or a crash in the host, so the
   compiler checks it rather than review.

## 2. Targets (re-measured at every migration gate)

| Measure | Now | Gate |
|---|---|---|
| Dev rebuild, DSP family function edit, `cargo test -p kontra-dsp-<family>` | 15.9 s (core tests) | ≤ 5 s |
| `fast` rebuild of plugin after a DSP edit | 112.6 s (release) | ≤ 25 s |
| `release` rebuild of plugin after a DSP edit | 112.6 s | ≤ 60 s |
| Engine-only test build pulls GPU crates | yes | no (checked by `xtask deps-check`) |
| Cold dev build | 45 s | ≤ 45 s (more crates, but parallel) |

## 3. `xtask`

```
tools/xtask/  (cargo xtask <cmd>, not shipped)
├── deps-check     parse `cargo metadata`; fail on any edge not in 01-LAYOUT.md §2, and on GPU crates under engine/dsp/script/format
├── size           production/test lines per crate (00-BASELINE.md table), fails if a crate > 8k or a file > 800 without an allow entry
├── timings        cargo build --timings for the gate edits above, writes a table into docs/architecture-v3/00-BASELINE.md
├── debt           counts: #[path], allow, Processor::, cfg(test) in prod fns, Mutex in RT crates
└── ci             the one local gate: rustfmt --check and clippy on changed files, tests of changed packages, deps-check, debt --check
```

Debt and size are ratchets: `debt-baseline.toml` is committed, the `v3 plan`
check fails a PR that grows any counter, and `--update` tightens it when a
counter falls. Production lines may grow only with the `size-growth` label.

## 4. CI (GitHub runners, per memory: CI on GitHub runners)

- Matrix per layer: `base+model`, `dsp`, `engine+script`, `format`, `app+plugin`.
  Every job runs `cargo test -p <crates>` with `sccache` and the `Swatinem/rust-cache`
  keyed by `Cargo.lock`.
- `xtask deps-check`, `xtask debt` (must not grow) and the contracts on the
  native-format fixtures (10-TESTING.md) on every PR.
- The plugin bundle build and real-library surveys stay in nightly, not per-PR.
- Locally, every cargo command still goes through `build-slot`.
