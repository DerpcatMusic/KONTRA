# Modularity audit: engine, format modules, root plugin

Status: audit at `origin/main` 42db76a3a (2026-10-23). Measured with `rg` over `.rs` files;
LOC is `src/**` only unless noted. Scope: dependency direction, DSP that sits in format crates,
how the root plugin binds to formats, and what deleting a format would break. The DSP
migration plan belongs to `docs/architecture/DSP_MODEL.md` (GPT lane); this file lists the
leaks for it to consume.

## 1. Dependency direction (measured)

Crate edges from `crates/*/Cargo.toml` (`sampler-*` only):

| Crate | Depends on | Layer |
|---|---|---|
| sampler-ir | nothing | engine vocabulary |
| sampler-core | ir, simd, pool | engine |
| sampler-midi | core | engine |
| sampler-script, sampler-ui-ir, sampler-parity, sampler-simd, sampler-pool | nothing | shared |
| sampler-ksp | parity, core, ir, ui-ir, script | Kontakt script language runtime (`.nckp`), not a container |
| sampler-kontakt | parity, ir, core, **ksp**, ui-ir | format |
| **sampler-uvi** | ir, ui-ir, core, midi, parity, script, **kontakt** | format |
| sampler-ample, sampler-ikmpak | nothing | format stubs, unwired |
| sampler-native | core, midi, ksp, **kontakt**, ir, **uvi** | host glue |
| sampler-perf | core, midi, ir, **kontakt**, **uvi** | bench |
| root `kontosto` | core, ir, **kontakt (hard)**, midi, **uvi (hard)**, **ksp (hard)**, ui-ir, parity | plugin |

Findings:

- **Engine is clean at the crate level.** `sampler-core`, `sampler-ir`, `sampler-midi` have no
  dependency on any format crate. The only textual hits are a doc comment in
  `crates/sampler-core/src/lower.rs:674` and a string literal in `crates/sampler-ksp/src/parity.rs:33`.
  Nothing to fix here.
- **Format→format edges (violation, high).** `sampler-uvi` depends on `sampler-kontakt`
  (26 references in uvi `src` and tests). It uses Kontakt's *loading and streaming machinery*
  (`StreamPolicy`, `LoadError`, `Options`, `DynamicSources`, `SampleReader`,
  `stream_instrument`, `install_resident_source_reload`, `resident_storage_from_interleaved`,
  `audit::Span`), not Kontakt's format. Generic loaders therefore live in the Kontakt crate.
  Target: move them into a neutral loader/stream module (engine or `sampler-ir`) so uvi depends on
  engine only.
- **Kontakt depends on KSP.** `sampler-kontakt` references `sampler_ksp` 91 times (`src`).
  KSP is the Kontakt script language (23.6k LOC runtime, `.nckp` inside libraries), not a file
  container, so it is not a `FormatModule`. It belongs beside `sampler-script` as a shared script
  runtime that Kontakt calls, instead of being a dependency of the Kontakt reader.
- **Root binds all formats unconditionally.** `Cargo.toml` lists `sampler-kontakt`,
  `sampler-uvi` and `sampler-ksp` as non-optional dependencies. Root `src/` references
  `sampler_kontakt` in 60+ places across `sound/`, `ui/`, `plugin.rs`, `library.rs`, `parity.rs`,
  `diagnostics.rs`; `sampler_uvi` in ~27 places; `sampler_ksp` in ~50. No feature gates exist for
  any format today. The root, `sampler-native`, and `sampler-perf` pull all of them.
- **Unwired stubs.** `sampler-ample` (293 LOC, `.ascf` reader) and `sampler-ikmpak` (456 LOC src,
  one test file) are workspace members with no consumer in the root. They are the cheapest
  proof of the registry: they can be added with zero breakage.

## 2. Where DSP leaks (for GPT lane to consume)

Format crates are mostly mapping code; a few DSP-shaped pieces remain.

| # | Location | What | Size | Assessment |
|---|---|---|---|---|
| D1 | `crates/sampler-kontakt/src/effects/*.rs` (`timemod` 1,304; `color` 821; `filter` 781; `eq` 654; `space` 546; `dynamics` 447; `ringmod` 120; `stereotune` 54; `surround` 387; incl. tests) | Preset and slot translators | ~9.2k LOC incl. tests | Mostly mapping (correct place). Check for per-sample arithmetic: only `effects/color.rs` (2 hits) and `effects/filter.rs`/`timemod.rs` tests reference engine curves. Little real DSP. |
| D2 | `crates/sampler-uvi/src/fx/*.rs` (`timemod` 527; `space` 282; `filter` 218; `eq` 171; `dynamics` 150; `color` 27) | Preset translators | ~1.4k LOC | Mapping; cites `sampler_core` `dsp/timemod/{tape,velvet,buzz,diffuse,uvi_legacy}.rs` and `dsp/space/sparkverb.rs` as the native law. Fine. |
| D3 | `sampler-core/src/dsp/replika/` (12 files, 5,063 LOC) + `replika_head.rs` | Product-named delay/head engine | 5.1k LOC | **Product copy inside the engine.** Name is a product; must become a generic core + settings. |
| D4 | `sampler-core/src/dsp/machine.rs`, `machine_tables.rs`, `source/time_machine.rs`, `source/tone_machine.rs` | Kontakt "machine" modules named by product | — | Product names inside the engine; verify each is a generic core. |
| D5 | `sampler-core/src/dsp/gater.rs` (+ `gater_kemu_vectors.json`) | Gater named after a Kontakt kemu vector set | — | Product-named; vectors are evidence, keep. |
| D6 | `sampler-core/src/dsp/surround_panner.rs` (+ 2 `*_native_vectors.json`) | Surround panner law | — | Product-named; used by kontakt `effects/surround.rs`. |
| D7 | `sampler-core/src/dsp/stereo_tune.rs` (+ `stereo_tune_native_vectors.json`) | Stereo tune law | — | Product-named; used by kontakt `effects/stereotune.rs`. |
| D8 | `sampler-core/src/dsp/ringmod.rs`, `ring_ramp.rs` | Ring modulator | — | Generic name; ok. |
| D9 | `sampler-core/src/dsp/filter/kontaktfx.rs` (75 product tokens), `filter/{cascade,daft,formant,ladder,svf,ar}.rs` | Kontakt filter family beside generic filters | — | Five topologies (Gap G1 in ENGINE.md). `kontaktfx` is the product copy. |
| D10 | `sampler-core/src/dsp/timemod/{tape,velvet,buzz,diffuse,psyche,twin,uvi_legacy}.rs` | Product-named modulation delays | — | Product-named per-product cores; candidates for one timemod core + law settings. |
| D11 | `sampler-core/src/dsp/color/{amp/*,kolor/*,skreamer,tubescreamer,tape,bite,dirt,freak,...}` | Named distortions/amps (`bigfuzz`, `acbox`, `cabinet`, `cat`, `crywah`, `dstortion`, `jump`, `twang`, `kolor`, `skreamer`, `tubescreamer`) | ~? | Many product-named colour models. Likely generic nonlinear stages + tables (ENGINE.md "Color / drive" gap). |
| D12 | `sampler-core/src/dsp/space/design/{raum,galois_vectors}.rs` | Raum and Galois reverb designs | — | Product-named reverb designs. |
| D13 | `sampler-core/src/blocks/uvi_lfo.rs` (+ `native_vectors.rs`) | UVI LFO inside engine blocks | — | Product-named block. |
| D14 | `sampler-core/src/dsp/biquad.rs`, `tpt_svf.rs` | Generic primitives | — | Clean. |

Counts in `sampler-core/src`: 47 files are named after a product or product module (16.8k LOC);
180 files contain a product token in their text (`kontakt|kemu|replika|falcon|uvi|surround|gater|machine|stereo_tune|kontaktfx`).
Not all are copies: vectors and laws are evidence and stay. The check for GPT: is each
file a generic core with a product law/table, or a product copy?

Rows marked "—" were not measured per file. Per-file LOC is the first job of the DSP migration plan.

## 3. Ranked violations

1. **uvi→kontakt hard edge** (loader/stream machinery owned by the Kontakt crate). Blocks
   deleting Kontakt; forces the loader into a format. Fix: move shared loading into engine.
2. **Root hard-binds Kontakt, UVI and KSP** (no features, no registry). Blocks format removal and
   any format-off build. Fix: registry + cargo features (this lane's PR sequence).
3. **Kontakt→KSP hard edge** (91 refs). Fix: KSP as its own registered format; Kontakt stops
   depending on it except via the registry.
4. **Product-named DSP in the engine** (D3–D7, D9–D13). Not a dependency violation, but the
   "one engine" rule in `docs/ENGINE.md` says these should be settings of one core. Owner: GPT.
5. **Format crates carrying DSP** (D1, D2). Small and mostly mapping. Low priority.
6. **Unwired stubs** (ample, ikmpak). No violation; they should be registered formats once the
   registry exists.

## 4. Deletion test (what breaks)

Counts are files outside the crate that reference the crate (`rg`, excluding `target/`):

| Delete | Files that stop compiling | Breakdown | Hardest coupling |
|---|---|---|---|
| sampler-uvi | **45** | crates 3, examples 2, src 32, tests 3, tools 5 | root `ui/` (15), `sound/` (8); tools corpus-health/format-census/kontra-scan |
| sampler-kontakt | **91** | crates 41 (incl. uvi, native, perf), examples 6, src 37, tests 2, tools 5 | uvi hard dep; root `sound/` and `ui/`; ksp path |
| sampler-ksp | **74** | crates 32 (incl. kontakt 28), examples 2, src 35, tests 1, tools 4 | Kontakt's script path (28 refs) |
| sampler-ample | 0 | — | unwired |
| sampler-ikmpak | 1 (its own test only) | — | unwired |

Reading: uvi and ample/ikmpak are the cheapest to make optional. Kontakt cannot be removed
until the loader is out of it (ranked 1) and KSP is decoupled (ranked 3).

## 5. What is already clean

- `sampler-ir` has zero dependencies.
- `sampler-core` has zero dependencies on format crates.
- `sampler-kontakt/effects/surround.rs` and `stereotune.rs` call the engine (`sampler_core::surround_panner`,
  `sampler_core::dsp::stereo_tune`), the right shape even though the engine modules are product-named (D6, D7).
- `sampler-uvi/src/fx/filter.rs` maps onto `sampler_ir::fx::filter::Config` only; it contains no DSP.

## 6. Next

- Registry + features PR (lane): `FormatModule` in `sampler-ir`, a static registry, one cargo
  feature per format on the root, and a test matrix with formats off.
- Loader move (uvi→kontakt): owner this lane; touches `sampler-kontakt` loader files, rebase on
  `origin/main` before each PR.
- DSP_MODEL.md (GPT): consumes section 2.
