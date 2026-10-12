# Wait-capable load callbacks

Investigation started 2026-10-09 11:00:47 UTC with a 90-minute timebox.
Base: `0ad6b3f9`, including W0's regular-wait age fix `60c33d9c` and the
admitted MIDI-wait age fix. No budget or watchdog threshold change is proposed.

## Attribution limits

The NOIRE receipt is
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-noire-prepare-344/SOURCE-ATTRIBUTION.json`.
Its own status is SOURCE_ONLY, cause not confirmed. Four prepares take
5001.5–5131.5 ms and report the same generic script Warning at slot 2, line
2293. The warning message, callback owner, budget category and prepare
substage times were not retained. NOIRE is absent locally.

Glaze and Session Horns Pro are also absent locally. Their recorded
`KspFault/FuelExhausted` runtime snapshots are **not** static-evaluator budget
reports: `RuntimeLog::fault` creates that diagnostic from live core outcomes.
They separately have generic prepare script Warnings at lines 15 and 1659;
those warnings' reasons are unknown. Their prepares are 466.7 and 2571.8 ms.
No authored loop or common-library cause is inferred from these numbers.

## Source-proven mechanism

The static evaluator restores persistent values and evaluates
`persistence_changed` using the init fuel remaining from its 200M-step budget.
`wait`/`wait_ticks` there warn without suspending; `$ENGINE_UPTIME` and
`$KSP_TIMER` remain zero. A finite clock-polling callback therefore burns the
budget and returns partially evaluated state plus a generic Warning.

Frozen v1 `0cb7a8a0:src/ksp/runtime.rs:1273-1284` restores persistence and
spawns the persistence callback through the VM callback scheduler, then
settles runnable work. Suspended callbacks retain their continuation.

## Adaptation to v2

Detect reachable suspension before evaluating a load callback, including
calls through functions and nested branches. Wait builtins return no value;
the semantic checker rejects their use inside expressions. Schedule the whole
wait-capable persistence callback once through existing plan activation;
never evaluate its prefix statically and replay it. Non-suspending callbacks
retain the existing static model preparation used by compile-only UI clients.
The model/scan observation distinguishes **scheduled** from static completion.

Apply init engine writes before activation callbacks read or change live
engine parameters. UI effects and continuation completion use the existing
runtime/host paths.

For an init MIDI completion whose callback can wait, complete the MIDI
operation off audio as before. Retain its completed status in the bounded
initial-job queue and send a notification through existing host completion
ingress. Notification kind 5 requires no file IO or MIDI operation replay;
the completion uses its retained status, original async ID and source stage.
Queue capacity, admission and retry rules remain in the existing service.
Normal non-waiting init completion evaluation remains unchanged. Init
`wait_async` completes MIDI work synchronously; it alone does not defer an
init completion callback, preserving the existing recursion limit.

## Witnesses and validation

Synthetic fixtures use a **64-step test environment**, while the production
200M-step init budget remains unchanged. This gives a deterministic red
without deliberately burning five seconds of CPU. Cases cover clock polling,
function-call waits, tick waits, nested conditional/select waits, one activation/no prefix
replay, init engine order, and UI effects before/after the wait. Tick-wait
approximation diagnostics are retained.

The async case checks that MIDI reset is already visible when init continues,
that the operation is not performed twice, and that the scheduled callback
retains its ID/status and completes after a real wait. Existing persistence
restore, invalid/disabled-wait runaway guards and MIDI lifecycle checks are
retained.

Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w5-persistence-scheduler/`.
Builds are submitted through `kontakto-heavy`; the baseline fixture waits
outside the heavy queue while W9's timed quiet request exists. No library load
or CPU timing is part of this change.

The pre-restart red receipt has five fixtures: one passes and four fail with
static wait/budget warnings. The first candidate run exposed two fixture
setup errors (a void wait used as a value, and insertion without resizing a
reset MIDI buffer); both are corrected. The compatibility run caught
over-deferral of synchronous init `wait_async`; that recursion guard is now
preserved.

`targeted-green.log` passes 48 tests: `audit_followup` 6, `callback_fuel` 2,
`midi_object` 15, `params` 13, `persistence_waits` 7 (including scan), and
`script_state` 5. The seven scheduler cases finish in 0.00 seconds of reported
test runtime. These are functional witnesses, not quiet-machine performance
measurements. Core/KSP/host no-run compilation passes in `no-run.log`:
`cargo test --profile ci -p sampler-core -p sampler-ksp -p kontakto
--features shots,sampler-ksp/scan --no-run`, through `kontakto-heavy` (exit 0).
No real-library parity or census delta is claimed.

The first host no-run found a base compile error: coverage's `chain_with`
caller omitted the `live_eq` argument added by `8be442c7`. The one-line
`&[]` fix is copied from current integration `0d3aa593`; it is already in
0.3.355. `no-run-base-error.log` retains that diagnostic. No DSP behavior
change is introduced by this coverage-probe compatibility backport.

## Integration scope

The base `0ad6b3f9` includes `60c33d9c` (positive-wait preemption age),
`0ad6b3f9` (admitted MIDI-wait age), and `6cd368a8` (unpublished MIDI ingress).
Keep those prerequisites; the production budget/watchdog remains unchanged.

W15 `c49a329e` changes generated-note routing in `behavior.rs` and
`prepare/selection.rs`; this change neither edits those paths nor depends on
that commit. W15's NOIRE/Una attack, release and Cutoff work stays separate.
W9's complete persistence stack has a semantic integration boundary:
activation runs the scheduled initial persistence callback once, while an
explicit state restore invokes it once for that restore. Retain the
`script_state` callback-count and outcome tests when batching both stacks.

## Parked real-library attribution

Symptom: NOIRE prepare repeatedly takes about five seconds with a generic Warning.
Best supported hypothesis: a wait-capable persistence loop reaches the static budget; unproved.
Next evidence: actual callback/warning category or numeric prepare/evaluation profile from NOIRE.
