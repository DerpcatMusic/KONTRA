# Backend historical recovery, 2026-10-12

Baseline: `972b4410571bb9c3144e3af5d1800b69258f0570`. Source work is isolated in
`/tmp/kontra-parity-async-20261012`, branch `subagent/parity-async-20261012`.
No build, compilation, Cargo, native plugin, installed-library scan or PCM capture
was run. The initial async implementation investigation made no source edits
before the consolidation redirect.

## Review evidence and its boundary

[backend-history-scopes.json](backend-history-scopes.json) records all **700 local
branch identities** from the 15:35:13 UTC inventory, each backend tip scope, exact
current path comparisons, historical snapshot matches and disposition. It also
retains the 18 dirty backend donor scopes, their working hashes and operation
markers. This projects the existing full inventory; it does not declare every
prior branch commit behaviorally reviewed. In particular, tip-path equality says
nothing about an earlier commit whose paths were changed again.

[backend-history-commits.tsv](backend-history-commits.tsv) identifies **574
non-WIP, non-checkpoint commits dated October 8 onward** reachable from the frozen
local branch tips that touched sampler-core, sampler-kontakt, sampler-ksp,
sampler-midi or ni-file. Their source changes were screened for added function
and test identities absent current source, then promising consumer scopes were
compared directly. Name absence is a search aid, never proof of missing behavior.
The initial all-head/all-remote screen covered 582 such commits; the committed
list uses the frozen inventory to avoid mixing later preservation commits into
historical feature evidence.

The disposition counts are 5 exact current snapshots, 24 exact reachable
historical snapshots, 8 exact backend tip scopes, 227 tips without backend edits,
353 older preserved snapshots and 83 differing scopes requiring successor review.
This is **not** permission to delete a checkout without its parent's independent
preservation receipt. Graph-unmerged counts were not used as a recovery test.

Recovered source commits: `6780d659` (typed capture) and `91670749` (FX readers).

## Recovered production contracts

| Donor identity | Missing behavior and bounded recovery |
| --- | --- |
| Dirty `fix-load`, `039da7cd`; capture fixture blob `4e1acfb8ed733b9e99b0207b7a3dacbe47816e5b` | The donor only authored failing tests. Current ingress retained a Text-sized transfer for every table cell and continuously recaptured unchanged data. The recovery prepares one reusable `WIDGET_EDIT_CAPACITY` chunk, walks widget/index ranges, publishes control/script revision changes from production render and stops capture requests after a complete unchanged sweep. Changes during a sweep cause another sweep, preserving earlier-chunk updates. Five authored regressions cover retained capacity, idle queueing, callback-driven updates, changes during a sweep and normal MIDI/render publication. The global script revision is conservative: unrelated script-cell changes may still trigger a new sweep. |
| Dirty `gpt-format-fxmod`, `b7f6af0b`; typed-reader blob `77b1abdc` (full blob in preservation inventory) | Current typed FX reader still interpreted integer selector words as float and Lo-Fi as FFFBF. The restored native-reader layouts are Compressor IFFFFB, Saturation FI, Distortion IFF, Transient Master FFFI and Lo-Fi FFBFF. Executable Saturation/Distortion/Lo-Fi decoding is corrected through existing voice/bus consumers; no DSP kernel is replaced. Versioned extended fields remain typed storage with unassigned physical laws. Truncation, trailing bytes, integer/byte preservation and an authored executable-consumer regression accompany the recovery. Current byte-bit preservation tests remain. |

The primitive source evidence is retained in
[KONTAKT_FX_MOD_READERS.tsv](../architecture-v2/KONTAKT_FX_MOD_READERS.tsv) and the
native-reader follow-up in
[KONTAKT_FX_MOD_FORMAT.md](../architecture-v2/KONTAKT_FX_MOD_FORMAT.md).
It is pre-existing read-only RE and structural evidence, not a new native-host
receipt. The newer `Saturation` catalog name, compressor DSP and original
byte-preservation regression remain; the older donor file is not copied wholesale.

## Explicit supersession decisions

| Source identity | Exact consumer comparison / disposition |
| --- | --- |
| `4955b8f85bd395067242d3b1e873decdd8a341a3`, Step envelope retiming | Both complete touched envelope files equal baseline byte-for-byte. `step_curves_survive_zero_duration_stage_edits` and render/block partition regression are current. No recovery. |
| `b64d9d80accf77e183235f83aa56717c2cb3146b`, string-array callback sort | Current `Gen::sort` already selects `Home::Texts`, copies scratch text and performs typed comparisons. Both authored string-sort regressions are current. Older unrelated lowerer deletions must not be applied. |
| `42ebc8da805629294aa7882ea7c39349777c15bb` / `01a5bde9`, NKA snapshot budgeting | Dynamic slot calculation and mixed-job ownership tests are current. `d5b2f2c0` and release `01d1d337` deliberately changed the strict 8 MiB rejection into an authored-array memory target with a one-job minimum. The missing old strict-cap test would regress the newer policy. |
| `f994ead30a38358783db168d03856674c0b19073`, stored resource IDs | Current `ResourceContainer::read` selects by normalized unique name, then bounded `read_item`; `f123e2d1`/`dff99bbb` replace ID-only lookup. Duplicate IDs are retained and only ID-addressed reads reject ambiguity. An old duplicate-ID rejection assertion for a name-selected resource is superseded. |
| `ee7af138c78e05f84fca45c59555041b5190685b`, cold onset lifecycle | Current choke/cancel/all-sound-off/callback-fault/terminal-decode tests retain the immediate retirement behavior. Old key-up immediate retirement is deliberately replaced by `released_cold_note_off_waits_then_sounds_in_release`, preserving late released onset. No reversal. |
| Dirty `decipher-readers-v2`, `2fb8c926` | Rack decode errors already keep physical slots, scope and valid siblings; current `fx_decode_failures_keep_scope_slots_and_valid_siblings` covers it. Header/program/group/voice-limit and NIS regression families are current; the five previously exact files remain accounted in the original audit. |
| Dirty `decipher-readers-v1`, `4bffbb18` | Binary-state framing moved to UVI crypto (other lane). The remaining ni-file files need predecessor-to-successor comparison, not replay of v1 paths. Preserve pending that comparison. |
| Dirty `fix-w12-vista-pacific-resources`, `68052b42` | `read_index/read_member` intent is implemented by current `read_table/read_item` with better named-item ownership. No old API port. Resource-membership example remains a probe to preserve. |
| Dirty `fix-w12-gate`, `23b8e94f` | `Always AND_NOT` test remains an unproved native hypothesis conflicting with documented Always no-operation/end-marker semantics; no production change. |
| Dirty `t3code-e50b9147`, `7448dd15` | Backend envelope delta only replaces `any(|y| stop(y))` with `any(stop)`. This is syntactic cleanup of a v1 test, not a missing v2 consumer contract. |
| Dirty `v2-simd-dsp`, `ddad588e`; `w15-vista-harp`, `afe58f31` | Historical AR/slot-filter/phaser approximations and unmeasured subtype laws must not replace newer native kernels. Preserve probe source and authored fixtures as provenance. |
| Dirty `w5-conflux-red`, `54d9a5c5` | Marked-event selectors, menu getters and physical source-zone regression families already exist in the current successor tests. |
| Dirty `gpt-decipher-dsp`, `aa3430d6`; `w12-dsp-recount`, `5172a9ef`; `gpt-format-legacy-baseline`, `fb456b41`; `w15-eq`, `ada2634d`; `w15-harp-shipping`, `f199980d` | Metadata, numeric or installed-library probes. Preserve their hashes; no measurement was rerun and no unsupported feature is admitted from them. The shipping-Harp ignored fixture is not a production fix. |
| Dirty `audit-load-v1`, `0cb7a8a0` | Instrumentation-only bank timing; preserve isolated, no v2 consumer recovery. |
| Dirty `integration7-source-draft`, `4fcb6cb7` | Waveform cherry-pick retains an unmerged lowerer and mixed staged predecessor paths. Current waveform has successors. Preserve exact operation/index, then compare residual hunks; neither finish nor abort it here. |
| Dirty `ksp-rpn-deferred-fixture-successor-review`, `2a243bcf` | Staged mass deletions are a sparse/review anomaly, not evidence of requested source deletion. Preserve operation/index; do not replay. |

## Material unresolved scopes

The parent/scheduler lane owns recovery of live wavetable `35ded542`, sample-clock
Digital Multi fades `448cfaa5`, and host-block callback allowance
`c250646e`/`4e949aca`. Those code paths are genuinely absent baseline and require
its separate commit/test gate; they are not marked recovered by this report.

Whole-voice control work (`0158aefa`, `d8f1c623`, descriptor predecessor
`fee28276`) has useful original AHDSR, ordinary-envelope, live-source and native
primary ownership. Current support evaluators already exist, but the production
`whole_voice` admission/consumer does not. Historical production envelope edits
would overwrite the newer Step-retiming fix, and recorded native-primary CPU
acceptance remained HOLD. Recover the admitted descriptors/calls with authored
end-to-end voice lifecycle regressions; retain generic-v2 fallback and current
Step shape. No whole-voice performance or native parity claim is justified yet.

Voice-ring/preload series (`009a1366`, `5e57099a`, `718fcc4a`/`00f84421`) adds a
second streaming ownership path, generation-scoped failure/reuse, urgent fairness
and source-time resident onsets. It is much broader than a file-copy recovery;
identify the accepted successor rather than replay the HOLD draft. Existing
v2 stream-cache ownership and newer demand geometry must be reconciled.

The protected v1 stash `b89b78247f83acc8d6932aa930cd4412e9d9e958` and its untracked
commit `fe71daf0` retain major bank/output metadata, rack timeline, MPE ownership,
counted loops, envelope/LFO, stream and HQ sampling source. No stash operation was
performed. The typed `Bank::slot_settings`/`output_configuration` additions are
absent v2 but the donor also reverts the current tag-based `slot_list` fix to an
unsafe positional child lookup. Port metadata independently with framing/bounds
regressions and production consumers; do not replace `bank.rs` wholesale.
These scopes remain preserved and unresolved, rather than declared superseded
because names differ.

## Validation for the parent

Executed: `git diff --check`; `rustfmt --edition 2024 --check` on the new capture
regressions and changed small reader/source-parameter files; parser-only
`rustfmt --emit stdout` for the existing large v2/effects modules (discarded
format output, preserving their unrelated formatting).

Rust compilation and execution: **NOT_RUN / NOT_COMPILED**. Suggested authorized
central commands:

```sh
cargo test --locked --profile ci -p kontakto --lib sound::v2::capture_tests
cargo test --locked --profile ci -p kontakto --lib sound::v2::tests::typed_capture_reads_callback_changes_and_rejected_edits_roll_back
cargo test --locked --profile ci -p sampler-kontakt --lib native_typed_fx_modes_and_lofi_fields_reach_executable_consumers
cargo test --locked --profile ci -p sampler-kontakt --lib source_parameters
cargo test --locked --profile ci -p ni-file --lib effect_parameters
```

Task-specific risk: callback publication is observed at render completion;
readback remains a sequence of bounded chunks, not an atomic all-widget snapshot.
A change during the sweep triggers a follow-up sweep. Physical DSP field laws of
newly decoded unknown modules remain unsupported. None of the static checks
establishes native-host audio or scheduling equality.
