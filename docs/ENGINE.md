# One engine

KONTRA is **one engine**, not a Kontakt player next to a Falcon player.

The engine owns a single universal set of sampler, synthesis, modulation, filter and effect processing. Every sampler we emulate (Kontakt, UVI/Falcon, and later Opus, HALion, HISE and others) is a thin **extension** on top of it. An extension does three things only:

1. **Read** the format: files, presets, scripts, UI.
2. **Map** the format's modules, parameters, units and laws onto the shared engine's configuration.
3. **Choose** the engine's behaviour: the law, curve, ordering, rounding and edge-case variant that reproduces that product exactly.

An extension never contains its own DSP. If a format needs something the engine cannot do, the engine grows a general capability and the extension selects it.

## The rule

- **One implementation per kind of processing.** One EQ with any number of bands, not thirty. One filter family with every core. One dynamics, delay, reverb, modulation, distortion, oscillator and source engine. Product differences are *settings of the shared engine* (a law, a table, a topology, a constant), not copies of it.
- **Match, don't approximate.** The engine must be able to reproduce the corresponding module of each emulated product exactly, or as close as measurement allows. Behaviour includes parameter laws, units, smoothing, denormal and edge handling, scheduling, voice and scope lifetime, and the controls themselves.
- **Evidence decides.** Laws come from the original binaries (decompilation, recovered DSP, recorded vectors, null tests), never from guesses. Kontakt and Falcon DSP are not interchangeable without evidence.
- **Modular and robust.** Processing is built from small cores that compose, so a new product's module is usually a new configuration, not new code.
- **Format-neutral configuration.** The engine reads `sampler_ir` configuration. Format crates translate into it.

## Layout

| Layer | Where | Contents |
| --- | --- | --- |
| Engine | `crates/sampler-core/src/dsp/` | Shared cores by family: `eq_family`, `filter`, `dynamics`, `space`, `timemod`, `color`, plus `biquad` and primitives. Source, voice, modulation and scheduling live beside it in `sampler-core`. |
| Neutral configuration | `crates/sampler-ir/src/fx/` | Per-family `Config` types the engine reads. |
| Extensions | `crates/sampler-kontakt`, `crates/sampler-uvi` (and future crates) | File/preset/script readers and parameter mapping only. |
| Evidence | `docs/re/`, `crates/sampler-parity`, golden and recorded vectors | The proof that a mapping matches the original. |

## Adding a product or module

1. Find the original behaviour in the binary (REA/Ghidra decompilation; `unpacer` for the PACE-protected UVI/Falcon code). Record it in `docs/re/` with the evidence.
2. Check whether the shared engine already covers it with a different law or setting. Prefer extending an existing core over adding one.
3. If a capability is missing, add it to the engine in general form, with a bit-exact test (golden coefficients, recorded vector or null test).
4. Map the product's parameters in the extension crate. Unsupported parameters must be diagnosed, not silently ignored.
5. Verify through `sampler-parity` against the original where one is available.

## Standing constraints

- DSP arithmetic stays bit-exact once verified.
- No allocation on the audio thread.
- Fix mechanisms for all libraries, never per instrument.
- Targets: every supported library plays correctly; 300 voices at about 1% CPU.

See also: [DSP system inventory](re/DSP_SYSTEM_INVENTORY.md), [format specification](re/DSP_FORMAT_SPECIFICATION.md), [shared IR](architecture-v2/SHARED_IR.md), [DSP coverage](architecture-v2/DSP_COVERAGE.md).
