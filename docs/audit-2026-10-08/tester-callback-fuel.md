# Tester callback fuel investigation

Started 2026-10-09 09:49:40 UTC, within the requested 90-minute timebox.
Support build: 0.3.344, source `7cd326ee`. Investigation baseline:
`ce714f75`, which contains that shipped ancestry.

## What the support bundle establishes

`tester-logs-NwxC/support-1791538387751/journal.jsonl` contains two
`FuelExhausted` runtime snapshots:

| Library | Row | Program | Callback index | Session monotonic ms |
|---|---:|---:|---:|---:|
| Glaze | 45799 | 0 | 31 | 181462 |
| Session Horns Pro - Performance | 46047 | 0 | 48 | 526279 |

Neither snapshot records callback kind, initiating gesture, first-preemption
sample, PC/call path, or whether the callback waited. These are not proof that
the failing callback is init, a generated timer, or an infinite authored loop.

Both library names have zero matches in the 1494-row corpus inventory. The
local `/mnt/MAIN_STORAGE/Libraries/Kontakt` directory inventory and recursive
name search also contain neither library. Their authored scripts are therefore
unavailable to read here. No authored source was exported or substituted with
a claim about these libraries' implementation.

## Root cause that can be proved

The shipped/current public baseline does not contain W11's `235ac3eb` fix.
`wait_behavior` schedules a validated positive wait but retains `yielded_at`.
The next fuel preemption measures from before the wait, including intentional
sleep. A finite work/wait/work callback consequently faults after its wait.

This proves a shared runtime instance of category **(b), not resetting across
waits**. It does **not** establish this as the cause of either tester callback.
New callbacks already initialize their age to `None`; resumed callbacks receive
fresh instruction fuel, and aggregate block fuel is refreshed for rendering.

The existing W11 fix clears age only after a positive wait passes clock and
queue validation. Zero/disabled waits and the continuous-preemption runaway
guard stay intact. W0 already has equivalent commits `b8ddace3`/`60c33d9c` in
its private integration; the regular-wait fix is reused without changing its
threshold or duplicating it.

The same bug remains in `Op::WaitMidi` even after that regular-wait fix: an
admitted `wait_async` suspends on a pending MIDI job without clearing age.
A second failing-first witness proves this independent boundary. The new
product change clears age only inside the existing admitted/enabled-wait
branch. Missing jobs and disabled waits retain age. Job admission, completion,
capacity checks and scheduling are unchanged.

Category **(a)** remains unproven: the scheduler charges one unit per lowered
instruction, including instructions executed by the straight-line fast path.
The host caps aggregate work at `max(block_frames * 128, 8192)`; the fault is
based on one second of rendered sample time since first preemption, rather
than measured CPU duration. Without the failing scripts/PCs, instruction
expansion or legitimate work exceeding that guard cannot be attributed.

Category **(c)** also remains unproven. The receipt includes unsupported
commands (including Glaze `wait_ticks` and Session Horns `get_menu_item_str`),
but contains no link from those commands to the failing loop. No builtin value
was guessed and no silent no-op or budget waiver was introduced.

## Failing-first witnesses

Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w5-tester-callback-fuel/`.

1. Existing W11 finite callback test, copied without its product fix onto
   `ce714f75`: **RED**, expected Finished, received FuelExhausted after a
   two-second wait. With the existing fix: core behavior **26/26 PASS**, including
   infinite and zero-wait loop guards; generated listener **1/1 PASS**.
2. New synthetic KSP fixture uses two finite 20,000-iteration loops separated
   by `wait(2000000)`. It uses default callback fuel and the actual host block
   budget, tests 44.1/48 kHz at 32/64/128 frames, asserts 40,000 final increments,
   exactly one completed callback, preemption, elapsed wait and zero faults.
   Trigger/render/observation are checked for heap allocation. Baseline **RED**
   is specifically `Some(FuelExhausted)`, not a compile or unrelated builtin
   failure. This fixture is a runtime mechanism witness, not reconstructed
   Glaze/Session Horns source.
   **GREEN: all six rate/block cells PASS**, with 31 or 63 fuel preemptions per
   cell, exactly one completed callback and zero faults.
3. The async fixture performs 20,000 finite increments, submits `mf_reset`,
   waits while the host delays completion for 102,400 samples, then performs
   another 20,000 increments. **RED on `60c33d9c`**: FuelExhausted instead of
   Finished after completion. **GREEN with the async reset**: Finished and
   40,000 increments. Companion infinite loops using an invalid job and a
   valid job with waits disabled both still end with FuelExhausted. All three
   paths check trigger/render/completion for heap allocation.

The added commit is based on W0's existing `60c33d9c` fix. Product scope is
`sampler-core/src/ops.rs`'s admitted MIDI async-wait branch; tests live in
`sampler-ksp/tests/callback_fuel.rs`. No fuel accounting, builtin return value,
runaway threshold, or product API is changed.

Final targeted checks: core behavior **26/26**, KSP callback-fuel **2/2**
(six host-budget cells plus three async modes), MIDI-object **15/15**, and
generated listener **1/1 PASS**. Core and KSP `cargo test --profile ci --no-run`
also PASS. All cargo calls used `kontakto-heavy`. No full gate, CPU timing, or
actual Glaze/Session Horns playback claim is made.

## Parked tester-specific attribution

Symptom: Glaze callback 31 and Session Horns callback 48 fail in shipped 344.
Best supported hypothesis: shipped wait-aging bug; actual callback cause UNKNOWN.
Next evidence: retest after W0's existing fix; if still failing, collect owner/PC,
first-preemption sample and wait trace from those actual callbacks in RAM.
