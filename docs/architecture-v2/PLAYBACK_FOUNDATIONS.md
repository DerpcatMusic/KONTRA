# Playback foundations

The shared engine owns musical time. File readers configure it; host wrappers
deliver timestamped events. A normal event must finish its runnable script stages
and attack selection before rendering advances past that event. An intentional
script wait is a scheduled event, not a scheduler preemption.

## Ownership and execution

| Entry point | Thread / owner | Bounds and failure | Required invariant |
| --- | --- | --- | --- |
| Instrument preparation | Loader owns the candidate until publication; rejected candidates drop on the loader | Cancelable, finite script preparation allowance; physical head budget | Runnable startup work and normal attack heads are prepared before publication |
| Host events, control callbacks, script stages | One runtime owner on the audio thread, including CLAP/VST3/standalone adapters | Reserved callback arena, bounded explicit dispatch stack, shared host allowance | Nested continuations stay in the current dispatch transaction; later independent events preserve order |
| Callback overload | Runtime owner | Caller chooses explicit deferral or terminal failure; no allocation or waiting on another thread | Ordinary player overload is reported instead of replaying an attack at a later block |
| Sample/voice admission | Runtime owns handles; loader/stream workers own immutable PCM payloads | Existing voice and page admission limits; no source I/O in the callback | Normal startup uses resident attack windows; cold recovery remains a separately counted exception |
| Rendering | Audio owner and prepared voice workers | Preallocated output and voice state; exact sample boundaries | Rendering advances the clock; event dispatch does not |
| Health publication | Audio copies scalar counters to atomics; UI reads them | Fixed storage, saturating cumulative counters | Budget failures and timing delays survive the host bridge without formatting on audio |
| Reset / retirement | Runtime clears bounded active state; control owners retire heavy payloads | Existing generation/retirement queues | No stale callback can start a retired note or generation |

## Dispatch contract

The ready stack contains synchronous work within the current event. The deferred
FIFO contains independent work held behind a genuinely preempted callback.
Forwarding a stage while dispatch is active pushes onto the ready stack even when
the FIFO contains later events. A queue pass may block behind a callback that
actually preempts again, never behind a newly created runnable stage.

Deferral remains an explicit engine capability for offline/control clients.
Normal playback uses a deterministic instruction allowance per part and block;
wall-clock scheduling noise and idle loaded parts do not reduce it. The player
selects terminal overload: bounded work that cannot complete is a
visible failure, not an implicit musical delay. Intentional waits and external
asynchronous operations retain their authored timing and lifetime rules.

## Preparation and source readiness

Preparation runs runnable initialization at sample zero on the loader, with a
separate finite allowance and cancellation checks. Future waits and listeners do
not have to finish. Audio-block limits are installed after preparation.

Automatic streaming reserves a bounded portion of available memory for complete
attack heads. It does not silently shorten those heads to make a load succeed.
Failure to fit the normal contract is a load error. Explicit purge/sample-change
operations can still use existing cold recovery and must remain observable.

## Evidence

The 2026-10-24 audit reproduced chained-stage strumming with resident PCM and one
preemption, then unlimited fuel. Una Corda reproduced startup staggering with no
cold starts. Areia required 637 MB / 1,067 MB of head payload against the former
256 MiB automatic ceiling. These measurements justify separating event execution,
preparation and normal streaming readiness. No DSP arithmetic change is required.

Implementation checks are small and follow implementation. Existing core and
adapter checks, allocation instrumentation and the archived real-library workload
remain the qualification sources; a source build is not a DAW certification.
