# KONTRA roadmap

A free, open-source sampler for your own legally usable libraries. The goal is one musical runtime, faithful instrument behavior, and a clean native workspace on Windows, macOS and Linux.

**Planning snapshot: 10 October 2026.** This is a direction and acceptance-gate document, not a release schedule. Priorities describe the order of work; they do not imply assigned owners, committed dates or completed compatibility. Source implementations, targeted tests and published builds are separate evidence.

## Where we are

| Track | Current position | What it does not establish |
| --- | --- | --- |
| Public application | Experimental Kontakt playback; partial scripts, interfaces and effects; CLAP, VST3 and standalone nightly alpha builds | Arbitrary library compatibility, sound equivalence or stable saved-state formats |
| Falcon / UVI | Public README describes metadata inspection; dedicated format/runtime investigations document prerequisites | General UFS bank playback or Falcon parity |
| Independent v2 runtime | Tested slices for ownership, scheduling, selection, sample rendering, DSP, streaming, MIDI and a KSP subset | A complete production engine, replacement of the shipped plugin or full vendor semantics |

The [feature inventory](docs/FEATURES.md) and [compatibility checklist](docs/COMPATIBILITY.md) are dated evidence, not a live certification dashboard. The [v2 handoff](docs/architecture-v2/HANDOFF.md) records a paused architecture checkpoint dated 6 October; publishing this roadmap does not resume that work. Consult [releases](https://github.com/DerpcatMusic/KONTRA/releases/latest) for the exact downloadable build and its manifest.

## First — trustworthy playback

Make failures explicit and fix musical correctness before broadening compatibility claims.

- Preserve note, family, expression and release ownership across sustain/sostenuto, MPE channel reuse, voice stealing, script-generated notes and plan replacement.
- Complete missing MIDI 2.0 expression/management and source-specific event ordering against canonical timestamped events.
- Finish modulation composition, envelope/LFO routing and source parameter laws; keep unsupported targets visible in load reports.
- Establish production streaming admission, cold-onset preload, resource budgets and starvation behavior under real storage pressure.
- Keep script execution, worker handoffs and retirement bounded; move allocation, filesystem work and destruction off audio callbacks.

**Exit evidence:** regression cases exercise the actual musical behavior; callback/heap guards and real storage workloads demonstrate bounded operation; errors retain their cause and build identity. A silent fallback is not a passing result.

Sources: [ownership and scheduling](docs/architecture-v2/SCHEDULING_SLICE.md), [MIDI](docs/architecture-v2/MIDI_INGRESS.md), [modulation](docs/architecture-v2/MODULATION.md), [streaming](docs/architecture-v2/STREAMING.md), [current priorities](docs/architecture-v2/HANDOFF.md#known-engineering-limits-and-next-priorities).

## Next — Kontakt behavior and interfaces

Translate instruments into shared services while preserving native semantics and recognizable authored interfaces.

- Expand versioned containers, resources, samples, snapshots and instrument semantic admission with strict malformed-input boundaries.
- Complete KSP language, callbacks, builtins and underlying services, including asynchronous operations and persistent state.
- Resolve stored-event note-off across script stages without losing the physical key lifetime or upstream release callbacks.
- Expand source engines, loops, filters, effects and modulation laws against reference behavior.
- Complete generic, stock and bitmap KSP views, native control resources and requested UI generations; retain Original and Vector presentation as distinct modes.
- Preserve multi-part routing, automation identities and state recall through the production host integration.

**Exit evidence:** versioned instruments and scripts produce the expected audio, callbacks, UI interactions and recall in controlled reference comparisons. Decoding a file, displaying artwork or recognizing a builtin alone does not close the gate.

Sources: [Kontakt source boundary](docs/architecture-v2/KONTAKT_SOURCE.md), [KSP completion map](docs/architecture-v2/KSP_PARITY.md), [UI frontends](docs/architecture-v2/UI_FRONTENDS.md), [compatibility checklist](docs/COMPATIBILITY.md).

## Then — Falcon / UVI on the same runtime

Implement a separate source and behavior frontend without building a second musical engine.

- Establish readable, authorized bank/program/sample records and explicit resource identity; a UFS header is not playable content.
- Preserve Program, Layer, Keygroup, Oscillator and processor hierarchy, typed parameters and routing.
- Implement UVIScript event forwarding, coroutine timing, callback budgets, object services, persistence and restore order.
- Map source modules, modulation, effects and expressive controls to the shared prepared runtime with vendor-specific semantics.
- Reproduce authored Lua UI controls, parameter bindings, assets, layout and interaction.

**Exit evidence:** actual authorized programs exercise import, sample/synthesis playback, scripting, UI and saved-state behavior against Falcon. Lua interpreter availability and metadata inspection are prerequisites, not parity.

Sources: [format groundwork](docs/FALCON_FORMAT_GROUNDWORK.md), [runtime and UI groundwork](docs/FALCON_RUNTIME_UI_GROUNDWORK.md), [shared IR](docs/architecture-v2/SHARED_IR.md).

## Production integration — a coherent native instrument

Bring the complete foundations into the application and hosts when ownership and resource contracts are ready.

- Integrate the new runtime with production CLAP/VST3, standalone, headless state and automation; the v2 plan identifies Linux CLAP in Bitwig as the first hands-on target.
- Preserve the compact rack and library browser, improve the sample section and expose useful load/runtime reports.
- Build nested instrument mixing with source buses, gain, pan, mute/solo, inserts, sends and explicit output assignments.
- Extend native MPE expression to imported instruments with configurable routing and script event consumption preserved.
- Support articulation switching through velocity, channels and controllers as well as keyswitches.
- Validate UI scale, keyboard operation, native accessibility, window lifecycle and recall on each supported OS and relevant host.

**Exit evidence:** packaged builds complete real host load/play/save/reopen/unload sessions; routing and automation affect actual audio; the editor remains usable at supported scales and input methods.

Source: [v2 product requirements](docs/architecture-v2/PRODUCT.md), [delivery plan](docs/architecture-v2/PLAN.md), [native UI preservation](docs/UI_NATIVE_PRESERVATION.md).

## Release gates — evidence before claims

These apply throughout the roadmap, rather than being postponed until the end.

- **Correctness:** versioned vendor fixtures, repeatable musical scenarios and differential audio/behavior checks.
- **Performance:** matched instruments and audio quality, disclosed hardware and workloads, callback latency and memory measurements. Isolated microbenchmarks do not establish superiority over Kontakt or Falcon.
- **Distribution:** reproducible build identity, exact artifact manifests, cross-platform packaging and dependency notice/source bundles.
- **Rights and provenance:** resolve documented parser redistribution and reference/asset provenance questions; preserve records and use redistributable fixtures.
- **Honest status:** unsupported behavior stays diagnosable. Roadmap status changes require linked implementation and validation evidence.

Sources: [CI gates](docs/CI.md), [conformance allocation](docs/architecture-v2/CONFORMANCE_MAP.md), [render workloads](docs/architecture-v2/RENDER_WORKLOADS.md), [third-party notices](THIRD_PARTY.md), [documented legal questions](docs/LEGAL.md).

## Deliberately deferred

SFZ and other new format frontends remain deferred until the Kontakt/Falcon parity and matched-quality performance gates are met, as stated in the v2 plan. There is no announced stable-release date or claim of full compatibility. The architecture label “v2” does not change the currently released package version.

## Help move it forward

[Report an issue](https://github.com/DerpcatMusic/KONTRA/issues/new) with the exact build identity, OS/host, steps, expected behavior and reviewed diagnostics. Share only material you have rights to redistribute; keep credentials, access metadata, proprietary presets and samples private. [Contributing](CONTRIBUTING.md) explains code, fixture, validation and provenance requirements.
