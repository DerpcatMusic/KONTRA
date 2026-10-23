# One engine

KONTRA is **one engine**, not a Kontakt player next to a Falcon player.

The engine owns a single universal set of sampler, synthesis, modulation, filter and effect processing. Every sampler we emulate (Kontakt, UVI/Falcon, and later Opus, HALion, HISE and others) is a thin **extension** on top of it. An extension does three things only:

1. **Read** the format: files, presets, scripts, UI.
2. **Map** the format's modules, parameters, units and laws onto the shared engine's configuration.
3. **Choose** the engine's behaviour: the law, curve, ordering, rounding and edge-case variant that reproduces that product exactly.

An extension never contains its own DSP. If a format needs something the engine cannot do, the engine grows a general capability and the extension selects it.

## The rule

- **One implementation per kind of processing.** One EQ with any number of bands, not thirty. One filter family with every core. One dynamics, delay, reverb, modulation, distortion, oscillator and source engine. Product differences are *settings of the shared engine* (a law, a table, a topology, a constant), not copies of it.
- **Match the sound, not the code.** The engine reproduces each emulated product's audible behaviour (response shape, level, time constants, parameter laws, units, scheduling, voice and scope lifetime, the controls themselves) as close as measurement shows is needed. It does not have to reproduce the product's internal algorithm or topology step by step. See *KISS and YAGNI* below.
- **Evidence decides.** Laws come from the original binaries (decompilation, recovered DSP, recorded vectors, null tests), never from guesses. Kontakt and Falcon DSP are not interchangeable without evidence.
- **Modular and robust.** Processing is built from small cores that compose, so a new product's module is usually a new configuration, not new code.
- **Format-neutral configuration.** The engine reads `sampler_ir` configuration. Format crates translate into it.

## KISS and YAGNI

We live by KISS and YAGNI. More often than not the simpler solution is the correct one.

- **Example.** Matching Kontakt's EQ curve exactly can take thousands of lines of recreated steps. A standard, high-quality modern EQ in a fraction of the code is better (cleaner, less aliasing and fewer numerical problems) and close enough: a bell with the same shape sounds the same.
- **One model, many settings.** Write the compressor, EQ, filter, delay, reverb and so on once, globally and well. Kontakt libraries load Kontakt's parameter mapping and curves into it; UVI/Falcon load theirs. Matching a product is a different set of parameters, tables and laws, never a new implementation.
- **Close enough is the bar.** Accept a modern implementation when its measured response (magnitude and phase shape, static curves, time constants, level) lands inside the tolerance of the product's, checked against native vectors or oracle captures. Prefer the cleaner implementation over a faithful one that is noisier.
- **Do not build it until it is needed.** No speculative generality, no per-product copies, no scaffolding for later.

**The caveat.** Simple must not mean wrong. Keep the extra code when evidence shows it is audible or behaviourally required: a nonlinearity's shape, a quirk libraries rely on (script-visible behaviour, parameter rounding, a law that changes level or timbre), real-time safety, determinism, state recall. Measure first; add complexity only for a measured difference, and say which measurement justified it.

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

## Universal module architecture (design)

Status: design, published for the lane agents. Each product module is a **preset** of a universal
module (a config plus law tables) selected by its format crate. Sections marked **Gap** are
unfinished. Code described here already exists unless marked otherwise.

Products map onto the universal modules in the table below; no product gets its own DSP copy
(see "The rule" above).

### Layers

| Layer | Crate / path | Role |
|---|---|---|
| IR | `crates/sampler-ir/src/lib.rs` (`Processor`), `src/fx/*.rs` | Format-neutral description of a chain. `fx::<family>::Config` is the **superset** of one family; `fx::Law` and `fx::Lane` describe stored-value laws and script-addressable knobs. |
| Cores | `crates/sampler-core/src/dsp/**` | The DSP. `biquad.rs`, `filter/` (cascade, svf, ladder, ar, daft, formant, kontaktfx), `dynamics/` (detector, computer, ballistics, laws, output), `eq_family/`, `timemod/`, `space/`, `color/`, `lanes.rs`, `taps.rs`, `hiir.rs`, `kernels.rs`. |
| Mapping | `crates/sampler-kontakt/src/effects/*.rs`, `crates/sampler-uvi/src/fx/*.rs` | Per product: read the stored record, choose the preset (config + laws), report what is not modelled. |
| Runtime | `crates/sampler-core` (`Runtime`, processor dispatch) | Runs the IR processors; `Processor::Mix` wraps script-addressable blocks. |

### Universal module families

Each family has one config superset in `sampler-ir::fx` and one set of cores.

| Family | Universal shape | Products that are presets of it | Shared cores today |
|---|---|---|---|
| **Filter** | Topology + response + poles + drive + key tracking + cutoff/resonance laws (`fx::filter::Config`). Target: every topology is a **cascade of second-order sections (SOS)** of one biquad/SVF primitive, with a state-space or transfer-function description for the feedback designs. | Kontakt filter types (table in `kontakt effects/filter.rs`), UVI biquad/moog/rez/xpander/digital/vowel/comb | `cascade.rs` (direct-form SOS), `svf.rs`, `ladder.rs`, `ar.rs`, `daft.rs`, `formant.rs`, `kontaktfx.rs` |
| **EQ** | N bands, each a shape (`fx::eq::Band`/`Kind`), serial. | Kontakt legacy 1/2/3-band EQ, Solid G-EQ, UVI digital EQ | `eq_family/` (generic, legacy, solid, digital) |
| **Dynamics** | Detector + gain computer + ballistics + lookahead, composed (`fx::dynamics::Config`, `Law`, `Link`). | Compressor (Classic/Enhanced/Pro), Limiter, Bus comp, Transient, Hilbert limiter, Feedback comp, UVI comp/limiter/gate | `dynamics/` (detector, computer, ballistics, laws, output, pipeline) |
| **Time / modulation** | Delay lines, allpass, comb, LFO, feedback matrix, voice bank (`fx::timemod::Config`). | Legacy delay/chorus/flanger/phaser, Twin, Psyche, Choral, UVI delay/chorus | `timemod/` |
| **Space** | Network of delay/allpass/comb/filters with feedback (`fx::space::Config`), convolution as a separate impulse-based stage. | Reverb (Galois, legacy, plate), Raum, Convolution, UVI reverb | `space/` (Freeverb, Galois, Network), convolution in `sampler-core` |
| **Color / drive** | Pre-filter -> nonlinear transfer-function stage -> post-filter -> tone stack (`fx::color::Config`). Nonlinearities are composable transfer-function stages (table or curve), with explicit placement in the loop. | Distortion, Saturation, Lo-Fi, Skreamer, Tape, Stereo modeller, Gainer, Inverter | `color/` (drive, shaping, tonestack, tables, tape, ...) |
| **Source** | One resampler / looper / stretcher core shared by all sample playback. | Kontakt and UVI voices | `resample/`, `source/`, `voice_mod` |

### Gap table (honest state)

| Gap | Mandate says | Today | Next step |
|---|---|---|---|
| Filter cores are separate per topology | Ladder, SVF, AR, Daft as topologies of the same SOS / state-space primitives | Five distinct cores (`ladder`, `ar`, `daft`, `svf`, `cascade`) | Express the SVF and cascade as SOS; check ladder/AR as feedback designs of the same primitive; keep the native recurrences only where vectors prove the difference |
| Nonlinearities are not generic stages | Composable transfer-function stages with explicit loop placement | Shaping lives in `color/` per module (distortion, saturator, tube, diode, ...) | One transfer-function stage type (curve or table, pre/post placement) that those modules configure |
| Mapping is per family, not a registry | A new product module is a mapping table | Each format crate has per-family `translate`/`slot_*` functions; kontakt `effects/mod.rs` routes by `Params` variant | See section 4: a family descriptor table plus a routing table |
| Kontakt `effects/mod.rs` was 3,900 lines | Family split | Done: routing and rack reading stay in `mod.rs`; each family owns its `slot_*` translators; bus assembly in `bus.rs`; shared fixtures in `test_support.rs`. Behaviour-neutral: the 304 sampler-kontakt lib tests are unchanged | Remaining: remove per-family mapping boilerplate (section 4) |

### Extension surface: adding a module

To add a product module (no new DSP when a family covers it):

1. **Pick the family** from section 3. If none fits, the family's config must be widened first
   (add a field or variant to the superset), not copied.
2. **Law and lane tables.** Stored values map through `fx::Law` (`Identity`, `Linear`, `Exp`,
   `Pow`, `Table`, `LegacyEqFrequency`). Script-addressable knobs are `fx::Lane { name, role,
   value, law, scale }`. Units and ranges are the law's domain: keep them in the table, not in
   the translator.
3. **Preset.** A function in the format crate turns the stored record into the family config,
   using the same superset. Keep it a pure mapping: no buffers, no state.
4. **Route.** Add one arm in the format crate's routing table (`effects/mod.rs` in kontakt,
   `fx/mod.rs` in uvi) that names the module ID or element kind and calls the preset.
5. **Diagnose.** Anything not mapped returns a `sampler_ir::Reason` (`NotModeled`,
   `UnknownLaw`, `InvalidValue`) with the feature and value. Unsupported parameters are
   reported, never silently dropped.
6. **Prove it.** One golden vector or null test per behaviour (recorded vector, closed-form
   law, or bit-identity against the previous core). Keep it lean: no per-parameter snapshots.

The kontakt effects layer follows this shape. `effects/mod.rs` holds rack reading, the
`Params` parse and the routing table (`chain_with`, one match arm per stored module). Each
family file (`filter.rs`, `eq.rs`, `timemod.rs`, `dynamics.rs`, `space.rs`, `color.rs`) holds
its preset functions and `slot_*` translators. A translator takes `&mut Lowering` (the rack's
pending chain, stereo matrix and filters), `&Site` (the slot's address, gain and scope) and
`&mut Notes` (diagnostics), and returns whether the slot was modelled. `bus.rs` holds bus
assembly and send levels; `test_support.rs` holds the test fixtures.

### Rules for every module

- **Universal first.** Write the general form (config-driven, explicit law selector) before the
  product preset. A variant selector is an enum in the config, not a separate function.
- **Bit-identity.** Migrating a core behind an existing golden vector must keep the output
  bit-identical. Where no vector exists, add one before changing the code.
- **Delete duplication.** When two modules share a law or stage, move it to the shared core and
  delete the copy in the same change.
- **Audio thread.** No allocation, no locks, no `Vec::push` in process paths.
- **Evidence.** Constants and laws cite their source (decompiled function address, public
  documentation, or measured vector). Never paste decompiler output into the repo.

### Ownership and merge hygiene

Shared files (`sampler-ir/src/lib.rs`, `fx/mod.rs`, `effects/mod.rs`) get small, additive edits.
Lane agents add modules as new files under their family and one routing line. Rebase on
`origin/main` before pushing.
