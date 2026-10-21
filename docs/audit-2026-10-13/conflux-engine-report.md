# Conflux engine implementation handoff — 2026-10-13

This package fixes the headless audit's callback stall and a measured subset of
wavetable control/DSP behavior. **Complete Conflux sound/playability parity is
not established.** The installed post-fix audition belongs to the coordinator
under the latest no-build/no-corpus rule. No diagnostic was hidden to obtain a
passing result.

## Branch + worktree path + commit SHAs (oldest first)

- Branch: `gpt/conflux-engine`; base `origin/main` = `ce0a2e61`.
- Worktree: `/home/derpcat/.t3/worktrees/KONTAKTO/gpt-conflux-engine`.
- `d3e2c9ed` — initial library intent map.
- `3206f4db` — early WIP checkpoint: native phase vectors, source/mode lanes,
  failing-first probes. Its pending GREEN checks have since passed.
- `2e110c36` — ranked gap table and focused corpus test entry.
- `02b801ad` — valued WT constants and initializer form admission.
- `95672afd` — shared headless outbox service and corpus audio-pass drain.
- `8094e1d2` — repeated selection-pass drain, audit outcomes, discrete mode validation.
- `28267bda` — native Mirror phase branch and 490-case fixture.
- `2bfb7133` — replacement-schema and player-offset admission validation.
- The subsequent documentation commit contains this report and the source-free
  validation receipt; its SHA is in the final handoff message.

No push, merge, main-checkout modification or plugin/workspace build was done
by this package. Before the latest restriction, installed-source inspection and
one existing-runner audition were performed. After the restriction, Cargo calls
were exclusively focused `-j 4` default-profile crate tests and a crate check.
All own jobs are stopped; own scratch and target are removed at handoff.

## Per item: before → after, root cause, evidence

| Item | Before → after and root cause | Evidence |
|---|---|---|
| Headless note audition | A 256-effect outbox filled, suspending a note callback before `play_note`. The corpus path never drained it. Both main audio and repeated selection passes now service the existing owner/plan-checked UI adapter outside audio timing. External MIDI/array jobs remain queued until a real host accepts them. | [Shared service](../../crates/sampler-ksp/src/headless.rs#L10), [audio drain](../../tools/corpus-health/src/main.rs#L1216), [selection drain](../../tools/corpus-health/src/main.rs#L1354). `kontakt_audition_services_ui_backpressure_before_judging_silence` verified RED for sound and then separately RED for repeated selection. GREEN: synthetic PCM peak > .01, 1,000 drained effects, zero retained notes/voices, zero pending callbacks, no faults, nonempty repeated selections. `headless_external_jobs_stay_in_the_outbox_until_a_real_host_accepts_them` preserves the previous adapter contract; no new native async law. |
| Wavetable phase forms | Saved sources formerly admitted only identity and ASYM2MP. Admission now includes 14 measured phase ordinals: 0, 7..12, 14..17, 31..33. Rust implements PWM, asymmetric families, Flip, Mirror, Seesaw and Bend2 from original instructions. | [Phase law](../../crates/sampler-core/src/wavetable.rs#L16), [native numeric fixture](../../crates/sampler-core/src/wavetable_native_vectors.json), [probe](../../tools/dsp/verify_wavetable_forms.py). `phase_forms_match_original_kontakt_instructions` verified RED→GREEN, including a separate Mirror RED. All 490 cases pass absolute error ≤ 2e-7, with zero helper substitutions. Original instructions under Unicorn are the reference, not a Kontakt audio recording. |
| Held shape selectors | The source had three amount/position lanes, with no form-selector consumer. It now has five physical lanes, including two raw-ordinal selectors; edits update held cursors without restarting phase. Physical group 7 does not alias dense group 0. | [Lane construction](../../crates/sampler-core/src/lower.rs#L121), [cursor projection](../../crates/sampler-core/src/source.rs#L528). `addressed_phase_form_switch_changes_held_sound_without_restarting_the_clock` verified RED (`InvalidInput`)→GREEN. After 37 frames, a held 16→0 switch matches the next 41 frames exactly against the existing v1 readout oracle, with zero heap allocation. Existing `live_wave_writes_reach_active_voices_in_single_batch_and_parallel_paths_without_resetting_phase` passed for 64 voices, one/two threads and chain/no-chain paths; this is a functional/allocation assertion, not a CPU measurement. |
| Saved form decoding | Serialized one-based forms beyond 1/17 were refused. They now decode to the admitted native ordinal while unproved forms, random phase, audio modulation, inharmonic mode and untracked playback remain diagnosed/refused. | [Forward decoder](../../crates/sampler-kontakt/src/wavetable.rs#L4). `native_phase_ordinals_decode_from_one_based_saved_forms` verified RED using the former 0/16 predicate→GREEN for all 14 admitted ordinals. |
| WT enum values | Explicit WT constants became opaque interned IDs (ASYM2MP: 33554433), making script numeric writes wrong. The 51-value WT form/quality/modulation-type/tune-unit table now ports our v1 numeric values (ASYM2MP: 16; PM1: 6). Initializer form writes use the same admission predicate as the source. | [Valued constants](../../crates/sampler-ksp/src/builtins.rs#L527), [one-line initializer gate](../../crates/sampler-kontakt/src/library.rs#L1269). `wavetable_enums_keep_v1_numeric_identity_in_initializer_writes` verified RED→GREEN. This does not admit other enum families or prove the native KSP evaluator's numeric table independently. |
| Mode validation | Broad real domains could admit fractional/unproved mode controls and invalid replacement defaults. Those are now rejected atomically. Nonzero continuous player offsets are rejected on this raw-ordinal selector because no normalized offset law is admitted. | [Ordinal validation](../../crates/sampler-core/src/engine_parameters.rs#L160), [control validation](../../crates/sampler-core/src/control.rs#L555). `phase_form_controls_reject_unproved_or_fractional_modes_atomically` verified RED→GREEN, including separate REDs for offset and default admission, unchanged readback and no heap allocation. Other continuous parameter offsets retain their existing law. |
| Audit diagnosis | Finished faults alone could report an apparently clean silent note while continuations remained suspended. The runner now reports drained UI count, pending callback program/PC/waiting/note ownership, successful engine outcomes, counted failures by physical address, and dropped outcome count. | [Audit output](../../tools/corpus-health/src/main.rs#L1311). These fields contain no library script/state payload. The bounded 64-entry engine outcome queue can drop outcomes within a block; the reported drop count prevents treating observed failures as an exhaustive census. Listener/timer drivers may legitimately remain pending and are not classified as faults. |

The [source-free receipt](conflux-engine-receipt.json) records every focused
command and result. `cargo check -j 4 -p sampler-native` passed at `8094e1d2`;
later source-admission changes passed their core/Kontakt tests. No final
workspace `--no-run` was attempted because the new user rule overrides the old
handoff requirement.

DSP authority: [NI KSP Source Module parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters#instrument--source-and-amplifier-module),
[Conflux Edit/Source/Shaper contract](https://docs.native-instruments.com/ni-tech-manuals/conflux-manual/en/edit-page.html),
our v1 `0cb7a8a0` valued constants and existing readout, and read-only Kontakt
8.13.1 EXE SHA256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
Radare2 inspected phase entry `0x140567aa0`, table `0x140567fe0`, Mirror entry
`0x140567ed5`, and each used constant. REA opened the image but its deep-analysis
provider was unavailable; the old Ghidra cache is absent. Native registration
`0x140463abb..0x140463e08` / `0x140775ef0` identifies WT symbols, not public enum
values. [The RE note](../re/conflux-phase-forms-20261013.md) records addresses,
provenance and limits. Native standalone instruction behavior does not establish
standalone/plugin parity or complete oscillator sound.

## Files touched outside your scope (and why)

- **Shared/in-use:** `crates/sampler-kontakt/src/library.rs` has exactly one
  predicate replacement at line 1269. It admits script initializer writes only
  when the source/kernel support that phase ordinal. Coordinate this one-line
  hunk with the library owner.
- `crates/sampler-native/src/render_kontakt.rs`: extracted the existing headless
  host adapter into sampler-ksp and delegates to it, preserving its behavior.
- `tools/corpus-health/src/main.rs` and `tests/conflux_outbox.rs`: fixed the audit
  host required to judge silence; the test imports production code and runs only
  the authored regression, with no corpus walk/CLI/library access.
- Core source/control/engine-address files, `sampler-ir`, KSP builtins, focused
  tests, DSP vectors/probe and audit/RE docs are engine-package work.
- No UI layout/widget file, KSP `lower.rs`, shared DSP lane/effect/kernel file,
  core `behavior.rs` or MIDI-object file was edited. UI scene/projection bugs are
  recorded for `gpt/conflux-ui` in the intent and gap documents.

## Still open: item + concrete reason

The [ranked table](conflux-gaps.md) reviews all 15 feature families. Remaining
boundaries are concrete:

| Rank | Remaining item | Reason / next owner boundary |
|---:|---|---|
| 1, 3, 9, 14 | Real note, controllers, Animator, range/mono/glide behavior | Post-fix installed audition is prohibited for this worker. Synthetic servicing is proven; actual Conflux callback completion/group sound is not. Three PolyAt-runtime warnings remain; the backing per-key pressure values are not maintained. |
| 2 | Wavetable bank/source selection | Six `set_zone_par` writes change GROUP/LOW_KEY/HIGH_KEY; lowering/evaluation still stub them. Needs live zone membership/key selection and loader access to future bank zones. Protected KSP lowering is being edited by another worker. |
| 3 | Sample-source sound | 90 Time Machine 2 groups are explicitly played as ordinary samplers (`library.rs:1288`). Native granular time/pitch/formant processing and setters are missing, and the source translator is shared. Group gating/purge also depends on the other worker's operand fix. |
| 4 | Other forms, quality, phase/smoothing/order | Sync/first-bend/log/exp/quantize call native math helpers/imports not covered by this helper-free probe. Amplitude forms 18..23 and wrap 27 return identity in the phase leaf, proving another consumer must be traced. Native complete form ordering, anti-aliasing and smoothing are not established. |
| 5, 6 | Auxiliary oscillator, inharmonic and remaining WT enums | FM/PM/RM/mix/oscillator clocks and full native kernels are not recovered/admitted. Modulation-wave enums are outside the v1 51-value port. No guessed sine/FM or numeric enum substitute was introduced. |
| 7 | Full live LFO and held-envelope behavior | Saved Digital Multi sine/fade already works beyond older audit claims. Multi-wave laws, live wave/rate/reset/clock consumers and held primary-envelope retiming are distinct incomplete destinations; shared lanes are in use. |
| 8, 10 | Source-to-source and random modulation | 198 nonzero Constant→source-intensity targets, 2,190 diagnosed module-parameter routes and 550 RandomBipolar UnknownLaw losses remain. Getter/expression probes cannot establish combined depth/setter/reset laws. Translation/lane owners are in protected files. |
| 11 | FX choices and replacement | Dynamic type/subtype swaps and requested modern FX kernels/setters are not all admitted. `effects.rs` and shared DSP lanes are owned elsewhere. A parameter token/rack record is not an audible processor. |
| 12 | Live IR and async | `load_ir_sample` is still a lowering stub. Needs real resource resolution, convolution plan replacement and identity-correct async completion; those owners are working in protected lowering/core async code. External jobs remain visible/retained. |
| 13 | Snapshot playability | All 201 files decode, each with 223 script entries and no native group/FX state. Init retains `snapshot_mode`; runtime `set_snapshot_type` is still approximate. Zone/source/FX destinations and voice/persistence lifecycle need real recall checks. Decode success cannot establish playable presets. |
| 15 | All multis | 50 files exist; the aggregate inspection unit was stopped and did not finish. Latest user rule prohibits restarting corpus/survey work. Per-program MIDI helper/service ownership needs coordinator follow-up. |

No complete native host audio comparison, CPU acceptance, exhaustive multi
inspection or full snapshot recall audition has been established. Zero-voice
pre-fix timings cannot support a playing CPU claim. The frozen v1 binary/cache
is absent; no v1 runtime benchmark was performed.

## Anything the coordinator must act on (conflicts, follow-ups)

1. Merge the branch commits in order. Reconcile the one-line shared `library.rs`
   hunk and the other worker's KSP operand/purge/async changes. Source metadata
   admission and physical mode bindings must remain synchronized.
2. Use the coordinator's **merged, current** corpus-health executable for this
   single installed-file run. Set `CONFLUX_CORPUS_HEALTH` to that executable;
   this command does not build anything:

```sh
/home/derpcat/.cache/kontakto-heavy "$CONFLUX_CORPUS_HEALTH" run \
  /home/derpcat/.cache/kontakto-coordinator-conflux-after.jsonl \
  --tier full --workers 1 --timeout 180 \
  '/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki'
```

The pre-fix run loaded 94 zones, key 60/velocity 64, 48 kHz, 64-frame blocks:
`note=started`, zero voices/selections, one stuck note, no finished faults;
the controller pass returned `Err(Core(Capacity))`. Missing host draining is
reproduced and fixed. **It is not yet proved to be the only cause of actual
Conflux silence.** Compare `sounds`, `peak_voices`, selections, retained notes,
engine failure addresses/drop count, and note-owned pending callbacks after
merging. Timer/listener drivers alone are legitimate pending work.

3. If the merged note remains silent, use the new physical engine failure and
   pending-callback fields to separate lowering/group-gating/zone-selection from
   missing DSP, rather than accepting `Started` or an empty fault list as success.
4. Coordinator CPU measurement should report actual nonzero voices, 48 kHz /
   64-frame blocks, p50/p99/max, render time divided by block duration, allocation
   counts and top frames under a quiet machine. No such measurement was run here.
5. Route the NativeUI→Vector scene/layout projection findings to `gpt/conflux-ui`.
   Complete the bounded multi inventory and per-program services when authorized.

## Topology facts learned

- Five physical NKI slots survive inspection: two empty, then main synth,
  mono/portamento and helper scripts. Main has 279 UI callbacks; mono has 13.
  All three initializer frontends finish with zero faults. Main has note,
  release, controller, pressure, persistence, async, listener and PGS callbacks.
- Raw topology is one Sampler, 90 Time Machine 2 and one Wavetable group. Fresh
  pre-fix IR retains 92 groups, 1,985 zones, 1,962 assets, 17 buses, 108 chains,
  3,566 modulators, 2,724 routes and one impulse. This is not a simultaneous-voice
  count. Selection scripts choose/reassign/gate the intended sources.
- Wavetable table selection changes group/key topology, distinct from scalar
  position and two form amounts. Physical engine group/slot identities must
  survive dense packing and snapshot recall.
- 91 saved group filters comprise 82 active and nine bypassed objects.
  Instrument insert slot 3 is bypassed convolution; slot 7 is active module
  0x13. Authored four UI FX positions do not flatten group/insert/send/main racks.
- Every factory snapshot is script-only in the decoded group/FX sense: 223
  saved entries reconstruct its state. Actual sound therefore depends on
  reached persistence callbacks and their engine destinations.
- The installed ReadMe PDF was read fully; it contains initial-release history
  and the online manual pointer. The intent map, not that changelog, records
  the synthesis/performance control contract.
