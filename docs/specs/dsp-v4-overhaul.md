---
status: implemented
project: kontra
owner: antigravity
created: 2026-11-27
updated: 2026-11-27
proof_scope: internal
evidence_freshness_days: 180
---

# KONTRA DSP v4 Overhaul — Moldable Families, Honest Parity & Write-DSP-Once Enforcement

## Problem & Goal

- **Problem:** A forensic audit of `plugins/kontra` (~184,222 prod LOC across 47 crates; ~70,261 LOC across DSP, IR, effects, racks, and format translators) identified five structural defects:
  1. **Dead-code oracle proofs & parity blindspots:** `crates/engine/effects/src/measured_dsp.rs` (1,290 LOC: `EnhancedCompressor`, `SolidBusComp`, `SolidPassFilters`, `StereoModeller`) had zero production callers, yet `docs/parity/snapshots/dsp-oracle.tsv` cited its unit tests as `tested` for `compressor`, `solid_g_eq`, and `stereo_modeller`. Meanwhile `tools/parity-board.py` missed the 5 `mangling::*::MODULE` entries in `MODULES`, granted a free pass on `.driven` for `model(...)` entries with no binding TSV, and lacked provenance/verification-kind separation (`binary-re` vs `synthetic-approx`, `waveform-vector` vs `sanity`/`unit`).
  2. **Primitive duplication across DSP crates:** `flush`/`flush32` (15 files), one-pole smoothers, cookbook `peak`/`low_pass` biquads, delay ring buffers (`DelayLine`/`Ring`/`FixedRing`), `db_to_lin`, and Catmull-Rom interpolation were re-implemented locally instead of using `moose-dsp` and `kontra-dsp-core`.
  3. **Four-way effect boilerplate mirror:** Adding or changing an effect required editing verbose per-effect byte decoders & KSP rows in `format/kontakt-effects`, 44-variant `ir::EffectKind`, manual 1-to-1 field-copy glue + `Box<dyn Wet>` in `engine/effects/src/processors.rs`, and a narrow `engine/racks/src/voice_rack.rs` `enum Kind`.
  4. **Fragmented, un-oversampled, and bus-only DSP cores:** Separate algorithmic reverbs + `Convolver` with no diffusion-only or hybrid morph surface, 1x-rate `tanh`/diode clipping in `amp_stack.rs` and `color_sat.rs`, a Kontakt tube LUT embedded in `crates/dsp/drive/src/tube.rs`, trivial pass-through crates (`crates/dsp/eq`, empty 1-line `crates/dsp/modulators`), and a `VoiceRack` missing `EqPass`, `Wah`, `SpectralMorph`, and `ColorSat`.
  5. **Permissive architectural guards:** `arch_guards.rs` did not ban local primitive duplication, hand-rolled byte decoders, dead DSP cores, `Box<dyn Wet>`, or production LOC regressions.

- **Goal:** Execute an onion-layered overhaul inside `plugins/kontra` delivering:
  1. **Honest, bulletproof parity measurement:** Zero dead-code oracle proofs; all 59 Kontakt modules visible on `parity-board.py`; provenance and test-kind columns in `dsp-oracle.tsv`; verified `Detector::Enhanced`, `Detector::Bus { auto_release }`, and `PassBank` (18 dB/oct Butterworth HP + 12 dB/oct Vicanek LP) wired into real production DSP (`ir::EffectKind::EqPass` + `VoiceEq`).
  2. **Single canonical primitive layer (`kontra-dsp-core` on `moose-dsp`):** Shared `DelayLine`, `FixedRing`, `Smoother`, `Biquad` (`peak`, `low_pass`, `high_pass`, `shelf`, `first_order_hp`, `second_order_hp`, `vicanek_lp`), `Halfband`, `eq`, `flush`/`flush_f32`/`flush_f32_slice`, and `DspCore` contract macro — mechanically enforced by `arch_guards.rs`.
  3. **Moldable composable DSP families:** Static-dispatch `Space` (molding across pure stereo `Diffuser`, algorithmic `Fdn`/`Combs`/`Hadamard`/`Network`/`Plate`, partitioned-FFT `Convolver`, and zero-alloc `Space::Hybrid` IR↔FDN morph), `Dynamics` (`Compressor`, `Limiter`, `Transient`), and `Drive` (2x `Halfband`-oversampled `AmpStack`/`ColorSat` sharing `DcBlocker` + `ToneStack3`, `Cabinet` on `DelayLine`, Kontakt tube LUT moved out to `crates/format/kontakt/src/drive.rs`).
  4. **Static-dispatch `DspUnit` + zero-alloc `VoiceRack`:** Eliminate `Box<dyn Wet>` and collapse `processors.rs` 1-to-1 field-copy glue via `map_fields!`, while expanding stack-only zero-heap-allocation `VoiceRack` (`rt-paranoid` safe on `extend_from_slice`) to cover `EqPass`, `Wah`, `SpectralMorph`, and `ColorSat` reusing `processors.rs`/`summed.rs` constructors.
  5. **Dramatically smaller, cleaner, idiomatic Rust codebase:** Declarative `define_effect!`, `Cursor`, `KnobField`, and `knob_row` across `format/kontakt-effects` (-2,107 prod LOC / -29.5%), `engine/effects` (-1,559 prod LOC / -49.9%), deleted `measured_dsp.rs`, `crates/dsp/eq`, `crates/dsp/modulators`, and `crates/dsp/drive/src/tube.rs`, tightened `arch_guards.rs` ratchets, and 100% workspace test + clippy + RT-safety compliance.

- **Plan Reference:** Advances `plugins/kontra/docs/architecture-v3/13-IR-VNEXT.md` (DSP consolidation, IR unification, and verification hardening).

## Primary-Source & Version Proof

N/A — internal refactor: all changes are internal to `plugins/kontra` DSP, IR, engine, format translators, and parity tooling; no external OS, GPU, plugin-format ABI, or third-party protocol contracts change.

## Pinned Crate & SDK Ground Truth

| Dependency / Tool | Pinned Version | Lockfile / Header Source | Verified Symbols / Features |
|---|---|---|---|
| `moose-dsp` | `7.0.0` | `plugins/kontra/Cargo.lock` | `biquad::{Biquad, Coeffs, Side, cookbook}`, `smooth::OnePole`, `svf`, `follower`, `time`, `window`, `fft`, `flush`, `Float` |
| `smallvec` | `1.16.2` | `plugins/kontra/Cargo.lock` | `SmallVec` stack-backed stage/tap vectors for RT-safe DSP chains |

## Invariants & Non-Goals

### Hard Invariants
- **Audio RT-Safety:** Zero heap allocations, locks, blocking I/O, syscalls, or panics inside any `process`, `process_sidechain`, `set`/`retune`, or `reset` path. Every unified family passes block-size invariance (`[1, 7, 64, 256]`), zero-NaN/Inf stability, and silence/reset contract tests (`crates/dsp/registry/tests/dsp_contract.rs`).
- **Determinism & Compatibility:** All existing Kontakt, UVI, SFZ, SF2, DecentSampler, and Native patch translation tests, oracle audio tests, and golden vectors pass (including bit-exact `kontra-dsp-reverb::network::the_reference_render_is_unchanged` and live Solid G-EQ v0x11 pass-filter tests).
- **Mirror Boundaries & RE Hygiene:** `plugins/kontra` mirrors publicly to `DerpcatMusic/KONTRA`. Zero references to private monorepo paths, local filesystem paths, decompiler symbols (`FUN_`, `DAT_`, `LAB_`, `0x14...`), or private plugin DSP enter `plugins/kontra`.
- **Single-Project Scope:** All changes remain self-contained inside `plugins/kontra` (building on `moose-dsp` via `kontra-dsp-core`) so sibling plugins are unaffected.
- **Write DSP Once:** Every shared audio-processing DSP primitive lives in `moose-dsp` or `kontra-dsp-core`; every effect algorithm lives in `crates/dsp/*`; zero dead standalone oracle DSP cores live in `crates/engine/*`.

### Non-Goals
- Modifying `frameworks/moose`, `frameworks/mui`, or any other plugin in the monorepo.
- Rewriting KSP/Lua VM execution or UI layout/rendering (`crates/app/ui`, `crates/script/*`).
- Adding new binary reverse-engineering captures from proprietary binaries not already documented in existing test vectors.

## API / Data / File Contract

- **`docs/parity/snapshots/dsp-oracle.tsv`:** Extends the 5-column TSV (`effect_id`, `test_name`, `max_err_db`, `status`, `commit`) with `provenance` (`binary-re`, `physics`, `black-box-lut`, `synthetic-approx`) and `kind` (`waveform-vector`, `spectrum-sweep`, `scalar-spot`, `sanity`, `unit`). `tools/parity-board.py --check` tracks all 59 Kontakt modules, rejects dead-code/unreferenced module proofs (`measured_dsp.rs` banned), removes the `.driven` free pass, and enforces provenance/kind honesty.
- **`kontra-dsp-core`:** Re-exports `moose_dsp` primitives (`Side`, `Coeffs`, `cookbook`, `flush`) and provides canonical KONTRA building blocks (`DelayLine`, `FixedRing`, `hermite`, `Smoother`, `Biquad`, `peak`, `low_pass`, `high_pass`, `shelf`, `first_order_hp`, `second_order_hp`, `vicanek_lp`, `Halfband`, `eq`, `flush_f32`, `flush_f32_slice`, and `DspCore` trait + `impl_dsp_core!` + `assert_block_invariant_and_finite`). Dead `units.rs` removed.
- **Unified Moldable DSP Families (`crates/dsp/*`):**
  - `kontra-dsp-dynamics`: Static-dispatch `Dynamics` enum (`Compressor` with `Input`/`Output`/`Trigger`/`Enhanced`/`Bus { auto_release }` detectors, `Limiter`, boxed `Transient`).
  - `kontra-dsp-reverb`: Static-dispatch `Space` enum molding across pure stereo `Diffuser` (4-stage Schroeder allpass cloud, zero feedback tank), algorithmic tanks (`Fdn`, `Combs`, `Hadamard`, `Network`, `Plate` — all consolidated onto `kontra_dsp_core::{DelayLine, FixedRing}`), partitioned-FFT `Convolver`, and zero-alloc `Space::Hybrid { early: Convolver, late: Fdn, blend }`.
  - `kontra-dsp-drive`: Static-dispatch `Drive` enum (`Shaper`, `Overdrive`, boxed `Clipper`, `AmpStack`, `ColorSat`, `Cabinet`, `Crush`); shared `DcBlocker` + `ToneStack3` stages in `stage.rs`; 2x `Halfband` oversampling around `AmpStack` and `ColorSat`; `Cabinet` early reflections on `DelayLine`; Kontakt 4x97 tube LUT moved out of `crates/dsp/drive` into `crates/format/kontakt/src/drive.rs` via generic `DriveCurve::AsymBlend`.
  - `kontra-dsp-filter`: `PassBank` (18 dB/oct Butterworth HP + 12 dB/oct Vicanek LP, wired into production `ir::EffectKind::EqPass` / `VoiceEq`), const-generic `Svf`, `Wah`, `SpectralMorph`, `Formant`, `Notches`, `Cascade`, `Clocked`.
  - `kontra-dsp-delay`: `DualDelay` consolidated onto `DelayLine`, shared `CaptureRing`/`Smoother`/`hermite` across `ModDelay`, `CombBank`, `Shifter`, `Scanner`, and `mangling`.
  - Trivial/empty crates `crates/dsp/eq` (folded into `kontra-dsp-core::eq`) and `crates/dsp/modulators` deleted.
- **Static-Dispatch `DspUnit` & Expanded `VoiceRack` (`crates/engine/effects` + `crates/engine/racks`):** Static-dispatch `enum DspUnit` replaces `Box<dyn Wet>` and collapses 1-to-1 field-copy boilerplate via `map_fields!` (`engine/effects/src`: `3,127 -> 1,568` prod LOC). Stack-only zero-heap-allocation `VoiceRack` adds per-voice `EqPass`, `Wah`, `SpectralMorph`, and `ColorSat`, reusing `processors.rs` & `summed.rs` constructors directly.
- **Declarative Format Translators (`crates/format/kontakt-effects`):** `define_effect!`, `Cursor`, `KnobField`, and `knob_row` eliminate hand-rolled byte decoding and KSP row boilerplate (`7,143 -> 5,036` prod LOC; zero inline `from_le_bytes` outside `module.rs`).
- **Mechanical Guards (`crates/engine/engine/tests/arch_guards.rs`):** Bans `measured_dsp.rs`, `units.rs`, `dyn Wet`/`trait Wet`, `crates/dsp/eq`, `crates/dsp/modulators`, local `flush`/`flush32`/`db_to_lin`/cookbook biquads/local delay rings outside `crates/dsp/core`, inline `from_le_bytes` outside `kontakt-effects/src/module.rs`, requires `PassBank` in production `summed.rs`, enforces 0 vendor words in `dsp`, and ratchets prod LOC (`engine/effects <= 1,600`, `format/kontakt-effects <= 5,080`, `crates/dsp <= 16,320`).

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/dsp-v4-overhaul.md` | Exit 0, zero proof errors | PASS (2026-11-27, linux-x86_64, `2da38132cf`) |
| Stack & mirror boundary check | Doc | any | `python3 tools/sync-stack.py --check` | Exit 0, zero boundary/RE/manifest violations | PASS (2026-11-27, linux-x86_64, `2da38132cf`, 19/19 projects clean, 4/4 mirrors ready) |
| Parity board validation | Doc | any | `python3 plugins/kontra/tools/test_parity_board.py && python3 plugins/kontra/tools/parity-board.py --check` | Exit 0, all 59 modules tracked, 0 dead proofs, 0 flagged claims | PASS (2026-11-27, linux-x86_64, `2da38132cf`, 0 flagged claims) |
| Bitwig migrator unit tests | Validator/Runtime | any | `python3 plugins/kontra/tools/test_migrate_bitwig_auto.py` | Exit 0, all migrator tests pass | PASS (2026-11-27, linux-x86_64, `2da38132cf`) |
| DSP contract suite | Validator/Runtime | linux-x86_64 | `build-slot cargo nextest run --manifest-path plugins/kontra/Cargo.toml -p kontra-dsp --test dsp_contract` | Exit 0, 14 processors + `Space` diffusion/hybrid morph pass block invariance, finiteness & reset | PASS (2026-11-27, linux-x86_64, `2da38132cf`) |
| Architecture & ratchet guards | Validator/Runtime | linux-x86_64 | `build-slot cargo nextest run --manifest-path plugins/kontra/Cargo.toml -p kontra-engine --test arch_guards` | Exit 0, primitive/ring/decoder/dead-code/vendor/LOC ratchets pass | PASS (2026-11-27, linux-x86_64, `2da38132cf`, vendor words = 0, effects=1568<=1600, ke=5036<=5080, dsp=16314<=16320) |
| Full PR verification gate | Validator/Runtime | linux-x86_64 | `matari check origin/main --project kontra` | Exit 0: fmt, dbg/todo guard, full workspace nextest, `rt-paranoid`, clippy `-D indexing-slicing` green | PASS (2026-11-27, linux-x86_64, `2da38132cf`, `verified: plugins/kontra`) |

## Unverified Boundaries & Promotion Gate

None — all verification tiers execute on the current host.

**Hard Promotion Rules:**
1. This spec **MUST NOT** move to `status: implemented` while any required row in `## Verification Matrix` is `PENDING`, `PENDING_OS_CI`, or `BLOCKED`, or while any unchecked `- [ ]` acceptance criterion remains.
2. Each phase commits and pushes cleanly on `kontra-dsp-v4-overhaul` only after its scoped verification commands pass.

## Task Decomposition

| # | Subtask | Owned Paths | Depends On | Verification |
|---|---|---|---|---|
| 0 | Formal SDD spec & worktree setup | `plugins/kontra/docs/specs/dsp-v4-overhaul.md` | — | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/dsp-v4-overhaul.md` |
| 1 | Phase 1: Truth & dead-code purge (`measured_dsp.rs` wired into real DSP & deleted; `parity-board.py` & `dsp-oracle.tsv` hardened; `PassBank` wired into Solid G-EQ v0x11) | `plugins/kontra/tools/parity-board.py`, `plugins/kontra/docs/parity/snapshots/dsp-oracle.tsv`, `plugins/kontra/crates/engine/effects`, `plugins/kontra/crates/dsp/dynamics`, `plugins/kontra/crates/dsp/filter`, `plugins/kontra/crates/model/ir`, `plugins/kontra/crates/format/kontakt` | 0 | `python3 plugins/kontra/tools/parity-board.py --check && matari fast kontra` |
| 2 | Phase 2: Canonical `kontra-dsp-core` primitives (`DelayLine`, `FixedRing`, `Smoother`, `Biquad`, `Halfband`, `DspCore`), contract suite & mechanical `arch_guards.rs` | `plugins/kontra/crates/dsp/**`, `plugins/kontra/crates/engine/engine/tests/arch_guards.rs` | 1 | `build-slot cargo nextest run --manifest-path plugins/kontra/Cargo.toml -p kontra-dsp --test dsp_contract -p kontra-engine --test arch_guards` |
| 3 | Phase 3: Moldable DSP families (`Space` with `Diffuser` & `Hybrid`, `Dynamics`, `Drive` with 2x `Halfband` + `DcBlocker`/`ToneStack3` + LUT moved to format layer) | `plugins/kontra/crates/dsp/**`, `plugins/kontra/crates/model/ir`, `plugins/kontra/crates/engine/effects`, `plugins/kontra/crates/format/**` | 2 | `matari fast kontra` |
| 4 | Phase 4: Static-dispatch `DspUnit`, `map_fields!` glue collapse & expanded zero-alloc `VoiceRack` | `plugins/kontra/crates/engine/effects`, `plugins/kontra/crates/engine/racks`, `plugins/kontra/crates/engine/engine` | 3 | `matari fast kontra` |
| 5 | Phase 5: Declarative `kontakt-effects` (`define_effect!`, `Cursor`, `knob_row`), crate pruning, branch sync (`origin/main` + headless restore & Bitwig migrator) & final verification | `plugins/kontra/crates/format/**`, `plugins/kontra/Cargo.toml`, `plugins/kontra/crates/engine/engine/tests/arch_guards.rs` | 4 | `matari check origin/main --project kontra && python3 tools/sync-stack.py --check && python3 plugins/kontra/tools/parity-board.py --check` |

## Rollout & Affected Dependents

- **Affected Projects:** `kontra` only (`matari affected origin/main` reports `kontra`).
- **Mirror Impact:** Subtree mirror (`plugins/kontra` -> `DerpcatMusic/KONTRA`); spec lives inside `plugins/kontra/docs/specs/` and obeys all public mirror hygiene rules.
- **Versioning & Changelog:** Internal architectural overhaul; version bump deferred to release lane.
