# Vision: why v3 exists and what every change must keep possible

> **One gold-standard engine. Every format is a translation into one IR.
> Faithful where it is audible, modern everywhere else. Then go beyond what
> the originals can do.**

Every task brief opens with this line, and every PR answers the
[design test](#the-design-test) below. The rules in [README.md](README.md)
and [ENGINE.md](../ENGINE.md) say *how* to build. This page says *why*, and
what a change must not close off.

## Why

Sample libraries are locked into the player they were made for: Kontakt,
UVI/Falcon, Opus, HALion, SFZ players. Each player is an engine that is ten
or more years old. Scripts run at block rate, quirks are per product, CPU and
RAM use is high, and per-note expression is missing or bolted on.

KONTRA is one universal sampler. It loads every one of those libraries and
plays them as their authors intended, then plays them better than the
original player can: faster, leaner, cleaner, with per-note expression and
modulation the original never had.

The order of proof:

1. **Plays correctly.** Right sample, articulation, offset, level, direction
   and round robin, checked against the original (oracle captures, recorded
   vectors, decompiled laws). Audibility alone is not the bar.
2. **Beats the original.** Less CPU, less RAM, faster loads, higher DSP
   quality, sample accuracy.
3. **Goes beyond.** Features the original player cannot offer, available to
   every library through the shared engine.

Kontakt parity comes first, then UVI, then SFZ and the rest. A feature that
goes beyond never waits on a format: it lands in the engine, and every
format gets it.

## How: the pipeline

| Layer | Its job | It never |
|---|---|---|
| Format reader (`vendor/`, `format-*`) | Decode files, presets, scripts and UI | Plays audio or holds DSP |
| Translator (`format-*`) | Map the product's modules, parameters, units and **laws** (`kontra_law`) onto the IR; report anything not carried (`ir::Impact`) | Drops something silently, or bends the engine for one product |
| IR (`kontra-ir`) | Vendor-neutral description: zones, groups, routes, effects, scripts | Names a vendor or a vendor-only unit |
| Engine (`engine/*`, `dsp/*`, `script/*`) | One implementation per kind of processing, one script host, one clock | Branches on the source format |
| Plugin (`app/*`, `ui*`, `protocol`) | Host integration, UI and state, plus the features that go beyond | Reaches around the IR into a format |

A new format is one translator. A new capability is one engine change that
every format can use.

## The four commitments

1. **Universal.** Everything passes through the IR. Product behaviour is
   data: a law, a table or a constant in the translator. A second
   implementation of a DSP type needs a measurement showing that the
   topology really differs.
2. **Faithful.** Match what is audible or visible to scripts: response,
   level, time constants, parameter laws, scheduling, the quirks libraries
   rely on. Evidence decides (decompilation, oracle captures, vectors, null
   tests), and measurements are taken on real libraries, not only fixtures.
   Match the sound, not the code (ENGINE.md, *KISS and YAGNI*).
3. **Modern.** Inside that tolerance, choose the cleanest high-quality
   implementation. That means:
   - real-time safety (no locks, allocation or faults on the audio thread);
   - sample-accurate events;
   - click-free parameter changes (glides, ramps, stable filters under
     modulation);
   - f64 state where it is cheap;
   - offline and real-time renders that are identical.
4. **Beyond.** Every design keeps the items below possible. That does not
   mean building them now: it means not having to tear the design out to add
   them later.

### What the engine must keep possible

| Item | What it demands of today's designs |
|---|---|
| **Per-note everything (MPE, CLAP note expressions)** | Every voice knows its note and note id. Modulation sources are per note, and any route can take a per-note source. Never a per-group-only value where a per-voice one is plausible. |
| **Per-voice anything** | Any effect or parameter a route can move runs per voice when moved, and shared otherwise. The IR can describe a route to any parameter, even when the original could not. |
| **Sample accuracy** | Events, waits, parameter changes and script actions carry a sample time. Block size never changes the result. |
| **Efficiency far below the originals** | Share linear work across voices, load each sample once, and stream by a measured preload law. Targets: about 300 voices at about 1% CPU, Kontakt windows at most 60% of Kontakt's CPU, RAM below the original's. Measure on real libraries. |
| **Quality above the originals** | Where a change cannot be heard as different from the original, take the cleaner version: less aliasing, no zipper noise, no denormals. |
| **Modern scripting** | KSP and UVI Lua behave like the originals and old scripts keep working. Extensions (per-note, MPE-aware, faster) are added on top, never instead. |
| **An open, native format** | The IR is stable enough that a library can target KONTRA directly (`format-native`). |

## The design test

Every PR answers these in its *Vision check* section, one line each:

1. **One implementation?** Is this the single implementation every format
   uses? If it adds format-only behaviour, which law or table carries it?
2. **Evidence?** Which test or measurement shows it matches the original
   where that is audible or visible to scripts?
3. **Better?** In what way is it better than the original (quality, CPU,
   RAM, accuracy), or at least no worse?
4. **Future?** Does it keep [the list above](#what-the-engine-must-keep-possible)
   possible? Name the item it advances, or say which ones it touches and why
   they stay open.
5. **Reported?** Is everything it does not carry reported, not dropped
   silently?

## Anti-patterns (each one happened here)

- **A per-instrument fix.** Special-casing one library instead of fixing the
  mechanism for all of them.
- **Vendor knowledge below the translators.** A Kontakt branch in the engine,
  or a vendor unit in the IR.
- **Copying the original's internals step by step.** Thousands of lines to
  reproduce an EQ curve, when a standard design lands inside tolerance.
- **Fixing a symptom before reading the reference.** Change the code only
  after reading the library's scripts, UI and groups and the original's
  behaviour, not by guessing.
- **Silent drops.** Muted groups removed, routes ignored, effects passed
  through with no report.
- **Designs that close a door.** State that can only ever be per group, a
  block-rate path where a sample time is needed, or a global that one note
  cannot override.
- **Scaffolding for the vision.** Building UI stores, abstractions or
  features with no consumer yet. This page is a direction, not a licence to
  build ahead (YAGNI).
- **Trusting a counter instead of a contract.** A count going down is not
  proof the behaviour is right.

## Where the details live

- [README.md](README.md): the ten rules.
- [ENGINE.md](../ENGINE.md): one engine, and *KISS and YAGNI*.
- By subsystem: [02-ENGINE.md](02-ENGINE.md), [03-DSP.md](03-DSP.md),
  [04-SCRIPT.md](04-SCRIPT.md), [05-STREAM.md](05-STREAM.md),
  [06-FORMATS.md](06-FORMATS.md), [07-APP.md](07-APP.md).
- Evidence: `docs/re/`, `tools/kontakt-oracle/`, the vectors next to each DSP
  family.
