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
| 1 | Audition callback servicing | **Fixed in the audit host:** a full 256-entry outbox formerly suspended the callback before later musical instructions | Both measured audio and repeated selection passes now use the existing owner-checked headless service. The 1,000-refresh authored note and repeated selections each verified RED→GREEN; zero retained notes/voices and zero pending callbacks. New numeric outcomes expose remaining real-library failures. Coordinator installed-note verification is still pending. |
| 2 | Wavetable table selection; six zone writes, main slot 2 line 2613 | `set_zone_par` is a frontend stub for group/key reassignment; selecting another table does not create the intended source topology | Needs zone membership/key mutation in core and actual KSP lowering. `sampler-ksp/src/lower.rs` is explicitly owned by another worker; report the exact three ZONE_PAR families, do not patch a UI selector. |
| 3 | Sample layers, group gating/purge, mono note routing | 90 saved groups use Time Machine 2; `library.rs:1288` explicitly falls back to ordinary sampler playback with a diagnostic. Script callbacks compile, but the earlier stalled audition never reached selection | Native TM2 time/pitch/formant/clock processing remains unimplemented; its source translator is in shared `library.rs`. Outbox fix isolates callback stalls. The known set_engine_par/purge_group operand defect is owned by another thread. A started note handle cannot certify source DSP or group selection. |
| 4 | Phase-form amounts and selector | **Partly fixed:** identity/ASYM2MP-only admission expands to 14 original-byte-verified phase modes, including Mirror; selector binding expands the source's three physical lanes to five | 490 native scalar vectors pass within 2e-7. Held mode edits preserve phase and reject unsupported/fractional modes without allocation. More phase/amplitude forms, table selection, native form ordering, smoothing and oscillator quality remain separate. |
| 5 | Wavetable constants | **Fixed for v1's explicit 51-value WT subset:** form, quality, modulation-type and tune-unit tokens formerly became opaque identities | Initializer enum regression verified RED→GREEN. Other enum families, including modulation waves, are not admitted by this port. Native symbol registration was inspected but is not itself a numeric evaluator oracle; IDs 1201+ are not phase ordinals. |
| 6 | Audio modulation/auxiliary oscillator and inharmonic controls | WT_MOD_TYPE/WAVE/TUNE/TUNE_UNIT/AMOUNT and INHARMONIC/MODE are referenced, but source IR/kernel models only the admitted phase/table subset | Needs the complete native oscillator/mix/FM/PM/RM/inharmonic kernel and its clocks/setters. The phase leaf cannot establish those laws; the old extraction cache is absent and live REA deep analysis is unavailable. Retain diagnostics and avoid guessed sine/FM substitutes. |
| 7 | LFOs and envelopes | Saved Digital Multi sine/fade admission has advanced beyond older audits. Live frequency, wave components, delay, retrigger and primary held-envelope state have distinct consumers | Existing source/route diagnostics remain authoritative. LFO waveform changes require native multi-wave laws, not relabeling sine. Primary held-envelope retiming is explicitly still open in W9's recovered consumer. |
| 8 | Source-to-source motion; 198 nonzero Constant-to-intensity targets | 2,190 module-parameter routes are diagnosed; outgoing depth is not equivalent to scaling a source waveform | Native outgoing-target getter/expression probes exist, but complete combination/live setter/clock rules are not yet admitted. Translation is in `library.rs`; lane kernels are in another worker's `dsp/lanes.rs`. Preserve physical slot/target identity. |
| 9 | Macro/MIDI motion and Animator | Scripts expose six macros, timer callbacks and step values; controller and listener callbacks compile | Real callback completion and resulting engine outcomes must be checked after servicing. Timer drivers legitimately remain pending. KSP ledger status alone is not proof of Animator audio; advanced source/FX destinations remain unavailable. |
| 10 | Random modulation | 550 RandomBipolar source losses remain UnknownLaw | Native signed generator/reset/lifecycle law is not established; v1 also lacks admission. Do not replace it with a guessed uniform signed generator or drop zero-depth diagnostics. |
| 11 | Four FX selectors and modern FX | Saved filter/convolution/module 0x13 topology is retained, but scripts request effect type/subtype changes and BITE/FREAK/CHORAL/FLAIR/PHASIS/RAUM/etc. parameters | Dynamic module replacement and each requested modern processor need actual kernels and parameter setters; `effects.rs` and shared lanes are in use. An engine name in the catalog is not a DSP binding. Static support cannot certify all selectable FX. |
| 12 | Space/IR changes and async completion | One saved impulse is decoded, but live load_ir_sample is a lowering stub; main has async_complete and waits | Real resource resolution, convolution plan replacement and async identity completion are required. Host draining must not fabricate success or discard external jobs. Lowering and core async identity are owned by other workers. |
| 13 | Snapshot recall | All 201 files decode; each depends on script state to reconstruct its sound. Init evaluation does retain `model.snapshot_mode`; older docs saying it is ignored are stale | Runtime `set_snapshot_type` remains approximate in the parity ledger/lowering. Snapshot-type/voice lifecycle and all persistence destinations need reached-callback checks; decoding 223 entries is not playable preset parity. Mode/zone/FX gaps apply to recalled patches. |
| 14 | Key range, bend/velocity/pressure, mono/glide | Authored scripts and callbacks are retained, including 3 PolyAt-runtime warnings | Range learning and voice priority need reached-event tests; PolyAt source values are not maintained in the existing runtime warning. The note audit cannot certify them until startup/callback servicing is correct. |
| 15 | Multi MIDI Player/Chord Naming/Humanize and program ownership | Physical empty slots and program ownership are retained by the inspector; exhaustive 50-file inspection was stopped | Coordinator must run the bounded multi inventory and actual per-program MIDI/service check. Do not infer all multis are Conflux synth programs from program 0 alone. |

UI reports for `gpt/conflux-ui`: authored NativeUI-to-Vector projection is missing;
main init projects 378 widgets and reports NKS/custom-ID/NativeUI fields. Source
widget layout and pixel parity are deliberately outside this package.

No native Kontakt host capture, complete multi survey, CPU acceptance or full
sound equivalence has been established by this package.
