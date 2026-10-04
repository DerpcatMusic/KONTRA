# Feature status

**KONTRA is alpha software.** Expect major changes to features, UI, compatibility and saved-state formats, plus incomplete behavior and regressions.

Source reviewed: `d36380b`, 2026-10-03. This inventory compares development code with official Kontakt, KSP, Falcon and UVIScript documentation. It describes implementation, not certified sound equivalence or the contents of every nightly. Existing tests are cited as evidence; this documentation audit did not rerun the audio/host suites. See [CI](CI.md) for build gates and [the compatibility checklist](COMPATIBILITY.md) for deeper validation limits.

**✓ Implemented** within the stated scope · **◐ Partial** with a working subset and known limits · **✗ Missing** from the relevant implementation path. Recognition, metadata preservation and a drawable placeholder do not count as playback or interaction. Unverified behavior is identified in the notes rather than marked missing. A ✓ does not mean complete Kontakt/Falcon parity.

The sections below expand independently. Three columns keep the tables narrow; the last column combines proof and the reason for a partial/missing status. Code links are pinned to the reviewed commit so implementation evidence remains reproducible as development continues.

[Formats](#formats-and-content) · [Playback](#playback-and-storage) · [Modulation](#modulation-and-performance) · [Filters](#filters-and-effects) · [KSP](#ksp-scripting) · [UI](#interface) · [Routing](#rack-routing-and-hosts) · [Falcon](#falcon-and-uviscript)

## Formats and content

<details>
<summary>Kontakt formats, library resources and authoring</summary>

Baseline: NI's [file formats][ni-formats] and [Classic view][ni-classic] describe instruments, multis, banks, snapshots, sample resources and instrument editing.

| Capability | Status | Proof / remaining gap |
| --- | :---: | --- |
| NKI instrument loading | ◐ | [Importer](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/import.rs#L724) builds groups/zones and selected native state. Unsupported sources, inserts and modulation can leave an instrument only partly functional. |
| NKS / NIS preset wrappers | ✓ | [Container reader](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/import.rs#L217) unwraps supported presets; bounded input, expansion and nesting. Not every NI container type. |
| NKM multi loading | ◐ | [Multi reader](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/import.rs#L265) reads parts; [import warnings](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/import.rs#L900) explicitly exclude native multi routing, master processing and multi scripts. |
| Instrument-bank program switching | ✗ | [Multi reader](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/import.rs#L265) rejects slots containing multiple programs. NI's NKB bank switching requires that behavior; direct NKB loading is also unverified. |
| NKSN snapshots | ◐ | [Snapshot reader](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/import.rs#L289) needs the base NKI and matching identity; not a standalone instrument or universal saved-state restoration. |
| NKX / NKR resource access | ✓ | [Resolver](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/import.rs#L1048) reads supported archive members as virtual paths; [sample test](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/import.rs#L1324) exercises decoding. Not every archive layout/cipher. |
| Protected library access | ◐ | [Access metadata](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/access.rs#L21) supplies keys from local library metadata; [sample access](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/audio.rs#L816) rejects unavailable keys and legacy NKX cipher paths. No blanket protected-library support. |
| WAV / AIFF sample loading | ✓ | [Audio reader](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/audio.rs#L1152) decodes supported mono/stereo PCM files. |
| NCW sample loading | ✓ | [NCW reader](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/audio.rs#L1109) decodes supported mono/stereo lossless samples. |
| Multichannel sample files | ✗ | [Sample reader](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/audio.rs#L1152) rejects more than two channels, unlike NI's multichannel WAV/AIFF support. This is separate from multiple host outputs. |
| Missing-sample resolution | ◐ | [Resolver](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/import.rs#L1046) tries relative/archive paths and unambiguous suffix matches; ambiguous names fail. Native interactive relinking and resaving are not established. |
| NKP / NKC / NKL parity | ◐ | NI lists script/module presets, cache files and Leap kits. [Preset discovery](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/import.rs#L1170) establishes NKI/NKM/native paths; these other native workflows remain unverified, not implied by KONTRA's own cache. |
| Build libraries from loose samples | ✓ | [Creator](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/creator.rs#L549) scans WAV/AIFF, plans mappings and writes native instruments, samples and a library manifest. |
| Generate NKI / SFZ instruments | ◐ | [Creator](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/creator.rs#L610) writes selected generated group/zone/script state and SFZ. Arbitrary Kontakt instrument editing/export equivalence is not established. |
| Native start-record preservation | ✓ | [Compatibility evidence](COMPATIBILITY.md#containers-samples-and-saved-state) records roundtrip preservation; preserving bytes does not execute every start criterion. |

</details>

## Playback and storage

<details>
<summary>Sampler DSP, loops, envelopes, streaming and synthesis</summary>

Baseline: NI's [Source Module and Sample Loop documentation][ni-classic] distinguishes ordinary resampling, disk streaming, independent time/pitch modes, wavetable playback and multiple loop regions.

| Capability | Status | Proof / remaining gap |
| --- | :---: | --- |
| Key / velocity mapping and transposition | ✓ | [Voice creation](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/mod.rs#L1797) applies sample rate, root/key tracking and tuning. Pitch changes playback rate. |
| Sample interpolation | ✓ | [Voice reader](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/voice.rs#L1750) uses four-point Hermite interpolation; no claim of matching NI's interpolation modes. |
| Reverse sample playback | ✓ | [Play map](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/map.rs#L166) starts at the end marker; [copy path](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/voice.rs#L1713) reverses frame runs. |
| Forward loops / release tails | ✓ | [Loop map](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/map.rs#L99) and [release handling](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/voice.rs#L1657) support the selected wrap/until-release paths. |
| Alternating / ping-pong loops | ✓ | [Loop map](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/map.rs#L227) reflects the path and handles release without duplicating endpoints. |
| Loop crossfades | ◐ | [Map](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/map.rs#L46) blends forward boundaries; [bank preparation](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/bank.rs#L1349) converts alternating-plus-crossfade to forward crossfade. |
| Reverse combined with ordinary loops | ◐ | [Map](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/map.rs#L99) cycles reverse playback only for alternating loops; other combinations need reference fixtures. |
| Multiple active loops per zone | ✗ | [Zone](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/import.rs#L160) and [play map](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/map.rs#L24) retain one optional loop, versus NI's multiple regions. |
| AHDSR envelopes | ✓ | [Envelope tests](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/ahdsr.rs#L370) exercise block-independent rendering, finite release and no heap allocation. |
| Flex / pitch / filter envelopes | ◐ | [Voice runtime](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/voice.rs) and [modulation import](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/modulation.rs) implement selected paths; extra modes, targets and loop/one-shot semantics remain incomplete. |
| Disk streaming / RAM playback | ◐ | [Streaming modes](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/bank.rs#L50) support resident heads, streamed tails and RAM-first loading; RAM budget overflow can stream. Not NI's complete per-group DFD controls. |
| Adaptive preload / residency | ✓ | [Residency](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/residency.rs#L369) assigns hot/base/cold tiers under memory pressure; this is KONTRA's policy. |
| Bounded streaming cache | ✓ | [Stream tests](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/stream.rs#L800) exercise reader/block caps and eviction. Disk underruns can still cause silence. |
| Large sample-start offsets | ◐ | [Streaming and voice boundary](COMPATIBILITY.md#containers-samples-and-saved-state) supports offsets; real-disk onset timing remains unverified. |
| Polyphony and voice stealing | ✓ | [Player](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/mod.rs#L1993) makes room for voices; [engine limits](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/mod.rs#L58) and load shedding bound capacity. Not a universal performance guarantee. |
| Time / Tone / Beat Machine engines | ✗ | [Voice playback](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/voice.rs#L1156) changes traversal rate for pitch; independent stretch, granular formants and beat-slice engines are not implemented in this path. |
| Time Machine Pro | ✗ | [KSP calls](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/calls.rs#L1098) explicitly report unavailable stretch/voice allocation, separate from disk streaming. |
| Kontakt wavetable playback | ◐ | [Admission](../src/engine/wavetable.rs) allows tracked 2048-frame cycles with linear, bend, asymmetric, PWM, flip, mirror, quantize, seesaw and exp/log phase forms traced in the native phase switch. Sync readout, sample-domain shapers, phase randomness and inharmonic/audio-rate modulation remain unsupported; quality/anti-aliasing and Kontakt output parity are unverified. |

</details>

## Modulation and performance

<details>
<summary>Modulators, MIDI, pedals and expressive playback</summary>

Baseline: NI's [modulation reference][ni-modulation], [engine parameters][ksp-engine] and [Classic view][ni-classic] specify source/target routing, LFO modes and group start behavior.

| Capability | Status | Proof / remaining gap |
| --- | :---: | --- |
| Imported modulation assignments | ◐ | [Decoder](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/modulation.rs#L180) preserves readable slots and reports unsupported categories/shapers; preservation alone does not modulate audio. |
| Saved sine Multi LFO → pitch | ◐ | [Admission](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/modulation.rs#L282) and [clock](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/lfo.rs#L69) implement selected retriggered sine-only paths, phase and legacy fade. |
| Saved sine Multi LFO → volume | ◐ | [Admission](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/modulation.rs#L301) requires one target, zero source fade and nonnegative lag; [target runtime](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/lfo_volume.rs#L1) applies the selected volume law. |
| LFO live depth / bypass | ◐ | [Parameter routing](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/params.rs#L700) supports admitted pitch-target depth and source bypass. Volume intensity and live source timing/frequency/phase remain unsupported. |
| General LFO waveforms / free-running modes | ✗ | [Clock boundary](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/lfo.rs#L1) explicitly excludes other waveforms and free-running clocks. Selected saved timing does not imply full tempo-sync or live-clock parity. |
| Envelope target depth / bypass | ◐ | [Modulation](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/modulation.rs#L441) and [parameter routing](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/params.rs) support known targets; unknown laws, polarity/mode fields and additional targets remain gaps. |
| MIDI note on/off and channel ownership | ✓ | [Engine](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/mod.rs#L819) handles channel/owner-aware notes and releases. |
| CC / pitch bend / aftertouch | ✓ | [Engine controller paths](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/mod.rs#L890) cover CC, bend, channel and poly pressure; authored script behavior can still be partial. |
| Sustain / release-group behavior | ◐ | [Pedal handling](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/mod.rs#L2197) and [release triggers](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/mod.rs#L2153) implement selected ownership/release paths; every saved group condition is not established. |
| MPE | ◐ | [Articulation test](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/articulate.rs#L2576) exercises per-note bend/pressure/CC74 and RPN bend range. Broader device/host negotiation needs validation. |
| App articulation keyswitch routing | ◐ | [Remapping test](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/articulate.rs#L1586) exercises app routing; this does not execute imported native Start on Key conditions. |
| Native key / controller / round-robin / random group starts | ✗ | [Importer](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/import.rs#L765) explicitly retains these records without evaluating them. KSP-driven selection is a separate path. |
| Legato behavior | ◐ | [Timing test](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/timing.rs#L1150) exercises articulation/channel scheduling; full note-transition sample switching is not established. |
| General external modulation matrix | ◐ | [Assignment runtime](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/params.rs#L69) handles a bounded source/target subset, with eight volume/pitch assignments per voice; retained unsupported routes may have no audio effect. |
| Native step modulation | ✗ | [Internal-modulator import](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/modulation.rs#L180) skips undecoded kinds; the audited player has no native step-sequencer path. Scripts can implement separate sequencing behavior. |

</details>

## Filters and effects

<details>
<summary>Filter models, effect families, inserts and convolution</summary>

Baseline: NI's [filter reference][ni-filters] and [effect reference][ni-effects]. A shared effect name is not evidence of equal DSP, nonlinear behavior or every control being mapped.

| Capability | Status | Proof / remaining gap |
| --- | :---: | --- |
| LP / HP / BP, 2- and 4-pole filters | ✓ | [Type dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/filter.rs#L68) maps the supported standard and alias IDs. |
| Notch / legacy 6-pole LP filters | ✓ | [Type dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/filter.rs#L68) includes selected IDs; newer SV 6-pole HP/BP/notch forms are missing. |
| Phaser filter | ◐ | [Model](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/filter/models.rs#L108) implements notch geometry but omits resonance feedback. |
| Ladder / analog ladder filters | ◐ | [Type dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/filter.rs#L68) includes Ladder LP4; remaining LDR variants and HQ oversampling are missing. The saved HQ flag uses single-rate DSP with a warning. |
| Adaptive Resonance filters | ◐ | [Model](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/filter/models.rs#L78) supplies compensated linear ladder responses; NI's input-level-dependent resonance adaptation is missing. |
| DAFT filters | ◐ | [Type dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/filter.rs#L68) uses linear LP/HP proxies; not the complete NI model. |
| Formant filter | ◐ | [Type dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/filter.rs#L68) includes Formant; type 1 uses three vowel-positioned bell bands; Formant 2 is missing and NI model equivalence is not certified. |
| 3x2 Versatile filters | ✗ | [Type dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/filter.rs#L68) excludes type 19; no three-band morph/shift/bypass/gain model. |
| Dedicated parallel / composite filters | ✗ | [Type dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/filter.rs#L68) excludes the native multi-filter topologies; serial insert processing is not a parallel filter model. |
| Monark / Dual SKF / PRO-53 | ✗ | [Type dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/filter.rs#L68) lacks these subtypes and their FM/feedback models; unsupported IDs pass through with warnings. |
| Every Kontakt filter subtype | ✗ | [Capability warnings](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/filter.rs#L549) identify unsupported IDs/pass-through. Script constants and preserved subtype IDs exceed DSP coverage. |
| Group filter / EQ insert order | ◐ | [Group stages](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/filter.rs) support eight slots, bounded sections and decoded Amplifier splits; unknown placement/families remain gaps. |
| EQ / Solid G-EQ | ◐ | [Block dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/blocks.rs#L1016) provides rack filter/EQ and Solid G-EQ paths; full model/control equivalence is unverified. |
| Gain / stereo / send-level stages | ✓ | [Processor](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/processor.rs#L249) handles taps/sends and [stage construction](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/processor.rs#L875) handles gain/stereo. Placement support remains scoped to implemented chains. |
| Standard delay | ◐ | [Delay defaults](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/blocks.rs#L247) and [dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/blocks.rs#L1016) implement the core delay; bounded delay length and incomplete NI control parity. |
| Chorus / flanger / phaser effects | ◐ | [Block dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/blocks.rs#L1044) runs the base Sweep/Phaser DSP; not Choral/Flair/Phasis parity. |
| Distortion / Lo-Fi / Skreamer / tape saturation | ◐ | [Drive dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/blocks.rs#L400) supplies kernels; [group warnings](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/filter.rs#L549) identify omitted damping and unverified saturation-mode transfer. |
| Compressor / feedback compressor / limiter | ◐ | [Dynamics dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/blocks.rs#L635) implements these kernels; full NI parameter and reference-render parity is unverified. |
| Solid Bus Compressor / Transient Master | ◐ | [Block dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/blocks.rs#L1016) implements stages; group placement needs a decoded Amplifier split. |
| Algorithmic reverb | ◐ | [Reverb](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/reverb.rs#L53) implements predelay, diffusion, damping, modulation and decay; [parameter checks](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/params.rs#L1651) identify unmapped frequency controls. |
| Convolution reverb | ◐ | [Eligibility](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/mod.rs#L182) requires an IR; [swap test](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/tests.rs#L552) preserves wet/dry. Independent early/late processing, reverse and unknown flags remain incomplete. |
| Replika / PsycheDelay | ✗ | [Block inventory](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/blocks.rs#L247) and [stage dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/processor.rs#L875) have no DSP for these modern delay modules. |
| Choral / Flair / Phasis / Ring Modulator / Rotator | ✗ | [Block inventory](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/blocks.rs#L247) lacks these native modules; simple Chorus/Flanger/Phaser kernels do not replace them. |
| Supercharger GT / Transparent Limiter / Plate Reverb | ✗ | [DSP gate](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/kind.rs#L175) and [stage dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/processor.rs#L875) provide no processing for these named modules. |
| Multichannel / surround convolution | ✗ | [Convolution construction](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/processor.rs#L875) creates two convolvers, not the native multichannel/surround IR path. |
| Every named Kontakt effect | ✗ | [DSP gate](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/kind.rs#L175) requires an implemented stage/defaults; [block inventory](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/fx/blocks.rs#L247) is a strict subset of NI's reference. Names in the enum do not make Replika/Psyche, Choral/Flair/Phasis or other modules work. |
| Live effect parameters / replacement | ◐ | [Normalized parameters](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/params.rs) support selected edits/readback; unknown laws, type replacement and every bus placement remain incomplete. |

</details>

## KSP scripting

<details>
<summary>Language, callbacks, APIs, persistence and unsupported services</summary>

Baseline: NI's [KSP manual][ksp], [callbacks][ksp-callbacks], [variables][ksp-vars], [time commands][ksp-time], [load/save][ksp-load], [zone commands][ksp-zone] and [MIDI-object commands][ksp-midi]. Language comparisons also use NI's [operators][ksp-ops], [control statements][ksp-flow], [functions][ksp-functions], [preprocessing][ksp-advanced], [Multi Script][ksp-multi] and [event commands][ksp-events].

| Capability | Status | Proof / remaining gap |
| --- | :---: | --- |
| KSP compiler / VM | ◐ | [Compiler](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/compile.rs) and [VM](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/vm.rs) execute a supported language subset; not every KSP construct/builtin. |
| Operators / control flow | ✓ | [Parser](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/parser.rs#L391) and [compiler](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/compile.rs#L1025) handle KSP expressions, if/while/continue/select and case ranges. |
| User functions | ✓ | [Parser](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/parser.rs#L217) supports declarations/calls without arguments or return values, matching KSP's function contract. |
| Variables / arrays | ◐ | [Compiler](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/compile.rs#L801) supports six scalar/array sigils and fixed lengths; adds a 16-million-total-element ceiling. |
| Conditional preprocessing | ✓ | [Preprocessor](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/parser.rs#L13) handles SET/RESET_CONDITION, USE_CODE_IF/IF_NOT and END_USE_CODE with nesting. |
| Kontakt Multi Script | ✗ | [Callback dispatch](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/compile.rs#L373) omits `on midi_in`; [import](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/import.rs#L900) also warns multi scripts are not restored. |
| Kontakt 8.12 fade-curve arguments | ✗ | [Fade calls](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/calls.rs#L591) discard the optional curve argument and use fixed fades; accepting an argument does not apply it. |
| Callback registration / dispatch | ◐ | [Compiler](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/compile.rs#L373) recognizes MIDI/UI/listener/PGS/persistence/async families; recognition does not prove every trigger/context. |
| Waits and timed listeners | ◐ | [Scheduler](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/runtime.rs#L2320) resumes threads and listeners with bounded fuel; complete NI timing/transport edge behavior remains unverified. |
| Async completion delivery | ✓ | [Runtime](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/runtime.rs#L2440) delivers completion ID/status. Individual asynchronous services still need their own implementation. |
| PGS shared state | ◐ | [Runtime](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/runtime.rs#L2466) dispatches changes; broader ordering and library dependencies remain partial. |
| Persistent script values | ◐ | [Runtime](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/runtime.rs#L1363) and [roundtrip test](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/tests.rs#L1706) retain registered values; exact NKP/snapshot/code-replacement semantics are not fully established. |
| Engine parameter get/set | ◐ | [Address routing](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/params.rs) implements selected normalized controls; unsupported targets are not made functional by returning a value. |
| NKA array file operations | ◐ | [Worker-backed arrays](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/arrays.rs) and [playback test](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/plugin.rs#L5433) load selected files; dialogs and full filesystem semantics remain incomplete. |
| File-selector selected-path queries | ◐ | [Picker and callback boundary](COMPATIBILITY.md#ksp-and-imported-interfaces) supports selected filename/stem/path. Embedded columns and navigation remain unsupported. |
| Standard MIDI File APIs | ✗ | [Compiler fallback](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/compile.rs#L1553) diagnoses unknown `load_midi_file` / `mf_*` calls as no-op/zero; no SMF decoding/cursor service. A positive registry alone is not proof. |
| Zone / sample / slice APIs | ◐ | [Zone calls](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/calls.rs#L949) accept GROUP/LOW_KEY/HIGH_KEY and enforce native-zone snapshot modes; other parameters, slice queries and selected event/drop arrays remain unsupported. |
| Array load dialogs / mode-based saves | ✗ | [Array calls](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/calls.rs#L445) explicitly reject mode-0 load dialogs and mode-based saves; explicit-path worker operations are separate. |
| File selector `fs_navigate` | ✗ | [Calls](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/calls.rs#L1520) explicitly report navigation unavailable; opening a native picker does not implement previous/next callbacks. |
| Unknown functions / runtime faults | ◐ | [Tests](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/tests.rs#L804) exercise diagnostics/budgets; unsupported function fallback is not an implementation of that function. |
| Native NCKP control preparation | ◐ | [Compatibility evidence](COMPATIBILITY.md#ksp-and-imported-interfaces) covers selected exported records and slot bindings; arbitrary schema/hierarchy/property support remains incomplete. |
| Komplete UI | ✗ | [Compiler diagnostic](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/compile.rs#L1553) explicitly rejects `load_komplete_ui`; it does not provide NI's [Komplete UI][ni-komplete] framework. NCKP/fallback controls do not establish native UI execution. |

</details>

## Interface

<details>
<summary>Library browser, instrument panels and individual KSP widgets</summary>

Baseline: NI's [browser][ni-browser], [KSP controls][ksp-controls], [control parameters][ksp-control-pars] and [UI commands][ksp-ui]. A declared control or stored value is separate from rendering and user interaction.

| Capability | Status | Proof / remaining gap |
| --- | :---: | --- |
| Library roots / artwork assignment | ✓ | [Drop routing](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/mod.rs#L809) adds directories and assigns PNG/JPEG artwork. Not NI library installation/activation. |
| Name search / clear / selection | ✓ | [Search control](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/browser.rs#L1045) and [selection test](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/tests.rs#L979) exercise these actions. |
| Kontakt browser metadata parity | ◐ | [Browser](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/browser.rs) has library/search surfaces; full NI tag/bank/category/favorites/recommendation behavior is not verified by this audit. |
| Instrument / multi / snapshot drop loading | ✓ | [Drop routing](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/mod.rs#L839) queues multis/snapshots and adds or replaces instrument parts. |
| Loading / scan progress | ✓ | [Header](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/header.rs#L465) shows fraction/sweep; [scanner](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/library.rs#L966) reports folder/item progress. |
| Load failures / diagnostic logs | ✓ | [Diagnostics](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/diagnostics.rs) and [Logs UI](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/logs.rs) retain partial-load/failure context. A completed load does not prove correct sound. |
| Original / reconstructed instrument views | ◐ | [Performance view](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L443) renders selected authored controls/assets; fallback layout does not reproduce every instrument. |
| Knobs / sliders / value edits | ✓ | [Interaction](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L899) handles drag/value editing and sends script control edits. |
| Buttons / switches / menus | ✓ | [Interaction](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L899) handles activation and menu selection. |
| Panels / nested hiding | ◐ | [Panel tests](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L1522) verify relative placement/hiding; panels themselves do not draw a full native widget. |
| Labels / pictures / wallpaper / fonts | ◐ | [Text and pictures](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L634) support selected bitmap/state fonts and assets; exact factory metrics and arbitrary resource formats remain incomplete. |
| `ui_table` | ◐ | [Face](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L1088) draws array bars; [interaction](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L899) excludes table pointer editing and indexed callbacks. |
| `ui_waveform` | ◐ | [Waveform path](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L899) draws peaks/cursor; slice editing, slice tables and MIDI dragging are missing in this path. |
| `ui_xy` interaction / specific rendering | ✗ | [Kind mapping](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L65) falls back to Other; [interaction](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L899) supplies no XY input. |
| `ui_level_meter` live attachment | ✗ | [Face](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L1088) paints a static appearance without attached levels. This refers to the imported KSP widget. |
| `ui_wavetable` visualization | ✗ | [Kind mapping](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L65) lacks a dedicated renderer; a wavetable audio engine does not supply this UI. |
| `ui_mouse_area` events / drops | ✗ | [Interaction](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L899) disables this control's input; invisible painting is not an event target. |
| `ui_text_edit` text input | ✗ | [Typing](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L557) serves value edits; [control interaction](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L899) excludes text-edit input. |
| File-selector widget | ◐ | [Interaction](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/perf_view.rs#L899) launches a native picker with source epoch/base/type. Not Kontakt's complete embedded browser/navigation UI. |

</details>

## Rack, routing and hosts

<details>
<summary>Multis, mixing, outputs, automation and platform builds</summary>

Baseline: NI's [rack/output documentation][ni-classic] and [effects routing][ni-routing]. KONTRA can provide its own working workflow without restoring every imported Kontakt routing decision.

| Capability | Status | Proof / remaining gap |
| --- | :---: | --- |
| Multi-part rack | ✓ | [Rack](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/rack.rs) and [large-rack checks](COMPATIBILITY.md#containers-samples-and-saved-state) support growing storage/viewport rows. Actual capacity depends on workload/host. |
| Mixer mute / solo / part / master controls | ✓ | [Mixer test](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/tests.rs#L2340) exercises strips, buses, mute/solo and reset. |
| Part MIDI port / channel assignment | ✓ | [Mixer test](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/tests.rs#L2358) exercises assignment; full host-port behavior is separately unverified. |
| Part output / drag-to-bus routing | ✓ | [Mixer test](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/tests.rs#L2347) verifies manual selection and bus drops. |
| Mic / per-instrument host outputs | ◐ | [Host test](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/plugin.rs#L8282) exercises generated mic ports and audio delivery; arbitrary channel matrices/widths remain unverified. |
| Aux sends / imported routing | ◐ | [Mixer test](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ui/tests.rs#L2356) sets an aux bus; native multi routing, every send topology and placement are not restored. |
| Host transport into scripts | ✓ | [Transport test](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/plugin.rs#L7178) checks position, tempo, signature and stopped/seeking behavior for its fixture. |
| Script / host automation parity | ◐ | [Script test](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/ksp/tests.rs#L352) verifies IDs/names; every host exposure, parameter mapping and bidirectional path is not certified. |
| Saved state / project migration | ◐ | [Project migration](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/project_migration.rs#L253) and [state tests](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/plugin.rs) provide persistence paths. Alpha format changes and native Kontakt snapshot gaps still apply. |
| AU / AAX packages | ✗ | [Cargo features](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/Cargo.toml) and [nightly packaging](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/.github/workflows/nightly.yml) provide no AU/AAX target, unlike NI's [hosting formats][ni-hosting]. |
| Offline host rendering parity | ◐ | [Engine block setup](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/mod.rs#L466) accepts offline state; host-bounce timing and end-to-end NI parity were not validated in this audit. |
| CLAP / VST3 / standalone builds | ✓ | [Cargo features](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/Cargo.toml) and [nightly workflow](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/.github/workflows/nightly.yml) build/package all three. This is a build claim, not DAW certification. |
| Linux x86_64 | ✓ | [CI](CI.md) runs full Linux tests and packaging; real-library/host sound parity is not certified. |
| Windows x86_64 / macOS arm64 + x86_64 | ◐ | [CI](CI.md) documents native compile/packaging checks, with additional macOS installer checks; full test suites run on Linux, not all targets. |

</details>

## Falcon and UVIScript

<details>
<summary>UFS, programs, synthesis, Lua host behavior and UI</summary>

Baseline: UVI's [Falcon manual][falcon], [UFS description][ufs], [UVIScript API][uvi-api], [Lua host][uvi-lua], [callbacks][uvi-callbacks], [mapping][uvi-mapping] and [UI][uvi-ui], plus the current [oscillator/module catalog][falcon-catalog]. Falcon support is inspection groundwork, **not playback support**. Kontakt DSP and KSP do not automatically satisfy Falcon module or Lua contracts.

| Capability | Status | Proof / remaining gap |
| --- | :---: | --- |
| Observed UFS2 header inspection | ✓ | [Offline inspector](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/tools/inspect_uvi.py#L19) validates a bounded observed header and reports selected fields. Not a soundbank loader. |
| Clear UVIP XML metadata | ◐ | [Inspector](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/tools/inspect_uvi.py#L68) summarizes `UVI4`/Program nodes, attributes and hashes; does not interpret module semantics. |
| UFS mounting / member index / extraction | ✗ | [Inspector report](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/tools/inspect_uvi.py#L33) explicitly excludes directories, programs, scripts and samples. Opaque bytes are not proof of a particular encryption scheme. |
| UVIM multis / executable program hierarchy | ✗ | [Inspector](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/tools/inspect_uvi.py#L68) inventories XML, not Multi→Part→Program→Layer→Keygroup→Oscillator runtime objects. |
| UVI DMAP / dimensional mapping | ✗ | [Groundwork](FALCON_RUNTIME_UI_GROUNDWORK.md#first-implementation-and-verification-gates) identifies missing UVI dimension/mapping lowering and dispatch; Kontakt groups are not equivalent. |
| Falcon sample playback / streaming | ✗ | [Inspector](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/tools/inspect_uvi.py#L88) reports `playback_supported: false`; no decoded UFS sample/program route into the player. |
| Falcon analog / FM / additive oscillators | ✗ | [Voice source model](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/voice.rs#L957) provides sample/optional Kontakt wavetable state, not Falcon's generated analog, operator or partial engines. |
| Falcon granular / wavetable oscillators | ✗ | [Groundwork](FALCON_RUNTIME_UI_GROUNDWORK.md#prioritized-compatibility-map) has no Falcon grain scheduler or native wavetable lowering; Kontakt's table path is a different module. |
| Falcon physical-model sources | ✗ | [Voice source model](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/src/engine/voice.rs#L957) has no Pluck, Bowed String or Harmonic Resonator engine. |
| Falcon step envelopes / arpeggiator | ✗ | [Runtime groundwork](FALCON_RUNTIME_UI_GROUNDWORK.md#prioritized-compatibility-map) lacks UVI modulator/event-processor execution; KSP sequencing is a separate feature. |
| Falcon filters / effects / modulation | ✗ | [Inspector](https://github.com/DerpcatMusic/KONTRA/blob/d36380bbf2761fdfa7e3c50e29c2acfacbcdfb73/tools/inspect_uvi.py#L78) retains metadata without executing the module graph or scoped routing. |
| UVIScript Lua language / host API | ✗ | [Runtime boundary](FALCON_RUNTIME_UI_GROUNDWORK.md#languages-and-hosts) identifies separate Lua 5.1/UVI bindings; the implemented script runtime is KSP. |
| UVI event forwarding / coroutine scheduling | ✗ | [Event boundary](FALCON_RUNTIME_UI_GROUNDWORK.md#event-and-engine-boundaries) needs `onEvent`, forwarding and coroutine semantics; KSP callbacks are a different contract. |
| UVI async sample / IR / MIDI operations | ✗ | [Groundwork](FALCON_RUNTIME_UI_GROUNDWORK.md#first-implementation-and-verification-gates) requires UVI operation IDs, scope and completion behavior; existing KSP workers do not expose these APIs. |
| UVI widgets / parameter bindings / callbacks | ✗ | [UI boundary](FALCON_RUNTIME_UI_GROUNDWORK.md#ui-and-assets) requires typed float/bool/table state, UVI widgets and bidirectional binding; KSP integer controls do not supply them. |
| UVI persistent widgets / `onSave` / `onLoad` | ✗ | [Runtime boundary](FALCON_RUNTIME_UI_GROUNDWORK.md#first-implementation-and-verification-gates) identifies missing state/restore ordering and custom Lua data handling. |
| VWinds / H.A.T. / modeled articulation parity | ✗ | [Evidence boundary](FALCON_RUNTIME_UI_GROUNDWORK.md#evidence-and-missing-evidence) has no executed bank/runtime/reference render; shared expressive primitives do not implement proprietary modeling. |

</details>

## Maintaining this inventory

Update the relevant row when code changes, and keep its vendor requirement, implementation evidence and remaining gap together. Mark ✓ only for an exercised implementation within the stated scope; mark ✗ only for an explicit rejection or confirmed missing execution path. Keep isolated tests, integrated source, shipped artifacts and reference-host comparisons distinct. The [detailed checklist](COMPATIBILITY.md) retains older validation milestones; the Falcon [format](FALCON_FORMAT_GROUNDWORK.md) and [runtime/UI](FALCON_RUNTIME_UI_GROUNDWORK.md) audits explain the groundwork.

[ni-formats]: https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/file-formats
[ni-classic]: https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view
[ni-browser]: https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/browser-and-presets
[ni-modulation]: https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation
[ni-filters]: https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference
[ni-effects]: https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference
[ni-routing]: https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/using-filters-and-effects-in-classic-view
[ni-komplete]: https://developer.native-instruments.com/komplete-ui/docs/GetStarted/
[ksp]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/welcome-to-ksp
[ksp-callbacks]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks
[ksp-vars]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/variables
[ksp-time]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands
[ksp-load]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/load-save-commands
[ksp-zone]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/zone-commands
[ksp-midi]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/midi-object-commands
[ksp-engine]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters
[ksp-controls]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-controls
[ksp-control-pars]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters
[ksp-ui]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands
[falcon]: https://cdn.uvi.net/UVIFC_Falcon/manuals/Falcon_manual_en.pdf
[ufs]: https://support.uvi.net/hc/en-us/articles/201360562-What-is-a-UFS-file
[uvi-api]: https://lua.uvi.net/_a_p_i_page.html
[uvi-lua]: https://lua.uvi.net/_lua_reference.html
[uvi-callbacks]: https://lua.uvi.net/group___event_callbacks.html
[uvi-mapping]: https://lua.uvi.net/_sample_mapping_intro.html
[uvi-ui]: https://lua.uvi.net/_u_i_page.html

[ni-hosting]: https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/installation-and-setup

[ksp-ops]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/arithmetic-commands---operators
[ksp-flow]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-statements
[ksp-functions]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-defined-functions
[ksp-advanced]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/advanced-concepts
[ksp-multi]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/multi-script
[ksp-events]: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands

[falcon-catalog]: https://www.uvi.net/falcon
