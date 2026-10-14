# Opt-in UVI script-owner attribution

`KONTRA_UVI_OWNER_AUDIT=1` enables fixed numeric owner-thread clocks in a `scan`
build. Absent or `0` leaves them disabled; other values are rejected. Ordinary
builds contain neither this environment option nor these clocks. The disabled
scan path takes no phase-clock reads and performs no extra binding-graph scan.

`UiBridge::owner_audit()` reads aggregate atomics directly. It never queues a
request behind a busy Lua owner or acquires the interface mutex. It returns
`None` when disabled. No script bodies, arguments, file names, task paths or UI
strings enter this report. Storage is fixed at 24 stages and 13 counters; report
construction allocates only on the caller outside audio/owner work.

## Timing boundaries

The report names nine disjoint owner-turn groups: events, UI requests, advance,
UI publication, finding publication, scan snapshot, command collection, command
push and park. Loading and the initial UI snapshot are separate startup groups.

Nested groups must not be added to those outer totals:

| Nested group | Included work |
| --- | --- |
| Scheduled advance | Due coroutine lookup, resume and its deferred callback cycles |
| Data/sample advance polls | Each existing resource service, including owner bookkeeping |
| Deferred callback cycle | Each existing batch of spawned/completion callbacks; may nest in an event, UI request or advance |
| Interface export | Construction of the complete authored `Interface`, including hidden widgets |
| Control mirror | Control extraction and atomic value publication |
| Face mutex/equality/replacement | Lock wait, full-interface equality, replacement and retirement measured separately |
| Command backpressure | Existing full-queue retry, UI-request handling and 1 ms sleep; nested in command push |
| Backpressure UI requests | Request handling inside that retry; nested again |
| Seeded barrier events/advance | Barrier work nested in UI requests, separate from ordinary owner-turn stages |

Each completed interval records calls, total/max monotonic wall nanoseconds, a
16-bin wall histogram and caller-thread CPU nanoseconds/samples. Histogram upper
bounds in microseconds are `4^i`, `i=0..14`; bin 15 overflows. Linux uses the safe
thread CPU clock through `rustix`. Unsupported CPU measurements remain `None`;
wall time is never substituted. CPU excludes decode workers and other threads.
Both clocks include their instrumentation overhead, and wall includes scheduling
delay. An interval still in flight is named but excluded from completed totals.

Snapshots read independent atomics concurrently; they are observations, not an
atomic transaction or a completion barrier. A before/after subtraction gives
cumulative sums, calls and histogram deltas; lifetime maxima cannot be
subtracted into interval maxima. Park duration is waiting, not compute. The
existing owner progress marker now identifies ordinary advance and park phases
correctly; it remains a phase marker rather than a cost measurement.

Counters retain owner turns, popped events/requests, scheduled/deferred/all
coroutine resumes, exported widget/binding totals, changed faces, commands
collected/pushed, queue-full retries and backlog high-water. They establish work
frequency, not cost per widget or proof that a callback completed successfully.

## Validation and use

On 2026-10-15 three focused fixtures passed: optional off/on observation preserves
the complete final authored interface, commands and fault state; a delayed
callback is observed; a 1,100-command burst exercises the real 1,024-entry queue
and direct snapshots while the owner retries; nested clocks and bounded numeric
histograms remain separate. Ordinary all-target compilation and scan compilation
passed. These fixtures do not establish real-library timing or native pixels.
The existing seeded-audit integration suite passed 7/7 and UI integration passed
17/17 on the same source cohort.

The dynamic-source witness can capture `owner_audit_before` after service binding
and `owner_audit_after` after task settlement, before independent PCM checking.
Its worker delivery measurement separately brackets worker completion to owner
receipt. Submission-to-owner-ack includes core application, transport and owner
scheduling; it is not pure core commit time.

For a useful 1,746-task run, retain before/after snapshots, task stage histograms,
full interface/geometry hashes, asset outcomes, PCM verification and external
workload observations. Compare export/publication frequency, command retry time,
sample-poll cadence and CPU versus wall before proposing batching or caching.
No scheduler, queue, publication policy or live UI-value cache is changed here.

Existing seeded `ScriptThread` audits use a synchronous barrier clock. That is
different from the genuine real-clock production owner. An unseeded real-bank
run can attribute its observed work, but cannot provide a deterministic full-UI
off/on oracle when authored initialization is random. Keep deterministic authored
off/on checks and seeded single-thread inspector oracles separate until an
explicit diagnostic real-clock/seed contract is introduced. Do not report an
overhead percentage or speed improvement from unmatched scheduling or UI seeds.
