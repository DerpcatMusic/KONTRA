# KONTRA architecture v3: the refactor reference

This folder is the reference for the v3 refactor: one engine, one DSP library,
one script host, thin format translators, a crate layout that compiles fast
and code that one person can hold in their head. Every PR in the refactor
cites the section it implements. When this folder and the code disagree, fix
one of them in the same PR.

**Start with [VISION.md](VISION.md):** why v3 exists, the one-line mantra,
and the design test every PR answers. The docs below say how.

Read in order:

| Doc | What it fixes |
|---|---|
| [VISION.md](VISION.md) | Why: one engine, every format through one IR, faithful then beyond; the design test |
| [00-BASELINE.md](00-BASELINE.md) | Measured starting point: sizes, build times, structural debt |
| [01-LAYOUT.md](01-LAYOUT.md) | Crate tree, folder rules, dependency DAG, who owns what |
| [02-ENGINE.md](02-ENGINE.md) | Audio-thread runtime: clock, queue, ledger, voices, render |
| [03-DSP.md](03-DSP.md) | DSP library: one folder per type, module trait, laws, vectors |
| [04-SCRIPT.md](04-SCRIPT.md) | One script host; KSP and Lua front ends |
| [05-STREAM.md](05-STREAM.md) | Sample store, preload law, I/O service, memory |
| [06-FORMATS.md](06-FORMATS.md) | Translator contract for Kontakt, UVI, SFZ, native |
| [07-APP.md](07-APP.md) | Plugin shell, threads, state, UI boundary |
| [08-MACROS.md](08-MACROS.md) | Declarative and derive macros that replace boilerplate |
| [09-BUILD.md](09-BUILD.md) | Compile-time plan, profiles, CI |
| [10-TESTING.md](10-TESTING.md) | Contracts, vectors, test layout |
| [11-MIGRATION.md](11-MIGRATION.md) | PR-by-PR order with gates, and what gets deleted when |

Background: [../audit-2026-10-24/](../audit-2026-10-24/) (playback audit,
sampler landscape, one-engine audit, future plan).

## The ten rules

1. **One of everything.** One clock, one event type, one note ledger, one voice
   renderer, one script scheduler, one sample store, one class per DSP type.
   A second implementation needs a measurement recorded next to it.
2. **Vendors are data.** Kontakt, UVI and every other format are translators
   that produce the IR and law tables. No vendor names below `format-*`
   except inside law tables.
3. **Time is samples.** Every event, wait, parameter change and engine edit
   carries a sample timestamp and takes effect at it. Block size never matters.
4. **Voices start on time.** From RAM, at their sample, every voice of a note
   together. Storage problems are dropouts, never delays.
5. **The audio thread owns the engine.** No locks, no shared mutable state.
   Other threads talk to it through three channels only (commands in,
   publications out, retirements out). See 07-APP.md.
6. **Scripts run to completion** at their event time, under one scheduler,
   whatever the language.
7. **Downward dependencies only.** The DAG in 01-LAYOUT.md is enforced in CI.
8. **Declare once.** A DSP module, a parameter, a KSP builtin, a counter is
   declared in one place; macros generate the enums, dispatch, metadata,
   serialization and UI descriptors (08-MACROS.md).
9. **Small crates, small files.** Crates under ~8k production lines, files
   under ~800. A crate does one thing, and its name says what.
10. **Proof over counters.** Contracts in CI (10-TESTING.md) define "works".
    A counter that measures a bug class that should be impossible gets
    deleted along with that bug class.

## Targets

| Measure | Now | Target |
|---|---|---|
| Production Rust lines | ~271k | ~120k |
| Test lines | ~121k | ~70k, contract- and vector-driven |
| Largest crate (production lines) | `sampler-core` ~103k | < 8k |
| Release rebuild after a DSP or engine edit | 113 s | < 25 s (`fast` profile), < 60 s (`release`) |
| Dev rebuild after an engine edit | ~5 s | ≤ 5 s |
| Files touched to add a DSP module | ~10 files in 5 crates | 1 folder + 1 registry line |
| Script runtimes | 2 (KSP inline, Lua threaded) | 1 host, 2 front ends, inline |
