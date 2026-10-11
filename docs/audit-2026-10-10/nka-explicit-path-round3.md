# Explicit-path NKA source candidate — round3

## Verdict and ownership

Source/test commit: **63178224f27e566aaaf05e91adb15b0ae23de91a**.
Source tree: **4421f478d1571a18a2d479306336864fe9529e8c**.
Machine-readable source/test/document gates are in
`nka-explicit-path-round3-test-manifest.json`; every Rust execution field is
NOT_RUN and every native-runtime field is UNKNOWN.
Base: **429e7dff3ecb0f53aef32c43201ab796ccc8c28c** (tree
94ad5caaa5e5ec70e1595d34166c25110d294c67, version0.3.403).
Exclusive checkout `/home/derpcat/.t3/worktrees/KONTAKTO/nka-explicit-path-round3`,
branch `pi/nka-explicit-path-round3`. No edits to the paired native checkout,
frozen v1, reference tree, transport lane or main checkout's dirty source.

This is a **source candidate**, not compiler GREEN, a tested implementation,
or Kontakt runtime parity. All Rust source/tests are **UNRUN** here.
Rustfmt parsed the changed Rust; `git diff --check` passed. Python source
assertions checked the production call chain and native-document hashes only.
They did not execute Rust or establish runtime behavior. Integration3 is the
sole validation owner; this lane did not run Cargo, rustc, clippy or a host.
Complete polished product plus lower CPU AND RAM than both frozen v1 and
Kontakt remains **UNACHIEVED**. No performance threshold or percentage is claimed.

## Native specification evidence (REA-equivalent, not live observation)

The coordinator supplied immutable HTTP200 authoritative NI documentation.
Both raw HTML files were rehashed in this lane and matched the manifest:

- [load_array_str()](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/load-save-commands#load_array_str--),
  [save_array_str()](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/load-save-commands#save_array_str--),
  same page General Information and save_array() format discussion.
  `handoff/official-docs/ksp-load.html`, SHA256
  `9bcd5f671eac2301211c5d7c542d8fc506657c6517535709674fe31d14abffbf`.
- [on async_complete](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks#on-async_complete).
  `handoff/official-docs/ksp-callbacks.html`, SHA256
  `f91a2a1411c18e84e15f4e89e74499c3ae74dec75f230bb8c99df65e316267ba`.

Archive root: coordinator scratch
`/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/`.
NI pages are unversioned and show the8.12 additions; they are not an installed
Kontakt8.13.1 execution receipt. No live native behavior was measured.
This exact immutable native specification is the permitted REA-equivalent check
for this lane. It establishes the documented requirements below, not undocumented
header coercion, callback timing or an internal implementation. No decompiled
pseudocode, library/sample/key data or native execution was used.

NI explicitly states:

1. Explicit absolute path, forward slash separators.
2. Exact array name in the first line; then one value per line. Integer, real and
   string arrays can be saved/loaded.
3. Synchronous use in `on init`; asynchronous use in `on persistence_changed`,
   `on ui_control` and `on pgs_changed`.
4. Explicit `load_array_str` does **not** implicitly persist its target.
5. Saving creates a new file when its folder already exists.
6. Async commands return unique IDs. On action completion, `on async_complete`
   receives that ID in `$NI_ASYNC_ID`; `$NI_ASYNC_EXIT_STATUS` is1 on success,
   otherwise0.

## Implementing paths pinned to63178224

Line spans in this section are all from
**63178224f27e566aaaf05e91adb15b0ae23de91a**, not mutable integration HEAD.

| Requirement / responsibility | Implementing source |
| --- | --- |
| Typed name/type/range, not a UI ID or arg0=0 | `crates/sampler-ksp/src/array_file.rs:5-27` (`array`); `crates/sampler-core/src/array_file.rs:20-46` (`ArrayFileArray::validate`) |
| Synchronous init application before subsequent statements; no request replay / implicit persistence | `crates/sampler-ksp/src/eval.rs:1703-1763` (`LoadArrayStr/SaveArrayStr` branch) |
| Persistence callbacks containing file calls, including assignment/condition/function calls, schedule the whole callback rather than replaying a prefix | `crates/sampler-ksp/src/eval.rs:42-115` (`may_suspend`, `statement_has_array_file`) |
| Dedicated typed instruction in approved callbacks; unsupported callbacks reject with-1 | `crates/sampler-ksp/src/lower.rs:2342-2380` (`Op::ArrayFile` lowering) |
| Only referenced runtime arrays prepare file resources; oversized initial text rejected | `crates/sampler-ksp/src/lib.rs:1453-1475` (`array_files`, `ScriptResources`) |
| Prepared typed bank bounds and unreachable instruction validation | `crates/sampler-core/src/ops.rs:704-747` (`ScriptResources::apply`); `script.rs:83-115` (`validate`) |
| Generation-owned bounded snapshot pools, absent when unused | `crates/sampler-core/src/array_file.rs:50-154` (`ArrayFilePayload`, `ArrayFileState`) and `ops.rs:625-699` (`ScriptBank`, `ScriptInitial::bank`) |
| Admission returns a positive unique job ID; request-time save snapshot | `crates/sampler-core/src/array_file.rs:381-455` (`request_array_file`); `ops.rs:1018-1041` (`Op::ArrayFile`) |
| Capture copies into control-owned allocated storage; completion installs without transferring/freeing payload ownership on audio | `crates/sampler-core/src/array_file.rs:456-541` (`capture_array_file`, `complete_array_file`); `control/transfer.rs:8-9,180-185` |
| MIDI and NKA share ID counter, preflight, callback context, status and waiter wake semantics | `crates/sampler-core/src/midi_object.rs:627-631,1110-1206`; `ops.rs:1072-1105` (`WaitMidi`) |
| Cancelled jobs cannot apply late results; pool clearing does not free backing storage | `crates/sampler-core/src/lib.rs:1880-1887`; `array_file.rs:145-153,484-541` |
| Actual production consumer, epoch check retained, exact plan check added | `src/plugin.rs:1725-1754` (`Shared::apply_effects`); `src/sound/v2.rs:529-573` (`ControlIngress::service_midi`) |
| Control replies route captures to the lazy NKA worker and worker completions back to the same ControlClient | `src/sound/v2.rs:802-941` (`ControlIngress::settle`); `src/sound/v2/array_file.rs:8-62` (`Worker`) |
| Worker/preparation-only strict UTF8 codec, finite typed values, bounded bytes/cells, absolute paths | `crates/sampler-core/src/array_file.rs:209-334` (`synchronous`, `perform`, `decode`, `encode`, `validate_path`) |
| Atomic sibling replacement; no existing-file truncation or implicit directory creation | `crates/sampler-core/src/array_file.rs:336-378` (`atomic_write`) |
| Truncated runtime paths/strings are rejected, including text-to-text copies | `crates/sampler-core/src/ops.rs:492-540,1258-1262`; `array_file.rs:80-96,245-247` |
| External init read/write is not silently suppressed by old product caches | `crates/sampler-ksp/src/init_cache.rs:337-342`; sync init allocates an ID, so existing `capture_initialized` refuses nondefault MIDI state |

The own frozen v1 code at `0cb7a8a0:src/ksp/arrays.rs:114-164` was inspected
read-only. Its sibling temporary, permission preservation, flush/sync and rename
architecture was ported/adapted. V1's lossy UTF8, case fallback and permissive
bare-name header were **not** ported or treated as Kontakt evidence.

### Ownership / work boundaries

Init reads, writes and allocation execute on the preparation owner. Runtime
callbacks only copy bounded values into prepared generation-owned snapshots and
emit a small Copy effect. Shared's off-audio pump requests an audio-side capture
through the existing bounded ControlClient. Audio fills the caller-owned buffer
and returns it. The lazy `kontra-nka` thread owns filesystem I/O, parsing and
encoding. The pump sends its result back through the same ControlClient. Audio
validates the original plan/instance/job/descriptor, copies values into that
bank, removes the job, then delivers the existing async callback/waiter flow.
The reply retains the allocated payload for control-side disposal.

Existing plugin epoch fencing remains unchanged. A ControlIngress is never
retargeted; it also rejects an effect whose exact PlanId is not its plan.
Core supports completion of a still-retained old generation but never applies
it to a new generation. Panic clears pending jobs; late completions then fail.
The extracted MIDI preflight/finish helper keeps its previous context and order;
this lane does not invent a new async collector, transport ABI or generic host
service layer.

## Prepared regression evidence — UNRUN

`crates/sampler-ksp/tests/array_file.rs` at63178224 contains16 normal test cases
and one cache-gated test (17 total). The cache case is explicitly **NOT_RUN**.
Without the cache feature it is excluded, not passed. It is not an ignored native
probe: it creates its own authorized tiny temporary fixture. If the validation
host lacks an authorized temporary filesystem, record blocked/NOT_RUN rather
than substituting any proprietary fixture or official reader. Key lines:

-82: sync init int/real/string read and subsequent-statement visibility.
-116: all three sync save/load types and no implicit persistence.
-157: non-UI identity, actual worker-thread load, async ID/status/one callback,
  wait resume and duplicate-completion rejection; audio heap guard.
-204: save snapshot reflects request time, not delayed host capture time.
-224: real/string runtime typed destinations.
-253: file-call assignment defers the entire persistence prefix once.
-290: failed read keeps values and still gives one status0 completion.
-311: wrong instance and cancelled job rejection.
-342:8 admitted NKA jobs, explicit overflow rejection, distinct MIDI/NKA IDs and
  independent callbacks.
-371: plan adoption; wrong new plan rejects; retained old completion edits only
  the old bank, not the replacement.
-415: concatenation/text-copy truncation cannot select a different shorter path.
-432: invalid unreachable typed operand, overflowing address and snapshot budget
  rejected during preparation.
-491: unsupported note context reports rejection.
-513: exact header/type/cardinality, UTF8, i32 range, finite real and byte limits.
-568: invalid/read-only save keeps old bytes; missing parent is not created;
  temporary cleanup.
-618: explicit path and string length limits.
-655: external file init cache is declined, including old cache restore.

`src/sound/v2.rs:5259-5393` contains
`nka_service_uses_actual_worker_control_capture_and_exact_async_completion`.
It is authored to exercise the real ControlIngress, actual `kontra-nka` thread, control-channel
capture and completion, both load and request-time save, exact callback ID/status
and no allocation/free on the audio-side admissions. Test deadlines are bounded.
No test was run here. Both allocator checks reuse existing approved test support;
no new unsafe allocator/library code or proprietary fixtures were introduced.

### Concrete integration3 validation request

Cherry-pick63178224, then this documentation receipt commit. Validate in the
single shared serial combined cycle. Do not set CARGO_TARGET_DIR or RUSTC_WRAPPER.

1. `sampler-ksp --test array_file`: full file, then cache feature coverage.
2. `sampler-ksp --test midi_object`: full existing suite, especially
   `async_buffer_reset_completion_is_slot_owned_retained_across_wait_and_cancelled_on_panic`,
   `wait_async_waits_for_midi_jobs_and_invalid_ids_continue_in_init_and_runtime`,
   `reset_requested_in_init_completes_asynchronously_after_plan_activation`,
   `unpublished_init_completion_is_rejected_when_adoption_has_a_full_effect_queue`.
3. KSP existing `persistence_waits`, `script_state`, `initialized`, `arrays`,
   `strings`, `typed_widgets`; cache initializer tests where enabled.
4. Core existing `ops`, `plans`, `controls`, `persistence_revision`.
   `midi_instruction_stays_within_the_shared_code_size_budget` remains required.
5. Root lib filter
   `nka_service_uses_actual_worker_control_capture_and_exact_async_completion`,
   and existing
   `midi_file_service_uses_the_host_control_queue_and_completes_its_source_slot`.

Package names are sampler-ksp / sampler-core; root package is kontakto. Integration
owns build/test configuration, shared Cargo+sccache, all actual outputs and the
combined SHA. These requests are not receipts that the commands ran or passed.
Do not invoke tools/kontra-gate/gate.py, probes/all14/UVI or any official reader.

## Explicit policies and remaining limits

-8 pending jobs per script instance. Only referenced runtime arrays create pools.
  Numeric/text maximums determine preallocated snapshots; total snapshot backing
  storage is capped at8MiB per instance. Capture duplicates a bounded payload on
  control. Admission, capture and application are O(array length), not O(1).
-16384 cells maximum;2MiB file payload;320 UTF8 bytes per name/path/string cell.
  Text arrays may hit the8MiB snapshot budget before the cell maximum. Limits
  reject; they do not silently truncate a path or file. Text now retains one
  truncation bit (which can add alignment bytes per runtime text cell).
- Queue/ID exhaustion returns-1 without pretending a job was admitted. File I/O
  failures after admission deliver status0. ID exhaustion never wraps. The sync
  init return is a distinct positive ID with no async callback; undocumented NI
  exact sync-return/failure values remain UNKNOWN.
- Strict fully typed sigilled header, valid UTF8, exact declared value count,
  finite f64 and signed32 numeric cells. These malformed-file policies are our
  deliberate safe boundary, **not proven Kontakt edge behavior**. No bare-name,
  lossy encoding, resize/truncate or case-fold filename fallback is claimed.
- `load_array`/`save_array` modes, pickers, resource-container paths and
  `fs_navigate` are still outside this slice. Falcon/UVI semantics are not part
  of this NI-only builtin implementation and are not claimed.
- Atomic replacement ports the own v1 pattern. Windows replacement behavior,
  sharing violations, durable parent-directory sync, disk-full failure injection,
  concurrent writers and native malformed-file coercion remain unmeasured.
- A worker disk action already captured can finish after cancellation/replacement;
  result ownership is fenced, but this is not a cancellable filesystem API.
  Worker shutdown joins off audio and may wait for an OS I/O call. No guarantee
  of native callback wall-clock order or file-operation cancellation is claimed.
- Native table-specific load normalization and broad UI presentation parity are
  not inferred from direct typed-bank application. Runtime publication still
  uses the existing widget/state capture path.
- No Kontakt/v1 comparable CPU/RAM measurement. Source presence plus authored
  tests does not prove compatibility or the overall performance goal.

## NEXT

Integration3 must compile and execute the requested combined regressions, report
actual failures and pin the combined tree/artifacts. Fix only demonstrated lane
failures in a new coordinator-routed round. Then obtain controlled native
reference/runtime receipts where allowed before upgrading any parity label.
Do not block or rewrite the independently ready fade/Mapping/perf batch for this
unvalidated candidate. No self-loop, schedule, install, publication, host or PR.
