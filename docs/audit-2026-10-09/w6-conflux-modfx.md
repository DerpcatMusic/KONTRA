# Conflux 381 modulation/FX intent and first v1 port

Scope: saved `Conflux.nki`, pinned v1 `0cb7a8a0`, exact381 base `54d9a5c5` plus W6 controls `4507cb1e`. This is a source/untimed fidelity change, not CPU acceptance or Kontakt audio parity. The input journal has unique reason subjects; `journal_rows=1` is not library incidence. Scripts and group records were read in memory before choosing the fix. No decrypted scripts, sample PCM or private identifiers are retained.

## Intent evidence

The instrument has 92 groups: one sampler, 90 mode3 sources and one mode9 wavetable. All track keys and are neither saved-muted nor saved-soloed. The mode9 source has serialized forms17/1 (native16/0), no phase randomization, no inharmonic oscillator and modulation type0. That is precisely the source subset admitted by v1. Its authored scripts expose wavetable position, both forms, modulation and inharmonic controls; three init/note/release callbacks and 292 UI callbacks make this an interactive instrument.

The six saved wavetable destination families have zero depth. All 550 RandomBipolar source reports likewise concern saved zero-depth routes. There are 182 unmodeled LFO records (version0x73, waveform6, one nonzero wave weight). The raw census includes four nonzero pan targets; they cannot be declared inaudible just because most targets are zero.

The 2006 module-parameter losses are predominantly routes into source modules. There are 198 nonzero Constant-to-intensity targets across physical source slots1/2/3, rather than 2006 missing filter-cutoff routes. Eleven nonzero internal filterCutoff targets already translate. Of91 raw group filter slots,82 are active and9 bypassed. The missing instrument convolution at rack0 slot3 is saved-bypassed; the active rack0 slot7 module0x13 is separately modeled.

## Audible priority

| Rank | Class | Saved intent / impact | v1 comparison | Kontakt evidence / remaining limit |
|---|---|---|---|---|
| 1 | Admitted base wavetable source | v381 treats the oscillator table as a finite sample. Wrong source clock, cycle morph and phase-form timbre whenever that layer is selected. | Direct regression: v1 has a supported oscillator for this exact saved state. **Port in this change.** | Port preserves v1's2048-cycle/Hermite/morph/ASYM2MP law. A fresh native audio comparison is still UNKNOWN. |
| 2 | Source-module intensity and waveform6 LFO | 198 nonzero saved intensity targets and182 lost LFO sources; four raw nonzero pan targets. Can change modulation movement. | Pinned v1 does not admit waveform6/v0x73 or these frequency/intensity source-module laws; no supported v1 evaluator to copy. | Needs measured native source timing, type6 weights and module-target laws. Counts are not an audible A/B. |
| 3 | Saved automation target | One saved CC2 assignment outside modeled script-slider tags. May make controller interaction incomplete. | No verified equivalent from this census. | Binding subject remains typed/unresolved; cannot infer its target from a hash. |
| 4 | Six wavetable modulation targets / RandomBipolar | Zero saved depths; UI/script edits can make them audible later. | Pinned v1 also leaves these modulation routes unsupported. | Remain reported, not silently erased. |
| 5 | Convolution | One missing IR/effect, but bypassed in this saved patch. Enabling it remains incomplete. | Resource/DSP parity not established by this probe. | Needs library-aware IR resolution before activation; no third-party host was opened. |

The90 mode3 source warnings are retained: both pinned v1 and381 use ordinary sampler playback there. That is **already equal to v1**, not a claim of native time-mode fidelity. Other FX/source capabilities absent from pinned v1 remain v2-only or unproved rather than invented ports.

## Port and boundaries

The kernel is copied from `0cb7a8a0:src/engine/wavetable.rs`, with literal `src/engine/voice.rs::hermite` arithmetic. It adapts reads to core's `ReadFrames`, uses the caller's output, and keeps phase through arbitrary block fragments. The source uses440Hz at MIDI69 and2048-frame cycles, independent of the sample's rate/root; zone/group fine tuning and note expression remain applied. Pitch admission permits positive finite oscillator increments rather than the sampler's1/256..16 resampling bounds. Only identity and native16 ASYM2MP forms, key tracking, no random phase, no inharmonic mode and no modulation oscillator are admitted. Unsupported state keeps its diagnostic.

`ir::Group.wavetable` holds the forward-decoded saved scalar descriptor. `Playback` carries it to the cursor. The existing rare geometry pool stores descriptors so ordinary per-region cursor-template size stays unchanged. The oscillator branch precedes sampler dispatch; W9's `render_run`, fused resample/mix and whole-voice port are untouched. W9 must reject a nonempty wavetable descriptor before ordinary native Hermite admission.

Streaming keeps the complete table ranges resident, even under lazy policy, and pins admitted AssetIds across trim/purge. Mandatory ranges are merged before accounting, AssetIds deduplicate, checked byte sizing rejects a budget deficit before head reads or decoder threads. Retained bytes use actual packed-head accounting. Ordinary lazy/trim behavior is unchanged.

Init scalar writes use the existing `EngineParameterLaw::Linear` conversion. Live source controls are **not yet admitted**: W5 confirmed the catalog names exist but no live WT group controls are lowered. No dummy bindings or private write mirrors are created. The instrument retains a live-source diagnostic; W5 owns connection through physical `EngineParameterBinding` addresses and the consumer must read those same controls. Form-mode, inharmonic and modulation-oscillator edits beyond this subset are not claimed.

## Validation

Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w6-conflux-modfx-381/`.

- Genuine installed Rust translator RED: admitted saved source still emitted one `wavetable source` loss (`intent2.log`, exit101).
- Four core fixtures GREEN: all12 position/form cases equal the literal v1 oracle bit-for-bit across1/127/3/128/17/64/256/428 fragments; zero callback heap calls. Frequency probes cover MIDI57/69/81/127 at44100/48000, within one crossing per second, and sustain beyond the table length. IR lowering proves mapped root12/sample rate8000 do not override oscillator pitch; authored+12 semitones gives880Hz. Incomplete cycles reject before rendering.
- Existing compact-template/independent-loop-release fixture GREEN; ordinary template size retains its invariant.
- Final normal-FIFO check GREEN (`final-check.log`, exit0): four oscillator tests, eight existing cursor tests, three Kontakt admission/residency tests, one installed Conflux census and three KSP offset tests (19 total). Affected core/Kontakt/IR packages pass `test --no-run`. Installed translation has one admitted wavetable group, one mapped zone and nonzero saved group gain; the saved `wavetable source` loss is one before and zero after. The live-engine-parameter loss remains explicit.

No fresh native Kontakt rendering, timed CPU/RSS comparison, all-corpus scanner or whole workspace suite was run. Their acceptance remains UNKNOWN/HOLD; batch integration owns the broad gate. This closes the admitted saved source loss, not all Conflux modulation or source-control parity.
