# Kontakt 8 amplifier effects: Jump, Reverb Delight, Super Fast 100, Twang, Van51

Status: **partial.** The Kontakt 8 decompile is ready (`source/kontakt8/READY`). Reader layouts, effect vtables and the Jump stage topology are confirmed below. The DSP cores of Jump, Van51, Super Fast 100 and Reverb Delight are **not yet implemented**: their process and parameter-law functions are located but not yet read through. Nothing here is fabricated.

## Method

- Vtables: MSVC RTTI in `re-binaries/Kontakt8.exe` (static parse with pefile; `tools/kontakt-oracle/rtti_vtables.py`), image base `0x140000000`.
- Functions: `source/kontakt8/<bucket>.c`, bucket = address with the low 16 bits cleared (e.g. `0140a80000.c` for `0x140a8xxxx`).

| Class | Vtable | Ctor | Reader |
|---|---|---|---|
| BEffectJump | `0x144fc0330` | `0x140a86880` | `0x140d01e10` |
| BEffectVan51 | `0x144fc0570` | `0x140a86fb0` | `0x140d033e0` |
| BEffectReverbDelight | `0x144fc1548` | `0x140a86cc0` | `0x140ac5e50` |
| BEffectSuperFast100 | `0x144f16cd0` | `0x1407c8620`-area (not yet confirmed) | `0x140ac61a0` |

## Reader layouts (confirmed)

All four readers are `param_3 == 0x10` typed. Float fields read by `FUN_142a780f0`, byte fields by `FUN_142a780c0`. Offsets are into the effect object.

- **Jump** (`0x140d01e10`): 6 floats at `0x08,0x0c,0x10,0x14,0x18,0x1c`, then 2 bytes at `0x20,0x21`. Matches the binding table: PREAMP 0, MASTER 1, PRESENCE 2, TREBLE 3, MID 4, BASS 5, HIGAIN (`0x20`, field 6, not yet bound), MONO (`0x21`, field 7).
- **Van51** (`0x140d033e0`): 8 floats at `0x08..0x24` (PREGAINRHYTHM, PREGAINLEAD, BASS, MID, TREBLE, POSTGAIN, RESONANCE, PRESENCE), then 5 bytes at `0x28..0x2c` (LEADCHANNEL 0x28, HIGAIN 0x29; `0x2a..0x2c` unbound). Matches `fx_other.tsv`.
- **Super Fast 100** (`0x140ac61a0`): 8 floats at `0x08..0x24`, then 4 bytes at `0x28..0x2b`. No binding rows yet, so the names are unknown.
- **Reverb Delight** (`0x140ac5e50`): floats at `0x08,0x0c,0x10`, BRIGHT byte at `0x14`, floats at `0x18,0x1c,0x20`, TREM_ON byte at `0x24`, further floats and bytes up to VERB_ON at `0x30` and MONO at `0x31`. Matches the bindings; the `0x50`/`0x5c`/`0x60` floats are read but not yet bound.

## Jump: the core is a JCM800-style chain (confirmed names, not yet the laws)

The constructor `0x140a86880` builds these embedded objects (class names from RTTI):
`WaveShapingTables::JCM800BoostStage`, `JCM800Stage1`, `JCM800Stage2`, `JCM800SplitterP`, `JCM800SplitterN`, `JCM800PowerTube`, plus `TableBase` tables. The ctor also writes initial state: `0x3f800000` (1.0) and `0x3cf89219`/`0x3d0186e3` float constants, and a `0x3f000000` (0.5) group at `0x44a4..0x44b4`. These are the starting values before knobs are applied.

So Jump is a preamp chain: boost stage, two gain stages, a P/N splitter feeding a push-pull power tube table, then the tone and presence stages. The table data and the knob laws are not yet read.

## Vtable slots (Jump, not yet bound to roles)

Slots 0..39 of `0x144fc0330` point at: `0x140a92f80` (copy/assign-style), `0x140a9bd30` (2902 bytes, the largest, likely the kernel), `0x140a96190` (state copy, read), `0x140ab7e10`, `0x140a98850`, `0x140aa33c0`, `0x140a93ae0`, `0x140a94970`, `0x140a934c0`. Small thunks (under 15 bytes) are also present. Next step: read `0x140a9bd30` and `0x140ab7e10` for the process and apply entry points.

## Gap table

| Module | Native id | Engine core (`crates/sampler-core/src/dsp/`) | Format mapping | Reader layout | Process / laws | Tests |
|---|---|---|---|---|---|---|
| Twang | 0x23 | covered (`color/amp/twang.rs`) | covered (`effects/color.rs:96`) | measured | tables dumped under kemu, not re-derived | `matches_the_native_twang` |
| Jump | 0x4d | none | unmapped, diagnosed | confirmed | JCM800 chain located, tables and laws not read | none |
| Van51 | 0x52 | none | unmapped, diagnosed | confirmed | not read | none |
| Super Fast 100 | 0x76 | none | unmapped, diagnosed | confirmed (layout only) | not read; no bindings | none |
| Reverb Delight | 0x7a | none | partial (MONO unproved) | confirmed | not read | none |

## Measurement (Wine oracle, ramp stimulus, 2026-10-21)

Harness: `tools/kontakt-oracle/stimrender.py` with specs from `tools/kontakt-oracle/ampspecs.py`. Renders are in `~/.cache/kontra-amps-b/` and are not committed.

- Reverb Delight: all 31 steps rendered. Default peak 0.567 on a 0.766 bypass peak. Knob sweeps change the tail only.
- Jump: default peak 0.194. PREAMP 0 peak 0.021. Steps after PREAMP have no audio above threshold.
- Van51: default peak 0.471. PREGAINRHYTHM 0 near silence. Steps after that have no audio above threshold.

The NaN steps are not a law. The cause is unconfirmed.

## Open questions

- Jump: read the process entry and the stage tables. Confirm the knob laws (`0x140a9bd30`, `0x140ab7e10`).
- Van51: find its vtable process entry, then the channel switch and the resonance and presence stages.
- Super Fast 100: find the bindings (none yet). Then its process entry.
- Reverb Delight: the tremolo and spring cores, and the MONO field encoding.
- Harness: NaN gaps in Jump and Van51 sweeps.

## Status

No shared-engine change in this unit yet. The four unmodelled modules stay diagnosed as unproved. The engine work needs the process entries above read first. That is more than one session of work.

Session 2026-10-22 (CPU-limited, decompile text only, no oracle or build runs):

- Jump chunk entry `0x140aa33c0` splits the block at the 32-sample control boundary, calls `0x140acb910` (block-rate state update), then the kernel `0x140aac6f0` on each sub-block, then `0x140ac21f0`. Confirmed from the decompile text.
- Kernel prologue (confirmed): the 32-sample accumulator at `param_2+0xac010` is cleared, then each of the `*(param_1+0x124)` input lanes is added scaled by `1.0 / count` (`DAT_14470bad4` = 1.0). The stage chain after this loop is not yet read.
- The constants generator (`tools/kontakt-oracle/gen_amp_consts.py`) was run against `re-binaries/Kontakt8.exe`. It emits 2612 scalars. The output is a cache artifact (`~/.cache/kontra-amps-b/jump_consts.rs`), not committed: 165 KB of globals with no consumer yet. Commit only the constants a read stage actually uses.
- Still unread: stage order and per-stage tables (`0x140aac6f0` after the input sum), knob laws (`0x140a9bd30`, `0x140ab7e10`), Van51, Super Fast 100 and Reverb Delight. The Jump preset in `amp/jump.rs` is still the one-trace fit, not the kernel.

## Implementation (Jump, shared amp chain)

- Shared core: `crates/sampler-core/src/dsp/color/amp/chain.rs`. Input gain, one-pole pre high-pass, a 17-knot piecewise-linear shaper table over -1..1, one-pole post low-pass, and a fixed latency ring (`MAX_DELAY` = 64 samples). A product preamp is a `Spec` preset. The shared engine is `dsp/color/amp/`, which dispatches on `AmpModel`.
- Jump preset: `crates/sampler-core/src/dsp/color/amp/jump.rs`. Fitted by `tools/kontakt-oracle/fit_chain.py` to the Wine oracle's default-step left channel (`jump_default.f32le`, 28800 samples). Fit: pre HP 160 Hz, post LP 3 kHz, gain 2, latency 48, relative RMS 0.242. The fitted table is close to a linear inverting slope, so the nonlinearity is not captured.
- Mapping: `crates/sampler-kontakt/src/effects/color.rs` (module 0x4d, 8 fields) to `AmpModel::Jump`. Jump's knobs are stored but not used, and a note is emitted that says so (`effects/mod.rs`, Typed core branch).
- Tests: `chain::tests::identity_chain_is_a_null_path` (bit-tight null path), `chain::tests::delay_is_exact`, and `jump::tests::default_trace_within_fit_tolerance` (relative RMS below 0.25 against the native default trace).
- Not done: knob laws, the per-stage tables and the kernel's stage order (`0x140aac6f0` is 3380 bytes of vectorised code; only its input-sum loop has been read). Van51, Super Fast 100 and Reverb Delight remain unmodelled.
