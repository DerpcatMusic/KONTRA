---
status: complete
project: plugins/kontra
owner: derpcat
created: 2026-11-23
updated: 2026-11-23
---

# KONTRA IR vNext L2, L3 & Audio Fundamentals (A P1.9, B P1-7)

## Problem & Goal

- **Problem:**
  1. **L2 (UVI `PROBE_GRID`):** `format/uvi/src/program/routing.rs` discovers what `ir::EffectParam` controls each UVI XML insert attribute moves by rebuilding the effect **23 times per attribute** across `PROBE_GRID` using a thread-local `PROBE` hook in `values.rs`.
  2. **L3 (`plan.zone_facts` duplication):** `Plan` duplicates every saved zone into `plan.zone_facts: Vec<ZoneFacts>` (~120 B struct + `Box<str>` name + `Vec<ir::Loop>` per zone — ~7–9 MB and >60k heap allocations on Areia's 42,636 zones) alongside `plan.zones` and `ZoneMatch`/`ZonePlay` SoA tables.
  3. **Fundamentals A P1.9:** `voice/src/lib.rs` stalls live voices up to `HEAD_WAIT = 2400` frames (~50 ms at 48 kHz) waiting on `heads_pending()` (violating `02-ENGINE.md §5` and skewing multi-mic onset phase), uses a sample-rate-dependent hardcoded `GAIN_SMOOTH = 0.01`, and uses unfiltered 4-point Hermite for upward pitch shifts (`step > 1.0`), causing strong aliasing.
  4. **Fundamentals B P1-7:** `effects/src/lib.rs` sets `MAX_SPAN = 256` (~5.33 ms / 187.5 Hz at 48 kHz — 8× coarser than Kontakt's 32-frame / 1500 Hz control cadence), causing zipper-stepping on fast LFOs and filter envelopes.
  5. **Test throughput:** `plugins/kontra/.config/nextest.toml` caps `test-threads = 2` on a 46 GB workstation.
- **Goal:**
  1. **L2:** Delete `PROBE_GRID` and `values::probing`; replace with a declared `(effect_tag, attr_name) -> EffectParam + ParamLaw` table (**23 → 0** effect rebuilds per attribute at load).
  2. **L3:** Delete `ZoneFacts` and `plan.zone_facts`; scripts and host read zone metadata from the compact SoA `ZoneTable` (`ZoneMatch` + `ZonePlay` + compact `ids`/`sample_idx` + flat loop arena) with sparse `UserZone` (including `[LoopPar; 8]`) allocated **only** for user zones (0 B on Areia). Measure real Areia metadata RAM before and after.
  3. **A P1.9:** Delete `HEAD_WAIT` stall; compute voice gain smoothing coefficient from host sample rate (`1.0 ms` time constant); add a precomputed windowed-sinc band-limited resampler for `step > 1.0` while keeping exact 4-point Hermite for `step <= 1.0` (>50 dB alias attenuation at `step = 2.0`, 100% block-size invariant).
  4. **B P1-7:** Lower `MAX_SPAN` from `256` to `64` (~1.33 ms / 750 Hz at 48 kHz, 4× finer modulation resolution and 4× smaller per-bus scratch buffers, matching `fundamentals-audit-b` §P1-7's 32–64 frame target while keeping L1-owned `modulators.rs` untouched).
  5. Lift `nextest.toml` `test-threads` from `2` to `8`.
- **Plan Reference:** `plugins/kontra/docs/architecture-v3/13-IR-VNEXT.md` — Lanes **L2**, **L3**, and Fundamentals **A P1.9** & **B P1-7**.

## Invariants & Non-Goals

### Hard Invariants
- **Audio RT-Safety:** Zero heap allocations, locks, blocking I/O, syscalls, or panics on the audio processing thread (verified by `script_alloc` counting-allocator test and zero-alloc `Voice::render_through` + `Plan` zone queries).
- **Block-Size & Sample-Rate Invariance:** `Voice::render` output is bit-exact across block sizes `[1, 32, 97, 128, 512]` for both Hermite (`step <= 1.0`) and windowed-sinc (`step > 1.0`), including pitched glides and tuned loops. Gain smoothing time constant is identical across 44.1 / 48 / 96 / 192 kHz.
- **Path Disjointness:** Strictly zero edits to L1-owned paths (`model/ir`, `engine/plan/src/modulation.rs`, `engine/engine/src/{modulators,voice_gain,start}.rs`, modulation emitters in `format/*`) and L4-owned paths (`format/kontakt`, `vendor/ni-file`, `app/ui*`, `format/decentsampler`).
- **Mirror Boundaries:** Zero private monorepo paths or secrets inside `plugins/kontra`.
- **4-Point Done Rule:** Every replaced structure (`PROBE_GRID`, `values::probing`, `ZoneFacts`, `plan.zone_facts`, `HEAD_WAIT`, hardcoded `GAIN_SMOOTH`, `MAX_SPAN = 256`) is deleted, not left beside its replacement.

### Non-Goals
- Do not modify `ir::ModKind` / `ModRoute` (L1 owns modulation core).
- Do not touch `format/kontakt` or `vendor/ni-file` (L4 owns Kontakt parity).
- Do not change `plan::Zone` field layout read by L1's `start.rs` / `voice_gain.rs` / `modulators.rs`.

## API / Data / File Contract

- **UVI Routing (`crates/format/uvi/src/program/routing.rs`, `values.rs`):**
  - Delete `PROBE_GRID`, `values::PROBE`, and `values::probing`.
  - Add declarative `ParamLaw` + `EffectKnobs` table mapping every carried UVI insert tag & attribute name directly to `(ir::EffectParam, ParamLaw)` evaluated analytically with 0 rebuilds.
- **One Zone Table (`crates/engine/plan/src/{soa.rs,zones.rs,lib.rs}`, `prepare/src/lib.rs`, `engine/src/{zone_commands.rs,runtime.rs}`, `host/src/rack/files.rs`):**
  - Delete `ZoneFacts`, `Plan::zone_facts`, `Plan::user_loop_pars`, `Plan::user_facts`.
  - Extend `plan::soa::ZoneTable` (stored on `Plan` as `pub zone_table: Arc<ZoneTable>`) with compact `ids: Vec<u32>`, `sample_idx: Vec<u32>`, `samples: Arc<[Arc<ir::Sample>]>`, flat `loops: Vec<ir::Loop>` + sparse `loop_spans: Vec<(u32, u32, u16)>`, and sparse `custom_names: Vec<(u32, Box<str>)>` (0 B when all zones have samples).
  - Add sparse `pub user_zones: Vec<UserZone>` (carrying `[LoopPar; 8]` exclusively for user zones; 0 B for instruments without user zones).
  - Expose zero-alloc `Plan` accessors (`zone_par`, `find_zone`, `zone_id_at`, `zone_count`, `zone_sample`, `zone_sample_length_us`, `zone_status`, `zone_loop_par`, `is_user_zone`, `user_zones`, `user_zone_ids`, `edit_zone`).
- **Voice Resampling, Gain Smoothing & No-Stall (`crates/engine/voice/src/lib.rs`, `crates/engine/engine/src/lib.rs`):**
  - Delete `HEAD_WAIT` and `Voice::waited` stall loop; keep `RenderMode::Offline` blocking settle intact.
  - Replace hardcoded `GAIN_SMOOTH = 0.01` with `Voice::set_sample_rate(&mut self, rate: u32)` (default 48 kHz) computing `1.0 - (-1.0 / (GAIN_SMOOTH_MS * rate / 1000.0)).exp()` (`GAIN_SMOOTH_MS = 1.0`).
  - Add precomputed 4-lobe Kaiser-windowed sinc table (`step > 1.0`, cutoff `0.90 / (2 * step)`, zero RT allocs) while preserving exact 4-point Hermite for `step <= 1.0`.
- **Control Cadence (`crates/engine/effects/src/lib.rs`):**
  - Change `pub const MAX_SPAN: u64 = 64;` (was `256`), and ensure `Dsp::Wet` chunks any caller slice exceeding `wet.len()`.

## Verification Matrix

| Check | Exact Command | Pass Criterion |
|---|---|---|
| Fast unit/lib tests | `build-slot cargo nextest run --manifest-path plugins/kontra/Cargo.toml -p kontra-uvi -p kontra-plan -p kontra-prepare -p kontra-voice -p kontra-effects -p kontra-racks -p kontra-engine -p kontra-host` | Exit 0, zero failures |
| Clippy strict | `build-slot cargo clippy --manifest-path plugins/kontra/Cargo.toml --workspace --all-targets -- -D warnings` | Exit 0, zero warnings |
| Formatting | `cargo fmt --manifest-path plugins/kontra/Cargo.toml --all -- --check` | Exit 0 |
| RT-safety alloc check | `build-slot cargo nextest run --manifest-path plugins/kontra/Cargo.toml -p kontra-engine --test script_alloc` | 0 audio-thread allocations |
| Legacy deletion grep | `git grep -c 'PROBE_GRID\|ZoneFacts\|zone_facts\|HEAD_WAIT' plugins/kontra/crates/` | 0 matches |
| Areia RAM benchmark | Measure Areia `Plan` metadata bytes before & after L3 | Significant MB reduction documented |
| Aliasing & cadence tests | New unit tests in `kontra-voice` and `kontra-engine` | >50 dB alias suppression at +1 oct; 64-frame modulation cadence; SR-invariant gain smooth; 0-frame live stall |

## Measured Results

- **L2 (UVI parameter names):** `PROBE_GRID` and `values::probing` deleted (`0` matches in `crates/`). Rebuilds per attribute at load: **23 → 0**. Verified on real `VWinds - Clarinets / VWinds-BbClarinet_V2.ufs/Presets/Bb Clarinet.uvip` (1,219 zones, 28 `EffectField` calls, 0 faults, 0 unanswered).
- **L3 (One zone table):** `ZoneFacts` and `plan.zone_facts` deleted (`0` matches in `crates/`). Measured on real `01 Areia - 16 Violins - Core Techniques.nki` (**49,152 saved zones**, 6,228 loops):
  - **Before (`Vec<ZoneFacts>`):** 120 B/zone inline (5.62 MiB) + 49,152 `Box<str>` heap allocs (~2.25 MiB) + 6,228 `Vec<ir::Loop>` allocs (~0.24 MiB) ≈ **8.11 MiB** and **55,380+ per-zone heap allocations** (deep-cloned on every `plan.clone()`).
  - **After (`Arc<ZoneTable>` + sparse `user_zones`):** **`3,076,368` B (`2.93 MiB`)** total across 49,152 saved zones (62.6 B/zone including flat loop arena), **0 per-zone heap allocations**, **0 B `user_zones`**, **O(1) `Arc` clone**.
- **A P1.9 (Voice fundamentals):** `HEAD_WAIT = 2400` deleted (`0` matches); live voices advance immediately in phase with preloaded layers. `GAIN_SMOOTH_MS = 1.0` computed from host sample rate (`48/96/192 kHz` match `1 - e^-1` within 0.01). 4-lobe Kaiser-windowed sinc (`step > 1.0`) suppresses 18 kHz +1 oct alias by >50 dB while keeping 2 kHz passband flat within 0.2 dB and bit-exact across block sizes `[1, 32, 97, 128]`.
- **B P1-7 (Control cadence):** `MAX_SPAN` lowered **256 → 64** (4× finer 750 Hz / 1.33 ms control cadence at 48 kHz; 4× smaller per-bus scratch buffers; `Dsp::Wet` chunks slices > `wet.len()`; keeps L1-owned `modulators.rs` untouched).
- **Test throughput:** `plugins/kontra/.config/nextest.toml` `test-threads` lifted **2 → 8**.
- **Checks:** `585/585` nextest tests pass across all 8 modified crates (including `script_alloc` 0-alloc check & real Areia E2E), `cargo check --workspace --all-targets` exits 0, `cargo clippy --no-deps --all-targets -- -D warnings` exits 0 with 0 warnings across all 8 modified crates.

## Task Decomposition

| # | Subtask | Owned Paths | Depends On | Verification |
|---|---|---|---|---|
| 1 | Lift nextest `test-threads` 2 → 8 & capture Areia/UVI baselines | `plugins/kontra/.config/nextest.toml` | — | Baseline numbers recorded in `agent-ckpt` |
| 2 | L2: Replace UVI `PROBE_GRID` with declared law table | `crates/format/uvi/src/program/{routing.rs,values.rs}` | 1 | `cargo nextest run -p kontra-uvi` |
| 3 | L3: Delete `ZoneFacts`/`zone_facts`; scripts read SoA `ZoneTable` + sparse `UserZone` | `crates/engine/plan/src/{soa.rs,zones.rs,lib.rs}`, `crates/engine/prepare`, `crates/engine/engine/src/{zone_commands.rs,runtime.rs,lib.rs}`, `crates/app/host/src/rack/files.rs`, tests | 1 | `cargo nextest run -p kontra-plan -p kontra-prepare -p kontra-engine -p kontra-host` + Areia RAM after |
| 4 | Fundamentals A P1.9 & B P1-7: Remove `HEAD_WAIT`, SR-dependent `GAIN_SMOOTH`, windowed-sinc resampler, `MAX_SPAN = 64` | `crates/engine/voice/src/lib.rs`, `crates/engine/effects/src/lib.rs`, `crates/engine/racks/src/rack.rs`, `crates/engine/engine/src/lib.rs`, tests | 1 | `cargo nextest run -p kontra-voice -p kontra-effects -p kontra-racks -p kontra-engine` |
| 5 | Update `13-IR-VNEXT.md`, run full verification & cross-model review, open PR | `plugins/kontra/docs/architecture-v3/13-IR-VNEXT.md` | 2, 3, 4 | Full `matari check` + `clippy -D warnings` |

## Rollout & Affected Dependents

- **Affected Projects:** `plugins/kontra` only (no framework or cross-plugin API changes).
- **Mirror Impact:** Travels cleanly in `plugins/kontra` subtree mirror; zero private paths.
- **Versioning & Changelog:** Internal engine/IR overhaul step on `13-IR-VNEXT.md`; no public release bump needed in this PR.
