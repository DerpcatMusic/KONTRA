# 01 Layout: crates, folders, dependencies, ownership

## 1. The crate tree

Every crate is named `kontra-<what>`, lives under a folder named for its
layer, and does one thing. Sizes are production-line targets.

```
crates/
├── base/                         L0: no KONTRA dependencies
│   ├── time/          kontra-time       SampleTime, Frames, Tempo, Transport, beat↔sample      ~0.6k
│   ├── pool/          kontra-pool       generational arenas, Handle<T>, fixed queues, LatestCell ~1.5k
│   ├── simd/          kontra-simd       lane types, f32x4/x8 helpers, lane batching             ~0.8k
│   ├── law/           kontra-law        Law = curve/table/formula, eval, (de)serialize          ~0.8k
│   └── macros/        kontra-macros     proc macros: Params, Stats, Config, Builtins (08)       ~1.5k
├── model/                        L1–L2: data only, no audio code
│   ├── params/        kontra-params     DSP module catalog: ModuleKind, per-module Params       ~2.5k
│   ├── events/        kontra-events     Event, NoteRef, Expression; MIDI1/UMP/CLAP/MPE ingest   ~3k
│   ├── ir/            kontra-ir         instrument model (Articulation→Variation→Group→Zone…)  ~6k
│   ├── ui-model/      kontra-ui-model   widget tree, control store (latest-value), UI schema    ~2k
│   └── protocol/      kontra-protocol   host⇄audio⇄UI messages and published snapshots          ~1.5k
├── dsp/                          L2–L3: one folder per DSP type (03-DSP.md)
│   ├── core/          kontra-dsp-core   Module trait, Block, smoothing, oversampling, biquad math ~3k
│   ├── filter/        kontra-dsp-filter       SVF, ladder, biquad cascade, formant, comb      ~4k
│   ├── eq/            kontra-dsp-eq           parametric bands, shelves, graphic EQ            ~1.5k
│   ├── dynamics/      kontra-dsp-dynamics     compressor, limiter, gate, expander, transient   ~3k
│   ├── delay/         kontra-dsp-delay        delay line, chorus/flanger/phaser, tape, ensemble ~3k
│   ├── reverb/        kontra-dsp-reverb       FDN, plate/room/hall laws, convolver             ~3k
│   ├── drive/         kontra-dsp-drive        waveshaper, saturation, amp/cab, bitcrush/lofi   ~3k
│   ├── spatial/       kontra-dsp-spatial      pan laws, width, stereo tools, surround, rotator ~1.5k
│   ├── modulators/    kontra-dsp-modulators   envelopes, LFOs, step, follower, glide            ~3k
│   └── registry/      kontra-dsp              modules! registry: dispatch, chain compiler       ~1k
├── engine/                       L3–L4: the audio thread
│   ├── stream/        kontra-stream     sample store, preload law, page cache, I/O service      ~4k
│   ├── voice/         kontra-voice      source cursor, resampling, loops, voice chain run       ~4k
│   ├── behaviour/     kontra-behaviour  live switch, controller and round-robin state on a plan ~0.3k
│   ├── vm/            kontra-vm         bytecode, interpreter, coroutine scheduler, HostApi     ~6k
│   ├── engine/        kontra-engine     Engine: queue, ledger, allocator, buses, render          ~8k
│   └── prepare/       kontra-prepare    IR → Plan off the audio thread, sample preloads          ~0.5k
├── script/                       L5: script front ends (compile to kontra-vm)
│   ├── ksp/           kontra-ksp        lexer, parser, sema, lowering, KSP UI mapping           ~10k
│   └── lua/           kontra-lua        Luau host binding to HostApi, UVIScript surface         ~6k
├── format/                       L5: translators (06-FORMATS.md)
│   ├── common/        kontra-format     Format trait, registry, decrypting readers, decoders    ~2k
│   ├── kontakt/       kontra-format-kontakt  NKI/NKM/NKR/NCW/monolith → IR + laws         readers + ~8k mapping
│   ├── uvi/           kontra-format-uvi      UFS/UVIP/XML → IR + laws                     readers + ~6k mapping
│   ├── sfz/           kontra-format-sfz      SFZ ↔ IR                                      ~1.5k
│   ├── native/        kontra-format-native   KONTRA package (open, versioned)              ~1.5k
│   ├── ample/         kontra-format-ample    (existing)
│   └── ikmpak/        kontra-format-ikmpak   (existing)
├── app/                          L6–L7: everything that is not audio
│   ├── library/       kontra-library    scanning, catalog, favourites, artwork lookup           ~4k
│   ├── host/          kontra-host       rack, parts, load pipeline, loader thread, state         ~8k
│   ├── align/         kontra-align      auto-align measurement + scheduler (optional feature)   ~1.5k
│   └── ui/            kontra-ui         editor views (depends on protocol + ui-model, not engine) ~45k
└── plugin/                       L8: kontakto, the shipped artifact (moose CLAP/VST3/standalone) ~1.5k
tools/
├── probe/             kontra-probe      playback contracts (10-TESTING.md), CLI + CI
├── cli/               kontra-cli        create-library, render, inspect
├── bench/             kontra-bench      DSP and voice benchmarks (dspbench + perf)
└── shots/             kontra-shots      CPU-rendered UI screenshots (audit-ui)
vendor/                                  third-party forks (ni-file, ncw, moose patches), unchanged
```

38 crates instead of 17, about 120k production lines instead of 271k.

## 2. Dependency DAG

Arrows point to dependencies. Anything not drawn is forbidden. `xtask
deps-check` (09-BUILD.md) fails CI on a forbidden edge.

```
L8  plugin ─────────────▶ host, ui, protocol
L7  ui ─────────────────▶ protocol, ui-model, (library for browsing data types)
L6  host ───────────────▶ engine, stream, ksp, lua, format*, library, align?, protocol, ui-model
L5  ksp, lua ───────────▶ vm, ir, ui-model, events
    format-* ───────────▶ format, ir, params, law          (never engine, vm, stream, dsp)
L4  engine ─────────────▶ behaviour, plan, voice, stream, vm, dsp, ir, events, time, pool
    behaviour ──────────▶ plan, ir
L3  voice ──────────────▶ dsp-core, stream (read side), simd, time
    stream ─────────────▶ ir (zone geometry only), pool, time
    vm ─────────────────▶ events, time, pool
    dsp (registry) ─────▶ dsp-<family>…, params
L2  dsp-<family> ───────▶ dsp-core, params, law, simd
    dsp-core ───────────▶ simd, law, time
    ir ─────────────────▶ params, law, time
L1  params ─────────────▶ law, macros     events ─▶ time     ui-model ─▶ macros     protocol ─▶ ir, ui-model, events
L0  time, pool, simd, law, macros
```

What this buys:

- **A DSP kernel edit** rebuilds one family crate, the registry, voice,
  engine, host and plugin. Not `ir`, not the formats, not KSP, not the UI.
- **A format edit** rebuilds that format, host and plugin. Not the engine.
- **A UI edit** rebuilds `ui` and `plugin` only. The UI never depends on
  `engine`; it sees `protocol` messages and published snapshots.
- **Families compile in parallel**; there is no 100k-line serial unit left.
- Engine-only tests never compile `wgpu`, `naga` or `ash` (only `ui` pulls them in).

## 3. Who owns what

Ownership is per thread first, per crate second. Nothing is shared mutable
across threads except through `protocol` channels (07-APP.md).

| State | Owner (thread) | Crate | Others get |
|---|---|---|---|
| Engine: notes, voices, buses, script instances, clocks | audio thread, exclusively | `kontra-engine` | `protocol` publications (meters, values, voice taps) |
| Prepared plans (immutable after build) | built by loader, then read-only, shared by `Arc` | `kontra-engine::plan` | audio reads; loader retires |
| Sample store: preloads, page cache, file handles | store: loader builds; I/O threads fill pages; audio reads | `kontra-stream` | lock-free page table + request ring |
| Instrument IR | loader (load-time only) | `kontra-ir` | dropped after prepare, or kept for the editor |
| Script VM programs (bytecode) | immutable, inside the plan | `kontra-vm` | — |
| Script instance state (variables, coroutines) | audio thread | `kontra-vm` via engine | `persist` snapshot via protocol |
| UI control store (script UI writes) | audio writes, UI reads (`LatestCell` per control) | `kontra-ui-model` | latest value, never queued |
| Rack: parts, selection, routing, settings | host/control thread | `kontra-host` | audio gets commands |
| Library catalog | library worker | `kontra-library` | UI reads snapshots |
| Editor view state | UI thread | `kontra-ui` | — |

Rules:

- `pub` only for the crate's API, re-exported from `lib.rs`; everything else
  `pub(crate)`. A crate's public API fits on one screen (`lib.rs` re-exports).
- No `Arc<Mutex<_>>` reachable from the audio thread. No `Mutex` in `engine`,
  `voice`, `vm`, `dsp*` (lint: `clippy::disallowed_types` per crate).
- Counters live in one `Stats` struct per crate (`#[derive(Stats)]`, 08-MACROS.md),
  never as loose fields.

## 4. Folder and file rules

- **Module tree = file tree.** No `#[path]` (138 today). `foo.rs` + `foo/` for
  submodules (2018 style), no `mod.rs` except for DSP family roots where the
  folder is the unit.
- **Files ≤ ~800 lines**, functions ≤ ~80. Larger means two concepts.
- **Tests**: unit tests in `src/<file>.rs` `#[cfg(test)] mod tests` only when
  under ~150 lines; otherwise `tests/<topic>.rs` (integration, public API)
  per crate. Vectors in `tests/vectors/` next to the crate that proves them.
- **No test hooks inside production functions.** Instrumentation goes through
  `kontra_engine::probe!` (compiled out unless the `probe` feature is on).
- **Features** are for optional *products* (formats, UI, standalone, probe),
  never for behaviour variants. One feature set per profile, so
  dependencies are not rebuilt per feature combination (09-BUILD.md §1.5).
- **Naming**: types say what they are (`Compressor`, `PreloadLaw`,
  `NoteLedger`); no `V2`, `v1_`, `New`, `Legacy` in names. Vendor names only in
  `format-*` and in law tables (`laws/kontakt/*.toml`).

## 5. Current → new mapping

| Today | Goes to |
|---|---|
| `sampler-pool` | `base/pool` |
| `sampler-simd`, `sampler-core/src/dsp/lanes.rs` | `base/simd` |
| `sampler-parity` | folded into `tools/probe` (status types) |
| `sampler-ir` | `model/ir` (fx configs replaced by `params` specs) |
| `sampler-ui-ir`, `sampler-core/src/{widget,script_params}.rs` | `model/ui-model` |
| `src/sound/event.rs`, plain-data receiver wire types (P4.8) | `model/events` |
| `sampler-midi` receiver adapter (P6.4c) | `engine/engine` |
| `src/plugin/held_keys*` (P6.4b) | deleted; replaced by the `kontra-engine` `NoteLedger` |
| `sampler-core/src/dsp/**` (48.7k) | `dsp/*` families (target ~26k incl. laws) |
| `sampler-core/src/{blocks,voice_mod*,modulation*,envelope}` | `dsp/modulators` |
| `sampler-core/src/{stream,residency*,head_*,resident_data,packed*}`, `sampler-kontakt/src/{stream,head_prepare,head_cache,stream_initial_hints,channel_reader}.rs`, `sampler-uvi/src/stream.rs` | `engine/stream` |
| `sampler-core/src/{source*,resample*,whole_voice,v1_voice_controls,pitch,release}` | `engine/voice` |
| `sampler-core/src/{behavior*,ops,control/script_state,schedule}` | `engine/vm` |
| `sampler-core/src/{lib,ownership,steal,prepare*,plans,bus*,topology*,receiver*,groups,zone_routing,render*,parallel*,trace*}` | `engine/engine` |
| `sampler-ksp` minus `eval.rs`, `luau/` | `script/ksp` |
| `sampler-uvi/src/{script*,scripted*}`, `sampler-script` | `script/lua` (inline, no thread) |
| `sampler-kontakt` minus stream/effects DSP | `format/kontakt` |
| `sampler-uvi` minus script/stream | `format/uvi` |
| `src/formats.rs`, decoder glue | `format/common` |
| `src/library*`, `src/artwork.rs`, `src/creator*` | `app/library` |
| `src/sound/**`, `src/plugin.rs` (non-UI), `src/routing.rs`, `src/support/**` | `app/host` |
| `src/timing.rs` | `app/align` |
| `src/ui/**` | `app/ui` |
| `src/lib.rs` plugin entry, moose glue | `plugin/` (root package `kontakto`) |
| `sampler-native`, `sampler-perf`, `sampler-dspbench`, `src/create_library.rs` | `tools/*` |
