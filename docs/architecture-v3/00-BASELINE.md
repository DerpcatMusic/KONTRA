# 00 Baseline (measured 2026-10-24, commit 08e7af698)

All numbers come from this worktree. Rerun the commands to update them.

The tool-measured baseline on v3 (`xtask size`/`debt`, `kontra-probe` C1–C10 on
the fixtures and four libraries) is [gates/G0.md](gates/G0.md), commit 9acb0f9c8.
Later gates compare against G0. The tables below are the earlier hand count.

## Size

Production vs test lines (test = `*test*.rs`, `tests/`, and trailing
`#[cfg(test)] mod` blocks; an approximation):

| Crate | Production | Test |
|---|---|---|
| `sampler-core` | 103,298 | 31,571 |
| root `kontakto` (`src/`) | 75,407 | 56,979 |
| `sampler-uvi` | 25,620 | 10,917 |
| `sampler-kontakt` | 23,063 | 17,327 |
| `sampler-ksp` | 21,943 | 1,960 |
| `sampler-ir` | 8,853 | 705 |
| `sampler-native` | 3,230 | 462 |
| `sampler-dspbench` | 2,691 | 57 |
| `sampler-perf` | 2,260 | 50 |
| `sampler-ui-ir` | 1,389 | 426 |
| `sampler-midi` | 1,445 | 56 |
| `sampler-pool` | 825 | 290 |
| `sampler-ikmpak`, `-script`, `-simd`, `-ample`, `-parity` | 1,071 | 598 |
| **Total** | **~271,100** | **~121,400** |

Inside the big ones:

- `sampler-core/src/dsp/`: 48,685 lines (filters 9 kinds, eq_family 4 variants,
  color 20 files, space, timemod 13 files, replika, machine, dynamics).
- `sampler-core` other: `blocks/` 5.2k, `source/` 5.1k, `receiver/` 4.9k, and
  single files `lower.rs` 3.9k, `voice_mod.rs` 3.7k, `zone_routing.rs` 3.4k,
  `source.rs` 3.4k, `prepare.rs` 3.3k, `behavior.rs` 3.2k, `lib.rs` 2.8k, `dsp.rs` 2.8k.
- root `src/ui/`: 72.9k lines in 71 files (`v2_tests.rs` 8k, `native_ui.rs` 4.4k,
  `ir_view.rs` 3.9k); `src/sound/`: 23.8k (`v2.rs` 10.2k); `src/plugin.rs` 5.6k.
- `sampler-kontakt`: `library.rs` 7.5k, `stream.rs` 5.2k (the stream service
  every format uses), `load.rs` 2.4k, `effects/` ~6.5k.
- `sampler-uvi`: `script.rs` 5.3k, `lib.rs` 4.1k, `modulation.rs` 2.3k,
  `scripted/thread.rs` 1.8k (Lua on its own thread).
- Docs: 479 markdown files; 5.9 MB of JSON/TSV vectors.

## Build times (this machine, sccache warm, `build-slot`)

| Action | Time |
|---|---|
| `cargo build --lib` (dev), deps cached | 45 s |
| Dev rebuild after a function-body edit in `sampler-core` | 5.4 s |
| Dev rebuild of the root test binary after a core edit | 6.0 s |
| `cargo test -p sampler-core --lib --no-run` after a core edit | 15.9 s |
| `cargo test --lib --no-run` (root), first after a dev build | 35.9 s |
| `cargo build --release --lib`, cold for the workspace | 149.6 s |
| **Release rebuild after a one-constant edit in `sampler-core`** | **112.6 s** |
| Release rebuild after a one-constant edit in root | 16.7 s |

Why: release has no incremental compilation, `sampler-core` is one
134k-line compilation unit, and the internal graph is a chain, so a core
edit recompiles core → ksp → kontakt → uvi → root in series:

```
sampler-ir ─▶ sampler-core ─▶ sampler-ksp ─▶ sampler-kontakt ─▶ sampler-uvi ─▶ kontakto (root, 132k incl. UI)
                 ▲ also: pool, simd          ▲ ui-ir, parity, script
```

Heaviest units in a cold dev build: `sampler-core` 10.1 s, `ash` 9.4 s,
`kontakto` 9.0 s, `sampler-ir` 7.4 s, `naga` 7.3 s, `mlua-sys` build script 6.8 s.
GPU/UI crates (`ash`, `naga`, `wgpu-core`, `zbus`, `x11rb`) compile even for engine-only tests,
because the root crate always pulls in the UI. 565 unique external crates.

## Structural debt (counts)

| Pattern | Count | Where |
|---|---|---|
| `Processor::` match sites | 824 in 102 files (xtask: 885) | two `Processor` enums: `sampler-ir` (~26 kinds) and `sampler-core/src/dsp.rs` (~28 kinds) |
| Files touched by one effect type (e.g. dynamics/EQ) | ~10 in 5 crates | IR, IR validate, core lower, core dsp, kontakt effects, uvi fx, UI chain, cpu receipt, v2.rs |
| `Runtime` struct fields | 109 | ~25 are counters; notes, voices, scripts, streaming and stats live side by side |
| Plugin `Shared` fields | 54 | 7 `ArrayQueue`, 19 `AtomicU64`, 9 `Mutex`: one ad-hoc channel per feature |
| `#[path]` attributes | 138 (xtask: 141) | module tree does not match files |
| `#[allow(...)]` | 127 (xtask: 111) | |
| Handwritten `impl Default` | 93 | |
| Error enums | 25 | |
| `pub struct` | 1,530 | |
| `#[cfg(test)]` instrumentation inside production functions | 45 in `v2.rs`, 31 in `plugin.rs` | `callback_audit_mark`, `onset_audit` timers in hot paths |
| "port v1" references | 152 | |
| `ponytail:` markers | 119 | known ceilings |
| Parameter registries | 5 | `engine_parameters.rs`, `parameter_registry.rs`, `script_params.rs`, `engine_par_ids.tsv`, `sampler-uvi/engine_parameters.rs` |
| KSP execution engines | 3 | `eval.rs`, `lower.rs`, `luau/` |
| Script runtimes | 2 | KSP bytecode on audio, Lua (mlua/Luau) on a script thread |
| Render functions in core | ~54 `fn render*` | scalar, batch, parallel, signal, slices, stretch... |

Already good, keep: the IR as the meeting point; allocation-free audio
thread; sample-accurate block splitting; native vectors for DSP laws; the
`builtins!` and `RuntimeProblems` macros (already declare-once, extend the idea).

Playback behaviour: see [../audit-2026-10-24/PLAYBACK_AUDIT.md](../audit-2026-10-24/PLAYBACK_AUDIT.md)
(Areia dense 0/64 chords audible, Vista 100% cold starts).
