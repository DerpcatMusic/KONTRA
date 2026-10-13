# Conflux ranked engine gaps (2026-10-13)

Base: `ce0a2e61`. Source inspection used the existing library inspector in RAM;
raw census used the existing `w6_conflux_intent` test. Every factory snapshot
(201) decodes: each contains zero native groups/racks and 223 saved script
entries. Main synth, mono/portamento and helper init stages report zero faults.
The all-multi source inspection was stopped by the coordinator after the
no-build/no-survey directive; it must not be reported as complete.

The initial real-sample corpus audition loaded 94 zones at key 60/velocity 64,
48 kHz and 64-frame blocks, but recorded zero voices, no selections, no finished
script faults and one stuck note. The runner had no KSP outbox drain. The later
controller-driven pass hit Capacity. These observations do not prove a silent
Kontakt instrument or native sound parity. Fictitious-asset inspector binding
also failed at Regions/InvalidInput; actual sample loading succeeded, so that
placeholder failure is not a product load failure.

The user subsequently prohibited builds, surveys and measurements. Only focused
crate tests/checks are allowed. The coordinator owns the next installed audition.

| Rank | Feature / evidence | Current behavior | Action / remaining concrete boundary |
|---:|---|---|---|
| 1 | Audition callback servicing | The corpus runner never drains emitted KSP effects; a full 256-entry outbox suspends the callback before later musical instructions | Add the existing headless host's owner-checked UI service behavior to the corpus path. A 1,000-label-refresh authored note is the failing-first regression. Then the coordinator repeats the installed note once. |
| 2 | Wavetable table selection; six zone writes, main slot 2 line 2613 | `set_zone_par` is a frontend stub for group/key reassignment; selecting another table does not create the intended source topology | Needs zone membership/key mutation in core and actual KSP lowering. `sampler-ksp/src/lower.rs` is explicitly owned by another worker; report the exact three ZONE_PAR families, do not patch a UI selector. |
| 3 | Sample layers, group gating/purge, mono note routing | These callbacks compile, but a stalled audition never reaches their selection verdict | Outbox fix is the first isolation step. The known set_engine_par/purge_group operand defect is owned by another thread. Group selection cannot be certified by source metadata or a started note handle. |
| 4 | Phase-form amounts and selector | Position/form amounts have three existing physical control lanes, but selectors have no binding; only identity/ASYM2MP are admitted | Implement 13 original-byte-verified phase modes and two addressed held-voice selector lanes. More phase/amplitude forms, source selection and oscillator quality remain separate. |
| 5 | Wavetable constants | NI_WT_* tokens become opaque interned identities; the raw mode ordinal depends on numeric semantics | Focused initializer test compares against v1's explicit valued table. Native symbol registration was inspected but is not itself a numeric evaluator oracle. Do not use registration IDs 1201+ as phase ordinals. |
| 6 | Audio modulation/auxiliary oscillator and inharmonic controls | WT_MOD_TYPE/WAVE/TUNE/TUNE_UNIT/AMOUNT and INHARMONIC/MODE are referenced, but source IR/kernel models only the admitted phase/table subset | Needs the complete native oscillator/mix/FM/PM/RM/inharmonic kernel and its clocks/setters. The phase leaf cannot establish those laws; the old extraction cache is absent and live REA deep analysis is unavailable. Retain diagnostics and avoid guessed sine/FM substitutes. |
| 7 | LFOs and envelopes | Saved Digital Multi sine/fade admission has advanced beyond older audits. Live frequency, wave components, delay, retrigger and primary held-envelope state have distinct consumers | Existing source/route diagnostics remain authoritative. LFO waveform changes require native multi-wave laws, not relabeling sine. Primary held-envelope retiming is explicitly still open in W9's recovered consumer. |
| 8 | Source-to-source motion; 198 nonzero Constant-to-intensity targets | 2,190 module-parameter routes are diagnosed; outgoing depth is not equivalent to scaling a source waveform | Native outgoing-target getter/expression probes exist, but complete combination/live setter/clock rules are not yet admitted. Translation is in `library.rs`; lane kernels are in another worker's `dsp/lanes.rs`. Preserve physical slot/target identity. |
| 9 | Macro/MIDI motion and Animator | Scripts expose six macros, timer callbacks and step values; controller and listener callbacks compile | Real callback completion and resulting engine outcomes must be checked after servicing. Timer drivers legitimately remain pending. KSP ledger status alone is not proof of Animator audio; advanced source/FX destinations remain unavailable. |
| 10 | Random modulation | 550 RandomBipolar source losses remain UnknownLaw | Native signed generator/reset/lifecycle law is not established; v1 also lacks admission. Do not replace it with a guessed uniform signed generator or drop zero-depth diagnostics. |
| 11 | Four FX selectors and modern FX | Saved filter/convolution/module 0x13 topology is retained, but scripts request effect type/subtype changes and BITE/FREAK/CHORAL/FLAIR/PHASIS/RAUM/etc. parameters | Dynamic module replacement and each requested modern processor need actual kernels and parameter setters; `effects.rs` and shared lanes are in use. An engine name in the catalog is not a DSP binding. Static support cannot certify all selectable FX. |
| 12 | Space/IR changes and async completion | One saved impulse is decoded, but live load_ir_sample is a lowering stub; main has async_complete and waits | Real resource resolution, convolution plan replacement and async identity completion are required. Host draining must not fabricate success or discard external jobs. Lowering and core async identity are owned by other workers. |
| 13 | Snapshot recall | All 201 files decode; each depends on script state to reconstruct its sound | Native snapshot-type/voice lifecycle and all persistence writes must be checked; decoding 223 entries is not playable preset parity. Mode/zone/FX gaps apply to every recalled patch. |
| 14 | Key range, bend/velocity/pressure, mono/glide | Authored scripts and callbacks are retained, including 3 PolyAt-runtime warnings | Range learning and voice priority need reached-event tests; PolyAt source values are not maintained in the existing runtime warning. The note audit cannot certify them until startup/callback servicing is correct. |
| 15 | Multi MIDI Player/Chord Naming/Humanize and program ownership | Physical empty slots and program ownership are retained by the inspector; exhaustive 50-file inspection was stopped | Coordinator must run the bounded multi inventory and actual per-program MIDI/service check. Do not infer all multis are Conflux synth programs from program 0 alone. |

UI reports for `gpt/conflux-ui`: authored NativeUI-to-Vector projection is missing;
main init projects 378 widgets and reports NKS/custom-ID/NativeUI fields. Source
widget layout and pixel parity are deliberately outside this package.

No native Kontakt host capture, complete multi survey, CPU acceptance or full
sound equivalence has been established by this package.
